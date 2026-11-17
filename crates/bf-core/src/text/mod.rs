//! Text to outlines: the system's fonts ([`fonts`]) and laying a text object
//! out as Bezier paths ([`layout`]).
//!
//! Fonts come from the operating system through `fontdb`, loaded once. Text
//! is shaped with `rustybuzz`, so kerning and ligatures match what the font
//! intends, and each glyph's outline is copied out in millimetres.
//!
//! Sizes are cap heights: `height` is how tall a capital H stands, the way
//! engraved lettering is measured. The first
//! line's baseline is at y = 0 and later lines go down (Y is up). With left
//! alignment a line starts at x = 0; centred lines are centred on x = 0 and
//! right-aligned lines end there.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use rustybuzz::ttf_parser::{self, GlyphId, OutlineBuilder};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::project::{Point, Segment, Subpath};

/// Cap height as a share of the em, for fonts that don't record one and have
/// no H to measure.
const FALLBACK_CAP_HEIGHT: f64 = 0.7;

/// One font family installed on the system and the styles it comes in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "text/")]
pub struct FontFamily {
    pub name: String,
    /// Lightest first, upright before italic.
    pub styles: Vec<FontStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "text/")]
pub struct FontStyle {
    /// CSS weight, 100 (thin) to 900 (black); 400 is regular.
    pub weight: u16,
    pub italic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "text/")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// How a text object's string is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "text/")]
pub struct TextStyle {
    pub font_family: String,
    pub font_weight: u16,
    pub italic: bool,
    /// Cap height, mm.
    pub height: f64,
    /// Extra space after each character, mm; negative tightens.
    pub letter_spacing: f64,
    /// Distance between baselines as a multiple of the font's line height.
    pub line_spacing: f64,
    pub align: TextAlign,
}

/// A laid-out text object: its outline and where the caret can go.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "text/")]
pub struct TextLayout {
    /// Every glyph's contours, in mm, in the text object's own space.
    pub outline: Vec<Subpath>,
    /// One per line of the string (split at `\n`).
    pub lines: Vec<TextLine>,
    /// The family the outline was drawn with: the one asked for, or a
    /// stand-in when that isn't installed.
    pub font_family: String,
    /// True when the requested family isn't installed.
    pub substituted: bool,
}

/// Caret positions along one line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "text/")]
pub struct TextLine {
    /// The line's baseline, mm.
    pub baseline: f64,
    /// Cap height, mm: how tall to draw the caret above the baseline.
    pub cap_height: f64,
    /// The caret's x before each UTF-16 code unit of the line, then after the
    /// last: `length + 1` entries, so a JavaScript string index maps straight
    /// to a position.
    pub carets: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    /// No fonts are installed at all.
    NoFonts,
    /// The style can't be used, e.g. a height of zero.
    Invalid(String),
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextError::NoFonts => write!(f, "No fonts are installed"),
            TextError::Invalid(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for TextError {}

/// The system's fonts, loaded on first use (a few hundred milliseconds).
pub fn system_fonts() -> &'static Database {
    static SYSTEM: OnceLock<Database> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        db
    })
}

/// The installed font families, sorted by name.
pub fn fonts(db: &Database) -> Vec<FontFamily> {
    let mut families: BTreeMap<String, Vec<FontStyle>> = BTreeMap::new();
    for face in db.faces() {
        // The first name is the English one.
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        if name.starts_with('.') {
            continue; // Hidden system fonts on macOS.
        }
        families.entry(name.clone()).or_default().push(FontStyle {
            weight: face.weight.0,
            italic: face.style != Style::Normal,
        });
    }
    let mut list: Vec<FontFamily> = families
        .into_iter()
        .map(|(name, mut styles)| {
            styles.sort_by_key(|style| (style.weight, style.italic));
            styles.dedup();
            FontFamily { name, styles }
        })
        .collect();
    list.sort_by_key(|family| family.name.to_lowercase());
    list
}

/// Lays `text` out in `style`. An empty string has no outline and one line.
pub fn layout(db: &Database, text: &str, style: &TextStyle) -> Result<TextLayout, TextError> {
    check(style)?;
    let (id, substituted) = find_face(db, style).ok_or(TextError::NoFonts)?;
    let font_family = if substituted {
        db.face(id)
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .unwrap_or_default()
    } else {
        style.font_family.clone()
    };
    db.with_face_data(id, |data, index| {
        let face = rustybuzz::Face::from_slice(data, index)
            .ok_or_else(|| TextError::Invalid(format!("{font_family} can't be read")))?;
        Ok(TextLayout {
            font_family: font_family.clone(),
            substituted,
            ..set(&face, text, style)
        })
    })
    .unwrap_or(Err(TextError::NoFonts))
}

fn check(style: &TextStyle) -> Result<(), TextError> {
    let finite = [style.height, style.letter_spacing, style.line_spacing]
        .iter()
        .all(|value| value.is_finite());
    if !finite || style.height <= 0.0 {
        return Err(TextError::Invalid("Height must be above zero".into()));
    }
    if style.line_spacing <= 0.0 {
        return Err(TextError::Invalid("Line spacing must be above zero".into()));
    }
    if !(1..=1000).contains(&style.font_weight) {
        return Err(TextError::Invalid("Weight is from 1 to 1000".into()));
    }
    Ok(())
}

/// The face for `style`, and whether it had to stand in for a family that
/// isn't installed.
fn find_face(db: &Database, style: &TextStyle) -> Option<(fontdb::ID, bool)> {
    fn query<'a>(families: &'a [Family<'a>], style: &TextStyle) -> Query<'a> {
        Query {
            families,
            weight: Weight(style.font_weight),
            stretch: Stretch::Normal,
            style: if style.italic {
                Style::Italic
            } else {
                Style::Normal
            },
        }
    }
    let installed = db.faces().any(|face| {
        face.families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(&style.font_family))
    });
    if installed && let Some(id) = db.query(&query(&[Family::Name(&style.font_family)], style)) {
        return Some((id, false));
    }
    let fallback = db
        .query(&query(&[Family::SansSerif], style))
        .or_else(|| db.faces().next().map(|face| face.id))?;
    Some((fallback, true))
}

/// The face's cap height in font units.
fn cap_height(face: &ttf_parser::Face) -> f64 {
    if let Some(cap) = face.capital_height().filter(|cap| *cap > 0) {
        return f64::from(cap);
    }
    face.glyph_index('H')
        .and_then(|glyph| face.glyph_bounding_box(glyph))
        .map(|bounds| f64::from(bounds.y_max))
        .filter(|cap| *cap > 0.0)
        .unwrap_or(FALLBACK_CAP_HEIGHT * f64::from(face.units_per_em()))
}

fn set(face: &rustybuzz::Face, text: &str, style: &TextStyle) -> TextLayout {
    let scale = style.height / cap_height(face);
    let natural =
        f64::from(face.ascender()) - f64::from(face.descender()) + f64::from(face.line_gap());
    let line_height = natural * scale * style.line_spacing;

    let mut outline = Vec::new();
    let mut lines = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let baseline = -(row as f64) * line_height;
        let (mut paths, mut carets, width) = set_line(face, line, scale, style.letter_spacing);
        let shift = match style.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -width / 2.0,
            TextAlign::Right => -width,
        };
        for caret in &mut carets {
            *caret += shift;
        }
        for path in &mut paths {
            offset(path, shift, baseline);
        }
        outline.extend(paths);
        lines.push(TextLine {
            baseline,
            cap_height: style.height,
            carets,
        });
    }
    TextLayout {
        outline,
        lines,
        font_family: String::new(),
        substituted: false,
    }
}

/// One line's outlines on a baseline at y = 0 starting at x = 0, its carets
/// and its width (letter spacing after the last character not counted).
fn set_line(
    face: &rustybuzz::Face,
    line: &str,
    scale: f64,
    letter_spacing: f64,
) -> (Vec<Subpath>, Vec<f64>, f64) {
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(line);
    let glyphs = rustybuzz::shape(face, &[], buffer);

    let mut paths = Vec::new();
    // Where each cluster (a byte offset into `line`) starts.
    let mut cluster_x: BTreeMap<usize, f64> = BTreeMap::new();
    let mut x = 0.0;
    for (info, position) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
        let cluster = info.cluster as usize;
        let starts_cluster = !cluster_x.contains_key(&cluster);
        if starts_cluster {
            if !cluster_x.is_empty() {
                x += letter_spacing;
            }
            cluster_x.insert(cluster, x);
        }
        let origin = (
            x + f64::from(position.x_offset) * scale,
            f64::from(position.y_offset) * scale,
        );
        let mut pen = Pen::new(scale, origin);
        face.outline_glyph(GlyphId(info.glyph_id as u16), &mut pen);
        paths.extend(pen.finish());
        x += f64::from(position.x_advance) * scale;
    }
    let width = x;
    (paths, carets(line, &cluster_x, width), width)
}

/// The caret before each UTF-16 code unit of `line`, then after it. A
/// character inside a cluster (a ligature, say) gets its share of the
/// cluster's width; the second half of a surrogate pair sits with the first.
fn carets(line: &str, cluster_x: &BTreeMap<usize, f64>, width: f64) -> Vec<f64> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut carets = Vec::with_capacity(line.encode_utf16().count() + 1);
    for (i, &(byte, ch)) in chars.iter().enumerate() {
        let (start_byte, start) = cluster_x
            .range(..=byte)
            .next_back()
            .map(|(&b, &x)| (b, x))
            .unwrap_or((0, 0.0));
        let (end_byte, end) = cluster_x
            .range(byte + 1..)
            .next()
            .map(|(&b, &x)| (b, x))
            .unwrap_or((line.len(), width));
        // Characters of this cluster up to this one, and in all.
        let in_cluster = |to: usize| {
            chars[..to]
                .iter()
                .filter(|(b, _)| *b >= start_byte && *b < end_byte)
                .count()
        };
        let before = in_cluster(i);
        let total = in_cluster(chars.len()).max(1);
        let caret = start + (end - start) * before as f64 / total as f64;
        for _ in 0..ch.len_utf16() {
            carets.push(caret);
        }
    }
    carets.push(width);
    carets
}

fn offset(path: &mut Subpath, dx: f64, dy: f64) {
    let shift = |point: &mut Point| {
        point.x += dx;
        point.y += dy;
    };
    shift(&mut path.start);
    for segment in &mut path.segments {
        match segment {
            Segment::Line { to } => shift(to),
            Segment::Quad { control, to } => {
                shift(control);
                shift(to);
            }
            Segment::Cubic {
                control1,
                control2,
                to,
            } => {
                shift(control1);
                shift(control2);
                shift(to);
            }
        }
    }
}

/// Collects a glyph's contours, scaled to mm and moved to the pen position.
struct Pen {
    scale: f64,
    origin: (f64, f64),
    done: Vec<Subpath>,
    current: Option<Subpath>,
}

impl Pen {
    fn new(scale: f64, origin: (f64, f64)) -> Self {
        Pen {
            scale,
            origin,
            done: Vec::new(),
            current: None,
        }
    }

    fn point(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.origin.0 + f64::from(x) * self.scale,
            y: self.origin.1 + f64::from(y) * self.scale,
        }
    }

    fn push(&mut self, segment: Segment) {
        if let Some(current) = &mut self.current {
            current.segments.push(segment);
        }
    }

    fn end(&mut self) {
        if let Some(path) = self.current.take().filter(|path| !path.segments.is_empty()) {
            self.done.push(path);
        }
    }

    fn finish(mut self) -> Vec<Subpath> {
        self.end();
        self.done
    }
}

impl OutlineBuilder for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.end();
        self.current = Some(Subpath {
            start: self.point(x, y),
            segments: Vec::new(),
            closed: false,
        });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        self.push(Segment::Line { to });
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (control, to) = (self.point(x1, y1), self.point(x, y));
        self.push(Segment::Quad { control, to });
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (control1, control2, to) = (self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        self.push(Segment::Cubic {
            control1,
            control2,
            to,
        });
    }

    fn close(&mut self) {
        if let Some(current) = &mut self.current {
            current.closed = true;
        }
        self.end();
    }
}

#[cfg(test)]
mod tests;
