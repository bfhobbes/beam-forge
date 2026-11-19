//! DXF import through the `dxf` crate.
//!
//! Reads model-space LINE, LWPOLYLINE and POLYLINE (bulges included), ARC,
//! CIRCLE, ELLIPSE and SPLINE, expanding INSERTs of blocks. TEXT and MTEXT
//! become glyph outlines ([`lettering`]), one path per entity, left out of
//! joining. Units come from
//! `$INSUNITS`; when a file doesn't set them the caller has to choose (see
//! [`ImportError::NeedsUnits`]). Loose pieces whose ends meet within
//! [`JOIN_TOLERANCE_MM`] are joined into chains and closed where they come
//! back to their start, since CAD exports often write a rectangle as four
//! separate lines.
//!
//! Each chain becomes one path named after its DXF layer, coloured by its
//! true colour or ACI index (resolving BYLAYER and BYBLOCK), so the UI maps
//! colours to cut layers just as for SVG. The drawing's bounding box becomes
//! the page: its bottom-left corner is the origin.

mod aci;
mod lettering;
mod spline;

use std::borrow::Cow;
use std::f64::consts::TAU;
use std::io::Cursor;

use dxf::entities::{Entity, EntityType};
use dxf::enums::Units;
use dxf::{Drawing, Point as DxfPoint, Vector};
use fontdb::Database;

use super::curve::{Affine, Bounds, Point, arc, distance, pt};
use super::join::{Piece, join};
use super::{
    ImportError, ImportFormat, ImportedDrawing, ImportedPath, ImportedSegment, ImportedSubpath,
    LengthUnit,
};
use aci::aci_hex;
use lettering::{Fonts, Lettering};
use spline::{Spline, spline_to_subpath, through_points};

/// Ends closer than this (in mm) count as the same point.
pub const JOIN_TOLERANCE_MM: f64 = 0.01;

/// Blocks inside blocks deeper than this are left out, which also stops a
/// block that inserts itself.
const MAX_BLOCK_DEPTH: usize = 16;

/// Reads a DXF file's bytes (ASCII or binary). `units` is used when the file
/// doesn't say what its units are, and ignored when it does. Text is drawn
/// with `fonts` (the app passes [`crate::text::system_fonts`]).
pub fn import_dxf(
    data: &[u8],
    units: Option<LengthUnit>,
    fonts: &Database,
) -> Result<ImportedDrawing, ImportError> {
    let data = prepare(data);
    let drawing = Drawing::load(&mut Cursor::new(&*data)).map_err(|e| ImportError::Invalid {
        format: ImportFormat::Dxf,
        reason: e.to_string(),
    })?;

    let mut reader = Reader {
        drawing: &drawing,
        fonts: Fonts::new(fonts, &drawing),
        pieces: Vec::new(),
        texts: Vec::new(),
        skipped: 0,
    };
    let top = Context {
        transform: Affine::IDENTITY,
        layer: None,
        block_color: aci_hex(7),
        depth: 0,
    };
    for entity in drawing.entities() {
        if !entity.common.is_in_paper_space {
            reader.entity(entity, &top);
        }
    }
    let mut pieces = reader.pieces;
    let mut texts = reader.texts;
    if pieces.is_empty() && texts.is_empty() {
        return Err(ImportError::Empty(ImportFormat::Dxf));
    }

    let (mm_per_unit, unit_name) = match file_units(drawing.header.default_drawing_units) {
        Some(file) => file,
        None => match units {
            Some(LengthUnit::Millimeters) => (1.0, "millimetres"),
            Some(LengthUnit::Inches) => (25.4, "inches"),
            None => {
                let raw = bounds(&pieces, &texts);
                return Err(ImportError::NeedsUnits {
                    width: raw.width(),
                    height: raw.height(),
                });
            }
        },
    };
    let scale = Affine::scale(mm_per_unit, mm_per_unit);
    let subpaths = pieces
        .iter_mut()
        .map(|piece| &mut piece.subpath)
        .chain(texts.iter_mut().flat_map(|text| &mut text.subpaths));
    for subpath in subpaths {
        scale.apply_subpath(subpath);
    }

    let pieces = join(pieces, JOIN_TOLERANCE_MM);
    let page = bounds(&pieces, &texts);
    if page.is_empty() {
        return Err(ImportError::Empty(ImportFormat::Dxf));
    }
    let to_page = Affine::translate(-page.0[0], -page.0[1]);
    let paths: Vec<ImportedPath> = pieces
        .into_iter()
        .map(|piece| {
            let (name, color) = piece.key;
            ImportedPath {
                name,
                color,
                subpaths: vec![piece.subpath],
            }
        })
        .chain(texts)
        .map(|mut path| {
            for subpath in &mut path.subpaths {
                to_page.apply_subpath(subpath);
                round_subpath(subpath);
            }
            path
        })
        .collect();
    let segments = paths
        .iter()
        .flat_map(|path| &path.subpaths)
        .fold(0u32, |total, subpath| {
            total.saturating_add(u32::try_from(subpath.segments.len()).unwrap_or(u32::MAX))
        });
    Ok(ImportedDrawing {
        format: ImportFormat::Dxf,
        width: round(page.width()),
        height: round(page.height()),
        paths,
        segments,
        dpi: None,
        dpi_reason: None,
        units: Some(unit_name.to_string()),
        skipped: reader.skipped,
        substituted_fonts: reader.fonts.substitutions,
    })
}

/// Adjusts a text DXF's bytes for two gaps in the `dxf` crate (binary files
/// are passed through):
///
/// - Its `\U+XXXX` decoding, applied to files older than R2007, takes the
///   six characters after any backslash as one escape and drops them when
///   the string ends first, so MTEXT's `AB\PCD` reads back as `AB`. Each
///   backslash that doesn't start a `\U+XXXX` escape is written as
///   `\U+005C`, which the crate turns back into a backslash.
/// - It defaults an MTEXT's reference width (group 41) to 1, but a file
///   that leaves it out means "don't wrap" (ezdxf does this). Each MTEXT
///   gets a leading `41 = 0`, which the entity's own 41, if any, overrides.
fn prepare(data: &[u8]) -> Cow<'_, [u8]> {
    const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF";
    if data.starts_with(BINARY_SENTINEL) {
        return Cow::Borrowed(data);
    }
    let lines: Vec<&[u8]> = data.split_inclusive(|&b| b == b'\n').collect();
    let trimmed = |line: &[u8]| String::from_utf8_lossy(line).trim().to_ascii_uppercase();
    // $ACADVER's value is the second line after the variable's name. AC1021
    // is R2007, whose files are UTF-8 and read without the escape.
    let escaped = lines
        .iter()
        .position(|line| trimmed(line) == "$ACADVER")
        .and_then(|at| lines.get(at + 2))
        .is_none_or(|version| trimmed(version).as_str() < "AC1021");
    let protect = escaped && data.contains(&b'\\');
    let mtext = |i: usize| i > 0 && trimmed(lines[i]) == "MTEXT" && trimmed(lines[i - 1]) == "0";
    if !protect && !(0..lines.len()).any(mtext) {
        return Cow::Borrowed(data);
    }

    let unicode_escape = |rest: &[u8]| {
        rest.len() >= 7
            && rest[1].eq_ignore_ascii_case(&b'u')
            && rest[2] == b'+'
            && rest[3..7].iter().all(u8::is_ascii_hexdigit)
    };
    let mut out = Vec::with_capacity(data.len() + 256);
    for (i, line) in lines.iter().enumerate() {
        if protect {
            for (j, &byte) in line.iter().enumerate() {
                if byte == b'\\' && !unicode_escape(&line[j..]) {
                    out.extend_from_slice(b"\\U+005C");
                } else {
                    out.push(byte);
                }
            }
        } else {
            out.extend_from_slice(line);
        }
        if mtext(i) {
            let ending: &[u8] = if line.ends_with(b"\r\n") {
                b"\r\n"
            } else {
                b"\n"
            };
            for part in [&b" 41"[..], ending, b"0.0", ending] {
                out.extend_from_slice(part);
            }
        }
    }
    Cow::Owned(out)
}

/// Millimetres per drawing unit and the unit's name, or None for unitless.
fn file_units(units: Units) -> Option<(f64, &'static str)> {
    Some(match units {
        Units::Unitless => return None,
        Units::Inches => (25.4, "inches"),
        Units::Feet => (304.8, "feet"),
        Units::Miles => (1_609_344.0, "miles"),
        Units::Millimeters => (1.0, "millimetres"),
        Units::Centimeters => (10.0, "centimetres"),
        Units::Meters => (1000.0, "metres"),
        Units::Kilometers => (1e6, "kilometres"),
        Units::Microinches => (2.54e-5, "microinches"),
        Units::Mils => (0.0254, "mils"),
        Units::Yards => (914.4, "yards"),
        Units::Angstroms => (1e-7, "ångströms"),
        Units::Nanometers => (1e-6, "nanometres"),
        Units::Microns => (1e-3, "microns"),
        Units::Decimeters => (100.0, "decimetres"),
        Units::Decameters => (1e4, "decametres"),
        Units::Hectometers => (1e5, "hectometres"),
        Units::Gigameters => (1e12, "gigametres"),
        Units::AstronomicalUnits => (1.495_978_707e14, "astronomical units"),
        Units::LightYears => (9.460_730_472_580_8e18, "light years"),
        Units::Parsecs => (3.085_677_581_491_367e19, "parsecs"),
        Units::USSurveyFeet => (1200.0 / 3937.0 * 1000.0, "US survey feet"),
        Units::USSurveyInch => (1000.0 / 39.37, "US survey inches"),
        Units::USSurveyYard => (3600.0 / 3937.0 * 1000.0, "US survey yards"),
        Units::USSurveyMile => (6_336_000.0 / 3937.0 * 1000.0, "US survey miles"),
    })
}

fn bounds(pieces: &[Piece<(String, String)>], texts: &[ImportedPath]) -> Bounds {
    let mut bounds = Bounds::EMPTY;
    for piece in pieces {
        bounds.add_subpath(&piece.subpath);
    }
    for subpath in texts.iter().flat_map(|text| &text.subpaths) {
        bounds.add_subpath(subpath);
    }
    bounds
}

/// Drops float noise below 0.0001 mm.
fn round(mm: f64) -> f64 {
    let rounded = (mm * 1e4).round() / 1e4;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn round_subpath(subpath: &mut ImportedSubpath) {
    let r = |p: Point| pt(round(p.x), round(p.y));
    subpath.start = r(subpath.start);
    for segment in &mut subpath.segments {
        *segment = match *segment {
            ImportedSegment::Line { to } => ImportedSegment::Line { to: r(to) },
            ImportedSegment::Quad { control, to } => ImportedSegment::Quad {
                control: r(control),
                to: r(to),
            },
            ImportedSegment::Cubic {
                control1,
                control2,
                to,
            } => ImportedSegment::Cubic {
                control1: r(control1),
                control2: r(control2),
                to: r(to),
            },
        };
    }
}

/// Where an entity is being drawn: at the top level or inside a block.
struct Context {
    /// Block coordinates to world coordinates.
    transform: Affine,
    /// The INSERT's layer, which entities on layer "0" inside it take.
    layer: Option<String>,
    /// The colour BYBLOCK entities take.
    block_color: String,
    depth: usize,
}

struct Reader<'a> {
    drawing: &'a Drawing,
    fonts: Fonts<'a>,
    pieces: Vec<Piece<(String, String)>>,
    /// One path per text entity, kept out of joining.
    texts: Vec<ImportedPath>,
    skipped: u32,
}

impl Reader<'_> {
    fn entity(&mut self, entity: &Entity, context: &Context) {
        let common = &entity.common;
        let layer_name = match &context.layer {
            Some(insert_layer) if common.layer == "0" => insert_layer.clone(),
            _ => common.layer.clone(),
        };
        let layer = self.drawing.layers().find(|l| l.name == layer_name);
        let hidden = !common.is_visible
            || layer.is_some_and(|l| !l.is_layer_on || !l.is_layer_plotted)
            || layer_name.eq_ignore_ascii_case("defpoints");
        if hidden {
            return;
        }
        let color = if common.color_24_bit != 0 {
            let rgb = common.color_24_bit & 0xff_ffff;
            format!("#{rgb:06x}")
        } else if common.color.is_by_block() {
            context.block_color.clone()
        } else if let Some(index) = common.color.index() {
            aci_hex(index)
        } else {
            // BYLAYER, or anything else unusual.
            layer
                .and_then(|l| l.color.index())
                .map_or_else(|| aci_hex(7), aci_hex)
        };

        let lettering = match &entity.specific {
            EntityType::Text(text) => Some(self.fonts.text(text)),
            EntityType::MText(text) => Some(self.fonts.mtext(text)),
            _ => None,
        };
        if let Some(lettering) = lettering {
            match lettering {
                Lettering::Empty => {}
                Lettering::Skipped => self.skipped += 1,
                Lettering::Drawn(mut subpaths) => {
                    for subpath in &mut subpaths {
                        context.transform.apply_subpath(subpath);
                    }
                    if !subpaths.is_empty() {
                        self.texts.push(ImportedPath {
                            name: layer_name,
                            color,
                            subpaths,
                        });
                    }
                }
            }
            return;
        }

        let mut subpaths = Vec::new();
        let handled = match &entity.specific {
            EntityType::Line(line) => {
                subpaths.push(ImportedSubpath {
                    start: xy(&line.p1),
                    segments: vec![ImportedSegment::Line { to: xy(&line.p2) }],
                    closed: false,
                });
                true
            }
            EntityType::Circle(circle) => ocs(&circle.normal).is_some_and(|o| {
                let r = circle.radius;
                if r > 0.0 {
                    let mut c = arc(xy(&circle.center), pt(r, 0.0), pt(0.0, r), 0.0, TAU);
                    o.apply_subpath(&mut c);
                    subpaths.push(c);
                }
                true
            }),
            EntityType::Arc(a) => ocs(&a.normal).is_some_and(|o| {
                let r = a.radius;
                let start = a.start_angle.to_radians();
                let mut sweep = (a.end_angle - a.start_angle).rem_euclid(360.0).to_radians();
                if sweep == 0.0 {
                    sweep = TAU;
                }
                if r > 0.0 {
                    let mut c = arc(xy(&a.center), pt(r, 0.0), pt(0.0, r), start, sweep);
                    // A full-circle ARC is still an arc: leave it open for joining.
                    c.closed = false;
                    o.apply_subpath(&mut c);
                    subpaths.push(c);
                }
                true
            }),
            EntityType::Ellipse(e) => ellipse(e).map(|c| subpaths.push(c)).is_some(),
            EntityType::LwPolyline(poly) => ocs(&poly.extrusion_direction).is_some_and(|o| {
                let vertices: Vec<(Point, f64)> = poly
                    .vertices
                    .iter()
                    .map(|v| (pt(v.x, v.y), v.bulge))
                    .collect();
                if let Some(mut c) = polyline(&vertices, poly.flags & 1 != 0) {
                    o.apply_subpath(&mut c);
                    subpaths.push(c);
                }
                true
            }),
            EntityType::Polyline(poly) => {
                // Polyface and polygon meshes are 3D surfaces.
                let mesh = poly.flags & (16 | 64) != 0;
                let three_d = poly.flags & 8 != 0;
                let transform = if three_d {
                    Some(Affine::IDENTITY)
                } else {
                    ocs(&poly.normal)
                };
                match transform.filter(|_| !mesh) {
                    Some(o) => {
                        let vertices: Vec<(Point, f64)> = poly
                            .vertices()
                            // Spline frame control points aren't on the curve.
                            .filter(|v| v.flags & 16 == 0)
                            .map(|v| (xy(&v.location), if three_d { 0.0 } else { v.bulge }))
                            .collect();
                        if let Some(mut c) = polyline(&vertices, poly.flags & 1 != 0) {
                            o.apply_subpath(&mut c);
                            subpaths.push(c);
                        }
                        true
                    }
                    None => false,
                }
            }
            EntityType::Spline(s) => {
                let control: Vec<Point> = s.control_points.iter().map(xy).collect();
                let curve = if control.is_empty() {
                    let fit: Vec<Point> = s.fit_points.iter().map(xy).collect();
                    through_points(&fit, s.flags & 1 != 0)
                } else {
                    spline_to_subpath(&Spline {
                        degree: usize::try_from(s.degree_of_curve).unwrap_or(0),
                        knots: &s.knot_values,
                        control: &control,
                        weights: &s.weight_values,
                    })
                };
                curve.map(|c| subpaths.push(c)).is_some()
            }
            EntityType::Insert(insert) => {
                self.insert(insert, &layer_name, &color, context);
                true
            }
            _ => false,
        };
        if !handled {
            self.skipped += 1;
            return;
        }
        for mut subpath in subpaths {
            context.transform.apply_subpath(&mut subpath);
            if subpath.segments.is_empty() || degenerate(&subpath) {
                continue;
            }
            self.pieces.push(Piece {
                key: (layer_name.clone(), color.clone()),
                subpath,
            });
        }
    }

    fn insert(
        &mut self,
        insert: &dxf::entities::Insert,
        layer: &str,
        color: &str,
        context: &Context,
    ) {
        if context.depth >= MAX_BLOCK_DEPTH {
            self.skipped += 1;
            return;
        }
        let Some(block) = self.drawing.blocks().find(|b| b.name == insert.name) else {
            self.skipped += 1;
            return;
        };
        let Some(o) = ocs(&insert.extrusion_direction) else {
            self.skipped += 1;
            return;
        };
        let rotation = Affine::rotate(insert.rotation.to_radians());
        let columns = insert.column_count.max(1);
        let rows = insert.row_count.max(1);
        for row in 0..rows {
            for column in 0..columns {
                // Array copies step along the insert's rotated axes.
                let step = rotation.apply(pt(
                    f64::from(column) * insert.column_spacing,
                    f64::from(row) * insert.row_spacing,
                ));
                let location = xy(&insert.location);
                let transform = Affine::translate(-block.base_point.x, -block.base_point.y)
                    .then(Affine::scale(insert.x_scale_factor, insert.y_scale_factor))
                    .then(rotation)
                    .then(Affine::translate(location.x + step.x, location.y + step.y))
                    .then(o)
                    .then(context.transform);
                let inner = Context {
                    transform,
                    layer: Some(layer.to_string()),
                    block_color: color.to_string(),
                    depth: context.depth + 1,
                };
                for entity in &block.entities {
                    self.entity(entity, &inner);
                }
            }
        }
    }
}

fn xy(p: &DxfPoint) -> Point {
    pt(p.x, p.y)
}

/// Object coordinates to world for an extrusion direction straight up or
/// down (down mirrors X, per the DXF arbitrary axis algorithm). None for
/// tilted planes, which a flat laser bed can't show.
fn ocs(normal: &Vector) -> Option<Affine> {
    let length = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
    if length == 0.0 {
        return Some(Affine::IDENTITY);
    }
    let z = normal.z / length;
    if z > 1.0 - 1e-9 {
        Some(Affine::IDENTITY)
    } else if z < -1.0 + 1e-9 {
        Some(Affine::scale(-1.0, 1.0))
    } else {
        None
    }
}

fn ellipse(e: &dxf::entities::Ellipse) -> Option<ImportedSubpath> {
    let up = ocs(&e.normal)?;
    let major = pt(e.major_axis.x, e.major_axis.y);
    // The minor axis is the normal × the major axis, scaled by the ratio.
    let minor = if up.a < 0.0 {
        pt(major.y * e.minor_axis_ratio, -major.x * e.minor_axis_ratio)
    } else {
        pt(-major.y * e.minor_axis_ratio, major.x * e.minor_axis_ratio)
    };
    if major.x.hypot(major.y) == 0.0 || e.minor_axis_ratio == 0.0 {
        return None;
    }
    let mut sweep = (e.end_parameter - e.start_parameter).rem_euclid(TAU);
    if sweep < 1e-12 {
        sweep = TAU;
    }
    Some(arc(xy(&e.center), major, minor, e.start_parameter, sweep))
}

/// A polyline from vertices and their bulges (the tangent of a quarter of
/// the arc's angle to the next vertex; positive runs anticlockwise).
fn polyline(vertices: &[(Point, f64)], closed: bool) -> Option<ImportedSubpath> {
    let (first, _) = *vertices.first()?;
    let mut segments = Vec::new();
    let count = if closed {
        vertices.len()
    } else {
        vertices.len() - 1
    };
    for i in 0..count {
        let (from, bulge) = vertices[i];
        let (to, _) = vertices[(i + 1) % vertices.len()];
        if distance(from, to) < 1e-12 {
            continue;
        }
        if bulge.abs() < 1e-12 {
            segments.push(ImportedSegment::Line { to });
            continue;
        }
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let offset = (1.0 - bulge * bulge) / (4.0 * bulge);
        let center = pt(
            from.x + dx / 2.0 - dy * offset,
            from.y + dy / 2.0 + dx * offset,
        );
        let r = distance(center, from);
        let start = (from.y - center.y).atan2(from.x - center.x);
        let sweep = 4.0 * bulge.atan();
        let mut piece = arc(center, pt(r, 0.0), pt(0.0, r), start, sweep);
        // Land exactly on the next vertex.
        if let Some(
            ImportedSegment::Cubic { to: end, .. }
            | ImportedSegment::Line { to: end }
            | ImportedSegment::Quad { to: end, .. },
        ) = piece.segments.last_mut()
        {
            *end = to;
        }
        segments.append(&mut piece.segments);
    }
    if segments.is_empty() {
        return None;
    }
    Some(ImportedSubpath {
        start: first,
        segments,
        closed: closed && (vertices.len() > 2 || vertices.iter().any(|(_, b)| b.abs() >= 1e-12)),
    })
}

/// A piece with no extent, e.g. a zero-length line.
fn degenerate(subpath: &ImportedSubpath) -> bool {
    let mut bounds = Bounds::EMPTY;
    bounds.add_subpath(subpath);
    bounds.width() < 1e-9 && bounds.height() < 1e-9
}

#[cfg(test)]
mod tests;
