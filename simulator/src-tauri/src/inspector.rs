//! What the protocol inspector shows: every byte across the link and how full
//! the controller's buffers were, on the simulated clock.
//!
//! The log is raw wire traffic. Pairing lines with their `ok`/`error:N` and
//! naming realtime bytes happen in the window (`src/lib/inspector.ts`), so
//! this side only has to record faithfully and stay bounded.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bf_sim::Event;
use serde::Serialize;
use ts_rs::TS;

/// Wire events kept for the window. Older ones are dropped; a window that
/// fell this far behind is told so with [`InspectorBatch::missed`].
pub const EVENT_CAPACITY: usize = 20_000;
/// Buffer samples kept: 15 s at [`SAMPLE_INTERVAL`].
pub const SAMPLE_CAPACITY: usize = 300;
/// Each sample is the fullest the buffers got over this much wall time, so a
/// short spike still shows on the graph.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub enum Wire {
    /// Bytes from the host. The last `dropped` of the bytes bound for the RX
    /// buffer (not realtime, not extended ASCII) were lost to an overflow.
    /// `lines_before` counts the line ends (`\n`, `\r`) the RX buffer
    /// accepted before these bytes since the app started, so the next one
    /// ends line `lines_before + 1`: the numbering the burn view uses.
    #[serde(rename_all = "camelCase")]
    Received {
        bytes: Vec<u8>,
        dropped: u32,
        #[ts(type = "number")]
        lines_before: u64,
    },
    /// Bytes the controller sent.
    Sent { bytes: Vec<u8> },
    /// GRBL state change, as a status report shows it.
    State { from: String, to: String },
    /// Something that happened to the simulator rather than on the wire, e.g.
    /// a client connecting or a profile being loaded.
    Note { text: String },
    /// A fault injected from the panel or a scenario, in `bf_sim`'s text
    /// form, e.g. `alarm 1`.
    Fault { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct WireEvent {
    /// Increases by one per event, across controller resets.
    #[ts(type = "number")]
    pub seq: u64,
    /// Simulated clock in milliseconds. It restarts at 0 when a profile loads.
    pub at: f64,
    pub event: Wire,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct BufferSample {
    /// Simulated clock in milliseconds at the end of the sample.
    pub at: f64,
    /// Most bytes waiting in the RX buffer during the sample.
    pub rx: u32,
    /// Most blocks queued in the planner during the sample.
    pub planner: u32,
    /// Bytes lost to RX overflow during the sample.
    pub overflowed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct InspectorBatch {
    /// Events from the requested sequence number on, oldest first.
    pub events: Vec<WireEvent>,
    /// Pass this as `since` next time.
    #[ts(type = "number")]
    pub next: u64,
    /// Events between `since` and the first one returned were discarded.
    pub missed: bool,
    pub samples: Vec<BufferSample>,
    pub rx_capacity: u32,
    pub planner_capacity: u32,
}

/// The recorded wire traffic and buffer history.
#[derive(Debug, Default)]
pub struct Inspector {
    events: VecDeque<WireEvent>,
    next: u64,
    samples: VecDeque<BufferSample>,
    window: Option<Window>,
    /// Line ends accepted into the RX buffer so far.
    lines: u64,
}

#[derive(Debug)]
struct Window {
    started: Instant,
    peak: BufferSample,
}

impl Inspector {
    /// Records the emulator's events. An overflow is folded into the bytes
    /// that caused it, so the window can tell exactly which bytes were lost.
    pub fn record(&mut self, events: impl IntoIterator<Item = Event>) {
        for event in events {
            match event {
                Event::Received { at, bytes } => {
                    let lines_before = self.lines;
                    self.lines += line_ends(buffered(&bytes));
                    self.push(
                        at,
                        Wire::Received {
                            bytes,
                            dropped: 0,
                            lines_before,
                        },
                    )
                }
                Event::Sent { at, bytes } => self.push(at, Wire::Sent { bytes }),
                Event::Fault { at, fault } => self.push(
                    at,
                    Wire::Fault {
                        text: fault.to_string(),
                    },
                ),
                Event::StateChanged { at, from, to } => self.push(
                    at,
                    Wire::State {
                        from: from.to_string(),
                        to: to.to_string(),
                    },
                ),
                Event::RxOverflow { at, dropped } => {
                    let dropped = u32::try_from(dropped).unwrap_or(u32::MAX);
                    if let Some(Wire::Received {
                        bytes,
                        dropped: total,
                        ..
                    }) = self.events.back_mut().map(|event| &mut event.event)
                    {
                        // Line ends among the lost bytes were never accepted.
                        let lost =
                            |count: u32| line_ends(buffered(bytes).rev().take(count as usize));
                        let before = lost(*total);
                        *total = total.saturating_add(dropped);
                        self.lines -= lost(*total) - before;
                    } else {
                        // bf-sim reports an overflow straight after its bytes,
                        // but don't lose one if that ever changes.
                        self.push(
                            at,
                            Wire::Received {
                                bytes: Vec::new(),
                                dropped,
                                lines_before: self.lines,
                            },
                        );
                    }
                    if let Some(window) = &mut self.window {
                        window.peak.overflowed = window.peak.overflowed.saturating_add(dropped);
                    }
                }
            }
        }
    }

    /// Line ends accepted into the RX buffer since the app started.
    pub fn lines(&self) -> u64 {
        self.lines
    }

    pub fn note(&mut self, at: Duration, text: impl Into<String>) {
        self.push(at, Wire::Note { text: text.into() });
    }

    /// Notes the buffer fill at `now`, keeping the peak of each
    /// [`SAMPLE_INTERVAL`].
    pub fn sample(&mut self, now: Instant, at: Duration, rx: usize, planner: usize) {
        let current = BufferSample {
            at: millis(at),
            rx: u32::try_from(rx).unwrap_or(u32::MAX),
            planner: u32::try_from(planner).unwrap_or(u32::MAX),
            overflowed: 0,
        };
        let window = self.window.get_or_insert(Window {
            started: now,
            peak: current,
        });
        window.peak.at = current.at;
        window.peak.rx = window.peak.rx.max(current.rx);
        window.peak.planner = window.peak.planner.max(current.planner);
        if now.saturating_duration_since(window.started) >= SAMPLE_INTERVAL {
            let peak = window.peak;
            self.window = None;
            if self.samples.len() == SAMPLE_CAPACITY {
                self.samples.pop_front();
            }
            self.samples.push_back(peak);
        }
    }

    /// Events from `since` on, and the buffer history.
    pub fn since(&self, since: u64, rx_capacity: usize, planner_capacity: usize) -> InspectorBatch {
        let first = self.events.front().map_or(self.next, |event| event.seq);
        let skip = usize::try_from(since.saturating_sub(first)).unwrap_or(usize::MAX);
        InspectorBatch {
            events: self.events.iter().skip(skip).cloned().collect(),
            next: self.next,
            missed: since < first,
            samples: self.samples.iter().copied().collect(),
            rx_capacity: u32::try_from(rx_capacity).unwrap_or(u32::MAX),
            planner_capacity: u32::try_from(planner_capacity).unwrap_or(u32::MAX),
        }
    }

    fn push(&mut self, at: Duration, event: Wire) {
        if self.events.len() == EVENT_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(WireEvent {
            seq: self.next,
            at: millis(at),
            event,
        });
        self.next += 1;
    }
}

/// Bytes bound for the RX buffer: not realtime commands (bf-sim's
/// `is_realtime`), not extended ASCII, which GRBL ignores.
fn buffered(bytes: &[u8]) -> impl DoubleEndedIterator<Item = &u8> {
    bytes
        .iter()
        .filter(|&&byte| byte < 0x80 && !matches!(byte, b'?' | b'!' | b'~' | 0x18))
}

fn line_ends<'a>(bytes: impl Iterator<Item = &'a u8>) -> u64 {
    bytes
        .filter(|&&byte| byte == b'\n' || byte == b'\r')
        .count() as u64
}

fn millis(at: Duration) -> f64 {
    at.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use bf_sim::MachineState;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn an_overflow_is_folded_into_the_bytes_that_caused_it() {
        let mut inspector = Inspector::default();
        inspector.record([
            Event::Received {
                at: MS,
                bytes: b"G1X1\n".to_vec(),
            },
            Event::RxOverflow { at: MS, dropped: 3 },
            Event::StateChanged {
                at: MS,
                from: MachineState::Idle,
                to: MachineState::Run,
            },
        ]);
        let batch = inspector.since(0, 128, 15);
        assert_eq!(
            batch.events[0].event,
            Wire::Received {
                bytes: b"G1X1\n".to_vec(),
                dropped: 3,
                lines_before: 0,
            }
        );
        // The line end was lost, so it doesn't count.
        assert_eq!(inspector.lines(), 0);
        assert_eq!(
            batch.events[1].event,
            Wire::State {
                from: "Idle".into(),
                to: "Run".into()
            }
        );
        assert_eq!(batch.events[1].at, 1.0);
        assert_eq!(batch.next, 2);
    }

    #[test]
    fn line_ends_accepted_into_the_rx_buffer_are_counted() {
        let mut inspector = Inspector::default();
        inspector.record([
            Event::Received {
                at: MS,
                bytes: b"G0\r\n?\n".to_vec(),
            },
            Event::Received {
                at: MS,
                bytes: b"G1\nG2\nG3\n".to_vec(),
            },
            Event::RxOverflow { at: MS, dropped: 2 },
            Event::RxOverflow { at: MS, dropped: 1 },
        ]);
        let before: Vec<_> = inspector
            .since(0, 128, 15)
            .events
            .iter()
            .map(|event| match event.event {
                Wire::Received { lines_before, .. } => lines_before,
                _ => panic!("{event:?}"),
            })
            .collect();
        assert_eq!(before, [0, 3]);
        // "G3\n" was lost; G1 and G2 count.
        assert_eq!(inspector.lines(), 5);
    }

    #[test]
    fn since_returns_new_events_and_reports_missed_ones() {
        let mut inspector = Inspector::default();
        for n in 0..EVENT_CAPACITY + 5 {
            inspector.note(Duration::ZERO, n.to_string());
        }
        let batch = inspector.since(EVENT_CAPACITY as u64 + 3, 128, 15);
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[0].seq, EVENT_CAPACITY as u64 + 3);
        assert!(!batch.missed);

        let behind = inspector.since(0, 128, 15);
        assert!(behind.missed);
        assert_eq!(behind.events.len(), EVENT_CAPACITY);
        assert_eq!(behind.events[0].seq, 5);
        assert!(inspector.since(behind.next, 128, 15).events.is_empty());
    }

    #[test]
    fn samples_keep_the_peak_of_each_interval() {
        let mut inspector = Inspector::default();
        let start = Instant::now();
        inspector.sample(start, MS, 10, 2);
        inspector.sample(start + MS, 2 * MS, 120, 15);
        inspector.record([Event::RxOverflow { at: MS, dropped: 4 }]);
        inspector.sample(start + SAMPLE_INTERVAL, 3 * MS, 0, 1);
        inspector.sample(start + SAMPLE_INTERVAL + MS, 4 * MS, 5, 0);
        let samples = inspector.since(0, 128, 15).samples;
        assert_eq!(
            samples,
            [BufferSample {
                at: 3.0,
                rx: 120,
                planner: 15,
                overflowed: 4
            }]
        );
    }
}
