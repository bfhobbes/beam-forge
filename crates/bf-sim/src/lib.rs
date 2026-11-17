//! Headless GRBL 1.1 emulator for testing Beam Forge without a machine.
//!
//! The simulator is deterministic: feed it host bytes with
//! [`Simulator::feed`], move its clock with [`Simulator::advance`], and read
//! replies with [`Simulator::take_output`]. Nothing runs on threads or the
//! wall clock, so the same inputs always give byte-identical output.
//!
//! It shares no code with `bf-core` on purpose: an independent implementation
//! catches bugs that a shared GCode model would hide on both sides.
//!
//! It covers the serial protocol, realtime commands, status reports, `$`
//! commands and alarms, plus the G-code interpreter, trapezoidal motion
//! timing and the laser model. The interpreter accepts all of stock
//! GRBL 1.1's G-code except G38.x probing, which needs a probe pin the
//! simulator does not model and so returns `error:20`.
//! [`Simulator::trace`] records executed motion, and laser-on time with the
//! head still, with laser power for the burn view and job checks.
//! [`check`] streams a whole job through the emulator and reports its time,
//! extent and safety violations; the `bf-sim check` CLI wraps it.
//! [`Simulator::inject`] injects a [`Fault`] (alarms, errors, the door, feed
//! hold, a pulled cable, stalls, latency, a smaller RX buffer), and
//! [`Simulator::run_scenario`] scripts them against the host's lines.

mod check;
mod fault;
mod motion;
mod settings;
mod simulator;

pub use check::{Bounds, Pass, Report, Violation, check};
pub use fault::{Fault, MAX_ALARM, MAX_ERROR, Scenario, ScenarioError, ScenarioStep};
pub use motion::{LaserMode, TraceSegment};
pub use settings::{Profile, SettingError, Settings};
pub use simulator::{Event, MachineState, Overrides, Simulator};
