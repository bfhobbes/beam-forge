//! The material library: presets of layer settings for a material and an
//! operation, kept per machine profile as `presets/<profile id>.json` in the
//! app data directory, next to `machines/`. A library exports to, and
//! imports from, a JSON file of the same shape.
//!
//! A machine with no library file yet gets the [starter presets](starters):
//! starting points for 20 W diode lasers, to check with a test cut.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::machine::valid_id;

/// The library format this build writes.
pub const LIBRARY_FORMAT: u32 = 1;

/// Marks a file as a Beam Forge material library, so importing some other
/// JSON file fails clearly.
pub const LIBRARY_KIND: &str = "beam-forge-material-library";

/// The library file a machine without a profile (none connected yet) uses.
pub const NO_PROFILE: &str = "default";

/// What a preset does to the material. Cut and score follow outlines (a
/// line layer); engrave-fill scans across closed shapes (a fill layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "presets/")]
pub enum Operation {
    Cut,
    Score,
    EngraveFill,
}

/// The layer settings a preset holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "presets/")]
pub struct PresetSettings {
    pub operation: Operation,
    /// Feed rate while lasering, mm/min.
    pub speed: f64,
    /// Laser power, percent of the machine's maximum.
    pub max_power: f64,
    /// Power floor at corners and slowdowns, percent; at most `max_power`.
    pub min_power: f64,
    pub passes: u32,
    /// Engrave-fill only: distance between scan lines, mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub line_interval: Option<f64>,
    pub air_assist: bool,
}

impl PresetSettings {
    /// Why the settings can't be used, or `None`.
    pub fn problem(&self) -> Option<&'static str> {
        if !(self.speed.is_finite() && self.speed > 0.0) {
            return Some("a speed that isn't positive");
        }
        let percent = |value: f64| (0.0..=100.0).contains(&value);
        if !percent(self.max_power) || !percent(self.min_power) {
            return Some("a power outside 0–100%");
        }
        if self.min_power > self.max_power {
            return Some("a minimum power above its maximum");
        }
        if self.passes == 0 {
            return Some("no passes");
        }
        match (self.operation, self.line_interval) {
            (Operation::EngraveFill, None) => Some("no line interval"),
            (_, Some(interval)) if !(interval.is_finite() && interval > 0.0) => {
                Some("a line interval that isn't positive")
            }
            _ => None,
        }
    }
}

/// One entry in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "presets/")]
pub struct MaterialPreset {
    /// Unique within the library. Saving a preset with an empty id gives it one.
    pub id: String,
    pub material: String,
    /// Material thickness, mm; null where it doesn't matter (engraving).
    pub thickness: Option<f64>,
    pub settings: PresetSettings,
    /// Shipped with Beam Forge as a starting point to verify, not a tested value.
    #[serde(default)]
    pub starter: bool,
    #[serde(default)]
    pub notes: String,
}

impl MaterialPreset {
    fn validate(&self) -> Result<(), PresetError> {
        let fail = |what: &str| {
            Err(PresetError(format!(
                "The preset for {} has {what}.",
                if self.material.trim().is_empty() {
                    "a material"
                } else {
                    self.material.trim()
                }
            )))
        };
        if self.material.trim().is_empty() {
            return fail("no material name");
        }
        if let Some(thickness) = self.thickness
            && !(thickness.is_finite() && thickness > 0.0)
        {
            return fail("a thickness that isn't positive");
        }
        match self.settings.problem() {
            Some(problem) => fail(problem),
            None => Ok(()),
        }
    }
}

/// Why the library couldn't be read, changed or written, worded for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetError(String);

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PresetError {}

/// A machine's presets, in the order the user sees them. Also the export
/// file's contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "presets/")]
pub struct PresetLibrary {
    /// Always [`LIBRARY_KIND`].
    pub kind: String,
    pub format_version: u32,
    pub presets: Vec<MaterialPreset>,
}

/// What importing a library file did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "presets/")]
pub struct PresetImport {
    /// The library after the import.
    pub library: PresetLibrary,
    /// Presets the file added.
    pub added: u32,
    /// Presets the file replaced, matched by id.
    pub replaced: u32,
}

impl Default for PresetLibrary {
    fn default() -> Self {
        PresetLibrary::new(Vec::new())
    }
}

impl PresetLibrary {
    pub fn new(presets: Vec<MaterialPreset>) -> Self {
        PresetLibrary {
            kind: LIBRARY_KIND.to_string(),
            format_version: LIBRARY_FORMAT,
            presets,
        }
    }

    /// The starter presets: starting points for a 20 W diode laser. They
    /// are typical community values, not tested on this machine; the notes
    /// say to run a test cut first.
    pub fn starters() -> Self {
        let note = "Starting point for a 20 W diode laser, not a tested value: \
                    run a test cut on a scrap and adjust before trusting it.";
        let preset = |id: &str, material: &str, thickness, settings| MaterialPreset {
            id: id.to_string(),
            material: material.to_string(),
            thickness,
            settings,
            starter: true,
            notes: note.to_string(),
        };
        let line = |operation, speed, max_power, passes| PresetSettings {
            operation,
            speed,
            max_power,
            min_power: 0.0,
            passes,
            line_interval: None,
            air_assist: true,
        };
        PresetLibrary::new(vec![
            preset(
                "starter-plywood-3mm-cut",
                "Plywood",
                Some(3.0),
                line(Operation::Cut, 300.0, 100.0, 1),
            ),
            preset(
                "starter-plywood-3mm-score",
                "Plywood",
                Some(3.0),
                line(Operation::Score, 3000.0, 20.0, 1),
            ),
            preset(
                "starter-basswood-engrave",
                "Basswood",
                None,
                PresetSettings {
                    operation: Operation::EngraveFill,
                    speed: 3000.0,
                    max_power: 30.0,
                    min_power: 0.0,
                    passes: 1,
                    line_interval: Some(0.1),
                    air_assist: true,
                },
            ),
            preset(
                "starter-cardboard-3mm-cut",
                "Cardboard",
                Some(3.0),
                line(Operation::Cut, 1200.0, 60.0, 1),
            ),
        ])
    }

    /// Reads a library file's text and checks it.
    pub fn from_json(json: &str) -> Result<Self, PresetError> {
        let value: Value = serde_json::from_str(json).map_err(|error| {
            PresetError(format!(
                "This isn't a Beam Forge material library: the JSON is malformed ({error})."
            ))
        })?;
        if value.get("kind").and_then(Value::as_str) != Some(LIBRARY_KIND) {
            return Err(PresetError(
                "This isn't a Beam Forge material library.".to_string(),
            ));
        }
        if let Some(version) = value.get("formatVersion").and_then(Value::as_u64)
            && version > u64::from(LIBRARY_FORMAT)
        {
            return Err(PresetError(format!(
                "This material library was saved by a newer Beam Forge (format {version}). \
                 Update Beam Forge to open it."
            )));
        }
        let library: PresetLibrary = serde_json::from_value(value).map_err(|error| {
            PresetError(format!(
                "This isn't a valid Beam Forge material library: {error}."
            ))
        })?;
        library.validate()?;
        Ok(library)
    }

    /// The file's text: pretty JSON, so it can be read and shared.
    pub fn to_json(&self) -> Result<String, PresetError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|error| PresetError(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), PresetError> {
        if self.kind != LIBRARY_KIND || self.format_version != LIBRARY_FORMAT {
            return Err(PresetError(format!(
                "Expected a format {LIBRARY_FORMAT} material library."
            )));
        }
        let mut ids = std::collections::HashSet::new();
        for preset in &self.presets {
            if preset.id.is_empty() {
                return Err(PresetError("A preset has no id.".to_string()));
            }
            if !ids.insert(preset.id.as_str()) {
                return Err(PresetError(format!(
                    "Preset id {} is used twice.",
                    preset.id
                )));
            }
            preset.validate()?;
        }
        Ok(())
    }

    /// Adds `preset`, or replaces the one with its id. An empty id gets a
    /// new one. Returns the preset as stored.
    pub fn save(&mut self, mut preset: MaterialPreset) -> Result<MaterialPreset, PresetError> {
        preset.material = preset.material.trim().to_string();
        preset.validate()?;
        if preset.id.is_empty() {
            preset.id = self.new_id();
        }
        match self.presets.iter_mut().find(|other| other.id == preset.id) {
            Some(existing) => *existing = preset.clone(),
            None => self.presets.push(preset.clone()),
        }
        Ok(preset)
    }

    /// Removes the preset `id`. False if there was none.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.id != id);
        self.presets.len() != before
    }

    /// Adds `other`'s presets: one with an id already here replaces it in
    /// place, the rest are appended in order.
    pub fn merge(&mut self, other: PresetLibrary) -> PresetImport {
        let (mut added, mut replaced) = (0, 0);
        for preset in other.presets {
            match self.presets.iter_mut().find(|mine| mine.id == preset.id) {
                Some(mine) => {
                    *mine = preset;
                    replaced += 1;
                }
                None => {
                    self.presets.push(preset);
                    added += 1;
                }
            }
        }
        PresetImport {
            library: self.clone(),
            added,
            replaced,
        }
    }

    /// An id no preset here has, unlikely to clash with another library's
    /// so imports merge cleanly.
    fn new_id(&self) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default();
        let mut stamp = nanos;
        loop {
            let id = format!("preset-{stamp:x}");
            if !self.presets.iter().any(|preset| preset.id == id) {
                return id;
            }
            stamp += 1;
        }
    }
}

/// Libraries on disk, one JSON file per machine profile.
#[derive(Debug, Clone)]
pub struct PresetStore {
    dir: PathBuf,
}

impl PresetStore {
    /// Libraries in `dir`, usually `<app data>/presets`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        PresetStore { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file for profile `profile`, or for [`NO_PROFILE`].
    fn path(&self, profile: Option<&str>) -> Result<PathBuf, PresetError> {
        let id = profile.unwrap_or(NO_PROFILE);
        if !valid_id(id) {
            return Err(PresetError(format!("{id} isn't a machine profile id.")));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    /// The profile's library: the starters if it has none yet. A file that
    /// can't be read is an error rather than an empty library, so a later
    /// save can't overwrite the user's presets.
    pub fn load(&self, profile: Option<&str>) -> Result<PresetLibrary, PresetError> {
        let path = self.path(profile)?;
        match fs::read_to_string(&path) {
            Ok(json) => PresetLibrary::from_json(&json),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PresetLibrary::starters())
            }
            Err(error) => Err(PresetError(format!(
                "Couldn't read the material library: {error}"
            ))),
        }
    }

    /// Validates and writes the profile's library.
    pub fn store(&self, profile: Option<&str>, library: &PresetLibrary) -> Result<(), PresetError> {
        let path = self.path(profile)?;
        write_atomically(&path, &library.to_json()?)
            .map_err(|error| PresetError(format!("Couldn't save the material library: {error}")))
    }

    /// Adds or updates one preset in the profile's library and saves it.
    pub fn save(
        &self,
        profile: Option<&str>,
        preset: MaterialPreset,
    ) -> Result<(MaterialPreset, PresetLibrary), PresetError> {
        let mut library = self.load(profile)?;
        let saved = library.save(preset)?;
        self.store(profile, &library)?;
        Ok((saved, library))
    }

    /// Removes one preset from the profile's library and saves it.
    pub fn delete(&self, profile: Option<&str>, id: &str) -> Result<PresetLibrary, PresetError> {
        let mut library = self.load(profile)?;
        if library.remove(id) {
            self.store(profile, &library)?;
        }
        Ok(library)
    }

    /// Writes the profile's library to `path`.
    pub fn export(&self, profile: Option<&str>, path: &Path) -> Result<(), PresetError> {
        let library = self.load(profile)?;
        write_atomically(path, &library.to_json()?)
            .map_err(|error| PresetError(format!("Couldn't save {}: {error}", path.display())))
    }

    /// Merges the library file at `path` into the profile's and saves it.
    pub fn import(&self, profile: Option<&str>, path: &Path) -> Result<PresetImport, PresetError> {
        let json = fs::read_to_string(path)
            .map_err(|error| PresetError(format!("Couldn't read {}: {error}", path.display())))?;
        let incoming = PresetLibrary::from_json(&json)?;
        let mut library = self.load(profile)?;
        let import = library.merge(incoming);
        self.store(profile, &import.library)?;
        Ok(import)
    }
}

/// Writes through a temporary file and a rename, so a crash mid-write never
/// leaves half a library in place of a good one.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// An empty directory of its own under the system temp directory.
    fn temp_dir(name: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "bf-core-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn walnut() -> MaterialPreset {
        MaterialPreset {
            id: String::new(),
            material: " Walnut ".into(),
            thickness: Some(6.0),
            settings: PresetSettings {
                operation: Operation::Cut,
                speed: 150.0,
                max_power: 100.0,
                min_power: 10.0,
                passes: 3,
                line_interval: None,
                air_assist: true,
            },
            starter: false,
            notes: String::new(),
        }
    }

    #[test]
    fn starters_are_valid_and_marked_as_starting_points() {
        let starters = PresetLibrary::starters();
        starters.validate().unwrap();
        let materials: Vec<_> = starters
            .presets
            .iter()
            .map(|p| (p.material.as_str(), p.settings.operation))
            .collect();
        assert!(materials.contains(&("Plywood", Operation::Cut)));
        assert!(materials.contains(&("Basswood", Operation::EngraveFill)));
        assert!(materials.contains(&("Cardboard", Operation::Cut)));
        for preset in &starters.presets {
            assert!(preset.starter, "{}", preset.id);
            assert!(preset.notes.contains("not a tested value"), "{}", preset.id);
        }
    }

    #[test]
    fn a_new_machine_starts_with_the_starters() {
        let store = PresetStore::new(temp_dir("presets"));
        assert_eq!(
            store.load(Some("serial-com3")).unwrap(),
            PresetLibrary::starters()
        );
        assert_eq!(store.load(None).unwrap(), PresetLibrary::starters());
    }

    #[test]
    fn saving_and_updating_presets_persists_per_machine() {
        let store = PresetStore::new(temp_dir("presets"));
        let (saved, library) = store.save(Some("serial-com3"), walnut()).unwrap();
        assert!(saved.id.starts_with("preset-"), "{}", saved.id);
        assert_eq!(saved.material, "Walnut", "names are trimmed");
        assert_eq!(library.presets.last(), Some(&saved));
        assert_eq!(store.load(Some("serial-com3")).unwrap(), library);
        assert!(store.dir().join("serial-com3.json").exists());

        // Updating keeps the id and position.
        let mut updated = saved.clone();
        updated.settings.speed = 120.0;
        let (again, library) = store.save(Some("serial-com3"), updated.clone()).unwrap();
        assert_eq!(again, updated);
        let reloaded = store.load(Some("serial-com3")).unwrap();
        assert_eq!(reloaded, library);
        assert_eq!(
            reloaded.presets.len(),
            PresetLibrary::starters().presets.len() + 1
        );
        assert_eq!(reloaded.presets.last().unwrap().settings.speed, 120.0);

        // Another machine's library is separate.
        assert_eq!(
            store.load(Some("tcp-127-0-0-1-23232")).unwrap(),
            PresetLibrary::starters()
        );

        let library = store.delete(Some("serial-com3"), &saved.id).unwrap();
        assert_eq!(library, PresetLibrary::starters());
        assert_eq!(store.load(Some("serial-com3")).unwrap(), library);
    }

    #[test]
    fn invalid_presets_and_profiles_are_refused() {
        let store = PresetStore::new(temp_dir("presets"));
        type Break = fn(&mut MaterialPreset);
        let cases: [(Break, &str); 6] = [
            (|p| p.material = " ".into(), "no material name"),
            (|p| p.thickness = Some(0.0), "thickness"),
            (|p| p.settings.speed = 0.0, "speed"),
            (|p| p.settings.min_power = 101.0, "power outside"),
            (|p| p.settings.passes = 0, "no passes"),
            (
                |p| p.settings.operation = Operation::EngraveFill,
                "no line interval",
            ),
        ];
        for (break_it, expected) in cases {
            let mut preset = walnut();
            break_it(&mut preset);
            let error = store.save(None, preset).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
        let error = store.load(Some("../evil")).unwrap_err();
        assert!(error.to_string().contains("isn't a machine profile id"));
        assert!(!store.dir().join("default.json").exists());
    }

    #[test]
    fn a_corrupt_library_is_an_error_not_an_empty_library() {
        let store = PresetStore::new(temp_dir("presets"));
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.dir().join("serial-com3.json"), "{ nope").unwrap();
        let error = store.save(Some("serial-com3"), walnut()).unwrap_err();
        assert!(error.to_string().contains("malformed"), "{error}");
        let still = fs::read_to_string(store.dir().join("serial-com3.json")).unwrap();
        assert_eq!(still, "{ nope", "the file is left for the user to repair");
    }

    #[test]
    fn export_then_import_round_trips() {
        let dir = temp_dir("presets-export");
        let store = PresetStore::new(dir.join("presets"));
        let (saved, _) = store.save(Some("serial-com3"), walnut()).unwrap();
        let mut engrave = PresetLibrary::starters().presets[2].clone();
        engrave.id = String::new();
        engrave.material = "Cherry".into();
        engrave.notes = "Darker than basswood".into();
        let (_, library) = store.save(Some("serial-com3"), engrave).unwrap();
        assert!(library.presets.contains(&saved));

        let file = dir.join("export").join("library.json");
        store.export(Some("serial-com3"), &file).unwrap();

        // Into a fresh machine: the exported library exactly.
        let import = store.import(Some("tcp-127-0-0-1-23232"), &file).unwrap();
        assert_eq!(import.library, library);
        assert_eq!((import.added, import.replaced), (2, 4));
        assert_eq!(store.load(Some("tcp-127-0-0-1-23232")).unwrap(), library);

        // Into the machine it came from: nothing changes.
        let again = store.import(Some("serial-com3"), &file).unwrap();
        assert_eq!(again.library, library);
        assert_eq!((again.added, again.replaced), (0, 6));

        // Into a machine with its own presets: they are kept.
        let (mine, _) = store.save(None, walnut()).unwrap();
        let merged = store.import(None, &file).unwrap();
        assert!(merged.library.presets.contains(&mine));
        assert_eq!(merged.library.presets.len(), library.presets.len() + 1);
    }

    #[test]
    fn importing_something_else_fails_clearly() {
        let dir = temp_dir("presets-import");
        let store = PresetStore::new(dir.join("presets"));
        fs::create_dir_all(&dir).unwrap();
        let cases = [
            ("{ nope", "malformed"),
            (
                r#"{ "formatVersion": 2 }"#,
                "isn't a Beam Forge material library",
            ),
            (
                r#"{ "kind": "beam-forge-material-library", "formatVersion": 9, "presets": [] }"#,
                "newer Beam Forge",
            ),
            (
                r#"{ "kind": "beam-forge-material-library", "formatVersion": 1 }"#,
                "missing field `presets`",
            ),
        ];
        for (text, expected) in cases {
            let file = dir.join("bad.json");
            fs::write(&file, text).unwrap();
            let error = store.import(None, &file).unwrap_err().to_string();
            assert!(error.contains(expected), "{expected}: {error}");
        }
        let mut twice = PresetLibrary::starters();
        twice.presets.push(twice.presets[0].clone());
        let error = PresetLibrary::from_json(&serde_json::to_string(&twice).unwrap()).unwrap_err();
        assert!(error.to_string().contains("used twice"), "{error}");
        assert_eq!(store.load(None).unwrap(), PresetLibrary::starters());
    }

    #[test]
    fn line_presets_leave_out_the_interval() {
        let json = PresetLibrary::starters().to_json().unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert!(
            value["presets"][0]["settings"]
                .get("lineInterval")
                .is_none()
        );
        assert_eq!(value["presets"][2]["settings"]["lineInterval"], 0.1);
        assert_eq!(value["presets"][2]["settings"]["operation"], "engraveFill");
    }
}
