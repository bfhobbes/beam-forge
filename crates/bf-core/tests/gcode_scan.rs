//! The GCode file scanner on the files in `tests/fixtures/gcode/`, and a
//! cross-check of its bounds against the simulator, which is an independent
//! implementation of GRBL: the two must agree on where every file goes.

use std::path::{Path, PathBuf};

use bf_core::gcode::{JobSummary, scan};
use bf_core::machine::{BedSize, CheckLevel, MachineProfile, check_bounds, check_constant_power};
use bf_sim::Profile;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gcode")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The simulated diode laser's bed: 400 × 400 mm.
fn profile() -> MachineProfile {
    MachineProfile {
        bed: BedSize {
            width: 400.0,
            height: 400.0,
        },
        laser_mode: true,
        ..MachineProfile::default()
    }
}

fn scan_fixture(name: &str) -> JobSummary {
    scan(&read(&fixtures().join(name)), &profile())
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: {actual} != {expected}"
    );
}

fn assert_bounds(summary: &JobSummary, min: (f64, f64), max: (f64, f64)) {
    let b = summary.bounds;
    assert_near(b.min.x, min.0, "min x");
    assert_near(b.min.y, min.1, "min y");
    assert_near(b.max.x, max.0, "max x");
    assert_near(b.max.y, max.1, "max y");
}

#[test]
fn a_file_inside_the_bed_passes() {
    let summary = scan_fixture("scan-inside-bed.gcode");
    assert_bounds(&summary, (10.0, 10.0), (110.0, 110.0));
    assert!(summary.inside_bed);
    assert_eq!(summary.warning, None);
    assert_eq!(summary.unknown_bounds, None);
    assert_eq!(summary.lines, 8);
    assert_near(summary.cut_length, 400.0, "cut");
    assert_eq!(check_bounds(&summary).level, CheckLevel::Pass);
}

#[test]
fn a_file_going_5_mm_past_the_bed_warns() {
    let summary = scan_fixture("scan-outside-bed.gcode");
    assert!(!summary.inside_bed);
    let check = check_bounds(&summary);
    assert_eq!(check.level, CheckLevel::Warn);
    assert_eq!(check.title, "Job leaves the bed");
    assert_eq!(
        check.detail.as_deref(),
        Some("The job goes up to 5 mm outside the 400 × 400 mm bed.")
    );
}

#[test]
fn relative_moves_are_bounded_from_the_last_absolute_position() {
    let summary = scan_fixture("scan-relative.gcode");
    assert_bounds(&summary, (50.0, 50.0), (150.0, 130.0));
    assert!(summary.inside_bed);
    assert_near(summary.cut_length, 360.0, "cut");
}

#[test]
fn inches_are_converted_to_mm() {
    let summary = scan_fixture("scan-inches.gcode");
    assert_bounds(&summary, (25.4, 25.4), (76.2, 50.8));
    assert_near(summary.cut_length, 6.0 * 25.4, "cut");
}

#[test]
fn an_arc_bulging_past_the_bed_edge_is_caught() {
    let summary = scan_fixture("scan-arc-past-edge.gcode");
    assert_bounds(&summary, (395.0, 200.0), (405.0, 220.0));
    assert_eq!(
        check_bounds(&summary).detail.as_deref(),
        Some("The job goes up to 5 mm outside the 400 × 400 mm bed.")
    );
}

#[test]
fn an_m3_s0_header_before_m4_cutting_passes_the_constant_power_check() {
    let summary = scan_fixture("scan-m3-header-m4-cut.gcode");
    assert_eq!(summary.constant_power_line, None);
    assert!(summary.constant_power_layers.is_empty());
    assert_eq!(check_constant_power(&summary).level, CheckLevel::Pass);
}

#[test]
fn m3_with_a_lit_cutting_move_warns_naming_the_first_line() {
    let summary = scan_fixture("scan-m3-lit.gcode");
    assert_eq!(summary.constant_power_line, Some(7));
    let check = check_constant_power(&summary);
    assert_eq!(check.level, CheckLevel::Warn);
    assert_eq!(check.title, "Constant power (M3) from line 7");
}

#[test]
fn a_g92_file_reports_unknown_bounds_and_the_bounds_check_warns() {
    let summary = scan_fixture("scan-g92.gcode");
    let unknown = summary.unknown_bounds.clone().expect("unknown bounds");
    assert_eq!((unknown.construct.as_str(), unknown.line), ("G92", 4));
    assert!(!summary.inside_bed);
    let check = check_bounds(&summary);
    assert_eq!(check.level, CheckLevel::Warn);
    assert_eq!(check.title, "Couldn't check bounds");
    let detail = check.detail.unwrap();
    assert!(
        detail.starts_with("Couldn't check bounds: the file uses G92 (line 4)."),
        "{detail}"
    );
}

/// Every fixture the scanner can bound, run on the simulated diode laser:
/// the head goes over the same X/Y box in both. The simulator's box includes
/// where the head starts (the origin, since the profile doesn't home), and
/// its positions are whole steps with arcs cut into chords, hence the
/// tolerance.
#[test]
fn scanned_bounds_match_the_simulator_on_every_fixture() {
    const TOLERANCE: f64 = 0.01;
    let mut checked = 0;
    let mut paths: Vec<_> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "gcode"))
        .collect();
    paths.sort();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let gcode = read(&path);
        let summary = scan(&gcode, &profile());
        if summary.unknown_bounds.is_some() {
            continue;
        }
        let report = bf_sim::check(&gcode, Profile::diode_laser());
        let sim = report
            .bounds
            .clone()
            .unwrap_or_else(|| panic!("{name}: the simulator saw no motion"));
        let b = summary.bounds;
        let scanned = [
            b.min.x.min(0.0),
            b.min.y.min(0.0),
            b.max.x.max(0.0),
            b.max.y.max(0.0),
        ];
        let simulated = [sim.min[0], sim.min[1], sim.max[0], sim.max[1]];
        for (scanned, simulated) in scanned.into_iter().zip(simulated) {
            assert!(
                (scanned - simulated).abs() < TOLERANCE,
                "{name}: scanned {scanned:?} vs simulated {simulated:?}\n{report}"
            );
        }
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} fixtures cross-checked");
}
