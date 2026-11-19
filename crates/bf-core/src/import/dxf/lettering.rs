//! TEXT and MTEXT as outlines, drawn with the text module in the font their
//! text style names, or in the system sans when that isn't installed (SHX
//! fonts never are).
//!
//! A style's font is looked up by the TrueType family in its ACAD XDATA,
//! then by its `.ttf`/`.otf`/`.ttc` file name among the installed faces, then
//! by that file's stem as a family name. Anything else is drawn in the
//! fallback and reported as a [`FontSubstitution`].
//!
//! Outlines are built in the entity's own units; the importer scales them
//! with everything else. Per-run MTEXT formatting (fonts, heights, colours)
//! is dropped: the whole entity is set in its style.

use std::collections::HashMap;

use dxf::Drawing;
use dxf::XDataItem;
use dxf::entities::{MText, Text};
use dxf::enums::{AttachmentPoint, HorizontalTextJustification, VerticalTextJustification};
use dxf::tables::Style;
use fontdb::{Database, Source};

use super::{ocs, xy};
use crate::import::curve::{Affine, Bounds};
use crate::import::{FontSubstitution, ImportedPoint, ImportedSegment, ImportedSubpath};
use crate::project::{Point, Segment, Subpath};
use crate::text::{self, TextAlign, TextStyle};

/// MTEXT baselines sit this many text heights apart at a spacing factor of 1.
const MTEXT_LINE_PITCH: f64 = 5.0 / 3.0;

/// What a text entity came to.
pub(super) enum Lettering {
    /// Nothing to draw, e.g. an empty string. Not counted as skipped.
    Empty,
    /// Couldn't be drawn: no fonts, a tilted plane, a zero height.
    Skipped,
    /// Every glyph's contours, in the entity's block (or world) coordinates.
    Drawn(Vec<ImportedSubpath>),
}

/// A text style's font, resolved once per style.
#[derive(Debug, Clone)]
struct Font {
    family: String,
    weight: u16,
    italic: bool,
    /// What the style asks for, named in the notice if it isn't installed.
    requested: String,
    /// The style's fixed height, or 0 when entities set their own.
    height: f64,
}

/// Fonts for a drawing's text styles and the substitutions made so far.
pub(super) struct Fonts<'a> {
    db: &'a Database,
    drawing: &'a Drawing,
    styles: HashMap<String, Font>,
    pub substitutions: Vec<FontSubstitution>,
}

impl<'a> Fonts<'a> {
    pub fn new(db: &'a Database, drawing: &'a Drawing) -> Self {
        Fonts {
            db,
            drawing,
            styles: HashMap::new(),
            substitutions: Vec::new(),
        }
    }

    /// A TEXT entity's outline.
    pub fn text(&mut self, entity: &Text) -> Lettering {
        let value = decode(&entity.value);
        if value.trim().is_empty() {
            return Lettering::Empty;
        }
        let Some(plane) = ocs(&entity.normal) else {
            return Lettering::Skipped;
        };
        let font = self.font(&entity.text_style_name);
        let height = if entity.text_height > 0.0 {
            entity.text_height
        } else {
            font.height
        };
        let Some((mut outline, advance)) = self.line(&font, &value, height, TextAlign::Left) else {
            return Lettering::Skipped;
        };

        // Width factor and oblique angle slant and stretch the glyphs about
        // the start of the baseline.
        let width_factor = if entity.relative_x_scale_factor > 0.0 {
            entity.relative_x_scale_factor
        } else {
            1.0
        };
        let shape = Affine {
            a: width_factor,
            c: entity.oblique_angle.to_radians().tan(),
            ..Affine::IDENTITY
        };
        transform(&mut outline, shape);
        let width = advance * width_factor;
        let bounds = bounds(&outline);

        let mirror = Affine::scale(
            if entity.text_generation_flags & 2 != 0 {
                -1.0
            } else {
                1.0
            },
            if entity.text_generation_flags & 4 != 0 {
                -1.0
            } else {
                1.0
            },
        );
        let first = xy(&entity.location);
        let second = xy(&entity.second_alignment_point);
        let horizontal = entity.horizontal_text_justification;
        let vertical = entity.vertical_text_justification;
        let between = (second.x - first.x, second.y - first.y);
        let span = between.0.hypot(between.1);

        let place = match horizontal {
            HorizontalTextJustification::Aligned | HorizontalTextJustification::Fit
                if span > 0.0 && width > 0.0 =>
            {
                // The baseline runs from the first point to the second.
                let stretch = span / width;
                let tall = if horizontal == HorizontalTextJustification::Aligned {
                    stretch
                } else {
                    1.0
                };
                mirror
                    .then(Affine::scale(stretch, tall))
                    .then(Affine::rotate(between.1.atan2(between.0)))
                    .then(Affine::translate(first.x, first.y))
            }
            _ => {
                let x = match horizontal {
                    HorizontalTextJustification::Center | HorizontalTextJustification::Middle => {
                        width / 2.0
                    }
                    HorizontalTextJustification::Right => width,
                    _ => 0.0,
                };
                let y = if horizontal == HorizontalTextJustification::Middle {
                    (bounds.0[1] + bounds.0[3]) / 2.0
                } else {
                    match vertical {
                        VerticalTextJustification::Baseline => 0.0,
                        VerticalTextJustification::Bottom => bounds.0[1],
                        VerticalTextJustification::Middle => height / 2.0,
                        VerticalTextJustification::Top => height,
                    }
                };
                let simple = matches!(
                    horizontal,
                    HorizontalTextJustification::Left
                        | HorizontalTextJustification::Aligned
                        | HorizontalTextJustification::Fit
                ) && vertical == VerticalTextJustification::Baseline;
                // The second point is the anchor whenever the text is justified.
                let anchor = if simple { first } else { second };
                Affine::translate(-x, -y)
                    .then(mirror)
                    .then(Affine::rotate(entity.rotation.to_radians()))
                    .then(Affine::translate(anchor.x, anchor.y))
            }
        };
        transform(&mut outline, place.then(plane));
        Lettering::Drawn(outline)
    }

    /// An MTEXT entity's outline.
    pub fn mtext(&mut self, entity: &MText) -> Lettering {
        let mut raw = entity.extended_text.concat();
        raw.push_str(&entity.text);
        let plain = mtext_plain(&raw);
        if plain.trim().is_empty() {
            return Lettering::Empty;
        }
        let Some(plane) = ocs(&entity.extrusion_direction) else {
            return Lettering::Skipped;
        };
        let font = self.font(&entity.text_style_name);
        let height = if entity.initial_text_height > 0.0 {
            entity.initial_text_height
        } else {
            font.height
        };
        if height <= 0.0 || !height.is_finite() {
            return Lettering::Skipped;
        }

        use AttachmentPoint as A;
        let point = entity.attachment_point;
        let align = match point {
            A::TopLeft | A::MiddleLeft | A::BottomLeft => TextAlign::Left,
            A::TopCenter | A::MiddleCenter | A::BottomCenter => TextAlign::Center,
            A::TopRight | A::MiddleRight | A::BottomRight => TextAlign::Right,
        };
        let mut lines = Vec::new();
        for paragraph in plain.split('\n') {
            if entity.reference_rectangle_width > 0.0 {
                let Some(wrapped) =
                    self.wrap(&font, paragraph, height, entity.reference_rectangle_width)
                else {
                    return Lettering::Skipped;
                };
                lines.extend(wrapped);
            } else {
                lines.push(paragraph.to_string());
            }
        }

        let spacing = if entity.line_spacing_factor > 0.0 {
            entity.line_spacing_factor
        } else {
            1.0
        };
        let pitch = MTEXT_LINE_PITCH * height * spacing;
        let mut outline = Vec::new();
        let mut bottom = 0.0;
        for (row, line) in lines.iter().enumerate() {
            let Some((mut glyphs, _)) = self.line(&font, line, height, align) else {
                return Lettering::Skipped;
            };
            let baseline = -(row as f64) * pitch;
            transform(&mut glyphs, Affine::translate(0.0, baseline));
            let drawn = bounds(&glyphs);
            // The block's bottom is the last line's lowest point.
            bottom = if drawn.is_empty() {
                baseline
            } else {
                drawn.0[1]
            };
            outline.append(&mut glyphs);
        }
        let top = height;
        let y = match point {
            A::TopLeft | A::TopCenter | A::TopRight => top,
            A::MiddleLeft | A::MiddleCenter | A::MiddleRight => (top + bottom) / 2.0,
            A::BottomLeft | A::BottomCenter | A::BottomRight => bottom,
        };

        // The X axis wins when it is given (it defaults to +X); otherwise
        // the rotation, in degrees as LibreCAD, QCAD and ezdxf write it.
        let axis = &entity.x_axis_direction;
        let angle = if (axis.x - 1.0).abs() > 1e-12 || axis.y.abs() > 1e-12 {
            axis.y.atan2(axis.x)
        } else {
            entity.rotation_angle.to_radians()
        };
        let insertion = xy(&entity.insertion_point);
        let place = Affine::translate(0.0, -y)
            .then(plane)
            .then(Affine::rotate(angle))
            .then(Affine::translate(insertion.x, insertion.y));
        transform(&mut outline, place);
        Lettering::Drawn(outline)
    }

    /// `paragraph` broken greedily at spaces into lines no wider than
    /// `width` (a word longer than that gets a line of its own).
    fn wrap(
        &mut self,
        font: &Font,
        paragraph: &str,
        height: f64,
        width: f64,
    ) -> Option<Vec<String>> {
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in paragraph.split(' ') {
            if current.is_empty() {
                current = word.to_string();
                continue;
            }
            let candidate = format!("{current} {word}");
            if self.advance(font, &candidate, height)? <= width {
                current = candidate;
            } else {
                lines.push(std::mem::replace(&mut current, word.to_string()));
            }
        }
        lines.push(current);
        Some(lines)
    }

    fn advance(&mut self, font: &Font, line: &str, height: f64) -> Option<f64> {
        self.line(font, line, height, TextAlign::Left)
            .map(|(_, advance)| advance)
    }

    /// One line laid out on a baseline at y = 0, and its advance width.
    /// None when it can't be set (no fonts at all, or a bad height).
    fn line(
        &mut self,
        font: &Font,
        line: &str,
        height: f64,
        align: TextAlign,
    ) -> Option<(Vec<ImportedSubpath>, f64)> {
        let laid = text::layout(
            self.db,
            &line.replace('\u{a0}', " "),
            &TextStyle {
                font_family: font.family.clone(),
                font_weight: font.weight,
                italic: font.italic,
                height,
                letter_spacing: 0.0,
                line_spacing: 1.0,
                align,
            },
        )
        .ok()?;
        if laid.substituted {
            let substitution = FontSubstitution {
                requested: font.requested.clone(),
                used: laid.font_family.clone(),
            };
            if !self.substitutions.contains(&substitution) {
                self.substitutions.push(substitution);
            }
        }
        let advance = laid
            .lines
            .first()
            .and_then(|line| line.carets.last())
            .copied()
            .unwrap_or(0.0);
        Some((laid.outline.iter().map(imported).collect(), advance))
    }

    fn font(&mut self, style_name: &str) -> Font {
        let name = if style_name.trim().is_empty() {
            "STANDARD"
        } else {
            style_name.trim()
        };
        let key = name.to_lowercase();
        if let Some(font) = self.styles.get(&key) {
            return font.clone();
        }
        let style = self
            .drawing
            .styles()
            .find(|style| style.name.eq_ignore_ascii_case(name));
        let font = resolve(self.db, style, name);
        self.styles.insert(key, font.clone());
        font
    }
}

/// The installed font for a text style, or the name to report when there
/// isn't one (the text module then falls back to the system sans).
fn resolve(db: &Database, style: Option<&Style>, name: &str) -> Font {
    let height = style.map_or(0.0, |style| style.text_height.max(0.0));
    let file = style.map_or("", |style| style.primary_font_file_name.trim());
    let family = style.and_then(acad_family);
    let installed = |family: &str| {
        db.faces().any(|face| {
            face.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(family))
        })
    };
    let font = |family: &str, weight: u16, italic: bool| Font {
        family: family.to_string(),
        weight,
        italic,
        requested: family.to_string(),
        height,
    };

    if let Some(family) = family.as_deref().filter(|family| installed(family)) {
        return font(family, 400, false);
    }
    // Only the file's name counts; CAD files often carry the author's paths.
    let file_name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let lower = file_name.to_lowercase();
    if [".ttf", ".otf", ".ttc"]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        let by_file = db.faces().find(|face| {
            face_file_name(&face.source).is_some_and(|name| name.eq_ignore_ascii_case(file_name))
        });
        if let Some(face) = by_file
            && let Some((family, _)) = face.families.first()
        {
            return font(family, face.weight.0, face.style != fontdb::Style::Normal);
        }
        let stem = &file_name[..file_name.len() - 4];
        if installed(stem) {
            return font(stem, 400, false);
        }
    }
    let requested = family
        .or_else(|| (!file_name.is_empty()).then(|| file_name.to_string()))
        .unwrap_or_else(|| name.to_string());
    font(&requested, 400, false)
}

/// The TrueType family a style's ACAD XDATA names, if any.
fn acad_family(style: &Style) -> Option<String> {
    style
        .x_data
        .iter()
        .filter(|x| x.application_name.eq_ignore_ascii_case("ACAD"))
        .flat_map(|x| &x.items)
        .find_map(|item| match item {
            XDataItem::Str(family) if !family.trim().is_empty() => Some(family.trim().to_string()),
            _ => None,
        })
}

fn face_file_name(source: &Source) -> Option<&str> {
    let path = match source {
        Source::File(path) | Source::SharedFile(path, _) => path,
        Source::Binary(_) => return None,
    };
    path.file_name().and_then(|name| name.to_str())
}

/// A text-module outline as an imported subpath.
pub(super) fn imported(path: &Subpath) -> ImportedSubpath {
    let p = |point: Point| ImportedPoint {
        x: point.x,
        y: point.y,
    };
    ImportedSubpath {
        start: p(path.start),
        segments: path
            .segments
            .iter()
            .map(|segment| match *segment {
                Segment::Line { to } => ImportedSegment::Line { to: p(to) },
                Segment::Quad { control, to } => ImportedSegment::Quad {
                    control: p(control),
                    to: p(to),
                },
                Segment::Cubic {
                    control1,
                    control2,
                    to,
                } => ImportedSegment::Cubic {
                    control1: p(control1),
                    control2: p(control2),
                    to: p(to),
                },
            })
            .collect(),
        closed: path.closed,
    }
}

fn transform(outline: &mut [ImportedSubpath], affine: Affine) {
    for subpath in outline {
        affine.apply_subpath(subpath);
    }
}

fn bounds(outline: &[ImportedSubpath]) -> Bounds {
    let mut bounds = Bounds::EMPTY;
    for subpath in outline {
        bounds.add_subpath(subpath);
    }
    bounds
}

/// TEXT's special codes: `%%d` degree, `%%p` plus-minus, `%%c` diameter,
/// `%%%` percent, `%%nnn` a character by number, `\U+XXXX` a character by
/// code point. `%%u` and `%%o` (underline and overline toggles) are dropped.
pub(super) fn decode(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && chars.get(i + 1) == Some(&'%') && i + 2 < chars.len() {
            let code = chars[i + 2];
            let replacement = match code.to_ascii_lowercase() {
                'd' => Some(Some('°')),
                'p' => Some(Some('±')),
                'c' => Some(Some('Ø')),
                '%' => Some(Some('%')),
                'u' | 'o' => Some(None),
                _ => None,
            };
            if let Some(replacement) = replacement {
                out.extend(replacement);
                i += 3;
                continue;
            }
            let digits: String = chars[i + 2..]
                .iter()
                .take(3)
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if digits.len() == 3
                && let Some(c) = digits.parse().ok().and_then(char::from_u32)
            {
                out.push(c);
                i += 5;
                continue;
            }
        }
        if let Some((c, used)) = unicode_escape(&chars[i..]) {
            out.push(c);
            i += used;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `\U+XXXX` at the start of `chars`: the character and how many chars the
/// escape took.
fn unicode_escape(chars: &[char]) -> Option<(char, usize)> {
    if chars.len() < 7 || chars[0] != '\\' || !matches!(chars[1], 'U' | 'u') || chars[2] != '+' {
        return None;
    }
    let hex: String = chars[3..7].iter().collect();
    let c = u32::from_str_radix(&hex, 16)
        .ok()
        .and_then(char::from_u32)?;
    Some((c, 7))
}

/// MTEXT's string with its formatting stripped: `\P` and `\N` become line
/// breaks, `\~` a non-breaking space (U+00A0), stacked fractions `a/b`,
/// `\\ \{ \}` literals; grouping braces and every other code (fonts,
/// heights, colours, underlines...) are dropped. Then [`decode`]s the
/// result.
pub(super) fn mtext_plain(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' | '}' => i += 1,
            '\\' => {
                if let Some((code, used)) = unicode_escape(&chars[i..]) {
                    out.push(code);
                    i += used;
                    continue;
                }
                let Some(&code) = chars.get(i + 1) else {
                    i += 1;
                    continue;
                };
                i += 2;
                match code {
                    'P' | 'N' => out.push('\n'),
                    '~' => out.push('\u{a0}'),
                    '\\' | '{' | '}' => out.push(code),
                    'S' => {
                        // A stacked fraction up to the next ';'.
                        let end = chars[i..]
                            .iter()
                            .position(|&c| c == ';')
                            .map_or(chars.len(), |p| i + p);
                        for &c in &chars[i..end] {
                            out.push(if matches!(c, '^' | '#') { '/' } else { c });
                        }
                        i = end + 1;
                    }
                    'A' | 'C' | 'c' | 'F' | 'f' | 'H' | 'h' | 'Q' | 'q' | 'T' | 't' | 'W' | 'w'
                    | 'p' => {
                        // A code with a value, up to the next ';'.
                        i = chars[i..]
                            .iter()
                            .position(|&c| c == ';')
                            .map_or(chars.len(), |p| i + p + 1);
                    }
                    // Underline, overline, strike-through toggles and the
                    // like, and anything unknown: drop the code.
                    _ => {}
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    decode(&out)
}
