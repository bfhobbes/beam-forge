//! The real-hardware gate. Beam Forge only connects to a port that may be a
//! real laser (see [`PortAddress::may_be_hardware`]) once the user has
//! switched real hardware on, which needs the current safety notice
//! accepted. The acceptance is stored as `real-hardware.json` in the app
//! config directory; switching off forgets it, so switching on again shows
//! the notice again.

use std::path::PathBuf;
use std::sync::Mutex;

use bf_core::machine::{ConnectionState, PortAddress};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use ts_rs::TS;

use super::Machine;

/// The safety notice's version. Bump it whenever the notice's text changes
/// in a way users must read again (`src/lib/machine/SafetyNotice.svelte`
/// sends the version it showed), so earlier acceptances lapse.
pub const NOTICE_VERSION: u32 = 1;

const FILE_NAME: &str = "real-hardware.json";

/// Whether the app may drive a real laser, and which notice enables it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RealHardware {
    pub enabled: bool,
    /// The safety notice version that must be accepted to enable it.
    pub notice_version: u32,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    accepted_notice: Option<u32>,
}

/// Where the acceptance is stored: the app config directory unless set
/// otherwise.
#[derive(Default)]
pub struct RealHardwareGate(Mutex<Option<PathBuf>>);

impl RealHardwareGate {
    /// Stores the acceptance in `dir` instead, for tests.
    #[cfg(test)]
    pub fn use_dir(&self, dir: PathBuf) {
        *self.0.lock().unwrap() = Some(dir.join(FILE_NAME));
    }

    fn path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
        let mut path = app
            .state::<RealHardwareGate>()
            .inner()
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if path.is_none() {
            let dir = app
                .path()
                .app_config_dir()
                .map_err(|error| format!("Couldn't find the app config folder: {error}"))?;
            *path = Some(dir.join(FILE_NAME));
        }
        Ok(path.clone().unwrap_or_default())
    }

    /// Whether real hardware is on. A missing or unreadable file means off.
    pub fn enabled<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
        let path = Self::path(app)?;
        let stored: Stored = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Ok(stored.accepted_notice == Some(NOTICE_VERSION))
    }

    fn store<R: Runtime>(app: &AppHandle<R>, accepted_notice: Option<u32>) -> Result<(), String> {
        let path = Self::path(app)?;
        let save = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let json = serde_json::to_vec_pretty(&Stored { accepted_notice })?;
            std::fs::write(&path, json)
        };
        save().map_err(|error| format!("Couldn't save the real hardware setting: {error}"))
    }
}

/// Refuses to reach `address` while real hardware is off and it may be a
/// real laser.
pub fn check<R: Runtime>(app: &AppHandle<R>, address: &PortAddress) -> Result<(), String> {
    if address.may_be_hardware() && !RealHardwareGate::enabled(app)? {
        return Err(
            "Real laser control is off. Turn on \"Real laser\" and accept the \
             safety notice to connect to this port. The simulators work without it."
                .to_string(),
        );
    }
    Ok(())
}

fn current<R: Runtime>(app: &AppHandle<R>) -> Result<RealHardware, String> {
    Ok(RealHardware {
        enabled: RealHardwareGate::enabled(app)?,
        notice_version: NOTICE_VERSION,
    })
}

#[tauri::command]
pub fn machine_real_hardware<R: Runtime>(app: AppHandle<R>) -> Result<RealHardware, String> {
    current(&app)
}

/// Switches real hardware on, which needs `accepted_notice` to be the
/// current [`NOTICE_VERSION`], or off, which forgets the acceptance. Refused
/// while connected, so the switch can't change under a live machine.
#[tauri::command]
pub fn machine_set_real_hardware<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
    accepted_notice: Option<u32>,
) -> Result<RealHardware, String> {
    if !matches!(
        Machine::of(&app).lock().state,
        ConnectionState::Disconnected { .. }
    ) {
        return Err("Disconnect first.".to_string());
    }
    if enabled {
        if accepted_notice != Some(NOTICE_VERSION) {
            return Err("The safety notice has changed. Read and accept it again.".to_string());
        }
        RealHardwareGate::store(&app, Some(NOTICE_VERSION))?;
    } else {
        RealHardwareGate::store(&app, None)?;
    }
    current(&app)
}
