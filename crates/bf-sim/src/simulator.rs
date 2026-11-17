//! The GRBL 1.1 emulator: serial byte stream in, controller responses out.
//!
//! Modelled on GRBL's own structure. Realtime bytes are intercepted as they
//! arrive (GRBL's serial interrupt); everything else waits in the RX buffer
//! until the main loop reads it, which here happens in [`Simulator::advance`].
//! A host therefore sees acknowledgements only after advancing the clock, and
//! sending more than the RX buffer holds loses bytes, exactly like real GRBL.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::fault::Fault;
use crate::motion::{
    Context, LaserMode, Limits, LineData, Motion, Pwm, Spindle, Step, TraceSegment,
};
use crate::settings::{Profile, Settings};

mod faults;
mod gcode;

const VERSION: &str = "1.1h";
const BUILD_DATE: &str = "20190825";
/// GRBL's line buffer; longer lines fail with `error:11`.
const LINE_BUFFER_SIZE: usize = 80;
const HELP: &str = "[HLP:$$ $# $G $I $N $x=val $Nx=line $J=line $SLP $C $X $H ~ ! ? ctrl-x]";
const UNLOCK_HINT: &str = "[MSG:'$H'|'$X' to unlock]";

// GRBL 1.1 error codes used here.
const ERR_EXPECTED_LETTER: u8 = 1;
const ERR_BAD_NUMBER: u8 = 2;
const ERR_INVALID_STATEMENT: u8 = 3;
const ERR_HOMING_DISABLED: u8 = 5;
const ERR_NOT_IDLE: u8 = 8;
const ERR_LOCKED: u8 = 9;
const ERR_CHECK_DOOR: u8 = 13;
const ERR_LINE_OVERFLOW: u8 = 11;
const ERR_TRAVEL_EXCEEDED: u8 = 15;
const ERR_INVALID_JOG: u8 = 16;
const ERR_UNDEFINED_FEED: u8 = 22;
const ERR_NO_AXIS_WORDS: u8 = 26;

/// Alarm raised when a line would move outside the soft limits.
const ALARM_SOFT_LIMIT: u8 = 2;
/// Alarm raised when a reset interrupts motion.
const ALARM_ABORT_CYCLE: u8 = 3;

/// GRBL's `SAFETY_DOOR_SPINDLE_DELAY` and `SAFETY_DOOR_COOLANT_DELAY`: how
/// long a door resume waits after restarting each (the spindle only
/// outside laser mode).
const DOOR_SPINDLE_DELAY: Duration = Duration::from_secs(4);
const DOOR_COOLANT_DELAY: Duration = Duration::from_secs(1);

/// Controller state, as shown at the start of a status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineState {
    Idle,
    Run,
    /// `Hold:0` complete (ready to resume), `Hold:1` decelerating.
    Hold(u8),
    Jog,
    Alarm,
    /// `Door:0` closed and ready to resume, `Door:1` ajar, `Door:3`
    /// restoring the laser and air assist after a resume. GRBL's `Door:2`
    /// (parking, or still stopping) is not modelled: lasers don't park.
    Door(u8),
    Check,
    Home,
    Sleep,
}

impl fmt::Display for MachineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MachineState::Idle => write!(f, "Idle"),
            MachineState::Run => write!(f, "Run"),
            MachineState::Hold(n) => write!(f, "Hold:{n}"),
            MachineState::Jog => write!(f, "Jog"),
            MachineState::Alarm => write!(f, "Alarm"),
            MachineState::Door(n) => write!(f, "Door:{n}"),
            MachineState::Check => write!(f, "Check"),
            MachineState::Home => write!(f, "Home"),
            MachineState::Sleep => write!(f, "Sleep"),
        }
    }
}

/// Everything that crossed the wire or changed state, timestamped with the
/// simulated clock. The protocol inspector reads this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Received {
        at: Duration,
        bytes: Vec<u8>,
    },
    Sent {
        at: Duration,
        bytes: Vec<u8>,
    },
    /// The host sent more than the RX buffer could hold; these bytes were lost.
    RxOverflow {
        at: Duration,
        dropped: usize,
    },
    StateChanged {
        at: Duration,
        from: MachineState,
        to: MachineState,
    },
    /// A fault was injected, directly or by a scenario.
    Fault {
        at: Duration,
        fault: Fault,
    },
}

/// Feed, rapid and spindle (laser power) overrides, in percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overrides {
    pub feed: u8,
    pub rapid: u8,
    pub spindle: u8,
}

impl Default for Overrides {
    fn default() -> Self {
        Overrides {
            feed: 100,
            rapid: 100,
            spindle: 100,
        }
    }
}

/// G-code parser modal state, as reported by `$G`, plus the tool length
/// offset. A reset clears all of it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Modal {
    /// 0-3, or 80 for `G80`.
    pub motion: u8,
    /// G54-G59 as 0-5.
    pub coord_select: usize,
    /// G17, G18, G19 as 0, 1, 2.
    pub plane: u8,
    pub inches: bool,
    pub incremental: bool,
    /// `G93`.
    pub inverse_time: bool,
    pub spindle: Spindle,
    pub flood: bool,
    pub mist: bool,
    pub tool: u8,
    /// mm/min, or 1/min in `G93`.
    pub feed: f64,
    pub speed: f64,
    /// `G43.1` dynamic tool length offset on Z, mm.
    pub tool_length_offset: f64,
}

impl Default for Modal {
    fn default() -> Self {
        Modal {
            motion: 0,
            coord_select: 0,
            plane: 0,
            inches: false,
            incremental: false,
            inverse_time: false,
            spindle: Spindle::Off,
            flood: false,
            mist: false,
            tool: 0,
            feed: 0.0,
            speed: 0.0,
            tool_length_offset: 0.0,
        }
    }
}

/// Work coordinate data shown by `$#`. G54-G59, G28 and G30 persist across
/// resets (GRBL keeps them in EEPROM); G92 does not.
#[derive(Debug, Clone, Default, PartialEq)]
struct Coordinates {
    systems: [[f64; 3]; 6],
    g28: [f64; 3],
    g30: [f64; 3],
    g92: [f64; 3],
}

/// GRBL's EEPROM index of the stored G28 position; G30 follows it.
const STORED_G28: usize = 6;
const STORED_G30: usize = 7;

impl Coordinates {
    /// Stored data by GRBL's index: G54-G59, then G28 and G30.
    fn stored(&self, index: usize) -> [f64; 3] {
        match index {
            STORED_G28 => self.g28,
            STORED_G30 => self.g30,
            _ => self.systems[index],
        }
    }

    fn stored_mut(&mut self, index: usize) -> &mut [f64; 3] {
        match index {
            STORED_G28 => &mut self.g28,
            STORED_G30 => &mut self.g30,
            _ => &mut self.systems[index],
        }
    }
}

/// Laser output with the head still, open until it changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Burn {
    t0: Duration,
    position: [f64; 3],
    power: f64,
    laser_mode: LaserMode,
    air_assist: bool,
    line: u64,
}

/// The physical effects of a G-code line, in GRBL's order of execution.
/// GRBL blocks mid-line while it waits for motion to finish (`Sync`) or for
/// planner space, and acknowledges the line only at the end; queueing the
/// effects reproduces that without threads.
#[derive(Debug, Clone, PartialEq)]
enum Op {
    /// Wait until all queued motion has finished.
    Sync,
    Spindle(Spindle, f64),
    Coolant {
        flood: bool,
        mist: bool,
    },
    Dwell(f64),
    /// Store coordinate data (`G10`, `G28.1`, `G30.1`) by GRBL's index; GRBL
    /// syncs before EEPROM writes.
    Coordinates(usize, [f64; 3]),
    /// `M0`: a feed hold once motion has finished.
    Pause,
    /// Wait for cycle start to end the `M0` hold.
    AwaitResume,
    /// Queue a straight move (GRBL's `mc_line`).
    Line([f64; 3], LineData),
    ProgramEnd,
    Ok,
}

/// A door resume restarting powered outputs (`Door:3`), as GRBL's suspend
/// loop does: the spindle, then coolant, each followed by its delay.
#[derive(Debug, Clone, Copy)]
struct DoorRestore {
    /// When air assist comes back on, if it has yet to.
    coolant_at: Option<Duration>,
    /// When the cycle resumes.
    resume_at: Duration,
}

/// Why the machine is decelerating to a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    FeedHold,
    Door,
    JogCancel,
    SoftLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comment {
    None,
    Parenthesis,
    Semicolon,
}

/// A deterministic GRBL 1.1 controller. Feed it host bytes, advance its clock,
/// and read what it sends back. No threads and no wall-clock time.
pub struct Simulator {
    profile: Profile,
    now: Duration,
    rx: VecDeque<u8>,
    realtime: Vec<u8>,
    line: String,
    line_overflow: bool,
    comment: Comment,
    output: Vec<u8>,
    events: Vec<Event>,
    state: MachineState,
    motion: Motion,
    ops: VecDeque<Op>,
    dwell_until: Option<Duration>,
    /// A dwell's time left while the door suspends it: GRBL's dwell loop
    /// blocks in the suspend loop, so the dwell resumes where it stopped.
    dwell_paused: Option<Duration>,
    door_restore: Option<DoorRestore>,
    stop_reason: Option<StopReason>,
    /// After a soft limit GRBL ignores everything but reset.
    critical: bool,
    /// The parser's position: where the last line ends, in machine mm.
    gc_position: [f64; 3],
    coordinates: Coordinates,
    modal: Modal,
    overrides: Overrides,
    /// Physical coolant outputs; M8 drives air assist.
    flood: bool,
    mist: bool,
    /// Physical spindle (laser) direction and output.
    spindle_dir: Spindle,
    laser: Pwm,
    /// GRBL's spindle stop override is in force: a hold turned the laser
    /// off (`SPINDLE_STOP_OVR_ENABLED`) until resumed or toggled back.
    spindle_stopped: bool,
    /// A spindle stop was asked for (`0x9E`, or a laser-mode hold) and acts
    /// once the hold completes (`SPINDLE_STOP_OVR_INITIATE`).
    spindle_stop_requested: bool,
    burn: Option<Burn>,
    door_open: bool,
    wco_counter: u32,
    ovr_counter: u32,
    /// Line ends accepted into the RX buffer, and read from it, since
    /// creation. See [`TraceSegment::line`].
    lines_accepted: u64,
    lines_read: u64,
    faults: faults::Faults,
}

impl Simulator {
    /// Powers up a controller: prints the banner, and locks in alarm if homing
    /// is enabled, like GRBL with `HOMING_INIT_LOCK`.
    pub fn new(profile: Profile) -> Self {
        let mut sim = Simulator {
            profile,
            now: Duration::ZERO,
            rx: VecDeque::new(),
            realtime: Vec::new(),
            line: String::new(),
            line_overflow: false,
            comment: Comment::None,
            output: Vec::new(),
            events: Vec::new(),
            state: MachineState::Idle,
            motion: Motion::default(),
            ops: VecDeque::new(),
            dwell_until: None,
            dwell_paused: None,
            door_restore: None,
            stop_reason: None,
            critical: false,
            gc_position: [0.0; 3],
            coordinates: Coordinates::default(),
            modal: Modal::default(),
            overrides: Overrides::default(),
            flood: false,
            mist: false,
            spindle_dir: Spindle::Off,
            laser: Pwm::OFF,
            spindle_stopped: false,
            spindle_stop_requested: false,
            burn: None,
            door_open: false,
            wco_counter: 0,
            ovr_counter: 0,
            lines_accepted: 0,
            lines_read: 0,
            faults: faults::Faults::default(),
        };
        sim.power_up();
        sim
    }

    /// Power-cycles the controller, as opening a USB serial port does on an
    /// Arduino-based board. Settings and stored coordinate systems survive;
    /// position, buffers and G92 offsets do not.
    /// A cable pulled by [`Fault::Disconnect`] is plugged back in; other
    /// faults stay in force.
    pub fn power_up(&mut self) {
        self.faults.disconnected = false;
        self.clear_volatile_state();
        self.motion.reset_to([0.0; 3]);
        self.gc_position = [0.0; 3];
        let state = if self.profile.settings.homing_enabled() {
            MachineState::Alarm
        } else {
            MachineState::Idle
        };
        self.set_state(state);
        self.print_banner();
    }

    /// Receives bytes from the host. Realtime commands are taken immediately;
    /// other bytes wait in the RX buffer until [`Simulator::advance`].
    /// Plugs a cable pulled by [`Fault::Disconnect`] back in without a
    /// reset, as on a board that doesn't reset when its port opens: the
    /// controller carries on from where it got to.
    pub fn replug(&mut self) {
        self.faults.disconnected = false;
    }

    /// Nothing arrives while the cable is pulled ([`Fault::Disconnect`]).
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || self.faults.disconnected {
            return;
        }
        self.events.push(Event::Received {
            at: self.now,
            bytes: bytes.to_vec(),
        });
        let capacity = self.rx_capacity();
        let mut dropped = 0;
        for &byte in bytes {
            if is_realtime(byte) {
                // After a soft limit only reset gets through.
                if !self.critical || byte == 0x18 {
                    self.realtime.push(byte);
                }
            } else if byte >= 0x80 {
                // GRBL silently ignores unrecognised extended-ASCII bytes.
            } else if self.rx.len() < capacity {
                if byte == b'\n' || byte == b'\r' {
                    self.lines_accepted += 1;
                }
                self.rx.push_back(byte);
            } else {
                dropped += 1;
            }
        }
        if dropped > 0 {
            self.events.push(Event::RxOverflow {
                at: self.now,
                dropped,
            });
        }
    }

    /// Runs the controller for `dt` of simulated time: handles pending realtime
    /// commands, reads and executes lines, and moves the machine.
    /// `advance(Duration::ZERO)` processes input without moving time.
    pub fn advance(&mut self, dt: Duration) {
        let end = self.now + dt;
        loop {
            self.service();
            self.update_burn();
            // Stop the clock where a fault has to act, e.g. a stall ending.
            let deadline = self.fault_deadline().filter(|&at| at <= end);
            let until = deadline.unwrap_or(end);
            if self.motion_active() {
                let ctx = self.motion_context();
                match self.motion.step(self.now, until, &ctx) {
                    Step::Executed(at) => {
                        self.now = at;
                        if let Some((pwm, _)) = self.motion.last_laser() {
                            self.laser = pwm;
                        }
                        continue;
                    }
                    Step::Stopped => {
                        self.motion_stopped();
                        if !self.motion_active() {
                            continue;
                        }
                    }
                    Step::Pending => {}
                }
            } else if let Some(at) = self.dwell_until.filter(|&at| at <= until) {
                self.now = at;
                self.dwell_until = None;
                continue;
            } else if let Some(at) = self.door_restore_deadline().filter(|&at| at <= until) {
                self.now = at;
                continue;
            }
            if deadline.is_some() {
                self.now = until;
                continue;
            }
            self.update_burn();
            self.now = end;
            break;
        }
    }

    /// Bytes the controller has sent since the last call.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn now(&self) -> Duration {
        self.now
    }

    pub fn state(&self) -> MachineState {
        self.state
    }

    pub fn machine_position(&self) -> [f64; 3] {
        self.motion.position_at(self.now)
    }

    pub fn work_position(&self) -> [f64; 3] {
        let position = self.machine_position();
        let offset = self.work_offset();
        std::array::from_fn(|axis| position[axis] - offset[axis])
    }

    /// Motion and stationary burns so far, oldest first, for the burn view
    /// and job checks. A segment appears once it has finished or been cut
    /// short by a hold, an override change or a reset; a stationary burn
    /// once the laser turns off or changes, or the head moves.
    pub fn trace(&self) -> &[TraceSegment] {
        self.motion.trace()
    }

    pub fn take_trace(&mut self) -> Vec<TraceSegment> {
        self.motion.take_trace()
    }

    /// Laser output now, as a fraction of full PWM; 0 when off.
    pub fn laser_power(&self) -> f64 {
        if self.laser.on { self.laser.power } else { 0.0 }
    }

    /// Whether air assist (`M8`, the flood coolant output) is on.
    pub fn air_assist(&self) -> bool {
        self.flood
    }

    pub fn overrides(&self) -> Overrides {
        self.overrides
    }

    pub fn settings(&self) -> &Settings {
        &self.profile.settings
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Bytes waiting in the RX buffer; `Bf:` reports the space left.
    pub fn rx_buffered(&self) -> usize {
        self.rx.len()
    }

    /// Blocks queued in the planner buffer; `Bf:` reports the space left.
    pub fn planner_queued(&self) -> usize {
        self.motion.len()
    }

    /// Opens or closes the safety door switch. Opening it enters `Door:1`;
    /// closing it moves to `Door:0`, where `~` resumes.
    pub fn set_door_open(&mut self, open: bool) {
        self.door_open = open;
        if open && self.door_acts() {
            self.enter_door();
        } else if !open && self.state == MachineState::Door(1) {
            self.set_state(MachineState::Door(0));
        }
    }

    /// Nothing left to do until the host sends more: every received byte is
    /// processed and all motion, dwells and line effects have finished. Any
    /// acknowledgement the host still waits for will never come.
    pub(crate) fn is_quiescent(&self) -> bool {
        self.rx.is_empty()
            && self.realtime.is_empty()
            && self.ops.is_empty()
            && self.motion.is_empty()
            && self.dwell_until.is_none()
            && self.dwell_paused.is_none()
            && self.door_restore.is_none()
            && !self.motion_active()
    }

    /// The spindle (laser) enable output: `M3`/`M4` and not yet `M5`.
    pub(crate) fn spindle_enabled(&self) -> bool {
        self.spindle_dir != Spindle::Off
    }

    /// The laser PWM output is on, even at minimum power.
    pub(crate) fn laser_lit(&self) -> bool {
        self.laser.on
    }

    // ---- main loop ----

    fn service(&mut self) {
        for byte in std::mem::take(&mut self.realtime) {
            self.realtime_command(byte);
        }
        self.run_door_restore();
        self.run_ops();
        self.fire_scenario();
        self.read_lines();
    }

    /// GRBL stops reading the RX buffer while the planner is full or a line
    /// is still executing, which is what makes the host's character-counting
    /// protocol necessary.
    fn can_read(&mut self) -> bool {
        self.motion.len() < self.profile.planner_blocks
            && self.ops.is_empty()
            && self.dwell_until.is_none()
            && !self.critical
            && self.stop_reason != Some(StopReason::SoftLimit)
            && self.state != MachineState::Sleep
            && !self.suspended_by_door()
            && !self.stalled()
    }

    fn read_lines(&mut self) {
        while self.can_read() {
            let Some(byte) = self.rx.pop_front() else {
                break;
            };
            if byte == b'\n' || byte == b'\r' {
                if self.line_delayed() {
                    self.rx.push_front(byte);
                    break;
                }
                self.lines_read += 1;
                let line = std::mem::take(&mut self.line);
                let overflow = std::mem::take(&mut self.line_overflow);
                self.comment = Comment::None;
                if let Some(code) = self.take_line_error(self.lines_read) {
                    self.reply_error(code);
                } else if overflow {
                    self.reply_error(ERR_LINE_OVERFLOW);
                } else {
                    self.execute_line(&line);
                }
                self.fire_scenario();
            } else {
                self.accept_char(byte);
            }
        }
    }

    /// Strips whitespace, control characters and comments, and upper-cases.
    fn accept_char(&mut self, byte: u8) {
        match self.comment {
            Comment::Parenthesis => {
                if byte == b')' {
                    self.comment = Comment::None;
                }
                return;
            }
            Comment::Semicolon => return,
            Comment::None => {}
        }
        match byte {
            b'(' => self.comment = Comment::Parenthesis,
            b';' => self.comment = Comment::Semicolon,
            // Whitespace, control characters and block delete are ignored.
            0..=b' ' | b'/' => {}
            _ if self.line.len() >= LINE_BUFFER_SIZE - 1 => self.line_overflow = true,
            _ => self.line.push(byte.to_ascii_uppercase() as char),
        }
    }

    fn execute_line(&mut self, line: &str) {
        let result = if line.is_empty() {
            Ok(Reply::Ok)
        } else if let Some(command) = line.strip_prefix('$') {
            self.system_command(command)
        } else {
            self.gcode_line(line)
        };
        match result {
            Ok(Reply::Ok) => self.send_line("ok"),
            Ok(Reply::Sent) => {}
            Err(code) => self.reply_error(code),
        }
    }

    // ---- line execution ----

    /// Runs queued line effects until one has to wait.
    fn run_ops(&mut self) {
        while let Some(op) = self.ops.front().cloned() {
            if self.dwell_until.is_some() || self.suspended_by_door() {
                return;
            }
            match op {
                Op::Sync => {
                    if !self.motion.is_empty() || self.state == MachineState::Run {
                        self.auto_cycle_start();
                        return;
                    }
                }
                Op::Spindle(dir, rpm) => self.set_spindle(dir, rpm),
                Op::Coolant { flood, mist } => {
                    self.flood = flood;
                    self.mist = mist;
                    self.ovr_counter = 0;
                }
                Op::Dwell(seconds) => {
                    self.dwell_until = Some(self.now + Duration::from_secs_f64(seconds));
                }
                Op::Coordinates(index, data) => {
                    *self.coordinates.stored_mut(index) = data;
                    if index == self.modal.coord_select {
                        self.wco_counter = 0;
                    }
                }
                Op::Pause => self.feed_hold(),
                Op::AwaitResume => {
                    if matches!(self.state, MachineState::Hold(_)) {
                        return;
                    }
                }
                Op::Line(target, data) => {
                    if self.profile.settings.soft_limits() && !self.within_travel(target) {
                        self.soft_limit_violation();
                        return;
                    }
                    if self.state != MachineState::Check {
                        if self.motion.len() >= self.profile.planner_blocks {
                            self.auto_cycle_start();
                            return;
                        }
                        let limits = self.limits();
                        if self.motion.plan(target, data, &limits) {
                            self.auto_cycle_start();
                        } else if self.profile.settings.laser_mode() && data.spindle == Spindle::Cw
                        {
                            // A move of no whole steps still updates an M3 laser,
                            // after a sync, as GRBL's mc_line does.
                            self.ops.pop_front();
                            self.ops.push_front(Op::Spindle(Spindle::Cw, data.rpm));
                            self.ops.push_front(Op::Sync);
                            continue;
                        }
                    }
                }
                Op::ProgramEnd => {
                    self.overrides = Overrides::default();
                    if self.state != MachineState::Check {
                        self.wco_counter = 0;
                        self.set_spindle(Spindle::Off, 0.0);
                        self.flood = false;
                        self.mist = false;
                    }
                    self.send_line("[MSG:Pgm End]");
                }
                Op::Ok => self.send_line("ok"),
            }
            self.ops.pop_front();
        }
    }

    /// GRBL starts a cycle as soon as motion is queued and the machine is idle.
    fn auto_cycle_start(&mut self) {
        if self.state == MachineState::Idle && !self.motion.is_empty() {
            self.set_state(MachineState::Run);
        }
    }

    /// GRBL's `spindle_set_state`. In laser mode an idle `M4` laser stays off
    /// until motion sets its power.
    fn set_spindle(&mut self, dir: Spindle, rpm: f64) {
        self.spindle_dir = dir;
        self.laser = if dir == Spindle::Off {
            Pwm::OFF
        } else {
            let rpm = if self.profile.settings.laser_mode() && dir == Spindle::Ccw {
                0.0
            } else {
                rpm
            };
            Pwm::compute(rpm, &self.motion_context())
        };
        self.ovr_counter = 0;
    }

    /// A line would leave the machine's travel. GRBL brings a running cycle
    /// to a controlled stop first so position is kept, then raises `ALARM:2`
    /// and ignores everything until reset.
    fn soft_limit_violation(&mut self) {
        self.ops.clear();
        if self.state == MachineState::Run {
            self.begin_stop(StopReason::SoftLimit);
        } else {
            self.soft_limit_alarm();
        }
    }

    fn soft_limit_alarm(&mut self) {
        self.motion.clear(self.now);
        self.gc_position = self.machine_position();
        self.critical_alarm(ALARM_SOFT_LIMIT);
    }

    // ---- motion ----

    fn motion_active(&self) -> bool {
        matches!(self.state, MachineState::Run | MachineState::Jog) || self.motion.is_stopping()
    }

    fn motion_context(&self) -> Context {
        let settings = &self.profile.settings;
        Context {
            feed_override: f64::from(self.overrides.feed),
            rapid_override: f64::from(self.overrides.rapid),
            spindle_override: f64::from(self.overrides.spindle),
            laser_mode: settings.laser_mode(),
            rpm_min: settings.rpm_min(),
            rpm_max: settings.rpm_max(),
            air_assist: self.flood,
        }
    }

    fn limits(&self) -> Limits {
        let settings = &self.profile.settings;
        Limits {
            steps_per_mm: settings.steps_per_mm(),
            max_rate: settings.max_rate(),
            acceleration: settings.acceleration(),
            junction_deviation: settings.junction_deviation(),
        }
    }

    /// Motion has nothing left to run: the buffer drained or a stop finished.
    fn motion_stopped(&mut self) {
        // GRBL turns a speed-scaled (M4) laser off when motion ends; an M3
        // laser keeps its power.
        if let Some((_, LaserMode::Dynamic)) = self.motion.last_laser() {
            self.laser.on = false;
        }
        if self.stop_reason.is_some() {
            self.finish_stop();
        } else if matches!(self.state, MachineState::Run | MachineState::Jog) {
            self.set_state(MachineState::Idle);
        }
    }

    /// Starts a controlled deceleration to a stop.
    fn begin_stop(&mut self, reason: StopReason) {
        self.motion.stop(self.now);
        self.stop_reason = Some(reason);
        if matches!(reason, StopReason::FeedHold | StopReason::SoftLimit) {
            self.set_state(MachineState::Hold(1));
        }
        if self.motion.speed_at(self.now) <= 0.0 {
            self.finish_stop();
        }
    }

    fn finish_stop(&mut self) {
        self.motion.resume();
        match self.stop_reason.take() {
            Some(StopReason::FeedHold) => {
                if matches!(self.state, MachineState::Hold(_)) {
                    self.hold_complete();
                }
            }
            Some(StopReason::JogCancel) => {
                self.motion.clear(self.now);
                self.gc_position = self.machine_position();
                if self.state == MachineState::Jog {
                    self.set_state(MachineState::Idle);
                }
            }
            Some(StopReason::SoftLimit) => self.soft_limit_alarm(),
            Some(StopReason::Door) | None => {}
        }
        if matches!(self.state, MachineState::Door(_)) {
            self.door_stopped();
        }
    }

    /// The machine has stopped in a feed hold. With `DISABLE_LASER_DURING_HOLD`
    /// (on in stock GRBL) a laser-mode hold asks for the spindle stop
    /// override, as `0x9E` does, so the laser stays off until resumed.
    fn hold_complete(&mut self) {
        self.set_state(MachineState::Hold(0));
        if self.profile.settings.laser_mode() {
            self.spindle_stop_requested = true;
        }
        self.stop_spindle_for_hold();
    }

    /// GRBL's suspend loop acting on `SPINDLE_STOP_OVR_INITIATE` once the
    /// hold is complete: the laser goes off if the program has it on.
    fn stop_spindle_for_hold(&mut self) {
        if !std::mem::take(&mut self.spindle_stop_requested) {
            return;
        }
        if self.modal.spindle != Spindle::Off {
            // The direction output stays as it was; only the PWM stops.
            self.laser = Pwm::OFF;
            self.spindle_stopped = true;
            // `spindle_set_state` reports the change in the next report.
            self.ovr_counter = 0;
        }
    }

    /// The spindle stop override (`0x9E`), which GRBL allows only in a
    /// hold: it stops the laser once the hold completes, or, sent again
    /// while stopped, restores it. The program stays `M3`/`M4` either way,
    /// and resuming restores the laser.
    fn spindle_stop_override(&mut self) {
        if !matches!(self.state, MachineState::Hold(_)) {
            return;
        }
        if self.spindle_stopped {
            self.spindle_stopped = false;
            if self.modal.spindle != Spindle::Off {
                self.send_line("[MSG:Restoring spindle]");
                self.set_spindle(self.modal.spindle, self.modal.speed);
            }
        } else {
            self.spindle_stop_requested = true;
            if self.state == MachineState::Hold(0) {
                self.stop_spindle_for_hold();
            }
        }
    }

    /// GRBL acts on the door everywhere but alarm, sleep and check mode.
    fn door_acts(&self) -> bool {
        !matches!(
            self.state,
            MachineState::Alarm | MachineState::Sleep | MachineState::Check
        )
    }

    /// The safety door holds running motion and cancels jogs. Once the
    /// machine has stopped, the laser and air assist go off. Opening it
    /// again while a resume restores them starts over.
    fn enter_door(&mut self) {
        self.send_line("[MSG:Check Door]");
        // GRBL's door routine clears any spindle stop override.
        self.spindle_stopped = false;
        self.spindle_stop_requested = false;
        self.door_restore = None;
        // A dwell blocks in GRBL's suspend loop until the resume.
        if let Some(at) = self.dwell_until.take() {
            self.dwell_paused = Some(at.saturating_sub(self.now));
        }
        let was = self.state;
        self.set_state(MachineState::Door(u8::from(self.door_open)));
        if self.stop_reason.is_none() {
            match was {
                MachineState::Run => self.begin_stop(StopReason::Door),
                MachineState::Jog => self.begin_stop(StopReason::JogCancel),
                _ => {}
            }
        }
        if self.stop_reason.is_none() {
            self.door_stopped();
        }
    }

    /// The door's hold is complete: GRBL's suspend loop de-energizes the
    /// spindle and coolant (`spindle_set_state(SPINDLE_DISABLE)`,
    /// `coolant_set_state(COOLANT_DISABLE)`). The program keeps its
    /// `M3`/`M4` and `M7`/`M8`, which the resume restores.
    fn door_stopped(&mut self) {
        // The direction output stays as it was; only the PWM stops.
        self.laser = Pwm::OFF;
        self.flood = false;
        self.mist = false;
        self.ovr_counter = 0;
    }

    /// Lines, their effects and dwells wait while the door holds the
    /// machine, as GRBL's main loop blocks in its suspend loop.
    fn suspended_by_door(&self) -> bool {
        matches!(self.state, MachineState::Door(_))
    }

    /// `~` with the door closed: GRBL restarts what the door stopped
    /// (`Door:3`), then resumes. An `M3`/`M4` laser in laser mode comes
    /// back with the next motion, so needs no delay; otherwise the spindle
    /// restarts and waits 4 s. Air assist (`M7`/`M8`) restarts and waits
    /// 1 s, even in laser mode. Unlike the spindle stop override, GRBL
    /// sends no `[MSG:Restoring spindle]` here.
    fn begin_door_restore(&mut self) {
        self.set_state(MachineState::Door(3));
        let mut at = self.now;
        if self.modal.spindle != Spindle::Off && !self.profile.settings.laser_mode() {
            self.set_spindle(self.modal.spindle, self.modal.speed);
            at += DOOR_SPINDLE_DELAY;
        }
        let coolant_at = (self.modal.flood || self.modal.mist).then_some(at);
        if coolant_at.is_some() {
            at += DOOR_COOLANT_DELAY;
        }
        self.door_restore = Some(DoorRestore {
            coolant_at,
            resume_at: at,
        });
        self.run_door_restore();
    }

    /// When a door restore next has something to do.
    fn door_restore_deadline(&self) -> Option<Duration> {
        let restore = self.door_restore?;
        Some(restore.coolant_at.unwrap_or(restore.resume_at))
    }

    /// Carries out the steps of a door restore that are due.
    fn run_door_restore(&mut self) {
        let Some(mut restore) = self.door_restore.take() else {
            return;
        };
        if self.state != MachineState::Door(3) {
            return;
        }
        if restore.coolant_at.is_some_and(|at| at <= self.now) {
            self.flood = self.modal.flood;
            self.mist = self.modal.mist;
            self.ovr_counter = 0;
            restore.coolant_at = None;
        }
        if restore.coolant_at.is_none() && restore.resume_at <= self.now {
            self.resume_cycle();
        } else {
            self.door_restore = Some(restore);
        }
    }

    /// Leaves a completed hold or door: back to running the queue, or idle,
    /// and a dwell the door suspended carries on.
    fn resume_cycle(&mut self) {
        if let Some(left) = self.dwell_paused.take() {
            self.dwell_until = Some(self.now + left);
        }
        let next = if self.motion.is_empty() {
            MachineState::Idle
        } else {
            MachineState::Run
        };
        self.set_state(next);
    }

    // ---- $ system commands ----

    /// Mirrors GRBL's `system_execute_line`, including which states allow
    /// each command and which error code each malformed form returns.
    fn system_command(&mut self, command: &str) -> Result<Reply, u8> {
        let first = command.chars().next();
        match first {
            None => {
                self.send_line(HELP);
                Ok(Reply::Ok)
            }
            Some('J') => {
                if !matches!(self.state, MachineState::Idle | MachineState::Jog) {
                    return Err(ERR_NOT_IDLE);
                }
                let words = command.strip_prefix("J=").ok_or(ERR_INVALID_STATEMENT)?;
                self.jog(words)
            }
            Some('$' | 'G' | 'C' | 'X') if command.len() > 1 => Err(ERR_INVALID_STATEMENT),
            Some('$') => {
                // Printing takes too long to allow mid-cycle.
                if matches!(self.state, MachineState::Run | MachineState::Hold(_)) {
                    return Err(ERR_NOT_IDLE);
                }
                for line in self.profile.settings.report_lines() {
                    self.send_line(&line);
                }
                Ok(Reply::Ok)
            }
            Some('G') => {
                let report = self.parser_state();
                self.send_line(&report);
                Ok(Reply::Ok)
            }
            Some('C') => self.toggle_check_mode(),
            Some('X') => {
                if self.state == MachineState::Alarm {
                    if self.door_open {
                        return Err(ERR_CHECK_DOOR);
                    }
                    self.send_line("[MSG:Caution: Unlocked]");
                    self.set_state(MachineState::Idle);
                }
                Ok(Reply::Ok)
            }
            Some(_) => {
                if !matches!(self.state, MachineState::Idle | MachineState::Alarm) {
                    return Err(ERR_NOT_IDLE);
                }
                self.idle_command(command)
            }
        }
    }

    /// Commands GRBL accepts only when idle or in alarm.
    fn idle_command(&mut self, command: &str) -> Result<Reply, u8> {
        match command {
            "RST=$" | "RST=#" | "RST=*" => {
                let defaults = Profile::grbl_defaults().settings;
                if command != "RST=#" {
                    self.profile.settings = defaults;
                }
                if command != "RST=$" {
                    self.coordinates = Coordinates::default();
                }
                self.send_line("[MSG:Restoring defaults]");
                self.send_line("ok");
                self.soft_reset();
                return Ok(Reply::Sent);
            }
            _ if command.starts_with("RST") || command.starts_with('S') => {
                if command != "SLP" {
                    return Err(ERR_INVALID_STATEMENT);
                }
                self.send_line("ok");
                self.send_line("[MSG:Sleeping]");
                self.set_state(MachineState::Sleep);
                return Ok(Reply::Sent);
            }
            "#" => {
                for line in self.parameter_lines() {
                    self.send_line(&line);
                }
            }
            "I" => {
                self.send_line(&format!("[VER:{VERSION}.{BUILD_DATE}:]"));
                // Variable spindle, M7 mist, no homing lock.
                self.send_line(&format!(
                    "[OPT:VML,{},{}]",
                    self.profile.planner_blocks, self.profile.rx_buffer_size
                ));
            }
            "N" => {
                self.send_line("$N0=");
                self.send_line("$N1=");
            }
            "H" => self.home()?,
            // Single-axis homing ($HX) and build-info writes ($I=) are compile-time
            // options that stock GRBL leaves off.
            _ if command.starts_with(['H', '#', 'I']) => return Err(ERR_INVALID_STATEMENT),
            // Storing startup lines ($Nx=line) is not modelled.
            _ if command.starts_with('N') => return Err(ERR_INVALID_STATEMENT),
            _ => self.write_setting(command)?,
        }
        Ok(Reply::Ok)
    }

    /// `$n=value`. GRBL reads the number first, so `$Q` is a bad number
    /// (`error:2`), while an unknown or out-of-range number is `error:3`.
    fn write_setting(&mut self, command: &str) -> Result<(), u8> {
        let split = command
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(command.len());
        let (id, rest) = command.split_at(split);
        let id = parse_number(id).ok_or(ERR_BAD_NUMBER)?;
        let value = rest.strip_prefix('=').ok_or(ERR_INVALID_STATEMENT)?;
        let value = parse_number(value).ok_or(ERR_BAD_NUMBER)?;
        if id > 255.0 {
            return Err(ERR_INVALID_STATEMENT);
        }
        self.profile
            .settings
            .set(id as u16, value)
            .map_err(|error| error.code())
    }

    fn toggle_check_mode(&mut self) -> Result<Reply, u8> {
        if self.state == MachineState::Check {
            // Leaving check mode resets the controller.
            self.send_line("[MSG:Disabled]");
            self.send_line("ok");
            self.soft_reset();
            return Ok(Reply::Sent);
        }
        if self.state != MachineState::Idle {
            return Err(ERR_NOT_IDLE);
        }
        self.set_state(MachineState::Check);
        self.send_line("[MSG:Enabled]");
        Ok(Reply::Ok)
    }

    /// Homing completes instantly. GRBL's machine origin convention: an axis
    /// homing towards positive sits at `-pulloff`; one homing towards negative
    /// (`$23` bit set) sits at `-max_travel + pulloff`.
    fn home(&mut self) -> Result<(), u8> {
        let settings = &self.profile.settings;
        if !settings.homing_enabled() {
            return Err(ERR_HOMING_DISABLED);
        }
        if self.door_open {
            return Err(ERR_CHECK_DOOR);
        }
        let pulloff = settings.homing_pulloff();
        let mask = settings.homing_dir_mask();
        let homed: [f64; 3] = std::array::from_fn(|axis| {
            if mask & (1 << axis) != 0 {
                -settings.max_travel(axis) + pulloff
            } else {
                -pulloff
            }
        });
        self.set_state(MachineState::Home);
        self.motion.reset_to(homed);
        self.gc_position = homed;
        self.set_state(MachineState::Idle);
        Ok(())
    }

    /// `$J=` jogging: G20/G21, G90/G91 and G53 apply to this line only and
    /// default to the parser's current modes.
    fn jog(&mut self, words: &str) -> Result<Reply, u8> {
        if !matches!(self.state, MachineState::Idle | MachineState::Jog) {
            return Err(ERR_NOT_IDLE);
        }
        let mut inches = self.modal.inches;
        let mut incremental = self.modal.incremental;
        let mut machine_coordinates = false;
        let mut axes: [Option<f64>; 3] = [None; 3];
        let mut feed = None;
        for (letter, value) in parse_words(words)? {
            match (letter, value) {
                ('G', 20.0) => inches = true,
                ('G', 21.0) => inches = false,
                ('G', 90.0) => incremental = false,
                ('G', 91.0) => incremental = true,
                ('G', 53.0) => machine_coordinates = true,
                ('X', v) => axes[0] = Some(v),
                ('Y', v) => axes[1] = Some(v),
                ('Z', v) => axes[2] = Some(v),
                ('F', v) => feed = Some(v),
                _ => return Err(ERR_INVALID_JOG),
            }
        }
        if axes.iter().all(Option::is_none) {
            return Err(ERR_NO_AXIS_WORDS);
        }
        let scale = if inches { 25.4 } else { 1.0 };
        let feed = feed.ok_or(ERR_UNDEFINED_FEED)? * scale;
        let start = self.gc_position;
        let offset = self.work_offset();
        let target: [f64; 3] = std::array::from_fn(|axis| match axes[axis] {
            None => start[axis],
            Some(v) if machine_coordinates => v * scale,
            Some(v) if incremental => start[axis] + v * scale,
            Some(v) => v * scale + offset[axis],
        });
        if self.profile.settings.soft_limits() && !self.within_travel(target) {
            return Err(ERR_TRAVEL_EXCEEDED);
        }
        // Jogs carry the current spindle state and ignore the feed override.
        let data = LineData {
            feed,
            rapid: false,
            no_feed_override: true,
            inverse_time: false,
            spindle: self.modal.spindle,
            rpm: self.modal.speed,
            line: self.lines_read,
        };
        let limits = self.limits();
        if self.motion.plan(target, data, &limits) && self.state == MachineState::Idle {
            self.set_state(MachineState::Jog);
        }
        self.gc_position = target;
        Ok(Reply::Ok)
    }

    /// GRBL's machine space runs from `-max_travel` to 0 on each axis.
    fn within_travel(&self, target: [f64; 3]) -> bool {
        target
            .iter()
            .enumerate()
            .all(|(axis, &v)| v <= 0.0 && v >= -self.profile.settings.max_travel(axis))
    }

    // ---- realtime commands ----

    fn realtime_command(&mut self, byte: u8) {
        match byte {
            b'?' => self.status_report(),
            b'!' => self.feed_hold(),
            b'~' => self.cycle_start(),
            0x18 => self.soft_reset(),
            0x84 => self.safety_door(),
            0x85 => self.jog_cancel(),
            0x90..=0x9D => self.adjust_override(byte),
            0x9E => self.spindle_stop_override(),
            0xA0 | 0xA1 => self.toggle_coolant(byte),
            _ => {}
        }
    }

    /// Running motion decelerates through `Hold:1` to `Hold:0`.
    fn feed_hold(&mut self) {
        match self.state {
            MachineState::Idle => self.hold_complete(),
            MachineState::Run => self.begin_stop(StopReason::FeedHold),
            MachineState::Jog => self.jog_cancel(),
            _ => {}
        }
    }

    fn cycle_start(&mut self) {
        if self.stop_reason.is_some() {
            return;
        }
        match self.state {
            MachineState::Hold(0) => {
                // A laser stopped for the hold comes back: in laser mode with
                // the next motion, otherwise at once.
                self.spindle_stop_requested = false;
                if std::mem::take(&mut self.spindle_stopped) && self.modal.spindle != Spindle::Off {
                    self.send_line("[MSG:Restoring spindle]");
                    if !self.profile.settings.laser_mode() {
                        self.set_spindle(self.modal.spindle, self.modal.speed);
                    }
                }
                self.resume_cycle();
            }
            MachineState::Door(0) => self.begin_door_restore(),
            _ => {}
        }
    }

    /// A jog decelerates to a stop, then the rest of the queue is discarded.
    fn jog_cancel(&mut self) {
        if self.state == MachineState::Jog && self.stop_reason.is_none() {
            self.begin_stop(StopReason::JogCancel);
        }
    }

    /// `0x84`. In the door state already, GRBL repeats `[MSG:Check Door]`
    /// and, mid-restore, stops the outputs again.
    fn safety_door(&mut self) {
        if self.door_acts() {
            self.enter_door();
        }
    }

    /// Ctrl-X. Interrupting motion loses position, so GRBL raises `ALARM:3`.
    /// A completed hold has stopped the head where GRBL knows it is, so
    /// resetting then raises nothing, even with moves still queued: GRBL
    /// clears `STEP_CONTROL_EXECUTE_HOLD` when the hold completes, and
    /// `mc_reset` only alarms while it is set or a cycle runs.
    fn soft_reset(&mut self) {
        let aborted = matches!(
            self.state,
            MachineState::Run | MachineState::Jog | MachineState::Home
        ) || (matches!(self.state, MachineState::Hold(_) | MachineState::Door(_))
            && self.stop_reason.is_some());
        let was_locked = matches!(self.state, MachineState::Alarm | MachineState::Sleep);
        self.clear_volatile_state();
        if aborted {
            self.send_line(&format!("ALARM:{ALARM_ABORT_CYCLE}"));
        }
        let state = if aborted || was_locked {
            MachineState::Alarm
        } else {
            MachineState::Idle
        };
        self.set_state(state);
        self.print_banner();
    }

    fn adjust_override(&mut self, byte: u8) {
        let o = &mut self.overrides;
        let step = |value: u8, delta: i16| (i16::from(value) + delta).clamp(10, 200) as u8;
        match byte {
            0x90 => o.feed = 100,
            0x91 => o.feed = step(o.feed, 10),
            0x92 => o.feed = step(o.feed, -10),
            0x93 => o.feed = step(o.feed, 1),
            0x94 => o.feed = step(o.feed, -1),
            0x95 => o.rapid = 100,
            0x96 => o.rapid = 50,
            0x97 => o.rapid = 25,
            0x99 => o.spindle = 100,
            0x9A => o.spindle = step(o.spindle, 10),
            0x9B => o.spindle = step(o.spindle, -10),
            0x9C => o.spindle = step(o.spindle, 1),
            0x9D => o.spindle = step(o.spindle, -1),
            _ => return,
        }
        // GRBL reports changed overrides in the next status report and
        // replans running motion with them.
        self.ovr_counter = 0;
        self.motion.interrupt(self.now);
    }

    fn toggle_coolant(&mut self, byte: u8) {
        if matches!(
            self.state,
            MachineState::Idle | MachineState::Run | MachineState::Hold(_)
        ) {
            if byte == 0xA0 {
                self.flood = !self.flood;
            } else {
                self.mist = !self.mist;
            }
            self.modal.flood = self.flood;
            self.modal.mist = self.mist;
            self.ovr_counter = 0;
            self.motion.interrupt(self.now);
        }
    }

    // ---- reports ----

    /// `<State|MPos:...|Bf:...|FS:...|WCO:...>` per `$10`, with `WCO:` and
    /// `Ov:` on GRBL's refresh schedule (never both in one report).
    fn status_report(&mut self) {
        let mask = self.profile.settings.status_mask();
        let mut report = format!("<{}", self.state);
        if mask & 1 != 0 {
            report += &format!("|MPos:{}", triple(self.machine_position()));
        } else {
            report += &format!("|WPos:{}", triple(self.work_position()));
        }
        if mask & 2 != 0 {
            let blocks = self.profile.planner_blocks - self.motion.len();
            let bytes = self.rx_capacity().saturating_sub(self.rx.len());
            report += &format!("|Bf:{blocks},{bytes}");
        }
        let moving = matches!(
            self.state,
            MachineState::Run | MachineState::Hold(_) | MachineState::Jog | MachineState::Door(_)
        );
        let feed = if moving {
            self.motion.speed_at(self.now) * 60.0
        } else {
            0.0
        };
        let rpm = self
            .motion
            .current_laser()
            .map_or(self.laser.rpm, |pwm| pwm.rpm);
        report += &format!("|FS:{},{}", feed.round(), rpm.round());

        let busy = matches!(
            self.state,
            MachineState::Home
                | MachineState::Run
                | MachineState::Hold(_)
                | MachineState::Jog
                | MachineState::Door(_)
        );
        if self.wco_counter > 0 {
            self.wco_counter -= 1;
        } else {
            self.wco_counter = if busy { 29 } else { 9 };
            if self.ovr_counter == 0 {
                self.ovr_counter = 1;
            }
            report += &format!("|WCO:{}", triple(self.work_offset()));
        }
        if self.ovr_counter > 0 {
            self.ovr_counter -= 1;
        } else {
            self.ovr_counter = if busy { 19 } else { 9 };
            let o = self.overrides;
            report += &format!("|Ov:{},{},{}", o.feed, o.rapid, o.spindle);
            let accessories = self.accessories();
            if !accessories.is_empty() {
                report += &format!("|A:{accessories}");
            }
        }
        report.push('>');
        self.send_line(&report);
    }

    /// GRBL reports the spindle only while its PWM output is enabled.
    fn accessories(&self) -> String {
        let mut a = String::new();
        let on = self
            .motion
            .current_laser()
            .map_or(self.laser.on, |pwm| pwm.on);
        match self.spindle_dir {
            Spindle::Cw if on => a.push('S'),
            Spindle::Ccw if on => a.push('C'),
            _ => {}
        }
        if self.flood {
            a.push('F');
        }
        if self.mist {
            a.push('M');
        }
        a
    }

    /// `$G` parser state.
    fn parser_state(&self) -> String {
        let m = &self.modal;
        let spindle = match m.spindle {
            Spindle::Off => "M5",
            Spindle::Cw => "M3",
            Spindle::Ccw => "M4",
        };
        let coolant = match (m.mist, m.flood) {
            (false, false) => "M9",
            (true, false) => "M7",
            (false, true) => "M8",
            (true, true) => "M7 M8",
        };
        format!(
            "[GC:G{} G{} G{} {} {} {} {spindle} {coolant} T{} F{} S{}]",
            m.motion,
            54 + m.coord_select,
            17 + m.plane,
            if m.inches { "G20" } else { "G21" },
            if m.incremental { "G91" } else { "G90" },
            if m.inverse_time { "G93" } else { "G94" },
            m.tool,
            m.feed.round(),
            m.speed.round()
        )
    }

    /// `$#` coordinate parameters.
    fn parameter_lines(&self) -> Vec<String> {
        let c = &self.coordinates;
        let mut lines: Vec<String> = c
            .systems
            .iter()
            .enumerate()
            .map(|(i, system)| format!("[G{}:{}]", 54 + i, triple(*system)))
            .collect();
        lines.push(format!("[G28:{}]", triple(c.g28)));
        lines.push(format!("[G30:{}]", triple(c.g30)));
        lines.push(format!("[G92:{}]", triple(c.g92)));
        lines.push(format!(
            "[TLO:{}]",
            coordinate(self.modal.tool_length_offset)
        ));
        lines.push("[PRB:0.000,0.000,0.000:0]".to_string());
        lines
    }

    // ---- helpers ----

    /// Active work coordinate offset: the selected G54-G59 system plus G92,
    /// and the tool length offset on Z.
    fn work_offset(&self) -> [f64; 3] {
        let c = &self.coordinates;
        let system = c.systems[self.modal.coord_select];
        let mut offset: [f64; 3] = std::array::from_fn(|axis| system[axis] + c.g92[axis]);
        offset[2] += self.modal.tool_length_offset;
        offset
    }

    /// The laser output now if the head is still: GRBL keeps an `M3` laser
    /// (and any spindle outside laser mode) on while motion is stopped.
    fn stationary_laser(&self) -> Option<(f64, LaserMode, bool)> {
        let moving = !self.motion.is_empty() && self.motion_active();
        if moving || !self.laser.on || self.spindle_dir == Spindle::Off {
            return None;
        }
        let mode = if self.spindle_dir == Spindle::Ccw && self.profile.settings.laser_mode() {
            LaserMode::Dynamic
        } else {
            LaserMode::Constant
        };
        Some((self.laser.power, mode, self.flood))
    }

    /// Closes the open stationary burn into the trace if the output has
    /// changed, and opens a new one if the laser is on with the head still.
    fn update_burn(&mut self) {
        let output = self.stationary_laser();
        let position = self.machine_position();
        if let Some(burn) = self.burn {
            if output == Some((burn.power, burn.laser_mode, burn.air_assist))
                && burn.position == position
            {
                return;
            }
            self.burn = None;
            if self.now > burn.t0 {
                self.motion.record(TraceSegment {
                    start: burn.position,
                    end: burn.position,
                    t0: burn.t0,
                    t1: self.now,
                    feed: 0.0,
                    start_speed: 0.0,
                    end_speed: 0.0,
                    power: burn.power,
                    laser_mode: burn.laser_mode,
                    air_assist: burn.air_assist,
                    rapid: false,
                    line: burn.line,
                });
            }
        }
        if let Some((power, laser_mode, air_assist)) = output {
            self.burn = Some(Burn {
                t0: self.now,
                position,
                power,
                laser_mode,
                air_assist,
                line: self.lines_read,
            });
        }
    }

    /// Everything a reset clears. Settings and stored coordinates survive,
    /// and the head stays where it stopped.
    fn clear_volatile_state(&mut self) {
        self.rx.clear();
        self.faults.line_ready_at = None;
        // Flushed lines keep their numbers, so numbering still matches the wire.
        self.lines_read = self.lines_accepted;
        self.realtime.clear();
        self.line.clear();
        self.line_overflow = false;
        self.comment = Comment::None;
        self.motion.clear(self.now);
        self.gc_position = self.motion.position_at(self.now);
        self.ops.clear();
        self.dwell_until = None;
        self.dwell_paused = None;
        self.door_restore = None;
        self.stop_reason = None;
        self.critical = false;
        self.spindle_dir = Spindle::Off;
        self.laser = Pwm::OFF;
        self.spindle_stopped = false;
        self.spindle_stop_requested = false;
        self.coordinates.g92 = [0.0; 3];
        self.modal = Modal::default();
        self.overrides = Overrides::default();
        self.flood = false;
        self.mist = false;
        self.wco_counter = 0;
        self.ovr_counter = 0;
    }

    fn print_banner(&mut self) {
        self.send_raw(b"\r\n");
        self.send_line(&format!("Grbl {VERSION} ['$' for help]"));
        if self.state == MachineState::Alarm {
            self.send_line(UNLOCK_HINT);
        }
    }

    fn set_state(&mut self, to: MachineState) {
        if to != self.state {
            self.events.push(Event::StateChanged {
                at: self.now,
                from: self.state,
                to,
            });
            self.state = to;
        }
    }

    fn reply_error(&mut self, code: u8) {
        self.send_line(&format!("error:{code}"));
    }

    fn send_line(&mut self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.extend_from_slice(b"\r\n");
        self.send_raw(&bytes);
    }

    /// Output is lost while the cable is pulled ([`Fault::Disconnect`]).
    fn send_raw(&mut self, bytes: &[u8]) {
        if self.faults.disconnected {
            return;
        }
        self.output.extend_from_slice(bytes);
        self.events.push(Event::Sent {
            at: self.now,
            bytes: bytes.to_vec(),
        });
    }
}

enum Reply {
    /// Send `ok`.
    Ok,
    /// The handler already sent its acknowledgement.
    Sent,
}

/// Bytes GRBL acts on immediately instead of buffering. They may arrive
/// anywhere, even mid-line, and never count against the RX buffer.
fn is_realtime(byte: u8) -> bool {
    matches!(
        byte,
        b'?' | b'!' | b'~' | 0x18 | 0x84 | 0x85 | 0x90..=0x9E | 0xA0 | 0xA1
    )
}

/// `0.000,0.000,0.000`, never printing negative zero.
fn triple(v: [f64; 3]) -> String {
    v.map(coordinate).join(",")
}

/// `0.000`, never printing negative zero.
fn coordinate(n: f64) -> String {
    let s = format!("{n:.3}");
    if s == "-0.000" {
        "0.000".to_string()
    } else {
        s
    }
}

/// GRBL's `read_float`: optional sign, digits and one decimal point.
fn parse_number(text: &str) -> Option<f64> {
    let valid = !text.is_empty()
        && text
            .trim_start_matches(['-', '+'])
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
        && text.matches('.').count() <= 1
        && text.chars().any(|c| c.is_ascii_digit());
    if valid { text.parse().ok() } else { None }
}

/// Splits `X10Y-2.5F1000` into letter/value words.
fn parse_words(text: &str) -> Result<Vec<(char, f64)>, u8> {
    let mut words = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((_, letter)) = chars.next() {
        if !letter.is_ascii_uppercase() {
            return Err(ERR_EXPECTED_LETTER);
        }
        let start = chars.peek().map_or(text.len(), |&(i, _)| i);
        let mut end = start;
        while let Some(&(i, c)) = chars.peek() {
            if c.is_ascii_uppercase() {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let value = parse_number(&text[start..end]).ok_or(ERR_BAD_NUMBER)?;
        words.push((letter, value));
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_words_and_rejects_bad_numbers() {
        assert_eq!(
            parse_words("X10Y-2.5F1000").unwrap(),
            vec![('X', 10.0), ('Y', -2.5), ('F', 1000.0)]
        );
        assert_eq!(parse_words("X1..2"), Err(ERR_BAD_NUMBER));
        assert_eq!(parse_words("1X"), Err(ERR_EXPECTED_LETTER));
        assert_eq!(parse_words("X"), Err(ERR_BAD_NUMBER));
    }

    #[test]
    fn formats_coordinates_without_negative_zero() {
        assert_eq!(triple([-0.0001, 1.5, -2.0]), "0.000,1.500,-2.000");
    }
}
