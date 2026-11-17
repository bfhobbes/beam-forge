//! Headless job checks: each bad fixture fails with exactly its
//! violation, and the clean job's report is a stable snapshot.

use std::path::Path;
use std::process::Command;

use bf_sim::{Profile, Report, Violation, check};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap()
}

fn check_fixture(name: &str) -> Report {
    check(&fixture(name), Profile::diode_laser())
}

fn kinds(report: &Report) -> Vec<&'static str> {
    report.violations.iter().map(|v| v.kind()).collect()
}

#[test]
fn the_clean_job_passes() {
    let report = check_fixture("sample-job.gc");
    assert!(report.passed(), "{report}");
    assert_eq!(report.lines, 37);
    assert_eq!(report.passes.len(), 1);
    assert!(report.total_time_s > report.passes[0].time_s);
    assert!(report.cut_distance_mm > 0.0 && report.travel_distance_mm > 0.0);
    let bounds = report.bounds.as_ref().unwrap();
    assert_eq!(bounds.min, [0.0, 0.0, 0.0]);
    // The G2 arc bulges past the stated bounds: centre X40 Y20, radius 10.
    assert_eq!(bounds.max, [50.0, 30.0, 0.0]);
    assert_eq!(report.max_power, 0.8);
}

#[test]
fn each_bad_fixture_fails_with_exactly_its_violation() {
    for (name, kind) in [
        ("bad-laser-on-during-rapid.gc", "laser_on_during_rapid"),
        ("bad-laser-on-at-end.gc", "laser_on_at_end"),
        ("bad-outside-bed.gc", "outside_bed"),
        ("bad-missing-m5.gc", "missing_m5"),
        ("bad-error.gc", "error"),
        ("bad-alarm.gc", "alarm"),
        ("bad-rx-overflow.gc", "rx_overflow"),
    ] {
        let report = check_fixture(name);
        assert_eq!(kinds(&report), [kind], "{name}:\n{report}");
    }
}

#[test]
fn violations_say_where_they_happened() {
    let report = check_fixture("bad-error.gc");
    assert_eq!(
        serde_json::to_value(&report.violations).unwrap(),
        serde_json::json!([{ "kind": "error", "line": 4, "code": 22 }])
    );
    let report = check_fixture("bad-alarm.gc");
    assert_eq!(
        serde_json::to_value(&report.violations).unwrap(),
        serde_json::json!([{ "kind": "alarm", "line": 6, "code": 2 }])
    );
    let report = check_fixture("bad-rx-overflow.gc");
    assert_eq!(
        serde_json::to_value(&report.violations).unwrap(),
        serde_json::json!([{ "kind": "rx_overflow", "line": 2, "dropped": 25 }])
    );
    let report = check_fixture("bad-outside-bed.gc");
    let [Violation::OutsideBed { time_s, position }] = report.violations[..] else {
        panic!("{report}");
    };
    assert!(time_s > 0.0 && time_s < report.total_time_s);
    assert_eq!(
        position,
        [400.0, 10.0, 0.0],
        "where the head crosses the edge"
    );
}

#[test]
fn m2_turns_the_laser_off_like_m5() {
    let job = "G21 G90\nM3 S500\nG1 X10 F1000\nM2\n";
    assert!(check(job, Profile::diode_laser()).passed());
}

#[test]
fn m0_is_resumed_like_an_operator_would() {
    let job = "G1 X10 F1000\nM0\nG1 X0\nM5\n";
    let report = check(job, Profile::diode_laser());
    assert!(report.passed(), "{report}");
    assert_eq!(report.travel_distance_mm, 20.0);
}

#[test]
fn a_homing_profile_homes_first_and_checks_negative_space() {
    let mut profile = Profile::diode_laser();
    profile.settings.set(22, 1.0).unwrap();
    let report = check("G0 X-10 Y-10\nG0 X-1 Y-1\n", profile.clone());
    assert!(report.passed(), "{report}");
    let report = check("G0 X10 Y-10\n", profile);
    assert_eq!(kinds(&report), ["outside_bed"]);
}

// ---- CLI ----

fn cli(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_bf-sim"))
        .args(args)
        .current_dir(FIXTURES)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    (output.status.code().unwrap(), stdout)
}

/// `--json` output for the clean job matches `sample-job.report.json`. Set
/// `UPDATE_SNAPSHOTS=1` to rewrite it after an intended change.
#[test]
fn json_output_is_a_stable_snapshot() {
    let (code, stdout) = cli(&["check", "sample-job.gc", "--json"]);
    assert_eq!(code, 0, "{stdout}");
    let path = Path::new(FIXTURES).join("sample-job.report.json");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &stdout).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap().replace('\r', "");
    assert_eq!(stdout.replace('\r', ""), expected);
}

#[test]
fn the_cli_exits_non_zero_on_violations() {
    let (code, stdout) = cli(&["check", "bad-missing-m5.gc"]);
    assert_eq!(code, 1);
    assert!(stdout.contains("FAIL: 1 violation(s)"), "{stdout}");
    assert!(stdout.contains("missing_m5"), "{stdout}");

    let (code, stdout) = cli(&["check", "sample-job.gc"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("PASS"), "{stdout}");
}

#[test]
fn the_cli_reads_a_profile() {
    let dir = std::env::temp_dir().join(format!("bf-sim-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("small-bed.json");
    let mut profile = Profile::diode_laser();
    profile.settings.set(130, 20.0).unwrap();
    std::fs::write(&path, serde_json::to_string(&profile).unwrap()).unwrap();
    let (code, stdout) = cli(&[
        "check",
        "sample-job.gc",
        "--profile",
        path.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("outside_bed"), "{stdout}");
}

#[test]
fn the_cli_rejects_bad_usage() {
    assert_eq!(cli(&[]).0, 2);
    assert_eq!(cli(&["check"]).0, 2);
    assert_eq!(cli(&["check", "missing.gc"]).0, 2);
    assert_eq!(cli(&["check", "sample-job.gc", "--bogus"]).0, 2);
}
