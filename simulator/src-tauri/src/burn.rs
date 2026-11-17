//! What the burn view draws: every piece of executed motion and every
//! stationary burn, from `bf-sim`'s trace, on one app-wide clock and line
//! numbering.
//!
//! Loading a profile replaces the emulator, whose clock and line count start
//! again at zero. The bed keeps its marks, as real material would, so this
//! store shifts each emulator's times and line numbers to carry on from the
//! last one. Drawing, styling and playback happen in the window
//! (`src/lib/burn.ts`).

use std::time::Duration;

use bf_sim::{LaserMode, TraceSegment};
use serde::Serialize;
use ts_rs::TS;

/// Segments kept. A 10k-line job at 10 ms speed steps stays well inside
/// this; past it, new segments are dropped and the batch says so.
pub const SEGMENT_CAPACITY: usize = 500_000;
/// Most segments one [`Burns::since`] call returns, to keep IPC replies small.
pub const BATCH_LIMIT: usize = 20_000;

/// How a segment's laser power was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub enum Laser {
    /// `M5`.
    Off,
    /// `M3`, or `M4` without laser mode.
    Constant,
    /// `M4` in laser mode: power follows speed.
    Dynamic,
}

impl From<LaserMode> for Laser {
    fn from(mode: LaserMode) -> Self {
        match mode {
            LaserMode::Off => Laser::Off,
            LaserMode::Constant => Laser::Constant,
            LaserMode::Dynamic => Laser::Dynamic,
        }
    }
}

/// One trace segment in bed coordinates (machine X and Y, mm).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct BurnSegment {
    pub start: [f64; 2],
    pub end: [f64; 2],
    /// App clock in milliseconds: simulated time, carried across profile loads.
    pub t0: f64,
    pub t1: f64,
    /// Speeds in mm/min; the head accelerates evenly between them.
    pub start_speed: f64,
    pub end_speed: f64,
    /// Programmed feed in mm/min (the rapid rate for `G0`).
    pub feed: f64,
    /// Laser output as a fraction of full power, 0 to 1.
    pub power: f64,
    pub laser: Laser,
    pub air_assist: bool,
    /// `G0` motion.
    pub rapid: bool,
    /// Laser on with the head still, e.g. `M3` during a dwell.
    pub stationary: bool,
    /// The line that queued it, numbered like the protocol inspector's
    /// `linesBefore`: line ends accepted since the app started.
    #[ts(type = "number")]
    pub line: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct BurnBatch {
    /// Changes when the bed is cleared; segment indexes restart at 0.
    pub generation: u32,
    /// Segments from the requested index on, at most [`BATCH_LIMIT`].
    pub segments: Vec<BurnSegment>,
    /// Pass this as `since` next time.
    pub next: u32,
    /// Segments recorded in this generation; more than `next` means another
    /// batch is waiting.
    pub total: u32,
    /// The store filled up and later motion was not recorded.
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct Burns {
    segments: Vec<BurnSegment>,
    generation: u32,
    truncated: bool,
    /// Added to the current emulator's clock and line numbers.
    time_base: Duration,
    line_base: u64,
}

impl Burns {
    /// Adds the current emulator's trace.
    pub fn record(&mut self, trace: Vec<TraceSegment>) {
        for segment in trace {
            if self.segments.len() == SEGMENT_CAPACITY {
                self.truncated = true;
                return;
            }
            let stationary = segment.is_stationary();
            self.segments.push(BurnSegment {
                start: [segment.start[0], segment.start[1]],
                end: [segment.end[0], segment.end[1]],
                t0: millis(self.time_base + segment.t0),
                t1: millis(self.time_base + segment.t1),
                start_speed: segment.start_speed,
                end_speed: segment.end_speed,
                feed: segment.feed,
                power: segment.power,
                laser: segment.laser_mode.into(),
                air_assist: segment.air_assist,
                rapid: segment.rapid,
                stationary,
                line: self.line_base + segment.line,
            });
        }
    }

    /// A new emulator takes over: its clock and line count start at zero.
    /// `clock` is how far the old one's clock had run; `lines` the app-wide
    /// line ends so far.
    pub fn restart(&mut self, clock: Duration, lines: u64) {
        self.time_base += clock;
        self.line_base = lines;
    }

    /// Empties the bed.
    pub fn clear(&mut self) {
        self.segments.clear();
        self.truncated = false;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn since(&self, since: u32) -> BurnBatch {
        let start = (since as usize).min(self.segments.len());
        let end = (start + BATCH_LIMIT).min(self.segments.len());
        BurnBatch {
            generation: self.generation,
            segments: self.segments[start..end].to_vec(),
            next: index(end),
            total: index(self.segments.len()),
            truncated: self.truncated,
        }
    }
}

fn index(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn millis(at: Duration) -> f64 {
    at.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(t0: u64, t1: u64, line: u64) -> TraceSegment {
        TraceSegment {
            start: [0.0, 0.0, 0.0],
            end: [1.0, 2.0, 0.0],
            t0: Duration::from_millis(t0),
            t1: Duration::from_millis(t1),
            feed: 600.0,
            start_speed: 0.0,
            end_speed: 600.0,
            power: 0.5,
            laser_mode: LaserMode::Dynamic,
            air_assist: true,
            rapid: false,
            line,
        }
    }

    #[test]
    fn a_new_emulator_carries_on_the_clock_and_line_numbers() {
        let mut burns = Burns::default();
        burns.record(vec![segment(0, 10, 3)]);
        burns.restart(Duration::from_millis(50), 7);
        burns.record(vec![segment(5, 20, 2)]);
        let batch = burns.since(0);
        let second = &batch.segments[1];
        assert_eq!((second.t0, second.t1, second.line), (55.0, 70.0, 9));
        assert_eq!(second.end, [1.0, 2.0]);
        assert_eq!(second.laser, Laser::Dynamic);
        assert!(!second.stationary);
        assert_eq!((batch.next, batch.total), (2, 2));
    }

    #[test]
    fn batches_are_bounded_and_clear_starts_a_new_generation() {
        let mut burns = Burns::default();
        burns.record(
            (0..BATCH_LIMIT as u64 + 5)
                .map(|n| segment(n, n + 1, n))
                .collect(),
        );
        let first = burns.since(0);
        assert_eq!(first.segments.len(), BATCH_LIMIT);
        assert_eq!(first.total, BATCH_LIMIT as u32 + 5);
        let rest = burns.since(first.next);
        assert_eq!(rest.segments.len(), 5);
        assert!(burns.since(rest.next).segments.is_empty());

        burns.clear();
        let cleared = burns.since(rest.next);
        assert_ne!(cleared.generation, first.generation);
        assert_eq!((cleared.total, cleared.next), (0, 0));
    }

    #[test]
    fn a_full_store_drops_new_segments_and_says_so() {
        let mut burns = Burns::default();
        burns.record(vec![segment(0, 1, 1); SEGMENT_CAPACITY + 1]);
        let batch = burns.since(u32::MAX);
        assert_eq!(batch.total, SEGMENT_CAPACITY as u32);
        assert!(batch.truncated);
    }
}
