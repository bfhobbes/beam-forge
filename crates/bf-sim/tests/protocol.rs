//! GRBL 1.1 protocol behaviour, driven only through the public byte API.

use std::time::Duration;

use bf_sim::{Event, MachineState, Profile, Simulator};

const BANNER: &str = "\r\nGrbl 1.1h ['$' for help]\r\n";

fn sim() -> Simulator {
    let mut sim = Simulator::new(Profile::grbl_defaults());
    sim.take_output();
    sim
}

fn sim_with(settings: &[(u16, f64)]) -> Simulator {
    let mut profile = Profile::grbl_defaults();
    for &(id, value) in settings {
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

fn status(sim: &mut Simulator) -> String {
    send(sim, b"?")
}

// ---- banner and reset ----

#[test]
fn power_up_prints_the_banner() {
    let mut sim = Simulator::new(Profile::grbl_defaults());
    assert_eq!(String::from_utf8(sim.take_output()).unwrap(), BANNER);
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn soft_reset_prints_the_banner_and_keeps_idle() {
    let mut sim = sim();
    assert_eq!(send(&mut sim, &[0x18]), BANNER);
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn reset_during_a_jog_raises_alarm_3() {
    let mut sim = sim();
    send(&mut sim, b"$J=G91X10F600\n");
    sim.advance(Duration::from_millis(100));
    assert_eq!(
        send(&mut sim, &[0x18]),
        format!("ALARM:3\r\n{BANNER}[MSG:'$H'|'$X' to unlock]\r\n")
    );
    assert_eq!(sim.state(), MachineState::Alarm);
}

#[test]
fn reset_after_a_completed_hold_keeps_idle() {
    let mut sim = sim();
    send(&mut sim, b"G1 X10 F600\nG1 X20\n");
    sim.advance(Duration::from_millis(300));
    send(&mut sim, b"!");
    sim.advance(Duration::from_millis(500));
    assert_eq!(sim.state(), MachineState::Hold(0));
    assert!(sim.planner_queued() > 0);

    // The head is at rest where GRBL knows it is, so nothing is lost.
    assert_eq!(send(&mut sim, &[0x18]), BANNER);
    assert_eq!(sim.state(), MachineState::Idle);
}

// ---- line protocol ----

#[test]
fn acknowledges_lines_and_reports_errors() {
    let mut sim = sim();
    assert_eq!(send(&mut sim, b"G1 X10 F100\n"), "ok\r\n");
    // GRBL refuses most $ commands mid-cycle (error:8), so let the move finish.
    sim.advance(Duration::from_secs(10));
    assert_eq!(sim.state(), MachineState::Idle);
    assert_eq!(
        send(&mut sim, b"\n"),
        "ok\r\n",
        "empty lines are acknowledged"
    );
    assert_eq!(send(&mut sim, b"(comment only)\n"), "ok\r\n");
    // GRBL parses a setting number first, so an unknown letter is a bad number.
    assert_eq!(send(&mut sim, b"$Q\n"), "error:2\r\n");
    assert_eq!(send(&mut sim, b"$GX\n"), "error:3\r\n");
    assert_eq!(send(&mut sim, b"$SLEEP\n"), "error:3\r\n");
    assert_eq!(send(&mut sim, b"$JX1F100\n"), "error:3\r\n");
    assert_eq!(send(&mut sim, b"$100=abc\n"), "error:2\r\n");
    assert_eq!(send(&mut sim, b"$100=-5\n"), "error:4\r\n");
    let long = format!("G1X{}\n", "1".repeat(90));
    assert_eq!(send(&mut sim, long.as_bytes()), "error:11\r\n");
}

#[test]
fn carriage_returns_end_lines_too() {
    let mut sim = sim();
    // GRBL treats \r and \n each as end of line, so CRLF yields two acks.
    assert_eq!(send(&mut sim, b"G1X1F100\r\n"), "ok\r\nok\r\n");
    assert_eq!(
        send(&mut sim, b"/G1X1F100\n"),
        "ok\r\n",
        "block delete ignored"
    );
}

#[test]
fn lines_are_acknowledged_only_when_the_main_loop_runs() {
    let mut sim = sim();
    sim.feed(b"G1X1F100\nG1X2\n");
    assert!(sim.take_output().is_empty());
    sim.advance(Duration::ZERO);
    assert_eq!(sim.take_output(), b"ok\r\nok\r\n");
}

// ---- RX buffer ----

#[test]
fn status_reports_rx_buffer_space() {
    let mut sim = sim_with(&[(10, 2.0)]);
    sim.feed(b"G1X1F100\n"); // 9 bytes waiting
    sim.feed(b"?");
    // The status request is handled before the waiting line is read.
    sim.advance(Duration::ZERO);
    let out = String::from_utf8(sim.take_output()).unwrap();
    assert!(
        out.starts_with("<Idle|WPos:0.000,0.000,0.000|Bf:15,119|"),
        "{out}"
    );
    assert!(out.ends_with("ok\r\n"));
}

#[test]
fn overflowing_the_rx_buffer_drops_bytes_and_records_an_event() {
    let mut sim = sim();
    let flood = vec![b'G'; 130];
    sim.feed(&flood);
    let dropped: Vec<usize> = sim
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::RxOverflow { dropped, .. } => Some(*dropped),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, vec![2], "128 bytes fit; 2 are lost");
}

#[test]
fn realtime_bytes_do_not_use_rx_buffer_space() {
    let mut sim = sim();
    let mut bytes = vec![b'G'; 128];
    bytes.extend_from_slice(b"????");
    sim.feed(&bytes);
    assert!(
        !sim.events()
            .iter()
            .any(|e| matches!(e, Event::RxOverflow { .. }))
    );
}

// ---- realtime commands ----

#[test]
fn feed_hold_and_cycle_start() {
    let mut sim = sim();
    send(&mut sim, b"!");
    assert_eq!(sim.state(), MachineState::Hold(0));
    send(&mut sim, b"~");
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn realtime_commands_work_mid_line() {
    let mut sim = sim();
    let out = send(&mut sim, b"G1X?10F100\n");
    assert!(out.starts_with("<Idle|"), "{out}");
    assert!(out.ends_with("ok\r\n"));
}

#[test]
fn overrides_change_and_clamp() {
    let mut sim = sim();
    send(&mut sim, &[0x91, 0x91, 0x93]);
    assert_eq!(sim.overrides().feed, 121);
    send(&mut sim, &[0x92; 30]);
    assert_eq!(sim.overrides().feed, 10, "feed override floors at 10%");
    send(&mut sim, &[0x90, 0x97, 0x9A]);
    let o = sim.overrides();
    assert_eq!((o.feed, o.rapid, o.spindle), (100, 25, 110));
    send(&mut sim, &[0x99, 0x95]);
    assert_eq!((sim.overrides().spindle, sim.overrides().rapid), (100, 100));
}

#[test]
fn changed_overrides_and_coolant_appear_in_the_next_report() {
    let mut sim = sim();
    status(&mut sim); // WCO
    status(&mut sim); // Ov
    assert!(!status(&mut sim).contains("Ov:"));
    send(&mut sim, &[0x91, 0xA0]);
    let report = status(&mut sim);
    assert!(report.contains("|Ov:110,100,100|A:F>"), "{report}");
}

#[test]
fn safety_door_holds_until_closed_and_resumed() {
    let mut sim = sim();
    sim.set_door_open(true);
    assert_eq!(sim.state(), MachineState::Door(1));
    send(&mut sim, b"~");
    assert_eq!(
        sim.state(),
        MachineState::Door(1),
        "cannot resume while open"
    );
    sim.set_door_open(false);
    assert_eq!(sim.state(), MachineState::Door(0));
    send(&mut sim, b"~");
    assert_eq!(sim.state(), MachineState::Idle);

    send(&mut sim, &[0x84]);
    assert_eq!(
        sim.state(),
        MachineState::Door(0),
        "door command with door closed"
    );
}

// ---- status reports ----

#[test]
fn status_report_format_follows_the_status_mask() {
    let mut sim = sim_with(&[(10, 0.0)]);
    assert_eq!(
        status(&mut sim),
        "<Idle|WPos:0.000,0.000,0.000|FS:0,0|WCO:0.000,0.000,0.000>\r\n"
    );
    assert_eq!(
        status(&mut sim),
        "<Idle|WPos:0.000,0.000,0.000|FS:0,0|Ov:100,100,100>\r\n"
    );
    assert_eq!(status(&mut sim), "<Idle|WPos:0.000,0.000,0.000|FS:0,0>\r\n");

    let mut sim = sim_with(&[(10, 1.0)]);
    assert!(status(&mut sim).starts_with("<Idle|MPos:0.000,0.000,0.000|FS:0,0|"));

    let mut sim = sim_with(&[(10, 3.0)]);
    assert!(status(&mut sim).starts_with("<Idle|MPos:0.000,0.000,0.000|Bf:15,128|FS:0,0|"));
}

#[test]
fn wco_and_overrides_refresh_on_grbl_schedule_when_idle() {
    let mut sim = sim();
    let reports: Vec<String> = (0..21).map(|_| status(&mut sim)).collect();
    let wco: Vec<usize> = (0..21).filter(|&i| reports[i].contains("WCO:")).collect();
    let ov: Vec<usize> = (0..21).filter(|&i| reports[i].contains("Ov:")).collect();
    assert_eq!(wco, vec![0, 10, 20]);
    assert_eq!(ov, vec![1, 11]);
}

// ---- $ commands ----

#[test]
fn dollar_dollar_lists_settings_and_writes_persist() {
    let mut sim = sim();
    let out = send(&mut sim, b"$$\n");
    assert!(out.starts_with("$0=10\r\n$1=25\r\n"));
    assert!(out.contains("$32=0\r\n"));
    assert!(out.ends_with("$132=200.000\r\nok\r\n"));
    assert_eq!(send(&mut sim, b"$32=1\n"), "ok\r\n");
    assert!(send(&mut sim, b"$$\n").contains("$32=1\r\n"));
    assert_eq!(send(&mut sim, b"$999=1\n"), "error:3\r\n");
}

#[test]
fn info_parser_state_parameters_and_help() {
    let mut sim = sim();
    assert_eq!(
        send(&mut sim, b"$I\n"),
        "[VER:1.1h.20190825:]\r\n[OPT:VML,15,128]\r\nok\r\n"
    );
    assert_eq!(
        send(&mut sim, b"$G\n"),
        "[GC:G0 G54 G17 G21 G90 G94 M5 M9 T0 F0 S0]\r\nok\r\n"
    );
    let params = send(&mut sim, b"$#\n");
    assert!(params.starts_with("[G54:0.000,0.000,0.000]\r\n"));
    assert!(params.contains("[G92:0.000,0.000,0.000]\r\n[TLO:0.000]\r\n"));
    assert_eq!(send(&mut sim, b"$N\n"), "$N0=\r\n$N1=\r\nok\r\n");
    assert!(send(&mut sim, b"$\n").starts_with("[HLP:"));
}

#[test]
fn check_mode_toggles_and_resets_on_exit() {
    let mut sim = sim();
    assert_eq!(send(&mut sim, b"$C\n"), "[MSG:Enabled]\r\nok\r\n");
    assert_eq!(sim.state(), MachineState::Check);
    assert!(
        send(&mut sim, b"$$\n").ends_with("ok\r\n"),
        "$$ is allowed outside a cycle"
    );
    assert_eq!(
        send(&mut sim, b"$#\n"),
        "error:8\r\n",
        "$# needs idle or alarm"
    );
    assert_eq!(
        send(&mut sim, b"$C\n"),
        format!("[MSG:Disabled]\r\nok\r\n{BANNER}")
    );
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn sleep_stops_reading_until_reset() {
    let mut sim = sim();
    assert_eq!(send(&mut sim, b"$SLP\n"), "ok\r\n[MSG:Sleeping]\r\n");
    assert_eq!(send(&mut sim, b"G1X1F100\n"), "", "lines wait while asleep");
    let out = send(&mut sim, &[0x18]);
    assert!(out.starts_with(BANNER), "{out}");
    assert_eq!(sim.state(), MachineState::Alarm, "waking from sleep locks");
}

// ---- alarm lock and homing ----

#[test]
fn homing_enabled_locks_at_power_up_until_unlocked() {
    let mut profile = Profile::grbl_defaults();
    profile.settings.set(22, 1.0).unwrap();
    let mut sim = Simulator::new(profile);
    assert_eq!(
        String::from_utf8(sim.take_output()).unwrap(),
        format!("{BANNER}[MSG:'$H'|'$X' to unlock]\r\n")
    );
    assert_eq!(sim.state(), MachineState::Alarm);
    assert_eq!(send(&mut sim, b"G1X1F100\n"), "error:9\r\n");
    assert!(
        send(&mut sim, b"$$\n").ends_with("ok\r\n"),
        "$$ works in alarm"
    );
    assert_eq!(send(&mut sim, b"$X\n"), "[MSG:Caution: Unlocked]\r\nok\r\n");
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn homing_sets_grbl_machine_origin() {
    let mut sim = sim_with(&[(22, 1.0)]);
    assert_eq!(send(&mut sim, b"$H\n"), "ok\r\n");
    assert_eq!(sim.machine_position(), [-1.0, -1.0, -1.0]);
    assert_eq!(sim.state(), MachineState::Idle);

    // $23=3 homes X and Y towards negative: they sit at -max_travel + pulloff.
    let mut sim = sim_with(&[(22, 1.0), (23, 3.0)]);
    send(&mut sim, b"$X\n");
    send(&mut sim, b"$H\n");
    assert_eq!(sim.machine_position(), [-199.0, -199.0, -1.0]);
}

#[test]
fn unlock_and_homing_are_refused_while_the_door_is_open() {
    let mut sim = sim_with(&[(22, 1.0)]);
    sim.set_door_open(true);
    assert_eq!(
        sim.state(),
        MachineState::Alarm,
        "an open door does not clear an alarm"
    );
    assert_eq!(send(&mut sim, b"$X\n"), "error:13\r\n");
    assert_eq!(send(&mut sim, b"$H\n"), "error:13\r\n");
}

#[test]
fn restore_defaults_resets_settings_and_controller() {
    let mut sim = sim();
    send(&mut sim, b"$32=1\n");
    assert_eq!(
        send(&mut sim, b"$RST=$\n"),
        format!("[MSG:Restoring defaults]\r\nok\r\n{BANNER}")
    );
    assert!(send(&mut sim, b"$$\n").contains("$32=0\r\n"));
    assert_eq!(send(&mut sim, b"$RST=X\n"), "error:3\r\n");
}

#[test]
fn homing_requires_it_to_be_enabled() {
    let mut sim = sim();
    assert_eq!(send(&mut sim, b"$H\n"), "error:5\r\n");
}

// ---- jogging ----

#[test]
fn jog_moves_over_time_and_finishes_idle() {
    let mut sim = sim();
    assert_eq!(send(&mut sim, b"$J=G91X10F600\n"), "ok\r\n");
    assert_eq!(sim.state(), MachineState::Jog);
    // F600 is capped at the 500 mm/min max rate. Accelerating at 10 mm/s^2,
    // after 0.5 s the head is at 1.25 mm doing 5 mm/s.
    sim.advance(Duration::from_millis(500));
    assert!((sim.machine_position()[0] - 1.25).abs() < 1e-9);
    let report = status(&mut sim);
    assert!(
        report.starts_with("<Jog|MPos:1.250,0.000,0.000|FS:300,0"),
        "{report}"
    );
    sim.advance(Duration::from_secs(2));
    assert_eq!(sim.machine_position(), [10.0, 0.0, 0.0]);
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn jog_cancel_decelerates_then_discards_queued_jogs() {
    let mut sim = sim();
    send(&mut sim, b"$J=G91X10F600\n$J=G91Y10F600\n");
    // 0.3 s into the jog: 0.45 mm at 3 mm/s.
    sim.advance(Duration::from_millis(300));
    send(&mut sim, &[0x85]);
    assert_eq!(sim.state(), MachineState::Jog, "still braking");
    sim.advance(Duration::from_secs(5));
    assert_eq!(sim.state(), MachineState::Idle);
    // Braking from 3 mm/s at 10 mm/s^2 takes another 0.45 mm.
    assert!((sim.machine_position()[0] - 0.9).abs() < 1e-9);
    assert_eq!(sim.machine_position()[1], 0.0, "queued jogs are discarded");
}

#[test]
fn feed_hold_cancels_a_jog() {
    let mut sim = sim();
    send(&mut sim, b"$J=G91X10F600\n");
    send(&mut sim, b"!");
    assert_eq!(sim.state(), MachineState::Idle);
}

#[test]
fn jog_validation_matches_grbl() {
    let mut sim = sim();
    assert_eq!(
        send(&mut sim, b"$J=G91X10\n"),
        "error:22\r\n",
        "feed required"
    );
    assert_eq!(
        send(&mut sim, b"$J=F100\n"),
        "error:26\r\n",
        "axis required"
    );
    assert_eq!(send(&mut sim, b"$J=M3X1F100\n"), "error:16\r\n");
    assert_eq!(send(&mut sim, b"$J=G20G91X1F10\n"), "ok\r\n");
    sim.advance(Duration::from_secs(60));
    assert!((sim.machine_position()[0] - 25.4).abs() < 1e-9, "inches");

    let mut sim = sim_with(&[(20, 1.0)]);
    assert_eq!(
        send(&mut sim, b"$J=G53X5F100\n"),
        "error:15\r\n",
        "soft limits"
    );
    assert_eq!(send(&mut sim, b"$J=G53X-5F100\n"), "ok\r\n");
}

#[test]
fn jogging_is_refused_in_alarm() {
    let mut sim = sim_with(&[(22, 1.0)]);
    sim.power_up();
    sim.take_output();
    assert_eq!(send(&mut sim, b"$J=G91X1F100\n"), "error:8\r\n");
}

// ---- real jobs ----

#[test]
fn a_sample_grbl_job_streams_without_errors() {
    let mut sim = Simulator::new(Profile::diode_laser());
    sim.take_output();
    let mut out = String::new();
    // Send-response streaming: each line waits for its acknowledgement.
    for line in include_str!("fixtures/sample-job.gc").lines() {
        sim.feed(format!("{line}\n").as_bytes());
        let reply = loop {
            sim.advance(Duration::from_millis(1));
            out += &String::from_utf8(sim.take_output()).unwrap();
            if out.ends_with("ok\r\n") || out.contains("error:") || out.contains("ALARM:") {
                break std::mem::take(&mut out);
            }
        };
        assert!(reply.ends_with("ok\r\n"), "{line:?} -> {reply:?}");
    }
    while sim.state() != MachineState::Idle {
        sim.advance(Duration::from_millis(10));
    }
    assert_eq!(sim.machine_position(), [0.0, 0.0, 0.0]);
    assert!(sim.trace().iter().any(|s| s.power > 0.0));
}

// ---- determinism ----

#[test]
fn identical_input_gives_identical_output() {
    let script: &[(&[u8], u64)] = &[
        (b"$$\n", 0),
        (b"$J=G91X10Y5F900\n", 10),
        (b"?", 200),
        (&[0x91], 0),
        (b"?", 300),
        (&[0x85], 0),
        (b"$J=G91X-3F300\n?", 1000),
        (&[0x18], 5),
        (b"$I\n?", 0),
    ];
    let run = || {
        let mut sim = Simulator::new(Profile::diode_laser());
        for &(bytes, ms) in script {
            sim.feed(bytes);
            sim.advance(Duration::from_millis(ms));
        }
        (sim.take_output(), sim.events().to_vec())
    };
    assert_eq!(run(), run());
}
