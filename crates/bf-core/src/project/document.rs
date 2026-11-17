//! The design document as stored in a project file. The frontend owns the
//! document (`src/lib/document/model.ts`); these types mirror it so Rust can
//! validate what it reads and writes. `src/lib/project/contract.ts` fails
//! svelte-check if the two drift apart.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{CutLayer, ProjectError};
use crate::text::TextAlign;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "project/")]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub enum Segment {
    Line {
        to: Point,
    },
    Quad {
        control: Point,
        to: Point,
    },
    Cubic {
        control1: Point,
        control2: Point,
        to: Point,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "project/")]
pub struct Subpath {
    pub start: Point,
    pub segments: Vec<Segment>,
    pub closed: bool,
}

/// `[a, b, c, d, e, f]` in SVG/canvas order.
pub type Transform = [f64; 6];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, export_to = "project/")]
pub enum SceneObject {
    Path {
        id: String,
        name: String,
        layer_id: String,
        #[ts(type = "readonly [number, number, number, number, number, number]")]
        transform: Transform,
        subpaths: Vec<Subpath>,
    },
    Rect {
        id: String,
        name: String,
        layer_id: String,
        #[ts(type = "readonly [number, number, number, number, number, number]")]
        transform: Transform,
        width: f64,
        height: f64,
        corner_radius: f64,
    },
    Ellipse {
        id: String,
        name: String,
        layer_id: String,
        #[ts(type = "readonly [number, number, number, number, number, number]")]
        transform: Transform,
        rx: f64,
        ry: f64,
    },
    Polygon {
        id: String,
        name: String,
        layer_id: String,
        #[ts(type = "readonly [number, number, number, number, number, number]")]
        transform: Transform,
        sides: u32,
        radius: f64,
        inner_radius: Option<f64>,
    },
    /// Editable text. `outline` is the text as laid out by
    /// [`crate::text::layout`] when it was last edited, so the file draws
    /// and cuts the same on a machine without the font.
    Text {
        id: String,
        name: String,
        layer_id: String,
        #[ts(type = "readonly [number, number, number, number, number, number]")]
        transform: Transform,
        text: String,
        font_family: String,
        font_weight: u16,
        italic: bool,
        /// Cap height, mm.
        height: f64,
        letter_spacing: f64,
        line_spacing: f64,
        align: TextAlign,
        outline: Vec<Subpath>,
    },
    Group {
        id: String,
        name: String,
        layer_id: String,
        #[ts(type = "readonly [number, number, number, number, number, number]")]
        transform: Transform,
        children: Vec<SceneObject>,
    },
}

impl SceneObject {
    pub fn id(&self) -> &str {
        match self {
            SceneObject::Path { id, .. }
            | SceneObject::Rect { id, .. }
            | SceneObject::Ellipse { id, .. }
            | SceneObject::Polygon { id, .. }
            | SceneObject::Text { id, .. }
            | SceneObject::Group { id, .. } => id,
        }
    }

    pub fn layer_id(&self) -> &str {
        match self {
            SceneObject::Path { layer_id, .. }
            | SceneObject::Rect { layer_id, .. }
            | SceneObject::Ellipse { layer_id, .. }
            | SceneObject::Polygon { layer_id, .. }
            | SceneObject::Text { layer_id, .. }
            | SceneObject::Group { layer_id, .. } => layer_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "project/")]
pub struct DesignDocument {
    #[ts(type = "1")]
    pub version: u32,
    #[ts(type = "\"mm\"")]
    pub units: String,
    /// Cut layers in cut order. Every object's `layerId` names one of them.
    pub layers: Vec<CutLayer>,
    pub objects: Vec<SceneObject>,
}

impl DesignDocument {
    /// Checks what serde can't: the document version, units, unique ids,
    /// sensible shape sizes and layer settings, and that every object is on a
    /// layer the document has.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != 1 {
            return Err(ProjectError::invalid(format!(
                "unsupported document version {}",
                self.version
            )));
        }
        if self.units != "mm" {
            return Err(ProjectError::invalid(format!(
                "unsupported units \"{}\"",
                self.units
            )));
        }
        let mut layers = HashSet::new();
        for layer in &self.layers {
            layer.validate()?;
            if !layers.insert(layer.id.as_str()) {
                return Err(ProjectError::invalid(format!(
                    "layer id {} is used twice",
                    layer.id
                )));
            }
        }
        let mut ids = HashSet::new();
        validate_objects(&self.objects, &mut ids, &layers)
    }
}

fn validate_objects<'a>(
    objects: &'a [SceneObject],
    ids: &mut HashSet<&'a str>,
    layers: &HashSet<&str>,
) -> Result<(), ProjectError> {
    for object in objects {
        let id = object.id();
        if !layers.contains(object.layer_id()) {
            return Err(ProjectError::invalid(format!(
                "object {id} is on layer {}, which the document doesn't have",
                object.layer_id()
            )));
        }
        if id.is_empty() {
            return Err(ProjectError::invalid("an object has no id".to_string()));
        }
        if !ids.insert(id) {
            return Err(ProjectError::invalid(format!(
                "object id {id} is used twice"
            )));
        }
        let negative = |what: &str, value: f64| {
            if value < 0.0 {
                Err(ProjectError::invalid(format!(
                    "object {id} has a negative {what}"
                )))
            } else {
                Ok(())
            }
        };
        match object {
            SceneObject::Path { .. } => {}
            SceneObject::Rect {
                width,
                height,
                corner_radius,
                ..
            } => {
                negative("width", *width)?;
                negative("height", *height)?;
                negative("corner radius", *corner_radius)?;
            }
            SceneObject::Ellipse { rx, ry, .. } => {
                negative("radius", *rx)?;
                negative("radius", *ry)?;
            }
            SceneObject::Polygon {
                sides,
                radius,
                inner_radius,
                ..
            } => {
                if *sides < 3 {
                    return Err(ProjectError::invalid(format!(
                        "polygon {id} has fewer than 3 sides"
                    )));
                }
                negative("radius", *radius)?;
                negative("inner radius", inner_radius.unwrap_or(0.0))?;
            }
            SceneObject::Text {
                height,
                line_spacing,
                font_weight,
                ..
            } => {
                let positive = |value: f64| value.is_finite() && value > 0.0;
                if !positive(*height) || !positive(*line_spacing) {
                    return Err(ProjectError::invalid(format!(
                        "text {id} needs a height and line spacing above zero"
                    )));
                }
                if !(1..=1000).contains(font_weight) {
                    return Err(ProjectError::invalid(format!(
                        "text {id} has a font weight outside 1 to 1000"
                    )));
                }
            }
            SceneObject::Group { children, .. } => validate_objects(children, ids, layers)?,
        }
    }
    Ok(())
}
