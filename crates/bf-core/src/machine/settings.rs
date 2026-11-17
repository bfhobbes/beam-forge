//! GRBL 1.1 `$` settings: parsing the `$$` report, what each setting means,
//! and checking a value before it is written with `$n=value`.
//!
//! Names, units and limits follow the GRBL 1.1 wiki and `settings.c` in
//! gnea/grbl. Settings this table doesn't know (grblHAL extensions) are kept
//! and shown without a description.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How a setting's value is read and checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum SettingKind {
    /// A whole number up to `max`. GRBL truncates fractions; Beam Forge
    /// refuses them instead.
    Integer { max: u32 },
    /// A decimal number, printed with three decimals.
    Decimal,
    /// `0` or `1`.
    Boolean,
    /// A bit mask, one bit per axis: X = 1, Y = 2, Z = 4.
    AxisMask,
    /// `$10`: 1 = report machine position, 2 = report buffer state.
    StatusMask,
}

/// What a GRBL 1.1 setting means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingDefinition {
    pub number: u16,
    pub name: &'static str,
    pub unit: Option<&'static str>,
    pub description: &'static str,
    pub kind: SettingKind,
}

const fn def(
    number: u16,
    name: &'static str,
    unit: Option<&'static str>,
    kind: SettingKind,
    description: &'static str,
) -> SettingDefinition {
    SettingDefinition {
        number,
        name,
        unit,
        description,
        kind,
    }
}

const BYTE: SettingKind = SettingKind::Integer { max: 255 };
const WORD: SettingKind = SettingKind::Integer { max: 65_535 };
const DECIMAL: SettingKind = SettingKind::Decimal;
const BOOL: SettingKind = SettingKind::Boolean;
const AXES: SettingKind = SettingKind::AxisMask;
const US: Option<&str> = Some("µs");
const MS: Option<&str> = Some("ms");
const MM: Option<&str> = Some("mm");
const MM_MIN: Option<&str> = Some("mm/min");
const MM_S2: Option<&str> = Some("mm/s²");
const STEPS_MM: Option<&str> = Some("steps/mm");
const MASK: Option<&str> = Some("mask");
const S: Option<&str> = Some("S");

/// Every GRBL 1.1 setting, in `$$` order.
pub const DEFINITIONS: &[SettingDefinition] = &[
    def(
        0,
        "Step pulse time",
        US,
        BYTE,
        "Length of each step pulse. At least 3 µs.",
    ),
    def(
        1,
        "Step idle delay",
        MS,
        BYTE,
        "How long motors stay powered after a move. 255 keeps them on.",
    ),
    def(
        2,
        "Step pulse invert",
        MASK,
        AXES,
        "Inverts the step signal of the axes in the mask.",
    ),
    def(
        3,
        "Step direction invert",
        MASK,
        AXES,
        "Reverses the direction of the axes in the mask.",
    ),
    def(
        4,
        "Invert step enable pin",
        None,
        BOOL,
        "Inverts the stepper enable signal.",
    ),
    def(
        5,
        "Invert limit pins",
        None,
        BOOL,
        "Inverts the limit switch inputs, for normally-closed switches.",
    ),
    def(
        6,
        "Invert probe pin",
        None,
        BOOL,
        "Inverts the probe input.",
    ),
    def(
        10,
        "Status report options",
        MASK,
        SettingKind::StatusMask,
        "1 reports machine position (else work position); 2 adds buffer state.",
    ),
    def(
        11,
        "Junction deviation",
        MM,
        DECIMAL,
        "How fast the machine takes corners. Smaller is slower and smoother.",
    ),
    def(
        12,
        "Arc tolerance",
        MM,
        DECIMAL,
        "How closely arcs (G2/G3) are split into lines.",
    ),
    def(
        13,
        "Report in inches",
        None,
        BOOL,
        "Reports positions in inches instead of millimetres.",
    ),
    def(
        20,
        "Soft limits enable",
        None,
        BOOL,
        "Rejects moves beyond max travel. Needs homing.",
    ),
    def(
        21,
        "Hard limits enable",
        None,
        BOOL,
        "Stops the machine when a limit switch triggers.",
    ),
    def(
        22,
        "Homing cycle enable",
        None,
        BOOL,
        "Enables $H. The machine starts locked in alarm until it homes.",
    ),
    def(
        23,
        "Homing direction invert",
        MASK,
        AXES,
        "Homes the axes in the mask towards the negative end.",
    ),
    def(
        24,
        "Homing locate feed rate",
        MM_MIN,
        DECIMAL,
        "Slow rate for finding the switch precisely.",
    ),
    def(
        25,
        "Homing search seek rate",
        MM_MIN,
        DECIMAL,
        "Fast rate for finding the switch.",
    ),
    def(
        26,
        "Homing switch debounce delay",
        MS,
        WORD,
        "Wait for the switch signal to settle.",
    ),
    def(
        27,
        "Homing switch pull-off distance",
        MM,
        DECIMAL,
        "How far to back off the switch after homing.",
    ),
    def(
        30,
        "Maximum spindle speed",
        S,
        WORD,
        "The S value for full laser power.",
    ),
    def(
        31,
        "Minimum spindle speed",
        S,
        WORD,
        "The S value for the lowest laser power.",
    ),
    def(
        32,
        "Laser-mode enable",
        None,
        BOOL,
        "Keeps moving between G1 moves and switches the laser off for G0. Must be 1 for a laser.",
    ),
    def(
        100,
        "X steps per mm",
        STEPS_MM,
        DECIMAL,
        "Steps the X motor takes to move 1 mm.",
    ),
    def(
        101,
        "Y steps per mm",
        STEPS_MM,
        DECIMAL,
        "Steps the Y motor takes to move 1 mm.",
    ),
    def(
        102,
        "Z steps per mm",
        STEPS_MM,
        DECIMAL,
        "Steps the Z motor takes to move 1 mm.",
    ),
    def(
        110,
        "X max rate",
        MM_MIN,
        DECIMAL,
        "Fastest X speed, used for rapid moves.",
    ),
    def(
        111,
        "Y max rate",
        MM_MIN,
        DECIMAL,
        "Fastest Y speed, used for rapid moves.",
    ),
    def(
        112,
        "Z max rate",
        MM_MIN,
        DECIMAL,
        "Fastest Z speed, used for rapid moves.",
    ),
    def(
        120,
        "X acceleration",
        MM_S2,
        DECIMAL,
        "How quickly X speeds up and slows down.",
    ),
    def(
        121,
        "Y acceleration",
        MM_S2,
        DECIMAL,
        "How quickly Y speeds up and slows down.",
    ),
    def(
        122,
        "Z acceleration",
        MM_S2,
        DECIMAL,
        "How quickly Z speeds up and slows down.",
    ),
    def(
        130,
        "X max travel",
        MM,
        DECIMAL,
        "Length of the X axis. Sets the bed width.",
    ),
    def(
        131,
        "Y max travel",
        MM,
        DECIMAL,
        "Length of the Y axis. Sets the bed depth.",
    ),
    def(132, "Z max travel", MM, DECIMAL, "Length of the Z axis."),
];

pub fn definition(number: u16) -> Option<&'static SettingDefinition> {
    DEFINITIONS.iter().find(|d| d.number == number)
}

/// Settings as `$$` reported them, keyed by number. Values keep the
/// controller's formatting (`0.010`, `1000`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings(BTreeMap<u16, String>);

impl Settings {
    /// Collects the `$n=value` lines of a `$$` reply. Other lines are skipped.
    pub fn parse<'a>(lines: impl IntoIterator<Item = &'a str>) -> Settings {
        Settings(lines.into_iter().filter_map(parse_setting_line).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn raw(&self, number: u16) -> Option<&str> {
        self.0.get(&number).map(String::as_str)
    }

    pub fn number(&self, number: u16) -> Option<f64> {
        self.raw(number)?
            .parse()
            .ok()
            .filter(|v: &f64| v.is_finite())
    }

    pub fn flag(&self, number: u16) -> Option<bool> {
        self.number(number).map(|value| value != 0.0)
    }

    /// Records a value written with `$n=value`.
    pub fn set(&mut self, number: u16, value: String) {
        self.0.insert(number, value);
    }

    /// The settings with what each one means, for the settings table.
    pub fn describe(&self) -> Vec<MachineSetting> {
        self.0
            .iter()
            .map(|(&number, value)| {
                let definition = definition(number);
                MachineSetting {
                    number,
                    value: value.clone(),
                    name: definition.map(|d| d.name.to_string()),
                    unit: definition.and_then(|d| d.unit).map(str::to_string),
                    description: definition.map(|d| d.description.to_string()),
                    kind: definition.map_or(SettingKind::Decimal, |d| d.kind),
                }
            })
            .collect()
    }
}

/// Parses `$11=0.010`. GRBL 0.9 appended a comment (`$0=10 (step pulse,
/// usec)`); that is dropped.
pub fn parse_setting_line(line: &str) -> Option<(u16, String)> {
    let (number, value) = line.trim().strip_prefix('$')?.split_once('=')?;
    let number = number.parse().ok()?;
    let value = value.split_whitespace().next()?;
    value.parse::<f64>().ok()?;
    Some((number, value.to_string()))
}

/// One row of the settings table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MachineSetting {
    pub number: u16,
    /// As the controller printed it.
    pub value: String,
    /// `None` for settings outside GRBL 1.1.
    pub name: Option<String>,
    pub unit: Option<String>,
    pub description: Option<String>,
    pub kind: SettingKind,
}

/// Why a value can't be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingError(String);

impl fmt::Display for SettingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SettingError {}

/// Checks `value` for setting `number` and returns it as it should be sent.
/// Catches what GRBL would reject (negative values, `$0` under 3 µs) and
/// what it would silently truncate.
pub fn validate_setting(number: u16, value: &str) -> Result<String, SettingError> {
    let label = definition(number).map_or_else(
        || format!("${number}"),
        |d| format!("${number} ({})", d.name),
    );
    let fail = |why: &str| Err(SettingError(format!("{label} {why}")));
    let trimmed = value.trim();
    let Ok(parsed) = trimmed.parse::<f64>() else {
        return fail("needs a number.");
    };
    if !parsed.is_finite() {
        return fail("needs a number.");
    }
    if parsed < 0.0 {
        return fail("can't be negative.");
    }
    let Some(definition) = definition(number) else {
        return Ok(trimmed.to_string());
    };
    let whole = |max: u32| {
        if parsed.fract() != 0.0 {
            fail("needs a whole number.")
        } else if parsed > f64::from(max) {
            fail(&format!("can be at most {max}."))
        } else {
            Ok(format!("{}", parsed as u32))
        }
    };
    match definition.kind {
        SettingKind::Integer { max } => {
            if number == 0 && parsed < 3.0 {
                return fail("must be at least 3 µs.");
            }
            whole(max)
        }
        SettingKind::Decimal => Ok(trimmed.to_string()),
        SettingKind::Boolean if parsed == 0.0 || parsed == 1.0 => whole(1),
        SettingKind::Boolean => fail("must be 0 or 1."),
        SettingKind::AxisMask => whole(7),
        SettingKind::StatusMask => whole(3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grbl_setting_is_defined_once_in_order() {
        let numbers: Vec<_> = DEFINITIONS.iter().map(|d| d.number).collect();
        let mut sorted = numbers.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(numbers, sorted);
        assert_eq!(numbers.len(), 34);
    }

    #[test]
    fn parses_a_settings_report_and_skips_other_lines() {
        let settings = Settings::parse([
            "$0=10",
            "$11=0.010",
            "$32=1",
            "$0x=1",
            "[MSG:hello]",
            "$N0=",
            "ok",
        ]);
        assert_eq!(settings.raw(0), Some("10"));
        assert_eq!(settings.number(11), Some(0.01));
        assert_eq!(settings.flag(32), Some(true));
        assert_eq!(settings.describe().len(), 3);
    }

    #[test]
    fn accepts_grbl_09_comments() {
        assert_eq!(
            parse_setting_line("$0=10 (step pulse, usec)"),
            Some((0, "10".to_string()))
        );
    }

    #[test]
    fn describes_known_and_unknown_settings() {
        let rows = Settings::parse(["$32=0", "$300=5"]).describe();
        assert_eq!(rows[0].name.as_deref(), Some("Laser-mode enable"));
        assert_eq!(rows[0].kind, SettingKind::Boolean);
        assert_eq!(rows[1].name, None);
        assert_eq!(rows[1].kind, SettingKind::Decimal);
    }

    #[test]
    fn validates_values_the_way_grbl_would_take_them() {
        assert_eq!(validate_setting(110, " 6000.5 ").unwrap(), "6000.5");
        assert_eq!(validate_setting(32, "1").unwrap(), "1");
        assert_eq!(validate_setting(32, "1.0").unwrap(), "1");
        assert_eq!(validate_setting(30, "1000").unwrap(), "1000");
        assert_eq!(validate_setting(3, "5").unwrap(), "5");
        assert_eq!(validate_setting(300, "2.5").unwrap(), "2.5");
        for (number, value, why) in [
            (110, "fast", "needs a number"),
            (110, "inf", "needs a number"),
            (110, "-1", "negative"),
            (32, "2", "0 or 1"),
            (30, "1.5", "whole number"),
            (1, "256", "at most 255"),
            (3, "8", "at most 7"),
            (10, "4", "at most 3"),
            (0, "2", "at least 3"),
        ] {
            let error = validate_setting(number, value).unwrap_err().to_string();
            assert!(error.contains(why), "${number}={value}: {error}");
            assert!(error.starts_with(&format!("${number}")), "{error}");
        }
    }
}
