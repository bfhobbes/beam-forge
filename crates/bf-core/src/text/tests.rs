use super::*;

/// Geist Regular (OFL, see `testdata/OFL.txt`), cut down to printable ASCII.
/// Its cap height is 710 units of a 1000-unit em.
const GEIST: &[u8] = include_bytes!("testdata/Geist-Regular-ascii.ttf");

fn db() -> Database {
    let mut db = Database::new();
    db.load_font_data(GEIST.to_vec());
    db
}

fn style() -> TextStyle {
    TextStyle {
        font_family: "Geist".into(),
        font_weight: 400,
        italic: false,
        height: 10.0,
        letter_spacing: 0.0,
        line_spacing: 1.0,
        align: TextAlign::Left,
    }
}

fn points(outline: &[Subpath]) -> impl Iterator<Item = Point> + '_ {
    outline.iter().flat_map(|path| {
        std::iter::once(path.start).chain(path.segments.iter().map(|segment| match segment {
            Segment::Line { to } | Segment::Quad { to, .. } | Segment::Cubic { to, .. } => *to,
        }))
    })
}

fn bounds(outline: &[Subpath]) -> (f64, f64, f64, f64) {
    points(outline).fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
    )
}

#[test]
fn lists_families_with_their_styles() {
    let fonts = fonts(&db());
    assert_eq!(
        fonts,
        vec![FontFamily {
            name: "Geist".into(),
            styles: vec![FontStyle {
                weight: 400,
                italic: false
            }],
        }]
    );
}

#[test]
fn height_is_the_cap_height_of_an_h_on_the_baseline() {
    let laid = layout(&db(), "H", &style()).unwrap();
    assert!(!laid.substituted);
    assert_eq!(laid.font_family, "Geist");
    let (x0, y0, _, y1) = bounds(&laid.outline);
    assert!(y0.abs() < 1e-9, "H sits on the baseline, got {y0}");
    assert!((y1 - 10.0).abs() < 1e-9, "H is 10 mm tall, got {y1}");
    assert!(x0 >= 0.0, "left-aligned text starts at x = 0");
    assert!(laid.outline.iter().all(|path| path.closed));
}

#[test]
fn carets_step_through_each_character() {
    let laid = layout(&db(), "HI", &style()).unwrap();
    let carets = &laid.lines[0].carets;
    assert_eq!(carets.len(), 3);
    assert_eq!(carets[0], 0.0);
    assert!(carets[0] < carets[1] && carets[1] < carets[2]);
    let (_, _, x1, _) = bounds(&laid.outline);
    assert!(carets[2] >= x1, "the end caret is past the last glyph");
}

#[test]
fn letter_spacing_adds_space_between_characters_only() {
    let plain = layout(&db(), "HI", &style()).unwrap();
    let spaced = layout(
        &db(),
        "HI",
        &TextStyle {
            letter_spacing: 2.0,
            ..style()
        },
    )
    .unwrap();
    let (a, b) = (&plain.lines[0].carets, &spaced.lines[0].carets);
    assert_eq!(b[0], a[0]);
    assert!((b[1] - a[1] - 2.0).abs() < 1e-9, "the gap comes before I");
    assert!((b[2] - a[2] - 2.0).abs() < 1e-9, "and none after the last");
}

#[test]
fn kerning_pulls_pairs_together() {
    let db = db();
    let width = |text: &str| {
        *layout(&db, text, &style()).unwrap().lines[0]
            .carets
            .last()
            .unwrap()
    };
    let (a, v) = (width("A"), width("V"));
    assert!(width("AV") < a + v, "Geist kerns AV");
}

#[test]
fn lines_go_down_and_align() {
    let left = layout(&db(), "HH\nH", &style()).unwrap();
    assert_eq!(left.lines.len(), 2);
    assert_eq!(left.lines[0].baseline, 0.0);
    assert!(
        left.lines[1].baseline < -10.0,
        "the second line is below the first"
    );

    let centred = layout(
        &db(),
        "HH\nH",
        &TextStyle {
            align: TextAlign::Center,
            ..style()
        },
    )
    .unwrap();
    for line in &centred.lines {
        let (first, last) = (line.carets[0], *line.carets.last().unwrap());
        assert!((first + last).abs() < 1e-9, "centred on x = 0");
    }

    let right = layout(
        &db(),
        "HH\nH",
        &TextStyle {
            align: TextAlign::Right,
            ..style()
        },
    )
    .unwrap();
    for line in &right.lines {
        assert!(line.carets.last().unwrap().abs() < 1e-9, "ends at x = 0");
    }

    let double = layout(
        &db(),
        "H\nH",
        &TextStyle {
            line_spacing: 2.0,
            ..style()
        },
    )
    .unwrap();
    assert!((double.lines[1].baseline - 2.0 * left.lines[1].baseline).abs() < 1e-9);
}

#[test]
fn empty_text_has_no_outline_and_one_caret() {
    let laid = layout(&db(), "", &style()).unwrap();
    assert!(laid.outline.is_empty());
    assert_eq!(laid.lines.len(), 1);
    assert_eq!(laid.lines[0].carets, vec![0.0]);
}

#[test]
fn carets_count_utf16_code_units() {
    // é is one UTF-16 unit, 😀 two. Neither is in the subset font, but they
    // still take a caret position each.
    let laid = layout(&db(), "é😀H", &style()).unwrap();
    let carets = &laid.lines[0].carets;
    assert_eq!(carets.len(), "é😀H".encode_utf16().count() + 1);
    assert_eq!(
        carets[1], carets[2],
        "the surrogate pair shares one position"
    );
}

#[test]
fn a_missing_family_falls_back_and_says_so() {
    let laid = layout(
        &db(),
        "H",
        &TextStyle {
            font_family: "Not A Real Font".into(),
            ..style()
        },
    )
    .unwrap();
    assert!(laid.substituted);
    assert_eq!(laid.font_family, "Geist");
    assert!(!laid.outline.is_empty());
}

#[test]
fn refuses_bad_sizes_and_reports_no_fonts() {
    let bad = |style: TextStyle| layout(&db(), "H", &style).unwrap_err();
    assert!(matches!(
        bad(TextStyle {
            height: 0.0,
            ..style()
        }),
        TextError::Invalid(_)
    ));
    assert!(matches!(
        bad(TextStyle {
            line_spacing: f64::NAN,
            ..style()
        }),
        TextError::Invalid(_)
    ));
    assert_eq!(
        layout(&Database::new(), "H", &style()).unwrap_err(),
        TextError::NoFonts
    );
}
