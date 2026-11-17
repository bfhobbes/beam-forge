//! Headless job checks: stream a G-code job through the emulator the way a
//! host would and report what it did and anything unsafe about it.
//!
//! The job is sent with GRBL's character-counting protocol: lines go out as
//! long as the bytes still awaiting `ok`/`error:` fit the RX buffer, and each
//! response retires the oldest line. Statistics and most violations come from
//! the motion trace, so they describe what the machine physically did.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use serde::Serialize;

use crate::{Event, LaserMode, MachineState, Profile, Settings, Simulator, TraceSegment};

/// How long the host waits between polls of the serial port.
const POLL: Duration = Duration::from_millis(1);
/// Give up on a job that has not finished after this much simulated time.
const TIME_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);
/// Positions are quantised to whole steps; allow for rounding at the bed edge.
const BED_TOLERANCE: f64 = 1e-6;

/// What a job did on the simulated machine, and what was wrong with it.
/// Times are seconds from the first job line, distances mm, all rounded to
/// 0.001; power is a fraction of full PWM rounded to 0.0001.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// The machine profile's name.
    pub profile: String,
    /// Lines in the job, including blank and comment lines.
    pub lines: usize,
    pub total_time_s: f64,
    /// Each stretch with the laser enabled, from `M3`/`M4` to `M5`.
    pub passes: Vec<Pass>,
    /// Motion with the laser emitting.
    pub cut_distance_mm: f64,
    /// Motion with the laser dark.
    pub travel_distance_mm: f64,
    /// Every position the head visited, in machine coordinates; `None` if it
    /// never moved.
    pub bounds: Option<Bounds>,
    pub max_power: f64,
    pub violations: Vec<Violation>,
}

impl Report {
    /// The job ran without violations.
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

/// A stretch of the job with the laser enabled.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pass {
    pub start_s: f64,
    pub time_s: f64,
}

/// An axis-aligned box in machine coordinates, mm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// Something unsafe or wrong about a job. Line numbers count from 1.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Violation {
    /// A rapid (`G0`) moved with the laser emitting. In laser mode (`$32=1`)
    /// GRBL keeps the laser dark on rapids, so this only happens without it.
    LaserOnDuringRapid { time_s: f64, position: [f64; 3] },
    /// The laser was still emitting when the job finished.
    LaserOnAtEnd,
    /// The head left the bed. The bed is GRBL's machine space
    /// (`-$130..0`, `-$131..0`) when homing is enabled. Without homing GRBL
    /// has no machine reference, so the head is assumed to power up at the
    /// bed's origin corner and the bed is `0..$130`, `0..$131`. Z is not
    /// checked.
    OutsideBed { time_s: f64, position: [f64; 3] },
    /// The job finished with the laser enabled (`M3`/`M4`) but dark, so it
    /// is not burning yet but will fire on the next move. `M2`/`M30` turn it
    /// off like `M5` does.
    MissingM5,
    /// GRBL rejected a line.
    Error { line: usize, code: u8 },
    /// GRBL raised an alarm; the rest of the job was not sent.
    Alarm { line: Option<usize>, code: u8 },
    /// The line was longer than the RX buffer, so GRBL lost its last bytes.
    RxOverflow { line: usize, dropped: usize },
    /// The controller stopped making progress (sleep, or a job still running
    /// after a day), so the check gave up.
    Stalled { time_s: f64 },
}

impl Violation {
    /// The violation's kind, as in the JSON report.
    pub fn kind(&self) -> &'static str {
        match self {
            Violation::LaserOnDuringRapid { .. } => "laser_on_during_rapid",
            Violation::LaserOnAtEnd => "laser_on_at_end",
            Violation::OutsideBed { .. } => "outside_bed",
            Violation::MissingM5 => "missing_m5",
            Violation::Error { .. } => "error",
            Violation::Alarm { .. } => "alarm",
            Violation::RxOverflow { .. } => "rx_overflow",
            Violation::Stalled { .. } => "stalled",
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::LaserOnDuringRapid { time_s, position } => write!(
                f,
                "laser on during a rapid at {} ({time_s:.3} s)",
                xyz(position)
            ),
            Violation::LaserOnAtEnd => write!(f, "laser still on at the end of the job"),
            Violation::OutsideBed { time_s, position } => {
                write!(f, "outside the bed at {} ({time_s:.3} s)", xyz(position))
            }
            Violation::MissingM5 => write!(f, "laser still enabled at the end: missing M5"),
            Violation::Error { line, code } => write!(f, "line {line}: error:{code}"),
            Violation::Alarm {
                line: Some(line),
                code,
            } => write!(f, "line {line}: ALARM:{code}"),
            Violation::Alarm { line: None, code } => write!(f, "ALARM:{code}"),
            Violation::RxOverflow { line, dropped } => write!(
                f,
                "line {line}: longer than the RX buffer, {dropped} bytes lost"
            ),
            Violation::Stalled { time_s } => {
                write!(f, "controller stopped making progress at {time_s:.3} s")
            }
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Profile    {}", self.profile)?;
        writeln!(f, "Lines      {}", self.lines)?;
        writeln!(f, "Time       {}", seconds(self.total_time_s))?;
        for (index, pass) in self.passes.iter().enumerate() {
            writeln!(
                f,
                "  Pass {}   {} from {}",
                index + 1,
                seconds(pass.time_s),
                seconds(pass.start_s)
            )?;
        }
        writeln!(f, "Cut        {:.3} mm", self.cut_distance_mm)?;
        writeln!(f, "Travel     {:.3} mm", self.travel_distance_mm)?;
        match &self.bounds {
            Some(bounds) => writeln!(f, "Bounds     {} to {}", xyz(&bounds.min), xyz(&bounds.max))?,
            None => writeln!(f, "Bounds     no motion")?,
        }
        writeln!(f, "Max power  {:.1}%", self.max_power * 100.0)?;
        if self.passed() {
            write!(f, "PASS: no violations")
        } else {
            write!(f, "FAIL: {} violation(s)", self.violations.len())?;
            for violation in &self.violations {
                write!(f, "\n  {}: {violation}", violation.kind())?;
            }
            Ok(())
        }
    }
}

fn xyz(p: &[f64; 3]) -> String {
    format!("X{:.3} Y{:.3} Z{:.3}", p[0], p[1], p[2])
}

fn seconds(s: f64) -> String {
    let minutes = (s / 60.0).floor();
    if minutes > 0.0 {
        format!("{minutes:.0} min {:.3} s", s - minutes * 60.0)
    } else {
        format!("{s:.3} s")
    }
}

/// Streams `gcode` through a simulated machine with `profile` and runs it to
/// completion. If the profile enables homing, the host homes (`$H`) first,
/// as it must to unlock GRBL; that is not part of the job's time.
pub fn check(gcode: &str, profile: Profile) -> Report {
    let mut sim = Simulator::new(profile);
    sim.take_output();
    if sim.state() == MachineState::Alarm {
        sim.feed(b"$H\n");
        sim.advance(Duration::ZERO);
        sim.take_output();
    }
    sim.take_events();
    sim.take_trace();
    let start = sim.now();
    // A job may change settings; the bed is the machine it started on.
    let bed = bed(sim.settings());

    let lines: Vec<&str> = gcode.lines().collect();
    let mut host = Host::new(sim.profile().rx_buffer_size);
    let violations = host.stream(&mut sim, &lines, start);

    let end = sim.now();
    let elapsed = |at: Duration| (at.saturating_sub(start)).as_secs_f64();
    let mut report = summarise(&sim, sim.trace(), start);
    report.lines = lines.len();
    report.total_time_s = round3(elapsed(end));
    report.violations = violations;
    report
        .violations
        .extend(trace_violations(sim.trace(), &bed, start));
    if host.finished {
        if sim.laser_lit() {
            report.violations.push(Violation::LaserOnAtEnd);
        } else if sim.spindle_enabled() {
            report.violations.push(Violation::MissingM5);
        }
    }
    report
}

/// A character-counting host: how many bytes of which lines GRBL has not
/// acknowledged yet.
struct Host {
    capacity: usize,
    /// (line number, bytes sent) for each unacknowledged line, oldest first.
    in_flight: VecDeque<(usize, usize)>,
    received: String,
    /// The job ran to completion rather than stopping on an alarm or stall.
    finished: bool,
}

impl Host {
    fn new(capacity: usize) -> Self {
        Host {
            capacity,
            in_flight: Default::default(),
            received: String::new(),
            finished: false,
        }
    }

    fn in_flight_bytes(&self) -> usize {
        self.in_flight.iter().map(|&(_, bytes)| bytes).sum()
    }

    fn stream(&mut self, sim: &mut Simulator, lines: &[&str], start: Duration) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut next = 0;
        loop {
            // Send every line that fits. A line longer than the whole buffer
            // goes out once nothing else is in flight, and overflows.
            while let Some(line) = lines.get(next) {
                let bytes = format!("{}\n", line.trim());
                if !self.in_flight.is_empty()
                    && self.in_flight_bytes() + bytes.len() > self.capacity
                {
                    break;
                }
                next += 1;
                sim.feed(bytes.as_bytes());
                self.in_flight.push_back((next, bytes.len()));
                for event in sim.take_events() {
                    if let Event::RxOverflow { dropped, .. } = event {
                        violations.push(Violation::RxOverflow {
                            line: next,
                            dropped,
                        });
                    }
                }
            }
            sim.advance(POLL);
            sim.take_events();
            if let Some(alarm) = self.read_responses(sim, &mut violations) {
                violations.push(alarm);
                return violations;
            }
            if sim.is_quiescent() {
                // GRBL has nothing left to answer: responses still awaited
                // belong to bytes it lost.
                self.in_flight.clear();
                if next == lines.len() {
                    self.finished = true;
                    return violations;
                }
            }
            match sim.state() {
                // `M0` waits for the operator to press cycle start.
                MachineState::Hold(0) => sim.feed(b"~"),
                MachineState::Sleep => {
                    violations.push(Violation::Stalled {
                        time_s: round3((sim.now() - start).as_secs_f64()),
                    });
                    return violations;
                }
                _ => {}
            }
            if sim.now() - start > TIME_LIMIT {
                violations.push(Violation::Stalled {
                    time_s: round3((sim.now() - start).as_secs_f64()),
                });
                return violations;
            }
        }
    }

    /// Retires a line per `ok`/`error:`, recording errors. Returns the
    /// violation for an alarm, which ends the job.
    fn read_responses(
        &mut self,
        sim: &mut Simulator,
        violations: &mut Vec<Violation>,
    ) -> Option<Violation> {
        self.received
            .push_str(&String::from_utf8_lossy(&sim.take_output()));
        while let Some(end) = self.received.find("\r\n") {
            let response: String = self.received.drain(..end + 2).collect();
            let response = response.trim_end();
            if response == "ok" {
                self.in_flight.pop_front();
            } else if let Some(code) = response.strip_prefix("error:") {
                let line = self.in_flight.pop_front().map_or(0, |(line, _)| line);
                violations.push(Violation::Error {
                    line,
                    code: code.parse().unwrap_or(0),
                });
            } else if let Some(code) = response.strip_prefix("ALARM:") {
                // The alarm interrupts the oldest line, which never gets its
                // response.
                return Some(Violation::Alarm {
                    line: self.in_flight.front().map(|&(line, _)| line),
                    code: code.parse().unwrap_or(0),
                });
            }
        }
        None
    }
}

/// Statistics over the trace. Fills everything but the line count, total
/// time and violations.
fn summarise(sim: &Simulator, trace: &[TraceSegment], start: Duration) -> Report {
    let elapsed = |at: Duration| (at.saturating_sub(start)).as_secs_f64();
    let mut passes = Vec::new();
    let mut pass: Option<(Duration, Duration)> = None;
    let mut cut = 0.0;
    let mut travel = 0.0;
    let mut bounds: Option<([f64; 3], [f64; 3])> = None;
    let mut max_power = 0.0f64;
    for segment in trace {
        if segment.laser_mode == LaserMode::Off {
            if let Some((t0, t1)) = pass.take() {
                passes.push((t0, t1));
            }
        } else {
            let t0 = pass.map_or(segment.t0, |(t0, _)| t0);
            pass = Some((t0, segment.t1));
        }
        let length = distance(&segment.start, &segment.end);
        if segment.power > 0.0 {
            cut += length;
        } else {
            travel += length;
        }
        max_power = max_power.max(segment.power);
        for point in [segment.start, segment.end] {
            let (min, max) = bounds.get_or_insert((point, point));
            for axis in 0..3 {
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        }
    }
    passes.extend(pass);
    Report {
        profile: sim.profile().name.clone(),
        lines: 0,
        total_time_s: 0.0,
        passes: passes
            .into_iter()
            .map(|(t0, t1)| Pass {
                start_s: round3(elapsed(t0)),
                time_s: round3((t1 - t0).as_secs_f64()),
            })
            .collect(),
        cut_distance_mm: round3(cut),
        travel_distance_mm: round3(travel),
        bounds: bounds.map(|(min, max)| Bounds {
            min: min.map(round3),
            max: max.map(round3),
        }),
        max_power: round(max_power, 1e4),
        violations: Vec::new(),
    }
}

/// The X and Y range of the bed; see [`Violation::OutsideBed`].
fn bed(settings: &Settings) -> [(f64, f64); 2] {
    let travel: [f64; 2] = std::array::from_fn(|axis| settings.max_travel(axis));
    if settings.homing_enabled() {
        travel.map(|t| (-t, 0.0))
    } else {
        travel.map(|t| (0.0, t))
    }
}

/// Lit rapids and moves off the bed, one violation per run of offending
/// segments.
fn trace_violations(
    trace: &[TraceSegment],
    bed: &[(f64, f64); 2],
    start: Duration,
) -> Vec<Violation> {
    let on_bed = |p: &[f64; 3]| {
        (0..2).all(|axis| {
            p[axis] >= bed[axis].0 - BED_TOLERANCE && p[axis] <= bed[axis].1 + BED_TOLERANCE
        })
    };
    let time = |at: Duration| round3((at.saturating_sub(start)).as_secs_f64());

    let mut violations = Vec::new();
    let (mut lit_rapid, mut off_bed) = (false, false);
    for segment in trace {
        let lit = segment.rapid && segment.power > 0.0;
        if lit && !lit_rapid {
            violations.push(Violation::LaserOnDuringRapid {
                time_s: time(segment.t0),
                position: segment.start.map(round3),
            });
        }
        lit_rapid = lit;
        if !off_bed && let Some(fraction) = leaves_bed(segment, bed) {
            let position = std::array::from_fn(|axis| {
                segment.start[axis] + (segment.end[axis] - segment.start[axis]) * fraction
            });
            let at = segment.t0 + time_to(segment, fraction);
            violations.push(Violation::OutsideBed {
                time_s: time(at),
                position: position.map(round3),
            });
        }
        off_bed = !on_bed(&segment.end);
    }
    violations
}

/// How far along a segment, from 0 to 1, the head first leaves the bed.
fn leaves_bed(segment: &TraceSegment, bed: &[(f64, f64); 2]) -> Option<f64> {
    let (start, end) = (&segment.start, &segment.end);
    let mut fraction: Option<f64> = None;
    for (axis, &(lo, hi)) in bed.iter().enumerate() {
        let (lo, hi) = (lo - BED_TOLERANCE, hi + BED_TOLERANCE);
        if start[axis] < lo || start[axis] > hi {
            return Some(0.0);
        }
        let edge = if end[axis] > hi {
            hi
        } else if end[axis] < lo {
            lo
        } else {
            continue;
        };
        let f = (edge - start[axis]) / (end[axis] - start[axis]);
        fraction = Some(fraction.map_or(f, |g| g.min(f)));
    }
    fraction
}

/// Time from the start of a segment to `fraction` of its length, at its
/// constant acceleration.
fn time_to(segment: &TraceSegment, fraction: f64) -> Duration {
    let duration = (segment.t1 - segment.t0).as_secs_f64();
    let d = distance(&segment.start, &segment.end) * fraction;
    let (v0, v1) = (segment.start_speed / 60.0, segment.end_speed / 60.0);
    let accel = if duration > 0.0 {
        (v1 - v0) / duration
    } else {
        0.0
    };
    // d = v0 t + a t^2 / 2, in the form that stays stable as a goes to 0.
    let denominator = v0 + (v0 * v0 + 2.0 * accel * d).max(0.0).sqrt();
    let t = if denominator > 0.0 {
        2.0 * d / denominator
    } else {
        0.0
    };
    Duration::from_secs_f64(t.clamp(0.0, duration))
}

fn distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (b[axis] - a[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn round3(value: f64) -> f64 {
    round(value, 1e3)
}

/// Rounds to `1 / scale`, without printing negative zero.
fn round(value: f64, scale: f64) -> f64 {
    (value * scale).round() / scale + 0.0
}
