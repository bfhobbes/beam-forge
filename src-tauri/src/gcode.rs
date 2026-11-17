//! GCode generation commands over `bf_core::gcode`: a summary for the job
//! dialog, the job's time estimate, the toolpath preview's moves, and export
//! to a file. Running a generated job on the machine is
//! `machine::job::machine_run_document`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bf_core::estimate::JobEstimate;
use bf_core::gcode::{
    GeneratedJob, JobPlacement, JobSummary, Toolpath, estimate, generate, toolpath,
};
use bf_core::machine::MachineProfile;
use bf_core::project::DesignDocument;
use tauri::{AppHandle, Manager, Runtime};

use crate::files;
use crate::machine::Machine;

const GCODE_FILES: &[files::Filter] = &[("GCode", &["gcode", "nc", "gc"])];

/// Generates `document` for `profile` (GRBL defaults when there is none
/// yet), placed as `placement` says. "Current position" is the connected
/// machine's latest work position.
pub fn generate_job<R: Runtime>(
    app: &AppHandle<R>,
    document: &DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
) -> Result<GeneratedJob, String> {
    let profile = profile.unwrap_or_default();
    generate(document, &profile, placement, Machine::work_position(app))
        .map_err(|error| error.to_string())
}

/// Where the next `gcode_export` writes instead of asking, for tests (the
/// native dialog can't run under the mock runtime).
#[derive(Default)]
pub struct ExportPicker(Mutex<Option<PathBuf>>);

impl ExportPicker {
    #[cfg(test)]
    pub fn pick(&self, path: PathBuf) {
        *self.0.lock().unwrap() = Some(path);
    }
}

/// What the job would do: its bounds, whether it fits the bed, its size.
#[tauri::command]
pub fn gcode_summary<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
) -> Result<JobSummary, String> {
    Ok(generate_job(&app, &document, profile, placement)?.summary)
}

/// The moves the job's GCode is written from, in document coordinates, for
/// the toolpath preview. Placed as `gcode_summary` places the job.
#[tauri::command]
pub fn gcode_toolpath<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
) -> Result<Toolpath, String> {
    let profile = profile.unwrap_or_default();
    toolpath(&document, &profile, placement, Machine::work_position(&app))
        .map_err(|error| error.to_string())
}

/// How long the job would take on the profile's machine, by layer, for the
/// top bar. Placed as `gcode_summary` places the job.
#[tauri::command]
pub fn gcode_estimate<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
) -> Result<JobEstimate, String> {
    let profile = profile.unwrap_or_default();
    estimate(&document, &profile, placement, Machine::work_position(&app))
        .map_err(|error| error.to_string())
}

/// Generates the job, asks where to save it (suggesting `<name>.gcode`) and
/// writes it. The saved path, or `None` if the user cancelled.
#[tauri::command]
pub async fn gcode_export<R: Runtime>(
    app: AppHandle<R>,
    document: DesignDocument,
    profile: Option<MachineProfile>,
    placement: JobPlacement,
    name: String,
) -> Result<Option<String>, String> {
    // Generate first, so a job that can't be made fails before the dialog.
    let job = generate_job(&app, &document, profile, placement)?;
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app.state::<ExportPicker>().0.lock().unwrap().take();
        let path = match picked {
            Some(path) => path,
            None => {
                let suggested = PathBuf::from(format!("{name}.gcode"));
                match files::pick_save_file(&app, "Export GCode", GCODE_FILES, &suggested) {
                    Some(path) => path,
                    None => return Ok(None),
                }
            }
        };
        write(&path, &job.gcode)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Writes `gcode` to `path` in one step, so a failed export can't leave half
/// a file where a good one was.
fn write(path: &Path, gcode: &str) -> Result<(), String> {
    let fail = |error: std::io::Error| format!("Couldn't save {}: {error}", path.display());
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, gcode).map_err(fail)?;
    std::fs::rename(&temp, path).map_err(fail)
}
