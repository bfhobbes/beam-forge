//! DXF import against fixtures in the styles LibreCAD and Fusion 360 export.
//! Each draws an 80 x 30 mm rectangle as four loose lines, 10 mm in from
//! the origin, with a 20 mm circle at its centre. LibreCAD's is in
//! millimetres; Fusion's is an inch document with CRLF line endings.

use bf_core::import::dxf::import_dxf;
use bf_core::import::{ImportedDrawing, ImportedPath, ImportedSegment, ImportedSubpath};

fn fixture(name: &str) -> ImportedDrawing {
    let path = format!("{}/tests/fixtures/dxf/{name}", env!("CARGO_MANIFEST_DIR"));
    import_dxf(&std::fs::read(&path).unwrap(), None).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Bounds over every on-curve point and control point.
fn bounds(subpath: &ImportedSubpath) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut add = |x: f64, y: f64| b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    add(subpath.start.x, subpath.start.y);
    for segment in &subpath.segments {
        let to = segment.to();
        add(to.x, to.y);
    }
    b
}

#[track_caller]
fn assert_near(actual: [f64; 4], expected: [f64; 4]) {
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() <= 1e-3, "{actual:?} != {expected:?}");
    }
}

/// The closed four-line path: the rectangle.
fn rectangle(drawing: &ImportedDrawing) -> &ImportedPath {
    let rects: Vec<_> = drawing
        .paths
        .iter()
        .filter(|p| {
            let s = &p.subpaths[0];
            s.closed
                && s.segments.len() == 4
                && s.segments
                    .iter()
                    .all(|s| matches!(s, ImportedSegment::Line { .. }))
        })
        .collect();
    assert_eq!(rects.len(), 1, "one closed rectangle");
    rects[0]
}

#[test]
fn librecad_millimetre_drawing_imports_at_size() {
    let drawing = fixture("librecad.dxf");
    assert_eq!(drawing.units.as_deref(), Some("millimetres"));
    // The page is the drawing's extent: the rectangle.
    assert_near(
        [0.0, 0.0, drawing.width, drawing.height],
        [0.0, 0.0, 80.0, 30.0],
    );
    let rect = rectangle(&drawing);
    assert_eq!(
        (rect.name.as_str(), rect.color.as_str()),
        ("Cut", "#ff0000")
    );
    assert_near(bounds(&rect.subpaths[0]), [0.0, 0.0, 80.0, 30.0]);

    let circle = drawing.paths.iter().find(|p| p.name == "Engrave").unwrap();
    assert_eq!(circle.color, "#0000ff");
    assert_near(bounds(&circle.subpaths[0]), [30.0, 5.0, 50.0, 25.0]);

    // The bulged slot is one closed path: 10 x 5 between its arc centres.
    let slot = drawing
        .paths
        .iter()
        .find(|p| p.name == "Cut" && p.subpaths[0].segments.len() == 6)
        .unwrap();
    assert!(slot.subpaths[0].closed);
    assert_eq!(drawing.paths.len(), 3);
}

#[test]
fn fusion_inch_drawing_imports_at_size() {
    let drawing = fixture("fusion360.dxf");
    assert_eq!(drawing.units.as_deref(), Some("inches"));
    assert_near(
        [0.0, 0.0, drawing.width, drawing.height],
        [0.0, 0.0, 80.0, 30.0],
    );
    let rect = rectangle(&drawing);
    assert_near(bounds(&rect.subpaths[0]), [0.0, 0.0, 80.0, 30.0]);
    assert_eq!(rect.color, "#000000");

    // Line, fillet arc, line: one open chain from (10, 5) round to (25, 20).
    let corner = drawing
        .paths
        .iter()
        .find(|p| {
            let s = &p.subpaths[0];
            !s.closed && s.segments.len() == 3
        })
        .expect("the corner joins into one chain");
    let s = &corner.subpaths[0];
    assert!(matches!(s.segments[1], ImportedSegment::Cubic { .. }));
    assert_near(bounds(s), [10.0, 5.0, 25.0, 20.0]);

    // The spline, by its control points, peaks 3.75 mm above its ends.
    let spline = drawing
        .paths
        .iter()
        .find(|p| {
            let s = &p.subpaths[0];
            s.segments.len() == 1 && matches!(s.segments[0], ImportedSegment::Cubic { .. })
        })
        .unwrap();
    assert_near(bounds(&spline.subpaths[0]), [50.0, 5.0, 70.0, 5.0]);
    assert_eq!(drawing.paths.len(), 4);
}
