//! Feed and laser power overrides, changed live while a job runs.
//!
//! GRBL 1.1 only nudges overrides with realtime bytes (reset to 100%, ±10%,
//! ±1%), each clamped to 10-200%, and reports the result in the `Ov:` field
//! of the next status report. So a slider sets a target, and the owner
//! thread sends the fewest bytes that move the last reported value onto it,
//! then waits for the report that shows it before nudging again. Bytes sent
//! from a stale value would overshoot, and GRBL can merge repeated bytes
//! that arrive between two of its realtime checks (they set a flag), so it
//! may fall short: the next round corrects that.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Overrides;
use super::job::Tick;

/// GRBL's override limits, in percent (`config.h`).
pub const OVERRIDE_MIN: u16 = 10;
pub const OVERRIDE_MAX: u16 = 200;
pub const OVERRIDE_DEFAULT: u16 = 100;

/// How long to wait for the report showing a change before nudging again
/// from the last value known. GRBL reports a change in its next report, so
/// this only matters when a report is lost.
const OVERRIDE_TIMEOUT: Duration = Duration::from_secs(1);

/// An override a slider changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OverrideKind {
    /// Feed rate of cuts (`G1`). Rapids have their own override.
    Feed,
    /// Laser power: the spindle speed override.
    Spindle,
}

/// GRBL's realtime override bytes for one override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverrideBytes {
    pub reset: u8,
    pub coarse_up: u8,
    pub coarse_down: u8,
    pub fine_up: u8,
    pub fine_down: u8,
}

/// GRBL 1.1's bytes: feed 0x90-0x94, spindle 0x99-0x9D.
pub fn grbl_override_bytes(kind: OverrideKind) -> OverrideBytes {
    match kind {
        OverrideKind::Feed => OverrideBytes {
            reset: 0x90,
            coarse_up: 0x91,
            coarse_down: 0x92,
            fine_up: 0x93,
            fine_down: 0x94,
        },
        OverrideKind::Spindle => OverrideBytes {
            reset: 0x99,
            coarse_up: 0x9A,
            coarse_down: 0x9B,
            fine_up: 0x9C,
            fine_down: 0x9D,
        },
    }
}

/// `percent` within GRBL's limits.
pub fn clamp_override(percent: u16) -> u16 {
    percent.clamp(OVERRIDE_MIN, OVERRIDE_MAX)
}

/// The fewest bytes that take an override from `from` (`None` when unknown)
/// to `to`: tens then ones, or a reset to 100% first when that is shorter
/// or the start is unknown.
pub fn override_steps(bytes: OverrideBytes, from: Option<u16>, to: u16) -> Vec<u8> {
    let to = clamp_override(to);
    // Tens first, overshooting by up to 5 when the ones back are fewer, but
    // never past a limit, where GRBL would clamp the step.
    let nudges = |from: u16| {
        let (from, to) = (i32::from(from), i32::from(to));
        let diff = to - from;
        let within =
            |value: i32| (i32::from(OVERRIDE_MIN)..=i32::from(OVERRIDE_MAX)).contains(&value);
        let tens = [diff / 10 - 1, diff / 10, diff / 10 + 1]
            .into_iter()
            .filter(|tens| within(from + 10 * tens))
            .min_by_key(|tens| tens.abs() + (diff - 10 * tens).abs())
            .unwrap_or(0);
        let ones = diff - 10 * tens;
        let coarse = if tens >= 0 {
            bytes.coarse_up
        } else {
            bytes.coarse_down
        };
        let fine = if ones >= 0 {
            bytes.fine_up
        } else {
            bytes.fine_down
        };
        let mut out = vec![coarse; tens.unsigned_abs() as usize];
        out.extend(std::iter::repeat_n(fine, ones.unsigned_abs() as usize));
        out
    };
    let mut reset = vec![bytes.reset];
    reset.extend(nudges(OVERRIDE_DEFAULT));
    match from {
        Some(from) => {
            let direct = nudges(clamp_override(from));
            if direct.len() <= reset.len() {
                direct
            } else {
                reset
            }
        }
        None => reset,
    }
}

fn reported(overrides: &Overrides, kind: OverrideKind) -> u16 {
    match kind {
        OverrideKind::Feed => overrides.feed,
        OverrideKind::Spindle => overrides.spindle,
    }
}

/// The owner thread's override targets: what the sliders asked for, and
/// whether bytes are on their way and not yet reported.
#[derive(Debug, Default)]
pub(super) struct OverrideTargets {
    feed: Option<u16>,
    spindle: Option<u16>,
    /// Polls sent, and when, as the last bytes went out.
    waiting: Option<(u64, Duration)>,
}

impl OverrideTargets {
    pub fn set(&mut self, kind: OverrideKind, percent: u16) {
        let target = Some(clamp_override(percent));
        match kind {
            OverrideKind::Feed => self.feed = target,
            OverrideKind::Spindle => self.spindle = target,
        }
    }

    /// A status report arrived; `fresh` is its own `Ov:` field, if it had
    /// one. A report polled after the bytes went out that carries `Ov:`
    /// shows them.
    pub fn on_report(&mut self, fresh: Option<Overrides>, tick: Tick) {
        if fresh.is_some() && self.waiting.is_some_and(|(mark, _)| tick.reports > mark) {
            self.waiting = None;
        }
    }

    /// The controller reset, which puts every override back to 100%.
    /// Targets set before it no longer apply.
    pub fn on_reset(&mut self) {
        *self = OverrideTargets::default();
    }

    /// The bytes to send now to move `current` towards the targets. Targets
    /// already reached are dropped, so a later change from elsewhere (a
    /// reset) isn't fought.
    pub fn next(
        &mut self,
        current: Option<Overrides>,
        tick: Tick,
        bytes: impl Fn(OverrideKind) -> OverrideBytes,
    ) -> Vec<u8> {
        if let Some((_, since)) = self.waiting {
            if tick.now.saturating_sub(since) < OVERRIDE_TIMEOUT {
                return Vec::new();
            }
            self.waiting = None;
        }
        let mut out = Vec::new();
        for kind in [OverrideKind::Feed, OverrideKind::Spindle] {
            let target = match kind {
                OverrideKind::Feed => &mut self.feed,
                OverrideKind::Spindle => &mut self.spindle,
            };
            let Some(to) = *target else { continue };
            let from = current.as_ref().map(|o| reported(o, kind));
            if from == Some(to) {
                *target = None;
                continue;
            }
            out.extend(override_steps(bytes(kind), from, to));
        }
        if !out.is_empty() {
            self.waiting = Some((tick.polls, tick.now));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: OverrideBytes = OverrideBytes {
        reset: 0x90,
        coarse_up: 0x91,
        coarse_down: 0x92,
        fine_up: 0x93,
        fine_down: 0x94,
    };

    #[test]
    fn steps_are_the_fewest_tens_then_ones() {
        assert_eq!(
            override_steps(FEED, Some(100), 123),
            [0x91, 0x91, 0x93, 0x93, 0x93]
        );
        assert_eq!(override_steps(FEED, Some(100), 81), [0x92, 0x92, 0x93]);
        // Never past a limit, where GRBL would clamp the coarse step.
        assert_eq!(override_steps(FEED, Some(195), 198), [0x93; 3]);
        assert_eq!(override_steps(FEED, Some(15), 12), [0x94; 3]);
        assert_eq!(override_steps(FEED, Some(100), 79), [0x92, 0x92, 0x94]);
        assert_eq!(
            override_steps(FEED, Some(100), 137),
            [0x91, 0x91, 0x91, 0x91, 0x94, 0x94, 0x94]
        );
        assert_eq!(override_steps(FEED, Some(100), 96), [0x94; 4]);
        assert_eq!(override_steps(FEED, Some(57), 57), Vec::<u8>::new());
    }

    #[test]
    fn a_reset_is_used_when_it_is_shorter_or_the_start_unknown() {
        assert_eq!(override_steps(FEED, Some(187), 100), [0x90]);
        assert_eq!(override_steps(FEED, Some(190), 101), [0x90, 0x93]);
        assert_eq!(override_steps(FEED, None, 120), [0x90, 0x91, 0x91]);
    }

    #[test]
    fn targets_are_clamped_to_grbl_limits() {
        assert_eq!(override_steps(FEED, Some(20), 0), [0x92]);
        assert_eq!(override_steps(FEED, Some(190), 250), [0x91]);
    }

    #[test]
    fn grbl_bytes_match_the_realtime_command_table() {
        let spindle = grbl_override_bytes(OverrideKind::Spindle);
        assert_eq!(
            [
                spindle.reset,
                spindle.coarse_up,
                spindle.coarse_down,
                spindle.fine_up,
                spindle.fine_down
            ],
            [0x99, 0x9A, 0x9B, 0x9C, 0x9D]
        );
        assert_eq!(grbl_override_bytes(OverrideKind::Feed), FEED);
    }

    fn tick(now_ms: u64, polls: u64, reports: u64) -> Tick {
        Tick {
            now: Duration::from_millis(now_ms),
            polls,
            reports,
        }
    }

    fn ov(feed: u16, spindle: u16) -> Option<Overrides> {
        Some(Overrides {
            feed,
            rapid: 100,
            spindle,
        })
    }

    #[test]
    fn targets_wait_for_the_report_that_shows_the_last_bytes() {
        let mut targets = OverrideTargets::default();
        targets.set(OverrideKind::Feed, 120);
        assert_eq!(
            targets.next(ov(100, 100), tick(0, 5, 5), grbl_override_bytes),
            [0x91, 0x91]
        );

        // The slider moves on before GRBL reports: nothing until it does.
        targets.set(OverrideKind::Feed, 125);
        assert!(
            targets
                .next(ov(100, 100), tick(50, 5, 5), grbl_override_bytes)
                .is_empty()
        );
        // A report polled before the bytes doesn't count, nor one without Ov.
        targets.on_report(ov(100, 100), tick(60, 5, 5));
        targets.on_report(None, tick(70, 6, 6));
        assert!(
            targets
                .next(ov(100, 100), tick(70, 6, 6), grbl_override_bytes)
                .is_empty()
        );
        targets.on_report(ov(120, 100), tick(80, 6, 6));
        assert_eq!(
            targets.next(ov(120, 100), tick(80, 6, 6), grbl_override_bytes),
            [0x93; 5]
        );
        targets.on_report(ov(125, 100), tick(200, 7, 7));
        assert!(
            targets
                .next(ov(125, 100), tick(200, 7, 7), grbl_override_bytes)
                .is_empty()
        );

        // Reached targets are dropped: a reset to 100% isn't fought.
        assert!(
            targets
                .next(ov(100, 100), tick(300, 8, 8), grbl_override_bytes)
                .is_empty()
        );
    }

    #[test]
    fn a_lost_report_times_out() {
        let mut targets = OverrideTargets::default();
        targets.set(OverrideKind::Spindle, 90);
        assert_eq!(
            targets.next(ov(100, 100), tick(0, 1, 1), grbl_override_bytes),
            [0x9B]
        );
        assert!(
            targets
                .next(ov(100, 100), tick(900, 9, 9), grbl_override_bytes)
                .is_empty()
        );
        assert_eq!(
            targets.next(ov(100, 100), tick(1000, 10, 10), grbl_override_bytes),
            [0x9B]
        );
    }
}
