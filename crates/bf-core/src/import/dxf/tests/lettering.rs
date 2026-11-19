//! TEXT and MTEXT, drawn in the Geist test font.

use super::super::lettering::{decode, imported, mtext_plain};
use super::*;
use crate::import::FontSubstitution;
use crate::import::curve::Bounds;
use crate::text::{TextAlign, TextStyle, layout};

const HEIGHT: f64 = 5.0;
/// Glyph outlines carry a little float noise; cap heights are checked to
/// the issue's 0.05 mm.
const CAP: f64 = 0.05;

/// A LINE far down and left, so the page's corner (which becomes the
/// origin) is at (-100, -100) and text keeps its file coordinates + 100.
fn marker() -> String {
    line("0", 7, (-100.0, -100.0), (-99.0, -100.0))
}

/// Imports `entities` beside the marker and returns the bounds of the paths
/// on layer "T", in file coordinates.
fn text_bounds_in(file: &File, entities: &[String]) -> [f64; 4] {
    let mut all = vec![marker()];
    all.extend_from_slice(entities);
    let drawing = file.import(&all);
    let mut bounds = Bounds::EMPTY;
    for path in drawing.paths.iter().filter(|p| p.name == "T") {
        for subpath in &path.subpaths {
            bounds.add_subpath(subpath);
        }
    }
    assert!(!bounds.is_empty(), "no text was drawn");
    let [x0, y0, x1, y1] = bounds.0;
    [x0 - 100.0, y0 - 100.0, x1 - 100.0, y1 - 100.0]
}

fn text_bounds(entities: &[String]) -> [f64; 4] {
    text_bounds_in(&mm(), entities)
}

#[track_caller]
fn assert_bounds(actual: [f64; 4], expected: [f64; 4], tolerance: f64) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a - e).abs() <= tolerance,
            "{actual:?} != {expected:?} (± {tolerance})"
        );
    }
}

fn shift([x0, y0, x1, y1]: [f64; 4], dx: f64, dy: f64) -> [f64; 4] {
    [x0 + dx, y0 + dy, x1 + dx, y1 + dy]
}

/// `text` laid out by the text module at `HEIGHT`, left-aligned on a
/// baseline at the origin: its tight bounds and advance width.
fn reference(text: &str) -> ([f64; 4], f64) {
    let laid = layout(
        &geist(),
        text,
        &TextStyle {
            font_family: "Geist".into(),
            font_weight: 400,
            italic: false,
            height: HEIGHT,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: TextAlign::Left,
        },
    )
    .unwrap();
    let mut bounds = Bounds::EMPTY;
    for path in &laid.outline {
        bounds.add_subpath(&imported(path));
    }
    (bounds.0, *laid.lines[0].carets.last().unwrap())
}

/// A TEXT on layer "T" with extra group codes.
fn text(value: &str, at: (f64, f64), extra: &[(i32, &str)]) -> String {
    let mut text = pairs(&[
        (0, "TEXT"),
        (8, "T"),
        (10, &at.0.to_string()),
        (20, &at.1.to_string()),
        (30, "0"),
        (40, &HEIGHT.to_string()),
        (1, value),
    ]);
    text += &pairs(extra);
    text
}

/// An MTEXT on layer "T", `chunks` written as code 3 before the final code 1.
fn mtext(chunks: &[&str], last: &str, at: (f64, f64), extra: &[(i32, &str)]) -> String {
    let mut text = pairs(&[
        (0, "MTEXT"),
        (8, "T"),
        (10, &at.0.to_string()),
        (20, &at.1.to_string()),
        (30, "0"),
        (40, &HEIGHT.to_string()),
        (41, "0"),
    ]);
    text += &pairs(extra);
    for chunk in chunks {
        text += &pairs(&[(3, chunk)]);
    }
    text += &pairs(&[(1, last)]);
    text
}

fn style(name: &str, font: &str, extra: &[(i32, &str)]) -> String {
    let mut text = pairs(&[
        (0, "STYLE"),
        (2, name),
        (70, "0"),
        (40, "0"),
        (41, "1"),
        (50, "0"),
        (71, "0"),
        (42, "2.5"),
        (3, font),
        (4, ""),
    ]);
    text += &pairs(extra);
    text
}

#[test]
fn text_cap_height_sits_on_its_baseline() {
    let [x0, y0, _, y1] = text_bounds(&[text("H", (10.0, 20.0), &[])]);
    assert!((y0 - 20.0).abs() <= CAP, "baseline {y0}");
    assert!((y1 - 25.0).abs() <= CAP, "cap top {y1}");
    let (h, _) = reference("H");
    assert!((x0 - (10.0 + h[0])).abs() <= 1e-3);
}

#[test]
fn text_rotates_and_takes_its_width_factor() {
    let (h, _) = reference("H");
    let bounds = text_bounds(&[text("H", (10.0, 20.0), &[(50, "90"), (41, "0.5")])]);
    // Turned a quarter anticlockwise: the cap height runs along -x and the
    // halved width up y.
    assert_bounds(
        bounds,
        [10.0 - HEIGHT, 20.0 + h[0] * 0.5, 10.0, 20.0 + h[2] * 0.5],
        CAP,
    );
}

#[test]
fn justified_text_lands_on_the_second_point() {
    let (h, width) = reference("H");
    let at = |justify: &[(i32, &str)]| {
        let mut extra = vec![(11, "50"), (21, "40"), (31, "0")];
        extra.extend_from_slice(justify);
        // The first point is ignored once the text is justified.
        text_bounds(&[text("H", (0.0, 0.0), &extra)])
    };
    // Center and Right: the baseline's middle and end.
    assert_bounds(at(&[(72, "1")]), shift(h, 50.0 - width / 2.0, 40.0), CAP);
    assert_bounds(at(&[(72, "2")]), shift(h, 50.0 - width, 40.0), CAP);
    // Middle: the text's centre.
    let [x0, y0, x1, y1] = at(&[(72, "4")]);
    assert!(((x0 + x1) / 2.0 - 50.0).abs() <= CAP && ((y0 + y1) / 2.0 - 40.0).abs() <= CAP);
    // Top left: the cap height hangs from the point.
    assert_bounds(at(&[(73, "3")]), shift(h, 50.0, 40.0 - HEIGHT), CAP);
    // Middle centre: half the cap height below.
    assert_bounds(
        at(&[(72, "1"), (73, "2")]),
        shift(h, 50.0 - width / 2.0, 40.0 - HEIGHT / 2.0),
        CAP,
    );
}

#[test]
fn fit_and_aligned_text_fill_between_the_points() {
    let (h, width) = reference("H");
    let fit = text_bounds(&[text(
        "H",
        (10.0, 20.0),
        &[(72, "5"), (11, "110"), (21, "20"), (31, "0")],
    )]);
    let s = 100.0 / width;
    // Only the width stretches.
    assert_bounds(
        fit,
        [10.0 + h[0] * s, 20.0 + h[1], 10.0 + h[2] * s, 20.0 + h[3]],
        CAP,
    );
    let aligned = text_bounds(&[text(
        "H",
        (10.0, 20.0),
        &[(72, "3"), (11, "10"), (21, "40"), (31, "0")],
    )]);
    // Aligned up the y axis: both axes scale by 20 / width, turned 90°.
    let s = 20.0 / width;
    assert_bounds(
        aligned,
        [
            10.0 - h[3] * s,
            20.0 + h[0] * s,
            10.0 - h[1] * s,
            20.0 + h[2] * s,
        ],
        CAP,
    );
}

#[test]
fn text_in_an_insert_follows_the_insert() {
    let (h, _) = reference("H");
    let file = File {
        blocks: vec![("Label", (0.0, 0.0), vec![text("H", (0.0, 0.0), &[])])],
        ..mm()
    };
    let bounds = text_bounds_in(
        &file,
        &[pairs(&[
            (0, "INSERT"),
            (8, "T"),
            (2, "Label"),
            (10, "100"),
            (20, "50"),
            (41, "2"),
            (42, "2"),
            (50, "90"),
        ])],
    );
    // Doubled, then turned a quarter anticlockwise about the insert point.
    assert_bounds(
        bounds,
        [
            100.0 - 2.0 * HEIGHT,
            50.0 + 2.0 * h[0],
            100.0,
            50.0 + 2.0 * h[2],
        ],
        2.0 * CAP,
    );
}

#[test]
fn mirror_flags_flip_the_text_about_its_point() {
    let (h, _) = reference("H");
    let backward = text_bounds(&[text("H", (10.0, 20.0), &[(71, "2")])]);
    assert_bounds(backward, [10.0 - h[2], 20.0, 10.0 - h[0], 25.0], CAP);
    let upside_down = text_bounds(&[text("H", (10.0, 20.0), &[(71, "4")])]);
    assert_bounds(upside_down, [10.0 + h[0], 15.0, 10.0 + h[2], 20.0], CAP);
}

#[test]
fn special_codes_decode() {
    assert_eq!(decode("45%%d"), "45°");
    assert_eq!(decode("%%p0.1 %%c20 100%%%"), "±0.1 Ø20 100%");
    assert_eq!(decode("%%uunder%%u %%oover"), "under over");
    assert_eq!(decode("\\U+00B0C"), "°C");
    assert_eq!(decode("%%176"), "°");
    assert_eq!(mtext_plain("{\\fArial|b0;AB}\\PCD"), "AB\nCD");
    assert_eq!(
        mtext_plain("\\A1;\\H2.5x;\\C1;Red\\~text \\{x\\} a\\\\b"),
        "Red\u{a0}text {x} a\\b"
    );
    assert_eq!(
        mtext_plain("\\S1^2; and \\S3/4; and \\S5#6;"),
        "1/2 and 3/4 and 5/6"
    );
    assert_eq!(
        mtext_plain("\\LUnder\\l\\Oover\\o\\Kgone\\k%%d"),
        "Underovergone°"
    );
    assert_eq!(mtext_plain("a\\Nb\\U+00B1"), "a\nb±");
}

#[test]
fn mtext_breaks_lines_five_thirds_of_the_height_apart() {
    let pitch = HEIGHT * 5.0 / 3.0;
    // Top left: the first line's cap top is at the insertion point.
    let [_, y0, _, y1] = text_bounds(&[mtext(&[], "H\\PH", (10.0, 20.0), &[(71, "1")])]);
    assert!((y1 - 20.0).abs() <= CAP, "top {y1}");
    assert!((y0 - (20.0 - HEIGHT - pitch)).abs() <= CAP, "bottom {y0}");
    // Formatting goes; two lines stay.
    let [_, y0, _, y1] = text_bounds(&[mtext(
        &[],
        "{\\fArial|b0;AB}\\PCD",
        (10.0, 20.0),
        &[(71, "1")],
    )]);
    assert!((y1 - 20.0).abs() <= CAP, "top {y1}");
    // C and D's round bottoms dip a little below the baseline.
    assert!((y0 - (20.0 - HEIGHT - pitch)).abs() <= 0.15, "bottom {y0}");
    // A line spacing factor stretches the gap.
    let [_, y0, _, _] = text_bounds(&[mtext(&[], "H\\PH", (10.0, 20.0), &[(71, "1"), (44, "2")])]);
    assert!(
        (y0 - (20.0 - HEIGHT - 2.0 * pitch)).abs() <= CAP,
        "bottom {y0}"
    );
}

#[test]
fn short_codes_at_the_end_of_a_string_survive_in_old_and_new_files() {
    // The dxf crate drops a backslash and what follows it when fewer than
    // six characters are left in a pre-R2007 file (see prepare).
    let entities = [mtext(&[], "AB\\PCD", (0.0, 0.0), &[(71, "1")])];
    for version in ["AC1015", "AC1021"] {
        let file = mm().build(&entities).replace("AC1015", version);
        let drawing = Drawing::load(&mut Cursor::new(&*prepare(file.as_bytes()))).unwrap();
        let Some(EntityType::MText(text)) = drawing.entities().next().map(|e| &e.specific) else {
            panic!("no MTEXT");
        };
        assert_eq!(text.text, "AB\\PCD", "{version}");
    }
    // A real \U+ escape is still decoded by the crate.
    let file = mm().build(&[text("45\\U+00B0", (0.0, 0.0), &[])]);
    let drawing = Drawing::load(&mut Cursor::new(&*prepare(file.as_bytes()))).unwrap();
    let Some(EntityType::Text(text)) = drawing.entities().next().map(|e| &e.specific) else {
        panic!("no TEXT");
    };
    assert_eq!(text.value, "45°");
}

#[test]
fn mtext_without_a_reference_width_does_not_wrap() {
    let widths = |extra: &[(i32, &str)]| {
        let mut entity = pairs(&[(0, "MTEXT"), (8, "T"), (10, "0"), (20, "0"), (40, "5")]);
        entity += &pairs(extra);
        entity += &pairs(&[(1, "Top left")]);
        let file = mm().build(&[entity]);
        let drawing = Drawing::load(&mut Cursor::new(&*prepare(file.as_bytes()))).unwrap();
        let Some(EntityType::MText(text)) = drawing.entities().next().map(|e| &e.specific) else {
            panic!("no MTEXT");
        };
        text.reference_rectangle_width
    };
    // The crate's default would be 1, wrapping every word.
    assert_eq!(widths(&[]), 0.0);
    assert_eq!(widths(&[(41, "30")]), 30.0);
}

#[test]
fn mtext_anchors_by_attachment_point() {
    let (h, width) = reference("H");
    let at = |point: &str| text_bounds(&[mtext(&[], "H", (50.0, 40.0), &[(71, point)])]);
    assert_bounds(at("1"), shift(h, 50.0, 40.0 - HEIGHT), CAP);
    assert_bounds(
        at("5"),
        shift(h, 50.0 - width / 2.0, 40.0 - HEIGHT / 2.0),
        CAP,
    );
    assert_bounds(at("9"), shift(h, 50.0 - width, 40.0), CAP);
}

#[test]
fn mtext_wraps_at_its_reference_width() {
    let (two, width) = reference("HH HH");
    let [x0, y0, x1, y1] = text_bounds(&[mtext(
        &[],
        "HH HH HH",
        (0.0, 0.0),
        &[(71, "1"), (41, &(width + 0.1).to_string())],
    )]);
    // Two lines, "HH HH" and "HH".
    assert!((y1 - y0 - (HEIGHT + HEIGHT * 5.0 / 3.0)).abs() <= CAP);
    assert!((x0 - two[0]).abs() <= CAP && (x1 - two[2]).abs() <= CAP);
}

#[test]
fn mtext_reads_text_split_across_chunks() {
    let whole = text_bounds(&[mtext(&[], "HH HH HHH", (0.0, 0.0), &[])]);
    let split = text_bounds(&[mtext(&["HH ", "HH"], " HHH", (0.0, 0.0), &[])]);
    assert_bounds(split, whole, 1e-6);
}

#[test]
fn mtext_rotation_is_in_degrees_or_an_x_axis() {
    let (h, _) = reference("H");
    // Bottom left, turned 90°: the cap height runs along -x.
    let expected = [10.0 - HEIGHT, 20.0 + h[0], 10.0, 20.0 + h[2]];
    let by_angle = text_bounds(&[mtext(&[], "H", (10.0, 20.0), &[(71, "7"), (50, "90")])]);
    assert_bounds(by_angle, expected, CAP);
    let by_axis = text_bounds(&[mtext(
        &[],
        "H",
        (10.0, 20.0),
        &[(71, "7"), (11, "0"), (21, "1"), (31, "0")],
    )]);
    assert_bounds(by_axis, expected, CAP);
}

#[test]
fn fonts_resolve_by_family_then_file_then_fall_back() {
    let entities = |style: &str| {
        vec![
            marker(),
            text("H", (0.0, 0.0), &[(7, style)]),
            mtext(&[], "H", (0.0, 20.0), &[(7, style)]),
        ]
    };
    let file = File {
        styles: vec![
            style("Shx", "romans.shx", &[]),
            style(
                "Family",
                "",
                &[(1001, "ACAD"), (1000, "Geist"), (1071, "34")],
            ),
            style("Missing", "nothere.ttf", &[]),
        ],
        ..mm()
    };
    let shx = file.import(&entities("Shx"));
    assert_eq!(shx.paths.len(), 3);
    assert_eq!(
        shx.substituted_fonts,
        [FontSubstitution {
            requested: "romans.shx".into(),
            used: "Geist".into()
        }]
    );
    assert!(
        file.import(&entities("Family"))
            .substituted_fonts
            .is_empty()
    );
    assert_eq!(
        file.import(&entities("Missing")).substituted_fonts[0].requested,
        "nothere.ttf"
    );

    // Matched by the installed file's name.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/text/testdata/Geist-Regular-ascii.ttf"
    );
    let mut db = Database::new();
    db.load_font_file(path).unwrap();
    let by_file = File {
        styles: vec![style("File", "GEIST-Regular-ascii.TTF", &[])],
        ..mm()
    };
    let drawing = import_dxf(by_file.build(&entities("File")).as_bytes(), None, &db).unwrap();
    assert!(drawing.substituted_fonts.is_empty());
    assert_eq!(drawing.paths.len(), 3);
}

#[test]
fn empty_text_is_ignored_and_text_without_fonts_is_skipped() {
    let entities = [
        line("0", 7, (0.0, 0.0), (10.0, 0.0)),
        text("", (0.0, 0.0), &[]),
        mtext(&[], "{}", (0.0, 0.0), &[]),
        text("H", (0.0, 0.0), &[]),
    ];
    let drawing = mm().import(&entities);
    assert_eq!((drawing.paths.len(), drawing.skipped), (2, 0));
    let none = import_dxf(mm().build(&entities).as_bytes(), None, &Database::new()).unwrap();
    assert_eq!((none.paths.len(), none.skipped), (1, 1));
    assert!(none.substituted_fonts.is_empty());
}

#[test]
fn text_is_one_path_counted_in_size_and_segments() {
    let file = File::units(None).build(&[text("HH", (0.0, 0.0), &[(62, "5")])]);
    let Err(ImportError::NeedsUnits { width, height }) =
        import_dxf(file.as_bytes(), None, &geist())
    else {
        panic!("expected to ask for units");
    };
    let (h, _) = reference("HH");
    assert!((width - (h[2] - h[0])).abs() <= CAP && (height - HEIGHT).abs() <= CAP);
    let drawing = import_dxf(file.as_bytes(), Some(LengthUnit::Millimeters), &geist()).unwrap();
    assert_eq!(drawing.paths.len(), 1);
    let path = &drawing.paths[0];
    assert_eq!(path.color, "#0000ff");
    // Two H's, each one closed contour.
    assert_eq!(path.subpaths.len(), 2);
    let segments: usize = path.subpaths.iter().map(|s| s.segments.len()).sum();
    assert_eq!(drawing.segments as usize, segments);
}
