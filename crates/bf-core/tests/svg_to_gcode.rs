//! An imported SVG all the way to GCode: import, one cut layer per colour
//! (as the editor's import does), generate, and run it on the simulator.

use bf_core::gcode::{JobAnchor, JobPlacement, StartFrom, generate, run};
use bf_core::import::svg::import_svg;
use bf_core::import::{ImportedPoint, ImportedSegment};
use bf_core::machine::{BedSize, MachineProfile, Settings};
use bf_core::project::{CutLayer, DesignDocument, LayerMode, Point, SceneObject, Segment, Subpath};
use bf_sim::Profile;

fn point(p: ImportedPoint) -> Point {
    Point { x: p.x, y: p.y }
}

fn segment(segment: &ImportedSegment) -> Segment {
    match *segment {
        ImportedSegment::Line { to } => Segment::Line { to: point(to) },
        ImportedSegment::Quad { control, to } => Segment::Quad {
            control: point(control),
            to: point(to),
        },
        ImportedSegment::Cubic {
            control1,
            control2,
            to,
        } => Segment::Cubic {
            control1: point(control1),
            control2: point(control2),
            to: point(to),
        },
    }
}

/// `shapes.svg` (curves, transforms, `<use>`) as a design document.
fn shapes() -> DesignDocument {
    svg_document("shapes.svg")
}

/// An SVG fixture as a design document, one cut layer per colour, moved
/// 20 mm onto the bed.
fn svg_document(name: &str) -> DesignDocument {
    let path = format!("{}/tests/fixtures/svg/{name}", env!("CARGO_MANIFEST_DIR"));
    let drawing = import_svg(&std::fs::read(path).unwrap()).unwrap();
    let mut layers: Vec<CutLayer> = Vec::new();
    let mut objects = Vec::new();
    for (index, imported) in drawing.paths.iter().enumerate() {
        let layer_id = format!("layer-{}", imported.color.trim_start_matches('#'));
        if !layers.iter().any(|layer| layer.id == layer_id) {
            layers.push(CutLayer::new(&layer_id, &layer_id, &imported.color));
        }
        objects.push(SceneObject::Path {
            id: format!("p{index}"),
            name: imported.name.clone(),
            layer_id,
            transform: [1.0, 0.0, 0.0, 1.0, 20.0, 20.0],
            subpaths: imported
                .subpaths
                .iter()
                .map(|subpath| Subpath {
                    start: point(subpath.start),
                    segments: subpath.segments.iter().map(segment).collect(),
                    closed: subpath.closed,
                })
                .collect(),
        });
    }
    DesignDocument {
        version: 1,
        units: "mm".into(),
        layers,
        objects,
    }
}

fn diode_laser() -> MachineProfile {
    MachineProfile {
        id: "diode".into(),
        name: "Diode".into(),
        bed: BedSize {
            width: 400.0,
            height: 400.0,
        },
        laser_mode: true,
        ..MachineProfile::default()
    }
}

#[test]
fn an_imported_svg_runs_cleanly_on_the_simulator() {
    let document = shapes();
    assert!(document.layers.len() > 1, "one layer per colour");
    for placement in [
        JobPlacement::default(),
        JobPlacement {
            start_from: StartFrom::CurrentPosition,
            anchor: JobAnchor::Center,
        },
    ] {
        let job = generate(
            &document,
            &diode_laser(),
            placement,
            Some(Point { x: 200.0, y: 200.0 }),
        )
        .unwrap();
        assert!(job.summary.inside_bed, "{:?}", job.summary);
        let report = bf_sim::check(&job.gcode, Profile::diode_laser());
        assert!(report.passed(), "{placement:?}: {report}");
        assert!(
            (report.cut_distance_mm - job.summary.cut_length).abs() < 0.01,
            "the simulator cut what was planned: {} vs {}",
            report.cut_distance_mm,
            job.summary.cut_length
        );
    }
}

/// Imported curves are many short moves, so the planner's lookahead and
/// junction speeds decide how long they take. The estimate stays within 5%
/// of the simulated machine, cutting the outlines and filling them.
#[test]
fn estimates_of_imported_svgs_match_the_simulator() {
    let sim = Profile::diode_laser();
    let mut machine = diode_laser();
    let settings = sim.settings.report_lines();
    machine.apply_settings(&Settings::parse(settings.iter().map(String::as_str)));
    // Filling shapes.svg would take 13 simulated minutes: too slow a test.
    for (name, mode) in [
        ("shapes.svg", LayerMode::Line),
        ("fill-text.svg", LayerMode::Line),
        ("fill-text.svg", LayerMode::Fill),
    ] {
        let mut document = svg_document(name);
        for layer in &mut document.layers {
            layer.mode = mode;
            layer.speed = 3000.0;
        }
        let job = run(&document, &machine, JobPlacement::default(), None, name).unwrap();
        let report = bf_sim::check(&job.job.gcode, sim.clone());
        assert!(report.passed(), "{name} {mode:?}: {report}");
        let estimate = job.plan.toolpath.estimate.seconds;
        let error = estimate / report.total_time_s - 1.0;
        println!(
            "{name} {mode:?}: estimated {estimate:.3} s, simulated {:.3} s ({:+.2}%)",
            report.total_time_s,
            error * 100.0
        );
        assert!(error.abs() < 0.05, "{name} {mode:?}: {error:+.4}");
    }
}
