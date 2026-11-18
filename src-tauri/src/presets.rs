//! Material library commands over `bf_core::presets`. Each machine profile
//! has its own library, `presets/<profile id>.json` in the app data
//! directory next to `machines/`; `profile` is null before any machine has a
//! profile.

use std::path::PathBuf;
use std::sync::Mutex;

use bf_core::presets::{MaterialPreset, PresetImport, PresetLibrary, PresetStore};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use ts_rs::TS;

use crate::files;

const LIBRARY_FILES: &[files::Filter] = &[("Beam Forge material library", &["json"])];

/// A saved preset (with the id it was given) and the library it is now in.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "presets/")]
pub struct SavedPreset {
    pub preset: MaterialPreset,
    pub library: PresetLibrary,
}

/// Where libraries are stored (`<app data>/presets` unless a test says
/// otherwise), and the file the next export or import uses instead of
/// asking, for tests (the native dialog can't run under the mock runtime).
#[derive(Default)]
pub struct Presets {
    dir: Mutex<Option<PathBuf>>,
    picked: Mutex<Option<PathBuf>>,
}

impl Presets {
    #[cfg(test)]
    pub fn use_dir(&self, dir: PathBuf) {
        *self.dir.lock().unwrap() = Some(dir);
    }

    #[cfg(test)]
    pub fn pick(&self, path: PathBuf) {
        *self.picked.lock().unwrap() = Some(path);
    }

    fn store<R: Runtime>(app: &AppHandle<R>) -> Result<PresetStore, String> {
        let mut dir = app.state::<Presets>().inner().dir.lock().unwrap();
        if dir.is_none() {
            let data = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("Couldn't find the app data folder: {error}"))?;
            *dir = Some(data.join("presets"));
        }
        Ok(PresetStore::new(dir.clone().unwrap_or_default()))
    }

    fn picked<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
        app.state::<Presets>().inner().picked.lock().unwrap().take()
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// The profile's library; the starter presets if it has none yet.
#[tauri::command]
pub fn presets_library<R: Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
) -> Result<PresetLibrary, String> {
    Presets::store(&app)?
        .load(profile.as_deref())
        .map_err(|error| error.to_string())
}

/// Adds a preset (an empty id gets a new one) or updates the one with its id.
#[tauri::command]
pub fn presets_save<R: Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
    preset: MaterialPreset,
) -> Result<SavedPreset, String> {
    let (preset, library) = Presets::store(&app)?
        .save(profile.as_deref(), preset)
        .map_err(|error| error.to_string())?;
    Ok(SavedPreset { preset, library })
}

#[tauri::command]
pub fn presets_delete<R: Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
    id: String,
) -> Result<PresetLibrary, String> {
    Presets::store(&app)?
        .delete(profile.as_deref(), &id)
        .map_err(|error| error.to_string())
}

/// Asks where to save the library (suggesting `<name>.json`) and writes it.
/// The saved path, or null if the user cancelled.
#[tauri::command]
pub async fn presets_export<R: Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
    name: String,
) -> Result<Option<String>, String> {
    blocking(move || {
        let store = Presets::store(&app)?;
        // Read first, so a library that can't be exported fails before the dialog.
        store
            .load(profile.as_deref())
            .map_err(|error| error.to_string())?;
        let path = match Presets::picked(&app) {
            Some(path) => path,
            None => {
                let suggested = PathBuf::from(format!("{name}.json"));
                match files::pick_save_file(
                    &app,
                    "Export material library",
                    LIBRARY_FILES,
                    &suggested,
                ) {
                    Some(path) => path,
                    None => return Ok(None),
                }
            }
        };
        store
            .export(profile.as_deref(), &path)
            .map_err(|error| error.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

/// Asks for a library file and merges it into the profile's library:
/// presets with an id already there replace it. Null if the user cancelled.
#[tauri::command]
pub async fn presets_import<R: Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
) -> Result<Option<PresetImport>, String> {
    blocking(move || {
        let path = match Presets::picked(&app) {
            Some(path) => path,
            None => match files::pick_file(&app, "Import material library", LIBRARY_FILES) {
                Some(path) => path,
                None => return Ok(None),
            },
        };
        Presets::store(&app)?
            .import(profile.as_deref(), &path)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}
