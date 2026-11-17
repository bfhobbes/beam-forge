//! Project files on disk, the recent-files list and the recovery copy.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::{Project, ProjectError};

/// How many recent projects are remembered.
const RECENT_LIMIT: usize = 10;

/// The name shown for a project: its file name without the extension.
pub fn project_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Writes through a temporary file and a rename, so a crash mid-write never
/// leaves a half-written file in place of a good one.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

impl Project {
    /// Reads, migrates and validates a `.bfp` file.
    pub fn read(path: &Path) -> Result<Project, ProjectError> {
        let text = fs::read_to_string(path)
            .map_err(|error| ProjectError(format!("Couldn't read {}: {error}", path.display())))?;
        Project::from_json(&text)
    }

    /// Validates the project and writes it to `path`.
    pub fn write(&self, path: &Path) -> Result<(), ProjectError> {
        let text = self.to_json()?;
        write_atomically(path, &text)
            .map_err(|error| ProjectError(format!("Couldn't save {}: {error}", path.display())))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "project/")]
pub struct RecentProject {
    pub path: String,
    pub name: String,
}

/// Recently opened or saved projects, most recent first, kept in `recent.json`.
pub struct RecentStore {
    file: PathBuf,
}

impl RecentStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        RecentStore {
            file: dir.as_ref().join("recent.json"),
        }
    }

    fn paths(&self) -> Vec<String> {
        fs::read_to_string(&self.file)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Projects that still exist, most recent first.
    pub fn list(&self) -> Vec<RecentProject> {
        self.paths()
            .into_iter()
            .filter(|path| Path::new(path).is_file())
            .map(|path| RecentProject {
                name: project_name(Path::new(&path)),
                path,
            })
            .collect()
    }

    /// Moves `path` to the top of the list.
    pub fn add(&self, path: &Path) -> Result<Vec<RecentProject>, ProjectError> {
        let path = path.display().to_string();
        let mut paths = self.paths();
        paths.retain(|other| *other != path);
        paths.insert(0, path);
        paths.truncate(RECENT_LIMIT);
        let json = serde_json::to_string_pretty(&paths).unwrap_or_default();
        write_atomically(&self.file, &json)?;
        Ok(self.list())
    }
}

/// An autosaved copy of unsaved work, offered back after a crash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub struct Recovery {
    /// The file the work belongs to, or null for a project never saved.
    pub path: Option<String>,
    pub name: String,
    /// When the copy was written, in milliseconds since the Unix epoch.
    pub saved_at: f64,
    pub project: Project,
}

/// The recovery copy, `recovery/recovery.json` in the app data directory.
/// It exists only while there is unsaved work, so finding one at launch means
/// the app didn't close cleanly.
pub struct RecoveryStore {
    file: PathBuf,
}

impl RecoveryStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        RecoveryStore {
            file: dir.as_ref().join("recovery").join("recovery.json"),
        }
    }

    pub fn save(&self, project: &Project, path: Option<&Path>) -> Result<(), ProjectError> {
        project.validate()?;
        let saved_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as f64)
            .unwrap_or_default();
        let recovery = Recovery {
            path: path.map(|path| path.display().to_string()),
            name: path.map_or_else(|| "Untitled".to_string(), project_name),
            saved_at,
            project: project.clone(),
        };
        let json = serde_json::to_string(&recovery).map_err(|e| ProjectError(e.to_string()))?;
        write_atomically(&self.file, &json)
            .map_err(|error| ProjectError(format!("Couldn't write the recovery copy: {error}")))
    }

    /// The recovery copy, if there is a readable one. An unreadable copy is
    /// dropped rather than offered.
    pub fn load(&self) -> Option<Recovery> {
        let json = fs::read_to_string(&self.file).ok()?;
        let parsed = serde_json::from_str::<Value>(&json)
            .ok()
            .and_then(|mut value| {
                let project = Project::from_value(value.get_mut("project")?.take()).ok()?;
                value["project"] = serde_json::to_value(project).ok()?;
                serde_json::from_value::<Recovery>(value).ok()
            });
        if parsed.is_none() {
            self.clear();
        }
        parsed
    }

    pub fn clear(&self) {
        let _ = fs::remove_file(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::DesignDocument;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bf-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn empty() -> Project {
        Project::new(
            DesignDocument {
                version: 1,
                units: "mm".to_string(),
                layers: vec![],
                objects: vec![],
            },
            None,
        )
    }

    #[test]
    fn writes_and_reads_a_project_file() {
        let dir = temp_dir("file");
        let path = dir.join("Coaster.bfp");
        empty().write(&path).unwrap();
        assert_eq!(Project::read(&path).unwrap(), empty());
        assert_eq!(project_name(&path), "Coaster");
        assert!(!dir.join("Coaster.bfp.tmp").exists());

        fs::write(&path, "not json").unwrap();
        assert!(Project::read(&path).is_err());
        let missing = Project::read(&dir.join("missing.bfp")).unwrap_err();
        assert!(
            missing.to_string().starts_with("Couldn't read"),
            "{missing}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_list_is_most_recent_first_without_duplicates_or_missing_files() {
        let dir = temp_dir("recent");
        let store = RecentStore::new(&dir);
        assert!(store.list().is_empty());
        let paths: Vec<_> = (0..12).map(|i| dir.join(format!("p{i}.bfp"))).collect();
        for path in &paths {
            empty().write(path).unwrap();
            store.add(path).unwrap();
        }
        store.add(&paths[5]).unwrap();
        let list = store.list();
        assert_eq!(list.len(), RECENT_LIMIT);
        assert_eq!(list[0].name, "p5");
        assert_eq!(list[1].name, "p11");
        assert_eq!(list.iter().filter(|p| p.name == "p5").count(), 1);

        fs::remove_file(&paths[11]).unwrap();
        assert_eq!(store.list()[1].name, "p10");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_copy_round_trips_and_clears() {
        let dir = temp_dir("recovery");
        let store = RecoveryStore::new(&dir);
        assert!(store.load().is_none());
        let path = dir.join("Sign.bfp");
        store.save(&empty(), Some(&path)).unwrap();
        let recovery = store.load().unwrap();
        assert_eq!(recovery.project, empty());
        assert_eq!(recovery.name, "Sign");
        assert_eq!(
            recovery.path.as_deref(),
            Some(path.display().to_string().as_str())
        );
        assert!(recovery.saved_at > 0.0);

        store.clear();
        assert!(store.load().is_none());

        fs::create_dir_all(dir.join("recovery")).unwrap();
        fs::write(dir.join("recovery").join("recovery.json"), "{ broken").unwrap();
        assert!(store.load().is_none());
        assert!(!dir.join("recovery").join("recovery.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
