//! Material test grids: a speed × power grid of cells, each on its own cut
//! layer with its own settings, labelled with the values along each axis.
//!
//! Columns step through power (left to right) and rows through speed (top
//! to bottom), so the grid reads like a table. The labels are text objects
//! (laid out with [`crate::text::layout`], so they keep their outlines) on a
//! separate low-power score layer that cuts first. Everything goes in one
//! group, placed with its bottom-left corner at the requested origin; a grid
//! that would run off the bed is refused, saying by how much it is too big.
//!
//! Ids are built from the id the caller passes: the group is `<id>`, its
//! label layer `<id>-labels` and cell layers `<id>-r<row>-c<column>`, so the
//! editor can find a grid's cells again in a saved document.

use std::fmt;

use fontdb::Database;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::machine::BedSize;
use crate::project::{CutLayer, LayerMode, Point, SceneObject, Segment, Subpath, Transform};
use crate::text::{self, TextAlign, TextStyle};

/// The most values along either axis: 20 × 20 is already 400 layers.
pub const MAX_STEPS: u32 = 20;

/// The label layer's colour: grey, visible on light and dark canvases.
const LABEL_COLOR: &str = "#808080";

/// Values evenly spaced from `from` to `to`, both included.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "test-grid/")]
pub struct ValueRange {
    pub from: f64,
    pub to: f64,
    /// How many values; 1 uses just `from`.
    pub steps: u32,
}

impl ValueRange {
    /// The values, each rounded to `decimals` places.
    fn values(&self, decimals: i32) -> Vec<f64> {
        let round = |value: f64| {
            let scale = 10f64.powi(decimals);
            (value * scale).round() / scale
        };
        if self.steps <= 1 {
            return vec![round(self.from)];
        }
        let last = f64::from(self.steps - 1);
        (0..self.steps)
            .map(|i| round(self.from + (self.to - self.from) * f64::from(i) / last))
            .collect()
    }
}

/// How the axis labels are set and lasered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "test-grid/")]
pub struct LabelSettings {
    /// Requested family; a stand-in is used when it isn't installed.
    pub font_family: String,
    /// Cap height, mm.
    pub height: f64,
    /// Score speed, mm/min.
    pub speed: f64,
    /// Score power, percent.
    pub power: f64,
}

/// What the test-grid dialog asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "test-grid/")]
pub struct TestGridSettings {
    /// One row per value, mm/min, top to bottom.
    pub speed: ValueRange,
    /// One column per value, percent, left to right.
    pub power: ValueRange,
    /// Side of each square cell, mm.
    pub cell_size: f64,
    /// Space between cells, mm.
    pub gap: f64,
    pub mode: LayerMode,
    /// Fill only: distance between scan lines, mm.
    pub line_interval: f64,
    pub passes: u32,
    pub air_assist: bool,
    pub labels: LabelSettings,
    /// Where the bottom-left corner of the whole test (labels included) goes, mm.
    pub origin: Point,
}

impl Default for TestGridSettings {
    fn default() -> Self {
        TestGridSettings {
            speed: ValueRange {
                from: 1000.0,
                to: 3000.0,
                steps: 5,
            },
            power: ValueRange {
                from: 20.0,
                to: 100.0,
                steps: 5,
            },
            cell_size: 10.0,
            gap: 2.0,
            mode: LayerMode::Fill,
            line_interval: 0.1,
            passes: 1,
            air_assist: true,
            labels: LabelSettings {
                font_family: "Arial".into(),
                height: 3.0,
                speed: 3000.0,
                power: 10.0,
            },
            origin: Point { x: 10.0, y: 10.0 },
        }
    }
}

/// A generated grid, ready to add to the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "test-grid/")]
pub struct TestGrid {
    /// The label layer, then one layer per cell, row by row: the cut order.
    pub layers: Vec<CutLayer>,
    /// One group holding the cells and labels.
    pub objects: Vec<SceneObject>,
    /// The whole test's size, labels included, mm.
    pub width: f64,
    pub height: f64,
    /// The family the labels were drawn with.
    pub label_font: String,
}

/// Why a grid can't be made, worded for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestGridError(String);

impl fmt::Display for TestGridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TestGridError {}

fn fail<T>(why: impl Into<String>) -> Result<T, TestGridError> {
    Err(TestGridError(why.into()))
}

/// The id of the layer for the cell at `row`, `column` of grid `id`.
pub fn cell_layer_id(id: &str, row: usize, column: usize) -> String {
    format!("{id}-r{row}-c{column}")
}

/// The id of grid `id`'s label layer.
pub fn label_layer_id(id: &str) -> String {
    format!("{id}-labels")
}

fn check(settings: &TestGridSettings) -> Result<(), TestGridError> {
    let positive = |value: f64| value.is_finite() && value > 0.0;
    let percent = |value: f64| value.is_finite() && (0.0..=100.0).contains(&value);
    for (name, range) in [("speed", &settings.speed), ("power", &settings.power)] {
        if !(1..=MAX_STEPS).contains(&range.steps) {
            return fail(format!("Use 1 to {MAX_STEPS} {name} steps"));
        }
    }
    let speed = &settings.speed;
    if !positive(speed.from) || (speed.steps > 1 && !positive(speed.to)) {
        return fail("Speeds must be above zero");
    }
    let power = &settings.power;
    if !percent(power.from) || (power.steps > 1 && !percent(power.to)) {
        return fail("Power is 0–100%");
    }
    if !positive(settings.cell_size) {
        return fail("Cell size must be above zero");
    }
    if !(settings.gap.is_finite() && settings.gap >= 0.0) {
        return fail("The gap can't be negative");
    }
    if !positive(settings.line_interval) {
        return fail("Line interval must be above zero");
    }
    if settings.passes == 0 {
        return fail("Passes is a whole number from 1");
    }
    let labels = &settings.labels;
    if !positive(labels.height) {
        return fail("Label height must be above zero");
    }
    if !positive(labels.speed) {
        return fail("Label speed must be above zero");
    }
    if !percent(labels.power) {
        return fail("Label power is 0–100%");
    }
    if !(settings.origin.x.is_finite() && settings.origin.y.is_finite()) {
        return fail("The position must be a number");
    }
    Ok(())
}

/// `1500`, `12.5`: no trailing zeros.
fn number(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").unwrap_or(&text).to_string()
}

/// A colour per cell, around the hue wheel so neighbours stay distinct.
fn cell_color(index: usize, count: usize) -> String {
    let hue = 360.0 * index as f64 / count.max(1) as f64;
    let (s, l) = (0.85, 0.5);
    let c = (1.0 - (2.0 * l - 1.0f64).abs()) * s;
    let h = hue / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

fn translate(x: f64, y: f64) -> Transform {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

/// A quarter turn anticlockwise, then a move: text reading bottom to top.
fn rotate_up(x: f64, y: f64) -> Transform {
    [0.0, 1.0, -1.0, 0.0, x, y]
}

fn apply(t: &Transform, p: Point) -> Point {
    Point {
        x: t[0] * p.x + t[2] * p.y + t[4],
        y: t[1] * p.x + t[3] * p.y + t[5],
    }
}

/// Min and max corners around `points`, or None if there are none.
fn bounds(points: impl IntoIterator<Item = Point>) -> Option<(Point, Point)> {
    points.into_iter().fold(None, |acc, p| {
        Some(match acc {
            None => (p, p),
            Some((min, max)) => (
                Point {
                    x: min.x.min(p.x),
                    y: min.y.min(p.y),
                },
                Point {
                    x: max.x.max(p.x),
                    y: max.y.max(p.y),
                },
            ),
        })
    })
}

/// Every point of an outline, control points included (a box around them
/// holds the curve).
fn outline_points(outline: &[Subpath]) -> impl Iterator<Item = Point> + '_ {
    outline.iter().flat_map(|path| {
        std::iter::once(path.start).chain(path.segments.iter().flat_map(|segment| match *segment {
            Segment::Line { to } => vec![to],
            Segment::Quad { control, to } => vec![control, to],
            Segment::Cubic {
                control1,
                control2,
                to,
            } => vec![control1, control2, to],
        }))
    })
}

/// A label laid out but not yet placed.
struct Label {
    text: String,
    align: TextAlign,
    outline: Vec<Subpath>,
    /// Outline bounds in the text's own space.
    min: Point,
    max: Point,
}

impl Label {
    fn width(&self) -> f64 {
        self.max.x - self.min.x
    }
}

/// A label with where it goes.
struct Placed {
    label: Label,
    transform: Transform,
}

/// Lays out a speed × power test grid as layers and objects, ids made from
/// `id`, placed at `settings.origin`. Fails if the settings don't make sense
/// or the grid doesn't fit on `bed`.
pub fn test_grid(
    db: &Database,
    id: &str,
    settings: &TestGridSettings,
    bed: BedSize,
) -> Result<TestGrid, TestGridError> {
    if id.is_empty() {
        return fail("A test grid needs an id");
    }
    check(settings)?;
    let speeds = settings.speed.values(0);
    let powers = settings.power.values(1);
    let labels = &settings.labels;
    let h = labels.height;
    let pad = h;

    let mut label_font = labels.font_family.clone();
    let mut set = |text: &str, align: TextAlign| -> Result<Label, TestGridError> {
        let style = TextStyle {
            font_family: labels.font_family.clone(),
            font_weight: 400,
            italic: false,
            height: h,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align,
        };
        let laid = text::layout(db, text, &style).map_err(|e| TestGridError(e.to_string()))?;
        label_font = laid.font_family;
        let (min, max) = bounds(outline_points(&laid.outline))
            .unwrap_or((Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: h }));
        Ok(Label {
            text: text.to_string(),
            align,
            outline: laid.outline,
            min,
            max,
        })
    };

    let speed_labels = speeds
        .iter()
        .map(|speed| set(&number(*speed), TextAlign::Right))
        .collect::<Result<Vec<_>, _>>()?;
    let power_labels = powers
        .iter()
        .map(|power| set(&number(*power), TextAlign::Center))
        .collect::<Result<Vec<_>, _>>()?;
    let speed_title = set("Speed (mm/min)", TextAlign::Center)?;
    let power_title = set("Power (%)", TextAlign::Center)?;
    let caption = set(&caption(settings), TextAlign::Left)?;

    // Built with the grid's bottom-left cell corner at (0, 0), then moved.
    let (rows, columns) = (speeds.len(), powers.len());
    let pitch = settings.cell_size + settings.gap;
    let grid_width = columns as f64 * pitch - settings.gap;
    let grid_height = rows as f64 * pitch - settings.gap;
    let cell_x = |column: usize| column as f64 * pitch;
    // Row 0 is the top row.
    let cell_y = |row: usize| (rows - 1 - row) as f64 * pitch;

    let mut placed = Vec::new();
    let widest = speed_labels.iter().map(Label::width).fold(0.0f64, f64::max);
    for (row, label) in speed_labels.into_iter().enumerate() {
        let baseline = cell_y(row) + (settings.cell_size - h) / 2.0;
        placed.push(Placed {
            label,
            transform: translate(-pad, baseline),
        });
    }
    for (column, label) in power_labels.into_iter().enumerate() {
        let x = cell_x(column) + settings.cell_size / 2.0;
        placed.push(Placed {
            label,
            transform: translate(x, grid_height + pad),
        });
    }
    placed.push(Placed {
        transform: translate(grid_width / 2.0, grid_height + pad + h + pad),
        label: power_title,
    });
    // Turned to read upwards, its own y runs along -x: the bottom of its
    // outline (descenders) sits `pad` left of the widest speed label.
    let title_x = -pad - widest - pad + speed_title.min.y;
    placed.push(Placed {
        transform: rotate_up(title_x, grid_height / 2.0),
        label: speed_title,
    });
    placed.push(Placed {
        transform: translate(0.0, -pad - caption.max.y),
        label: caption,
    });

    let cell_corners = [
        Point { x: 0.0, y: 0.0 },
        Point {
            x: grid_width,
            y: grid_height,
        },
    ];
    let label_points = placed.iter().flat_map(|p| {
        let t = p.transform;
        [
            Point {
                x: p.label.min.x,
                y: p.label.min.y,
            },
            Point {
                x: p.label.max.x,
                y: p.label.min.y,
            },
            Point {
                x: p.label.min.x,
                y: p.label.max.y,
            },
            Point {
                x: p.label.max.x,
                y: p.label.max.y,
            },
        ]
        .map(move |corner| apply(&t, corner))
    });
    let (min, max) = bounds(cell_corners.into_iter().chain(label_points)).expect("cells exist");
    let (width, height) = (max.x - min.x, max.y - min.y);
    let origin = settings.origin;
    let (dx, dy) = (origin.x - min.x, origin.y - min.y);

    let mm = |value: f64| format!("{value:.0}");
    if origin.x < 0.0 || origin.y < 0.0 {
        return fail(format!(
            "The test starts at X{} Y{}, off the bed. Use a position of 0 or more.",
            number(origin.x),
            number(origin.y)
        ));
    }
    if origin.x + width > bed.width || origin.y + height > bed.height {
        return fail(format!(
            "The test is {} × {} mm with its labels, so at X{} Y{} it runs off the {} × {} mm \
             bed. Use fewer steps, smaller cells or a smaller gap, or move it.",
            mm(width.ceil()),
            mm(height.ceil()),
            number(origin.x),
            number(origin.y),
            mm(bed.width),
            mm(bed.height)
        ));
    }

    let mut label_layer = CutLayer::new(label_layer_id(id), "Test labels", LABEL_COLOR);
    label_layer.speed = labels.speed;
    label_layer.max_power = labels.power;
    label_layer.air_assist = settings.air_assist;
    let mut layers = vec![label_layer];
    let mut children = Vec::new();
    for (row, speed) in speeds.iter().enumerate() {
        for (column, power) in powers.iter().enumerate() {
            let name = format!("{} mm/min · {}%", number(*speed), number(*power));
            let layer_id = cell_layer_id(id, row, column);
            let mut layer = CutLayer::new(
                &layer_id,
                &name,
                cell_color(row * columns + column, rows * columns),
            );
            layer.mode = settings.mode;
            layer.speed = *speed;
            layer.max_power = *power;
            layer.passes = settings.passes;
            layer.line_interval = settings.line_interval;
            layer.air_assist = settings.air_assist;
            layers.push(layer);
            children.push(SceneObject::Rect {
                id: format!("{id}-cell-r{row}-c{column}"),
                name,
                layer_id,
                transform: translate(cell_x(column) + dx, cell_y(row) + dy),
                width: settings.cell_size,
                height: settings.cell_size,
                corner_radius: 0.0,
            });
        }
    }
    for (index, Placed { label, transform }) in placed.into_iter().enumerate() {
        let mut transform = transform;
        transform[4] += dx;
        transform[5] += dy;
        children.push(SceneObject::Text {
            id: format!("{id}-label-{index}"),
            name: label.text.clone(),
            layer_id: label_layer_id(id),
            transform,
            text: label.text,
            font_family: labels.font_family.clone(),
            font_weight: 400,
            italic: false,
            height: h,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: label.align,
            outline: label.outline,
        });
    }
    let group = SceneObject::Group {
        id: id.to_string(),
        name: "Material test".into(),
        layer_id: label_layer_id(id),
        transform: translate(0.0, 0.0),
        children,
    };
    Ok(TestGrid {
        layers,
        objects: vec![group],
        width,
        height,
        label_font,
    })
}

/// "Fill 0.1 mm, 1 pass": the settings every cell shares.
fn caption(settings: &TestGridSettings) -> String {
    let mode = match settings.mode {
        LayerMode::Fill => format!("Fill {} mm", number(settings.line_interval)),
        LayerMode::Line => "Line".to_string(),
    };
    let passes = match settings.passes {
        1 => "1 pass".to_string(),
        n => format!("{n} passes"),
    };
    format!("{mode}, {passes}")
}

#[cfg(test)]
mod tests;
