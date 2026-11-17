//! Fault injection: alarms, errors, switches, cable and timing faults,
//! and scenarios that fire them as the host's lines are answered.

use std::collections::VecDeque;
use std::time::Duration;

use super::{ALARM_SOFT_LIMIT, Event, MachineState, Simulator, Spindle};
use crate::fault::{Fault, Scenario, ScenarioStep};

/// Faults in force. Power-ups and resets keep them, as they would a wiring
/// or firmware fault; [`Simulator::clear_faults`] removes them.
#[derive(Debug, Default)]
pub(super) struct Faults {
    pub disconnected: bool,
    pub stall_until: Option<Duration>,
    pub ack_latency: Duration,
    /// When the line waiting at the front of the RX buffer may run, while
    /// [`Faults::ack_latency`] delays it.
    pub line_ready_at: Option<Duration>,
    pub rx_limit: Option<usize>,
    /// Lines (by [`Simulator::lines_read`] number) to answer with an error.
    pub line_errors: Vec<(u64, u8)>,
    pub scenario: Option<Armed>,
}

#[derive(Debug)]
pub(super) struct Armed {
    /// Lines finished when the scenario started.
    base: u64,
    steps: VecDeque<ScenarioStep>,
}

impl Simulator {
    /// Injects a fault now. See [`Fault`] for what each one does.
    pub fn inject(&mut self, fault: Fault) {
        self.events.push(Event::Fault {
            at: self.now,
            fault,
        });
        match fault {
            Fault::Alarm(code) => self.raise_alarm(code),
            Fault::Error { code, line } => {
                let line = self.lines_read + line.max(1);
                self.faults.line_errors.push((line, code));
            }
            Fault::Door { open } => self.set_door_open(open),
            Fault::FeedHold => self.feed_hold(),
            Fault::Disconnect => self.faults.disconnected = true,
            Fault::Stall(time) => {
                self.faults.stall_until = (!time.is_zero()).then_some(self.now + time);
            }
            Fault::AckLatency(time) => {
                self.faults.ack_latency = time;
                self.faults.line_ready_at = None;
            }
            Fault::RxBuffer(limit) => self.faults.rx_limit = limit,
        }
    }

    /// Starts a scenario, replacing any still running. Its lines count from
    /// now; steps for line 0 fire straight away.
    pub fn run_scenario(&mut self, scenario: Scenario) {
        self.faults.scenario = Some(Armed {
            base: self.lines_finished(),
            steps: scenario.steps.into(),
        });
        self.fire_scenario();
    }

    /// Steps of the running scenario that have yet to fire.
    pub fn scenario_pending(&self) -> usize {
        self.faults
            .scenario
            .as_ref()
            .map_or(0, |armed| armed.steps.len())
    }

    /// Ends every fault in force and any running scenario. The door switch
    /// and alarms are machine state, not faults, and stay as they are; a
    /// pulled cable stays pulled until the next power-up.
    pub fn clear_faults(&mut self) {
        let disconnected = self.faults.disconnected;
        self.faults = Faults {
            disconnected,
            ..Faults::default()
        };
    }

    /// The cable was pulled by [`Fault::Disconnect`]: the host should drop
    /// its connection. A power-up (a new connection) plugs it back in.
    pub fn is_disconnected(&self) -> bool {
        self.faults.disconnected
    }

    /// RX buffer size in force: the profile's, unless [`Fault::RxBuffer`]
    /// shrank it.
    pub fn rx_capacity(&self) -> usize {
        let size = self.profile.rx_buffer_size;
        self.faults.rx_limit.map_or(size, |limit| limit.min(size))
    }

    /// Lines read and fully executed: the one still running its effects
    /// (waiting on motion, a dwell) has not finished yet.
    pub(super) fn lines_finished(&self) -> u64 {
        self.lines_read - u64::from(!self.ops.is_empty())
    }

    /// Fires scenario steps whose line has been answered.
    pub(super) fn fire_scenario(&mut self) {
        let finished = self.lines_finished();
        while let Some(armed) = &mut self.faults.scenario {
            let due = armed
                .steps
                .front()
                .is_some_and(|step| armed.base + step.after <= finished);
            if !due {
                break;
            }
            if let Some(step) = armed.steps.pop_front() {
                self.inject(step.fault);
            }
        }
    }

    /// The error to answer line `line` with, if a fault asked for one. A
    /// fault whose line a reset flushed applies to the next line read.
    pub(super) fn take_line_error(&mut self, line: u64) -> Option<u8> {
        let errors = &mut self.faults.line_errors;
        let index = errors.iter().position(|&(at, _)| at <= line)?;
        Some(errors.remove(index).1)
    }

    /// Whether a stall stops the main loop reading lines.
    pub(super) fn stalled(&mut self) -> bool {
        match self.faults.stall_until {
            Some(until) if self.now < until => true,
            Some(_) => {
                self.faults.stall_until = None;
                false
            }
            None => false,
        }
    }

    /// With [`Fault::AckLatency`], whether the complete line at the front of
    /// the RX buffer still has to wait. Starts its wait if it hasn't yet.
    pub(super) fn line_delayed(&mut self) -> bool {
        if self.faults.ack_latency.is_zero() {
            return false;
        }
        match self.faults.line_ready_at {
            None => {
                self.faults.line_ready_at = Some(self.now + self.faults.ack_latency);
                true
            }
            Some(ready) if self.now < ready => true,
            Some(_) => {
                self.faults.line_ready_at = None;
                false
            }
        }
    }

    /// When a timed fault (a stall, a line's extra latency) next acts, if
    /// one is pending. A host that skips idle time, like the simulator app's
    /// instant speed, must still run the clock up to it.
    pub fn fault_deadline(&self) -> Option<Duration> {
        [self.faults.stall_until, self.faults.line_ready_at]
            .into_iter()
            .flatten()
            .filter(|&at| at > self.now)
            .min()
    }

    /// `ALARM:n`, raised the way GRBL raises it (see [`Fault::Alarm`]).
    fn raise_alarm(&mut self, code: u8) {
        match code {
            1 => {
                // The limit switch ISR calls mc_reset: the steppers stop dead.
                self.motion.clear(self.now);
                self.gc_position = self.machine_position();
                self.ops.clear();
                self.dwell_until = None;
                self.stop_reason = None;
                self.critical_alarm(code);
            }
            ALARM_SOFT_LIMIT => self.soft_limit_violation(),
            _ => {
                // system_set_exec_alarm + mc_reset: the alarm is reported,
                // then the reset reinitialises GRBL, which keeps the alarm.
                self.clear_volatile_state();
                self.set_state(MachineState::Alarm);
                self.send_line(&format!("ALARM:{code}"));
                self.print_banner();
            }
        }
    }

    /// Hard and soft limits: laser and air off, then everything but a reset
    /// is ignored.
    pub(super) fn critical_alarm(&mut self, code: u8) {
        self.set_spindle(Spindle::Off, 0.0);
        self.flood = false;
        self.mist = false;
        self.set_state(MachineState::Alarm);
        self.send_line(&format!("ALARM:{code}"));
        self.send_line("[MSG:Reset to continue]");
        self.critical = true;
    }
}
