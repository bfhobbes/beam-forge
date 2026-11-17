//! GRBL 1.1 realtime status reports (`?`), and the live [`MachineStatus`]
//! built from them.
//!
//! A report carries only some fields each time: `WCO:` every 10-30 reports,
//! `Ov:` and `A:` every 10-20, `Pn:` only while a pin is active, `Bf:` only
//! when `$10` asks for it. [`StatusTracker`] caches what a report leaves out
//! and derives whichever of the machine and work position it didn't send.
//! Positions are in the controller's report units (mm unless `$13=1`).

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Machine state, the first field of a status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "name", rename_all = "camelCase")]
#[ts(export)]
pub enum MachineState {
    Idle,
    Run,
    /// `code` 0: held, ready to resume. 1: decelerating to a hold.
    Hold {
        code: u8,
    },
    Jog,
    Alarm,
    /// `code` 0: door closed, ready to resume. 1: stopped, door ajar.
    /// 2: door opened, parking. 3: door closed, restoring from park.
    Door {
        code: u8,
    },
    Check,
    Home,
    Sleep,
}

impl MachineState {
    /// True while the machine may be moving, when status is polled faster.
    pub fn is_moving(self) -> bool {
        matches!(
            self,
            MachineState::Run
                | MachineState::Jog
                | MachineState::Home
                | MachineState::Hold { code: 1 }
                | MachineState::Door { code: 1..=3 }
        )
    }
}

/// A position on the X, Y and Z axes.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Position {
    fn plus(self, other: Position) -> Position {
        Position {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    fn minus(self, other: Position) -> Position {
        Position {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
}

/// Override percentages (`Ov:feed,rapid,spindle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Overrides {
    pub feed: u16,
    pub rapid: u16,
    pub spindle: u16,
}

/// Free planner blocks and serial receive buffer bytes (`Bf:blocks,bytes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BufferState {
    pub planner_blocks: u16,
    pub rx_bytes: u16,
}

/// Input pins reported active (`Pn:XYZPDHRS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Pins {
    pub limit_x: bool,
    pub limit_y: bool,
    pub limit_z: bool,
    pub probe: bool,
    pub door: bool,
    pub hold: bool,
    pub soft_reset: bool,
    pub cycle_start: bool,
}

/// Accessories switched on (`A:SCFM`). Diode lasers usually wire air assist
/// to flood (`M8`) or mist (`M7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Accessories {
    pub spindle_cw: bool,
    pub spindle_ccw: bool,
    pub flood: bool,
    pub mist: bool,
}

/// One status report as sent; absent fields are `None`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusReport {
    pub state: Option<MachineState>,
    pub machine_position: Option<Position>,
    pub work_position: Option<Position>,
    pub work_offset: Option<Position>,
    pub feed: Option<f64>,
    pub spindle: Option<f64>,
    pub overrides: Option<Overrides>,
    pub buffer: Option<BufferState>,
    pub pins: Option<Pins>,
    pub accessories: Option<Accessories>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusError {
    /// Not wrapped in `<` and `>`.
    NotAReport,
    UnknownState(String),
    /// A known field with a value that doesn't parse.
    BadField(String),
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatusError::NotAReport => write!(f, "not a status report"),
            StatusError::UnknownState(state) => write!(f, "unknown machine state {state:?}"),
            StatusError::BadField(field) => write!(f, "malformed status field {field:?}"),
        }
    }
}

impl std::error::Error for StatusError {}

/// True when `line` looks like a status report, well-formed or not.
pub fn is_status_report(line: &str) -> bool {
    line.starts_with('<') && line.ends_with('>')
}

/// Parses `<Idle|MPos:0.000,0.000,0.000|FS:0,0|WCO:0.000,0.000,0.000>`.
/// Fields it doesn't know (grblHAL extensions) are skipped.
pub fn parse_status(line: &str) -> Result<StatusReport, StatusError> {
    let body = line
        .trim()
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .ok_or(StatusError::NotAReport)?;
    let mut fields = body.split('|');
    let mut report = StatusReport {
        state: Some(parse_state(fields.next().unwrap_or_default())?),
        ..StatusReport::default()
    };
    for field in fields {
        let bad = || StatusError::BadField(field.to_string());
        let Some((name, value)) = field.split_once(':') else {
            continue;
        };
        match name {
            "MPos" => report.machine_position = Some(position(value).ok_or_else(bad)?),
            "WPos" => report.work_position = Some(position(value).ok_or_else(bad)?),
            "WCO" => report.work_offset = Some(position(value).ok_or_else(bad)?),
            "FS" => {
                let [feed, spindle] = numbers(value).ok_or_else(bad)?;
                report.feed = Some(feed);
                report.spindle = Some(spindle);
            }
            "F" => report.feed = Some(value.parse().map_err(|_| bad())?),
            "Ov" => {
                let [feed, rapid, spindle] = numbers(value).ok_or_else(bad)?;
                report.overrides = Some(Overrides {
                    feed,
                    rapid,
                    spindle,
                });
            }
            "Bf" => {
                let [planner_blocks, rx_bytes] = numbers(value).ok_or_else(bad)?;
                report.buffer = Some(BufferState {
                    planner_blocks,
                    rx_bytes,
                });
            }
            "Ln" => report.line = Some(value.parse().map_err(|_| bad())?),
            "Pn" => report.pins = Some(pins(value)),
            "A" => report.accessories = Some(accessories(value)),
            _ => {}
        }
    }
    Ok(report)
}

fn parse_state(text: &str) -> Result<MachineState, StatusError> {
    let unknown = || StatusError::UnknownState(text.to_string());
    let (name, code) = match text.split_once(':') {
        Some((name, code)) => (name, Some(code.parse::<u8>().map_err(|_| unknown())?)),
        None => (text, None),
    };
    Ok(match (name, code) {
        ("Idle", None) => MachineState::Idle,
        ("Run", None) => MachineState::Run,
        ("Hold", code) => MachineState::Hold {
            code: code.unwrap_or(0),
        },
        ("Jog", None) => MachineState::Jog,
        ("Alarm", None) => MachineState::Alarm,
        ("Door", code) => MachineState::Door {
            code: code.unwrap_or(0),
        },
        ("Check", None) => MachineState::Check,
        ("Home", None) => MachineState::Home,
        ("Sleep", None) => MachineState::Sleep,
        _ => return Err(unknown()),
    })
}

/// Exactly `N` comma-separated numbers.
fn numbers<T: std::str::FromStr, const N: usize>(value: &str) -> Option<[T; N]> {
    let parts: Vec<T> = value
        .split(',')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// X, Y and Z; extra axes (grblHAL's A, B, C) are ignored.
fn position(value: &str) -> Option<Position> {
    let mut axes = value.split(',').map(|axis| axis.parse::<f64>().ok());
    let mut next = || axes.next().flatten();
    Some(Position {
        x: next()?,
        y: next()?,
        z: next()?,
    })
}

fn pins(value: &str) -> Pins {
    let has = |c| value.contains(c);
    Pins {
        limit_x: has('X'),
        limit_y: has('Y'),
        limit_z: has('Z'),
        probe: has('P'),
        door: has('D'),
        hold: has('H'),
        soft_reset: has('R'),
        cycle_start: has('S'),
    }
}

fn accessories(value: &str) -> Accessories {
    let has = |c| value.contains(c);
    Accessories {
        spindle_cw: has('S'),
        spindle_ccw: has('C'),
        flood: has('F'),
        mist: has('M'),
    }
}

/// The machine as last reported, with fields a report left out filled in
/// from earlier ones. Emitted to the UI on every report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MachineStatus {
    pub state: MachineState,
    /// `None` until a `WCO:` arrives when the controller reports `WPos`.
    pub machine_position: Option<Position>,
    /// `None` until a `WCO:` arrives when the controller reports `MPos`.
    pub work_position: Option<Position>,
    pub work_offset: Option<Position>,
    /// Current feed rate, in report units per minute.
    pub feed: f64,
    /// Spindle speed or laser power (`S`), when the controller reports it.
    pub spindle: Option<f64>,
    pub overrides: Option<Overrides>,
    /// `None` unless `$10` includes buffer state.
    pub buffer: Option<BufferState>,
    pub pins: Pins,
    /// `None` until the first `Ov:` report, which `A:` accompanies.
    pub accessories: Option<Accessories>,
    /// The executing line number, when GRBL is built to report it.
    pub line: Option<u32>,
}

/// Turns a stream of partial reports into full [`MachineStatus`]es.
#[derive(Debug, Default)]
pub struct StatusTracker {
    work_offset: Option<Position>,
    overrides: Option<Overrides>,
    accessories: Option<Accessories>,
    last: Option<MachineStatus>,
}

impl StatusTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The most recent status, if any report has arrived.
    pub fn last(&self) -> Option<&MachineStatus> {
        self.last.as_ref()
    }

    pub fn update(&mut self, report: StatusReport) -> MachineStatus {
        if let Some(offset) = report.work_offset {
            self.work_offset = Some(offset);
        }
        if let Some(overrides) = report.overrides {
            self.overrides = Some(overrides);
            // GRBL sends `A:` with `Ov:` only while something is on.
            self.accessories = Some(report.accessories.unwrap_or_default());
        } else if let Some(accessories) = report.accessories {
            self.accessories = Some(accessories);
        }
        let previous = self.last.as_ref();
        let (machine_position, work_position) =
            match (report.machine_position, report.work_position) {
                (Some(mpos), wpos) => (
                    Some(mpos),
                    wpos.or(self.work_offset.map(|wco| mpos.minus(wco))),
                ),
                (None, Some(wpos)) => (
                    Some(wpos).zip(self.work_offset).map(|(w, o)| w.plus(o)),
                    Some(wpos),
                ),
                (None, None) => (
                    previous.and_then(|p| p.machine_position),
                    previous.and_then(|p| p.work_position),
                ),
            };
        let status = MachineStatus {
            state: report
                .state
                .or(previous.map(|p| p.state))
                .unwrap_or(MachineState::Idle),
            machine_position,
            work_position,
            work_offset: self.work_offset,
            feed: report.feed.unwrap_or(0.0),
            spindle: report.spindle,
            overrides: self.overrides,
            buffer: report.buffer,
            pins: report.pins.unwrap_or_default(),
            accessories: self.accessories,
            line: report.line,
        };
        self.last = Some(status.clone());
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f64, y: f64, z: f64) -> Position {
        Position { x, y, z }
    }

    #[test]
    fn parses_every_machine_state() {
        let cases = [
            ("Idle", MachineState::Idle),
            ("Run", MachineState::Run),
            ("Hold:0", MachineState::Hold { code: 0 }),
            ("Hold:1", MachineState::Hold { code: 1 }),
            ("Jog", MachineState::Jog),
            ("Alarm", MachineState::Alarm),
            ("Door:0", MachineState::Door { code: 0 }),
            ("Door:3", MachineState::Door { code: 3 }),
            ("Check", MachineState::Check),
            ("Home", MachineState::Home),
            ("Sleep", MachineState::Sleep),
        ];
        for (text, state) in cases {
            let report = parse_status(&format!("<{text}|MPos:0.000,0.000,0.000|FS:0,0>")).unwrap();
            assert_eq!(report.state, Some(state), "{text}");
        }
    }

    #[test]
    fn rejects_unknown_states_and_non_reports() {
        assert_eq!(
            parse_status("<Dancing|MPos:0,0,0>"),
            Err(StatusError::UnknownState("Dancing".into()))
        );
        assert_eq!(
            parse_status("<Idle:5|MPos:0,0,0>"),
            Err(StatusError::UnknownState("Idle:5".into()))
        );
        assert_eq!(parse_status("ok"), Err(StatusError::NotAReport));
        assert_eq!(parse_status("<Idle"), Err(StatusError::NotAReport));
        assert!(!is_status_report("[MSG:Reset to continue]"));
    }

    #[test]
    fn parses_a_full_report() {
        let report = parse_status(
            "<Run|MPos:10.500,-2.250,0.000|Bf:15,128|Ln:42|FS:1200,800|Pn:XZPDHRS|WCO:1.000,2.000,0.000|Ov:110,50,90|A:SFM>",
        )
        .unwrap();
        assert_eq!(report.state, Some(MachineState::Run));
        assert_eq!(report.machine_position, Some(pos(10.5, -2.25, 0.0)));
        assert_eq!(report.work_position, None);
        assert_eq!(report.work_offset, Some(pos(1.0, 2.0, 0.0)));
        assert_eq!(
            report.buffer,
            Some(BufferState {
                planner_blocks: 15,
                rx_bytes: 128
            })
        );
        assert_eq!(report.line, Some(42));
        assert_eq!((report.feed, report.spindle), (Some(1200.0), Some(800.0)));
        assert_eq!(
            report.pins,
            Some(Pins {
                limit_x: true,
                limit_y: false,
                limit_z: true,
                probe: true,
                door: true,
                hold: true,
                soft_reset: true,
                cycle_start: true,
            })
        );
        assert_eq!(
            report.overrides,
            Some(Overrides {
                feed: 110,
                rapid: 50,
                spindle: 90
            })
        );
        assert_eq!(
            report.accessories,
            Some(Accessories {
                spindle_cw: true,
                spindle_ccw: false,
                flood: true,
                mist: true,
            })
        );
    }

    #[test]
    fn parses_wpos_feed_only_and_counter_clockwise_spindle() {
        let report = parse_status("<Jog|WPos:-1.5,3,0.25|F:500|A:C>").unwrap();
        assert_eq!(report.work_position, Some(pos(-1.5, 3.0, 0.25)));
        assert_eq!(report.machine_position, None);
        assert_eq!((report.feed, report.spindle), (Some(500.0), None));
        assert!(report.accessories.unwrap().spindle_ccw);
    }

    #[test]
    fn partial_report_leaves_missing_fields_empty() {
        let report = parse_status("<Idle|MPos:0.000,0.000,0.000|FS:0,0>").unwrap();
        assert_eq!(
            report,
            StatusReport {
                state: Some(MachineState::Idle),
                machine_position: Some(Position::default()),
                feed: Some(0.0),
                spindle: Some(0.0),
                ..StatusReport::default()
            }
        );
        // The bare minimum GRBL can send with $10 masking everything else.
        let bare = parse_status("<Alarm>").unwrap();
        assert_eq!(bare.state, Some(MachineState::Alarm));
        assert_eq!(bare.machine_position, None);
    }

    #[test]
    fn ignores_extra_axes_and_unknown_fields() {
        let report = parse_status("<Idle|MPos:1,2,3,4|FS:0,0|SD:1,0|H:1>").unwrap();
        assert_eq!(report.machine_position, Some(pos(1.0, 2.0, 3.0)));
    }

    #[test]
    fn rejects_malformed_known_fields() {
        for line in [
            "<Idle|MPos:1,2>",
            "<Idle|MPos:a,b,c>",
            "<Idle|FS:100>",
            "<Idle|Ov:100,100>",
            "<Idle|Bf:15>",
            "<Idle|Ln:x>",
        ] {
            assert!(
                matches!(parse_status(line), Err(StatusError::BadField(_))),
                "{line}"
            );
        }
    }

    /// Captured from a GRBL 1.1h controller with `$10=1`.
    const CAPTURED: &[&str] = &[
        "<Idle|MPos:0.000,0.000,0.000|FS:0,0|WCO:-10.000,-5.000,0.000>",
        "<Idle|MPos:0.000,0.000,0.000|FS:0,0|Ov:100,100,100>",
        "<Run|MPos:2.400,0.000,0.000|FS:600,0>",
        "<Run|MPos:6.150,0.000,0.000|FS:600,1000|Ov:100,100,100|A:S>",
        "<Hold:1|MPos:8.010,0.000,0.000|FS:240,1000>",
        "<Hold:0|MPos:8.200,0.000,0.000|FS:0,1000|Pn:H>",
    ];

    #[test]
    fn tracks_captured_reports() {
        let mut tracker = StatusTracker::new();
        let statuses: Vec<_> = CAPTURED
            .iter()
            .map(|line| tracker.update(parse_status(line).unwrap()))
            .collect();

        // WCO from the first report derives work positions for later ones.
        assert_eq!(statuses[0].work_position, Some(pos(10.0, 5.0, 0.0)));
        assert_eq!(statuses[2].work_position, Some(pos(12.4, 5.0, 0.0)));
        assert_eq!(statuses[2].work_offset, Some(pos(-10.0, -5.0, 0.0)));
        // Ov without A means every accessory is off; A then switches one on.
        assert_eq!(statuses[1].accessories, Some(Accessories::default()));
        assert!(statuses[3].accessories.unwrap().spindle_cw);
        assert!(statuses[4].accessories.unwrap().spindle_cw, "cached");
        assert_eq!(statuses[4].overrides.unwrap().feed, 100);
        assert_eq!(statuses[4].state, MachineState::Hold { code: 1 });
        assert_eq!(statuses[4].spindle, Some(1000.0));
        // Pn is sent whenever a pin is active, so its absence means none are.
        assert!(statuses[5].pins.hold);
        assert_eq!(statuses[4].pins, Pins::default());
    }

    #[test]
    fn derives_machine_position_from_work_position() {
        let mut tracker = StatusTracker::new();
        let before = tracker.update(parse_status("<Idle|WPos:1.000,2.000,0.000|FS:0,0>").unwrap());
        assert_eq!(before.machine_position, None, "no WCO yet");
        assert_eq!(before.accessories, None);
        assert_eq!(before.overrides, None);

        let status = tracker.update(
            parse_status("<Idle|WPos:1.000,2.000,0.000|FS:0,0|WCO:5.000,5.000,-1.000>").unwrap(),
        );
        assert_eq!(status.machine_position, Some(pos(6.0, 7.0, -1.0)));
        assert_eq!(status.work_position, Some(pos(1.0, 2.0, 0.0)));
    }

    #[test]
    fn report_without_a_position_keeps_the_last_one() {
        let mut tracker = StatusTracker::new();
        tracker.update(parse_status("<Idle|MPos:3,4,0|WCO:1,1,0>").unwrap());
        let status = tracker.update(parse_status("<Alarm>").unwrap());
        assert_eq!(status.state, MachineState::Alarm);
        assert_eq!(status.machine_position, Some(pos(3.0, 4.0, 0.0)));
        assert_eq!(status.work_position, Some(pos(2.0, 3.0, 0.0)));
        assert_eq!(status.feed, 0.0);
        assert_eq!(tracker.last(), Some(&status));
    }

    #[test]
    fn moving_states_poll_faster() {
        assert!(MachineState::Run.is_moving());
        assert!(MachineState::Hold { code: 1 }.is_moving());
        assert!(!MachineState::Hold { code: 0 }.is_moving());
        assert!(!MachineState::Door { code: 0 }.is_moving());
        assert!(!MachineState::Idle.is_moving());
    }

    #[test]
    fn status_serializes_for_the_ui() {
        let mut tracker = StatusTracker::new();
        let status = tracker.update(parse_status("<Hold:0|MPos:0,0,0|FS:0,0>").unwrap());
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json["state"],
            serde_json::json!({ "name": "hold", "code": 0 })
        );
        assert_eq!(json["machinePosition"]["x"], 0.0);
        assert_eq!(json["pins"]["limitX"], false);
    }
}
