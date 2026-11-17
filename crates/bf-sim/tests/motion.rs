//! G-code interpretation, motion timing and the laser model, driven
//! through the public byte API.

use std::time::Duration;

use bf_sim::{Event, LaserMode, MachineState, Profile, Simulator, TraceSegment};

const BANNER: &str = "\r\nGrbl 1.1h ['$' for help]\r\n";

/// A laser in laser mode with round numbers: 100 mm/s^2 on X and Y, rapids
/// at 6000 mm/min, full power at S1000.
fn laser() -> Simulator {
    laser_with(&[])
}

fn laser_with(settings: &[(u16, f64)]) -> Simulator {
    let mut profile = Profile::grbl_defaults();
    let base = [
        (32, 1.0),
        (110, 6000.0),
        (111, 6000.0),
        (120, 100.0),
        (121, 100.0),
    ];
    for &(id, value) in base.iter().chain(settings) {
        profile.settings.set(id, value).unwrap();
    }
    let mut sim = Simulator::new(profile);
    sim.take_output();
    sim
}

/// Sends bytes and processes them without moving the clock.
fn send(sim: &mut Simulator, bytes: &[u8]) -> String {
    sim.feed(bytes);
    sim.advance(Duration::ZERO);
    String::from_utf8(sim.take_output()).unwrap()
}

/// Sends one line and runs the clock until it is acknowledged.
fn line(sim: &mut Simulator, text: &str) -> String {
    sim.feed(format!("{text}\n").as_bytes());
    let mut out = String::new();
    for _ in 0..100_000 {
        sim.advance(Duration::from_millis(1));
        out += &String::from_utf8(sim.take_output()).unwrap();
        if out.ends_with("ok\r\n") || out.contains("error:") || out.contains("ALARM:") {
            return out;
        }
    }
    panic!("no reply to {text:?}: {out:?}");
}

/// Runs a program line by line and waits for the machine to go idle.
fn run(sim: &mut Simulator, program: &str) {
    for text in program.lines() {
        let reply = line(sim, text);
        assert!(reply.ends_with("ok\r\n"), "{text:?} -> {reply:?}");
    }
    finish(sim);
}

fn finish(sim: &mut Simulator) {
    for _ in 0..100_000 {
        if sim.state() == MachineState::Idle {
            return;
        }
        sim.advance(Duration::from_millis(10));
    }
    panic!("never went idle: {:?}", sim.state());
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn assert_position(actual: [f64; 3], expected: [f64; 3]) {
    assert!(
        (0..3).all(|i| close(actual[i], expected[i], 1e-6)),
        "{actual:?} != {expected:?}"
    );
}

/// The feed and spindle speed in a status report's `FS:` field.
fn fs(report: &str) -> (f64, f64) {
    let field = report
        .split('|')
        .find_map(|f| f.strip_prefix("FS:"))
        .unwrap();
    let field = field.trim_end_matches(['>', '\r', '\n']);
    let (feed, speed) = field.split_once(',').unwrap();
    (feed.parse().unwrap(), speed.parse().unwrap())
}

fn duration(trace: &[TraceSegment]) -> f64 {
    (trace.last().unwrap().t1 - trace[0].t0).as_secs_f64()
}

fn ok_times(sim: &Simulator) -> Vec<Duration> {
    sim.events()
        .iter()
        .filter_map(|e| match e {
            Event::Sent { at, bytes } if bytes == b"ok\r\n" => Some(*at),
            _ => None,
        })
        .collect()
}

// ---- motion timing ----

#[test]
fn a_move_takes_as_long_as_its_hand_computed_trapezoid() {
    // 20 mm at 20 mm/s with 100 mm/s^2: 0.2 s and 2 mm to reach speed, 16 mm
    // cruising for 0.8 s, 0.2 s braking. 1.2 s in all.
    let mut sim = laser();
    send(&mut sim, b"G1 X20 F1200\n");
    assert_eq!(sim.state(), MachineState::Run);
    sim.advance(Duration::from_millis(100));
    assert_position(sim.machine_position(), [0.5, 0.0, 0.0]);
    sim.advance(Duration::from_millis(500));
    assert_position(sim.machine_position(), [10.0, 0.0, 0.0]);
    let report = send(&mut sim, b"?");
    assert!(
        report.starts_with("<Run|MPos:10.000,0.000,0.000|FS:1200,0"),
        "{report}"
    );
    finish(&mut sim);
    let trace = sim.trace();
    assert!(close(duration(trace), 1.2, 1e-6), "{}", duration(trace));
    assert_position(trace.last().unwrap().end, [20.0, 0.0, 0.0]);
    assert!(close(trace.last().unwrap().end_speed, 0.0, 1e-6));

    // Too short to reach 20 mm/s: a triangle, 2 * sqrt(d / a) = 0.2828 s.
    let mut sim = laser();
    run(&mut sim, "G1 X2 F1200");
    assert!(close(duration(sim.trace()), 2.0 * 0.02_f64.sqrt(), 1e-6));
}

#[test]
fn speed_changes_in_10_ms_steps_like_grbl() {
    let mut sim = laser();
    run(&mut sim, "G1 X20 F1200");
    let trace = sim.trace();
    // 20 pieces up to speed, one cruise, 20 down.
    assert_eq!(trace.len(), 41);
    for segment in &trace[..20] {
        assert!(close((segment.t1 - segment.t0).as_secs_f64(), 0.01, 1e-9));
        assert!(segment.end_speed > segment.start_speed);
    }
    assert!(close(trace[20].start_speed, 1200.0, 1e-6));
    assert!(close(trace[20].end_speed, 1200.0, 1e-6));
}

#[test]
fn rate_and_acceleration_are_limited_per_axis() {
    // Y is half as fast and accelerates half as hard. On a 45 degree move the
    // Y component limits both: rapid = 3000 / cos 45, accel = 50 / cos 45.
    let mut sim = laser_with(&[(111, 3000.0), (121, 50.0)]);
    run(&mut sim, "G0 X100 Y100");
    let trace = sim.trace();
    let top = trace.iter().map(|s| s.end_speed).fold(0.0, f64::max);
    let rapid = 3000.0 / std::f64::consts::FRAC_1_SQRT_2;
    assert!(close(top, rapid, 1e-6), "{top}");
    assert!(trace.iter().all(|s| s.rapid && close(s.feed, rapid, 1e-6)));
    let first = &trace[0];
    let accel = (first.end_speed - first.start_speed) / 60.0 / 0.01;
    assert!(close(accel, 50.0 / std::f64::consts::FRAC_1_SQRT_2, 1e-6));
}

#[test]
fn junction_speed_follows_junction_deviation() {
    // Straight on: no slow-down at the join.
    let mut sim = laser();
    send(&mut sim, b"G1 X10 F1200\nG1 X20\n");
    finish(&mut sim);
    let join = sim.trace().iter().find(|s| s.end[0] == 10.0).unwrap();
    assert!(close(join.end_speed, 1200.0, 1e-6));
    assert!(
        close(duration(sim.trace()), 1.2, 1e-6),
        "same as one 20 mm move"
    );

    // A right angle: v^2 = a * $11 * sin(t/2) / (1 - sin(t/2)), with a limited
    // along the junction direction (100 / cos 45).
    let mut sim = laser();
    send(&mut sim, b"G1 X10 F1200\nG1 Y10\n");
    finish(&mut sim);
    let join = sim
        .trace()
        .iter()
        .find(|s| s.end == [10.0, 0.0, 0.0])
        .unwrap();
    let a = 100.0 / std::f64::consts::FRAC_1_SQRT_2;
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let expected = (a * 0.010 * s / (1.0 - s)).sqrt() * 60.0;
    assert!(
        close(join.end_speed, expected, 1e-6),
        "{} vs {expected}",
        join.end_speed
    );
}

#[test]
fn the_planner_drains_at_the_rate_motion_completes() {
    let mut sim = laser_with(&[(10, 2.0)]);
    // 15 one-millimetre moves in a line fill the planner. They blend into
    // one 15 mm move at F600: 0.1 s and 0.5 mm to reach 10 mm/s, so the
    // first block is done after 0.15 s.
    for i in 1..=15 {
        assert_eq!(send(&mut sim, format!("G1X{i}F600\n").as_bytes()), "ok\r\n");
    }
    let report = send(&mut sim, b"?");
    assert!(report.contains("|Bf:0,"), "{report}");
    // The 16th line waits for space.
    assert_eq!(send(&mut sim, b"G1X16\n"), "");
    sim.advance(Duration::from_secs(1));
    let times = ok_times(&sim);
    assert_eq!(times.len(), 16);
    assert!(close(times[15].as_secs_f64(), 0.15, 1e-6), "{times:?}");
    finish(&mut sim);
    assert_position(sim.machine_position(), [16.0, 0.0, 0.0]);
}

#[test]
fn feed_hold_decelerates_and_cycle_start_resumes() {
    let mut sim = laser();
    send(&mut sim, b"G1 X50 F1200\n");
    sim.advance(Duration::from_millis(500)); // cruising at 20 mm/s
    send(&mut sim, b"!");
    assert_eq!(sim.state(), MachineState::Hold(1));
    sim.advance(Duration::from_millis(100));
    assert_eq!(sim.state(), MachineState::Hold(1), "braking takes 0.2 s");
    sim.advance(Duration::from_millis(200));
    assert_eq!(sim.state(), MachineState::Hold(0));
    // 0.5 s in: 2 mm + 6 mm; braking from 20 mm/s adds 2 mm.
    assert_position(sim.machine_position(), [10.0, 0.0, 0.0]);
    sim.advance(Duration::from_secs(1));
    assert_position(sim.machine_position(), [10.0, 0.0, 0.0]);
    send(&mut sim, b"~");
    assert_eq!(sim.state(), MachineState::Run);
    finish(&mut sim);
    assert_position(sim.machine_position(), [50.0, 0.0, 0.0]);
}

#[test]
fn feed_override_changes_speed_mid_move() {
    let mut sim = laser();
    send(&mut sim, b"G1 X50 F1200\n");
    sim.advance(Duration::from_millis(500));
    send(&mut sim, &[0x92, 0x92]); // -20%
    sim.advance(Duration::from_millis(500));
    let report = send(&mut sim, b"?");
    assert!(report.contains("|FS:960,0"), "{report}");
}

// ---- laser model ----

#[test]
fn m3_holds_constant_power_through_acceleration() {
    let mut sim = laser();
    run(&mut sim, "M3 S500\nG1 X20 F1200");
    let trace = sim.trace();
    assert!(
        trace
            .iter()
            .all(|s| s.power == 0.5 && s.laser_mode == LaserMode::Constant)
    );
}

#[test]
fn m4_power_scales_with_speed_during_acceleration() {
    let mut sim = laser();
    run(&mut sim, "M4 S1000\nG1 X20 F1200");
    let trace = sim.trace();
    assert!(trace.iter().all(|s| s.laser_mode == LaserMode::Dynamic));
    // GRBL sets each 10 ms piece's power from the speed at its end.
    for segment in trace {
        assert!(close(segment.power, segment.end_speed / 1200.0, 1e-9));
    }
    let ramp: Vec<f64> = trace[..20].iter().map(|s| s.power).collect();
    assert!(ramp.windows(2).all(|w| w[1] > w[0]), "{ramp:?}");
    assert!(close(ramp[0], 0.05, 1e-9), "1 m/s after 10 ms of 20");
    assert!(
        close(trace[20].power, 1.0, 1e-9),
        "full power at full speed"
    );
    assert_eq!(trace.last().unwrap().power, 0.0, "off once stopped");
}

#[test]
fn g0_is_laser_off_under_m3_and_m4() {
    for (command, mode) in [("M3", LaserMode::Constant), ("M4", LaserMode::Dynamic)] {
        let mut sim = laser();
        run(
            &mut sim,
            &format!("{command} S1000\nG0 X10\nG1 X20 F1200\nG0 X0"),
        );
        let trace = sim.trace();
        let (rapids, cuts): (Vec<_>, Vec<_>) = trace.iter().partition(|s| s.rapid);
        assert!(!rapids.is_empty() && !cuts.is_empty());
        assert!(rapids.iter().all(|s| s.power == 0.0), "{command}");
        assert!(rapids.iter().all(|s| s.laser_mode == mode));
        assert!(cuts.iter().any(|s| s.power > 0.0), "{command}");
    }
}

#[test]
fn without_laser_mode_m4_is_a_plain_spindle() {
    let mut sim = laser_with(&[(32, 0.0)]);
    run(&mut sim, "M4 S500\nG0 X10");
    // The spindle runs through rapids and M4 does not scale with speed.
    assert!(
        sim.trace()
            .iter()
            .all(|s| s.power == 0.5 && s.laser_mode == LaserMode::Constant)
    );
}

#[test]
fn s_is_clamped_to_the_spindle_range() {
    let mut sim = laser_with(&[(30, 1000.0), (31, 100.0)]);
    run(&mut sim, "M3 S2000\nG1 X5 F600");
    assert!(sim.trace().iter().all(|s| s.power == 1.0));
    // An M3 laser stays on after the move, at the clamped speed.
    assert_eq!(fs(&send(&mut sim, b"?")), (0.0, 1000.0));

    let mut sim = laser_with(&[(30, 1000.0), (31, 100.0)]);
    run(&mut sim, "M3 S550\nG1 X5 F600");
    assert!(sim.trace().iter().all(|s| close(s.power, 0.5, 1e-9)));
    send(&mut sim, b"G1 X0 S50\n");
    sim.advance(Duration::from_millis(50));
    assert_eq!(fs(&send(&mut sim, b"?")).1, 100.0, "below $31 runs at $31");
}

#[test]
fn spindle_override_scales_power() {
    let mut sim = laser();
    send(&mut sim, &[0x9B, 0x9B, 0x9B, 0x9B, 0x9B]); // 50%
    run(&mut sim, "M3 S1000\nG1 X5 F600");
    assert!(sim.trace().iter().all(|s| close(s.power, 0.5, 1e-9)));
}

#[test]
fn m8_turns_on_air_assist() {
    let mut sim = laser();
    run(&mut sim, "M8\nG1 X5 F600\nM9\nG1 X0");
    let trace = sim.trace();
    let split = trace.iter().position(|s| s.end[0] == 5.0).unwrap();
    assert!(trace[..=split].iter().all(|s| s.air_assist));
    assert!(trace[split + 1..].iter().all(|s| !s.air_assist));
}

#[test]
fn spindle_and_coolant_changes_wait_for_motion() {
    let mut sim = laser();
    send(&mut sim, b"M3 S100\nG1 X10 F600\nM5\nM8\n");
    sim.advance(Duration::from_secs(2));
    let times = ok_times(&sim);
    assert_eq!(times[..2], [Duration::ZERO; 2]);
    // 10 mm at 10 mm/s with 0.1 s ramps: 1.1 s.
    assert!(close(times[2].as_secs_f64(), 1.1, 1e-6), "{times:?}");
    assert_eq!(times[2], times[3]);
}

// ---- G-code commands ----

#[test]
fn g0_and_g1_move_and_report_run() {
    let mut sim = laser();
    assert_eq!(send(&mut sim, b"G0 X10 Y5\n"), "ok\r\n");
    assert_eq!(sim.state(), MachineState::Run);
    finish(&mut sim);
    assert_position(sim.machine_position(), [10.0, 5.0, 0.0]);
    run(&mut sim, "G1 X0 Y0 F3000");
    assert_position(sim.machine_position(), [0.0, 0.0, 0.0]);
    assert!(
        sim.trace()
            .iter()
            .filter(|s| !s.rapid)
            .all(|s| s.feed == 3000.0)
    );
    assert!(send(&mut sim, b"$G\n").starts_with("[GC:G1 G54 G17 G21 G90 G94 M5 M9 T0 F3000 S0]"));
}

fn on_circle(trace: &[TraceSegment], center: [f64; 2], radius: f64) -> bool {
    trace.iter().all(|s| {
        let r = (s.end[0] - center[0]).hypot(s.end[1] - center[1]);
        close(r, radius, 0.01)
    })
}

fn max_y(trace: &[TraceSegment]) -> f64 {
    trace.iter().map(|s| s.end[1]).fold(f64::MIN, f64::max)
}

fn min_y(trace: &[TraceSegment]) -> f64 {
    trace.iter().map(|s| s.end[1]).fold(f64::MAX, f64::min)
}

#[test]
fn g2_clockwise_arcs_in_both_forms() {
    for program in ["G2 X10 Y0 I5 J0 F1200", "G2 X10 Y0 R5 F1200"] {
        let mut sim = laser();
        run(&mut sim, program);
        let trace = sim.trace();
        assert_position(trace.last().unwrap().end, [10.0, 0.0, 0.0]);
        assert!(on_circle(trace, [5.0, 0.0], 5.0), "{program}");
        // Clockwise from (0,0) to (10,0) about (5,0) passes over the top.
        assert!(close(max_y(trace), 5.0, 0.01), "{program}");
        assert!(min_y(trace) > -0.01, "{program}");
    }
}

#[test]
fn g3_counter_clockwise_arcs_in_both_forms() {
    for program in ["G3 X10 Y0 I5 J0 F1200", "G3 X10 Y0 R5 F1200"] {
        let mut sim = laser();
        run(&mut sim, program);
        let trace = sim.trace();
        assert_position(trace.last().unwrap().end, [10.0, 0.0, 0.0]);
        assert!(on_circle(trace, [5.0, 0.0], 5.0), "{program}");
        assert!(close(min_y(trace), -5.0, 0.01), "{program}");
        assert!(max_y(trace) < 0.01, "{program}");
    }
}

#[test]
fn negative_r_takes_the_long_way_round() {
    // A quarter circle the short way is centred at (10,0); R-10 picks (0,10)
    // and sweeps 270 degrees.
    let mut sim = laser();
    run(&mut sim, "G2 X10 Y10 R10 F3000");
    assert!(on_circle(sim.trace(), [10.0, 0.0], 10.0));
    let mut sim = laser();
    run(&mut sim, "G2 X10 Y10 R-10 F3000");
    assert!(on_circle(sim.trace(), [0.0, 10.0], 10.0));
    assert!(close(max_y(sim.trace()), 20.0, 0.01));
}

#[test]
fn full_circle_and_inch_arcs() {
    let mut sim = laser();
    run(&mut sim, "G2 X0 Y0 I5 J0 F3000");
    let trace = sim.trace();
    assert!(on_circle(trace, [5.0, 0.0], 5.0));
    assert!(close(max_y(trace), 5.0, 0.01) && close(min_y(trace), -5.0, 0.01));

    let mut sim = laser();
    run(&mut sim, "G20 G3 X1 Y0 I0.5 J0 F100");
    assert!(on_circle(sim.trace(), [12.7, 0.0], 12.7));
}

#[test]
fn arc_errors_match_grbl() {
    let mut sim = laser();
    assert_eq!(
        send(&mut sim, b"G2 X10 R2 F100\n"),
        "error:34\r\n",
        "radius too small"
    );
    assert_eq!(
        send(&mut sim, b"G2 X10 F100\n"),
        "error:35\r\n",
        "no offsets"
    );
    assert_eq!(
        send(&mut sim, b"G2 I5 F100\n"),
        "error:26\r\n",
        "no axis words"
    );
    assert_eq!(
        send(&mut sim, b"G2 Z5 I5 F100\n"),
        "error:32\r\n",
        "not in plane"
    );
    assert_eq!(
        send(&mut sim, b"G2 X10 I4 F100\n"),
        "error:33\r\n",
        "end off the circle"
    );
    assert_eq!(
        send(&mut sim, b"G2 X0 Y0 R5 F100\n"),
        "error:33\r\n",
        "R needs a target"
    );
    assert_eq!(
        send(&mut sim, b"G2 X10 I5\n"),
        "error:22\r\n",
        "no feed rate"
    );
    assert_eq!(
        send(&mut sim, b"G1 X10 I5 F100\n"),
        "error:36\r\n",
        "I unused by G1"
    );
}

#[test]
fn g4_dwells_after_motion_finishes() {
    let mut sim = laser();
    send(&mut sim, b"G1 X10 F600\nG4 P0.5\n");
    assert_eq!(sim.state(), MachineState::Run);
    sim.advance(Duration::from_secs(3));
    let times = ok_times(&sim);
    assert!(close(times[1].as_secs_f64(), 1.1 + 0.5, 1e-6), "{times:?}");
    sim.take_output();
    assert_eq!(send(&mut sim, b"G4\n"), "error:28\r\n", "P required");
}

#[test]
fn units_distance_mode_plane_and_coordinate_system() {
    let mut sim = laser();
    run(&mut sim, "G17 G54 G20 G91 G0 X1");
    assert_position(sim.machine_position(), [25.4, 0.0, 0.0]);
    run(&mut sim, "G21 G0 X1 Y2");
    assert_position(sim.machine_position(), [26.4, 2.0, 0.0]);
    run(&mut sim, "G90 G0 X3");
    assert_position(sim.machine_position(), [3.0, 2.0, 0.0]);
    run(&mut sim, "G20");
    assert!(send(&mut sim, b"$G\n").starts_with("[GC:G0 G54 G17 G20 G90 G94"));
    // F in inches per minute.
    run(&mut sim, "G1 X0 F10");
    assert!(
        sim.trace()
            .iter()
            .filter(|s| !s.rapid)
            .all(|s| close(s.feed, 254.0, 1e-9))
    );
}

#[test]
fn g92_offsets_the_work_position() {
    let mut sim = laser();
    run(&mut sim, "G0 X10 Y20");
    run(&mut sim, "G92 X0 Y5");
    assert_position(sim.work_position(), [0.0, 5.0, 0.0]);
    assert!(send(&mut sim, b"$#\n").contains("[G92:10.000,15.000,0.000]"));
    run(&mut sim, "G0 X1 Y0");
    assert_position(sim.machine_position(), [11.0, 15.0, 0.0]);
    // The next report refreshes WCO.
    assert!(send(&mut sim, b"?").contains("|WCO:10.000,15.000,0.000"));
}

#[test]
fn g10_l20_and_l2_set_work_coordinates() {
    let mut sim = laser();
    run(&mut sim, "G0 X10 Y20");
    // L20: make the current position read X5 in G54.
    run(&mut sim, "G10 L20 P1 X5");
    assert_position(sim.work_position(), [5.0, 20.0, 0.0]);
    assert!(send(&mut sim, b"$#\n").starts_with("[G54:5.000,0.000,0.000]"));
    // With a G92 offset too, L20 still lands the work position where asked.
    run(&mut sim, "G92 Y0");
    run(&mut sim, "G10 L20 P1 X0 Y1");
    assert_position(sim.work_position(), [0.0, 1.0, 0.0]);
    // L2 stores the values directly; P0 is the active system.
    run(&mut sim, "G10 L2 P0 X3 Y0");
    assert!(send(&mut sim, b"$#\n").starts_with("[G54:3.000,0.000,0.000]"));
    run(&mut sim, "G92 X0 Y0");
    run(&mut sim, "G0 X1 Y1");
    // G92 now holds 7,20: machine = 1 + 3 + 7, 1 + 0 + 20.
    assert_position(sim.machine_position(), [11.0, 21.0, 0.0]);
    // Errors.
    assert_eq!(send(&mut sim, b"G10 L2 P1\n"), "error:26\r\n");
    assert_eq!(send(&mut sim, b"G10 X1\n"), "error:28\r\n");
    assert_eq!(send(&mut sim, b"G10 L3 P1 X1\n"), "error:20\r\n");
    assert_eq!(send(&mut sim, b"G10 L2 P7 X1\n"), "error:29\r\n");
    assert_eq!(send(&mut sim, b"G10 L2 P1 R1 X1\n"), "error:20\r\n");
}

#[test]
fn g53_moves_in_machine_coordinates() {
    let mut sim = laser();
    run(&mut sim, "G0 X10\nG92 X0");
    run(&mut sim, "G53 G0 X2");
    assert_position(sim.machine_position(), [2.0, 0.0, 0.0]);
    run(&mut sim, "G91 G53 G1 X4 F600");
    assert_position(sim.machine_position(), [4.0, 0.0, 0.0]);
    assert_eq!(send(&mut sim, b"G53 G2 X1 I1 F100\n"), "error:30\r\n");
}

#[test]
fn m2_and_m30_end_the_program() {
    for end in ["M2", "M30"] {
        let mut sim = laser();
        run(&mut sim, "G91 M3 S300 M8\nG1 X5 F600");
        send(&mut sim, &[0x91]);
        assert_eq!(line(&mut sim, end), "[MSG:Pgm End]\r\nok\r\n");
        assert_eq!(
            send(&mut sim, b"$G\n"),
            "[GC:G1 G54 G17 G21 G90 G94 M5 M9 T0 F600 S300]\r\nok\r\n"
        );
        assert_eq!(sim.overrides().feed, 100, "overrides restored");
    }
}

#[test]
fn m3_m4_m5_set_the_spindle_state() {
    let mut sim = laser();
    run(&mut sim, "M3 S1000");
    assert!(send(&mut sim, b"$G\n").contains(" M3 M9 T0 F0 S1000]"));
    run(&mut sim, "M4");
    assert!(send(&mut sim, b"$G\n").contains(" M4 "));
    run(&mut sim, "G1 X5 F600\nM5\nG1 X0");
    let trace = sim.trace();
    let split = trace.iter().position(|s| s.end[0] == 5.0).unwrap();
    assert!(trace[..split].iter().any(|s| s.power > 0.0));
    assert!(
        trace[split + 1..]
            .iter()
            .all(|s| s.power == 0.0 && s.laser_mode == LaserMode::Off)
    );
}

#[test]
fn line_numbers_and_comments_are_accepted() {
    let mut sim = laser();
    assert_eq!(send(&mut sim, b"N10 G0 X1 (move) ; tail\n"), "ok\r\n");
    assert_eq!(send(&mut sim, b"N10000001 G0 X1\n"), "error:27\r\n");
    assert_eq!(send(&mut sim, b"N-1 G0 X1\n"), "error:4\r\n");
}

#[test]
fn unsupported_commands_return_error_20() {
    let mut sim = laser();
    for text in [
        // Probing needs a probe pin, which the simulator does not model.
        "G38.2 X1", "G38.3 X1", "G5", "G28.2", "G43 Z1", "G61.1", "G90.1", "G92.2", "M6", "M100",
        "A1", "Q1",
    ] {
        assert_eq!(
            send(&mut sim, format!("{text}\n").as_bytes()),
            "error:20\r\n",
            "{text}"
        );
    }
    assert_eq!(sim.machine_position(), [0.0, 0.0, 0.0]);
    assert!(sim.trace().is_empty());
}

#[test]
fn other_parser_errors_match_grbl() {
    let mut sim = laser();
    for (text, code) in [
        ("G1.5 X1 F100", 23),
        ("M3.5", 23),
        ("G0 G1 X1", 24),
        ("G92 G0 X1", 24),
        ("G90 G91", 21),
        ("M3 M5", 21),
        ("G4 G53 P1", 21),
        ("X1 X2", 25),
        ("F-1", 4),
        ("S-1", 4),
        ("G1 X1", 22),
        ("P1", 36),
        ("G0 X1 R1", 36),
        ("1X", 1),
        ("X1-2", 1),
        ("X1..2", 2),
    ] {
        assert_eq!(
            send(&mut sim, format!("{text}\n").as_bytes()),
            format!("error:{code}\r\n"),
            "{text}"
        );
    }
    // A failed line changes nothing.
    assert!(send(&mut sim, b"$G\n").starts_with("[GC:G0 G54 G17 G21 G90 G94 M5 M9 T0 F0 S0]"));
}

// ---- the rest of GRBL 1.1's G-code ----

fn parameters(sim: &mut Simulator) -> String {
    send(sim, b"$#\n")
}

fn parser_state(sim: &mut Simulator) -> String {
    send(sim, b"$G\n")
}

#[test]
fn header_and_no_op_commands_are_accepted() {
    let mut sim = laser();
    for text in ["G00 G17 G40 G21 G54", "G61", "G91.1", "G94", "M1"] {
        assert_eq!(
            send(&mut sim, format!("{text}\n").as_bytes()),
            "ok\r\n",
            "{text}"
        );
    }
    assert_eq!(sim.state(), MachineState::Idle, "M1 is ignored");
    assert_eq!(
        parser_state(&mut sim),
        "[GC:G0 G54 G17 G21 G90 G94 M5 M9 T0 F0 S0]\r\nok\r\n"
    );
}

#[test]
fn new_commands_check_modal_groups_and_words_like_grbl() {
    let mut sim = laser();
    for (text, code) in [
        ("G17 G18", 21),
        ("G93 G94", 21),
        ("G54 G55", 21),
        ("G40 G40", 21),
        ("G61 G61", 21),
        ("G91.1 G91.1", 21),
        ("G0 G80", 21),
        ("G43.1 G49 Z1", 24),
        ("G28.1 G30.1", 21),
        ("G28.1 G92.1", 21),
        ("M0 M2", 21),
        ("M1 M30", 21),
        ("M7 M8", 21),
        ("M7 M9", 21),
        ("G28 G30", 24),
        ("G28 G1 X1", 24),
        ("G43.1 Z1 G0", 24),
        ("G0 G49", 24),
        ("G59.1", 23),
        ("G80 X1", 31),
        ("G80 G43.1 Z1", 31),
        ("G43.1", 37),
        ("G43.1 X1 Z1", 37),
        ("T256", 38),
        ("T-1", 4),
        ("T1 T2", 25),
        ("G93 G1 X1", 22),
        ("G18 G2 Y1 J1 F100", 32),
        ("G19 G2 Y1 I1 F100", 35),
        ("G1 X1 K1 F100", 36),
    ] {
        assert_eq!(
            send(&mut sim, format!("{text}\n").as_bytes()),
            format!("error:{code}\r\n"),
            "{text}"
        );
    }
    assert_eq!(
        parser_state(&mut sim),
        "[GC:G0 G54 G17 G21 G90 G94 M5 M9 T0 F0 S0]\r\nok\r\n",
        "failed lines change nothing"
    );
    assert_eq!(sim.machine_position(), [0.0, 0.0, 0.0]);
}

#[test]
fn parser_state_reports_every_modal_group() {
    let mut sim = laser();
    run(&mut sim, "G55 G18 G20 G91 G93 T3 M7");
    assert_eq!(
        parser_state(&mut sim),
        "[GC:G0 G55 G18 G20 G91 G93 M5 M7 T3 F0 S0]\r\nok\r\n"
    );
    // GRBL keeps T only until the next line without one, which resets it to 0.
    run(&mut sim, "M8");
    assert_eq!(
        parser_state(&mut sim),
        "[GC:G0 G55 G18 G20 G91 G93 M5 M7 M8 T0 F0 S0]\r\nok\r\n"
    );
    run(&mut sim, "G80");
    assert!(parser_state(&mut sim).starts_with("[GC:G80 G55 G18"));
    // Program end resets motion, coordinate system, plane, distance and feed
    // modes, spindle and coolant, but not units.
    run(&mut sim, "M2");
    assert_eq!(
        parser_state(&mut sim),
        "[GC:G1 G54 G17 G20 G90 G94 M5 M9 T0 F0 S0]\r\nok\r\n"
    );
    run(&mut sim, "T1.5");
    assert!(parser_state(&mut sim).contains(" T1 "));
    assert_eq!(send(&mut sim, b"M6\n"), "error:20\r\n", "no tool changes");
}

#[test]
fn m7_turns_on_mist() {
    let mut sim = laser();
    run(&mut sim, "M7");
    let reports: String = (0..3).map(|_| send(&mut sim, b"?")).collect();
    assert!(reports.contains("|A:M>"), "{reports}");
    run(&mut sim, "M8");
    assert!(parser_state(&mut sim).contains(" M5 M7 M8 T0 "));
    run(&mut sim, "M9");
    assert!(parser_state(&mut sim).contains(" M5 M9 T0 "));
}

#[test]
fn g55_to_g59_select_the_active_coordinate_system() {
    let mut sim = laser();
    run(&mut sim, "G10 L2 P2 X10 Y20");
    assert!(
        parameters(&mut sim).starts_with("[G54:0.000,0.000,0.000]\r\n[G55:10.000,20.000,0.000]")
    );
    run(&mut sim, "G55");
    assert!(send(&mut sim, b"?").contains("|WCO:10.000,20.000,0.000"));
    assert_position(sim.work_position(), [-10.0, -20.0, 0.0]);
    run(&mut sim, "G0 X1 Y1");
    assert_position(sim.machine_position(), [11.0, 21.0, 0.0]);
    // P0 is the active system: G55 here.
    run(&mut sim, "G10 L20 P0 X0");
    assert!(
        parameters(&mut sim).starts_with("[G54:0.000,0.000,0.000]\r\n[G55:11.000,20.000,0.000]")
    );
    assert_position(sim.work_position(), [0.0, 1.0, 0.0]);
    // A G54 on the same line makes P0 mean G54.
    run(&mut sim, "G54 G10 L2 P0 Y5");
    assert!(parameters(&mut sim).starts_with("[G54:0.000,5.000,0.000]"));
    assert!(send(&mut sim, b"?").contains("|WCO:0.000,5.000,0.000"));
    run(&mut sim, "G59");
    assert!(parser_state(&mut sim).starts_with("[GC:G0 G59 "));
    // Program end returns to G54.
    run(&mut sim, "G56\nM2");
    assert!(send(&mut sim, b"?").contains("|WCO:0.000,5.000,0.000"));
}

#[test]
fn g43_1_offsets_tool_length_until_g49() {
    let mut sim = laser();
    run(&mut sim, "G43.1 Z2");
    assert!(parameters(&mut sim).contains("[TLO:2.000]"));
    assert!(send(&mut sim, b"?").contains("|WCO:0.000,0.000,2.000"));
    run(&mut sim, "G0 Z1");
    assert_position(sim.machine_position(), [0.0, 0.0, 3.0]);
    assert_position(sim.work_position(), [0.0, 0.0, 1.0]);
    run(&mut sim, "G20 G43.1 Z1");
    assert!(parameters(&mut sim).contains("[TLO:25.400]"));
    run(&mut sim, "G21 G49");
    assert!(parameters(&mut sim).contains("[TLO:0.000]"));
    assert!(send(&mut sim, b"?").contains("|WCO:0.000,0.000,0.000"));
}

#[test]
fn g28_and_g30_go_to_stored_positions() {
    let mut sim = laser();
    run(&mut sim, "G0 X10 Y10\nG28.1");
    assert!(parameters(&mut sim).contains("[G28:10.000,10.000,0.000]"));
    run(&mut sim, "G0 X0 Y0");
    sim.take_trace();
    run(&mut sim, "G28");
    assert_position(sim.machine_position(), [10.0, 10.0, 0.0]);
    assert!(sim.take_trace().iter().all(|s| s.rapid));

    // With an intermediate point, only the named axes go home.
    run(&mut sim, "G0 X0 Y0");
    sim.take_trace();
    run(&mut sim, "G28 X5");
    assert_position(sim.machine_position(), [10.0, 0.0, 0.0]);
    assert!(sim.take_trace().iter().any(|s| s.end == [5.0, 0.0, 0.0]));

    run(&mut sim, "G0 X20 Y5\nG30.1\nG0 X0 Y0");
    assert!(parameters(&mut sim).contains("[G30:20.000,5.000,0.000]"));
    sim.take_trace();
    // The intermediate point follows the distance mode.
    run(&mut sim, "G91 G30 Y2\nG90");
    assert_position(sim.machine_position(), [0.0, 5.0, 0.0]);
    assert!(sim.take_trace().iter().any(|s| s.end == [0.0, 2.0, 0.0]));

    // Axis words on G28.1 are a move; the position stored is the one before it.
    run(&mut sim, "G28.1 X3");
    assert_position(sim.machine_position(), [3.0, 5.0, 0.0]);
    assert!(parameters(&mut sim).contains("[G28:0.000,5.000,0.000]"));
}

#[test]
fn g92_1_clears_the_g92_offset() {
    let mut sim = laser();
    run(&mut sim, "G0 X10\nG92 X0");
    assert!(parameters(&mut sim).contains("[G92:10.000,0.000,0.000]"));
    run(&mut sim, "G92.1");
    assert!(parameters(&mut sim).contains("[G92:0.000,0.000,0.000]"));
    assert_position(sim.work_position(), [10.0, 0.0, 0.0]);
    assert!(send(&mut sim, b"?").contains("|WCO:0.000,0.000,0.000"));
}

#[test]
fn g93_inverse_time_finishes_each_move_in_one_over_f_minutes() {
    let mut sim = laser();
    // F60: 1 s per move. 10 mm then runs at 10 mm/s: 0.1 s each way to
    // accelerate and brake, 0.9 s cruising.
    run(&mut sim, "G93 G1 X10 F60");
    let trace = sim.take_trace();
    assert!(close(duration(&trace), 1.1, 1e-6), "{}", duration(&trace));
    assert!(trace.iter().all(|s| close(s.feed, 600.0, 1e-9)));
    // 20 mm at 20 mm/s: 0.2 s each way, 0.8 s cruising.
    run(&mut sim, "G1 X30 F60");
    let trace = sim.take_trace();
    assert!(close(duration(&trace), 1.2, 1e-6), "{}", duration(&trace));
    // Every feed move needs its own F, and F is undefined after G94.
    assert_eq!(send(&mut sim, b"G1 X0\n"), "error:22\r\n");
    assert_eq!(send(&mut sim, b"G94 G1 X0\n"), "error:22\r\n");
    run(&mut sim, "G94 G1 X0 F600");
    sim.take_trace();
    // GRBL multiplies an inverse-time arc's F by its segment count and runs
    // the segments in mm/min: a 39-segment quarter circle at F2 runs at 78.
    run(&mut sim, "G93 G3 X10 Y10 J10 F2");
    let trace = sim.take_trace();
    assert!(!trace.is_empty() && trace.iter().all(|s| close(s.feed, 78.0, 1e-9)));
}

fn on_circle_in(trace: &[TraceSegment], axes: [usize; 2], center: [f64; 2], radius: f64) -> bool {
    trace.iter().all(|s| {
        let r = (s.end[axes[0]] - center[0]).hypot(s.end[axes[1]] - center[1]);
        close(r, radius, 0.01)
    })
}

#[test]
fn g18_and_g19_arc_in_the_zx_and_yz_planes() {
    let mut sim = laser();
    // ZX plane: X and Z on the circle, Y moving linearly (a helix).
    run(&mut sim, "G18 G2 X10 Y4 I5 F3000");
    assert_position(sim.machine_position(), [10.0, 4.0, 0.0]);
    let trace = sim.take_trace();
    assert!(on_circle_in(&trace, [0, 2], [5.0, 0.0], 5.0));
    let min_z = trace.iter().map(|s| s.end[2]).fold(f64::MAX, f64::min);
    assert!(close(min_z, -5.0, 0.01), "{min_z}");
    let ys: Vec<f64> = trace.iter().map(|s| s.end[1]).collect();
    assert!(ys.windows(2).all(|w| w[1] >= w[0]));

    // YZ plane, centred with K.
    run(&mut sim, "G0 X0 Y0 Z0\nG19 G2 Z10 K5 F3000");
    assert_position(sim.machine_position(), [0.0, 0.0, 10.0]);
    let trace: Vec<_> = sim.take_trace().into_iter().filter(|s| !s.rapid).collect();
    assert!(on_circle_in(&trace, [1, 2], [0.0, 5.0], 5.0));
    assert!(trace.iter().all(|s| s.end[0] == 0.0));
    let min_y = trace.iter().map(|s| s.end[1]).fold(f64::MAX, f64::min);
    assert!(close(min_y, -5.0, 0.01), "{min_y}");

    // R form in the ZX plane.
    run(&mut sim, "G0 X0 Y0 Z0\nG18 G2 X10 R5");
    let trace: Vec<_> = sim.take_trace().into_iter().filter(|s| !s.rapid).collect();
    assert!(on_circle_in(&trace, [0, 2], [5.0, 0.0], 5.0));
}

/// Holds a running `M3 S1000` burn along X and returns the first two
/// `Hold:0` reports: `WCO:` can put `Ov:` off by one.
fn hold_a_burn(sim: &mut Simulator) -> String {
    sim.feed(b"G1 X50 F600 M3 S1000\n");
    sim.advance(Duration::from_secs(1));
    assert_eq!(sim.state(), MachineState::Run);
    send(sim, b"!");
    sim.advance(Duration::from_secs(2));
    assert_eq!(sim.state(), MachineState::Hold(0));
    sim.take_output();
    send(sim, b"?") + &send(sim, b"?")
}

#[test]
fn without_laser_mode_a_hold_keeps_the_laser_on_until_the_spindle_stop_override() {
    let mut sim = laser_with(&[(32, 0.0)]);
    hold_a_burn(&mut sim);
    // Without laser mode GRBL leaves the spindle running through a hold.
    assert_eq!(sim.laser_power(), 1.0);

    // 0x9E stops it; GRBL reports the change in the next report.
    assert_eq!(send(&mut sim, &[0x9E]), "");
    assert_eq!(sim.laser_power(), 0.0);
    let report = send(&mut sim, b"?");
    assert!(report.contains("|Ov:"), "{report}");
    assert!(!report.contains("|A:S"), "{report}");
    sim.advance(Duration::from_secs(5));
    assert_eq!(sim.laser_power(), 0.0);

    // Resuming restores it with the program's power.
    assert_eq!(send(&mut sim, b"~"), "[MSG:Restoring spindle]\r\n");
    assert_eq!(sim.laser_power(), 1.0);
    finish(&mut sim);
    assert_position(sim.machine_position(), [50.0, 0.0, 0.0]);
}

#[test]
fn the_spindle_stop_override_toggles_and_only_acts_in_a_hold() {
    // In laser mode the hold has already stopped the laser, and reports it.
    let mut sim = laser();
    let report = hold_a_burn(&mut sim);
    assert!(
        report.contains("|Ov:") && !report.contains("|A:S"),
        "{report}"
    );
    assert_eq!(sim.laser_power(), 0.0);
    // So 0x9E is the second toggle: it restores the laser, head still.
    assert_eq!(send(&mut sim, &[0x9E]), "[MSG:Restoring spindle]\r\n");
    assert_eq!(sim.laser_power(), 1.0);
    // ...and a third stops it again.
    send(&mut sim, &[0x9E]);
    assert_eq!(sim.laser_power(), 0.0);

    // Outside a hold it does nothing.
    let mut sim = laser_with(&[(32, 0.0)]);
    sim.feed(b"G1 X50 F600 M3 S1000\n");
    sim.advance(Duration::from_secs(1));
    send(&mut sim, &[0x9E]);
    assert_eq!(sim.laser_power(), 1.0);
}

#[test]
fn m0_holds_until_cycle_start_with_the_laser_off() {
    let mut sim = laser();
    sim.feed(b"G1 X10 F600 M3 S1000\nM0\nG1 X20\nM5\n");
    sim.advance(Duration::from_secs(3));
    assert_eq!(sim.state(), MachineState::Hold(0));
    assert_position(sim.machine_position(), [10.0, 0.0, 0.0]);
    assert_eq!(
        String::from_utf8(sim.take_output()).unwrap(),
        "ok\r\n",
        "M0 is acknowledged only after resuming"
    );
    // GRBL turns the laser off during a hold in laser mode.
    let report = send(&mut sim, b"?");
    assert!(report.starts_with("<Hold:0|"), "{report}");
    assert_eq!(fs(&report), (0.0, 0.0));
    sim.advance(Duration::from_secs(10));
    assert_eq!(sim.state(), MachineState::Hold(0));
    assert_eq!(
        send(&mut sim, b"~"),
        "[MSG:Restoring spindle]\r\nok\r\nok\r\n"
    );
    assert_eq!(sim.state(), MachineState::Run);
    finish(&mut sim);
    assert_position(sim.machine_position(), [20.0, 0.0, 0.0]);
    let trace = sim.trace();
    assert!(!trace.iter().any(TraceSegment::is_stationary), "{trace:?}");
    assert!(
        trace
            .iter()
            .filter(|s| s.start[0] >= 10.0)
            .all(|s| s.power == 1.0)
    );

    // Check mode parses M0 without pausing.
    let mut sim = laser();
    send(&mut sim, b"$C\n");
    assert_eq!(send(&mut sim, b"M0\n"), "ok\r\n");
    assert_eq!(sim.state(), MachineState::Check);
}

// ---- stationary burns ----

#[test]
fn m3_and_a_dwell_leave_a_stationary_burn() {
    let mut sim = laser();
    // In laser mode M3 only fires with G1-G3 active.
    sim.feed(b"G1 F600\nM3 S500\nG4 P1\nM5\n");
    sim.advance(Duration::from_secs(2));
    let trace = sim.trace();
    assert_eq!(trace.len(), 1, "{trace:?}");
    let burn = &trace[0];
    assert!(burn.is_stationary());
    assert_eq!(burn.start, [0.0, 0.0, 0.0]);
    assert_eq!(burn.end, burn.start);
    assert_eq!(burn.t1 - burn.t0, Duration::from_secs(1));
    assert!(close(burn.power, 0.5, 1e-9));
    assert_eq!(burn.laser_mode, LaserMode::Constant);
    assert_eq!((burn.start_speed, burn.end_speed), (0.0, 0.0));

    // Without laser mode the spindle output stays on whatever the motion.
    let mut sim = laser_with(&[(32, 0.0)]);
    sim.feed(b"M3 S250\nG4 P1\nM5\n");
    sim.advance(Duration::from_secs(2));
    let trace = sim.trace();
    assert_eq!(trace.len(), 1, "{trace:?}");
    assert_eq!(trace[0].t1 - trace[0].t0, Duration::from_secs(1));
    assert!(close(trace[0].power, 0.25, 1e-9));
}

#[test]
fn an_m4_laser_is_off_while_stationary() {
    let mut sim = laser();
    sim.feed(b"G1 F600\nM4 S500\nG4 P1\nM5\n");
    sim.advance(Duration::from_secs(2));
    assert!(sim.trace().is_empty(), "{:?}", sim.trace());
}

#[test]
fn a_dwell_between_m3_moves_burns_where_the_head_stopped() {
    let mut sim = laser();
    sim.feed(b"G1 X10 F600 M3 S1000\nG4 P0.5\nG1 X20\nM5\n");
    sim.advance(Duration::from_secs(5));
    let trace = sim.trace();
    let burns: Vec<_> = trace.iter().filter(|s| s.is_stationary()).collect();
    assert_eq!(burns.len(), 1, "{trace:?}");
    assert_eq!(burns[0].start, [10.0, 0.0, 0.0]);
    assert!(close((burns[0].t1 - burns[0].t0).as_secs_f64(), 0.5, 1e-9));
    assert_eq!(burns[0].power, 1.0);
    for pair in trace.windows(2) {
        assert!(pair[0].t1 <= pair[1].t0);
        assert_position(pair[1].start, pair[0].end);
    }
}

// ---- soft limits ----

fn soft_limited() -> Simulator {
    let mut sim = laser_with(&[(20, 1.0), (22, 1.0), (130, 300.0), (131, 200.0)]);
    send(&mut sim, b"$H\n");
    assert_position(sim.machine_position(), [-1.0, -1.0, -1.0]);
    sim
}

#[test]
fn soft_limit_violation_raises_alarm_2() {
    let mut sim = soft_limited();
    assert_eq!(
        send(&mut sim, b"G0 X5\n"),
        "ALARM:2\r\n[MSG:Reset to continue]\r\n",
        "machine space is -300..0, so X5 is outside"
    );
    assert_eq!(sim.state(), MachineState::Alarm);
    assert_position(sim.machine_position(), [-1.0, -1.0, -1.0]);
    // Everything but reset is ignored, even status reports.
    assert_eq!(send(&mut sim, b"$$\n?G0X-5\n"), "");
    assert_eq!(
        send(&mut sim, &[0x18]),
        format!("{BANNER}[MSG:'$H'|'$X' to unlock]\r\n")
    );
    assert_eq!(sim.state(), MachineState::Alarm);
    assert_eq!(send(&mut sim, b"$X\n"), "[MSG:Caution: Unlocked]\r\nok\r\n");
    run(&mut sim, "G0 X-5");
    assert_position(sim.machine_position(), [-5.0, -1.0, -1.0]);
}

#[test]
fn a_soft_limit_mid_job_stops_under_control_first() {
    let mut sim = soft_limited();
    send(&mut sim, b"G1 X-100 F6000\n");
    sim.advance(Duration::from_millis(500));
    assert_eq!(
        send(&mut sim, b"G1 X1\n"),
        "",
        "decelerating, not yet alarmed"
    );
    assert_eq!(sim.state(), MachineState::Hold(1));
    sim.advance(Duration::from_secs(2));
    assert_eq!(
        String::from_utf8(sim.take_output()).unwrap(),
        "ALARM:2\r\n[MSG:Reset to continue]\r\n"
    );
    assert_eq!(sim.state(), MachineState::Alarm);
    let x = sim.machine_position()[0];
    assert!(
        x < -1.0 && x > -100.0,
        "stopped short of the queued target: {x}"
    );
}

#[test]
fn soft_limits_check_every_arc_segment() {
    let mut sim = soft_limited();
    run(&mut sim, "G0 X-50 Y-10");
    // A full circle about (-50, -4) with radius 6 reaches Y = 2, past 0.
    let reply = line(&mut sim, "G2 X-50 Y-10 I0 J6 F3000");
    assert!(reply.contains("ALARM:2"), "{reply}");
}

// ---- check mode ----

#[test]
fn check_mode_parses_without_moving() {
    let mut sim = laser();
    send(&mut sim, b"$C\n");
    assert_eq!(
        send(&mut sim, b"G0 X10\nM3 S100\nG4 P10\nG2 X20 R5 F100\n"),
        "ok\r\nok\r\nok\r\nok\r\n"
    );
    assert_eq!(send(&mut sim, b"G5\n"), "error:20\r\n");
    assert_eq!(sim.machine_position(), [0.0, 0.0, 0.0]);
    assert!(sim.trace().is_empty());
}

// ---- trace ----

#[test]
fn trace_segments_join_up_in_space_and_time() {
    let mut sim = laser();
    run(
        &mut sim,
        "M4 S800 M8\nG0 X5 Y5\nG1 X15 F2400\nG2 X25 Y5 R5\nG1 Y0",
    );
    let trace = sim.take_trace();
    for pair in trace.windows(2) {
        assert!(pair[0].t1 <= pair[1].t0);
        assert_position(pair[1].start, pair[0].end);
    }
    assert!(trace.iter().all(|s| s.air_assist));
    assert!(sim.trace().is_empty(), "take_trace drains it");
}

#[test]
fn trace_segments_name_the_line_that_queued_them() {
    let mut sim = laser();
    // Lines 1-2 are blank and `$` lines; 4 is the G0, 5 the G1, 7 the dwell.
    // \r\n ends two lines, as in GRBL.
    sim.feed(b"\n$G\nM3 S1000 G1 F600\nG0 X5\nG1 X10\r\nG4 P0.5\nM5\n");
    sim.advance(Duration::from_secs(5));
    let lines: Vec<_> = sim
        .trace()
        .iter()
        .map(|s| (s.line, s.rapid, s.is_stationary()))
        .collect();
    assert_eq!(lines.first(), Some(&(4, true, false)));
    assert!(lines.contains(&(5, false, false)), "{lines:?}");
    assert_eq!(lines.last(), Some(&(7, false, true)));
    assert_eq!(sim.laser_power(), 0.0);

    // A reset flushes unread lines, but numbering continues from the wire.
    sim.feed(b"G4 P10\nG0 X0\n\x18");
    sim.advance(Duration::from_millis(10));
    sim.take_trace();
    sim.feed(b"$X\nM8\nG1 X1 F600 M3 S500\n");
    sim.advance(Duration::from_millis(50));
    assert!(sim.air_assist());
    assert_eq!(sim.laser_power(), 0.5);
    sim.advance(Duration::from_secs(1));
    assert_eq!(sim.trace()[0].line, 13);
}

#[test]
fn motion_is_deterministic() {
    let run_once = || {
        let mut sim = laser();
        sim.feed(b"M4 S1000\nG0 X5\nG1 X20 Y10 F3000\nG3 X10 Y20 R10\n");
        for _ in 0..50 {
            sim.advance(Duration::from_millis(37));
            sim.feed(b"?");
        }
        (sim.take_output(), sim.take_trace())
    };
    assert_eq!(run_once(), run_once());
}
