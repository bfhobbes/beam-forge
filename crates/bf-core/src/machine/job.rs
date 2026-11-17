//! Streaming a GCode job: the job's lines, its progress, and the state
//! machine the connection's owner thread drives it with.
//!
//! The owner thread sends job lines with GRBL's character-counting protocol
//! (see `connection.rs`); this module decides what the job does next as
//! replies and status reports arrive.

use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Accessories, MachineState};

/// How long a stop waits for the feed hold to bring the head to rest before
/// it resets anyway.
const HOLD_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a stop waits for the banner after the soft reset.
const RESET_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a stop waits, after the reset, for a status report.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

/// The lines of a job, read lazily so a large file never sits in memory.
pub type JobLines = Box<dyn Iterator<Item = io::Result<String>> + Send>;

/// A GCode job to stream.
pub struct Job {
    /// Shown in the UI, e.g. the file name.
    pub name: String,
    pub lines: JobLines,
    /// Command lines in the job (see [`job_line`]), if known.
    pub total: Option<u32>,
}

impl Job {
    /// A job from lines already in memory, for tests and small jobs.
    pub fn from_lines<I, S>(name: impl Into<String>, lines: I) -> Job
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let lines: Vec<String> = lines.into_iter().map(Into::into).collect();
        let total = lines.iter().filter_map(|line| job_line(line)).count();
        Job {
            name: name.into(),
            total: u32::try_from(total).ok(),
            lines: Box::new(lines.into_iter().map(Ok)),
        }
    }

    /// Streams the file at `path`. It is read twice: once now to count its
    /// command lines, then line by line as the job runs.
    pub fn from_file(path: &Path) -> io::Result<Job> {
        let mut total = 0u32;
        for line in BufReader::new(File::open(path)?).lines() {
            if job_line(&line?).is_some() {
                total = total.saturating_add(1);
            }
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        Ok(Job {
            name,
            total: Some(total),
            lines: Box::new(BufReader::new(File::open(path)?).lines()),
        })
    }
}

impl fmt::Debug for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("name", &self.name)
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}

/// The command a raw job line sends, without comments or surrounding
/// whitespace; `None` for a line with nothing to send. Every byte sent
/// takes room in the controller's small RX buffer.
pub fn job_line(raw: &str) -> Option<String> {
    let mut line = String::with_capacity(raw.len());
    let mut in_comment = false;
    for c in raw.chars() {
        match c {
            _ if in_comment => in_comment = c != ')',
            '(' => in_comment = true,
            ';' => break,
            _ => line.push(c),
        }
    }
    let line = line.trim();
    // `%` only marks the start and end of a program.
    (!line.is_empty() && line != "%").then(|| line.to_string())
}

/// What a job is doing, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum JobState {
    Running,
    /// Feed hold, until resumed.
    Paused,
    /// The controller rejected a line, so the job is held: resume continues
    /// with the next line, stop aborts.
    Rejected {
        /// 1-based among the job's command lines.
        line: u32,
        command: String,
        message: String,
    },
    /// Holding, resetting and turning the laser off.
    Stopping,
    Finished,
    /// Stopped by the user or a reset. `warning` says if the machine wasn't
    /// left idle.
    Stopped {
        warning: Option<String>,
    },
    /// Ended by an alarm, a lost connection or an unreadable file.
    Failed {
        reason: String,
    },
}

impl JobState {
    /// True once the job is over.
    pub fn is_done(&self) -> bool {
        matches!(
            self,
            JobState::Finished | JobState::Stopped { .. } | JobState::Failed { .. }
        )
    }
}

/// A job's progress, emitted as it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobProgress {
    pub name: String,
    pub state: JobState,
    /// Command lines the controller has acknowledged.
    pub acknowledged: u32,
    pub total: Option<u32>,
    pub elapsed_ms: u32,
}

/// Why a job couldn't start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    Closed,
    AlreadyRunning,
    /// The machine is in a state that would reject or garble the job.
    NotReady(String),
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::Closed => write!(f, "The machine is not connected"),
            JobError::AlreadyRunning => write!(f, "A job is already running"),
            JobError::NotReady(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for JobError {}

/// What the owner thread must send for the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum JobAction {
    FeedHold,
    CycleStart,
    SoftReset,
    /// GRBL's spindle stop override (`0x9E`): turns the laser off during a
    /// hold without changing the program, so resuming turns it back on.
    SpindleStop,
    /// `M5`, then `M9` if the job had air assist on: command lines outside
    /// the job's own, sent once a stop has left the machine idle.
    LaserOff {
        air: bool,
    },
}

/// Whether `line` turns air assist on (`M7`, `M8`) or off (`M9`), if it
/// does either. The last such word wins, as in GRBL's parser.
pub(super) fn air_assist_change(line: &str) -> Option<bool> {
    let upper = line.to_ascii_uppercase();
    let mut change = None;
    let mut rest = upper.as_str();
    while let Some(at) = rest.find('M') {
        rest = &rest[at + 1..];
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        match digits.parse::<u32>() {
            Ok(7 | 8) => change = Some(true),
            Ok(9) => change = Some(false),
            _ => {}
        }
    }
    change
}

/// When the owner thread calls in: the clock, how many status polls it has
/// sent, and how many of them have been answered. GRBL answers polls in
/// order, so a report is fresh for a mark (the polls sent at some moment)
/// once more than `mark` have been answered: it answers a poll sent after
/// that moment, and so describes the machine after it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Tick {
    pub now: Duration,
    pub polls: u64,
    pub reports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for the head to come to rest after `!`.
    Holding,
    /// Waiting for the banner after the soft reset.
    Resetting,
    /// Waiting for a status report to confirm the machine is idle.
    Settling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Running,
    Paused,
    Rejected {
        line: u32,
        command: String,
        message: String,
    },
    Stopping {
        stage: Stage,
        /// When the stage began, for its timeout.
        since: Duration,
        /// Polls sent when the stage began.
        mark: u64,
        /// Set when the job is stopping because it failed.
        failure: Option<String>,
    },
    Done(JobState),
}

/// A job on the owner thread.
pub(super) struct ActiveJob {
    name: String,
    lines: JobLines,
    total: Option<u32>,
    started: Duration,
    /// The next command line, read ahead to see whether it fits the buffer.
    next: Option<String>,
    exhausted: bool,
    /// Why the job's lines couldn't be read, until the stop begins.
    unreadable: Option<String>,
    sent: u32,
    acknowledged: u32,
    /// Job lines sent but not yet answered.
    in_flight: u32,
    /// Polls sent when the last line was acknowledged.
    drained: Option<u64>,
    /// Whether the job may have air assist on: set by its `M7`/`M8` lines
    /// and by reports showing it on, cleared by its `M9` lines.
    air: bool,
    /// The spindle stop override went out during this hold.
    spindle_stopped: bool,
    phase: Phase,
    /// Set when the state changes, so the change is reported at once.
    changed: bool,
}

impl ActiveJob {
    pub fn new(job: Job, now: Duration) -> Self {
        ActiveJob {
            name: job.name,
            lines: job.lines,
            total: job.total,
            started: now,
            next: None,
            exhausted: false,
            unreadable: None,
            sent: 0,
            acknowledged: 0,
            in_flight: 0,
            drained: None,
            air: false,
            spindle_stopped: false,
            phase: Phase::Running,
            changed: true,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, Phase::Done(_))
    }

    /// The next line to send, read ahead but not taken. `None` when the job
    /// has nothing to send now.
    pub fn peek(&mut self) -> Option<&str> {
        if !matches!(self.phase, Phase::Running | Phase::Paused) {
            return None;
        }
        while self.next.is_none() && !self.exhausted {
            match self.lines.next() {
                Some(Ok(raw)) => self.next = job_line(&raw),
                Some(Err(error)) => {
                    // The next tick stops the machine.
                    self.exhausted = true;
                    self.unreadable = Some(format!("Couldn't read the job: {error}"));
                }
                None => self.exhausted = true,
            }
        }
        self.next.as_deref()
    }

    /// Takes the line [`peek`](Self::peek) returned, now that it is sent.
    /// Returns its 1-based number.
    pub fn take(&mut self) -> Option<(u32, String)> {
        let line = self.next.take()?;
        if let Some(on) = air_assist_change(&line) {
            self.air = on;
        }
        self.sent += 1;
        self.in_flight += 1;
        self.drained = None;
        Some((self.sent, line))
    }

    /// The controller answered job line `number`.
    pub fn answered(
        &mut self,
        number: u32,
        command: String,
        result: Result<(), String>,
        tick: Tick,
    ) -> Option<JobAction> {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.acknowledged += 1;
        if self.exhausted && self.next.is_none() && self.in_flight == 0 {
            self.drained = Some(tick.polls);
        }
        let Err(message) = result else { return None };
        match self.phase {
            Phase::Running | Phase::Paused => {
                self.set_phase(Phase::Rejected {
                    line: number,
                    command,
                    message,
                });
                Some(JobAction::FeedHold)
            }
            // Lines already in the buffer may fail too; the first error is
            // the one to explain.
            _ => None,
        }
    }

    /// A status report arrived. `fresh` is what it said about the
    /// accessories, when it said anything (GRBL reports them with `Ov:`).
    pub fn on_status(
        &mut self,
        state: MachineState,
        fresh: Option<Accessories>,
        tick: Tick,
    ) -> Option<JobAction> {
        if let Some(on) = fresh
            && (on.flood || on.mist)
            && matches!(
                self.phase,
                Phase::Running | Phase::Paused | Phase::Rejected { .. }
            )
        {
            self.air = true;
        }
        match self.phase.clone() {
            Phase::Running => {
                // A job with nothing left to send and nothing in flight is
                // drained, even if its last reply came before it knew so.
                let empty = self.exhausted && self.next.is_none() && self.in_flight == 0;
                if empty && self.drained.is_none() {
                    self.drained = Some(tick.polls);
                }
                let drained = self.drained.is_some_and(|mark| tick.reports > mark);
                // Check mode (`$C`) reports `Check` instead of `Idle`.
                let idle = matches!(state, MachineState::Idle | MachineState::Check);
                if drained && idle {
                    self.set_phase(Phase::Done(JobState::Finished));
                }
                None
            }
            // In laser mode GRBL turns the laser off for a hold by itself
            // (`DISABLE_LASER_DURING_HOLD`, with the spindle stop override).
            // Without laser mode it keeps it on. 0x9E toggles, so it goes out
            // only when the head is at rest and a report says the laser is
            // still on: sent after GRBL's own stop it would turn it back on.
            Phase::Paused | Phase::Rejected { .. }
                if state == (MachineState::Hold { code: 0 })
                    && !self.spindle_stopped
                    && fresh.is_some_and(|on| on.spindle_cw || on.spindle_ccw) =>
            {
                self.spindle_stopped = true;
                Some(JobAction::SpindleStop)
            }
            Phase::Stopping {
                stage,
                mark,
                failure,
                ..
            } if tick.reports > mark => match stage {
                Stage::Holding if !state.is_moving() => {
                    self.stage(Stage::Resetting, tick, failure);
                    Some(JobAction::SoftReset)
                }
                Stage::Settling if state == MachineState::Idle => {
                    self.end_stop(failure, None);
                    Some(JobAction::LaserOff { air: self.air })
                }
                Stage::Settling if state == MachineState::Alarm => {
                    self.end_stop(
                        failure,
                        Some("The machine is in alarm. Unlock it before the next job.".into()),
                    );
                    None
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// The controller printed its banner: everything in flight is gone.
    pub fn on_reset(&mut self, tick: Tick) {
        self.in_flight = 0;
        match self.phase.clone() {
            Phase::Stopping {
                stage: Stage::Resetting,
                failure,
                ..
            } => self.stage(Stage::Settling, tick, failure),
            Phase::Stopping { .. } | Phase::Done(_) => {}
            _ => self.set_phase(Phase::Done(JobState::Stopped {
                warning: Some("The machine reset during the job.".into()),
            })),
        }
    }

    /// An alarm ends the job. The controller turns the laser off itself.
    pub fn on_alarm(&mut self, alarm: &str) {
        if !self.is_done() {
            self.set_phase(Phase::Done(JobState::Failed {
                reason: format!("The machine raised an alarm: {alarm}"),
            }));
        }
    }

    /// The connection closed under the job.
    pub fn on_disconnect(&mut self, reason: &str) {
        if !self.is_done() {
            self.set_phase(Phase::Done(JobState::Failed {
                reason: reason.to_string(),
            }));
        }
    }

    pub fn pause(&mut self) -> Option<JobAction> {
        (self.phase == Phase::Running).then(|| {
            self.set_phase(Phase::Paused);
            JobAction::FeedHold
        })
    }

    /// Resumes a paused job, or continues past a rejected line.
    pub fn resume(&mut self) -> Option<JobAction> {
        matches!(self.phase, Phase::Paused | Phase::Rejected { .. }).then(|| {
            // Resuming restores a laser the spindle stop override turned off.
            self.spindle_stopped = false;
            self.set_phase(Phase::Running);
            JobAction::CycleStart
        })
    }

    /// Feed hold, then soft reset once the head is at rest, then `M5` (and
    /// `M9` if the job had air assist on) once idle.
    pub fn stop(&mut self, tick: Tick) -> Option<JobAction> {
        if matches!(self.phase, Phase::Stopping { .. } | Phase::Done(_)) {
            return None;
        }
        self.stage(Stage::Holding, tick, None);
        Some(JobAction::FeedHold)
    }

    /// Moves the stop along when the controller doesn't answer in time.
    pub fn on_tick(&mut self, tick: Tick) -> Option<JobAction> {
        if let Some(reason) = self.unreadable.take() {
            if matches!(self.phase, Phase::Stopping { .. } | Phase::Done(_)) {
                return None;
            }
            self.stage(Stage::Holding, tick, Some(reason));
            return Some(JobAction::FeedHold);
        }
        let Phase::Stopping {
            stage,
            since,
            failure,
            ..
        } = self.phase.clone()
        else {
            return None;
        };
        let waited = tick.now.saturating_sub(since);
        match stage {
            Stage::Holding if waited >= HOLD_TIMEOUT => {
                self.stage(Stage::Resetting, tick, failure);
                Some(JobAction::SoftReset)
            }
            Stage::Resetting if waited >= RESET_TIMEOUT => {
                self.end_stop(
                    failure,
                    Some("The machine didn't confirm the reset. Check it is stopped.".into()),
                );
                None
            }
            Stage::Settling if waited >= SETTLE_TIMEOUT => {
                self.end_stop(
                    failure,
                    Some("The machine didn't report idle after the reset.".into()),
                );
                None
            }
            _ => None,
        }
    }

    /// The progress to report, and whether the state changed since the
    /// last call.
    pub fn progress(&mut self, now: Duration) -> (JobProgress, bool) {
        let state = match &self.phase {
            Phase::Running => JobState::Running,
            Phase::Paused => JobState::Paused,
            Phase::Rejected {
                line,
                command,
                message,
            } => JobState::Rejected {
                line: *line,
                command: command.clone(),
                message: message.clone(),
            },
            Phase::Stopping { .. } => JobState::Stopping,
            Phase::Done(state) => state.clone(),
        };
        let elapsed = now.saturating_sub(self.started).as_millis();
        let progress = JobProgress {
            name: self.name.clone(),
            state,
            acknowledged: self.acknowledged,
            total: self.total,
            elapsed_ms: u32::try_from(elapsed).unwrap_or(u32::MAX),
        };
        (progress, std::mem::take(&mut self.changed))
    }

    fn stage(&mut self, stage: Stage, tick: Tick, failure: Option<String>) {
        self.set_phase(Phase::Stopping {
            stage,
            since: tick.now,
            mark: tick.polls,
            failure,
        });
    }

    fn end_stop(&mut self, failure: Option<String>, warning: Option<String>) {
        let state = match failure {
            Some(reason) => JobState::Failed {
                reason: match warning {
                    Some(warning) => format!("{reason} {warning}"),
                    None => reason,
                },
            },
            None => JobState::Stopped { warning },
        };
        self.set_phase(Phase::Done(state));
    }

    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.changed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_lines_drop_comments_and_blank_lines() {
        assert_eq!(job_line("G1 X10 ; to the edge"), Some("G1 X10".into()));
        assert_eq!(job_line("G0 (rapid) X5 (fast)"), Some("G0  X5".into()));
        assert_eq!(job_line("  M5\r"), Some("M5".into()));
        assert_eq!(job_line("; Generated by a CAM tool"), None);
        assert_eq!(job_line("(a comment only)"), None);
        assert_eq!(job_line("%"), None);
        assert_eq!(job_line("   "), None);
    }

    #[test]
    fn a_file_job_counts_its_command_lines() {
        let path = std::env::temp_dir().join(format!("bf-job-{}.gcode", std::process::id()));
        std::fs::write(
            &path,
            "; header\nG21 G90\n\nM4 S300\nG1 X1 F600 (cut)\nM5\n",
        )
        .unwrap();

        let job = Job::from_file(&path).unwrap();
        let lines: Vec<String> = job.lines.map(Result::unwrap).collect();

        assert_eq!(job.total, Some(4));
        assert!(job.name.starts_with("bf-job-"), "{}", job.name);
        assert_eq!(lines.len(), 6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn air_assist_words_turn_it_on_and_off() {
        assert_eq!(air_assist_change("M8"), Some(true));
        assert_eq!(air_assist_change("m7"), Some(true));
        assert_eq!(air_assist_change("G1 X10 M08"), Some(true));
        assert_eq!(air_assist_change("M9"), Some(false));
        assert_eq!(air_assist_change("M8 M9"), Some(false));
        assert_eq!(air_assist_change("M5"), None);
        assert_eq!(air_assist_change("M4 S0"), None);
        assert_eq!(air_assist_change("M80"), None);
        assert_eq!(air_assist_change("G1 X9"), None);
    }

    #[test]
    fn job_progress_serializes_for_the_ui() {
        let progress = JobProgress {
            name: "a.nc".into(),
            state: JobState::Rejected {
                line: 3,
                command: "G99".into(),
                message: "GRBL error 20".into(),
            },
            acknowledged: 2,
            total: Some(5),
            elapsed_ms: 1200,
        };
        assert_eq!(
            serde_json::to_value(&progress).unwrap(),
            serde_json::json!({
                "name": "a.nc",
                "state": { "state": "rejected", "line": 3, "command": "G99", "message": "GRBL error 20" },
                "acknowledged": 2,
                "total": 5,
                "elapsedMs": 1200,
            })
        );
    }
}
