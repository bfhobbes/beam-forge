//! Motion: GRBL's planner and step segment generator, as exact kinematics on
//! the simulated clock.
//!
//! Each queued block is a straight line in machine coordinates, quantised to
//! whole steps like GRBL's planner. A block's speed and acceleration are
//! limited per axis (`$110`-`$112`, `$120`-`$122`), the speed through each
//! junction is limited from `$11`, and the plan always leaves room to stop at
//! the end of the buffer. The front block runs as a trapezoid. Speed changes
//! in 10 ms pieces like GRBL's segment generator, which is also the cadence at
//! which GRBL recomputes `M4` laser power.

use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// GRBL's `ACCELERATION_TICKS_PER_SECOND` (100): one speed step per 10 ms.
const SEGMENT_SECONDS: f64 = 0.01;
/// GRBL's `MINIMUM_FEED_RATE`, 1 mm/min, in mm/s.
const MINIMUM_SPEED: f64 = 1.0 / 60.0;
/// GRBL's `SOME_LARGE_VALUE`: the junction speed of a straight continuation.
const SOME_LARGE_VALUE: f64 = 1.0e38;
const EPSILON: f64 = 1e-9;

/// Spindle (laser) direction, as set by `M3`/`M4`/`M5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spindle {
    Off,
    Cw,
    Ccw,
}

/// How a segment's laser power was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaserMode {
    /// `M5`: off.
    Off,
    /// `M3`, or `M4` without laser mode (`$32=0`): power follows S alone.
    Constant,
    /// `M4` in laser mode: power scales with current speed / programmed feed.
    Dynamic,
}

/// A piece of executed motion: a straight move at constant acceleration with
/// constant laser power. Positions are machine coordinates in mm, speeds are
/// mm/min. During acceleration GRBL changes speed (and `M4` power) every
/// 10 ms, so a trace has one segment per 10 ms while speed changes and one per
/// cruise.
///
/// Laser-on time with the head still (an `M3` laser during a dwell or
/// between moves) is a stationary segment: `start == end`, zero speeds and
/// feed. See [`TraceSegment::is_stationary`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceSegment {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub t0: Duration,
    pub t1: Duration,
    /// Programmed feed rate of the block, mm/min. Rapids report the block's
    /// rapid rate.
    pub feed: f64,
    pub start_speed: f64,
    pub end_speed: f64,
    /// Laser power as a fraction of full PWM, 0 to 1. GRBL's linear model maps
    /// S from `$31` (0) to `$30` (1); the 8-bit PWM quantisation is not modelled.
    pub power: f64,
    pub laser_mode: LaserMode,
    pub air_assist: bool,
    /// G0 (rapid) motion.
    pub rapid: bool,
    /// The line that queued this motion, or the last line read when a
    /// stationary burn began: the count of line ends (`\n` or `\r`) the RX
    /// buffer had accepted, up to and including that line's, since this
    /// [`crate::Simulator`] was created. Empty lines and `$` commands count,
    /// and resets do not restart it, so it matches a count of the wire.
    pub line: u64,
}

impl TraceSegment {
    /// Laser-on time with the head still, rather than motion.
    pub fn is_stationary(&self) -> bool {
        self.start == self.end && self.start_speed == 0.0 && self.end_speed == 0.0
    }
}

/// Machine limits from the `$` settings, in mm and seconds.
pub(crate) struct Limits {
    pub steps_per_mm: [f64; 3],
    pub max_rate: [f64; 3],
    pub acceleration: [f64; 3],
    pub junction_deviation: f64,
}

/// What a line asks the planner for: GRBL's `plan_line_data_t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LineData {
    /// mm/min; ignored for rapids.
    pub feed: f64,
    pub rapid: bool,
    /// Jogs ignore the feed override.
    pub no_feed_override: bool,
    /// `G93`: finish the block in 1/`feed` minutes.
    pub inverse_time: bool,
    pub spindle: Spindle,
    /// Spindle speed (S) for this block; zero for laser-disabled motion.
    pub rpm: f64,
    /// The line that queued the block; see [`TraceSegment::line`].
    pub line: u64,
}

/// Run-time inputs to execution that can change mid-move.
pub(crate) struct Context {
    /// Overrides in percent.
    pub feed_override: f64,
    pub rapid_override: f64,
    pub spindle_override: f64,
    pub laser_mode: bool,
    pub rpm_min: f64,
    pub rpm_max: f64,
    pub air_assist: bool,
}

/// A laser (spindle PWM) output, computed like GRBL's
/// `spindle_compute_pwm_value`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pwm {
    /// The spindle speed GRBL reports in `FS:` after clamping.
    pub rpm: f64,
    pub power: f64,
    /// Whether the PWM output is enabled at all.
    pub on: bool,
}

impl Pwm {
    pub const OFF: Pwm = Pwm {
        rpm: 0.0,
        power: 0.0,
        on: false,
    };

    /// S clamped to `$31`..`$30` after the spindle override. S0 is off; any
    /// other S at or below `$31` runs at the minimum. Like GRBL, `$31 >= $30`
    /// leaves no PWM range and the output is fully on.
    pub fn compute(rpm: f64, ctx: &Context) -> Pwm {
        let rpm = rpm * ctx.spindle_override / 100.0;
        let (min, max) = (ctx.rpm_min, ctx.rpm_max);
        if min >= max || rpm >= max {
            Pwm {
                rpm: max,
                power: 1.0,
                on: true,
            }
        } else if rpm <= min {
            if rpm == 0.0 {
                Pwm::OFF
            } else {
                Pwm {
                    rpm: min,
                    power: 0.0,
                    on: true,
                }
            }
        } else {
            Pwm {
                rpm,
                power: (rpm - min) / (max - min),
                on: true,
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Block {
    start: [f64; 3],
    target: [f64; 3],
    unit: [f64; 3],
    length: f64,
    /// mm/s
    programmed_rate: f64,
    rapid_rate: f64,
    /// mm/s^2
    acceleration: f64,
    /// (mm/s)^2
    max_junction_sqr: f64,
    data: LineData,
}

impl Block {
    /// GRBL's `plan_compute_profile_nominal_speed`.
    fn nominal_speed(&self, ctx: &Context) -> f64 {
        let speed = if self.data.rapid {
            self.programmed_rate * ctx.rapid_override / 100.0
        } else {
            let speed = if self.data.no_feed_override {
                self.programmed_rate
            } else {
                self.programmed_rate * ctx.feed_override / 100.0
            };
            speed.min(self.rapid_rate)
        };
        speed.max(MINIMUM_SPEED)
    }

    fn point(&self, s: f64) -> [f64; 3] {
        if s >= self.length {
            self.target
        } else {
            std::array::from_fn(|axis| self.start[axis] + self.unit[axis] * s)
        }
    }

    /// Laser output with the head moving at `speed`, per GRBL's segment
    /// generator: `M4` in laser mode scales S by speed / programmed rate.
    fn laser(&self, speed: f64, ctx: &Context) -> (Pwm, LaserMode) {
        match self.data.spindle {
            Spindle::Off => (Pwm::OFF, LaserMode::Off),
            Spindle::Ccw if ctx.laser_mode => {
                let rpm = self.data.rpm * speed / self.programmed_rate;
                (Pwm::compute(rpm, ctx), LaserMode::Dynamic)
            }
            _ => (Pwm::compute(self.data.rpm, ctx), LaserMode::Constant),
        }
    }
}

/// The piece being executed: constant acceleration from `s0` to `s1` along
/// the front block.
#[derive(Debug, Clone)]
struct Piece {
    t0: Duration,
    t1: Duration,
    duration: f64,
    s0: f64,
    s1: f64,
    v0: f64,
    v1: f64,
    accel: f64,
    pwm: Pwm,
    laser_mode: LaserMode,
    air_assist: bool,
}

impl Piece {
    /// Distance along the block and speed at `now`.
    fn at(&self, now: Duration) -> (f64, f64) {
        if now >= self.t1 {
            return (self.s1, self.v1);
        }
        let tau = now.saturating_sub(self.t0).as_secs_f64().min(self.duration);
        let s = self.s0 + self.v0 * tau + 0.5 * self.accel * tau * tau;
        (
            s.clamp(self.s0, self.s1),
            (self.v0 + self.accel * tau).max(0.0),
        )
    }
}

pub(crate) enum Step {
    /// A piece finished at this time.
    Executed(Duration),
    /// The current piece runs past the end of the window.
    Pending,
    /// Nothing to run: the buffer is empty or a stop has completed.
    Stopped,
}

/// The planner buffer and the executing move.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    blocks: VecDeque<Block>,
    /// GRBL's `pl.position`: where the last queued block ends, in whole steps.
    planned: [f64; 3],
    /// Machine position at the start of the current piece, or now if idle.
    position: [f64; 3],
    /// Speed at the start of the current piece, mm/s.
    speed: f64,
    /// Distance travelled along the front block.
    progress: f64,
    piece: Option<Piece>,
    /// Decelerating to a stop (feed hold, jog cancel, soft limit).
    stopping: bool,
    /// Laser output and mode of the last piece executed.
    last_laser: Option<(Pwm, LaserMode)>,
    trace: Vec<TraceSegment>,
}

impl Motion {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping
    }

    pub fn trace(&self) -> &[TraceSegment] {
        &self.trace
    }

    pub fn take_trace(&mut self) -> Vec<TraceSegment> {
        std::mem::take(&mut self.trace)
    }

    /// Adds a segment that did not come from the planner: a stationary burn.
    pub fn record(&mut self, segment: TraceSegment) {
        self.trace.push(segment);
    }

    pub fn position_at(&self, now: Duration) -> [f64; 3] {
        match (&self.piece, self.blocks.front()) {
            (Some(piece), Some(block)) => block.point(piece.at(now).0),
            _ => self.position,
        }
    }

    /// Current speed in mm/s.
    pub fn speed_at(&self, now: Duration) -> f64 {
        self.piece
            .as_ref()
            .map_or(self.speed, |piece| piece.at(now).1)
    }

    /// Laser output of the executing piece.
    pub fn current_laser(&self) -> Option<Pwm> {
        self.piece.as_ref().map(|piece| piece.pwm)
    }

    pub fn last_laser(&self) -> Option<(Pwm, LaserMode)> {
        self.last_laser
    }

    /// Queues a line, like GRBL's `plan_buffer_line`. Returns false for a
    /// block that moves no whole step, which GRBL discards.
    pub fn plan(&mut self, target: [f64; 3], data: LineData, limits: &Limits) -> bool {
        let start = self.planned;
        let mut delta = [0.0; 3];
        let mut target_q = [0.0; 3];
        let mut steps = 0.0_f64;
        for axis in 0..3 {
            let spm = limits.steps_per_mm[axis];
            let target_steps = (target[axis] * spm).round();
            let start_steps = (start[axis] * spm).round();
            steps = steps.max((target_steps - start_steps).abs());
            target_q[axis] = target_steps / spm;
            delta[axis] = (target_steps - start_steps) / spm;
        }
        if steps == 0.0 {
            return false;
        }
        let length = delta.iter().map(|d| d * d).sum::<f64>().sqrt();
        let unit = delta.map(|d| d / length);
        let acceleration = limit_by_axis(limits.acceleration, unit);
        let rapid_rate = limit_by_axis(limits.max_rate, unit);
        let programmed_rate = if data.rapid {
            rapid_rate
        } else if data.inverse_time {
            data.feed * length / 60.0
        } else {
            data.feed / 60.0
        };
        let max_junction_sqr = match self.blocks.back() {
            None => 0.0,
            Some(previous) => junction_speed_sqr(previous.unit, unit, limits),
        };
        self.blocks.push_back(Block {
            start,
            target: target_q,
            unit,
            length,
            programmed_rate,
            rapid_rate,
            acceleration,
            max_junction_sqr,
            data,
        });
        self.planned = target_q;
        true
    }

    /// Runs until the current piece ends or `end`, whichever is first.
    pub fn step(&mut self, now: Duration, end: Duration, ctx: &Context) -> Step {
        if self.piece.is_none() {
            self.piece = self.next_piece(now, ctx);
        }
        match &self.piece {
            None => Step::Stopped,
            Some(piece) if piece.t1 <= end => {
                let t1 = piece.t1;
                self.finish_piece(t1);
                Step::Executed(t1)
            }
            Some(_) => Step::Pending,
        }
    }

    /// Ends the current piece early at `now`, so the next one is planned
    /// with up-to-date overrides and hold state.
    pub fn interrupt(&mut self, now: Duration) {
        let Some(piece) = &self.piece else { return };
        if now >= piece.t1 {
            self.finish_piece(piece.t1);
        } else {
            self.finish_piece(now);
        }
    }

    /// Starts decelerating to a stop.
    pub fn stop(&mut self, now: Duration) {
        self.interrupt(now);
        self.stopping = true;
    }

    /// Resumes after a completed hold.
    pub fn resume(&mut self) {
        self.stopping = false;
    }

    /// Discards all queued motion where the head is now.
    pub fn clear(&mut self, now: Duration) {
        self.interrupt(now);
        self.blocks.clear();
        self.progress = 0.0;
        self.speed = 0.0;
        self.stopping = false;
        self.planned = self.position;
    }

    /// Discards all motion and puts the head at `position`, as homing does.
    pub fn reset_to(&mut self, position: [f64; 3]) {
        self.piece = None;
        self.blocks.clear();
        self.progress = 0.0;
        self.speed = 0.0;
        self.stopping = false;
        self.position = position;
        self.planned = position;
    }

    fn finish_piece(&mut self, at: Duration) {
        let Some(piece) = self.piece.take() else {
            return;
        };
        let block = self
            .blocks
            .front()
            .expect("a piece runs on the front block");
        let (s, v) = piece.at(at);
        let start = block.point(piece.s0);
        let end = block.point(s);
        if at > piece.t0 {
            self.trace.push(TraceSegment {
                start,
                end,
                t0: piece.t0,
                t1: at,
                feed: block.programmed_rate * 60.0,
                start_speed: piece.v0 * 60.0,
                end_speed: v * 60.0,
                power: piece.pwm.power,
                laser_mode: piece.laser_mode,
                air_assist: piece.air_assist,
                rapid: block.data.rapid,
                line: block.data.line,
            });
        }
        self.last_laser = Some((piece.pwm, piece.laser_mode));
        self.position = end;
        self.progress = s;
        self.speed = v;
        if s >= block.length {
            self.blocks.pop_front();
            self.progress = 0.0;
        }
    }

    /// Squared entry speed of the second block, from a reverse pass over the
    /// buffer that ends at rest: the fastest the front block may exit.
    fn exit_speed_sqr(&self, ctx: &Context) -> f64 {
        let mut entry_sqr = 0.0;
        for i in (1..self.blocks.len()).rev() {
            let block = &self.blocks[i];
            let nominal = block.nominal_speed(ctx);
            let previous = self.blocks[i - 1].nominal_speed(ctx);
            let max_entry_sqr = block
                .max_junction_sqr
                .min(nominal * nominal)
                .min(previous * previous);
            entry_sqr = max_entry_sqr.min(entry_sqr + 2.0 * block.acceleration * block.length);
        }
        entry_sqr
    }

    /// Plans the next piece of the front block: accelerate, cruise or brake.
    fn next_piece(&mut self, now: Duration, ctx: &Context) -> Option<Piece> {
        loop {
            let block = self.blocks.front()?;
            let remaining = block.length - self.progress;
            if remaining <= EPSILON {
                self.position = block.target;
                self.blocks.pop_front();
                self.progress = 0.0;
                continue;
            }
            let a = block.acceleration;
            let v = self.speed;
            let (v_end, len) = if self.stopping {
                if v <= EPSILON {
                    self.speed = 0.0;
                    return None;
                }
                let braking = v * v / (2.0 * a);
                if braking < remaining {
                    (0.0, braking)
                } else {
                    ((v * v - 2.0 * a * remaining).max(0.0).sqrt(), remaining)
                }
            } else {
                let nominal = block.nominal_speed(ctx);
                let exit_sqr = self.exit_speed_sqr(ctx).min(v * v + 2.0 * a * remaining);
                let braking = (v * v - exit_sqr).max(0.0) / (2.0 * a);
                if braking >= remaining - EPSILON {
                    (exit_sqr.sqrt().min(v), remaining)
                } else if v > nominal + EPSILON {
                    // An override lowered the speed: slow to the new nominal.
                    let to_nominal = (v * v - nominal * nominal) / (2.0 * a);
                    if to_nominal <= EPSILON {
                        self.speed = nominal;
                        continue;
                    }
                    (nominal, to_nominal)
                } else if v < nominal - EPSILON {
                    let to_nominal = (nominal * nominal - v * v) / (2.0 * a);
                    if to_nominal <= EPSILON {
                        self.speed = nominal;
                        continue;
                    }
                    // Where the acceleration and braking curves meet.
                    let meet = (exit_sqr - v * v + 2.0 * a * remaining) / (4.0 * a);
                    let len = to_nominal.min(meet);
                    ((v * v + 2.0 * a * len).sqrt(), len)
                } else {
                    (v, remaining - braking)
                }
            };
            let accel = (v_end * v_end - v * v) / (2.0 * len);
            let total = 2.0 * len / (v + v_end);
            let (duration, s_len, v1) = if accel.abs() > EPSILON && total > SEGMENT_SECONDS {
                let dt = SEGMENT_SECONDS;
                (dt, v * dt + 0.5 * accel * dt * dt, v + accel * dt)
            } else {
                (total, len, v_end)
            };
            let s1 = if self.progress + s_len >= block.length - EPSILON {
                block.length
            } else {
                self.progress + s_len
            };
            let (pwm, laser_mode) = block.laser(v1, ctx);
            return Some(Piece {
                t0: now,
                t1: now + Duration::from_secs_f64(duration),
                duration,
                s0: self.progress,
                s1,
                v0: v,
                v1,
                accel,
                pwm,
                laser_mode,
                air_assist: ctx.air_assist,
            });
        }
    }
}

/// GRBL's `limit_value_by_axis_maximum`: the largest value along `unit` that
/// keeps every axis within its own limit.
fn limit_by_axis(limits: [f64; 3], unit: [f64; 3]) -> f64 {
    (0..3)
        .filter(|&axis| unit[axis] != 0.0)
        .map(|axis| (limits[axis] / unit[axis]).abs())
        .fold(SOME_LARGE_VALUE, f64::min)
}

/// GRBL's junction deviation model: the fastest speed through the corner
/// that keeps centripetal acceleration on a circle `$11` from the corner
/// within the axis limits.
fn junction_speed_sqr(previous: [f64; 3], unit: [f64; 3], limits: &Limits) -> f64 {
    let cos_theta: f64 = -(0..3).map(|i| previous[i] * unit[i]).sum::<f64>();
    if cos_theta > 0.999999 {
        // A full reversal: stop (GRBL's MINIMUM_JUNCTION_SPEED is 0).
        return 0.0;
    }
    if cos_theta < -0.999999 {
        return SOME_LARGE_VALUE;
    }
    let junction: [f64; 3] = std::array::from_fn(|i| unit[i] - previous[i]);
    let norm = junction.iter().map(|v| v * v).sum::<f64>().sqrt();
    let junction = junction.map(|v| v / norm);
    let acceleration = limit_by_axis(limits.acceleration, junction);
    let sin_theta_d2 = (0.5 * (1.0 - cos_theta)).sqrt();
    (acceleration * limits.junction_deviation * sin_theta_d2 / (1.0 - sin_theta_d2)).max(0.0)
}

/// An arc's plane: GRBL's `axis_0` and `axis_1` span it and the arc moves
/// linearly along `axis_linear`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plane {
    pub axis_0: usize,
    pub axis_1: usize,
    pub axis_linear: usize,
}

impl Plane {
    /// `G17`, `G18` or `G19`, as 0, 1 or 2.
    pub fn select(plane: u8) -> Plane {
        let [axis_0, axis_1, axis_linear] = match plane {
            0 => [0, 1, 2],
            1 => [2, 0, 1],
            _ => [1, 2, 0],
        };
        Plane {
            axis_0,
            axis_1,
            axis_linear,
        }
    }
}

/// GRBL's `mc_arc`: the points of the line segments that approximate an arc
/// within the `$12` tolerance, ending exactly at `target`, and GRBL's segment
/// count (0 when the arc is a single line). `offset` is the centre relative
/// to `position`, by axis.
pub(crate) fn arc_points(
    position: [f64; 3],
    target: [f64; 3],
    offset: [f64; 3],
    radius: f64,
    clockwise: bool,
    tolerance: f64,
    plane: Plane,
) -> (Vec<[f64; 3]>, u32) {
    const ARC_ANGULAR_TRAVEL_EPSILON: f64 = 5e-7;
    const N_ARC_CORRECTION: u32 = 12;
    let Plane {
        axis_0,
        axis_1,
        axis_linear,
    } = plane;
    let offset = [offset[axis_0], offset[axis_1]];
    let center = [position[axis_0] + offset[0], position[axis_1] + offset[1]];
    let (mut r0, mut r1) = (-offset[0], -offset[1]);
    let rt0 = target[axis_0] - center[0];
    let rt1 = target[axis_1] - center[1];
    let mut angular_travel = (r0 * rt1 - r1 * rt0).atan2(r0 * rt0 + r1 * rt1);
    if clockwise {
        if angular_travel >= -ARC_ANGULAR_TRAVEL_EPSILON {
            angular_travel -= 2.0 * std::f64::consts::PI;
        }
    } else if angular_travel <= ARC_ANGULAR_TRAVEL_EPSILON {
        angular_travel += 2.0 * std::f64::consts::PI;
    }
    let segments = ((0.5 * angular_travel * radius).abs()
        / (tolerance * (2.0 * radius - tolerance)).sqrt())
    .floor() as u32;
    let mut points = Vec::with_capacity(segments as usize + 1);
    if segments > 0 {
        let theta = angular_travel / f64::from(segments);
        let linear = (target[axis_linear] - position[axis_linear]) / f64::from(segments);
        // GRBL's small-angle rotation, corrected exactly every N steps.
        let mut cos_t = 2.0 - theta * theta;
        let sin_t = theta * 0.166_666_67 * (cos_t + 4.0);
        cos_t *= 0.5;
        let mut point = position;
        let mut count = 0;
        for i in 1..segments {
            if count < N_ARC_CORRECTION {
                let ri = r0 * sin_t + r1 * cos_t;
                r0 = r0 * cos_t - r1 * sin_t;
                r1 = ri;
                count += 1;
            } else {
                let (sin_ti, cos_ti) = (f64::from(i) * theta).sin_cos();
                r0 = -offset[0] * cos_ti + offset[1] * sin_ti;
                r1 = -offset[0] * sin_ti - offset[1] * cos_ti;
                count = 0;
            }
            point[axis_0] = center[0] + r0;
            point[axis_1] = center[1] + r1;
            point[axis_linear] += linear;
            points.push(point);
        }
    }
    points.push(target);
    (points, segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_limits_scale_with_direction() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!((limit_by_axis([10.0, 5.0, 1.0], [s, s, 0.0]) - 5.0 / s).abs() < 1e-9);
        assert_eq!(limit_by_axis([10.0, 5.0, 1.0], [1.0, 0.0, 0.0]), 10.0);
    }

    #[test]
    fn arc_points_follow_the_circle_and_end_on_target() {
        // Quarter circle, radius 10, centre at (0, 10) relative to the start.
        let (points, segments) = arc_points(
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
            10.0,
            false,
            0.002,
            Plane::select(0),
        );
        // floor((pi/4 * 10) / sqrt(0.002 * (20 - 0.002))) = 39 segments.
        assert_eq!(segments, 39);
        assert_eq!(points.len(), 39);
        assert_eq!(*points.last().unwrap(), [10.0, 10.0, 0.0]);
        for p in &points {
            let r = (p[0].powi(2) + (p[1] - 10.0).powi(2)).sqrt();
            // GRBL's small-angle rotation drifts slightly between corrections.
            assert!((r - 10.0).abs() < 1e-4, "{p:?}");
        }
    }
}
