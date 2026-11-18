//! The machine profile: what the editor (bed size), GCode generator and time
//! estimator know about a machine. Each machine has one, stored as
//! `machines/<id>.json` in the app data directory, and refreshed from `$$`
//! every time Beam Forge connects.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::settings::Settings;
use super::{MachineInfo, PortInfo};
use crate::Firmware;
use crate::gcode::MAX_FRAME_POWER;
use crate::project::Point;

/// Which corner of the bed is X0 Y0, seen from the front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OriginCorner {
    #[default]
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
}

/// A step of the "Set up this machine" checklist. The hands-on tests
/// (`JogTest`, `FrameTest`, `FireTest`) make up one checklist step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SetupStep {
    Connected,
    Bed,
    LaserMode,
    Homing,
    JogTest,
    FrameTest,
    FireTest,
    AirAssist,
}

/// How a setup step ended. A step without one is still to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StepOutcome {
    Done,
    Skipped,
}

/// Progress through the "Set up this machine" checklist, kept with the
/// profile so it resumes where it was left.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MachineSetup {
    /// The steps finished or skipped so far.
    pub steps: BTreeMap<SetupStep, StepOutcome>,
    /// Closed by the user: not offered again on connect.
    pub dismissed: bool,
}

/// Width (X) and depth (Y) of the work area, in mm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BedSize {
    pub width: f64,
    pub height: f64,
}

/// A value per axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AxisValues {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Everything Beam Forge knows about one machine. The fields marked `$n`
/// mirror that setting and are refreshed on every connect; the rest belong
/// to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MachineProfile {
    /// File name stem; letters, digits and `-` only.
    pub id: String,
    pub name: String,
    pub firmware: Firmware,
    /// The port this machine was last connected on.
    pub port_key: Option<String>,
    pub bed: BedSize,
    /// When true, `bed` follows max travel (`$130`, `$131`).
    pub bed_from_machine: bool,
    pub origin: OriginCorner,
    /// `$30`: the S value for full power.
    pub max_spindle: f64,
    /// `$31`: the S value for minimum power.
    pub min_spindle: f64,
    /// `$32`. With laser mode off GRBL stops at the end of every move.
    pub laser_mode: bool,
    /// `$110`-`$112`, in mm/min.
    pub max_rate: AxisValues,
    /// `$120`-`$122`, in mm/s².
    pub acceleration: AxisValues,
    /// `$11`, in mm: how far GRBL lets the head cut a corner, which sets how
    /// fast it may take one.
    pub junction_deviation: f64,
    /// `$22`.
    pub homing: bool,
    /// The command that turns air assist on, usually `M8` (flood) or `M7`.
    pub air_assist: String,
    /// Where "Start from: user origin" places jobs, in work coordinates. Set
    /// from the head's position while connected.
    pub user_origin: Option<Point>,
    /// Low-power frame: percent of full power to frame at (`M4`), up to
    /// [`MAX_FRAME_POWER`]. 0 frames with the laser off.
    pub frame_power: f64,
    /// The setup checklist's progress. `None` on profiles made before the
    /// checklist existed, which aren't offered it on connect.
    pub setup: Option<MachineSetup>,
}

impl Default for MachineProfile {
    /// A GRBL 1.1 machine with factory settings, until `$$` says otherwise.
    fn default() -> Self {
        MachineProfile {
            id: String::new(),
            name: String::new(),
            firmware: Firmware::Grbl,
            port_key: None,
            bed: BedSize {
                width: 200.0,
                height: 200.0,
            },
            bed_from_machine: true,
            origin: OriginCorner::FrontLeft,
            max_spindle: 1000.0,
            min_spindle: 0.0,
            laser_mode: false,
            max_rate: AxisValues {
                x: 500.0,
                y: 500.0,
                z: 500.0,
            },
            acceleration: AxisValues {
                x: 10.0,
                y: 10.0,
                z: 10.0,
            },
            junction_deviation: 0.010,
            homing: false,
            air_assist: "M8".to_string(),
            user_origin: None,
            frame_power: 0.0,
            setup: None,
        }
    }
}

/// Why a profile can't be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError(String);

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProfileError {}

impl From<io::Error> for ProfileError {
    fn from(error: io::Error) -> Self {
        ProfileError(format!("Couldn't save the machine profile: {error}"))
    }
}

/// The profile id for a machine on `port`: `serial-com3`, `tcp-127-0-0-1-23232`.
/// GRBL has no serial number, so the port is the best stable key there is.
pub fn profile_id(port: &PortInfo) -> String {
    let mut id = String::new();
    for c in port.key.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_matches('-').to_string()
}

/// Whether `id` can name a profile file: letters, digits and `-`, up to 64.
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl MachineProfile {
    /// A new profile for the machine that answered on `port`, with the
    /// setup checklist still to do, so the app offers it.
    pub fn new(port: &PortInfo, machine: &MachineInfo) -> Self {
        MachineProfile {
            id: profile_id(port),
            name: format!("{} on {}", machine.name, port.label),
            firmware: machine.firmware,
            port_key: Some(port.key.clone()),
            setup: Some(MachineSetup::default()),
            ..MachineProfile::default()
        }
    }

    /// Copies what `$$` reported into the mirrored fields. Settings the
    /// report left out keep their previous values.
    pub fn apply_settings(&mut self, settings: &Settings) {
        let number = |n: u16, field: &mut f64| {
            if let Some(value) = settings.number(n) {
                *field = value;
            }
        };
        let flag = |n: u16, field: &mut bool| {
            if let Some(value) = settings.flag(n) {
                *field = value;
            }
        };
        number(11, &mut self.junction_deviation);
        number(30, &mut self.max_spindle);
        number(31, &mut self.min_spindle);
        flag(32, &mut self.laser_mode);
        flag(22, &mut self.homing);
        number(110, &mut self.max_rate.x);
        number(111, &mut self.max_rate.y);
        number(112, &mut self.max_rate.z);
        number(120, &mut self.acceleration.x);
        number(121, &mut self.acceleration.y);
        number(122, &mut self.acceleration.z);
        if self.bed_from_machine {
            number(130, &mut self.bed.width);
            number(131, &mut self.bed.height);
        }
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        let fail = |why: &str| Err(ProfileError(why.to_string()));
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if !valid_id(&self.id) {
            return fail("A profile id may only contain letters, digits and dashes.");
        }
        if self.name.trim().is_empty() {
            return fail("The machine needs a name.");
        }
        if !positive(self.bed.width) || !positive(self.bed.height) {
            return fail("The bed width and depth must be more than 0 mm.");
        }
        if !self.min_spindle.is_finite() || self.min_spindle < 0.0 {
            return fail("The minimum power (S) can't be negative.");
        }
        if !positive(self.max_spindle) || self.max_spindle <= self.min_spindle {
            return fail("The maximum power (S) must be more than the minimum.");
        }
        let axes = |a: AxisValues| positive(a.x) && positive(a.y) && positive(a.z);
        if !axes(self.max_rate) || !axes(self.acceleration) {
            return fail("Max rates and accelerations must be more than 0.");
        }
        if !self.junction_deviation.is_finite() || self.junction_deviation < 0.0 {
            return fail("The junction deviation can't be negative.");
        }
        let air = self.air_assist.trim();
        if air.is_empty() || air.contains(['\r', '\n']) {
            return fail("The air-assist command must be one line of GCode, such as M8.");
        }
        if let Some(origin) = self.user_origin
            && !(origin.x.is_finite() && origin.y.is_finite())
        {
            return fail("The user origin must be a position.");
        }
        if !(0.0..=MAX_FRAME_POWER).contains(&self.frame_power) {
            return fail("The low-power frame must be between 0% (laser off) and 5%.");
        }
        Ok(())
    }
}

/// Profiles on disk, one JSON file each.
#[derive(Debug, Clone)]
pub struct ProfileStore {
    dir: PathBuf,
}

impl ProfileStore {
    /// Profiles in `dir`, usually `<app data>/machines`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ProfileStore { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Every readable, valid profile, by name. Files that aren't are skipped
    /// rather than hiding the rest.
    pub fn list(&self) -> Vec<MachineProfile> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut profiles: Vec<_> = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let id = path.file_stem()?.to_str()?;
                (path.extension()? == "json").then(|| self.load(id))?
            })
            .collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        profiles
    }

    /// The profile `id`, if it exists and is valid.
    pub fn load(&self, id: &str) -> Option<MachineProfile> {
        if !valid_id(id) {
            return None;
        }
        let json = fs::read_to_string(self.path(id)).ok()?;
        let profile: MachineProfile = serde_json::from_str(&json).ok()?;
        (profile.id == id && profile.validate().is_ok()).then_some(profile)
    }

    /// Validates and writes `profile`, replacing the file in one step so a
    /// crash can't leave half a profile behind.
    pub fn save(&self, profile: &MachineProfile) -> Result<(), ProfileError> {
        profile.validate()?;
        fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_string_pretty(profile)
            .map_err(|error| ProfileError(error.to_string()))?;
        let temp = self.dir.join(format!("{}.json.tmp", profile.id));
        fs::write(&temp, json)?;
        fs::rename(&temp, self.path(&profile.id))?;
        Ok(())
    }

    /// Creates or updates the profile for the machine on `port` from its
    /// `$$` report, and saves it.
    pub fn sync(
        &self,
        port: &PortInfo,
        machine: &MachineInfo,
        settings: &Settings,
    ) -> Result<MachineProfile, ProfileError> {
        let mut profile = self
            .load(&profile_id(port))
            .unwrap_or_else(|| MachineProfile::new(port, machine));
        profile.firmware = machine.firmware;
        profile.port_key = Some(port.key.clone());
        profile.apply_settings(settings);
        self.save(&profile)?;
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::machine::PortAddress;

    /// An empty directory of its own under the system temp directory.
    fn temp_store() -> ProfileStore {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "bf-core-profiles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        ProfileStore::new(dir)
    }

    fn com3() -> PortInfo {
        PortInfo {
            key: "serial:COM3".into(),
            address: PortAddress::Serial {
                path: "COM3".into(),
            },
            label: "COM3".into(),
            description: None,
            vid: None,
            pid: None,
        }
    }

    fn grbl() -> MachineInfo {
        MachineInfo {
            firmware: Firmware::Grbl,
            name: "Grbl".into(),
            version: "1.1h".into(),
            banner: "Grbl 1.1h ['$' for help]".into(),
        }
    }

    fn report(lines: &[&str]) -> Settings {
        Settings::parse(lines.iter().copied())
    }

    #[test]
    fn ids_come_from_the_port_key() {
        assert_eq!(profile_id(&com3()), "serial-com3");
        assert_eq!(profile_id(&PortInfo::simulator()), "tcp-127-0-0-1-23232");
        let mut usb = com3();
        usb.key = "serial:/dev/ttyUSB0".into();
        assert_eq!(profile_id(&usb), "serial-dev-ttyusb0");
    }

    #[test]
    fn connecting_creates_a_profile_from_the_settings_report() {
        let store = temp_store();
        let settings = report(&[
            "$11=0.020",
            "$22=1",
            "$30=1000",
            "$31=0",
            "$32=1",
            "$110=6000.000",
            "$111=5000.000",
            "$120=500.000",
            "$121=400.000",
            "$130=400.000",
            "$131=300.000",
        ]);

        let profile = store.sync(&com3(), &grbl(), &settings).unwrap();

        assert_eq!(profile.id, "serial-com3");
        assert_eq!(profile.name, "Grbl on COM3");
        assert_eq!(profile.origin, OriginCorner::FrontLeft);
        assert_eq!(profile.air_assist, "M8");
        assert_eq!(
            profile.bed,
            BedSize {
                width: 400.0,
                height: 300.0
            }
        );
        assert!(profile.laser_mode && profile.homing);
        assert_eq!(profile.max_rate.y, 5000.0);
        assert_eq!(profile.acceleration.x, 500.0);
        assert_eq!(profile.junction_deviation, 0.02);
        assert_eq!(store.load("serial-com3"), Some(profile));
    }

    #[test]
    fn reconnecting_keeps_the_users_choices() {
        let store = temp_store();
        let mut profile = store.sync(&com3(), &grbl(), &report(&["$32=0"])).unwrap();
        profile.name = "Phecda".into();
        profile.origin = OriginCorner::RearLeft;
        profile.air_assist = "M7".into();
        profile.bed_from_machine = false;
        profile.bed = BedSize {
            width: 380.0,
            height: 380.0,
        };
        store.save(&profile).unwrap();

        let updated = store
            .sync(&com3(), &grbl(), &report(&["$32=1", "$130=400.000"]))
            .unwrap();

        assert_eq!(updated.name, "Phecda");
        assert_eq!(updated.origin, OriginCorner::RearLeft);
        assert_eq!(updated.air_assist, "M7");
        assert_eq!(updated.bed.width, 380.0, "a custom bed stays");
        assert!(updated.laser_mode, "machine settings refresh");
    }

    #[test]
    fn refuses_invalid_profiles() {
        let store = temp_store();
        let valid = MachineProfile::new(&com3(), &grbl());
        for (broken, why) in [
            (
                MachineProfile {
                    id: "../evil".into(),
                    ..valid.clone()
                },
                "id",
            ),
            (
                MachineProfile {
                    name: " ".into(),
                    ..valid.clone()
                },
                "name",
            ),
            (
                MachineProfile {
                    bed: BedSize {
                        width: 0.0,
                        height: 1.0,
                    },
                    ..valid.clone()
                },
                "bed",
            ),
            (
                MachineProfile {
                    min_spindle: 1000.0,
                    ..valid.clone()
                },
                "maximum power",
            ),
            (
                MachineProfile {
                    air_assist: "M8\nM3".into(),
                    ..valid.clone()
                },
                "air-assist",
            ),
        ] {
            let error = store.save(&broken).unwrap_err().to_string();
            assert!(error.contains(why), "{why}: {error}");
        }
        assert!(store.list().is_empty());
    }

    #[test]
    fn lists_valid_profiles_by_name_and_skips_broken_files() {
        let store = temp_store();
        let mut b = MachineProfile::new(&com3(), &grbl());
        b.name = "B".into();
        store.save(&b).unwrap();
        let mut a = MachineProfile::new(&PortInfo::simulator(), &grbl());
        a.name = "A".into();
        store.save(&a).unwrap();
        fs::write(store.dir().join("broken.json"), "{ not json").unwrap();

        let names: Vec<_> = store.list().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["A", "B"]);
    }

    #[test]
    fn older_files_load_with_defaults_for_missing_fields() {
        let store = temp_store();
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(
            store.dir().join("old.json"),
            r#"{ "id": "old", "name": "Old", "laserMode": true }"#,
        )
        .unwrap();

        let profile = store.load("old").unwrap();
        assert!(profile.laser_mode);
        assert_eq!(profile.setup, None, "made before the setup checklist");
        assert_eq!(profile.air_assist, "M8");
        assert_eq!(profile.junction_deviation, 0.01, "GRBL's default $11");
    }

    #[test]
    fn a_new_machine_gets_the_setup_checklist_and_keeps_its_progress() {
        let store = temp_store();
        let mut profile = store.sync(&com3(), &grbl(), &report(&["$32=0"])).unwrap();
        assert_eq!(profile.setup, Some(MachineSetup::default()), "offered");

        let mut setup = MachineSetup::default();
        setup.steps.insert(SetupStep::Connected, StepOutcome::Done);
        setup.steps.insert(SetupStep::Homing, StepOutcome::Skipped);
        profile.setup = Some(setup.clone());
        store.save(&profile).unwrap();
        let json = fs::read_to_string(store.dir().join("serial-com3.json")).unwrap();
        assert!(
            json.contains(r#""steps": {"#) && json.contains(r#""homing": "skipped""#),
            "{json}"
        );

        let reconnected = store.sync(&com3(), &grbl(), &report(&["$32=1"])).unwrap();
        assert_eq!(reconnected.setup, Some(setup), "progress resumes");
    }

    #[test]
    fn profiles_from_before_the_checklist_are_not_offered_it() {
        let store = temp_store();
        let mut old = MachineProfile::new(&com3(), &grbl());
        old.setup = None;
        store.save(&old).unwrap();

        let profile = store.sync(&com3(), &grbl(), &report(&["$32=1"])).unwrap();
        assert_eq!(profile.setup, None);
    }
}
