//! Running a GCode file, or a job generated from the design, on the
//! connected machine.
//!
//! `machine_run_file` asks for a file and streams it;
//! `machine_run_document` generates GCode from the design in memory and
//! streams that the same way; `machine_frame` streams the job's frame.
//! Pause, resume and stop act on the running job, and `machine_override`
//! moves the feed and laser power overrides live. Progress arrives as
//! `machine:job` events carrying a [`JobProgress`]; the last one for a job
//! has a done state.
//!
//! Before a design runs, `machine_preflight` returns the pre-run checklist
//! ([`Preflight`]); `machine_run_document` runs the same checks again and
//! refuses unless every warning still raised was acknowledged.
//! `machine_job_stop_wait` stops a job and waits until the machine is idle
//! with the laser off, for closing the app mid-job.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bf_core::gcode::{DesignRun, FrameShape, JobPlacement, JobSummary, RunPlan, frame, run};
use bf_core::machine::{
    ConnectionHandle, Job, JobProgress, MachineProfile, OverrideKind, Preflight, PreflightCheckId,
    SendError, preflight,
};
use bf_core::project::DesignDocument;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;

use super::Machine;

pub const JOB_EVENT: &str = "machine:job";

/// The name a frame runs under.
pub const FRAME_JOB: &str = "Frame";

/// How long a stop may take: the feed hold, the reset and the idle report
/// each have their own timeout in `bf_core`, which add up to 18 s.
pub const STOP_WAIT: Duration = Duration::from_secs(25);

/// File extensions offered in the file dialog.
const GCODE_EXTENSIONS: &[&str] = &["gcode", "nc", "gc", "ngc", "tap"];

/// The file the next `machine_run_file` streams instead of asking, for tests
/// (the native dialog can't run under the mock runtime).
#[derive(Default)]
pub struct JobPicker(Mutex<Option<PathBuf>>);

impl JobPicker {
    #[cfg(test)]
    pub fn pick(&self, path: PathBuf) {
        *self.0.lock().unwrap() = Some(path);
    }
}

/// Asks for a GCode file and streams it to the machine. Returns the job's
/// name, or `None` if the user cancelled the dialog.
#[tauri::command]
pub async fn machine_run_file<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    // Check before asking, so the user doesn't pick a file for nothing.
    ready(&handle(&app)?)?;
    let picked = app.state::<JobPicker>().0.lock().unwrap().take();
    let dialog = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = match picked {
            Some(path) => path,
            None => {
                let picked = dialog
                    .dialog()
                    .file()
                    .set_title("Run GCode file")
                    .add_filter("GCode", GCODE_EXTENSIONS)
                    .blocking_pick_file();
                let Some(file) = picked else { return Ok(None) };
                file.into_path().map_err(|error| error.to_string())?
            }
        };
        let job = Job::from_file(&path)
            .map_err(|error| format!("Couldn't read {}: {error}", path.display()))?;
        let name = job.name.clone();
        // The connection may have changed while the dialog was open.
        handle(&dialog)?
            .start_job(job)
            .map_err(|error| error.to_string())?;
        Ok(Some(name))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The pre-run checklist for the design, placed as `placement` says: the
/// machine's state, its settings (from `profile`, the active machine
/// profile) and whether the job stays on the bed.
#[tauri::command]
pub async fn machine_preflight<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
    name: String,
) -> Result<Preflight, String> {
    let handle = handle(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let design = generate(&app, &document, profile.as_ref(), placement, name)?;
        Ok(checks(&app, &handle, profile.as_ref(), &design.job.summary))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Generates GCode from the design, placed as `placement` says, and streams
/// it to the machine as the job `name`, straight from memory. Returns the
/// plan the UI follows the job's progress on the canvas with.
///
/// The run path is: refuse if not ready, generate, run the pre-run checks
/// (refusing on any that blocks, or warns and isn't in `acknowledged`),
/// then start streaming. Starting resets the overrides to 100%.
#[tauri::command]
pub async fn machine_run_document<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
    name: String,
    acknowledged: Option<Vec<PreflightCheckId>>,
) -> Result<RunPlan, String> {
    let handle = handle(&app)?;
    ready(&handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        let design = generate(&app, &document, profile.as_ref(), placement, name)?;
        let checks = checks(&app, &handle, profile.as_ref(), &design.job.summary);
        if let Some(refusal) = checks.refusal(&acknowledged.unwrap_or_default()) {
            return Err(refusal);
        }
        start_design(&handle, &design.job.gcode, &design.plan.name)?;
        Ok(design.plan)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn generate<R: Runtime>(
    app: &AppHandle<R>,
    document: &DesignDocument,
    profile: Option<&MachineProfile>,
    placement: JobPlacement,
    name: String,
) -> Result<DesignRun, String> {
    let profile = profile.cloned().unwrap_or_default();
    run(
        document,
        &profile,
        placement,
        Machine::work_position(app),
        name,
    )
    .map_err(|error| error.to_string())
}

/// The pre-run checks against the machine as last reported.
fn checks<R: Runtime>(
    app: &AppHandle<R>,
    handle: &ConnectionHandle,
    profile: Option<&MachineProfile>,
    summary: &JobSummary,
) -> Preflight {
    let status = Machine::of(app).lock().status.clone();
    preflight(profile, status.as_ref(), handle.homed(), summary)
}

/// Streams a generated design's GCode as the job `name`.
fn start_design(handle: &ConnectionHandle, gcode: &str, name: &str) -> Result<(), String> {
    let job = Job::from_lines(name, gcode.lines());
    handle.start_job(job).map_err(|error| error.to_string())
}

/// Traces the outline of the job the design would be, placed as `placement`
/// says, with the laser off (or at the profile's low frame power). Runs as a
/// job named "Frame", so it shows in the Job panel and can be stopped.
#[tauri::command]
pub async fn machine_frame<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
    shape: FrameShape,
) -> Result<String, String> {
    let handle = handle(&app)?;
    ready(&handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profile.unwrap_or_default();
        let framed = frame(
            &document,
            &profile,
            placement,
            Machine::work_position(&app),
            shape,
        )
        .map_err(|error| error.to_string())?;
        let job = Job::from_lines(FRAME_JOB.to_string(), framed.lines());
        handle.start_job(job).map_err(|error| error.to_string())?;
        Ok(FRAME_JOB.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The running job's latest progress, or the last job's final progress.
#[tauri::command]
pub fn machine_job<R: Runtime>(app: AppHandle<R>) -> Option<JobProgress> {
    Machine::of(&app).lock().job.clone()
}

/// Feed hold. Resume continues from where the head stopped.
#[tauri::command]
pub fn machine_job_pause<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    send(&app, ConnectionHandle::pause_job)
}

/// Resumes a paused job, or continues past a line the machine rejected.
#[tauri::command]
pub fn machine_job_resume<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    send(&app, ConnectionHandle::resume_job)
}

/// Stops the job: feed hold, soft reset, then `M5`, leaving the laser off
/// and the machine idle.
#[tauri::command]
pub fn machine_job_stop<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    send(&app, ConnectionHandle::stop_job)
}

/// Stops the running job, if any, and waits until the machine has turned
/// the laser off (and air assist, if the job had it on) and is idle. For
/// closing the app mid-job: the window closes only after this returns.
#[tauri::command]
pub async fn machine_job_stop_wait<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let Ok(handle) = handle(&app) else {
        return Ok(());
    };
    tauri::async_runtime::spawn_blocking(move || stop_and_wait(&handle, STOP_WAIT))
        .await
        .map_err(|error| error.to_string())?
}

/// Stops the job on `handle`, if one runs, and waits up to `timeout` for it
/// to end: by then the owner thread has sent `M5` (and `M9`).
pub fn stop_and_wait(handle: &ConnectionHandle, timeout: Duration) -> Result<(), String> {
    if !handle.job_active() {
        return Ok(());
    }
    // A closed connection has no job left to stop.
    if handle.stop_job().is_err() {
        return Ok(());
    }
    let deadline = Instant::now() + timeout;
    while handle.job_active() {
        if Instant::now() >= deadline {
            return Err(
                "The machine didn't confirm the stop. Check that the laser is off.".to_string(),
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// Moves the feed or laser power override towards `percent` (10-200), live.
/// The status reports' `overrides` show where it got to.
#[tauri::command]
pub fn machine_override<R: Runtime>(
    app: AppHandle<R>,
    kind: OverrideKind,
    percent: u16,
) -> Result<(), String> {
    send(&app, |handle| handle.set_override(kind, percent))
}

fn handle<R: Runtime>(app: &AppHandle<R>) -> Result<ConnectionHandle, String> {
    let inner = Machine::of(app).lock();
    let connection = inner.connection.as_ref();
    connection
        .map(|connection| connection.handle())
        .ok_or_else(|| "Connect to the machine first.".to_string())
}

fn ready(handle: &ConnectionHandle) -> Result<(), String> {
    if handle.job_active() {
        return Err("A job is already running.".to_string());
    }
    Ok(())
}

fn send<R: Runtime>(
    app: &AppHandle<R>,
    command: impl FnOnce(&ConnectionHandle) -> Result<(), SendError>,
) -> Result<(), String> {
    let handle = handle(app)?;
    command(&handle).map_err(|error| error.to_string())
}
