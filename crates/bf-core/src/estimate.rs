//! Job time estimates: how long the machine takes to make a plan's moves.
//!
//! The model is GRBL's motion planner. Each move is a straight block whose
//! speed is limited by its feed (cuts) or the axes' maximum rates (rapids,
//! `$110`, `$111`), and whose acceleration is limited per axis (`$120`,
//! `$121`). Through a corner the head may keep the speed GRBL's junction
//! deviation (`$11`) allows. Each block runs as a trapezoid: accelerate,
//! cruise, brake. Like GRBL, the planner only looks [`PLANNER_BLOCKS`] blocks
//! ahead and always leaves room to stop at the end of what it can see, so
//! jobs of many short moves run slower than their feed rates suggest.
//!
//! The firmware also waits for motion to finish before some commands (GRBL
//! syncs before laser and air-assist changes); the head is at rest there.
//! The estimate leaves out serial latency and the time commands take to
//! process, which matter only when the planner runs dry.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::machine::MachineProfile;
use crate::plan::{Move, Plan};

/// Blocks in GRBL's planner buffer: 16, one of which is always kept free.
pub const PLANNER_BLOCKS: usize = 15;
/// GRBL's `MINIMUM_FEED_RATE`, 1 mm/min, in mm/s.
const MINIMUM_SPEED: f64 = 1.0 / 60.0;
/// GRBL's `SOME_LARGE_VALUE`: the junction speed of a straight continuation.
const SOME_LARGE_VALUE: f64 = 1.0e38;
/// Shorter moves than this are dropped, as GRBL drops moves of no whole step.
const MIN_LENGTH: f64 = 1e-6;

/// What limits the head's motion, from the machine profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionLimits {
    /// X and Y maximum rates, mm/s.
    pub max_rate: [f64; 2],
    /// X and Y accelerations, mm/s².
    pub acceleration: [f64; 2],
    /// GRBL's junction deviation (`$11`), mm.
    pub junction_deviation: f64,
}

impl MotionLimits {
    pub fn of(profile: &MachineProfile) -> Self {
        MotionLimits {
            max_rate: [profile.max_rate.x / 60.0, profile.max_rate.y / 60.0],
            acceleration: [profile.acceleration.x, profile.acceleration.y],
            junction_deviation: profile.junction_deviation.max(0.0),
        }
    }
}

/// How long a job takes, by layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "plan/")]
pub struct JobEstimate {
    /// The whole job, seconds.
    pub seconds: f64,
    /// Each of the plan's layers, in cut order.
    pub layers: Vec<LayerEstimate>,
}

/// How long one layer takes, every pass included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "plan/")]
pub struct LayerEstimate {
    pub layer_id: String,
    pub seconds: f64,
}

impl JobEstimate {
    /// Sums `move_seconds` (from [`move_times`]) by layer.
    pub fn new(plan: &Plan, move_seconds: &[f64]) -> JobEstimate {
        let mut times = move_seconds.iter();
        let layers: Vec<_> = plan
            .layers
            .iter()
            .map(|layer| LayerEstimate {
                layer_id: layer.layer_id.clone(),
                seconds: times.by_ref().take(layer.moves.len()).sum(),
            })
            .collect();
        JobEstimate {
            seconds: layers.iter().map(|layer| layer.seconds).sum(),
            layers,
        }
    }
}

/// A move as GRBL's planner holds it.
#[derive(Debug, Clone, Copy)]
struct Block {
    /// The move's index in plan order.
    index: usize,
    /// mm
    length: f64,
    /// Cruise speed, mm/s.
    nominal: f64,
    /// mm/s²
    acceleration: f64,
    /// The fastest the head may enter, squared: limited by the junction with
    /// the previous block and both blocks' cruise speeds. 0 from rest.
    max_entry_sqr: f64,
}

/// Seconds each of `plan`'s moves takes, in plan order. The head starts and
/// ends at rest, and is at rest before each move whose index is in `stops`
/// (where the firmware waits for motion to finish).
pub fn move_times(plan: &Plan, limits: &MotionLimits, stops: &[usize]) -> Vec<f64> {
    let stops: BTreeSet<usize> = stops.iter().copied().collect();
    let mut blocks = Vec::new();
    let mut at = plan.start;
    // The previous block's direction and cruise speed, unless at rest.
    let mut previous: Option<([f64; 2], f64)> = None;
    let mut count = 0;
    for (index, step) in plan.moves().enumerate() {
        count += 1;
        if stops.contains(&index) {
            previous = None;
        }
        let (to, feed) = match *step {
            Move::Rapid { to } => (to, None),
            Move::Cut { to, speed, .. } => (to, Some(speed / 60.0)),
        };
        let delta = [to.x - at.x, to.y - at.y];
        at = to;
        let length = delta[0].hypot(delta[1]);
        if length < MIN_LENGTH {
            continue;
        }
        let unit = [delta[0] / length, delta[1] / length];
        let rapid_rate = limit_by_axis(limits.max_rate, unit);
        let nominal = feed
            .map_or(rapid_rate, |feed| feed.min(rapid_rate))
            .max(MINIMUM_SPEED);
        let max_entry_sqr = previous.map_or(0.0, |(before, previous_nominal)| {
            junction_speed_sqr(before, unit, limits)
                .min(nominal * nominal)
                .min(previous_nominal * previous_nominal)
        });
        blocks.push(Block {
            index,
            length,
            nominal,
            acceleration: limit_by_axis(limits.acceleration, unit),
            max_entry_sqr,
        });
        previous = Some((unit, nominal));
    }

    let mut seconds = vec![0.0; count];
    let mut entry_sqr: f64 = 0.0;
    for (i, block) in blocks.iter().enumerate() {
        // The fastest exit that still stops by the end of the buffer.
        let end = (i + PLANNER_BLOCKS).min(blocks.len());
        let mut exit_sqr: f64 = 0.0;
        for next in blocks[i + 1..end].iter().rev() {
            exit_sqr = next
                .max_entry_sqr
                .min(exit_sqr + 2.0 * next.acceleration * next.length);
        }
        let exit_sqr = exit_sqr.min(entry_sqr + 2.0 * block.acceleration * block.length);
        seconds[block.index] = trapezoid(block, entry_sqr, exit_sqr);
        entry_sqr = exit_sqr;
    }
    seconds
}

/// How long `block` takes from entry to exit speed (both squared): up to
/// its cruise speed, or as near as the length allows, and down again.
fn trapezoid(block: &Block, entry_sqr: f64, exit_sqr: f64) -> f64 {
    let a = block.acceleration;
    let peak_sqr =
        (0.5 * (2.0 * a * block.length + entry_sqr + exit_sqr)).min(block.nominal * block.nominal);
    let (entry, exit, peak) = (entry_sqr.sqrt(), exit_sqr.sqrt(), peak_sqr.sqrt());
    let ramps = (2.0 * peak_sqr - entry_sqr - exit_sqr) / (2.0 * a);
    let cruise = (block.length - ramps).max(0.0);
    ((peak - entry) + (peak - exit)) / a + cruise / peak
}

/// GRBL's `limit_value_by_axis_maximum`: the largest value along `unit` that
/// keeps each axis within its own limit.
fn limit_by_axis(limits: [f64; 2], unit: [f64; 2]) -> f64 {
    (0..2)
        .filter(|&axis| unit[axis] != 0.0)
        .map(|axis| (limits[axis] / unit[axis]).abs())
        .fold(SOME_LARGE_VALUE, f64::min)
}

/// GRBL's junction deviation: the fastest speed, squared, through the corner
/// from direction `before` to `after` that keeps the acceleration of a
/// circle `$11` from the corner within the axis limits.
fn junction_speed_sqr(before: [f64; 2], after: [f64; 2], limits: &MotionLimits) -> f64 {
    let cos_theta = -(before[0] * after[0] + before[1] * after[1]);
    if cos_theta > 0.999_999 {
        // A reversal: stop (GRBL's MINIMUM_JUNCTION_SPEED is 0).
        return 0.0;
    }
    if cos_theta < -0.999_999 {
        return SOME_LARGE_VALUE;
    }
    let junction = [after[0] - before[0], after[1] - before[1]];
    let norm = junction[0].hypot(junction[1]);
    let acceleration = limit_by_axis(
        limits.acceleration,
        [junction[0] / norm, junction[1] / norm],
    );
    let sin_theta_d2 = (0.5 * (1.0 - cos_theta)).sqrt();
    (acceleration * limits.junction_deviation * sin_theta_d2 / (1.0 - sin_theta_d2)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::LayerPlan;
    use crate::project::Point;

    /// 6000 mm/min (100 mm/s) and 500 mm/s² on both axes.
    const LIMITS: MotionLimits = MotionLimits {
        max_rate: [100.0, 100.0],
        acceleration: [500.0, 500.0],
        junction_deviation: 0.01,
    };

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn cut(x: f64, y: f64, speed: f64) -> Move {
        Move::Cut {
            to: p(x, y),
            power: 50.0,
            speed,
        }
    }

    fn rapid(x: f64, y: f64) -> Move {
        Move::Rapid { to: p(x, y) }
    }

    fn plan(layers: Vec<Vec<Move>>) -> Plan {
        Plan {
            start: p(0.0, 0.0),
            layers: layers
                .into_iter()
                .enumerate()
                .map(|(i, moves)| LayerPlan {
                    layer_id: format!("l{i}"),
                    air_assist: false,
                    moves,
                })
                .collect(),
        }
    }

    fn total(plan: &Plan, limits: &MotionLimits, stops: &[usize]) -> f64 {
        JobEstimate::new(plan, &move_times(plan, limits, stops)).seconds
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} s, expected {expected} s"
        );
    }

    #[test]
    fn a_long_move_accelerates_cruises_and_brakes() {
        // 100 mm/s at 500 mm/s²: 0.2 s and 10 mm to get up to speed, the
        // same to stop, and 80 mm at 100 mm/s between.
        close(
            total(&plan(vec![vec![rapid(100.0, 0.0)]]), &LIMITS, &[]),
            1.2,
        );
        // A cut at 3000 mm/min (50 mm/s): 0.1 s and 2.5 mm each way, then
        // 95 mm at 50 mm/s.
        close(
            total(&plan(vec![vec![cut(100.0, 0.0, 3000.0)]]), &LIMITS, &[]),
            0.2 + 95.0 / 50.0,
        );
    }

    #[test]
    fn a_short_move_never_reaches_its_feed() {
        // 10 mm: accelerate for 5 mm (t = sqrt(2 * 5 / 500) = 0.1414 s) and
        // brake for the other 5.
        close(
            total(&plan(vec![vec![rapid(10.0, 0.0)]]), &LIMITS, &[]),
            2.0 * (2.0 * 5.0 / 500.0_f64).sqrt(),
        );
    }

    #[test]
    fn a_straight_continuation_keeps_its_speed() {
        // Two 50 mm halves of a 100 mm line take as long as the whole line.
        close(
            total(
                &plan(vec![vec![rapid(50.0, 0.0), rapid(100.0, 0.0)]]),
                &LIMITS,
                &[],
            ),
            1.2,
        );
        // A stop between them makes each half its own trapezoid: 0.2 s up,
        // 30 mm at 100 mm/s, 0.2 s down.
        close(
            total(
                &plan(vec![vec![rapid(50.0, 0.0)], vec![rapid(100.0, 0.0)]]),
                &LIMITS,
                &[1],
            ),
            2.0 * (0.4 + 0.3),
        );
    }

    #[test]
    fn a_reversal_stops() {
        // There and back: two full stops, 1.2 s each way.
        close(
            total(
                &plan(vec![vec![rapid(100.0, 0.0), rapid(0.0, 0.0)]]),
                &LIMITS,
                &[],
            ),
            2.4,
        );
    }

    #[test]
    fn a_right_angle_slows_to_the_junction_speed() {
        // 90°: cos θ = 0, sin(θ/2) = √½. The junction direction is diagonal,
        // so its acceleration is 500 / cos 45° = 707.1 mm/s², and
        // v² = 707.1 × 0.01 × 0.7071 / (1 − 0.7071) = 17.07 (4.13 mm/s).
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let v_sqr = 500.0 / half * 0.01 * half / (1.0 - half);
        let v = f64::sqrt(v_sqr);
        assert!((v - 4.1317).abs() < 1e-3, "{v}");
        // Each 100 mm leg: 0.2 s and 10 mm between rest and full speed, and
        // (100² − v²) / 1000 mm and (100 − v) / 500 s between v and full.
        let ramp = (100.0 * 100.0 - v_sqr) / 1000.0;
        let leg = 0.2 + (100.0 - v) / 500.0 + (100.0 - 10.0 - ramp) / 100.0;
        close(
            total(
                &plan(vec![vec![rapid(100.0, 0.0), rapid(100.0, 100.0)]]),
                &LIMITS,
                &[],
            ),
            2.0 * leg,
        );
    }

    #[test]
    fn diagonals_are_limited_by_each_axis() {
        // X is slower: 3000 mm/min and 250 mm/s². Along a 3-4-5 diagonal X
        // carries 3/5 of the speed, so the move may run at 50 / 0.6 =
        // 83.3 mm/s and accelerate at 250 / 0.6 = 416.7 mm/s²; Y (4/5)
        // limits neither below 100 / 0.8 and 500 / 0.8.
        let limits = MotionLimits {
            max_rate: [50.0, 100.0],
            acceleration: [250.0, 500.0],
            ..LIMITS
        };
        let (v, a) = (50.0 / 0.6, 250.0 / 0.6);
        let ramp = v * v / (2.0 * a);
        close(
            total(&plan(vec![vec![rapid(60.0, 80.0)]]), &limits, &[]),
            2.0 * v / a + (100.0 - 2.0 * ramp) / v,
        );
    }

    #[test]
    fn the_planner_only_sees_fifteen_blocks_ahead() {
        // 0.1 mm cuts in a straight 100 mm line at 6000 mm/min. Seeing 15
        // blocks and keeping room to stop after the last, the head may only
        // leave a block as fast as it can stop from in the 14 blocks
        // (1.4 mm) behind it: v² = 2 × 500 × 1.4, 37.4 mm/s, far under the
        // feed. It ramps up to that over the first 14 blocks and down over
        // the last 14, v/a = 0.075 s each. Each block between enters and
        // leaves at v but, like GRBL, speeds up to v² + 50 (50 = a × 0.1 mm)
        // and back down within the block.
        let moves: Vec<_> = (1..=1000)
            .map(|i| cut(f64::from(i) * 0.1, 0.0, 6000.0))
            .collect();
        let (v_sqr, a) = (1400.0_f64, 500.0);
        let ramps = 2.0 * v_sqr.sqrt() / a;
        let middle = 2.0 * ((v_sqr + 50.0).sqrt() - v_sqr.sqrt()) / a;
        close(
            total(&plan(vec![moves]), &LIMITS, &[]),
            ramps + 972.0 * middle,
        );
    }

    #[test]
    fn layers_add_up_and_empty_moves_take_no_time() {
        let job = plan(vec![
            vec![rapid(100.0, 0.0), rapid(100.0, 0.0)],
            vec![rapid(0.0, 0.0)],
        ]);
        let seconds = move_times(&job, &LIMITS, &[2]);
        assert_eq!(seconds.len(), 3);
        assert_eq!(seconds[1], 0.0);
        let estimate = JobEstimate::new(&job, &seconds);
        close(estimate.layers[0].seconds, 1.2);
        close(estimate.layers[1].seconds, 1.2);
        close(estimate.seconds, 2.4);
        assert_eq!(estimate.layers[1].layer_id, "l1");
        let json = serde_json::to_value(&estimate).unwrap();
        assert_eq!(json["layers"][0]["layerId"], "l0");
    }

    #[test]
    fn limits_come_from_the_profile() {
        let profile = MachineProfile::default();
        let limits = MotionLimits::of(&profile);
        assert_eq!(limits.max_rate, [500.0 / 60.0, 500.0 / 60.0]);
        assert_eq!(limits.acceleration, [10.0, 10.0]);
        assert_eq!(limits.junction_deviation, 0.01);
    }
}
