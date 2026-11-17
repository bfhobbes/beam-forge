//! Manual control from the Machine panel's jog pad: jogging, homing, the
//! work origin, air assist and the laser test fire.
//!
//! Everything timed runs on the connection's owner thread in `bf_core`: a
//! held jog cancels itself unless `machine_jog_hold` keeps renewing it, and
//! the test fire's off command goes out after its duration even if the
//! window freezes or closes.

use bf_core::machine::{ConnectionHandle, JogDirection, MachineProfile, SendError, TestFire};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use ts_rs::TS;

use super::Machine;

/// A one-shot machine command from the jog pad.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "command", rename_all = "camelCase")]
#[ts(export)]
pub enum MachineCommand {
    /// The homing cycle (`$H`); the UI offers it only when `$22=1`.
    Home,
    /// Clear an alarm lock without homing (`$X`).
    Unlock,
    /// Jog to work X0 Y0 at `feed` mm/min.
    GoToOrigin { feed: f64 },
    /// Make the head's position work X0 Y0.
    SetOrigin,
    /// Put work X0 Y0 back on machine X0 Y0.
    ClearOrigin,
    /// Air assist on (the profile's command, usually `M8`) or off (`M9`).
    AirAssist { on: bool },
}

fn handle<R: Runtime>(app: &AppHandle<R>) -> Result<ConnectionHandle, String> {
    let inner = Machine::of(app).lock();
    inner
        .connection
        .as_ref()
        .map(|connection| connection.handle())
        .ok_or_else(|| SendError::Closed.to_string())
}

fn send<R: Runtime, T>(
    app: &AppHandle<R>,
    command: impl FnOnce(&ConnectionHandle) -> Result<T, SendError>,
) -> Result<T, String> {
    command(&handle(app)?).map_err(|error| error.to_string())
}

/// Jogs `distance` mm in `direction` at `feed` mm/min.
#[tauri::command]
pub fn machine_jog<R: Runtime>(
    app: AppHandle<R>,
    direction: JogDirection,
    distance: f64,
    feed: f64,
) -> Result<(), String> {
    send(&app, |handle| handle.jog(direction, distance, feed))
}

/// Starts or renews a held jog. Call about every 100 ms while held; the
/// jog stops by itself about 400 ms after the last call.
#[tauri::command]
pub fn machine_jog_hold<R: Runtime>(
    app: AppHandle<R>,
    direction: JogDirection,
    feed: f64,
) -> Result<(), String> {
    send(&app, |handle| handle.jog_hold(direction, feed))
}

/// Cancels any jog at once (GRBL's jog cancel, `0x85`).
#[tauri::command]
pub fn machine_jog_stop<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    send(&app, ConnectionHandle::jog_stop)
}

#[tauri::command]
pub fn machine_command<R: Runtime>(
    app: AppHandle<R>,
    command: MachineCommand,
    profile: Option<MachineProfile>,
) -> Result<(), String> {
    send(&app, |handle| {
        let dialect = handle.dialect();
        match command {
            MachineCommand::Home => handle.send_line(dialect.home()),
            MachineCommand::Unlock => handle.send_line(dialect.unlock()),
            MachineCommand::GoToOrigin { feed } => handle.go_to_origin(feed),
            MachineCommand::SetOrigin => handle.send_line(dialect.set_work_origin()),
            MachineCommand::ClearOrigin => handle.send_line(dialect.clear_work_origin()),
            MachineCommand::AirAssist { on: true } => {
                let profile = profile.unwrap_or_default();
                handle.send_line(profile.air_assist.trim())
            }
            MachineCommand::AirAssist { on: false } => handle.send_line(dialect.air_assist_off()),
        }
    })
}

/// Fires the laser with the head still: at most 10% of the profile's
/// maximum S for at most a second, whatever is asked. Returns what fired.
#[tauri::command]
pub fn machine_test_fire<R: Runtime>(
    app: AppHandle<R>,
    fire: TestFire,
    profile: Option<MachineProfile>,
) -> Result<TestFire, String> {
    let max_spindle = profile.unwrap_or_default().max_spindle;
    send(&app, |handle| handle.test_fire(fire, max_spindle))
}
