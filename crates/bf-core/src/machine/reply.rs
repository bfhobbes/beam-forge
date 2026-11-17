//! What a GRBL 1.1 controller line means: `ok`, a decoded `error:N` or
//! `ALARM:N`, a `[MSG:...]` or other `[...]` feedback message.
//!
//! The code tables follow gnea/grbl's `doc/csv/error_codes_en_US.csv` and
//! `alarm_codes_en_US.csv` (GRBL 1.1h). grblHAL keeps these codes and adds
//! more, which decode as unknown until it gets its own dialect.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One line from the controller, classified for the console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum Reply {
    /// `ok`: the last command line was accepted.
    Ok,
    /// `error:N`: the last command line was rejected.
    Error(ReplyCode),
    /// `ALARM:N`: the controller locked up until reset or unlock.
    Alarm(ReplyCode),
    /// `[MSG:text]`, a message for the user.
    Message { text: String },
    /// Any other `[...]` push message, such as `[VER:...]` or `[GC:...]`.
    Feedback { text: String },
    /// Anything else: the banner, `$N=value` settings, `$` help.
    Other,
}

/// A decoded error or alarm code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplyCode {
    pub code: u16,
    /// Short name, e.g. `Unsupported command`.
    pub title: String,
    /// What went wrong and what to do about it.
    pub description: String,
}

impl ReplyCode {
    fn decode(code: u16, table: &Table, kind: &str) -> ReplyCode {
        let (title, description) = match lookup(code, table) {
            Some((title, description)) => (title.to_string(), description.to_string()),
            None => (
                "Unknown".to_string(),
                format!("Unknown {kind} code {code}. GRBL 1.1 doesn't define it."),
            ),
        };
        ReplyCode {
            code,
            title,
            description,
        }
    }
}

type Table = [(u16, &'static str, &'static str)];

fn lookup(code: u16, table: &Table) -> Option<(&'static str, &'static str)> {
    table
        .iter()
        .find(|(n, ..)| *n == code)
        .map(|(_, title, description)| (*title, *description))
}

/// Title and description of GRBL `error:code`, if GRBL 1.1 defines it.
pub fn error_text(code: u16) -> Option<(&'static str, &'static str)> {
    lookup(code, ERRORS)
}

/// Title and description of GRBL `ALARM:code`, if GRBL 1.1 defines it.
pub fn alarm_text(code: u16) -> Option<(&'static str, &'static str)> {
    lookup(code, ALARMS)
}

/// Classifies one line from a GRBL 1.1 controller.
pub fn decode_reply(line: &str) -> Reply {
    let line = line.trim();
    if line == "ok" {
        return Reply::Ok;
    }
    if let Some(code) = line.strip_prefix("error:").and_then(|n| n.parse().ok()) {
        return Reply::Error(ReplyCode::decode(code, ERRORS, "error"));
    }
    if let Some(code) = line.strip_prefix("ALARM:").and_then(|n| n.parse().ok()) {
        return Reply::Alarm(ReplyCode::decode(code, ALARMS, "alarm"));
    }
    if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
        return match inner.strip_prefix("MSG:") {
            Some(text) => Reply::Message {
                text: text.to_string(),
            },
            None => Reply::Feedback {
                text: inner.to_string(),
            },
        };
    }
    Reply::Other
}

const ERRORS: &Table = &[
    (
        1,
        "Expected command letter",
        "G-code words consist of a letter and a value. Letter was not found.",
    ),
    (
        2,
        "Bad number format",
        "Missing the expected G-code word value or numeric value format is not valid.",
    ),
    (
        3,
        "Invalid statement",
        "Grbl '$' system command was not recognized or supported.",
    ),
    (
        4,
        "Value < 0",
        "Negative value received for an expected positive value.",
    ),
    (
        5,
        "Setting disabled",
        "Homing cycle failure. Homing is not enabled via settings.",
    ),
    (
        6,
        "Value < 3 µs",
        "Minimum step pulse time must be greater than 3 µs.",
    ),
    (
        7,
        "EEPROM read fail. Using defaults",
        "An EEPROM read failed. Auto-restoring affected EEPROM to default values.",
    ),
    (
        8,
        "Not idle",
        "Grbl '$' command cannot be used unless Grbl is idle. Ensures smooth operation during a job.",
    ),
    (
        9,
        "G-code lock",
        "G-code commands are locked out during alarm or jog state.",
    ),
    (
        10,
        "Homing not enabled",
        "Soft limits cannot be enabled without homing also enabled.",
    ),
    (
        11,
        "Line overflow",
        "Max characters per line exceeded. Received command line was not executed.",
    ),
    (
        12,
        "Step rate > 30 kHz",
        "Grbl '$' setting value causes the step rate to exceed the maximum supported.",
    ),
    (
        13,
        "Check door",
        "Safety door detected as opened and door state initiated.",
    ),
    (
        14,
        "Line length exceeded",
        "Build info or startup line exceeded EEPROM line length limit. Line not stored.",
    ),
    (
        15,
        "Travel exceeded",
        "Jog target exceeds machine travel. Jog command has been ignored.",
    ),
    (
        16,
        "Invalid jog command",
        "Jog command has no '=' or contains prohibited G-code.",
    ),
    (17, "Setting disabled", "Laser mode requires PWM output."),
    (
        20,
        "Unsupported command",
        "Unsupported or invalid G-code command found in block.",
    ),
    (
        21,
        "Modal group violation",
        "More than one G-code command from same modal group found in block.",
    ),
    (
        22,
        "Undefined feed rate",
        "Feed rate has not yet been set or is undefined.",
    ),
    (
        23,
        "Integer value required",
        "G-code command in block requires an integer value.",
    ),
    (
        24,
        "Axis word conflict",
        "More than one G-code command that requires axis words found in block.",
    ),
    (25, "Repeated word", "Repeated G-code word found in block."),
    (
        26,
        "Missing axis words",
        "No axis words found in block for G-code command or current modal state which requires them.",
    ),
    (27, "Invalid line number", "Line number value is invalid."),
    (
        28,
        "Missing value word",
        "G-code command is missing a required value word.",
    ),
    (
        29,
        "Unsupported coordinate system",
        "G59.x work coordinate systems are not supported.",
    ),
    (
        30,
        "Invalid G53 motion mode",
        "G53 only allowed with G0 and G1 motion modes.",
    ),
    (
        31,
        "Unused axis words",
        "Axis words found in block when no command or current modal state uses them.",
    ),
    (
        32,
        "Missing arc axis words",
        "G2 and G3 arcs require at least one in-plane axis word.",
    ),
    (33, "Invalid target", "Motion command target is invalid."),
    (34, "Invalid arc radius", "Arc radius value is invalid."),
    (
        35,
        "Missing arc offset words",
        "G2 and G3 arcs require at least one in-plane offset word.",
    ),
    (
        36,
        "Unused value words",
        "Unused value words found in block.",
    ),
    (
        37,
        "Invalid tool length offset axis",
        "G43.1 dynamic tool length offset is not assigned to configured tool length axis.",
    ),
    (
        38,
        "Tool number too large",
        "Tool number greater than max supported value.",
    ),
];

const ALARMS: &Table = &[
    (
        1,
        "Hard limit",
        "Hard limit has been triggered. Machine position is likely lost due to sudden halt. Re-homing is highly recommended.",
    ),
    (
        2,
        "Soft limit",
        "G-code motion target exceeds machine travel. Machine position retained. Alarm may be safely unlocked.",
    ),
    (
        3,
        "Abort during cycle",
        "Reset while in motion. Machine position is likely lost due to sudden halt. Re-homing is highly recommended.",
    ),
    (
        4,
        "Probe fail",
        "Probe is not in the expected initial state before starting probe cycle when G38.2 and G38.3 is not triggered and G38.4 and G38.5 is triggered.",
    ),
    (
        5,
        "Probe fail",
        "Probe did not contact the workpiece within the programmed travel for G38.2 and G38.4.",
    ),
    (6, "Homing fail", "The active homing cycle was reset."),
    (
        7,
        "Homing fail",
        "Safety door was opened during homing cycle.",
    ),
    (
        8,
        "Homing fail",
        "Pull off travel failed to clear limit switch. Try increasing pull-off setting or check wiring.",
    ),
    (
        9,
        "Homing fail",
        "Could not find limit switch within search distances. Try increasing max travel, decreasing pull-off distance, or check wiring.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_table_covers_every_grbl_1_1_error_code() {
        let codes: Vec<u16> = ERRORS.iter().map(|(code, ..)| *code).collect();
        let expected: Vec<u16> = (1..=17).chain(20..=38).collect();
        assert_eq!(codes, expected);
        assert!(
            ERRORS
                .iter()
                .all(|(_, t, d)| !t.is_empty() && !d.is_empty())
        );
    }

    #[test]
    fn alarm_table_covers_every_grbl_1_1_alarm_code() {
        let codes: Vec<u16> = ALARMS.iter().map(|(code, ..)| *code).collect();
        assert_eq!(codes, (1..=9).collect::<Vec<_>>());
        assert!(
            ALARMS
                .iter()
                .all(|(_, t, d)| !t.is_empty() && !d.is_empty())
        );
    }

    #[test]
    fn looks_up_codes_by_number() {
        assert_eq!(error_text(20).unwrap().0, "Unsupported command");
        assert_eq!(error_text(9).unwrap().0, "G-code lock");
        assert_eq!(alarm_text(2).unwrap().0, "Soft limit");
        assert_eq!(error_text(18), None);
        assert_eq!(alarm_text(10), None);
    }

    #[test]
    fn decodes_errors_and_alarms() {
        assert_eq!(
            decode_reply("error:3"),
            Reply::Error(ReplyCode {
                code: 3,
                title: "Invalid statement".into(),
                description: "Grbl '$' system command was not recognized or supported.".into(),
            })
        );
        match decode_reply("ALARM:3") {
            Reply::Alarm(code) => {
                assert_eq!(code.title, "Abort during cycle");
                assert!(code.description.contains("Re-homing"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_codes_still_decode_with_their_number() {
        match decode_reply("error:79") {
            Reply::Error(code) => {
                assert_eq!((code.code, code.title.as_str()), (79, "Unknown"));
                assert!(code.description.starts_with("Unknown error code 79."));
            }
            other => panic!("{other:?}"),
        }
        match decode_reply("ALARM:12") {
            Reply::Alarm(code) => assert!(code.description.starts_with("Unknown alarm code 12.")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn classifies_messages_and_feedback() {
        assert_eq!(decode_reply("ok"), Reply::Ok);
        assert_eq!(
            decode_reply("[MSG:'$H'|'$X' to unlock]"),
            Reply::Message {
                text: "'$H'|'$X' to unlock".into()
            }
        );
        assert_eq!(
            decode_reply("[VER:1.1h.20190825:]"),
            Reply::Feedback {
                text: "VER:1.1h.20190825:".into()
            }
        );
        for line in [
            "Grbl 1.1h ['$' for help]",
            "$0=10",
            "error:",
            "error:x",
            "okay",
        ] {
            assert_eq!(decode_reply(line), Reply::Other, "{line:?}");
        }
    }

    #[test]
    fn serializes_tagged_for_the_ui() {
        assert_eq!(
            serde_json::to_string(&decode_reply("ok")).unwrap(),
            r#"{"kind":"ok"}"#
        );
        let json = serde_json::to_value(decode_reply("error:22")).unwrap();
        assert_eq!(json["kind"], "error");
        assert_eq!(json["code"], 22);
        assert_eq!(json["title"], "Undefined feed rate");
    }
}

/// Decoding real replies from `bf-sim` through a [`Connection`].
#[cfg(test)]
mod sim_tests {
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use bf_sim::{Profile, Simulator};

    use super::*;
    use crate::machine::sim::SimLink;
    use crate::machine::{Connection, MachineEvent};

    const WAIT: Duration = Duration::from_secs(5);

    fn open() -> (Connection, Receiver<MachineEvent>) {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (events, inbox) = mpsc::channel();
        let connection = Connection::open(link.transport(), link.clock(), move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        (connection, inbox)
    }

    /// Decoded lines up to and including the first one `last` accepts.
    fn replies_until(
        connection: &Connection,
        inbox: &Receiver<MachineEvent>,
        last: impl Fn(&Reply) -> bool,
    ) -> Vec<(String, Reply)> {
        let mut replies = Vec::new();
        loop {
            match inbox.recv_timeout(WAIT).unwrap() {
                MachineEvent::Line(line) => {
                    let reply = connection.dialect().decode_reply(&line);
                    let done = last(&reply);
                    replies.push((line, reply));
                    if done {
                        return replies;
                    }
                }
                MachineEvent::Status(_) => {}
                other => panic!("expected a line, got {other:?}"),
            }
        }
    }

    #[test]
    fn build_info_arrives_as_feedback() {
        let (connection, inbox) = open();
        connection.send_line("$I").unwrap();
        let replies = replies_until(&connection, &inbox, |r| *r == Reply::Ok);
        assert!(
            replies.iter().any(|(_, reply)| matches!(
                reply,
                Reply::Feedback { text } if text.starts_with("VER:1.1h")
            )),
            "{replies:?}"
        );
    }

    #[test]
    fn invalid_commands_decode_to_error_text() {
        let (connection, inbox) = open();
        connection.send_line("G99").unwrap();
        let replies = replies_until(&connection, &inbox, |r| matches!(r, Reply::Error(_)));
        assert_eq!(replies.last().unwrap().0, "error:20");
        match &replies.last().unwrap().1 {
            Reply::Error(code) => assert_eq!(code.title, "Unsupported command"),
            other => panic!("{other:?}"),
        }

        connection.send_line("G1 X10").unwrap();
        let replies = replies_until(&connection, &inbox, |r| matches!(r, Reply::Error(_)));
        match &replies.last().unwrap().1 {
            Reply::Error(code) => assert_eq!(
                (code.code, code.title.as_str()),
                (22, "Undefined feed rate")
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reset_during_motion_decodes_the_alarm_and_unlock_hint() {
        let (connection, inbox) = open();
        connection.send_line("G1 X50 F300").unwrap();
        replies_until(&connection, &inbox, |r| *r == Reply::Ok);
        connection.soft_reset().unwrap();

        let replies = replies_until(&connection, &inbox, |r| matches!(r, Reply::Message { .. }));
        let alarm = replies
            .iter()
            .find_map(|(_, reply)| match reply {
                Reply::Alarm(code) => Some(code.clone()),
                _ => None,
            })
            .expect("an alarm");
        assert_eq!(
            (alarm.code, alarm.title.as_str()),
            (3, "Abort during cycle")
        );

        connection.send_line("$X").unwrap();
        let replies = replies_until(&connection, &inbox, |r| *r == Reply::Ok);
        assert!(
            replies.iter().any(|(_, reply)| *reply
                == Reply::Message {
                    text: "Caution: Unlocked".into()
                }),
            "{replies:?}"
        );
    }
}
