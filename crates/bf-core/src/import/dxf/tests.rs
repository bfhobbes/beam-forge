use super::*;
use crate::import::ImportedSegment;

/// Geist Regular cut down to ASCII (see `text/testdata`): the only font the
/// tests can see, so it is also the fallback.
const GEIST: &[u8] = include_bytes!("../../text/testdata/Geist-Regular-ascii.ttf");

fn geist() -> Database {
    let mut db = Database::new();
    db.load_font_data(GEIST.to_vec());
    db
}

/// DXF text from group-code pairs.
fn pairs(pairs: &[(i32, &str)]) -> String {
    pairs
        .iter()
        .map(|(code, value)| format!("{code}\n{value}\n"))
        .collect()
}

/// A drawing with `$INSUNITS` (if any), optional layers and blocks, and
/// `entities`, each already written as pairs.
struct File {
    units: Option<i32>,
    layers: Vec<(&'static str, i32)>,
    blocks: Vec<(&'static str, (f64, f64), Vec<String>)>,
    /// STYLE table rows, already written as pairs.
    styles: Vec<String>,
}

impl File {
    fn units(units: Option<i32>) -> File {
        File {
            units,
            layers: Vec::new(),
            blocks: Vec::new(),
            styles: Vec::new(),
        }
    }

    fn build(&self, entities: &[String]) -> String {
        let mut text = pairs(&[
            (0, "SECTION"),
            (2, "HEADER"),
            (9, "$ACADVER"),
            (1, "AC1015"),
        ]);
        if let Some(units) = self.units {
            text += &pairs(&[(9, "$INSUNITS"), (70, &units.to_string())]);
        }
        text += &pairs(&[(0, "ENDSEC")]);
        if !self.layers.is_empty() || !self.styles.is_empty() {
            text += &pairs(&[(0, "SECTION"), (2, "TABLES")]);
            if !self.layers.is_empty() {
                text += &pairs(&[(0, "TABLE"), (2, "LAYER")]);
                for (name, color) in &self.layers {
                    text += &pairs(&[(0, "LAYER"), (2, name), (70, "0"), (62, &color.to_string())]);
                }
                text += &pairs(&[(0, "ENDTAB")]);
            }
            if !self.styles.is_empty() {
                text += &pairs(&[(0, "TABLE"), (2, "STYLE")]);
                text += &self.styles.concat();
                text += &pairs(&[(0, "ENDTAB")]);
            }
            text += &pairs(&[(0, "ENDSEC")]);
        }
        if !self.blocks.is_empty() {
            text += &pairs(&[(0, "SECTION"), (2, "BLOCKS")]);
            for (name, (x, y), entities) in &self.blocks {
                text += &pairs(&[
                    (0, "BLOCK"),
                    (8, "0"),
                    (2, name),
                    (70, "0"),
                    (10, &x.to_string()),
                    (20, &y.to_string()),
                    (30, "0"),
                ]);
                text += &entities.concat();
                text += &pairs(&[(0, "ENDBLK"), (8, "0")]);
            }
            text += &pairs(&[(0, "ENDSEC")]);
        }
        text += &pairs(&[(0, "SECTION"), (2, "ENTITIES")]);
        text += &entities.concat();
        text += &pairs(&[(0, "ENDSEC"), (0, "EOF")]);
        text
    }

    fn import(&self, entities: &[String]) -> ImportedDrawing {
        import_dxf(self.build(entities).as_bytes(), None, &geist()).unwrap()
    }
}

fn mm() -> File {
    File::units(Some(4))
}

fn line(layer: &str, color: i32, from: (f64, f64), to: (f64, f64)) -> String {
    pairs(&[
        (0, "LINE"),
        (8, layer),
        (62, &color.to_string()),
        (10, &from.0.to_string()),
        (20, &from.1.to_string()),
        (30, "0"),
        (11, &to.0.to_string()),
        (21, &to.1.to_string()),
        (31, "0"),
    ])
}

fn circle(center: (f64, f64), radius: f64, extra: &[(i32, &str)]) -> String {
    let mut text = pairs(&[
        (0, "CIRCLE"),
        (8, "0"),
        (10, &center.0.to_string()),
        (20, &center.1.to_string()),
        (30, "0"),
        (40, &radius.to_string()),
    ]);
    text += &pairs(extra);
    text
}

fn rectangle_lines(layer: &str, color: i32, x: f64, y: f64, w: f64, h: f64) -> Vec<String> {
    vec![
        line(layer, color, (x, y), (x + w, y)),
        line(layer, color, (x + w, y + h), (x + w, y)),
        line(layer, color, (x, y + h), (x + w, y + h)),
        line(layer, color, (x, y + h), (x, y)),
    ]
}

fn ends(subpath: &ImportedSubpath) -> Vec<(f64, f64)> {
    std::iter::once(subpath.start)
        .chain(subpath.segments.iter().map(|s| s.to()))
        .map(|p| (p.x, p.y))
        .collect()
}

#[track_caller]
fn assert_near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}");
}

#[test]
fn four_loose_lines_become_one_closed_rectangle() {
    let drawing = mm().import(&rectangle_lines("Cut", 1, 10.0, 20.0, 80.0, 30.0));
    assert_eq!(drawing.format, ImportFormat::Dxf);
    assert_eq!(drawing.paths.len(), 1);
    let path = &drawing.paths[0];
    assert_eq!(path.name, "Cut");
    assert_eq!(path.color, "#ff0000");
    let rect = &path.subpaths[0];
    assert!(rect.closed);
    assert_eq!(rect.segments.len(), 4);
    // The drawing's corner moves to the origin.
    assert_eq!(
        ends(rect),
        [
            (0.0, 0.0),
            (80.0, 0.0),
            (80.0, 30.0),
            (0.0, 30.0),
            (0.0, 0.0)
        ]
    );
    assert_eq!((drawing.width, drawing.height), (80.0, 30.0));
    assert_eq!(drawing.units.as_deref(), Some("millimetres"));
}

#[test]
fn inches_scale_to_millimetres() {
    let drawing = File::units(Some(1)).import(&rectangle_lines("0", 7, 0.0, 0.0, 2.0, 1.0));
    assert_near(drawing.width, 50.8);
    assert_near(drawing.height, 25.4);
    assert_eq!(drawing.paths[0].color, "#000000");
    assert_eq!(drawing.units.as_deref(), Some("inches"));
}

#[test]
fn unitless_files_ask_and_then_use_the_answer() {
    let file = File::units(None);
    let text = file.build(&rectangle_lines("0", 7, 0.0, 0.0, 4.0, 2.0));
    assert_eq!(
        import_dxf(text.as_bytes(), None, &geist()),
        Err(ImportError::NeedsUnits {
            width: 4.0,
            height: 2.0
        })
    );
    let unitless = File::units(Some(0)).build(&rectangle_lines("0", 7, 0.0, 0.0, 4.0, 2.0));
    assert!(matches!(
        import_dxf(unitless.as_bytes(), None, &geist()),
        Err(ImportError::NeedsUnits { .. })
    ));
    let drawing = import_dxf(text.as_bytes(), Some(LengthUnit::Inches), &geist()).unwrap();
    assert_near(drawing.width, 101.6);
    let drawing = import_dxf(text.as_bytes(), Some(LengthUnit::Millimeters), &geist()).unwrap();
    assert_near(drawing.width, 4.0);
    // A file that names its units ignores the fallback.
    let inches = File::units(Some(1)).build(&rectangle_lines("0", 7, 0.0, 0.0, 4.0, 2.0));
    let drawing = import_dxf(inches.as_bytes(), Some(LengthUnit::Millimeters), &geist()).unwrap();
    assert_near(drawing.width, 101.6);
}

#[test]
fn colours_resolve_by_layer_and_true_colour() {
    let file = File {
        layers: vec![("Engrave", 5), ("Hidden", -3)],
        ..mm()
    };
    let drawing = file.import(&[
        line("Engrave", 256, (0.0, 0.0), (10.0, 0.0)),
        line("Engrave", 3, (0.0, 5.0), (10.0, 5.0)),
        pairs(&[
            (0, "LINE"),
            (8, "Engrave"),
            (420, &0x12_3456.to_string()),
            (10, "0"),
            (20, "10"),
            (11, "10"),
            (21, "10"),
        ]),
        line("Hidden", 256, (0.0, 20.0), (10.0, 20.0)),
    ]);
    let colors: Vec<_> = drawing.paths.iter().map(|p| p.color.as_str()).collect();
    assert_eq!(colors, ["#0000ff", "#00ff00", "#123456"]);
}

#[test]
fn circles_arcs_and_bulges_become_curves() {
    let drawing = mm().import(&[
        circle((50.0, 50.0), 10.0, &[]),
        // A 90° arc from (100, 0) round to (90, 10).
        pairs(&[
            (0, "ARC"),
            (8, "0"),
            (10, "90"),
            (20, "0"),
            (40, "10"),
            (50, "0"),
            (51, "90"),
        ]),
        // A slot: two lines and two semicircle bulges.
        pairs(&[
            (0, "LWPOLYLINE"),
            (8, "0"),
            (90, "4"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (10, "20"),
            (20, "0"),
            (42, "1"),
            (10, "20"),
            (20, "10"),
            (10, "0"),
            (20, "10"),
            (42, "1"),
        ]),
    ]);
    assert_eq!(drawing.paths.len(), 3);
    let circle = &drawing.paths[0].subpaths[0];
    assert!(circle.closed);
    assert_eq!(circle.segments.len(), 4);
    let arc = &drawing.paths[1].subpaths[0];
    assert!(!arc.closed);
    let slot = &drawing.paths[2].subpaths[0];
    assert!(slot.closed);
    // Slot: line, 2 quarter arcs, line, 2 quarter arcs.
    assert_eq!(slot.segments.len(), 6);
    // Bounds: the slot's arcs reach 5 past x = 0 and x = 20, so the page
    // spans x -5..100 and y 0..60.
    assert_near(drawing.width, 105.0);
    assert_near(drawing.height, 60.0);
}

#[test]
fn ellipses_follow_their_axes() {
    let drawing = mm().import(&[pairs(&[
        (0, "ELLIPSE"),
        (8, "0"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (11, "0"),
        (21, "20"),
        (31, "0"),
        (40, "0.5"),
        (41, "0"),
        (42, &std::f64::consts::TAU.to_string()),
    ])]);
    let ellipse = &drawing.paths[0].subpaths[0];
    assert!(ellipse.closed);
    // Major axis 40 tall, minor 20 wide.
    assert_near(drawing.width, 20.0);
    assert_near(drawing.height, 40.0);
}

#[test]
fn splines_convert_to_cubics() {
    let drawing = mm().import(&[pairs(&[
        (0, "SPLINE"),
        (8, "0"),
        (70, "8"),
        (71, "3"),
        (72, "8"),
        (73, "4"),
        (40, "0"),
        (40, "0"),
        (40, "0"),
        (40, "0"),
        (40, "1"),
        (40, "1"),
        (40, "1"),
        (40, "1"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (10, "10"),
        (20, "20"),
        (30, "0"),
        (10, "30"),
        (20, "20"),
        (30, "0"),
        (10, "40"),
        (20, "0"),
        (30, "0"),
    ])]);
    let spline = &drawing.paths[0].subpaths[0];
    assert!(matches!(
        spline.segments[..],
        [ImportedSegment::Cubic { .. }]
    ));
    assert_near(drawing.width, 40.0);
    // The curve peaks at 3/4 of the control height.
    assert_near(drawing.height, 15.0);
}

#[test]
fn inserts_expand_blocks_with_their_transform() {
    let file = File {
        blocks: vec![(
            "Hole",
            (5.0, 5.0),
            vec![
                circle((5.0, 5.0), 2.0, &[(62, "0")]),
                line("0", 256, (5.0, 5.0), (9.0, 5.0)),
            ],
        )],
        layers: vec![("Holes", 1)],
        ..mm()
    };
    let drawing = file.import(&[
        pairs(&[
            (0, "INSERT"),
            (8, "Holes"),
            (62, "5"),
            (2, "Hole"),
            (10, "100"),
            (20, "0"),
            (41, "2"),
            (42, "2"),
            (50, "90"),
            (70, "2"),
            (44, "50"),
        ]),
        // An insert of a block that doesn't exist is left out.
        pairs(&[
            (0, "INSERT"),
            (8, "0"),
            (2, "Missing"),
            (10, "0"),
            (20, "0"),
        ]),
    ]);
    assert_eq!(drawing.skipped, 1);
    assert_eq!(drawing.paths.len(), 4);
    for path in &drawing.paths {
        assert_eq!(path.name, "Holes");
    }
    // BYBLOCK takes the insert's colour; BYLAYER on layer 0 takes the
    // insert's layer's.
    assert_eq!(drawing.paths[0].color, "#0000ff");
    assert_eq!(drawing.paths[1].color, "#ff0000");
    // Rotated 90°: the line from the block's base runs up 8 mm, and the
    // second column sits 50 mm up the rotated X axis.
    let line = &drawing.paths[1].subpaths[0];
    let [(x0, y0), (x1, y1)] = ends(line)[..] else {
        panic!()
    };
    assert_near(x1 - x0, 0.0);
    assert_near(y1 - y0, 8.0);
    assert_near(drawing.height, 50.0 + 8.0 + 4.0);
}

#[test]
fn unsupported_and_paper_space_entities_are_left_out_but_text_is_not() {
    let drawing = mm().import(&[
        line("0", 7, (0.0, 0.0), (10.0, 0.0)),
        pairs(&[
            (0, "TEXT"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (40, "5"),
            (1, "Hi"),
        ]),
        pairs(&[(0, "POINT"), (8, "0"), (10, "0"), (20, "0")]),
        pairs(&[
            (0, "LINE"),
            (67, "1"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (11, "500"),
            (21, "0"),
        ]),
        // A circle on a tilted plane can't be laid flat.
        circle((0.0, 0.0), 5.0, &[(210, "1"), (220, "0"), (230, "0")]),
    ]);
    // The TEXT imports; the POINT, the tilted circle and the paper-space line
    // don't.
    assert_eq!(drawing.paths.len(), 2);
    assert_eq!(drawing.skipped, 2);
}

#[test]
fn downward_extrusion_mirrors_x() {
    let drawing = mm().import(&[
        circle((10.0, 0.0), 1.0, &[(210, "0"), (220, "0"), (230, "-1")]),
        line("0", 7, (0.0, 0.0), (1.0, 0.0)),
    ]);
    // The circle is centred on x = -10, so the page runs from -11 to 1.
    assert_near(drawing.width, 12.0);
}

#[test]
fn rejects_invalid_and_empty_files() {
    assert!(matches!(
        import_dxf(b"not a dxf", None, &geist()),
        Err(ImportError::Invalid { .. } | ImportError::Empty(_))
    ));
    assert_eq!(
        import_dxf(mm().build(&[]).as_bytes(), None, &geist()),
        Err(ImportError::Empty(ImportFormat::Dxf))
    );
}

mod lettering;
