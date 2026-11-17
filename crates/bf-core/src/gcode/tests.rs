//! Snapshot tests of generated GCode, each checked on the simulator with
//! `bf_sim::check`. Set `UPDATE_SNAPSHOTS=1` to rewrite the snapshots in
//! `tests/fixtures/gcode/` after an intended change, then review the diff.

use std::path::PathBuf;

use bf_sim::{Profile, Violation};

use super::*;
use crate::machine::BedSize;
use crate::project::{SceneObject, Segment, Subpath, Transform};

fn translate(x: f64, y: f64) -> Transform {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

fn rect(id: &str, layer: &str, x: f64, y: f64, width: f64, height: f64) -> SceneObject {
    SceneObject::Rect {
        id: id.into(),
        name: id.into(),
        layer_id: layer.into(),
        transform: translate(x, y),
        width,
        height,
        corner_radius: 0.0,
    }
}

fn triangle(id: &str, layer: &str, x: f64, y: f64) -> SceneObject {
    let point = |x, y| Point { x, y };
    SceneObject::Path {
        id: id.into(),
        name: id.into(),
        layer_id: layer.into(),
        transform: translate(x, y),
        subpaths: vec![Subpath {
            start: point(0.0, 0.0),
            segments: vec![
                Segment::Line {
                    to: point(20.0, 0.0),
                },
                Segment::Line {
                    to: point(10.0, 15.0),
                },
            ],
            closed: true,
        }],
    }
}

fn layer(id: &str, name: &str) -> CutLayer {
    CutLayer::new(id, name, "#0000ff")
}

fn document(layers: Vec<CutLayer>, objects: Vec<SceneObject>) -> DesignDocument {
    DesignDocument {
        version: 1,
        units: "mm".into(),
        layers,
        objects,
    }
}

/// The Phecda-like profile the simulator's diode laser matches.
fn profile() -> MachineProfile {
    MachineProfile {
        id: "test".into(),
        name: "Test".into(),
        bed: BedSize {
            width: 400.0,
            height: 400.0,
        },
        max_spindle: 1000.0,
        laser_mode: true,
        ..MachineProfile::default()
    }
}

fn absolute() -> JobPlacement {
    JobPlacement::default()
}

/// Compares `gcode` with `tests/fixtures/gcode/<name>.gcode`, and checks
/// it on the simulated diode laser.
fn snapshot(name: &str, gcode: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gcode")
        .join(format!("{name}.gcode"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, gcode).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        gcode, expected,
        "{name} changed; rerun with UPDATE_SNAPSHOTS=1 if that's intended"
    );
    let report = bf_sim::check(gcode, Profile::diode_laser());
    assert!(report.passed(), "{name}: {report}");
}

#[test]
fn line_layer_snapshot() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![
            rect("frame", "cut", 10.0, 10.0, 60.0, 40.0),
            triangle("hole", "cut", 30.0, 20.0),
        ],
    );
    let job = generate(&doc, &profile(), absolute(), None).unwrap();
    snapshot("line", &job.gcode);
    assert!(job.summary.inside_bed);
    assert_eq!(
        job.summary.bounds,
        JobBounds {
            min: Point { x: 10.0, y: 10.0 },
            max: Point { x: 70.0, y: 50.0 },
        }
    );
    // The hole is cut before the frame around it.
    let first_cut = job.lines().find(|l| l.starts_with("G1")).unwrap();
    assert!(first_cut.starts_with("G1 X50 Y20"), "{first_cut}");
}

#[test]
fn multi_pass_snapshot() {
    let mut cut = layer("cut", "Cut deep");
    cut.passes = 3;
    cut.speed = 300.0;
    cut.max_power = 100.0;
    let doc = document(vec![cut], vec![rect("r", "cut", 5.0, 5.0, 20.0, 10.0)]);
    let job = generate(&doc, &profile(), absolute(), None).unwrap();
    snapshot("multi-pass", &job.gcode);
    let cuts = job.lines().filter(|l| l.starts_with("G1")).count();
    assert_eq!(cuts, 12, "4 edges, 3 times");
    assert!((job.summary.cut_length - 180.0).abs() < 1e-9);
    // F and S are set once: the head never leaves the outline.
    assert_eq!(job.lines().filter(|l| sets_feed(l)).count(), 1);
    assert_eq!(job.lines().filter(|l| l.contains(" S1000")).count(), 1);
}

#[test]
fn air_assist_and_constant_power_snapshot() {
    let mut score = layer("score", "Score");
    score.air_assist = false;
    score.max_power = 20.0;
    score.speed = 3000.0;
    let mut cut = layer("cut", "Cut");
    cut.max_power = 80.0;
    cut.speed = 600.0;
    let mut mark = layer("mark", "Mark");
    mark.constant_power = true;
    mark.max_power = 10.0;
    mark.air_assist = false;
    let doc = document(
        vec![score, cut, mark],
        vec![
            rect("a", "score", 20.0, 20.0, 10.0, 10.0),
            rect("b", "cut", 50.0, 20.0, 10.0, 10.0),
            rect("c", "mark", 80.0, 20.0, 10.0, 10.0),
        ],
    );
    let job = generate(&doc, &profile(), absolute(), None).unwrap();
    snapshot("air-assist", &job.gcode);
    let commands: Vec<_> = job
        .lines()
        .filter(|l| l.starts_with('M'))
        .map(String::from)
        .collect();
    assert_eq!(commands, ["M4 S0", "M8", "M9", "M3 S0", "M5", "M9"]);
}

#[test]
fn fill_layer_snapshot() {
    let mut fill = layer("fill", "Engrave");
    fill.mode = crate::project::LayerMode::Fill;
    fill.line_interval = 1.0;
    fill.overscan = 1.0;
    fill.speed = 3000.0;
    fill.max_power = 30.0;
    // Two 10 x 5 mm blocks 1.5 mm apart: closer than two overscans, so each
    // row crosses the gap with the laser off rather than stopping.
    let doc = document(
        vec![fill],
        vec![
            rect("a", "fill", 10.0, 10.0, 10.0, 5.0),
            rect("b", "fill", 21.5, 10.0, 10.0, 5.0),
        ],
    );
    let job = generate(&doc, &profile(), absolute(), None).unwrap();
    snapshot("fill", &job.gcode);
    // Five rows, each burning both blocks.
    assert!((job.summary.cut_length - 100.0).abs() < 1e-9);
    // Per row: 1 mm overscan in, the 1.5 mm gap and 1 mm out, laser off.
    let off: f64 = 5.0 * (1.0 + 1.5 + 1.0);
    // And the rapids: to the first row's overscan, then 1 mm up per row.
    let rapids = 9f64.hypot(10.5) + 4.0;
    assert!(
        (job.summary.travel_length - off - rapids).abs() < 1e-9,
        "{}",
        job.summary.travel_length
    );
    // The overscan is part of where the head goes.
    assert_eq!(job.summary.bounds.min.x, 9.0);
    assert_eq!(job.summary.bounds.max.x, 32.5);
    // S0 crossings stay at cutting speed: F is set once.
    assert_eq!(job.lines().filter(|l| sets_feed(l)).count(), 1);
    assert_eq!(job.lines().filter(|l| l.starts_with("G0")).count(), 5);
}

#[test]
fn output_is_deterministic() {
    let doc = document(
        vec![layer("cut", "Cut")],
        (0..20)
            .map(|i| {
                rect(
                    &format!("r{i}"),
                    "cut",
                    (i * 13 % 7) as f64 * 20.0,
                    i as f64 * 9.0,
                    8.0,
                    5.0,
                )
            })
            .collect(),
    );
    let a = generate(&doc, &profile(), absolute(), None).unwrap();
    let b = generate(&doc.clone(), &profile(), absolute(), None).unwrap();
    assert_eq!(a, b);
}

#[test]
fn the_air_command_comes_from_the_profile() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 1.0, 1.0, 2.0, 2.0)],
    );
    let mut mist = profile();
    mist.air_assist = "M7".into();
    let job = generate(&doc, &mist, absolute(), None).unwrap();
    assert!(job.lines().any(|l| l == "M7"));
    assert!(!job.lines().any(|l| l == "M8"));
}

#[test]
fn power_scales_to_max_spindle() {
    let mut cut = layer("cut", "Cut");
    cut.max_power = 45.0;
    let doc = document(vec![cut], vec![rect("r", "cut", 1.0, 1.0, 2.0, 2.0)]);
    let mut machine = profile();
    machine.max_spindle = 255.0;
    let job = generate(&doc, &machine, absolute(), None).unwrap();
    assert!(job.gcode.contains(" S114.75 "), "{}", job.gcode);
}

#[test]
fn current_position_puts_the_anchor_under_the_head() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 10.0, 10.0, 40.0, 20.0)],
    );
    let head = Point { x: 100.0, y: 200.0 };
    let place = |anchor| JobPlacement {
        start_from: StartFrom::CurrentPosition,
        anchor,
    };
    let bounds = |anchor| {
        generate(&doc, &profile(), place(anchor), Some(head))
            .unwrap()
            .summary
            .bounds
    };
    assert_eq!(
        bounds(JobAnchor::Center),
        JobBounds {
            min: Point { x: 80.0, y: 190.0 },
            max: Point { x: 120.0, y: 210.0 },
        }
    );
    assert_eq!(bounds(JobAnchor::TopRight).max, head);
    assert_eq!(bounds(JobAnchor::BottomLeft).min, head);
    let top = bounds(JobAnchor::Top);
    assert_eq!((top.min.x, top.max.x, top.max.y), (80.0, 120.0, 200.0));

    let refused = generate(&doc, &profile(), place(JobAnchor::Center), None).unwrap_err();
    assert!(
        refused.to_string().contains("connected machine"),
        "{refused}"
    );
}

#[test]
fn user_origin_comes_from_the_profile() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 10.0, 10.0, 40.0, 20.0)],
    );
    let placement = JobPlacement {
        start_from: StartFrom::UserOrigin,
        anchor: JobAnchor::BottomLeft,
    };
    let refused = generate(&doc, &profile(), placement, None).unwrap_err();
    assert!(refused.to_string().contains("No user origin"), "{refused}");

    let mut machine = profile();
    machine.user_origin = Some(Point { x: 5.0, y: 7.0 });
    let job = generate(&doc, &machine, placement, Some(Point { x: 99.0, y: 99.0 })).unwrap();
    assert_eq!(job.summary.bounds.min, Point { x: 5.0, y: 7.0 });
    assert!(
        job.gcode
            .contains("; Start from: user origin, job origin bottom left")
    );
    let report = bf_sim::check(&job.gcode, Profile::diode_laser());
    assert!(report.passed(), "{report}");
}

#[test]
fn jobs_off_the_bed_are_flagged() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 380.0, 10.0, 40.0, 20.0)],
    );
    let job = generate(&doc, &profile(), absolute(), None).unwrap();
    assert!(!job.summary.inside_bed);
    let warning = job.summary.warning.unwrap();
    assert_eq!(
        warning,
        "The job goes up to 20 mm outside the 400 × 400 mm bed."
    );
    assert!(job.gcode.contains(&format!("; Warning: {warning}")));
    let report = bf_sim::check(&job.gcode, Profile::diode_laser());
    assert!(
        report
            .violations
            .iter()
            .any(|v| matches!(v, Violation::OutsideBed { .. })),
        "{report}"
    );

    let edge = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 0.0, 0.0, 400.0, 400.0)],
    );
    assert!(
        generate(&edge, &profile(), absolute(), None)
            .unwrap()
            .summary
            .inside_bed
    );
}

#[test]
fn other_origin_corners_mirror_the_design() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 10.0, 20.0, 30.0, 40.0)],
    );
    let mut machine = profile();
    machine.origin = OriginCorner::RearRight;
    let job = generate(&doc, &machine, absolute(), None).unwrap();
    assert_eq!(
        job.summary.bounds,
        JobBounds {
            min: Point { x: 360.0, y: 340.0 },
            max: Point { x: 390.0, y: 380.0 },
        }
    );
    // The anchor is still as seen in the editor.
    let head = Point { x: 0.0, y: 0.0 };
    let placement = JobPlacement {
        start_from: StartFrom::CurrentPosition,
        anchor: JobAnchor::BottomLeft,
    };
    let job = generate(&doc, &machine, placement, Some(head)).unwrap();
    assert_eq!(
        job.summary.bounds,
        JobBounds {
            min: Point { x: -30.0, y: -40.0 },
            max: head,
        }
    );
}

#[test]
fn empty_and_invalid_documents_are_refused() {
    let mut off = layer("cut", "Cut");
    off.output = false;
    let doc = document(vec![off], vec![rect("r", "cut", 1.0, 1.0, 2.0, 2.0)]);
    let error = generate(&doc, &profile(), absolute(), None).unwrap_err();
    assert!(error.to_string().contains("nothing to cut"), "{error}");

    let orphan = document(vec![], vec![rect("r", "missing", 1.0, 1.0, 2.0, 2.0)]);
    assert!(generate(&orphan, &profile(), absolute(), None).is_err());

    let mut marlin = profile();
    marlin.firmware = crate::Firmware::Marlin;
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 1.0, 1.0, 2.0, 2.0)],
    );
    let error = generate(&doc, &marlin, absolute(), None).unwrap_err();
    assert!(error.to_string().contains("Marlin"), "{error}");
}

#[test]
fn summary_counts_command_lines() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 1.0, 1.0, 2.0, 2.0)],
    );
    let job = generate(&doc, &profile(), absolute(), None).unwrap();
    let commands = job.lines().filter(|l| !l.starts_with(';')).count();
    assert_eq!(job.summary.lines as usize, commands);
}

/// A rectangle and a triangle off its corner, so the rubber band differs
/// from the bounding box.
fn frame_document() -> DesignDocument {
    document(
        vec![layer("cut", "Cut")],
        vec![
            rect("r", "cut", 10.0, 10.0, 40.0, 20.0),
            triangle("t", "cut", 60.0, 40.0),
        ],
    )
}

/// Every placement: absolute, and each anchor from the head and from the
/// user origin.
fn placements() -> Vec<JobPlacement> {
    const ANCHORS: [JobAnchor; 9] = [
        JobAnchor::TopLeft,
        JobAnchor::Top,
        JobAnchor::TopRight,
        JobAnchor::Left,
        JobAnchor::Center,
        JobAnchor::Right,
        JobAnchor::BottomLeft,
        JobAnchor::Bottom,
        JobAnchor::BottomRight,
    ];
    let mut all = vec![absolute()];
    for start_from in [StartFrom::CurrentPosition, StartFrom::UserOrigin] {
        all.extend(ANCHORS.map(|anchor| JobPlacement { start_from, anchor }));
    }
    all
}

const HEAD: Point = Point { x: 150.0, y: 200.0 };

fn framing_profile() -> MachineProfile {
    MachineProfile {
        user_origin: Some(Point { x: 120.0, y: 80.0 }),
        ..profile()
    }
}

/// The X Y of every `G0`/`G1` in `gcode`, read back from the text.
fn visited(gcode: &str) -> Vec<Point> {
    gcode
        .lines()
        .filter(|line| line.starts_with("G0 ") || line.starts_with("G1 "))
        .map(|line| {
            let axis = |name: char| {
                line.split(' ')
                    .find_map(|word| word.strip_prefix(name))
                    .and_then(|value| value.parse().ok())
                    .unwrap_or_else(|| panic!("no {name} in {line}"))
            };
            Point {
                x: axis('X'),
                y: axis('Y'),
            }
        })
        .collect()
}

#[test]
fn frame_path_is_the_job_bounds_for_every_placement() {
    let doc = frame_document();
    let machine = framing_profile();
    for placement in placements() {
        let job = generate(&doc, &machine, placement, Some(HEAD)).unwrap();
        let framed = frame(&doc, &machine, placement, Some(HEAD), FrameShape::Box).unwrap();
        let burned = JobBounds::around(visited(&job.gcode)).unwrap();
        assert_eq!(framed.bounds, job.summary.bounds, "{placement:?}");
        assert_eq!(
            JobBounds::around(framed.path.iter().copied()),
            Some(job.summary.bounds),
            "{placement:?}"
        );
        let JobBounds { min, max } = burned;
        let corners = [
            min,
            Point { x: max.x, y: min.y },
            max,
            Point { x: min.x, y: max.y },
            min,
        ];
        assert_eq!(visited(&framed.gcode)[..5], corners, "{placement:?}");
        assert_eq!(
            JobBounds::around(visited(&framed.gcode)),
            Some(burned),
            "{placement:?}"
        );
    }
}

#[test]
fn rubber_band_frame_is_the_convex_hull_for_every_placement() {
    let doc = frame_document();
    let machine = framing_profile();
    for placement in placements() {
        let job = generate(&doc, &machine, placement, Some(HEAD)).unwrap();
        let framed = frame(
            &doc,
            &machine,
            placement,
            Some(HEAD),
            FrameShape::RubberBand,
        )
        .unwrap();
        let mut hull = convex_hull(visited(&job.gcode));
        // 3 of the rectangle's corners and 2 of the triangle's.
        assert_eq!(hull.len(), 5, "{placement:?}: {hull:?}");
        hull.push(hull[0]);
        assert_eq!(
            visited(&framed.gcode)[..hull.len()],
            hull[..],
            "{placement:?}"
        );
        assert_eq!(framed.bounds, job.summary.bounds, "{placement:?}");
    }
}

#[test]
fn frames_return_to_the_start_when_it_matters() {
    let doc = frame_document();
    let machine = framing_profile();
    let here = JobPlacement {
        start_from: StartFrom::CurrentPosition,
        anchor: JobAnchor::Center,
    };
    let framed = frame(&doc, &machine, here, Some(HEAD), FrameShape::Box).unwrap();
    assert_eq!(visited(&framed.gcode).last(), Some(&HEAD));
    let origin = JobPlacement {
        start_from: StartFrom::UserOrigin,
        anchor: JobAnchor::TopRight,
    };
    let framed = frame(&doc, &machine, origin, None, FrameShape::Box).unwrap();
    assert_eq!(visited(&framed.gcode).last(), machine.user_origin.as_ref());
    // An absolute job doesn't depend on where the head is, so it stays put.
    let framed = frame(&doc, &machine, absolute(), None, FrameShape::Box).unwrap();
    assert_eq!(visited(&framed.gcode).len(), 5);

    let refused = frame(&doc, &machine, here, None, FrameShape::Box).unwrap_err();
    assert!(
        refused.to_string().contains("connected machine"),
        "{refused}"
    );
}

#[test]
fn frames_pass_the_simulator_with_the_job_bounds() {
    let doc = frame_document();
    let mut low = framing_profile();
    low.frame_power = 1.0;
    for machine in [framing_profile(), low] {
        for placement in placements() {
            let job = generate(&doc, &machine, placement, Some(HEAD)).unwrap();
            let job_report = bf_sim::check(&job.gcode, Profile::diode_laser());
            for shape in [FrameShape::Box, FrameShape::RubberBand] {
                let framed = frame(&doc, &machine, placement, Some(HEAD), shape).unwrap();
                let report = bf_sim::check(&framed.gcode, Profile::diode_laser());
                let what = format!("{placement:?} {shape:?} at {}%", machine.frame_power);
                assert!(report.passed(), "{what}: {report}");
                assert_eq!(report.bounds, job_report.bounds, "{what}");
                assert!(
                    (report.max_power - machine.frame_power / 100.0).abs() < 1e-9,
                    "{what}: {}",
                    report.max_power
                );
                assert_eq!(framed.lines().last(), Some("M5"), "{what}");
            }
        }
    }
}

#[test]
fn frame_snapshot_keeps_the_laser_off() {
    let doc = frame_document();
    let framed = frame(&doc, &profile(), absolute(), None, FrameShape::Box).unwrap();
    snapshot("frame", &framed.gcode);
    assert!(framed.gcode.contains("; Frame: bounding box, laser off\n"));
    let commands: Vec<_> = framed.lines().filter(|l| !l.starts_with(';')).collect();
    assert_eq!(commands[..2], ["G21 G90", "M5 S0"]);
    assert_eq!(commands.last(), Some(&"M5"));
    assert_eq!(framed.lines as usize, commands.len());
    for line in &commands {
        assert!(!line.contains("M3") && !line.contains("M4"), "{line}");
        assert!(
            !line.contains(" S") || line.ends_with(" S0"),
            "the laser is never given power: {line}"
        );
    }
}

#[test]
fn low_power_frame_snapshot() {
    let doc = frame_document();
    let mut machine = framing_profile();
    machine.frame_power = 1.0;
    let here = JobPlacement {
        start_from: StartFrom::CurrentPosition,
        anchor: JobAnchor::Center,
    };
    let framed = frame(&doc, &machine, here, Some(HEAD), FrameShape::RubberBand).unwrap();
    snapshot("frame-low-power", &framed.gcode);
    assert!(
        framed
            .gcode
            .contains("; Frame: rubber band, low power 1% (M4 S10)\n")
    );
    let commands: Vec<_> = framed.lines().filter(|l| !l.starts_with(';')).collect();
    assert_eq!(commands[1], "M4 S0");
    assert_eq!(commands.last(), Some(&"M5"));
    for line in &commands {
        assert!(!line.contains("M3"), "{line}");
        if let Some((_, s)) = line.split_once(" S") {
            assert!(
                s == "0" || s.starts_with("10 "),
                "only S0 or the low S: {line}"
            );
        }
    }
}

#[test]
fn frame_power_is_capped() {
    let doc = frame_document();
    let mut machine = profile();
    machine.frame_power = 50.0;
    assert!(machine.validate().is_err());
    let framed = frame(&doc, &machine, absolute(), None, FrameShape::Box).unwrap();
    assert!(framed.gcode.contains(" S50 F3000"), "{}", framed.gcode);
}

/// A move read back from GCode: `G0` or `G1`, where to, and the `S` and `F`
/// in force (modal, as GRBL keeps them).
#[derive(Debug, PartialEq)]
struct Written {
    cut: bool,
    x: String,
    y: String,
    s: String,
    f: String,
}

/// Whether a line of code (not a comment, which may say "Forge") sets F.
fn sets_feed(line: &str) -> bool {
    !line.starts_with(';') && line.contains(" F")
}

fn written(gcode: &str) -> Vec<Written> {
    let (mut s, mut f) = ("0".to_string(), String::new());
    let mut moves = Vec::new();
    for line in gcode.lines().filter(|l| !l.starts_with(';')) {
        let words: Vec<_> = line.split(' ').collect();
        let word = |name: char| {
            words
                .iter()
                .find_map(|w| w.strip_prefix(name))
                .map(String::from)
        };
        if let Some(value) = word('S') {
            s = value;
        }
        if let Some(value) = word('F') {
            f = value;
        }
        if words[0] == "G0" || words[0] == "G1" {
            moves.push(Written {
                cut: words[0] == "G1",
                x: word('X').unwrap(),
                y: word('Y').unwrap(),
                s: s.clone(),
                f: f.clone(),
            });
        }
    }
    moves
}

#[test]
fn the_toolpath_is_the_move_list_the_gcode_is_written_from() {
    let mut deep = layer("deep", "Deep");
    deep.passes = 2;
    deep.max_power = 90.0;
    deep.speed = 400.0;
    let mut doc = frame_document();
    doc.layers.push(deep);
    doc.objects.push(rect("d", "deep", 100.0, 60.0, 15.0, 25.0));

    for origin in [
        OriginCorner::FrontLeft,
        OriginCorner::FrontRight,
        OriginCorner::RearLeft,
        OriginCorner::RearRight,
    ] {
        let machine = MachineProfile {
            origin,
            ..framing_profile()
        };
        for placement in placements() {
            let what = format!("{origin:?} {placement:?}");
            let job = generate(&doc, &machine, placement, Some(HEAD)).unwrap();
            let preview = toolpath(&doc, &machine, placement, Some(HEAD)).unwrap();
            let placed = PlacedJob::new(&doc, &machine, placement, Some(HEAD)).unwrap();

            // Each preview move, placed on the machine, is the GCode's move.
            let (mut s, mut f) = ("0".to_string(), String::new());
            let expected: Vec<_> = preview
                .plan
                .moves()
                .map(|step| {
                    let (cut, to) = match *step {
                        Move::Rapid { to } => {
                            s = "0".to_string();
                            (false, to)
                        }
                        Move::Cut { to, power, speed } => {
                            s = number(power / 100.0 * machine.max_spindle);
                            f = number(speed);
                            (true, to)
                        }
                    };
                    let at = placed.to_work(to);
                    Written {
                        cut,
                        x: number(at.x),
                        y: number(at.y),
                        s: s.clone(),
                        f: f.clone(),
                    }
                })
                .collect();
            assert_eq!(written(&job.gcode), expected, "{what}");

            // Placing mirrors and moves, so lengths (and so times) agree.
            assert!(
                (preview.plan.cut_length() - job.summary.cut_length).abs() < 1e-9,
                "{what}"
            );
            assert!(
                (preview.plan.travel_length() - job.summary.travel_length).abs() < 1e-9,
                "{what}"
            );
            let ids: Vec<_> = preview.plan.layers.iter().map(|l| &l.layer_id).collect();
            assert_eq!(ids, ["cut", "deep"], "{what}");
        }
    }
}

#[test]
fn the_toolpath_is_in_document_coordinates() {
    let doc = document(
        vec![layer("cut", "Cut")],
        vec![rect("r", "cut", 10.0, 20.0, 30.0, 40.0)],
    );
    let mut machine = profile();
    machine.origin = OriginCorner::RearRight;
    machine.max_rate.x = 6000.0;
    machine.max_rate.y = 5000.0;
    let here = JobPlacement {
        start_from: StartFrom::CurrentPosition,
        anchor: JobAnchor::Center,
    };
    let preview = toolpath(&doc, &machine, here, Some(HEAD)).unwrap();
    let points: Vec<_> = preview.plan.moves().map(move_to).collect();
    assert_eq!(
        JobBounds::around(points),
        Some(JobBounds {
            min: Point { x: 10.0, y: 20.0 },
            max: Point { x: 40.0, y: 60.0 },
        })
    );
    // Planned from the job origin, as the GCode is.
    assert_eq!(preview.plan.start, Point { x: 25.0, y: 40.0 });
    assert_eq!(preview.rapid_rate, 5000.0);
    let json = serde_json::to_value(&preview).unwrap();
    assert_eq!(json["rapidRate"], 5000.0);
    assert_eq!(json["plan"]["layers"][0]["layerId"], "cut");

    // The head's position only moves the job on the bed, so the preview
    // doesn't need it (or a user origin).
    assert_eq!(toolpath(&doc, &machine, here, None).unwrap(), preview);
    let from_origin = JobPlacement {
        start_from: StartFrom::UserOrigin,
        ..here
    };
    assert_eq!(machine.user_origin, None);
    assert_eq!(
        toolpath(&doc, &machine, from_origin, None).unwrap(),
        preview
    );
    assert!(toolpath(&document(vec![], vec![]), &machine, absolute(), None).is_err());
}

#[test]
fn a_run_plan_says_which_line_makes_each_move_and_where_it_is() {
    let mut score = layer("score", "Score");
    score.passes = 2;
    let doc = document(
        vec![score, layer("cut", "Cut")],
        vec![
            rect("a", "score", 20.0, 20.0, 10.0, 10.0),
            triangle("b", "cut", 50.0, 30.0),
        ],
    );
    let head = Point { x: 150.0, y: 120.0 };
    for corner in [
        OriginCorner::FrontLeft,
        OriginCorner::FrontRight,
        OriginCorner::RearLeft,
        OriginCorner::RearRight,
    ] {
        let mut machine = profile();
        machine.origin = corner;
        let placement = JobPlacement {
            start_from: StartFrom::CurrentPosition,
            anchor: JobAnchor::TopLeft,
        };
        let run = run(&doc, &machine, placement, Some(head), "Tag").unwrap();

        // The same GCode and toolpath as generating and previewing it.
        assert_eq!(
            run.job,
            generate(&doc, &machine, placement, Some(head)).unwrap()
        );
        assert_eq!(
            run.plan.toolpath,
            toolpath(&doc, &machine, placement, Some(head)).unwrap()
        );
        assert_eq!(run.plan.name, "Tag");

        // Each move's line is the G0/G1 that goes where the move goes.
        let commands: Vec<String> = run
            .job
            .lines()
            .filter_map(crate::machine::job_line)
            .collect();
        let moves: Vec<&Move> = run.plan.toolpath.plan.moves().collect();
        assert_eq!(run.plan.move_lines.len(), moves.len());
        assert!(run.plan.move_lines.windows(2).all(|w| w[0] < w[1]));
        for (step, &line) in moves.iter().zip(&run.plan.move_lines) {
            let command = &commands[line as usize - 1];
            let work = run.plan.placement.to_work(move_to(step));
            let expected = format!("X{} Y{}", number(work.x), number(work.y));
            let kind = if matches!(step, Move::Rapid { .. }) {
                "G0 "
            } else {
                "G1 "
            };
            assert!(command.starts_with(kind), "{corner:?}: {command}");
            assert!(
                command.contains(&expected),
                "{corner:?}: {command} vs {expected}"
            );
            let back = run.plan.placement.to_document(work);
            assert!(
                (back.x - move_to(step).x).abs() < 1e-9 && (back.y - move_to(step).y).abs() < 1e-9
            );
        }
    }
}
