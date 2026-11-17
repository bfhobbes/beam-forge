//! Fault injection, driven through the public API, asserting the bytes
//! the host sees.

use std::time::Duration;

use bf_sim::{Event, Fault, MachineState, Profile, Scenario, Simulator};

const BANNER: &str = "\r\nGrbl 1.1h ['$' for help]\r\n";
const UNLOCK: &str = "[MSG:'$H'|'$X' to unlock]\r\n";
const MS: Duration = Duration::from_millis(1);

fn sim() -> Simulator {
    let mut sim = Simulator::new(Profile::diode_laser());
    sim.take_output();
    sim
}

/// Sends bytes and processes them without moving the clock.
fn send(sim: &mut Simulator, bytes: &[u8]) -> String {
    sim.feed(bytes);
    run(sim, Duration::ZERO)
}

/// Runs the clock and returns what the host received.
fn run(sim: &mut Simulator, dt: Duration) -> String {
    sim.advance(dt);
    String::from_utf8(sim.take_output()).unwrap()
}

fn inject(sim: &mut Simulator, fault: Fault) -> String {
    sim.inject(fault);
    run(sim, Duration::ZERO)
}

/// A burn in progress: laser on, air on, moving.
fn burning(sim: &mut Simulator) {
    assert_eq!(
        send(sim, b"G1 X50 F600 M3 S500 M8\nG1 X100\n"),
        "ok\r\nok\r\n"
    );
    run(sim, Duration::from_millis(500));
    assert_eq!(sim.state(), MachineState::Run);
    assert!(sim.laser_power() > 0.0 && sim.air_assist());
}

// ---- alarms ----

#[test]
fn hard_limit_alarm_stops_dead_and_waits_for_reset() {
    let mut sim = sim();
    burning(&mut sim);
    let x = sim.machine_position()[0];
    assert_eq!(
        inject(&mut sim, Fault::Alarm(1)),
        "ALARM:1\r\n[MSG:Reset to continue]\r\n"
    );
    assert_eq!(sim.state(), MachineState::Alarm);
    assert_eq!(sim.laser_power(), 0.0);
    assert!(!sim.air_assist());
    run(&mut sim, Duration::from_secs(1));
    assert_eq!(sim.machine_position()[0], x, "the head stopped dead");
    // Everything but a reset is ignored, status reports included.
    assert_eq!(send(&mut sim, b"G0 X0\n?$X\n"), "");
    assert_eq!(send(&mut sim, &[0x18]), format!("{BANNER}{UNLOCK}"));
    assert_eq!(send(&mut sim, b"$X\n"), "[MSG:Caution: Unlocked]\r\nok\r\n");
}

#[test]
fn soft_limit_alarm_decelerates_first() {
    let mut sim = sim();
    burning(&mut sim);
    assert_eq!(inject(&mut sim, Fault::Alarm(2)), "");
    assert_eq!(sim.state(), MachineState::Hold(1));
    assert_eq!(
        run(&mut sim, Duration::from_secs(1)),
        "ALARM:2\r\n[MSG:Reset to continue]\r\n"
    );
    assert_eq!(sim.laser_power(), 0.0);
    assert_eq!(send(&mut sim, b"?"), "");
}

#[test]
fn other_alarms_reset_the_controller_into_alarm() {
    for code in 3..=9 {
        let mut sim = sim();
        burning(&mut sim);
        assert_eq!(
            inject(&mut sim, Fault::Alarm(code)),
            format!("ALARM:{code}\r\n{BANNER}{UNLOCK}")
        );
        assert_eq!(sim.state(), MachineState::Alarm);
        assert_eq!((sim.laser_power(), sim.air_assist()), (0.0, false));
        assert_eq!(sim.planner_queued(), 0);
        assert_eq!(send(&mut sim, b"G0 X0\n"), "error:9\r\n");
    }
}

// ---- errors ----

#[test]
fn the_next_line_is_answered_with_an_error_and_not_run() {
    let mut sim = sim();
    sim.inject(Fault::Error { code: 20, line: 1 });
    assert_eq!(send(&mut sim, b"$J=X10F600\n$G\n"), {
        "error:20\r\n[GC:G0 G54 G17 G21 G90 G94 M5 M9 T0 F0 S0]\r\nok\r\n"
    });
    assert_eq!(sim.state(), MachineState::Idle, "the jog never ran");
}

#[test]
fn a_later_line_can_be_answered_with_an_error() {
    let mut sim = sim();
    sim.inject(Fault::Error { code: 33, line: 3 });
    assert_eq!(
        send(&mut sim, b"G21\nG90\nG0 X1\nG0 X2\n"),
        "ok\r\nok\r\nerror:33\r\nok\r\n"
    );
}

// ---- switches ----

#[test]
fn the_door_holds_a_job_until_closed_and_resumed() {
    let mut sim = sim();
    burning(&mut sim);
    inject(&mut sim, Fault::Door { open: true });
    assert_eq!(sim.state(), MachineState::Door(1));
    run(&mut sim, Duration::from_secs(1));
    assert!(send(&mut sim, b"?").starts_with("<Door:1|"));
    inject(&mut sim, Fault::Door { open: false });
    assert!(send(&mut sim, b"?").starts_with("<Door:0|"));
    send(&mut sim, b"~");
    assert_eq!(sim.state(), MachineState::Run);
}

#[test]
fn the_feed_hold_button_holds_like_a_host_hold() {
    let mut sim = sim();
    burning(&mut sim);
    inject(&mut sim, Fault::FeedHold);
    assert_eq!(sim.state(), MachineState::Hold(1));
    run(&mut sim, Duration::from_secs(1));
    assert!(send(&mut sim, b"?").starts_with("<Hold:0|"));
    // A laser-mode hold turns the laser off until resumed.
    assert_eq!(sim.laser_power(), 0.0);
}

// ---- the link ----

#[test]
fn a_pulled_cable_loses_traffic_but_the_job_runs_on() {
    let mut sim = sim();
    burning(&mut sim);
    assert_eq!(inject(&mut sim, Fault::Disconnect), "");
    assert!(sim.is_disconnected());
    // Host bytes never arrive, and the controller's output goes nowhere.
    assert_eq!(send(&mut sim, b"!M5\n?"), "");
    assert_eq!(sim.rx_buffered(), 0);
    let x = sim.machine_position()[0];
    run(&mut sim, Duration::from_secs(1));
    assert_eq!(sim.state(), MachineState::Run);
    assert!(sim.machine_position()[0] > x && sim.laser_power() > 0.0);
    // Plugging back in power-cycles the board.
    sim.power_up();
    assert!(!sim.is_disconnected());
    assert_eq!(run(&mut sim, Duration::ZERO), BANNER);
}

#[test]
fn a_stall_stops_acknowledging_lines_but_answers_status() {
    let mut sim = sim();
    sim.inject(Fault::Stall(Duration::from_secs(2)));
    assert_eq!(send(&mut sim, b"G0 X1\nG0 X2\n"), "");
    assert!(send(&mut sim, b"?").starts_with("<Idle|"));
    assert_eq!(run(&mut sim, 1999 * MS), "");
    assert_eq!(sim.rx_buffered(), 12);
    assert_eq!(run(&mut sim, MS), "ok\r\nok\r\n");
    // `stall 0` ends a stall early.
    sim.inject(Fault::Stall(Duration::from_secs(60)));
    assert_eq!(send(&mut sim, b"G0 X3\n"), "");
    assert_eq!(inject(&mut sim, Fault::Stall(Duration::ZERO)), "ok\r\n");
}

#[test]
fn ack_latency_delays_every_line() {
    let mut sim = sim();
    sim.inject(Fault::AckLatency(Duration::from_millis(50)));
    assert_eq!(send(&mut sim, b"G21\nG90\n"), "");
    assert_eq!(run(&mut sim, 49 * MS), "");
    assert_eq!(run(&mut sim, MS), "ok\r\n");
    assert_eq!(run(&mut sim, 49 * MS), "");
    assert_eq!(run(&mut sim, MS), "ok\r\n");
    // One big step still answers each line on its own deadline.
    send(&mut sim, b"G21\nG21\nG21\n");
    sim.advance(Duration::from_secs(1));
    sim.take_output();
    let answered: Vec<_> = sim
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Sent { at, bytes } if bytes == b"ok\r\n" => Some(at.as_millis()),
            _ => None,
        })
        .collect();
    assert_eq!(answered, [50, 100, 150, 200, 250]);
    sim.inject(Fault::AckLatency(Duration::ZERO));
    assert_eq!(send(&mut sim, b"G21\n"), "ok\r\n");
}

#[test]
fn a_shrunk_rx_buffer_overflows_sooner() {
    let mut sim = sim();
    send(&mut sim, b"$10=2\n");
    sim.inject(Fault::RxBuffer(Some(16)));
    assert_eq!(sim.rx_capacity(), 16);
    sim.inject(Fault::Stall(Duration::from_secs(1)));
    sim.feed(b"G21 G90 G94 G17 G54\n");
    assert!(
        sim.events()
            .iter()
            .any(|event| matches!(event, Event::RxOverflow { dropped: 4, .. }))
    );
    assert!(send(&mut sim, b"?").contains("|Bf:15,0|"));
    // `$I` still claims the profile's size, which is what trips the host.
    sim.inject(Fault::Stall(Duration::ZERO));
    sim.take_output();
    sim.inject(Fault::RxBuffer(None));
    assert!(send(&mut sim, b"\n$I\n").contains("[OPT:VML,15,128]"));
    assert_eq!(sim.rx_capacity(), 128);
}

#[test]
fn injected_faults_are_logged_as_events() {
    let mut sim = sim();
    sim.take_events();
    sim.inject(Fault::Door { open: true });
    let event = sim.take_events().into_iter().next();
    assert_eq!(
        event,
        Some(Event::Fault {
            at: Duration::ZERO,
            fault: Fault::Door { open: true }
        })
    );
}

#[test]
fn clearing_faults_ends_them_but_not_the_alarm_they_raised() {
    let mut sim = sim();
    sim.run_scenario(
        "latency 1s\nstall 5s\nrx 8\nerror 3\nafter 9: hold"
            .parse()
            .unwrap(),
    );
    sim.clear_faults();
    assert_eq!(sim.scenario_pending(), 0);
    assert_eq!(sim.rx_capacity(), 128);
    assert_eq!(send(&mut sim, b"G21\n"), "ok\r\n");
}

// ---- scenarios ----

/// A job, and the scenario shared with the simulator app's panel test.
const JOB: &str = include_str!("fixtures/fault-job.gc");
const SCENARIO: &str = include_str!("fixtures/fault-scenario.txt");

#[test]
fn a_scenario_fires_faults_after_the_host_lines_it_names() {
    let mut sim = sim();
    // Lines before the scenario starts don't count.
    send(&mut sim, b"$G\n");
    sim.run_scenario(SCENARIO.parse().unwrap());
    sim.take_output();
    sim.feed(JOB.as_bytes());
    let output = run(&mut sim, Duration::from_secs(30));
    assert_eq!(
        output,
        "ok\r\nok\r\nok\r\nerror:20\r\nALARM:1\r\n[MSG:Reset to continue]\r\n"
    );
    assert_eq!(sim.scenario_pending(), 0);
    assert_eq!(sim.laser_power(), 0.0);
    let faults: Vec<_> = sim
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Fault { fault, .. } => Some(fault.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(faults, ["latency 10ms", "error 20", "alarm 1"]);
}

#[test]
fn a_scenario_step_waits_for_its_line_to_finish() {
    let mut sim = sim();
    let scenario: Scenario = "after 1: hold".parse().unwrap();
    sim.run_scenario(scenario);
    // G4 dwells before its `ok`, so the hold comes after it.
    assert_eq!(send(&mut sim, b"G4 P0.5\n"), "");
    assert_eq!(sim.scenario_pending(), 1);
    assert_eq!(run(&mut sim, Duration::from_secs(1)), "ok\r\n");
    assert_eq!(sim.state(), MachineState::Hold(0));
}

#[test]
fn identical_scenarios_give_identical_output_however_the_clock_is_stepped() {
    let outputs: Vec<String> = [Duration::from_secs(30), Duration::from_millis(7)]
        .into_iter()
        .map(|step| {
            let mut sim = sim();
            sim.run_scenario(SCENARIO.parse().unwrap());
            sim.feed(JOB.as_bytes());
            let mut output = String::new();
            let mut ran = Duration::ZERO;
            while ran < Duration::from_secs(30) {
                output += &run(&mut sim, step);
                ran += step;
            }
            output
        })
        .collect();
    assert_eq!(outputs[0], outputs[1]);
}
