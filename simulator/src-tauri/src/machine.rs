//! One emulated controller on a real-time clock.
//!
//! `bf-sim` is deterministic and only moves when told to. [`Machine`] maps
//! wall-clock time onto it at the chosen [`Speed`], so a host on the TCP link
//! sees a controller that runs in (scaled) real time.

use std::time::{Duration, Instant};

use bf_sim::{Fault, MachineState, Profile, Scenario, Simulator};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::burn::{BurnBatch, Burns};
use crate::inspector::{Inspector, InspectorBatch};

/// How fast simulated time runs relative to the wall clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub enum Speed {
    #[default]
    X1,
    X10,
    X100,
    /// Motion completes as soon as it is queued; the clock advances only by
    /// the time the motion takes.
    Instant,
}

/// Step used to run motion to completion at [`Speed::Instant`].
const INSTANT_STEP: Duration = Duration::from_millis(10);
/// Most simulated time one instant pump may run, so a stuck state can't hang.
const INSTANT_LIMIT: Duration = Duration::from_secs(3600);

/// Machine profiles the UI offers as starting points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub enum Preset {
    DiodeLaser,
    GrblDefaults,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::DiodeLaser, Preset::GrblDefaults];

    pub fn profile(self) -> Profile {
        match self {
            Preset::DiodeLaser => Profile::diode_laser(),
            Preset::GrblDefaults => Profile::grbl_defaults(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct PresetInfo {
    pub id: Preset,
    pub name: String,
}

pub fn presets() -> Vec<PresetInfo> {
    Preset::ALL
        .into_iter()
        .map(|id| PresetInfo {
            id,
            name: id.profile().name,
        })
        .collect()
}

/// A `$` setting as `$$` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct SettingRow {
    pub id: u16,
    pub value: f64,
    /// The value with GRBL's formatting, e.g. `0.010`.
    pub text: String,
}

/// The work area in machine coordinates (mm), from `$130`/`$131`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct Bed {
    /// The bed's lowest X and Y. GRBL's machine space runs from
    /// `-max_travel` to 0 once homed (`$22=1`); an unhomed machine treats
    /// where it powered up as 0, so the bed is drawn from there instead.
    pub origin: [f64; 2],
    pub size: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct MachineSnapshot {
    pub profile: String,
    /// GRBL state as a status report shows it, e.g. `Idle` or `Hold:0`.
    pub state: String,
    pub machine_position: [f64; 3],
    pub work_position: [f64; 3],
    /// Simulated time since the controller was created (app start, preset or
    /// settings change), in milliseconds.
    pub clock_ms: f64,
    pub speed: Speed,
    /// Laser output now, 0 to 1.
    pub laser_power: f64,
    pub air_assist: bool,
    pub bed: Bed,
    /// Scenario steps still to fire; 0 when no scenario is running.
    pub scenario_pending: u32,
}

pub struct Machine {
    sim: Simulator,
    speed: Speed,
    last: Instant,
    inspector: Inspector,
    burns: Burns,
}

impl Machine {
    pub fn new(profile: Profile, now: Instant) -> Self {
        let mut machine = Machine {
            sim: Simulator::new(profile),
            speed: Speed::default(),
            last: now,
            inspector: Inspector::default(),
            burns: Burns::default(),
        };
        machine.note(format!("Started with {}", machine.sim.profile().name));
        machine
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    pub fn set_speed(&mut self, speed: Speed, now: Instant) {
        // Time so far ran at the old speed.
        self.pump(now);
        self.speed = speed;
    }

    /// Receives bytes from the host.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.sim.feed(bytes);
    }

    /// Runs the controller up to `now` and returns what it sent since the
    /// last pump.
    pub fn pump(&mut self, now: Instant) -> Vec<u8> {
        let wall = now.saturating_duration_since(self.last);
        self.last = now;
        match self.speed {
            Speed::X1 => self.sim.advance(wall),
            Speed::X10 => self.sim.advance(wall * 10),
            Speed::X100 => self.sim.advance(wall * 100),
            Speed::Instant => {
                self.sim.advance(Duration::ZERO);
                let mut ran = Duration::ZERO;
                // A stall or ack latency ends in no time too.
                while (busy(self.sim.state()) || self.sim.fault_deadline().is_some())
                    && ran < INSTANT_LIMIT
                {
                    self.sim.advance(INSTANT_STEP);
                    ran += INSTANT_STEP;
                }
            }
        }
        self.inspector.record(self.sim.take_events());
        self.burns.record(self.sim.take_trace());
        self.inspector.sample(
            now,
            self.sim.now(),
            self.sim.rx_buffered(),
            self.sim.planner_queued(),
        );
        self.sim.take_output()
    }

    /// Adds a line to the protocol inspector's log, e.g. a client connecting.
    pub fn note(&mut self, text: impl Into<String>) {
        self.inspector.note(self.sim.now(), text);
    }

    /// Wire traffic from `since` on, for the protocol inspector.
    pub fn inspector(&self, since: u64) -> InspectorBatch {
        let profile = self.sim.profile();
        self.inspector
            .since(since, self.sim.rx_capacity(), profile.planner_blocks)
    }

    /// Injects a fault given in `bf_sim`'s text form, e.g. `alarm 1`: what
    /// the fault panel's buttons send.
    pub fn inject(&mut self, fault: &str) -> Result<(), String> {
        let fault: Fault = fault.parse()?;
        self.sim.inject(fault);
        Ok(())
    }

    /// Starts a scenario script (see `bf_sim::Scenario`) from the panel. Its
    /// lines count from now.
    pub fn run_scenario(&mut self, script: &str) -> Result<(), String> {
        let scenario: Scenario = script.parse().map_err(|e| format!("Scenario {e}"))?;
        self.note(format!(
            "Scenario started: {} faults, lines count from here",
            scenario.steps.len()
        ));
        self.sim.run_scenario(scenario);
        Ok(())
    }

    /// Ends every fault in force and any running scenario.
    pub fn clear_faults(&mut self) {
        self.sim.clear_faults();
        self.note("Faults cleared");
    }

    /// A [`Fault::Disconnect`] pulled the cable: the link should drop the
    /// client.
    pub fn is_disconnected(&self) -> bool {
        self.sim.is_disconnected()
    }

    /// Burn view segments from index `since` on.
    pub fn burns(&self, since: u32) -> BurnBatch {
        self.burns.since(since)
    }

    pub fn clear_burns(&mut self) {
        self.burns.clear();
    }

    /// Power-cycles the controller, as a USB connection or the Reset button
    /// does. Output still pending for the old connection is discarded.
    pub fn power_up(&mut self, now: Instant) {
        self.pump(now);
        self.note("Power-cycled");
        self.sim.power_up();
    }

    /// Replaces the controller's settings and power-cycles it, like flashing
    /// new EEPROM contents.
    pub fn load_profile(&mut self, profile: Profile, now: Instant) {
        self.pump(now);
        self.burns.restart(self.sim.now(), self.inspector.lines());
        self.sim = Simulator::new(profile);
        // The new controller's clock starts again at zero.
        self.note(format!(
            "Loaded {}; clock restarted",
            self.sim.profile().name
        ));
    }

    pub fn load_preset(&mut self, preset: Preset, now: Instant) {
        self.load_profile(preset.profile(), now);
    }

    /// Writes one `$` setting with GRBL's validation, then power-cycles.
    pub fn set_setting(&mut self, id: u16, value: f64, now: Instant) -> Result<(), String> {
        let mut profile = self.sim.profile().clone();
        profile
            .settings
            .set(id, value)
            .map_err(|error| format!("${id}={value} rejected with error:{}", error.code()))?;
        self.load_profile(profile, now);
        Ok(())
    }

    pub fn settings(&self) -> Vec<SettingRow> {
        let settings = self.sim.settings();
        settings
            .report_lines()
            .iter()
            .filter_map(|line| {
                let (id, text) = line.strip_prefix('$')?.split_once('=')?;
                let id = id.parse().ok()?;
                Some(SettingRow {
                    id,
                    value: settings.get(id)?,
                    text: text.to_string(),
                })
            })
            .collect()
    }

    pub fn snapshot(&self) -> MachineSnapshot {
        MachineSnapshot {
            profile: self.sim.profile().name.clone(),
            state: self.sim.state().to_string(),
            machine_position: self.sim.machine_position(),
            work_position: self.sim.work_position(),
            clock_ms: self.sim.now().as_secs_f64() * 1000.0,
            speed: self.speed,
            laser_power: self.sim.laser_power(),
            air_assist: self.sim.air_assist(),
            bed: self.bed(),
            scenario_pending: u32::try_from(self.sim.scenario_pending()).unwrap_or(u32::MAX),
        }
    }

    fn bed(&self) -> Bed {
        let settings = self.sim.settings();
        let size = [130, 131].map(|id| settings.get(id).unwrap_or(0.0));
        let homed = settings.get(22).is_some_and(|homing| homing != 0.0);
        Bed {
            origin: if homed { size.map(|s| -s) } else { [0.0; 2] },
            size,
        }
    }
}

/// States that change on their own as time passes.
fn busy(state: MachineState) -> bool {
    matches!(
        state,
        MachineState::Run | MachineState::Jog | MachineState::Home | MachineState::Hold(1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::Wire;

    const BANNER: &str = "\r\nGrbl 1.1h ['$' for help]\r\n";

    fn machine() -> (Machine, Instant) {
        let start = Instant::now();
        let mut machine = Machine::new(Profile::diode_laser(), start);
        assert_eq!(String::from_utf8(machine.pump(start)).unwrap(), BANNER);
        (machine, start)
    }

    #[test]
    fn speed_scales_wall_time() {
        for (speed, factor) in [(Speed::X1, 1.0), (Speed::X10, 10.0), (Speed::X100, 100.0)] {
            let (mut machine, start) = machine();
            machine.set_speed(speed, start);
            machine.pump(start + Duration::from_millis(50));
            assert_eq!(machine.snapshot().clock_ms, 50.0 * factor);
        }
    }

    #[test]
    fn instant_speed_finishes_a_jog_in_one_pump() {
        let (mut machine, start) = machine();
        machine.set_speed(Speed::Instant, start);
        machine.feed(b"$J=G91X100F600\n");
        assert_eq!(machine.pump(start), b"ok\r\n");
        let snapshot = machine.snapshot();
        assert_eq!(snapshot.state, "Idle");
        assert_eq!(snapshot.machine_position, [100.0, 0.0, 0.0]);
        // 100 mm at 10 mm/s takes at least 10 s, and not much more.
        assert!((10_000.0..11_000.0).contains(&snapshot.clock_ms));
    }

    #[test]
    fn real_time_jog_moves_with_the_clock() {
        let (mut machine, start) = machine();
        machine.feed(b"$J=G91X100F600\n");
        machine.pump(start);
        machine.pump(start + Duration::from_secs(1));
        let snapshot = machine.snapshot();
        assert_eq!(snapshot.state, "Jog");
        let x = snapshot.machine_position[0];
        assert!(x > 0.0 && x <= 10.0, "moved {x} mm in 1 s at 10 mm/s");
    }

    #[test]
    fn power_up_drops_pending_output_and_prints_the_banner() {
        let (mut machine, start) = machine();
        machine.feed(b"$I\n");
        machine.power_up(start);
        assert_eq!(String::from_utf8(machine.pump(start)).unwrap(), BANNER);
    }

    #[test]
    fn settings_are_validated_and_survive_the_power_cycle() {
        let (mut machine, start) = machine();
        machine.set_setting(110, 1234.5, start).unwrap();
        assert_eq!(String::from_utf8(machine.pump(start)).unwrap(), BANNER);
        let row = machine.settings().into_iter().find(|row| row.id == 110);
        assert_eq!(
            row,
            Some(SettingRow {
                id: 110,
                value: 1234.5,
                text: "1234.500".into()
            })
        );
        assert_eq!(
            machine.set_setting(7, 1.0, start),
            Err("$7=1 rejected with error:3".into())
        );
        assert_eq!(
            machine.set_setting(110, -1.0, start),
            Err("$110=-1 rejected with error:4".into())
        );
    }

    #[test]
    fn the_burn_store_follows_the_trace_across_profile_loads() {
        let (mut machine, start) = machine();
        machine.set_speed(Speed::Instant, start);
        machine.feed(b"G1 X10 F600 M3 S500 M8\nM5\n");
        machine.pump(start);
        let first = machine.burns(0);
        assert!(!first.segments.is_empty());
        assert!(first.segments.iter().all(|s| s.line == 1 && s.air_assist));
        assert_eq!(first.segments.last().unwrap().end, [10.0, 0.0]);
        let end = first.segments.last().unwrap().t1;

        machine.load_preset(Preset::DiodeLaser, start);
        machine.pump(start);
        machine.feed(b"G1 X5 F600\n");
        machine.pump(start);
        let both = machine.burns(0);
        let later = &both.segments[first.segments.len()..];
        // The new emulator's first line is line 3 app-wide, after a later clock.
        assert!(
            later.iter().all(|s| s.line == 3 && s.t0 >= end),
            "{later:?}"
        );

        machine.clear_burns();
        assert_eq!(machine.burns(0).total, 0);
    }

    #[test]
    fn the_snapshot_reports_the_laser_air_assist_and_bed() {
        let (mut machine, start) = machine();
        machine.feed(b"G1 X10 F600 M3 S500 M8\n");
        machine.pump(start);
        machine.pump(start + Duration::from_millis(100));
        let snapshot = machine.snapshot();
        assert_eq!(snapshot.laser_power, 0.5);
        assert!(snapshot.air_assist);
        assert_eq!(
            snapshot.bed,
            Bed {
                origin: [0.0, 0.0],
                size: [400.0, 400.0]
            }
        );
    }

    #[test]
    fn faults_and_scenarios_come_from_text_and_reach_the_inspector() {
        let (mut machine, start) = machine();
        assert_eq!(
            machine.inject("alarm 12"),
            Err("GRBL 1.1 alarms are 1 to 9, not 12".into())
        );
        machine.inject("error 20").unwrap();
        machine.feed(b"G21\n");
        assert_eq!(machine.pump(start), b"error:20\r\n");
        assert_eq!(
            machine.run_scenario("after 1: hold\nbogus"),
            Err("Scenario line 2: unknown fault \"bogus\"".into())
        );
        machine.run_scenario("rx 64\nafter 1: hold").unwrap();
        machine.pump(start);
        assert_eq!(machine.snapshot().scenario_pending, 1);
        assert_eq!(machine.inspector(0).rx_capacity, 64);
        let texts: Vec<_> = machine
            .inspector(0)
            .events
            .into_iter()
            .filter_map(|event| match event.event {
                Wire::Fault { text } => Some(text),
                Wire::Note { text } if text.starts_with("Scenario") => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            [
                "error 20",
                "Scenario started: 2 faults, lines count from here",
                "rx 64"
            ]
        );
        machine.clear_faults();
        assert_eq!(machine.snapshot().scenario_pending, 0);
    }

    #[test]
    fn instant_speed_runs_out_a_stall() {
        let (mut machine, start) = machine();
        machine.set_speed(Speed::Instant, start);
        machine.inject("stall 2s").unwrap();
        machine.feed(b"G21\n");
        assert_eq!(machine.pump(start), b"ok\r\n");
        assert_eq!(machine.snapshot().clock_ms, 2000.0);
    }

    #[test]
    fn presets_replace_the_profile() {
        let (mut machine, start) = machine();
        machine.load_preset(Preset::GrblDefaults, start);
        assert_eq!(machine.snapshot().profile, Profile::grbl_defaults().name);
        let laser_mode = machine.settings().into_iter().find(|row| row.id == 32);
        assert_eq!(laser_mode.map(|row| row.value), Some(0.0));
        assert_eq!(presets().len(), Preset::ALL.len());
    }
}
