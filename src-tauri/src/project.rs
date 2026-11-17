//! Project file commands over `bf_core::project`: open, save, the recent
//! list and the crash-recovery copy. The frontend owns the document; these
//! commands validate it and move it to and from disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bf_core::project::{
    EXTENSION, Project, RecentProject, RecentStore, Recovery, RecoveryStore, project_name,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use ts_rs::TS;

use crate::files;

const PROJECT_FILES: &[files::Filter] = &[("Beam Forge project", &[EXTENSION])];

/// A project read from disk, with the recent list it was added to.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub struct OpenedProject {
    pub path: String,
    pub name: String,
    pub project: Project,
    pub recent: Vec<RecentProject>,
}

/// Where a project was saved, with the updated recent list.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub struct SavedProject {
    pub path: String,
    pub name: String,
    pub recent: Vec<RecentProject>,
}

/// Where the recent list and recovery copy live (the app data directory
/// unless a test says otherwise), and whether the launch file was handed out.
#[derive(Default)]
pub struct ProjectFiles {
    dir: Mutex<Option<PathBuf>>,
    launch_taken: AtomicBool,
}

impl ProjectFiles {
    #[cfg(test)]
    pub fn use_dir(&self, dir: PathBuf) {
        *self.dir.lock().unwrap() = Some(dir);
    }

    fn dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
        let mut dir = app.state::<ProjectFiles>().inner().dir.lock().unwrap();
        if dir.is_none() {
            *dir = Some(
                app.path()
                    .app_data_dir()
                    .map_err(|error| format!("Couldn't find the app data folder: {error}"))?,
            );
        }
        Ok(dir.clone().unwrap_or_default())
    }

    fn recent<R: Runtime>(app: &AppHandle<R>) -> Result<RecentStore, String> {
        Ok(RecentStore::new(Self::dir(app)?))
    }

    pub fn recovery<R: Runtime>(app: &AppHandle<R>) -> Result<RecoveryStore, String> {
        Ok(RecoveryStore::new(Self::dir(app)?))
    }
}

/// Only `.bfp` paths are accepted, so these commands can't be pointed at
/// arbitrary files.
fn project_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if files::has_extension(&path, PROJECT_FILES) {
        Ok(path)
    } else {
        Err(format!(
            "{} isn't a Beam Forge project (.{EXTENSION}) file.",
            path.display()
        ))
    }
}

fn open<R: Runtime>(app: &AppHandle<R>, path: &Path) -> Result<OpenedProject, String> {
    let project = Project::read(path).map_err(|error| error.to_string())?;
    let recent = ProjectFiles::recent(app)?
        .add(path)
        .map_err(|error| error.to_string())?;
    Ok(OpenedProject {
        path: path.display().to_string(),
        name: project_name(path),
        project,
        recent,
    })
}

fn save<R: Runtime>(
    app: &AppHandle<R>,
    project: &Project,
    path: &Path,
) -> Result<SavedProject, String> {
    project.write(path).map_err(|error| error.to_string())?;
    let recent = ProjectFiles::recent(app)?
        .add(path)
        .map_err(|error| error.to_string())?;
    Ok(SavedProject {
        path: path.display().to_string(),
        name: project_name(path),
        recent,
    })
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// Opens a `.bfp` file, validating and migrating it.
#[tauri::command]
pub async fn project_open<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> Result<OpenedProject, String> {
    blocking(move || open(&app, &project_path(&path)?)).await
}

/// Asks which project to open, then opens it. None if the user cancelled.
#[tauri::command]
pub async fn project_open_dialog<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<OpenedProject>, String> {
    blocking(move || {
        files::pick_file(&app, "Open project", PROJECT_FILES)
            .map(|path| open(&app, &path))
            .transpose()
    })
    .await
}

/// Saves the project to `path`, which must be a `.bfp` file.
#[tauri::command]
pub async fn project_save<R: Runtime>(
    app: AppHandle<R>,
    project: Project,
    path: String,
) -> Result<SavedProject, String> {
    blocking(move || save(&app, &project, &project_path(&path)?)).await
}

/// Asks where to save, starting from `current` (the project's file, if it has
/// one), then saves there. None if the user cancelled.
#[tauri::command]
pub async fn project_save_as<R: Runtime>(
    app: AppHandle<R>,
    project: Project,
    current: Option<String>,
) -> Result<Option<SavedProject>, String> {
    blocking(move || {
        let suggested = current
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("Untitled.{EXTENSION}")));
        files::pick_save_file(&app, "Save project", PROJECT_FILES, &suggested)
            .map(|path| save(&app, &project, &path))
            .transpose()
    })
    .await
}

/// Recently opened or saved projects that still exist, most recent first.
#[tauri::command]
pub fn project_recent<R: Runtime>(app: AppHandle<R>) -> Result<Vec<RecentProject>, String> {
    Ok(ProjectFiles::recent(&app)?.list())
}

/// The recovery copy left by a session that didn't close cleanly, if any.
#[tauri::command]
pub fn project_recovery<R: Runtime>(app: AppHandle<R>) -> Result<Option<Recovery>, String> {
    Ok(ProjectFiles::recovery(&app)?.load())
}

/// Writes the recovery copy of unsaved work.
#[tauri::command]
pub fn project_recovery_save<R: Runtime>(
    app: AppHandle<R>,
    project: Project,
    path: Option<String>,
) -> Result<(), String> {
    ProjectFiles::recovery(&app)?
        .save(&project, path.as_deref().map(Path::new))
        .map_err(|error| error.to_string())
}

/// Deletes the recovery copy: the work was saved, discarded or restored.
#[tauri::command]
pub fn project_recovery_clear<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    ProjectFiles::recovery(&app)?.clear();
    Ok(())
}

/// The `.bfp` file the app was launched to open (by double-clicking it, via
/// the file association), handed out once.
#[tauri::command]
pub fn project_launch_file<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    let files = app.state::<ProjectFiles>();
    if files.launch_taken.swap(true, Ordering::SeqCst) {
        return None;
    }
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|path| files::has_extension(path, PROJECT_FILES))
        .map(|path| path.display().to_string())
}
