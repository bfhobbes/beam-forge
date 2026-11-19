//! DXF import against fixtures in the styles LibreCAD and Fusion 360 export.
//! Each draws an 80 x 30 mm rectangle as four loose lines, 10 mm in from
//! the origin, with a 20 mm circle at its centre. LibreCAD's is in
//! millimetres; Fusion's is an inch document with CRLF line endings.
//!
//! Text fixtures: `freecad_text.dxf` is FreeCAD 1.1.3's own export of Draft
//! texts (TEXT only; see `freecad_text.FCMacro`). No CAD tool on hand writes
//! MTEXT, so `ezdxf_mtext.dxf` is written by the ezdxf library instead
//! (`ezdxf_mtext.py`), and its reference positions come from the script's
//! definitions. Text is drawn in the Geist test font, the only one the
//! tests load, so every font is substituted.

use bf_core::import::dxf::import_dxf;
use bf_core::import::{
    FontSubstitution, ImportedDrawing, ImportedPath, ImportedSegment, ImportedSubpath, LengthUnit,
};
use bf_core::project::Segment;
use bf_core::text::{TextAlign, TextStyle, layout};
use fontdb::Database;

fn fixture(name: &str) -> ImportedDrawing {
    fixture_in(name, None)
}

fn fixture_in(name: &str, units: Option<LengthUnit>) -> ImportedDrawing {
    let path = format!("{}/tests/fixtures/dxf/{name}", env!("CARGO_MANIFEST_DIR"));
    import_dxf(&std::fs::read(&path).unwrap(), units, &geist())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Geist Regular cut down to ASCII, the text module's test font.
fn geist() -> Database {
    let mut db = Database::new();
    db.load_font_data(include_bytes!("../src/text/testdata/Geist-Regular-ascii.ttf").to_vec());
    db
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

/// Bounds of every subpath of a path.
fn path_bounds(path: &ImportedPath) -> [f64; 4] {
    path.subpaths
        .iter()
        .map(bounds)
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
}

/// `text` laid out in Geist with cap height `height`, left-aligned on a
/// baseline at the origin: the bounds of its on-curve points and its
/// advance width.
fn reference(text: &str, height: f64) -> ([f64; 4], f64) {
    let laid = layout(
        &geist(),
        text,
        &TextStyle {
            font_family: "Geist".into(),
            font_weight: 400,
            italic: false,
            height,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: TextAlign::Left,
        },
    )
    .unwrap();
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for path in &laid.outline {
        let ends = path.segments.iter().map(|s| match s {
            Segment::Line { to } | Segment::Quad { to, .. } | Segment::Cubic { to, .. } => *to,
        });
        for p in std::iter::once(path.start).chain(ends) {
            b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
        }
    }
    (b, *laid.lines[0].carets.last().unwrap())
}

fn shift(b: [f64; 4], dx: f64, dy: f64) -> [f64; 4] {
    [b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy]
}

/// `b` turned a quarter anticlockwise about the origin, then moved.
fn quarter_turn(b: [f64; 4], dx: f64, dy: f64) -> [f64; 4] {
    [dx - b[3], dy + b[0], dx - b[1], dy + b[2]]
}

/// Within a tenth of the text height, the issue's tolerance.
#[track_caller]
fn assert_placed(actual: [f64; 4], expected: [f64; 4], height: f64) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a - e).abs() <= 0.1 * height,
            "{actual:?} != {expected:?} (± {})",
            0.1 * height
        );
    }
}

#[test]
fn freecad_text_lands_where_freecad_placed_it() {
    // FreeCAD writes no $INSUNITS; its documents are in millimetres.
    let drawing = fixture_in("freecad_text.dxf", Some(LengthUnit::Millimeters));
    assert_eq!(drawing.skipped, 0);
    // The 100 x 50 frame keeps the page origin on the file's.
    assert_near(
        [0.0, 0.0, drawing.width, drawing.height],
        [0.0, 0.0, 100.0, 50.0],
    );
    let [_, beam, centre, right, up] = &drawing.paths[..] else {
        panic!("expected the frame and four texts: {}", drawing.paths.len());
    };
    // FreeCAD writes 0.8 x the font size as the DXF height. Each text sits
    // on the Draft text's placement point, per its justification.
    let (b, _) = reference("Beam Forge", 4.0);
    assert_placed(path_bounds(beam), shift(b, 10.0, 30.0), 4.0);
    let (b, w) = reference("Centre", 3.2);
    assert_placed(path_bounds(centre), shift(b, 50.0 - w / 2.0, 10.0), 3.2);
    let (b, w) = reference("Right", 2.4);
    assert_placed(path_bounds(right), shift(b, 95.0 - w, 40.0), 2.4);
    let (b, _) = reference("Up", 4.8);
    assert_placed(path_bounds(up), quarter_turn(b, 90.0, 5.0), 4.8);
    // The STANDARD style names Arial, which the tests don't load.
    assert_eq!(
        drawing.substituted_fonts,
        [FontSubstitution {
            requested: "ARIAL.TTF".into(),
            used: "Geist".into()
        }]
    );
}

#[test]
fn ezdxf_mtext_lands_at_its_reference_points() {
    let drawing = fixture("ezdxf_mtext.dxf");
    assert_eq!(drawing.units.as_deref(), Some("millimetres"));
    assert_eq!(drawing.skipped, 0);
    assert_near(
        [0.0, 0.0, drawing.width, drawing.height],
        [0.0, 0.0, 100.0, 60.0],
    );
    let [
        _,
        top_left,
        middle,
        bottom_right,
        up,
        axis,
        wrapped,
        degrees,
    ] = &drawing.paths[..]
    else {
        panic!(
            "expected the frame and seven texts: {}",
            drawing.paths.len()
        );
    };

    // Top left: the cap top hangs from (10, 50), unwrapped although the
    // file gives no reference width.
    let (b, _) = reference("Top left", 5.0);
    assert_placed(path_bounds(top_left), shift(b, 10.0, 45.0), 5.0);

    // Middle centre: "Mid" over "line", 5/3 x 4 apart, centred on (50, 30).
    // The block runs from the first cap top (4) to the second line's
    // lowest point.
    let pitch = 4.0 * 5.0 / 3.0;
    let (mid, mid_w) = reference("Mid", 4.0);
    let (line, line_w) = reference("line", 4.0);
    let centre = (4.0 + (line[1] - pitch)) / 2.0;
    let expected = [
        (50.0 - mid_w / 2.0 + mid[0]).min(50.0 - line_w / 2.0 + line[0]),
        30.0 - centre - pitch + line[1],
        (50.0 - mid_w / 2.0 + mid[2]).max(50.0 - line_w / 2.0 + line[2]),
        30.0 - centre + mid[3],
    ];
    assert_placed(path_bounds(middle), expected, 4.0);

    let (b, w) = reference("BR", 3.0);
    assert_placed(path_bounds(bottom_right), shift(b, 90.0 - w, 10.0), 3.0);

    // Bottom left and turned 90°, by group 50 in degrees and by an X axis.
    // The bottom is the lowest point, so "Up" rises by its descender.
    let (b, _) = reference("Up", 4.0);
    assert_placed(
        path_bounds(up),
        quarter_turn(shift(b, 0.0, -b[1]), 95.0, 5.0),
        4.0,
    );
    let (b, _) = reference("Axis", 4.0);
    assert_placed(
        path_bounds(axis),
        quarter_turn(shift(b, 0.0, -b[1]), 5.0, 5.0),
        4.0,
    );

    // Wrapped to 30 mm: in Geist, "wraps at thirty" over "mm".
    let (first, _) = reference("wraps at thirty", 3.0);
    let (second, _) = reference("mm", 3.0);
    let expected = [
        10.0 + first[0].min(second[0]),
        40.0 - 3.0 - 5.0 + second[1],
        10.0 + first[2].max(second[2]),
        40.0 - 3.0 + first[3],
    ];
    assert_placed(path_bounds(wrapped), expected, 3.0);
    assert!(path_bounds(wrapped)[2] - 10.0 <= 30.0);

    // A TEXT centred on (50, 55), its %%d decoded.
    let (b, w) = reference("45\u{b0}", 3.0);
    assert_placed(path_bounds(degrees), shift(b, 50.0 - w / 2.0, 55.0), 3.0);

    assert!(drawing.substituted_fonts.contains(&FontSubstitution {
        requested: "romans.shx".into(),
        used: "Geist".into()
    }));
}
