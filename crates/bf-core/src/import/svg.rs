//! SVG import through `usvg`, which resolves CSS, `<use>`, transforms, units
//! and basic shapes, and converts text to outlines.
//!
//! Sizing follows the usual laser-software conventions: width and height with
//! absolute units (mm, cm, in, pt, pc) are honoured exactly, and the `viewBox`
//! maps onto them. Unitless or pixel sizes, or none at all, are read at 96 DPI
//! (Inkscape and the CSS standard) except for files from Adobe Illustrator
//! and Affinity, which use 72.

use std::sync::{Arc, OnceLock};

use usvg::tiny_skia_path::{PathSegment, Point};
use usvg::{Group, Node, Paint, Transform, fontdb, roxmltree};

use super::{
    ImportError, ImportFormat, ImportedDrawing, ImportedPath, ImportedPoint, ImportedSegment,
    ImportedSubpath,
};

/// usvg's pixel: a CSS pixel, 1/96 in.
const USVG_DPI: f64 = 96.0;
const MM_PER_INCH: f64 = 25.4;

/// Reads an SVG (or gzip-compressed SVGZ) file's bytes.
pub fn import_svg(data: &[u8]) -> Result<ImportedDrawing, ImportError> {
    if data.starts_with(&[0x1f, 0x8b]) {
        let text = usvg::decompress_svgz(data).map_err(|e| ImportError::Read(e.to_string()))?;
        return import_svg_str(std::str::from_utf8(&text).map_err(not_utf8)?);
    }
    import_svg_str(std::str::from_utf8(data).map_err(not_utf8)?)
}

fn not_utf8(_: std::str::Utf8Error) -> ImportError {
    invalid("the file is not UTF-8 text")
}

fn invalid(reason: impl ToString) -> ImportError {
    ImportError::Invalid {
        format: ImportFormat::Svg,
        reason: reason.to_string(),
    }
}

/// Reads SVG markup, e.g. pasted from the clipboard.
pub fn import_svg_str(text: &str) -> Result<ImportedDrawing, ImportError> {
    let xml = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(invalid)?;
    let root = xml.root_element();
    if root.tag_name().name() != "svg" {
        return Err(invalid("the root element is not <svg>"));
    }
    let scale = Scale::of(text, root);

    let mut options = usvg::Options::default();
    // Loading system fonts takes a moment, so only files with text pay for it.
    if text.contains("<text") {
        options.fontdb = system_fonts();
    }
    let tree = usvg::Tree::from_xmltree(&xml, &options).map_err(invalid)?;

    let mm_per_px = MM_PER_INCH / scale.dpi.unwrap_or(USVG_DPI);
    let size = tree.size();
    let mut collector = Collector {
        mm_per_px,
        page_height: f64::from(size.height()),
        paths: Vec::new(),
        segments: 0,
        skipped: 0,
    };
    collector.group(tree.root());
    if collector.paths.is_empty() {
        return Err(ImportError::Empty(ImportFormat::Svg));
    }
    Ok(ImportedDrawing {
        format: ImportFormat::Svg,
        width: round(f64::from(size.width()) * mm_per_px),
        height: round(f64::from(size.height()) * mm_per_px),
        paths: collector.paths,
        segments: collector.segments,
        dpi: scale.dpi,
        dpi_reason: scale.reason.map(str::to_string),
        units: None,
        skipped: collector.skipped,
    })
}

fn system_fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            set_generic_families(&mut db);
            Arc::new(db)
        })
        .clone()
}

/// Points the generic families (`sans-serif`, ...) at fonts that are
/// installed. fontdb defaults them to the Windows/macOS fonts (Arial, Times
/// New Roman, Courier New), so on a Linux system without those, text set in a
/// generic family would import as nothing. The fallbacks are metric-compatible
/// where possible (Liberation), then common Linux defaults.
fn set_generic_families(db: &mut fontdb::Database) {
    const SANS: &[&str] = &[
        "Arial",
        "Liberation Sans",
        "Arimo",
        "DejaVu Sans",
        "Noto Sans",
        "Ubuntu",
        "Cantarell",
    ];
    const SERIF: &[&str] = &[
        "Times New Roman",
        "Liberation Serif",
        "Tinos",
        "DejaVu Serif",
        "Noto Serif",
    ];
    const MONO: &[&str] = &[
        "Courier New",
        "Liberation Mono",
        "Cousine",
        "DejaVu Sans Mono",
        "Noto Sans Mono",
        "Ubuntu Mono",
    ];
    let installed = |names: &[&str]| {
        names
            .iter()
            .find(|name| {
                db.faces()
                    .any(|face| face.families.iter().any(|(family, _)| family == *name))
            })
            .map(|name| name.to_string())
    };
    let (sans, serif, mono) = (installed(SANS), installed(SERIF), installed(MONO));
    if let Some(family) = sans {
        db.set_sans_serif_family(family);
    }
    if let Some(family) = serif {
        db.set_serif_family(family);
    }
    if let Some(family) = mono {
        db.set_monospace_family(family);
    }
}

/// How the file's user units map to real size.
struct Scale {
    /// None when width and height have absolute units.
    dpi: Option<f64>,
    reason: Option<&'static str>,
}

impl Scale {
    fn of(text: &str, root: roxmltree::Node) -> Scale {
        let absolute = |name| root.attribute(name).is_some_and(has_absolute_unit);
        if absolute("width") && absolute("height") {
            return Scale {
                dpi: None,
                reason: None,
            };
        }
        let (dpi, reason) = if made_by_illustrator(text) {
            (72.0, "Adobe Illustrator")
        } else if root
            .namespaces()
            .any(|ns| ns.uri() == "http://www.serif.com/")
        {
            (72.0, "Affinity")
        } else {
            (96.0, "CSS pixels")
        };
        Scale {
            dpi: Some(dpi),
            reason: Some(reason),
        }
    }
}

fn has_absolute_unit(length: &str) -> bool {
    let length = length.trim().to_ascii_lowercase();
    ["mm", "cm", "in", "pt", "pc", "q"]
        .iter()
        .any(|unit| length.ends_with(unit))
}

/// Illustrator writes a generator comment before the root element.
fn made_by_illustrator(text: &str) -> bool {
    let head = text.get(..text.len().min(4096)).unwrap_or(text);
    head.contains("Generator: Adobe Illustrator")
}

struct Collector {
    mm_per_px: f64,
    page_height: f64,
    paths: Vec<ImportedPath>,
    segments: u32,
    skipped: u32,
}

impl Collector {
    fn group(&mut self, group: &Group) {
        for node in group.children() {
            match node {
                Node::Group(group) => self.group(group),
                Node::Path(path) => self.path(path),
                Node::Text(text) => self.group(text.flattened()),
                Node::Image(_) => self.skipped += 1,
            }
        }
    }

    fn path(&mut self, path: &usvg::Path) {
        // Unpainted shapes (an artboard background, say) are invisible.
        let paint = path
            .stroke()
            .map(|stroke| stroke.paint())
            .or_else(|| path.fill().map(|fill| fill.paint()));
        let Some(paint) = paint.filter(|_| path.is_visible()) else {
            return;
        };
        let color = paint_color(paint);
        let transform = path.abs_transform();
        let at = |point: Point| self.point(transform, point);

        let mut subpaths = Vec::new();
        let mut current: Option<ImportedSubpath> = None;
        for segment in path.data().segments() {
            let segment = match segment {
                PathSegment::MoveTo(point) => {
                    subpaths.extend(current.take());
                    current = Some(ImportedSubpath {
                        start: at(point),
                        segments: Vec::new(),
                        closed: false,
                    });
                    continue;
                }
                PathSegment::Close => {
                    if let Some(mut subpath) = current.take() {
                        subpath.closed = true;
                        subpaths.push(subpath);
                    }
                    continue;
                }
                PathSegment::LineTo(to) => ImportedSegment::Line { to: at(to) },
                PathSegment::QuadTo(control, to) => ImportedSegment::Quad {
                    control: at(control),
                    to: at(to),
                },
                PathSegment::CubicTo(control1, control2, to) => ImportedSegment::Cubic {
                    control1: at(control1),
                    control2: at(control2),
                    to: at(to),
                },
            };
            // A segment after a close starts from where the closed subpath began.
            let subpath = current.get_or_insert_with(|| ImportedSubpath {
                start: subpaths
                    .last()
                    .map_or(ImportedPoint { x: 0.0, y: 0.0 }, |s: &ImportedSubpath| {
                        s.start
                    }),
                segments: Vec::new(),
                closed: false,
            });
            subpath.segments.push(segment);
        }
        subpaths.extend(current);
        subpaths.retain(|subpath| !subpath.segments.is_empty());
        if subpaths.is_empty() {
            return;
        }
        let count: usize = subpaths.iter().map(|s| s.segments.len()).sum();
        self.segments = self
            .segments
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
        self.paths.push(ImportedPath {
            name: path.id().to_string(),
            color,
            subpaths,
        });
    }

    /// SVG user space (Y-down px) to document space (Y-up mm).
    fn point(&self, transform: Transform, mut point: Point) -> ImportedPoint {
        transform.map_point(&mut point);
        ImportedPoint {
            x: round(f64::from(point.x) * self.mm_per_px),
            y: round((self.page_height - f64::from(point.y)) * self.mm_per_px),
        }
    }
}

/// usvg works in f32, so its round-off is dropped at 0.0001 mm.
fn round(mm: f64) -> f64 {
    (mm * 1e4).round() / 1e4
}

/// A gradient takes its first stop's colour; a pattern is black.
fn paint_color(paint: &Paint) -> String {
    let color = match paint {
        Paint::Color(color) => Some(*color),
        Paint::LinearGradient(gradient) => gradient.stops().first().map(|stop| stop.color()),
        Paint::RadialGradient(gradient) => gradient.stops().first().map(|stop| stop.color()),
        Paint::Pattern(_) => None,
    }
    .unwrap_or(usvg::Color::black());
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(body: &str, attrs: &str) -> String {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attrs}>{body}</svg>"#)
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn millimetre_page_maps_the_view_box_and_flips_y() {
        let drawing = import_svg_str(&svg(
            r#"<path d="M 10 0 L 20 0" stroke="red"/>"#,
            r#"width="100mm" height="50mm" viewBox="0 0 200 100""#,
        ))
        .unwrap();
        assert!(approx(drawing.width, 100.0) && approx(drawing.height, 50.0));
        assert_eq!(drawing.dpi, None);
        let subpath = &drawing.paths[0].subpaths[0];
        assert!(approx(subpath.start.x, 5.0) && approx(subpath.start.y, 50.0));
        assert_eq!(drawing.paths[0].color, "#ff0000");
    }

    #[test]
    fn unitless_sizes_read_at_96_dpi() {
        let drawing = import_svg_str(&svg(
            r#"<rect width="96" height="48" fill="blue"/>"#,
            r#"width="96" height="48""#,
        ))
        .unwrap();
        assert!(approx(drawing.width, 25.4));
        assert_eq!(drawing.dpi, Some(96.0));
        assert_eq!(drawing.paths[0].color, "#0000ff");
    }

    #[test]
    fn illustrator_files_read_at_72_dpi() {
        let text = format!(
            "<!-- Generator: Adobe Illustrator 28.0.0 -->{}",
            svg(
                r#"<rect width="72" height="72"/>"#,
                r#"viewBox="0 0 72 72""#
            )
        );
        let drawing = import_svg_str(&text).unwrap();
        assert!(approx(drawing.width, 25.4));
        assert_eq!(drawing.dpi_reason.as_deref(), Some("Adobe Illustrator"));
    }

    #[test]
    fn stroke_colour_wins_over_fill() {
        let drawing = import_svg_str(&svg(
            r##"<circle r="5" cx="5" cy="5" fill="#00ff00" stroke="#123456"/><circle r="5" cx="5" cy="5" fill="lime"/>"##,
            r#"width="10mm" height="10mm""#,
        ))
        .unwrap();
        let colors: Vec<_> = drawing.paths.iter().map(|p| p.color.as_str()).collect();
        assert_eq!(colors, ["#123456", "#00ff00"]);
    }

    #[test]
    fn hidden_shapes_and_images_are_left_out() {
        let drawing = import_svg_str(&svg(
            r#"<rect width="1" height="1" visibility="hidden"/><rect width="1" height="1" display="none"/>
               <image width="1" height="1" href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="/>
               <rect width="2" height="2" stroke="black"/>"#,
            r#"width="10mm" height="10mm""#,
        ))
        .unwrap();
        assert_eq!(drawing.paths.len(), 1);
        assert_eq!(drawing.skipped, 1);
    }

    #[test]
    fn rejects_non_svg_and_empty_documents() {
        assert!(matches!(
            import_svg_str("<html/>"),
            Err(ImportError::Invalid { .. })
        ));
        assert!(matches!(
            import_svg_str("not xml"),
            Err(ImportError::Invalid { .. })
        ));
        assert_eq!(
            import_svg_str(&svg("", r#"width="1mm" height="1mm""#)),
            Err(ImportError::Empty(ImportFormat::Svg))
        );
    }

    #[test]
    fn segments_after_close_restart_at_the_subpath_start() {
        let drawing = import_svg_str(&svg(
            r#"<path d="M 0 0 L 10 0 L 10 10 Z L 0 10" stroke="black"/>"#,
            r#"width="10" height="10" viewBox="0 0 10 10""#,
        ))
        .unwrap();
        let subpaths = &drawing.paths[0].subpaths;
        assert_eq!(subpaths.len(), 2);
        assert!(subpaths[0].closed && !subpaths[1].closed);
        assert_eq!(subpaths[1].start, subpaths[0].start);
    }
}
