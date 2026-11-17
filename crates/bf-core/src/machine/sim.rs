//! Adapts `bf-sim` to [`Transport`] and [`Clock`] for tests. Reads advance
//! the simulator's clock, so timeouts take no wall-clock time.
//!
//! That clock runs as fast as the connection's owner thread can loop, while
//! a test reacts to status reports on other threads. On a busy machine those
//! threads can lag by any amount of simulated time, so a test that reacts to
//! a report, or checks the simulator against one, should use
//! [`SimLink::lockstep`]: the clock then waits for each report to be handled.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use bf_sim::Simulator;

use super::{Clock, Transport};

/// Simulated time each poll moves the controller forward.
const STEP: Duration = Duration::from_millis(10);

/// A simulator on the other end of a cable that tests can unplug or garble.
/// For controller faults (alarms, errors, the door, stalls...) inject an
/// [`bf_sim::Fault`] or run an [`bf_sim::Scenario`] through [`SimLink::sim`].
#[derive(Clone)]
pub struct SimLink {
    sim: Arc<Mutex<Simulator>>,
    unplugged: Arc<AtomicBool>,
    garbled: Arc<AtomicBool>,
    lockstep: Lockstep,
}

/// Holds the simulated clock until the test has handled every status report
/// the controller sent, so the test reacts at the simulated moment of the
/// report however slowly its threads run. Commands still go through while
/// the clock is held; only time stands still.
#[derive(Clone, Default)]
pub struct Lockstep(Arc<LockstepState>);

#[derive(Default)]
struct LockstepState {
    on: AtomicBool,
    /// Status reports the controller has sent.
    sent: AtomicU64,
    /// Status reports the test has finished with.
    handled: AtomicU64,
}

impl Lockstep {
    /// The test has finished with one more status report.
    pub fn handled(&self) {
        self.0.handled.fetch_add(1, Ordering::SeqCst);
    }

    /// Counts the status reports in bytes leaving the controller.
    fn sent(&self, bytes: &[u8]) {
        if self.0.on.load(Ordering::SeqCst) {
            let reports = bytes.iter().filter(|&&b| b == b'<').count() as u64;
            self.0.sent.fetch_add(reports, Ordering::SeqCst);
        }
    }

    /// A report is still waiting to be handled.
    fn behind(&self) -> bool {
        self.0.on.load(Ordering::SeqCst)
            && self.0.handled.load(Ordering::SeqCst) < self.0.sent.load(Ordering::SeqCst)
    }
}

impl SimLink {
    pub fn new(sim: Simulator) -> Self {
        SimLink {
            sim: Arc::new(Mutex::new(sim)),
            unplugged: Arc::default(),
            garbled: Arc::default(),
            lockstep: Lockstep::default(),
        }
    }

    /// Turns lockstep on (see [`Lockstep`]) and returns the handle the test
    /// reports handled status reports on: once per
    /// [`MachineEvent::Status`](super::MachineEvent::Status), after it has
    /// acted on it. Call before opening a connection.
    pub fn lockstep(&self) -> Lockstep {
        self.lockstep.0.on.store(true, Ordering::SeqCst);
        self.lockstep.clone()
    }

    pub fn sim(&self) -> MutexGuard<'_, Simulator> {
        self.sim.lock().unwrap()
    }

    pub fn transport(&self) -> SimTransport {
        SimTransport {
            link: self.clone(),
            pending: VecDeque::new(),
        }
    }

    pub fn clock(&self) -> SimClock {
        SimClock(self.sim.clone())
    }

    pub fn unplug(&self) {
        self.unplugged.store(true, Ordering::SeqCst);
    }

    /// Corrupts every byte from the controller, like a wrong baud rate.
    pub fn garble(&self, on: bool) {
        self.garbled.store(on, Ordering::SeqCst);
    }

    /// Fails once the cable is unplugged, by [`SimLink::unplug`] or by an
    /// injected [`bf_sim::Fault::Disconnect`].
    fn check(&self) -> io::Result<()> {
        if self.unplugged.load(Ordering::SeqCst) || self.sim().is_disconnected() {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "device unplugged",
            ))
        } else {
            Ok(())
        }
    }
}

pub struct SimTransport {
    link: SimLink,
    pending: VecDeque<u8>,
}

impl Transport for SimTransport {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.link.check()?;
        self.link.sim().feed(bytes);
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let mut waited = Duration::ZERO;
        while self.pending.is_empty() {
            self.link.check()?;
            let mut sim = self.link.sim();
            sim.advance(Duration::ZERO);
            let output = sim.take_output();
            if !output.is_empty() {
                self.link.lockstep.sent(&output);
                let garbled = self.link.garbled.load(Ordering::SeqCst);
                self.pending.extend(
                    output
                        .into_iter()
                        .map(|b| if garbled { b ^ 0x55 } else { b }),
                );
            } else if waited >= timeout {
                return Ok(0);
            } else if self.link.lockstep.behind() {
                // Hold the clock, but return so the owner thread can send
                // what the test asked for while handling the report.
                drop(sim);
                thread::sleep(Duration::from_millis(1));
                return Ok(0);
            } else {
                let dt = STEP.min(timeout - waited);
                sim.advance(dt);
                waited += dt;
            }
        }
        let count = buf.len().min(self.pending.len());
        for (slot, byte) in buf.iter_mut().zip(self.pending.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

pub struct SimClock(Arc<Mutex<Simulator>>);

impl Clock for SimClock {
    fn now(&self) -> Duration {
        self.0.lock().unwrap().now()
    }
}
