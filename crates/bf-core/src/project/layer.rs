//! Cut layers: a colour and the settings every object on it is cut with.
//! The frontend edits them in the Layers panel; the toolpath planner reads
//! them from the document. Layer order in the document is the cut order.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ProjectError;

/// How a layer's shapes are lasered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub enum LayerMode {
    /// Follow the outlines.
    Line,
    /// Scan back and forth to fill closed shapes.
    Fill,
}

/// A cut layer. Objects name their layer by `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub struct CutLayer {
    pub id: String,
    pub name: String,
    /// `#rrggbb`, from the layer palette. Objects draw in it.
    pub color: String,
    pub mode: LayerMode,
    /// Feed rate while lasering, mm/min.
    pub speed: f64,
    /// Laser power, percent of the machine's maximum.
    pub max_power: f64,
    /// Power floor at corners and slowdowns, percent; at most `max_power`.
    pub min_power: f64,
    /// How many times the layer is cut.
    pub passes: u32,
    /// Fire at constant power (GRBL `M3`) instead of the default dynamic
    /// power (`M4`), which scales power with speed so corners don't overburn.
    #[serde(default)]
    pub constant_power: bool,
    pub air_assist: bool,
    /// Whether the layer is sent to the laser.
    pub output: bool,
    /// Whether the layer is drawn on the canvas. Hidden layers still cut.
    pub shown: bool,
    /// Fill only: distance between scan lines, mm.
    pub line_interval: f64,
    /// Fill only: scan direction in degrees anticlockwise from +X.
    pub scan_angle: f64,
    /// Fill only: lase in both directions rather than only one.
    pub bidirectional: bool,
    /// Fill only: how far past each scan line the head travels, mm, so it is
    /// at speed before the laser turns on.
    pub overscan: f64,
    /// Fill only: gaps along a scan line shorter than this, mm, are crossed
    /// with the laser off at cutting speed rather than stopping for a rapid.
    #[serde(default = "default_fill_gap")]
    pub fill_gap: f64,
}

/// The fill gap a layer saved before it existed gets.
const DEFAULT_FILL_GAP: f64 = 1.0;

fn default_fill_gap() -> f64 {
    DEFAULT_FILL_GAP
}

impl CutLayer {
    /// A layer with the default settings.
    pub fn new(id: impl Into<String>, name: impl Into<String>, color: impl Into<String>) -> Self {
        CutLayer {
            id: id.into(),
            name: name.into(),
            color: color.into(),
            mode: LayerMode::Line,
            speed: 1000.0,
            max_power: 50.0,
            min_power: 0.0,
            passes: 1,
            constant_power: false,
            air_assist: true,
            output: true,
            shown: true,
            line_interval: 0.1,
            scan_angle: 0.0,
            bidirectional: true,
            overscan: 2.5,
            fill_gap: DEFAULT_FILL_GAP,
        }
    }

    /// Checks the settings make sense; the error names the layer and setting.
    pub fn validate(&self) -> Result<(), ProjectError> {
        let fail = |what: &str| {
            Err(ProjectError::invalid(format!(
                "layer {} has {what}",
                self.name
            )))
        };
        if self.id.is_empty() {
            return fail("no id");
        }
        let hex = self.color.strip_prefix('#').unwrap_or("");
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return fail("a colour that isn't #rrggbb");
        }
        if !(self.speed.is_finite() && self.speed > 0.0) {
            return fail("a speed that isn't positive");
        }
        let percent = |value: f64| (0.0..=100.0).contains(&value);
        if !percent(self.max_power) || !percent(self.min_power) {
            return fail("a power outside 0–100%");
        }
        if self.min_power > self.max_power {
            return fail("a minimum power above its maximum");
        }
        if self.passes == 0 {
            return fail("no passes");
        }
        if !(self.line_interval.is_finite() && self.line_interval > 0.0) {
            return fail("a line interval that isn't positive");
        }
        if !self.scan_angle.is_finite() {
            return fail("a scan angle that isn't a number");
        }
        if !(self.overscan.is_finite() && self.overscan >= 0.0) {
            return fail("a negative overscan");
        }
        if !(self.fill_gap.is_finite() && self.fill_gap >= 0.0) {
            return fail("a negative fill gap");
        }
        Ok(())
    }
}

/// The colours format 1 drew layers in: the common laser-software palette,
/// picked by the id's trailing number (`layer-3` is the third). Migrating a
/// format 1 file keeps each layer the colour it was.
const FORMAT_1_PALETTE: [&str; 16] = [
    "#0000ff", "#ff0000", "#00e000", "#d0d000", "#ff8000", "#00e0e0", "#ff00ff", "#b4b4b4",
    "#0000a0", "#a00000", "#00a000", "#a0a000", "#c08000", "#00a0ff", "#a000a0", "#808080",
];

/// The colour format 1 gave a layer id, matching its `layerColor`.
pub(super) fn format_1_color(id: &str) -> &'static str {
    let digits = id.len() - id.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let index: i64 = if digits > 0 {
        id[id.len() - digits..].parse::<i64>().unwrap_or(1) - 1
    } else {
        // UTF-16 code units, as JavaScript's `charCodeAt` hashes them.
        id.encode_utf16().fold(0u32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(u32::from(unit))
        }) as i64
    };
    FORMAT_1_PALETTE[index.rem_euclid(FORMAT_1_PALETTE.len() as i64) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layer_is_valid() {
        CutLayer::new("layer-1", "Layer 1", "#0000ff")
            .validate()
            .unwrap();
    }

    #[test]
    fn validation_names_the_bad_setting() {
        let base = CutLayer::new("layer-1", "Layer 1", "#0000ff");
        type Break = fn(&mut CutLayer);
        let cases: [(Break, &str); 8] = [
            (|l| l.color = "blue".into(), "colour"),
            (|l| l.speed = 0.0, "speed"),
            (|l| l.max_power = 101.0, "power outside"),
            (|l| l.min_power = 60.0, "minimum power above"),
            (|l| l.passes = 0, "no passes"),
            (|l| l.line_interval = 0.0, "line interval"),
            (|l| l.overscan = -1.0, "overscan"),
            (|l| l.fill_gap = f64::NAN, "fill gap"),
        ];
        for (break_it, expected) in cases {
            let mut layer = base.clone();
            break_it(&mut layer);
            let error = layer.validate().unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
            assert!(error.contains("layer Layer 1"), "{error}");
        }
    }

    #[test]
    fn format_1_colours_match_the_old_frontend() {
        assert_eq!(format_1_color("layer-1"), "#0000ff");
        assert_eq!(format_1_color("layer-2"), "#ff0000");
        assert_eq!(format_1_color("layer-17"), "#0000ff");
        assert_eq!(format_1_color("C00"), "#808080");
        // "abc": ((97 * 31 + 98) * 31 + 99) % 16 = 96354 % 16 = 2.
        assert_eq!(format_1_color("abc"), "#00e000");
    }
}
