//! Manual machine control: the jog pad's step and held jogs, and the laser
//! test fire. [`ConnectionHandle`](super::ConnectionHandle) sends them; the
//! timing that keeps them safe (renewing a held jog, the test fire's off
//! command) runs on the connection's owner thread, never in the UI.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::status::{MachineState, MachineStatus};

/// Step sizes the jog pad offers, in mm.
pub const JOG_STEPS: [f64; 4] = [0.1, 1.0, 10.0, 50.0];

/// A held jog stops if it isn't renewed for this long, so a frozen or closed
/// window can't leave the head running. The UI renews about every 100 ms.
pub const JOG_HOLD_TIMEOUT: Duration = Duration::from_millis(400);

/// A held jog is streamed as short jogs, each this long at the jog feed...
const JOG_SEGMENT: Duration = Duration::from_millis(100);
/// ...but never shorter than this, in mm.
const MIN_JOG_SEGMENT: f64 = 0.05;
/// How many segments may be queued ahead of the head. Few enough that they
/// all sit in GRBL's planner, which the jog cancel empties, and none wait in
/// its serial buffer to run after the cancel.
const JOG_LOOKAHEAD: f64 = 4.0;

/// Test-fire power cap, percent of the profile's maximum S (`$30`).
pub const TEST_FIRE_MAX_POWER: f64 = 10.0;
/// Test-fire duration cap.
pub const TEST_FIRE_MAX_MS: u32 = 1000;

/// Which way to jog: -1, 0 or 1 per axis, so diagonals are `{x: 1, y: -1}`.
/// Positive Y is towards the rear of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JogDirection {
    pub x: i8,
    pub y: i8,
}

impl JogDirection {
    /// Checks the direction is one of the eight jog pad arrows.
    pub fn validate(self) -> Result<Self, String> {
        let unit = |v: i8| (-1..=1).contains(&v);
        if unit(self.x) && unit(self.y) && (self.x, self.y) != (0, 0) {
            Ok(self)
        } else {
            Err(format!("Can't jog in direction ({}, {}).", self.x, self.y))
        }
    }

    /// Distance along the jog per unit of each axis's move: √2 diagonally.
    fn length(self) -> f64 {
        f64::from(self.x).hypot(f64::from(self.y))
    }
}

/// Checks a jog feed rate (mm/min) or distance (mm) is a positive number.
pub(crate) fn positive(value: f64, what: &str) -> Result<f64, String> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(format!("The jog {what} must be more than 0."))
    }
}

/// A short, low-power laser pulse with the head still, to find the beam or
/// check focus.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestFire {
    /// Percent of the profile's maximum S, up to [`TEST_FIRE_MAX_POWER`].
    pub power: f64,
    /// Up to [`TEST_FIRE_MAX_MS`].
    pub duration_ms: u32,
}

impl Default for TestFire {
    /// 1% for 100 ms.
    fn default() -> Self {
        TestFire {
            power: 1.0,
            duration_ms: 100,
        }
    }
}

impl TestFire {
    /// Within the caps, whatever was asked for. A power that isn't a number
    /// is 0.
    pub fn clamped(self) -> Self {
        let power = if self.power.is_finite() {
            self.power.clamp(0.0, TEST_FIRE_MAX_POWER)
        } else {
            0.0
        };
        TestFire {
            power,
            duration_ms: self.duration_ms.min(TEST_FIRE_MAX_MS),
        }
    }

    /// The S value for this power on a machine whose full power is
    /// `max_spindle`. Clamped, so never more than 10% of it.
    pub fn s_value(self, max_spindle: f64) -> f64 {
        self.clamped().power / 100.0 * max_spindle.max(0.0)
    }

    pub fn duration(self) -> Duration {
        Duration::from_millis(u64::from(self.clamped().duration_ms))
    }
}

/// A jog that runs while a button is held, streamed as short segments while
/// the UI keeps renewing it. Lives on the owner thread.
#[derive(Debug, Clone)]
pub(crate) struct HeldJog {
    direction: JogDirection,
    feed: f64,
    /// When the UI last renewed it.
    renewed: Duration,
    /// Where the head was when the first segment went out, and whether that
    /// is the machine position (else the work position). `None` until the
    /// machine reports idle after the previous jog was cancelled.
    origin: Option<((f64, f64), bool)>,
    /// Path length queued so far, mm.
    planned: f64,
    /// The machine rejected a segment: queue no more until released.
    failed: bool,
}

impl HeldJog {
    pub fn new(direction: JogDirection, feed: f64, now: Duration) -> Self {
        HeldJog {
            direction,
            feed,
            renewed: now,
            origin: None,
            planned: 0.0,
            failed: false,
        }
    }

    pub fn is(&self, direction: JogDirection, feed: f64) -> bool {
        self.direction == direction && self.feed == feed
    }

    pub fn renew(&mut self, now: Duration) {
        self.renewed = now;
    }

    pub fn expired(&self, now: Duration) -> bool {
        now.saturating_sub(self.renewed) > JOG_HOLD_TIMEOUT
    }

    pub fn fail(&mut self) -> bool {
        !std::mem::replace(&mut self.failed, true)
    }

    /// Segments to queue now, as `(dx, dy)` in mm. `settled` is true once a
    /// status report was polled after the last jog cancel, so `status` shows
    /// whether the head has stopped.
    pub fn segments(&mut self, status: Option<&MachineStatus>, settled: bool) -> Vec<(f64, f64)> {
        if self.failed {
            return Vec::new();
        }
        let Some(status) = status else {
            return Vec::new();
        };
        let travelled = match self.origin {
            None => {
                // Jogs sent while a cancelled jog still decelerates would be
                // dropped along with it.
                if !settled || status.state != MachineState::Idle {
                    return Vec::new();
                }
                let Some(start) = position(status, true).map(|p| (p, true)) else {
                    return Vec::new();
                };
                self.origin = Some(start);
                0.0
            }
            Some(((x0, y0), machine)) => match position(status, machine) {
                Some((x, y)) => (x - x0).hypot(y - y0),
                None => 0.0,
            },
        };
        let length = self.direction.length();
        let step = (self.feed / 60.0 * JOG_SEGMENT.as_secs_f64()).max(MIN_JOG_SEGMENT);
        let mut segments = Vec::new();
        while self.planned - travelled < step * length * JOG_LOOKAHEAD {
            segments.push((
                step * f64::from(self.direction.x),
                step * f64::from(self.direction.y),
            ));
            self.planned += step * length;
        }
        segments
    }

    pub fn feed(&self) -> f64 {
        self.feed
    }
}

/// The head's XY position, preferring the machine position; falls back to
/// the work position only when `machine` is false or none was reported.
fn position(status: &MachineStatus, machine: bool) -> Option<(f64, f64)> {
    let position = if machine {
        status.machine_position.or(status.work_position)
    } else {
        status.work_position
    };
    position.map(|p| (p.x, p.y))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    use bf_sim::{Event, Profile, Simulator};

    use super::*;
    use crate::machine::sim::SimLink;
    use crate::machine::{Connection, ConnectionHandle, Dialect, Grbl, MachineEvent, SendError};

    const WAIT: Duration = Duration::from_secs(5);

    // ---- command generation ----

    #[test]
    fn every_direction_including_diagonals_jogs_by_the_step() {
        let cases = [
            ((0, 1), "$J=G91 G21 Y10 F3000"),
            ((0, -1), "$J=G91 G21 Y-10 F3000"),
            ((1, 0), "$J=G91 G21 X10 F3000"),
            ((-1, 0), "$J=G91 G21 X-10 F3000"),
            ((1, 1), "$J=G91 G21 X10 Y10 F3000"),
            ((-1, 1), "$J=G91 G21 X-10 Y10 F3000"),
            ((1, -1), "$J=G91 G21 X10 Y-10 F3000"),
            ((-1, -1), "$J=G91 G21 X-10 Y-10 F3000"),
        ];
        for ((x, y), expected) in cases {
            let direction = JogDirection { x, y }.validate().unwrap();
            let line = Grbl.jog_by(
                10.0 * f64::from(direction.x),
                10.0 * f64::from(direction.y),
                3000.0,
            );
            assert_eq!(line, expected);
        }
    }

    #[test]
    fn step_size_and_feed_are_written_as_given() {
        let lines: Vec<_> = JOG_STEPS
            .iter()
            .map(|&step| Grbl.jog_by(step, 0.0, 1234.5))
            .collect();
        assert_eq!(
            lines,
            [
                "$J=G91 G21 X0.1 F1234.5",
                "$J=G91 G21 X1 F1234.5",
                "$J=G91 G21 X10 F1234.5",
                "$J=G91 G21 X50 F1234.5",
            ]
        );
    }

    #[test]
    fn refuses_directions_off_the_pad_and_non_positive_numbers() {
        for (x, y) in [(0, 0), (2, 0), (0, -2)] {
            assert!(JogDirection { x, y }.validate().is_err(), "{x},{y}");
        }
        assert!(positive(0.0, "feed").is_err());
        assert!(positive(-1.0, "feed").is_err());
        assert!(positive(f64::NAN, "step").is_err());
        assert_eq!(positive(0.1, "step"), Ok(0.1));
    }

    #[test]
    fn test_fire_values_above_the_caps_are_clamped() {
        let wild = TestFire {
            power: 80.0,
            duration_ms: 60_000,
        };
        assert_eq!(
            wild.clamped(),
            TestFire {
                power: TEST_FIRE_MAX_POWER,
                duration_ms: TEST_FIRE_MAX_MS
            }
        );
        assert_eq!(wild.s_value(1000.0), 100.0);
        assert_eq!(wild.duration(), Duration::from_secs(1));
        let negative = TestFire {
            power: -5.0,
            duration_ms: 50,
        };
        assert_eq!(negative.s_value(1000.0), 0.0);
        let nan = TestFire {
            power: f64::NAN,
            duration_ms: 50,
        };
        assert_eq!(nan.s_value(1000.0), 0.0);
        assert_eq!(TestFire::default().s_value(1000.0), 10.0);
        assert_eq!(TestFire::default().duration(), Duration::from_millis(100));
    }

    // ---- against the simulator ----
    //
    // The simulator's clock runs far ahead of the wall clock: the owner
    // thread advances it on every idle read. So these tests act from the
    // event handler, on simulated time, and check the wire and the burn
    // trace afterwards rather than watching the laser in wall time. The
    // link runs in lockstep, so the clock waits for the handler to finish
    // with each status report however slowly its thread is scheduled.

    type Slot = Arc<Mutex<Option<ConnectionHandle>>>;

    fn open(link: &SimLink) -> (Connection, Receiver<MachineEvent>) {
        open_with(link, |_, _| {})
    }

    /// Opens a connection whose handler also passes each status report to
    /// `on_status` with a handle, once the connection is up.
    fn open_with(
        link: &SimLink,
        mut on_status: impl FnMut(&MachineStatus, &ConnectionHandle) + Send + 'static,
    ) -> (Connection, Receiver<MachineEvent>) {
        let (events, inbox) = mpsc::channel();
        let slot: Slot = Arc::default();
        let handle = slot.clone();
        let lockstep = link.lockstep();
        let connection = Connection::open(link.transport(), link.clock(), move |event| {
            if let MachineEvent::Status(status) = &event {
                if let Some(handle) = handle.lock().unwrap().as_ref() {
                    on_status(status, handle);
                }
                lockstep.handled();
            }
            let _ = events.send(event);
        })
        .unwrap();
        // Wait for the first status, which manual control checks.
        loop {
            if let MachineEvent::Status(_) = inbox.recv_timeout(WAIT).unwrap() {
                break;
            }
        }
        *slot.lock().unwrap() = Some(connection.handle());
        (connection, inbox)
    }

    fn laser_link() -> SimLink {
        SimLink::new(Simulator::new(Profile::diode_laser()))
    }

    fn homing_link() -> SimLink {
        let mut profile = Profile::diode_laser();
        profile.settings.set(22, 1.0).unwrap();
        SimLink::new(Simulator::new(profile))
    }

    /// Command lines and realtime bytes (other than `?`) the controller got,
    /// with the simulated time each arrived.
    fn received(link: &SimLink) -> Vec<(Duration, String)> {
        link.sim()
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::Received { at, bytes } if bytes != b"?" => {
                    let text = match bytes.as_slice() {
                        [byte] if *byte >= 0x80 => format!("0x{byte:02X}"),
                        _ => String::from_utf8_lossy(bytes).trim_end().to_string(),
                    };
                    Some((*at, text))
                }
                _ => None,
            })
            .collect()
    }

    /// When the controller first got `command`.
    fn received_at(link: &SimLink, command: &str) -> Option<Duration> {
        received(link)
            .into_iter()
            .find(|(_, line)| line == command)
            .map(|(at, _)| at)
    }

    /// Waits, in wall time, until `until` holds.
    fn wait_until(until: impl Fn() -> bool) {
        let deadline = Instant::now() + WAIT;
        while !until() {
            assert!(Instant::now() < deadline, "timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Waits, in wall time, until `until` holds for the simulator.
    fn wait_for(link: &SimLink, until: impl Fn(&Simulator) -> bool) {
        wait_until(|| until(&link.sim()));
    }

    /// Lets `simulated` time pass on the controller.
    fn wait_simulated(link: &SimLink, simulated: Duration) {
        let until = link.sim().now() + simulated;
        wait_for(link, |sim| sim.now() >= until);
    }

    fn idle(sim: &Simulator) -> bool {
        sim.state() == bf_sim::MachineState::Idle
    }

    /// Stationary laser burns so far, as `(power, how long)`.
    fn burns(link: &SimLink) -> Vec<(f64, Duration)> {
        link.sim()
            .trace()
            .iter()
            .filter(|segment| segment.is_stationary() && segment.power > 0.0)
            .map(|segment| (segment.power, segment.t1 - segment.t0))
            .collect()
    }

    /// Waits until the controller has run a test fire's `M5`.
    fn wait_for_laser_off(link: &SimLink) {
        wait_until(|| received_at(link, "M5").is_some());
        wait_simulated(link, Duration::from_millis(20));
        assert_eq!(link.sim().laser_power(), 0.0);
    }

    #[test]
    fn a_step_jog_moves_the_head_by_the_step() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);

        connection
            .handle()
            .jog(JogDirection { x: 1, y: -1 }, 5.0, 3000.0)
            .unwrap();

        wait_for(&link, |sim| idle(sim) && sim.machine_position()[0] > 0.0);
        assert_eq!(link.sim().machine_position(), [5.0, -5.0, 0.0]);
        let sent: Vec<_> = received(&link).into_iter().map(|(_, line)| line).collect();
        assert_eq!(sent, ["$J=G91 G21 X5 Y-5 F3000"]);
    }

    #[test]
    fn a_held_jog_runs_until_released_then_cancels_with_0x85() {
        let link = laser_link();
        let right = JogDirection { x: 1, y: 0 };
        let released = Arc::new(Mutex::new(None));
        let release = released.clone();
        // The UI renews the hold as time passes, and lets go at X20.
        let (connection, _inbox) = open_with(&link, move |status, handle| {
            let mut release = release.lock().unwrap();
            if release.is_some() {
                return;
            }
            let x = status.machine_position.map_or(0.0, |p| p.x);
            if x < 20.0 {
                handle.jog_hold(right, 600.0).unwrap();
            } else {
                handle.jog_stop().unwrap();
                *release = Some(x);
            }
        });
        connection.handle().jog_hold(right, 600.0).unwrap();

        wait_until(|| released.lock().unwrap().is_some());
        wait_for(&link, idle);
        let stopped = link.sim().machine_position()[0];
        wait_simulated(&link, Duration::from_millis(500));

        assert_eq!(
            link.sim().machine_position()[0],
            stopped,
            "ran after release"
        );
        // Released within a status poll of X20; 600 mm/min decelerating at
        // 500 mm/s² takes 0.1 mm.
        let at = released.lock().unwrap().unwrap();
        assert!(stopped - at < 0.2, "released at {at}, stopped at {stopped}");
        let sent = received(&link);
        let (last, segments) = sent.split_last().unwrap();
        assert_eq!(last.1, "0x85");
        assert!(segments.len() > 10, "{sent:?}");
        assert!(
            segments
                .iter()
                .all(|(_, line)| line == "$J=G91 G21 X1 F600"),
            "{sent:?}"
        );
    }

    #[test]
    fn a_held_jog_that_isnt_renewed_cancels_itself() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);

        connection
            .handle()
            .jog_hold(JogDirection { x: 0, y: 1 }, 600.0)
            .unwrap();
        // The UI froze: no renewals, and no release.
        wait_until(|| received_at(&link, "0x85").is_some());
        wait_for(&link, idle);

        let started = received(&link)[0].0;
        let cancelled = received_at(&link, "0x85").unwrap();
        let held = cancelled - started;
        assert!(
            held >= JOG_HOLD_TIMEOUT && held <= JOG_HOLD_TIMEOUT + Duration::from_millis(20),
            "{held:?}"
        );
        assert!(link.sim().machine_position()[1] < 5.0);
    }

    #[test]
    fn test_fire_turns_the_laser_off_after_its_duration() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);

        let fire = connection.handle().test_fire(TestFire::default(), 1000.0);
        assert_eq!(fire, Ok(TestFire::default()));

        wait_for_laser_off(&link);
        let burns = burns(&link);
        assert_eq!(burns.len(), 1, "{burns:?}");
        let (power, lit) = burns[0];
        assert_eq!(power, 0.01);
        assert!(
            lit >= Duration::from_millis(100) && lit <= Duration::from_millis(120),
            "{lit:?}"
        );
        assert!(received_at(&link, "G1 F1000 M3 S10").is_some());
    }

    #[test]
    fn test_fire_values_above_the_caps_are_clamped_on_the_wire() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);

        let asked = TestFire {
            power: 50.0,
            duration_ms: 5000,
        };
        let fired = connection.handle().test_fire(asked, 1000.0).unwrap();
        assert_eq!(fired, asked.clamped());

        wait_for_laser_off(&link);
        let (power, lit) = burns(&link)[0];
        assert_eq!(power, 0.1);
        assert!(lit <= Duration::from_millis(1020), "{lit:?}");
        assert!(received_at(&link, "G1 F1000 M3 S100").is_some());
    }

    #[test]
    fn test_fire_turns_off_even_when_the_ui_side_is_dropped_mid_fire() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);
        // The caller (a Tauri command, the webview behind it) fires and goes
        // away: its thread ends and its handle is dropped.
        let handle = connection.handle();
        thread::spawn(move || {
            handle.test_fire(TestFire::default(), 1000.0).unwrap();
        })
        .join()
        .unwrap();

        wait_for_laser_off(&link);
        let (_, lit) = burns(&link)[0];
        assert!(lit <= Duration::from_millis(120), "{lit:?}");
    }

    #[test]
    fn closing_the_connection_mid_fire_turns_the_laser_off_at_once() {
        let link = laser_link();
        let fire = TestFire {
            power: 5.0,
            duration_ms: 1000,
        };
        let slot: Arc<Mutex<Option<Connection>>> = Arc::default();
        let held = slot.clone();
        // The app drops the connection as soon as a report shows the laser
        // on, well inside the second it should stay on.
        let (connection, inbox) = open_with(&link, move |status, _| {
            if status.accessories.is_some_and(|a| a.spindle_cw) {
                drop(held.lock().unwrap().take());
            }
        });
        let handle = connection.handle();
        *slot.lock().unwrap() = Some(connection);
        handle.test_fire(fire, 1000.0).unwrap();

        loop {
            match inbox.recv_timeout(WAIT).unwrap() {
                MachineEvent::Disconnected { .. } => break,
                _ => continue,
            }
        }
        // Nothing polls the simulator now; let it read what was sent.
        link.sim().advance(Duration::from_millis(10));

        assert_eq!(link.sim().laser_power(), 0.0);
        let on = received_at(&link, "G1 F1000 M3 S50").unwrap();
        let off = received_at(&link, "M5").unwrap();
        assert!(
            off > on && off - on < Duration::from_millis(500),
            "{on:?} {off:?}"
        );
    }

    #[test]
    fn other_commands_wait_out_a_test_fire() {
        let link = laser_link();
        let fire = TestFire {
            power: 1.0,
            duration_ms: 1000,
        };
        let tried = Arc::new(Mutex::new(None));
        let attempts = tried.clone();
        // Try the others from the handler while a report shows the laser on,
        // so no simulated time passes meanwhile.
        let (connection, _inbox) = open_with(&link, move |status, handle| {
            let on = status.accessories.is_some_and(|a| a.spindle_cw);
            let mut attempts = attempts.lock().unwrap();
            if on && attempts.is_none() {
                let right = JogDirection { x: 1, y: 0 };
                *attempts = Some([
                    handle.send_line("G4 P10"),
                    handle.jog(right, 1.0, 1000.0),
                    handle.jog_hold(right, 1000.0),
                    handle.test_fire(fire, 1000.0).map(|_| ()),
                ]);
            }
        });
        let handle = connection.handle();
        handle.test_fire(fire, 1000.0).unwrap();

        wait_for_laser_off(&link);
        let refused = Err(SendError::TestFiring);
        assert_eq!(
            tried.lock().unwrap().clone(),
            Some([refused.clone(), refused.clone(), refused.clone(), refused])
        );
        // Once off, the machine takes commands again.
        assert_eq!(handle.send_line("$I"), Ok(()));
        assert!(received_at(&link, "G4 P10").is_none());
    }

    #[test]
    fn test_fire_needs_an_idle_machine() {
        let link = homing_link();
        let (connection, _inbox) = open(&link);
        assert_eq!(link.sim().state(), bf_sim::MachineState::Alarm);

        let refused = connection.handle().test_fire(TestFire::default(), 1000.0);

        assert!(
            matches!(&refused, Err(SendError::NotReady(message)) if message.contains("idle")),
            "{refused:?}"
        );
        // A refused fire doesn't block the next attempt.
        assert!(matches!(
            connection.handle().test_fire(TestFire::default(), 1000.0),
            Err(SendError::NotReady(_))
        ));
        assert!(received(&link).is_empty());
    }

    #[test]
    fn work_origin_round_trips_through_set_go_and_clear() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);
        let handle = connection.handle();
        let dialect = handle.dialect();
        // GRBL refuses GCode while it jogs, so the UI waits for idle too.
        let settle = |x: f64| wait_for(&link, |sim| idle(sim) && sim.machine_position()[0] == x);

        handle
            .jog(JogDirection { x: 1, y: 1 }, 10.0, 3000.0)
            .unwrap();
        settle(10.0);
        handle.send_line(dialect.set_work_origin()).unwrap();
        wait_for(&link, |sim| sim.work_position()[0] == 0.0);
        handle
            .jog(JogDirection { x: 1, y: 0 }, 50.0, 3000.0)
            .unwrap();
        settle(60.0);
        assert_eq!(link.sim().work_position(), [50.0, 0.0, 0.0]);

        handle.go_to_origin(3000.0).unwrap();
        settle(10.0);
        assert_eq!(link.sim().machine_position(), [10.0, 10.0, 0.0]);
        assert_eq!(link.sim().work_position(), [0.0, 0.0, 0.0]);

        handle.send_line(dialect.clear_work_origin()).unwrap();
        wait_for(&link, |sim| sim.work_position()[0] == 10.0);
        assert_eq!(link.sim().work_position(), [10.0, 10.0, 0.0]);
    }

    #[test]
    fn air_assist_toggles_with_m8_and_m9() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);
        let handle = connection.handle();

        handle.send_line("M8").unwrap();
        wait_for(&link, Simulator::air_assist);
        handle.send_line(handle.dialect().air_assist_off()).unwrap();
        wait_for(&link, |sim| !sim.air_assist());
    }

    #[test]
    fn unlock_and_home_clear_an_alarm() {
        let link = homing_link();
        let (connection, _inbox) = open(&link);
        let handle = connection.handle();
        assert_eq!(link.sim().state(), bf_sim::MachineState::Alarm);

        handle.send_line(handle.dialect().unlock()).unwrap();
        wait_for(&link, idle);
        handle.send_line(handle.dialect().home()).unwrap();
        wait_for(&link, |sim| sim.machine_position()[0] != 0.0);
        assert!(idle(&link.sim()));
    }

    #[test]
    fn manual_control_is_refused_during_a_job() {
        let link = laser_link();
        let (connection, _inbox) = open(&link);
        let handle = connection.handle();
        let job = crate::machine::Job::from_lines("slow", ["G1 X50 F300"]);
        handle.start_job(job).unwrap();

        let right = JogDirection { x: 1, y: 0 };
        assert_eq!(handle.jog_hold(right, 600.0), Err(SendError::JobRunning));
        assert_eq!(handle.jog(right, 1.0, 600.0), Err(SendError::JobRunning));
        assert_eq!(
            handle.test_fire(TestFire::default(), 1000.0),
            Err(SendError::JobRunning)
        );
    }
}
