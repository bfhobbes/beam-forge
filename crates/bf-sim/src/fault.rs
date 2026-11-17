//! Faults the simulator can inject, and scripted scenarios of them.
//!
//! Each fault has a one-line text form, which is both its [`fmt::Display`]
//! and what [`str::parse`] reads, so a scenario reads the same in a test, a
//! fixture file and the simulator window's fault panel:
//!
//! ```text
//! # Comments start with '#'. Lines without "after" fire straight away.
//! latency 20ms
//! after 3: alarm 1         # once the 3rd line from now has been answered
//! after 5: error 20 line 2 # answer the 2nd line after that with error:20
//! ```
//!
//! | Text | Fault |
//! |------|-------|
//! | `alarm <1-9>` | raise `ALARM:n` |
//! | `error <1-38> [line <k>]` (no 18, 19) | answer the k-th line from now (default 1, the next) with `error:n` |
//! | `door open` / `door closed` | open or close the safety door switch |
//! | `hold` | press the feed hold button |
//! | `disconnect` | pull the USB cable |
//! | `stall <time>` | stop reading lines for a while (`stall 0` ends a stall) |
//! | `latency <time>` | take this much longer to answer every line (`latency 0` turns it off) |
//! | `rx <bytes>` / `rx default` | shrink the RX buffer, or restore the profile's |
//!
//! Times are whole milliseconds or seconds: `250ms`, `2s`.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Highest GRBL 1.1 alarm code (`ALARM:9`, homing fail: no switch found).
pub const MAX_ALARM: u8 = 9;
/// Highest GRBL 1.1 error code (`error:38`, tool number too large).
pub const MAX_ERROR: u8 = 38;

/// Something going wrong with the controller, its switches or its cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fault {
    /// Raises `ALARM:n` the way GRBL raises it:
    /// - 1 (hard limit) stops motion dead; 2 (soft limit) brings a running
    ///   cycle to a controlled stop first. Both turn the laser and air off,
    ///   print `[MSG:Reset to continue]` and ignore everything but a reset.
    /// - 3-9 reset the controller (`mc_reset`): motion stops, buffers are
    ///   flushed, the laser and air go off, and the banner follows in alarm.
    ///   GRBL raises 4 and 5 only while probing, which the simulator does not
    ///   model, so they reset like the others.
    Alarm(u8),
    /// Answers a line with `error:code` instead of running it. `line` counts
    /// lines read from now on: 1 is the next line.
    Error { code: u8, line: u64 },
    /// Opens or closes the safety door switch.
    Door { open: bool },
    /// The feed hold button: the same as the host sending `!`.
    FeedHold,
    /// The USB cable is pulled. The controller keeps running whatever it has
    /// buffered; its output is lost and host bytes never arrive until the
    /// next power-up (a new connection). Hosts watch
    /// [`Simulator::is_disconnected`](crate::Simulator::is_disconnected).
    Disconnect,
    /// The controller stops reading lines, so nothing is acknowledged and
    /// the RX buffer fills, for this long. Running motion and realtime
    /// commands (status reports included) carry on. Zero ends a stall.
    Stall(Duration),
    /// Every line waits this much longer before it is run and answered.
    /// Zero turns it off.
    AckLatency(Duration),
    /// Limits the RX buffer to this many bytes, so a host that assumes the
    /// usual size overflows it. `$I` still reports the profile's size; `Bf:`
    /// reports the space really left. `None` restores the profile's size.
    RxBuffer(Option<usize>),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Alarm(code) => write!(f, "alarm {code}"),
            Fault::Error { code, line: 1 } => write!(f, "error {code}"),
            Fault::Error { code, line } => write!(f, "error {code} line {line}"),
            Fault::Door { open: true } => write!(f, "door open"),
            Fault::Door { open: false } => write!(f, "door closed"),
            Fault::FeedHold => write!(f, "hold"),
            Fault::Disconnect => write!(f, "disconnect"),
            Fault::Stall(time) => write!(f, "stall {}", duration(*time)),
            Fault::AckLatency(time) => write!(f, "latency {}", duration(*time)),
            Fault::RxBuffer(Some(bytes)) => write!(f, "rx {bytes}"),
            Fault::RxBuffer(None) => write!(f, "rx default"),
        }
    }
}

fn duration(time: Duration) -> String {
    let ms = time.as_millis();
    if ms > 0 && ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

impl FromStr for Fault {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let fault = match words.as_slice() {
            ["alarm", code] => {
                let code = number(code)?;
                if !(1..=u64::from(MAX_ALARM)).contains(&code) {
                    return Err(format!("GRBL 1.1 alarms are 1 to {MAX_ALARM}, not {code}"));
                }
                Fault::Alarm(code as u8)
            }
            ["error", code, rest @ ..] => {
                let code = number(code)?;
                if !(1..=u64::from(MAX_ERROR)).contains(&code) {
                    return Err(format!("GRBL 1.1 errors are 1 to {MAX_ERROR}, not {code}"));
                }
                if matches!(code, 18 | 19) {
                    return Err(format!("GRBL 1.1 has no error:{code}"));
                }
                let line = match rest {
                    [] => 1,
                    ["line", line] => match number(line)? {
                        0 => return Err("line counts from 1, the next line".into()),
                        line => line,
                    },
                    _ => return Err(format!("expected \"error {code} [line <n>]\"")),
                };
                Fault::Error {
                    code: code as u8,
                    line,
                }
            }
            ["door", "open"] => Fault::Door { open: true },
            ["door", "closed" | "close"] => Fault::Door { open: false },
            ["hold"] => Fault::FeedHold,
            ["disconnect"] => Fault::Disconnect,
            ["stall", time] => Fault::Stall(parse_duration(time)?),
            ["latency", time] => Fault::AckLatency(parse_duration(time)?),
            ["rx", "default"] => Fault::RxBuffer(None),
            ["rx", bytes] => Fault::RxBuffer(Some(number(bytes)? as usize)),
            [] => return Err("missing fault".into()),
            _ => return Err(format!("unknown fault \"{}\"", words.join(" "))),
        };
        Ok(fault)
    }
}

fn number(text: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_| format!("\"{text}\" isn't a whole number"))
}

/// `250ms`, `2s`, or a bare `0`.
fn parse_duration(text: &str) -> Result<Duration, String> {
    if let Some(ms) = text.strip_suffix("ms") {
        Ok(Duration::from_millis(number(ms)?))
    } else if let Some(s) = text.strip_suffix('s') {
        Ok(Duration::from_secs(number(s)?))
    } else if text == "0" {
        Ok(Duration::ZERO)
    } else {
        Err(format!("\"{text}\" needs a unit, e.g. 250ms or 2s"))
    }
}

/// One step of a [`Scenario`]: once the `after`-th line since the scenario
/// started has been answered, inject `fault`. `after: 0` fires at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioStep {
    pub after: u64,
    pub fault: Fault,
}

/// A script of faults keyed to the host's lines: "after line N, do fault X".
/// Run it with [`Simulator::run_scenario`](crate::Simulator::run_scenario).
/// Lines count from when it starts, so start it just before the job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    /// In the order they fire.
    pub steps: Vec<ScenarioStep>,
}

impl Scenario {
    pub fn new(steps: impl IntoIterator<Item = ScenarioStep>) -> Self {
        let mut steps: Vec<_> = steps.into_iter().collect();
        // Stable, so steps after the same line fire in the order written.
        steps.sort_by_key(|step| step.after);
        Scenario { steps }
    }
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for step in &self.steps {
            writeln!(f, "after {}: {}", step.after, step.fault)?;
        }
        Ok(())
    }
}

/// A scenario line that didn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioError {
    /// 1-based.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScenarioError {}

impl FromStr for Scenario {
    type Err = ScenarioError;

    fn from_str(text: &str) -> Result<Self, ScenarioError> {
        let mut steps = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let error = |message: String| ScenarioError {
                line: index + 1,
                message,
            };
            let (after, fault) = match line.strip_prefix("after ") {
                Some(rest) => {
                    let (after, fault) = rest
                        .split_once(':')
                        .ok_or_else(|| error("expected \"after <line>: <fault>\"".into()))?;
                    (number(after.trim()).map_err(error)?, fault)
                }
                None => (0, line),
            };
            let fault = fault.parse().map_err(error)?;
            steps.push(ScenarioStep { after, fault });
        }
        Ok(Scenario::new(steps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_round_trip_through_text() {
        let faults = [
            Fault::Alarm(1),
            Fault::Error { code: 20, line: 1 },
            Fault::Error { code: 9, line: 3 },
            Fault::Door { open: true },
            Fault::Door { open: false },
            Fault::FeedHold,
            Fault::Disconnect,
            Fault::Stall(Duration::from_secs(2)),
            Fault::Stall(Duration::ZERO),
            Fault::AckLatency(Duration::from_millis(250)),
            Fault::RxBuffer(Some(64)),
            Fault::RxBuffer(None),
        ];
        for fault in faults {
            assert_eq!(fault.to_string().parse(), Ok(fault), "{fault}");
        }
    }

    #[test]
    fn bad_faults_say_why() {
        assert_eq!(
            "alarm 10".parse::<Fault>(),
            Err("GRBL 1.1 alarms are 1 to 9, not 10".into())
        );
        assert_eq!(
            "error 0".parse::<Fault>(),
            Err("GRBL 1.1 errors are 1 to 38, not 0".into())
        );
        assert_eq!(
            "error 18".parse::<Fault>(),
            Err("GRBL 1.1 has no error:18".into())
        );
        assert_eq!(
            "stall 5".parse::<Fault>(),
            Err("\"5\" needs a unit, e.g. 250ms or 2s".into())
        );
        assert_eq!(
            "explode".parse::<Fault>(),
            Err("unknown fault \"explode\"".into())
        );
    }

    #[test]
    fn scenarios_parse_with_comments_and_keep_order() {
        let scenario: Scenario = "# a job that goes wrong\n\
                                  after 3: alarm 1\n\
                                  \n\
                                  latency 20ms  # from the start\n\
                                  after 3: disconnect\n"
            .parse()
            .unwrap();
        assert_eq!(
            scenario.steps,
            [
                ScenarioStep {
                    after: 0,
                    fault: Fault::AckLatency(Duration::from_millis(20))
                },
                ScenarioStep {
                    after: 3,
                    fault: Fault::Alarm(1)
                },
                ScenarioStep {
                    after: 3,
                    fault: Fault::Disconnect
                },
            ]
        );
        assert_eq!(scenario.to_string().parse(), Ok(scenario));
        assert_eq!(
            "hold\nafter x: hold".parse::<Scenario>(),
            Err(ScenarioError {
                line: 2,
                message: "\"x\" isn't a whole number".into()
            })
        );
    }
}
