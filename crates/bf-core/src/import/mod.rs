//! File import: vector artwork read into paths in millimetres, Y-up like the
//! design document, each with the colour that decides its cut layer.
//!
//! [`svg`] reads SVG and [`dxf`] reads DXF. The UI maps colours to layers and
//! inserts the paths as one undoable command.

mod curve;
pub mod dxf;
mod join;
pub mod svg;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportedPoint {
    pub x: f64,
    pub y: f64,
}

/// One path segment, shaped like the document's `Segment`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ImportedSegment {
    Line {
        to: ImportedPoint,
    },
    Quad {
        control: ImportedPoint,
        to: ImportedPoint,
    },
    Cubic {
        control1: ImportedPoint,
        control2: ImportedPoint,
        to: ImportedPoint,
    },
}

impl ImportedSegment {
    pub fn to(&self) -> ImportedPoint {
        match *self {
            ImportedSegment::Line { to }
            | ImportedSegment::Quad { to, .. }
            | ImportedSegment::Cubic { to, .. } => to,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportedSubpath {
    pub start: ImportedPoint,
    pub segments: Vec<ImportedSegment>,
    pub closed: bool,
}

/// One shape from the file, in world millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportedPath {
    /// The source element's id, or empty.
    pub name: String,
    /// `#rrggbb`: the stroke colour, else the fill colour.
    pub color: String,
    pub subpaths: Vec<ImportedSubpath>,
}

/// The kind of file a drawing came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ImportFormat {
    Svg,
    Dxf,
}

impl std::fmt::Display for ImportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportFormat::Svg => "SVG",
            ImportFormat::Dxf => "DXF",
        })
    }
}

/// The units to read a file in when it doesn't say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum LengthUnit {
    #[serde(rename = "mm")]
    Millimeters,
    #[serde(rename = "in")]
    Inches,
}

/// An imported drawing. The page's bottom-left corner is the origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportedDrawing {
    pub format: ImportFormat,
    /// Page size in mm.
    pub width: f64,
    pub height: f64,
    /// Paths bottom to top.
    pub paths: Vec<ImportedPath>,
    /// Total segments across all paths.
    pub segments: u32,
    /// The DPI that unitless sizes were read at, or null when the file's
    /// width and height carry absolute units (mm, in, pt...).
    pub dpi: Option<f64>,
    /// Why that DPI was chosen, e.g. "Adobe Illustrator".
    pub dpi_reason: Option<String>,
    /// The units a DXF was drawn in, e.g. "inches"; null for SVG.
    pub units: Option<String>,
    /// Elements that can't be cut and were left out: bitmaps in an SVG;
    /// hatches, dimensions, 3D entities (and text when no fonts are installed)
    /// in a DXF.
    pub skipped: u32,
    /// Fonts a DXF's text asked for that aren't installed, and what drew it
    /// instead; empty for SVG.
    pub substituted_fonts: Vec<FontSubstitution>,
}

/// Text drawn in a stand-in because its font isn't installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FontSubstitution {
    /// The font the file names, e.g. `romans.shx`.
    pub requested: String,
    /// The installed family the text was drawn in.
    pub used: String,
}

/// Why a file could not be imported, worded for the user.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    Read(String),
    Invalid {
        format: ImportFormat,
        reason: String,
    },
    Empty(ImportFormat),
    /// A DXF that doesn't say what units it is in. Its size in its own units
    /// lets the user tell mm from inches; import again with a [`LengthUnit`].
    NeedsUnits {
        width: f64,
        height: f64,
    },
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Read(reason) => write!(f, "Couldn't read the file: {reason}"),
            ImportError::Invalid { format, reason } => {
                write!(f, "This isn't a valid {format}: {reason}")
            }
            ImportError::Empty(format) => write!(f, "The {format} has no shapes to import."),
            ImportError::NeedsUnits { .. } => {
                write!(
                    f,
                    "The DXF doesn't say whether it is in millimetres or inches."
                )
            }
        }
    }
}

impl std::error::Error for ImportError {}
