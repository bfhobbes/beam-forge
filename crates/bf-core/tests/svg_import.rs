//! SVG import against fixtures in the styles Inkscape, Illustrator and
//! Affinity export. Each draws an 80 x 30 mm red rectangle 10 mm in from the
//! bottom-left of a 100 x 50 mm page.

use bf_core::import::svg::{import_svg, import_svg_str};
use bf_core::import::{ImportedDrawing, ImportedPath, ImportedPoint, ImportedSegment};

fn fixture(name: &str) -> ImportedDrawing {
    let path = format!("{}/tests/fixtures/svg/{name}", env!("CARGO_MANIFEST_DIR"));
    import_svg(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn path<'a>(drawing: &'a ImportedDrawing, name: &str) -> &'a ImportedPath {
    drawing
        .paths
        .iter()
        .find(|path| path.name == name)
        .unwrap_or_else(|| panic!("no path {name}"))
}

/// Bounds over every point, control points included: [min x, min y, max x, max y].
fn bounds<'a>(paths: impl IntoIterator<Item = &'a ImportedPath>) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut add = |p: ImportedPoint| {
        b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
    };
    for path in paths {
        for subpath in &path.subpaths {
            add(subpath.start);
            for segment in &subpath.segments {
                match *segment {
                    ImportedSegment::Line { to } => add(to),
                    ImportedSegment::Quad { control, to } => {
                        add(control);
                        add(to);
                    }
                    ImportedSegment::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        add(control1);
                        add(control2);
                        add(to);
                    }
                }
            }
        }
    }
    b
}

#[track_caller]
fn assert_near(actual: [f64; 4], expected: [f64; 4], tolerance: f64) {
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() <= tolerance, "{actual:?} != {expected:?}");
    }
}

const RECT: [f64; 4] = [10.0, 10.0, 90.0, 40.0];

#[test]
fn inkscape_millimetre_page_imports_at_size() {
    let drawing = fixture("inkscape.svg");
    assert_near(
        [0.0, 0.0, drawing.width, drawing.height],
        [0.0, 0.0, 100.0, 50.0],
        1e-4,
    );
    assert_eq!(drawing.dpi, None);
    let rect = path(&drawing, "rect1");
    assert_eq!(rect.color, "#ff0000");
    // The layer's translate(-10,-20) moves the rect from (20,30) to (10,10).
    assert_near(bounds([rect]), RECT, 1e-4);
    let circle = path(&drawing, "path1");
    assert_eq!(circle.color, "#0000ff");
    assert_near(bounds([circle]), [40.0, 15.0, 60.0, 35.0], 1e-4);
}

#[test]
fn illustrator_points_import_at_72_dpi() {
    for name in ["illustrator.svg", "illustrator-legacy.svg"] {
        let drawing = fixture(name);
        assert_eq!(drawing.dpi, Some(72.0), "{name}");
        assert_eq!(drawing.dpi_reason.as_deref(), Some("Adobe Illustrator"));
        assert_near(
            [0.0, 0.0, drawing.width, drawing.height],
            [0.0, 0.0, 100.0, 50.0],
            0.01,
        );
        assert_eq!(
            drawing.paths[0].color, "#ff0000",
            "{name}: CSS class colour"
        );
        assert_near(bounds([&drawing.paths[0]]), RECT, 0.01);
    }
    let drawing = fixture("illustrator.svg");
    assert_eq!(drawing.paths[1].color, "#0000ff");
    assert!(drawing.paths[1].subpaths[0].closed);
}

#[test]
fn affinity_pixels_import_at_72_dpi() {
    let drawing = fixture("affinity.svg");
    assert_eq!(drawing.dpi_reason.as_deref(), Some("Affinity"));
    // The invisible artboard background is not a shape.
    assert_eq!(drawing.paths.len(), 1);
    assert_eq!(drawing.paths[0].color, "#ff0000");
    // The page is 284 px wide, rounded up from 100 mm.
    assert_near(
        bounds([&drawing.paths[0]]),
        [
            10.0,
            10.0 + 0.268 * 25.4 / 72.0,
            90.0,
            40.0 + 0.268 * 25.4 / 72.0,
        ],
        0.01,
    );
}

#[test]
fn basic_shapes_transforms_and_use_resolve() {
    let drawing = fixture("shapes.svg");
    assert_near(
        [0.0, 0.0, drawing.width, drawing.height],
        [0.0, 0.0, 200.0, 100.0],
        1e-4,
    );
    // SVG y is down from the top; document y is up from the bottom.
    let flip = |[x0, y0, x1, y1]: [f64; 4]| [x0, 100.0 - y1, x1, 100.0 - y0];
    let cases = [
        ("rounded", [10.0, 10.0, 40.0, 30.0]),
        ("circle", [50.0, 10.0, 70.0, 30.0]),
        ("ellipse", [75.0, 15.0, 105.0, 25.0]),
        ("line", [110.0, 10.0, 130.0, 30.0]),
        ("polyline", [140.0, 10.0, 160.0, 30.0]),
        ("polygon", [170.0, 10.0, 190.0, 30.0]),
        // rotate(90) about (100,50) swings the 20 x 10 rect to the left.
        ("rotated", [90.0, 50.0, 100.0, 70.0]),
    ];
    for (name, expected) in cases {
        assert_near(bounds([path(&drawing, name)]), flip(expected), 1e-3);
    }
    let colors: Vec<_> = ["rounded", "line", "polygon", "arc"]
        .map(|name| path(&drawing, name).color.clone())
        .into();
    assert_eq!(colors, ["#ff0000", "#0000ff", "#0000ff", "#000000"]);

    let arc = path(&drawing, "arc");
    assert!(arc.subpaths[0].closed);
    let kinds: Vec<_> = arc.subpaths[0]
        .segments
        .iter()
        .map(|s| match s {
            ImportedSegment::Line { .. } => "line",
            ImportedSegment::Quad { .. } => "quad",
            ImportedSegment::Cubic { .. } => "cubic",
        })
        .collect();
    assert!(
        kinds.contains(&"quad") && kinds.contains(&"cubic"),
        "{kinds:?}"
    );

    // <use> copies of the 10 mm tile: scaled 2x at (120,50) and plain at (160,60).
    let tiles: Vec<_> = drawing
        .paths
        .iter()
        .filter(|p| p.color == "#00e000")
        .collect();
    assert_eq!(tiles.len(), 2);
    assert_near(bounds([tiles[0]]), flip([120.0, 50.0, 140.0, 70.0]), 1e-3);
    assert_near(bounds([tiles[1]]), flip([160.0, 60.0, 170.0, 70.0]), 1e-3);
    let total: usize = drawing
        .paths
        .iter()
        .flat_map(|p| &p.subpaths)
        .map(|s| s.segments.len())
        .sum();
    assert_eq!(drawing.segments as usize, total);
}

#[test]
fn text_becomes_outlines() {
    let drawing = fixture("text.svg");
    assert!(!drawing.paths.is_empty(), "text converts to glyph paths");
    assert!(drawing.paths.iter().all(|p| p.color == "#ff8000"));
    let [x0, y0, x1, y1] = bounds(&drawing.paths);
    // "Laser" in 12 mm Arial starts near x = 5 and sits on the 5 mm baseline.
    assert!(
        (4.0..7.0).contains(&x0) && x1 > 30.0 && x1 < 45.0,
        "{x0} {x1}"
    );
    assert!(
        (4.0..5.5).contains(&y0) && y1 > 12.0 && y1 < 16.0,
        "{y0} {y1}"
    );
}

#[test]
fn large_files_import_quickly() {
    // 10k segments: 2,500 small squares.
    let mut body = String::new();
    for i in 0..2500 {
        let (x, y) = (i % 50 * 4, i / 50 * 4);
        body.push_str(&format!(
            r##"<path d="M{x} {y}h3v3h-3z" stroke="#ff0000" fill="none"/>"##
        ));
    }
    let text = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="200mm" height="200mm" viewBox="0 0 200 200">{body}</svg>"#
    );
    let started = std::time::Instant::now();
    let drawing = import_svg_str(&text).unwrap();
    assert_eq!(drawing.paths.len(), 2500);
    assert!(drawing.segments >= 7500, "{}", drawing.segments);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn svgz_is_decompressed() {
    use std::io::Write;
    let text = std::fs::read(format!(
        "{}/tests/fixtures/svg/inkscape.svg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&text).unwrap();
    let drawing = import_svg(&encoder.finish().unwrap()).unwrap();
    assert_eq!(drawing.paths.len(), 2);
}
