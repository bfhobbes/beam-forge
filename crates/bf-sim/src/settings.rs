//! GRBL 1.1 `$` settings and machine profiles.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A GRBL 1.1 setting: its number, factory default, and decimals shown by `$$`.
/// Settings shown with no decimals are integers; GRBL truncates written values.
struct Definition {
    id: u16,
    default: f64,
    decimals: usize,
}

const fn def(id: u16, default: f64, decimals: usize) -> Definition {
    Definition {
        id,
        default,
        decimals,
    }
}

const DEFINITIONS: &[Definition] = &[
    def(0, 10.0, 0),    // step pulse, microseconds
    def(1, 25.0, 0),    // step idle delay, milliseconds
    def(2, 0.0, 0),     // step port invert mask
    def(3, 0.0, 0),     // direction port invert mask
    def(4, 0.0, 0),     // step enable invert
    def(5, 0.0, 0),     // limit pins invert
    def(6, 0.0, 0),     // probe pin invert
    def(10, 1.0, 0),    // status report mask
    def(11, 0.010, 3),  // junction deviation, mm
    def(12, 0.002, 3),  // arc tolerance, mm
    def(13, 0.0, 0),    // report inches
    def(20, 0.0, 0),    // soft limits
    def(21, 0.0, 0),    // hard limits
    def(22, 0.0, 0),    // homing cycle
    def(23, 0.0, 0),    // homing direction invert mask
    def(24, 25.0, 3),   // homing feed, mm/min
    def(25, 500.0, 3),  // homing seek, mm/min
    def(26, 250.0, 0),  // homing debounce, milliseconds
    def(27, 1.0, 3),    // homing pull-off, mm
    def(30, 1000.0, 0), // max spindle (laser S value)
    def(31, 0.0, 0),    // min spindle
    def(32, 0.0, 0),    // laser mode
    def(100, 250.0, 3), // X steps/mm
    def(101, 250.0, 3), // Y steps/mm
    def(102, 250.0, 3), // Z steps/mm
    def(110, 500.0, 3), // X max rate, mm/min
    def(111, 500.0, 3), // Y max rate, mm/min
    def(112, 500.0, 3), // Z max rate, mm/min
    def(120, 10.0, 3),  // X acceleration, mm/s^2
    def(121, 10.0, 3),  // Y acceleration, mm/s^2
    def(122, 10.0, 3),  // Z acceleration, mm/s^2
    def(130, 200.0, 3), // X max travel, mm
    def(131, 200.0, 3), // Y max travel, mm
    def(132, 200.0, 3), // Z max travel, mm
];

fn definition(id: u16) -> Option<&'static Definition> {
    DEFINITIONS.iter().find(|d| d.id == id)
}

/// Why a `$n=value` write was rejected, as a GRBL error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingError {
    /// `error:3`: no such setting.
    Unknown,
    /// `error:4`: negative values are not allowed.
    Negative,
}

impl SettingError {
    pub fn code(self) -> u8 {
        match self {
            SettingError::Unknown => 3,
            SettingError::Negative => 4,
        }
    }
}

/// The `$` settings of a GRBL controller, keyed by setting number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings(BTreeMap<u16, f64>);

impl Settings {
    /// GRBL 1.1 factory defaults.
    pub fn grbl_defaults() -> Self {
        Settings(DEFINITIONS.iter().map(|d| (d.id, d.default)).collect())
    }

    pub fn get(&self, id: u16) -> Option<f64> {
        self.0.get(&id).copied()
    }

    /// Writes a setting the way `$n=value` does.
    pub fn set(&mut self, id: u16, value: f64) -> Result<(), SettingError> {
        let definition = definition(id).ok_or(SettingError::Unknown)?;
        if value < 0.0 {
            return Err(SettingError::Negative);
        }
        let value = if definition.decimals == 0 {
            value.trunc()
        } else {
            value
        };
        self.0.insert(id, value);
        Ok(())
    }

    /// The lines `$$` prints, e.g. `$11=0.010`.
    pub fn report_lines(&self) -> Vec<String> {
        DEFINITIONS
            .iter()
            .map(|d| {
                let value = self.value(d.id);
                format!("${}={:.*}", d.id, d.decimals, value)
            })
            .collect()
    }

    fn value(&self, id: u16) -> f64 {
        self.get(id)
            .or_else(|| definition(id).map(|d| d.default))
            .unwrap_or(0.0)
    }

    fn flag(&self, id: u16) -> bool {
        self.value(id) != 0.0
    }

    pub(crate) fn status_mask(&self) -> u8 {
        self.value(10) as u8
    }

    pub(crate) fn soft_limits(&self) -> bool {
        self.flag(20)
    }

    pub(crate) fn homing_enabled(&self) -> bool {
        self.flag(22)
    }

    pub(crate) fn homing_dir_mask(&self) -> u8 {
        self.value(23) as u8
    }

    pub(crate) fn homing_pulloff(&self) -> f64 {
        self.value(27)
    }

    /// Max travel per axis ($130-$132), in mm.
    pub(crate) fn max_travel(&self, axis: usize) -> f64 {
        self.value(130 + axis as u16)
    }

    pub(crate) fn junction_deviation(&self) -> f64 {
        self.value(11)
    }

    pub(crate) fn arc_tolerance(&self) -> f64 {
        self.value(12)
    }

    /// `$30`: the S value for full laser power.
    pub(crate) fn rpm_max(&self) -> f64 {
        self.value(30)
    }

    /// `$31`: the S value for minimum laser power.
    pub(crate) fn rpm_min(&self) -> f64 {
        self.value(31)
    }

    pub(crate) fn laser_mode(&self) -> bool {
        self.flag(32)
    }

    /// Steps per mm ($100-$102).
    pub(crate) fn steps_per_mm(&self) -> [f64; 3] {
        std::array::from_fn(|axis| self.value(100 + axis as u16))
    }

    /// Max rate per axis ($110-$112), in mm/s.
    pub(crate) fn max_rate(&self) -> [f64; 3] {
        std::array::from_fn(|axis| self.value(110 + axis as u16) / 60.0)
    }

    /// Acceleration per axis ($120-$122), in mm/s^2.
    pub(crate) fn acceleration(&self) -> [f64; 3] {
        std::array::from_fn(|axis| self.value(120 + axis as u16))
    }
}

/// A simulated machine: its settings and controller buffer sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub settings: Settings,
    /// Serial receive buffer size in bytes. GRBL allocates one extra byte, so
    /// hosts may have this many unacknowledged bytes in flight.
    pub rx_buffer_size: usize,
    /// Motion planner buffer size in blocks.
    pub planner_blocks: usize,
}

impl Profile {
    /// Stock GRBL 1.1 on an Arduino Uno.
    pub fn grbl_defaults() -> Self {
        Profile {
            name: "GRBL 1.1 defaults".to_string(),
            settings: Settings::grbl_defaults(),
            rx_buffer_size: 128,
            planner_blocks: 15,
        }
    }

    /// A GRBL diode laser in laser mode. Bed size, rates and acceleration are
    /// placeholders until the Phecda's real `$$` output is captured.
    pub fn diode_laser() -> Self {
        let mut profile = Profile::grbl_defaults();
        profile.name = "Diode laser (placeholder values)".to_string();
        for (id, value) in [
            (32, 1.0),
            (30, 1000.0),
            (110, 6000.0),
            (111, 6000.0),
            (120, 500.0),
            (121, 500.0),
            (130, 400.0),
            (131, 400.0),
        ] {
            profile.settings.set(id, value).expect("known setting");
        }
        profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lines_use_grbl_formatting() {
        let lines = Settings::grbl_defaults().report_lines();
        assert_eq!(lines[0], "$0=10");
        assert!(lines.contains(&"$11=0.010".to_string()));
        assert!(lines.contains(&"$30=1000".to_string()));
        assert_eq!(lines.last().unwrap(), "$132=200.000");
    }

    #[test]
    fn integer_settings_truncate_and_validate() {
        let mut settings = Settings::grbl_defaults();
        settings.set(32, 1.7).unwrap();
        assert_eq!(settings.get(32), Some(1.0));
        assert_eq!(settings.set(7, 1.0), Err(SettingError::Unknown));
        assert_eq!(settings.set(110, -1.0), Err(SettingError::Negative));
    }

    #[test]
    fn profiles_round_trip_through_json() {
        let profile = Profile::diode_laser();
        let json = serde_json::to_string(&profile).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), profile);
    }
}
