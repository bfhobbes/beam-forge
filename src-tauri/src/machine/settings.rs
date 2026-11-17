//! `$` settings and machine profile commands over `bf_core::machine`.
//!
//! Reading the settings also creates or refreshes the connected machine's
//! profile, stored as `machines/<id>.json` in the app data directory.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use bf_core::machine::{
    ConnectionHandle, ConnectionState, MachineInfo, MachineProfile, MachineSetting, PortInfo,
    ProfileStore, Settings, validate_setting,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use ts_rs::TS;

use super::Machine;

/// How long to wait for `$$` or a setting write. Writes go to EEPROM, which
/// takes GRBL a few milliseconds per byte.
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// The settings table and the profile they refreshed.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MachineSettings {
    pub settings: Vec<MachineSetting>,
    pub profile: MachineProfile,
}

/// Where profiles are stored: `<app data>/machines` unless set otherwise.
#[derive(Default)]
pub struct Profiles(Mutex<Option<PathBuf>>);

impl Profiles {
    /// Stores profiles in `dir` instead, for tests.
    #[cfg(test)]
    pub fn use_dir(&self, dir: PathBuf) {
        *self.0.lock().unwrap() = Some(dir);
    }

    fn store<R: Runtime>(app: &AppHandle<R>) -> Result<ProfileStore, String> {
        let mut dir = app.state::<Profiles>().inner().0.lock().unwrap();
        if dir.is_none() {
            let data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("Couldn't find the app data folder: {error}"))?;
            *dir = Some(data.join("machines"));
        }
        Ok(ProfileStore::new(dir.clone().unwrap_or_default()))
    }
}

/// The connected machine, or why there isn't one.
fn connected<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(ConnectionHandle, PortInfo, MachineInfo), String> {
    let inner = Machine::of(app).lock();
    match (&inner.state, &inner.connection) {
        (ConnectionState::Connected { port, machine }, Some(connection)) => {
            Ok((connection.handle(), port.clone(), machine.clone()))
        }
        _ => Err("Connect to the machine first.".to_string()),
    }
}

/// Reads `$$` and syncs the profile. Blocks until the machine replies.
fn read_settings<R: Runtime>(app: &AppHandle<R>) -> Result<MachineSettings, String> {
    let (handle, port, machine) = connected(app)?;
    let lines = handle
        .query(handle.dialect().settings_query(), QUERY_TIMEOUT)
        .map_err(|error| format!("Couldn't read the settings: {error}"))?;
    let settings = Settings::parse(lines.iter().map(String::as_str));
    if settings.is_empty() {
        return Err("The machine reported no settings.".to_string());
    }
    let profile = Profiles::store(app)?
        .sync(&port, &machine, &settings)
        .map_err(|error| error.to_string())?;
    Ok(MachineSettings {
        settings: settings.describe(),
        profile,
    })
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// Reads the settings table and creates or refreshes the machine's profile.
#[tauri::command]
pub async fn machine_settings<R: Runtime>(app: AppHandle<R>) -> Result<MachineSettings, String> {
    blocking(move || read_settings(&app)).await
}

/// Writes one setting, then reads them all back so the table shows what the
/// controller actually stored.
#[tauri::command]
pub async fn machine_write_setting<R: Runtime>(
    app: AppHandle<R>,
    number: u16,
    value: String,
) -> Result<MachineSettings, String> {
    let value = validate_setting(number, &value).map_err(|error| error.to_string())?;
    blocking(move || {
        let (handle, ..) = connected(&app)?;
        handle
            .query(
                &handle.dialect().write_setting(number, &value),
                QUERY_TIMEOUT,
            )
            .map_err(|error| format!("Couldn't change ${number}: {error}"))?;
        read_settings(&app)
    })
    .await
}

#[tauri::command]
pub fn machine_profiles<R: Runtime>(app: AppHandle<R>) -> Result<Vec<MachineProfile>, String> {
    Ok(Profiles::store(&app)?.list())
}

/// Validates and saves a profile the user edited.
#[tauri::command]
pub fn machine_profile_save<R: Runtime>(
    app: AppHandle<R>,
    profile: MachineProfile,
) -> Result<MachineProfile, String> {
    Profiles::store(&app)?
        .save(&profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}
