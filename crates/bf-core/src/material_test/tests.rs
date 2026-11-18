use std::collections::HashSet;

use super::*;
use crate::project::DesignDocument;

/// Geist Regular, cut down to printable ASCII (see `text/testdata`).
const GEIST: &[u8] = include_bytes!("../text/testdata/Geist-Regular-ascii.ttf");

fn db() -> Database {
    let mut db = Database::new();
    db.load_font_data(GEIST.to_vec());
    db
}

fn settings() -> TestGridSettings {
    let mut settings = TestGridSettings::default();
    settings.labels.font_family = "Geist".into();
    settings
}

const BED: BedSize = BedSize {
    width: 400.0,
    height: 400.0,
};

fn children(grid: &TestGrid) -> &[SceneObject] {
    match grid.objects.as_slice() {
        [SceneObject::Group { children, .. }] => children,
        other => panic!("expected one group, got {other:?}"),
    }
}

/// The text objects' strings and transforms.
fn texts(grid: &TestGrid) -> Vec<(&str, Transform)> {
    children(grid)
        .iter()
        .filter_map(|object| match object {
            SceneObject::Text {
                text, transform, ..
            } => Some((text.as_str(), *transform)),
            _ => None,
        })
        .collect()
}

fn layer<'a>(grid: &'a TestGrid, id: &str) -> &'a CutLayer {
    grid.layers
        .iter()
        .find(|layer| layer.id == id)
        .unwrap_or_else(|| panic!("no layer {id}"))
}

#[test]
fn a_five_by_five_grid_has_a_layer_per_cell_and_a_score_layer_of_labels() {
    let grid = test_grid(&db(), "test-1", &settings(), BED).unwrap();

    // The label layer cuts first, then the cells row by row.
    assert_eq!(grid.layers.len(), 26);
    let labels = &grid.layers[0];
    assert_eq!(labels.id, "test-1-labels");
    assert_eq!(labels.mode, LayerMode::Line);
    assert_eq!((labels.speed, labels.max_power), (3000.0, 10.0));
    let ids: Vec<&str> = grid.layers[1..].iter().map(|l| l.id.as_str()).collect();
    assert_eq!(
        &ids[..6],
        [
            "test-1-r0-c0",
            "test-1-r0-c1",
            "test-1-r0-c2",
            "test-1-r0-c3",
            "test-1-r0-c4",
            "test-1-r1-c0"
        ]
    );

    let speeds = [1000.0, 1500.0, 2000.0, 2500.0, 3000.0];
    let powers = [20.0, 40.0, 60.0, 80.0, 100.0];
    for (row, speed) in speeds.iter().enumerate() {
        for (column, power) in powers.iter().enumerate() {
            let cell = layer(&grid, &cell_layer_id("test-1", row, column));
            assert_eq!(cell.speed, *speed);
            assert_eq!(cell.max_power, *power);
            assert_eq!(cell.min_power, 0.0);
            assert_eq!(cell.mode, LayerMode::Fill);
            assert_eq!(cell.line_interval, 0.1);
            assert_eq!(cell.passes, 1);
            assert!(cell.air_assist && cell.output);
            assert_eq!(cell.name, format!("{speed} mm/min · {power}%"));
        }
    }
    let colors: HashSet<&str> = grid.layers.iter().map(|l| l.color.as_str()).collect();
    assert_eq!(colors.len(), 26, "every layer has its own colour");

    // One square per cell on its layer, rows top to bottom.
    let cells: Vec<_> = children(&grid)
        .iter()
        .filter_map(|object| match object {
            SceneObject::Rect {
                layer_id,
                transform,
                width,
                height,
                ..
            } => Some((
                layer_id.clone(),
                transform[4],
                transform[5],
                *width,
                *height,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(cells.len(), 25);
    for (layer_id, _, _, width, height) in &cells {
        assert!(layer_id.starts_with("test-1-r"));
        assert_eq!((*width, *height), (10.0, 10.0));
    }
    let (_, x00, y00, _, _) = &cells[0];
    let (_, x01, y01, _, _) = &cells[1];
    let (_, x10, y10, _, _) = &cells[5];
    assert_eq!(
        (x01 - x00, y01 - y00),
        (12.0, 0.0),
        "cell + gap to the right"
    );
    assert_eq!(
        (x10 - x00, y10 - y00),
        (0.0, -12.0),
        "the next speed is a row down"
    );

    // Labels: each speed beside its row, each power above its column, the
    // axis titles and a caption, all text on the label layer.
    let labels = texts(&grid);
    let strings: Vec<&str> = labels.iter().map(|(text, _)| *text).collect();
    assert_eq!(
        strings,
        [
            "1000",
            "1500",
            "2000",
            "2500",
            "3000",
            "20",
            "40",
            "60",
            "80",
            "100",
            "Power (%)",
            "Speed (mm/min)",
            "Fill 0.1 mm, 1 pass"
        ]
    );
    for object in children(&grid) {
        if let SceneObject::Text {
            layer_id, outline, ..
        } = object
        {
            assert_eq!(layer_id, "test-1-labels");
            assert!(!outline.is_empty(), "labels keep their outlines");
        }
    }
    for row in 0..5 {
        let (_, t) = labels[row];
        let (_, _, cell_y, _, _) = &cells[row * 5];
        assert!(t[4] < *x00, "speed labels are left of the grid");
        assert_eq!(t[5], cell_y + 3.5, "centred on the row's 10 mm cell");
    }
    for column in 0..5 {
        let (_, t) = labels[5 + column];
        let (_, cell_x, cell_y, _, _) = &cells[column];
        assert_eq!(t[4], cell_x + 5.0, "centred over the column");
        assert!(t[5] > cell_y + 10.0, "above the top row");
    }
    let (_, speed_title) = labels[11];
    assert_eq!(&speed_title[..4], [0.0, 1.0, -1.0, 0.0], "reads upwards");

    // The whole test starts at the origin and is a valid document.
    assert!(grid.width > 60.0 && grid.height > 60.0);
    let document = DesignDocument {
        version: 1,
        units: "mm".into(),
        layers: grid.layers.clone(),
        objects: grid.objects.clone(),
    };
    document.validate().unwrap();
    assert_eq!(grid.label_font, "Geist");
}

#[test]
fn the_test_starts_at_its_origin_labels_included() {
    let grid = test_grid(&db(), "t", &settings(), BED).unwrap();
    let mut points = Vec::new();
    for object in children(&grid) {
        match object {
            SceneObject::Rect {
                transform, width, ..
            } => {
                points.push(Point {
                    x: transform[4],
                    y: transform[5],
                });
                points.push(Point {
                    x: transform[4] + width,
                    y: transform[5] + width,
                });
            }
            SceneObject::Text {
                transform, outline, ..
            } => points.extend(outline_points(outline).map(|p| apply(transform, p))),
            _ => unreachable!(),
        }
    }
    let (min, max) = bounds(points).unwrap();
    assert!(
        (min.x - 10.0).abs() < 1e-9 && (min.y - 10.0).abs() < 1e-9,
        "{min:?}"
    );
    assert!((max.x - min.x - grid.width).abs() < 1e-9);
    assert!((max.y - min.y - grid.height).abs() < 1e-9);
}

#[test]
fn a_grid_too_big_for_the_bed_says_how_big_it_is() {
    let mut big = settings();
    big.cell_size = 80.0;
    let error = test_grid(&db(), "t", &big, BED).unwrap_err().to_string();
    assert!(error.starts_with("The test is "), "{error}");
    assert!(error.contains("runs off the 400 × 400 mm bed"), "{error}");

    let mut moved = settings();
    moved.origin = Point { x: 380.0, y: 10.0 };
    assert!(test_grid(&db(), "t", &moved, BED).is_err());
    moved.origin = Point { x: -1.0, y: 10.0 };
    assert!(
        test_grid(&db(), "t", &moved, BED)
            .unwrap_err()
            .to_string()
            .contains("off the bed")
    );
}

#[test]
fn one_step_uses_the_first_value_and_line_mode_says_so() {
    let mut one = settings();
    one.speed = ValueRange {
        from: 300.0,
        to: 900.0,
        steps: 1,
    };
    one.power.steps = 3;
    one.mode = LayerMode::Line;
    one.passes = 2;
    let grid = test_grid(&db(), "t", &one, BED).unwrap();
    assert_eq!(grid.layers.len(), 4);
    let speeds: Vec<f64> = grid.layers[1..].iter().map(|l| l.speed).collect();
    let powers: Vec<f64> = grid.layers[1..].iter().map(|l| l.max_power).collect();
    assert_eq!(speeds, [300.0; 3]);
    assert_eq!(powers, [20.0, 60.0, 100.0]);
    assert!(
        grid.layers[1..]
            .iter()
            .all(|l| l.mode == LayerMode::Line && l.passes == 2)
    );
    assert!(
        texts(&grid)
            .iter()
            .any(|(text, _)| *text == "Line, 2 passes")
    );
}

#[test]
fn values_are_rounded_to_what_the_layer_panel_shows() {
    let range = ValueRange {
        from: 10.0,
        to: 20.0,
        steps: 4,
    };
    assert_eq!(range.values(1), [10.0, 13.3, 16.7, 20.0]);
    assert_eq!(range.values(0), [10.0, 13.0, 17.0, 20.0]);
    assert_eq!(number(13.3), "13.3");
    assert_eq!(number(1500.0), "1500");
}

#[test]
fn settings_that_cant_be_cut_are_refused() {
    type Change = fn(&mut TestGridSettings);
    let cases: [(Change, &str); 8] = [
        (|s| s.speed.steps = 0, "speed steps"),
        (|s| s.power.steps = MAX_STEPS + 1, "power steps"),
        (|s| s.speed.to = 0.0, "Speeds must be above zero"),
        (|s| s.power.to = 120.0, "Power is 0–100%"),
        (|s| s.cell_size = 0.0, "Cell size"),
        (|s| s.gap = -1.0, "gap"),
        (|s| s.passes = 0, "Passes"),
        (|s| s.labels.power = 101.0, "Label power"),
    ];
    for (change, expected) in cases {
        let mut bad = settings();
        change(&mut bad);
        let error = test_grid(&db(), "t", &bad, BED).unwrap_err().to_string();
        assert!(
            error.contains(expected),
            "{error} should mention {expected}"
        );
    }
    assert!(test_grid(&db(), "", &settings(), BED).is_err());
}
