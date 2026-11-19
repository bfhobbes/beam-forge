use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::control::{HeldJog, JogDirection, TestFire, positive};
use super::handshake::{Handshake, HandshakeError, handshake};
use super::job::{ActiveJob, Job, JobAction, JobError, JobProgress, Tick};
use super::overrides::{OverrideKind, OverrideTargets};
use super::status::{MachineState, MachineStatus, StatusTracker};
use super::{BANNER_TIMEOUT, Clock, Dialect, LineReader, MachineInfo, Reply, Transport};

/// How long the owner thread waits for input before checking for commands.
const POLL: Duration = Duration::from_millis(10);
/// How often status is polled while the machine is still (5 Hz)...
pub const STATUS_INTERVAL: Duration = Duration::from_millis(200);
/// ...and while it moves (10 Hz), so the position readout keeps up.
pub const MOVING_STATUS_INTERVAL: Duration = Duration::from_millis(100);
/// How often a running job reports progress; state changes report at once.
pub const JOB_PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// What the owner thread reports.
#[derive(Debug, Clone, PartialEq)]
pub enum MachineEvent {
    /// A non-empty line from the controller, without its line ending.
    /// Status reports arrive as [`MachineEvent::Status`] instead.
    Line(String),
    /// A status report, completed from earlier ones. The owner thread polls
    /// for these every [`STATUS_INTERVAL`] while connected.
    Status(MachineStatus),
    /// A job's progress: whenever its state changes, and every
    /// [`JOB_PROGRESS_INTERVAL`] while lines are acknowledged. The last one
    /// for a job has a [done](super::JobState::is_done) state.
    Job(JobProgress),
    /// The owner thread stopped. `reason` is set when the port failed (a
    /// pulled cable) and absent when the connection was closed on purpose.
    /// Always the last event.
    Disconnected { reason: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The connection is closed.
    Closed,
    /// A command line may not contain line breaks; one is added on sending.
    LineBreak,
    /// A job is streaming; a line sent now would land in the middle of it.
    JobRunning,
    /// The laser is test firing; nothing may delay its off command.
    TestFiring,
    /// The machine can't do that now; the message says why.
    NotReady(String),
    /// The request itself is wrong, such as a jog of 0 mm.
    Invalid(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Closed => write!(f, "The machine is not connected"),
            SendError::LineBreak => write!(f, "A command line can't contain a line break"),
            SendError::JobRunning => write!(f, "A job is running. Stop it first."),
            SendError::TestFiring => write!(f, "The laser is test firing. Try again in a moment."),
            SendError::NotReady(message) | SendError::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SendError {}

/// Why [`ConnectionHandle::query`] got no reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    Send(SendError),
    /// No `ok` or `error` within the timeout.
    Timeout,
    /// The controller answered with an error, explained for the user.
    Rejected(String),
    /// The controller reset before it replied.
    Reset,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::Send(error) => error.fmt(f),
            QueryError::Timeout => write!(f, "The machine didn't reply in time"),
            QueryError::Rejected(message) => f.write_str(message),
            QueryError::Reset => write!(f, "The machine reset before it replied"),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<SendError> for QueryError {
    fn from(error: SendError) -> Self {
        QueryError::Send(error)
    }
}

type QueryReply = Sender<Result<Vec<String>, QueryError>>;

enum Command {
    /// Realtime bytes, which get no reply.
    Write(Vec<u8>),
    /// A command line, which the controller acknowledges.
    Line(Vec<u8>),
    /// A command line whose reply lines go to `QueryReply` instead of the
    /// events.
    Query(Vec<u8>, QueryReply),
    StartJob(Job, Sender<Result<(), JobError>>),
    PauseJob,
    ResumeJob,
    StopJob,
    RxBuffer(usize),
    /// Move an override to this percentage.
    Override(OverrideKind, u16),
    /// Turn the laser on with this line, then off after the duration.
    TestFire(Vec<u8>, Duration, Sender<Result<(), SendError>>),
    /// Start or renew a held jog.
    JogHold(JogDirection, f64),
    /// Cancel any jog, held or not.
    JogStop,
    Close,
}

/// Sends to an open [`Connection`] from any thread. Cheap to clone; fails
/// with [`SendError::Closed`] once the connection is gone.
#[derive(Clone)]
pub struct ConnectionHandle {
    dialect: &'static dyn Dialect,
    commands: Sender<Command>,
    /// Set by the owner thread while a job streams.
    job_active: Arc<AtomicBool>,
    /// Set from a test fire's request until its off command is queued.
    firing: Arc<AtomicBool>,
    /// Set by the owner thread when a homing cycle succeeds, cleared by an
    /// alarm.
    homed: Arc<AtomicBool>,
}

impl ConnectionHandle {
    pub fn dialect(&self) -> &'static dyn Dialect {
        self.dialect
    }

    /// Queues a command line; the dialect's line ending is added. Refused
    /// while a job streams.
    pub fn send_line(&self, line: &str) -> Result<(), SendError> {
        let bytes = self.line_bytes(line)?;
        self.check_manual()?;
        self.send(Command::Line(bytes))
    }

    /// Refuses manual commands while a job streams or the laser test fires.
    fn check_manual(&self) -> Result<(), SendError> {
        if self.job_active() {
            Err(SendError::JobRunning)
        } else if self.firing.load(Ordering::SeqCst) {
            Err(SendError::TestFiring)
        } else {
            Ok(())
        }
    }

    /// Jogs `distance` mm in `direction` (each moving axis by `distance`, so
    /// diagonals too) at `feed` mm/min.
    pub fn jog(&self, direction: JogDirection, distance: f64, feed: f64) -> Result<(), SendError> {
        let direction = direction.validate().map_err(SendError::Invalid)?;
        let distance = positive(distance, "step").map_err(SendError::Invalid)?;
        let feed = positive(feed, "speed").map_err(SendError::Invalid)?;
        self.send_line(&self.dialect.jog_by(
            distance * f64::from(direction.x),
            distance * f64::from(direction.y),
            feed,
        ))
    }

    /// Jogs to work X0 Y0 at `feed` mm/min.
    pub fn go_to_origin(&self, feed: f64) -> Result<(), SendError> {
        let feed = positive(feed, "speed").map_err(SendError::Invalid)?;
        self.send_line(&self.dialect.jog_to_origin(feed))
    }

    /// Starts jogging in `direction` at `feed` mm/min, or keeps a held jog
    /// going. Call it about every 100 ms while the button is held: the jog
    /// cancels itself after [`JOG_HOLD_TIMEOUT`](super::JOG_HOLD_TIMEOUT)
    /// without one. [`jog_stop`](Self::jog_stop) ends it at once.
    pub fn jog_hold(&self, direction: JogDirection, feed: f64) -> Result<(), SendError> {
        let direction = direction.validate().map_err(SendError::Invalid)?;
        let feed = positive(feed, "speed").map_err(SendError::Invalid)?;
        self.check_manual()?;
        self.send(Command::JogHold(direction, feed))
    }

    /// Cancels any jog: the head decelerates and queued jogs are dropped.
    pub fn jog_stop(&self) -> Result<(), SendError> {
        self.send(Command::JogStop)
    }

    /// Fires the laser with the head still, clamped to the test-fire caps,
    /// at a power relative to `max_spindle` (`$30`). The owner thread sends
    /// the off command after the duration whatever happens to the caller,
    /// and at once if the connection closes first. Returns what it fired.
    pub fn test_fire(&self, fire: TestFire, max_spindle: f64) -> Result<TestFire, SendError> {
        let fire = fire.clamped();
        let on = self.line_bytes(&self.dialect.laser_on(fire.s_value(max_spindle)))?;
        if self.job_active() {
            return Err(SendError::JobRunning);
        }
        if self
            .firing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(SendError::TestFiring);
        }
        let (reply, answer) = mpsc::channel();
        let result = self
            .send(Command::TestFire(on, fire.duration(), reply))
            .and_then(|()| answer.recv().unwrap_or(Err(SendError::Closed)));
        if result.is_err() {
            self.firing.store(false, Ordering::SeqCst);
        }
        result.map(|()| fire)
    }

    /// Queues a realtime command byte such as `?` or a soft reset.
    pub fn send_realtime(&self, byte: u8) -> Result<(), SendError> {
        self.send(Command::Write(vec![byte]))
    }

    /// Sends a command line and waits up to `timeout` for its reply. Returns
    /// the lines before the `ok`, such as the `$n=value` lines of `$$`.
    /// Those lines don't reach the event handler; alarms and messages that
    /// arrive meanwhile still do.
    pub fn query(&self, line: &str, timeout: Duration) -> Result<Vec<String>, QueryError> {
        let bytes = self.line_bytes(line)?;
        let (reply, answer) = mpsc::channel();
        self.send(Command::Query(bytes, reply))?;
        match answer.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(QueryError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(SendError::Closed.into()),
        }
    }

    /// Streams `job` with character counting. Fails if a job is already
    /// running or the machine is in alarm or busy; progress then arrives as
    /// [`MachineEvent::Job`]. Starting resets the feed, rapid and laser
    /// power overrides to 100%: GRBL keeps them between jobs, and one left
    /// at 50% would silently halve the next job's power.
    pub fn start_job(&self, job: Job) -> Result<(), JobError> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::StartJob(job, reply))
            .map_err(|_| JobError::Closed)?;
        answer.recv().unwrap_or(Err(JobError::Closed))
    }

    /// Feed hold. The job keeps filling the controller's buffer.
    pub fn pause_job(&self) -> Result<(), SendError> {
        self.send(Command::PauseJob)
    }

    /// Cycle start after a pause, or continue past a rejected line.
    pub fn resume_job(&self) -> Result<(), SendError> {
        self.send(Command::ResumeJob)
    }

    /// Feed hold, then soft reset once the head is at rest, then `M5` once
    /// idle, so the laser is off and the machine idle and unlocked.
    pub fn stop_job(&self) -> Result<(), SendError> {
        self.send(Command::StopJob)
    }

    /// Moves the feed or laser power override to `percent` (clamped to
    /// 10-200), with GRBL's realtime override bytes. The owner thread nudges
    /// from the last reported value and waits for each change to be
    /// reported, so dragging a slider can't overshoot. Works with or without
    /// a job; a reset puts every override back to 100%.
    pub fn set_override(&self, kind: OverrideKind, percent: u16) -> Result<(), SendError> {
        self.send(Command::Override(kind, percent))
    }

    /// True while a job streams.
    pub fn job_active(&self) -> bool {
        self.job_active.load(Ordering::SeqCst)
    }

    /// True once a homing cycle (`$H`) has succeeded on this connection,
    /// until an alarm says the position may be lost. GRBL doesn't report
    /// whether it is homed, so a machine homed before connecting counts as
    /// not homed.
    pub fn homed(&self) -> bool {
        self.homed.load(Ordering::SeqCst)
    }

    /// Changes how many bytes of unacknowledged lines the controller is
    /// trusted to buffer; see [`Dialect::rx_buffer_size`].
    pub fn set_rx_buffer(&self, bytes: usize) -> Result<(), SendError> {
        self.send(Command::RxBuffer(bytes))
    }

    fn line_bytes(&self, line: &str) -> Result<Vec<u8>, SendError> {
        if line.contains(['\r', '\n']) {
            return Err(SendError::LineBreak);
        }
        let mut bytes = line.as_bytes().to_vec();
        bytes.extend_from_slice(self.dialect.line_ending());
        Ok(bytes)
    }

    fn send(&self, command: Command) -> Result<(), SendError> {
        self.commands.send(command).map_err(|_| SendError::Closed)
    }
}

/// A query waiting for its reply.
struct Pending {
    lines: Vec<String>,
    reply: QueryReply,
}

/// Who a command line's reply goes to.
enum Awaiting {
    /// The event handler, as lines.
    Event,
    Query(Pending),
    /// The running job; only errors reach the event handler.
    Job {
        number: u32,
        command: String,
    },
    /// Nobody: a line the job sent outside its own, such as the final `M5`.
    Quiet,
    /// A held jog's segment; only its first error reaches the event handler.
    HeldJog,
}

/// A command line sent and not yet answered.
struct Sent {
    /// Bytes it takes in the controller's RX buffer until answered.
    len: usize,
    awaiting: Awaiting,
    /// It runs the homing cycle.
    homing: bool,
}

/// An open, handshaken controller. One owner thread holds the transport and
/// polls status; commands reach it through a channel and inbound lines come
/// back as events.
///
/// Events are handed to the event handler on a thread of their own, so a
/// slow or frozen handler (a UI that stopped responding) can't hold up the
/// owner thread: a running job keeps streaming, and stops cleanly when
/// asked, whatever the handler is doing.
///
/// Dropping the connection closes it.
pub struct Connection {
    machine: MachineInfo,
    handle: ConnectionHandle,
    owner: Option<JoinHandle<()>>,
}

impl Connection {
    /// Handshakes on `transport`, then starts the owner thread, which keeps
    /// `clock` to pace status polls. `on_event` runs on a thread of its own,
    /// in the order the owner thread emits events.
    pub fn open<T, C, F>(transport: T, clock: C, on_event: F) -> Result<Self, HandshakeError>
    where
        T: Transport + 'static,
        C: Clock + Send + 'static,
        F: FnMut(MachineEvent) + Send + 'static,
    {
        Self::open_with_timeout(transport, clock, BANNER_TIMEOUT, on_event)
    }

    pub fn open_with_timeout<T, C, F>(
        mut transport: T,
        clock: C,
        timeout: Duration,
        on_event: F,
    ) -> Result<Self, HandshakeError>
    where
        T: Transport + 'static,
        C: Clock + Send + 'static,
        F: FnMut(MachineEvent) + Send + 'static,
    {
        let mut on_event = on_event;
        let Handshake {
            machine,
            dialect,
            lines,
            reader,
        } = handshake(&mut transport, &clock, timeout)?;
        let (commands, inbox) = mpsc::channel();
        let job_active = Arc::new(AtomicBool::new(false));
        let active = job_active.clone();
        let firing = Arc::new(AtomicBool::new(false));
        let owner_firing = firing.clone();
        let homed = Arc::new(AtomicBool::new(false));
        let owner_homed = homed.clone();
        // The handler thread ends once the owner thread has emitted its last
        // event and dropped the sender. It isn't joined: a frozen handler
        // must not hold up closing either.
        let (events, outbox) = mpsc::channel::<MachineEvent>();
        thread::Builder::new()
            .name("machine-events".into())
            .spawn(move || {
                for event in outbox {
                    on_event(event);
                }
            })?;
        let emit = move |event| {
            let _ = events.send(event);
        };
        let owner = thread::Builder::new()
            .name("machine-connection".into())
            .spawn(move || {
                Owner {
                    transport,
                    clock,
                    dialect,
                    reader,
                    status: StatusTracker::new(),
                    pending: VecDeque::new(),
                    outbox: VecDeque::new(),
                    rx_buffer: dialect.rx_buffer_size(),
                    polls: 0,
                    reports: 0,
                    job: None,
                    job_active: active,
                    job_reported: (Duration::ZERO, 0),
                    actions: Vec::new(),
                    held_jog: None,
                    jog_cancelled_at: 0,
                    laser_off_at: None,
                    firing: owner_firing,
                    homed: owner_homed,
                    overrides: OverrideTargets::default(),
                }
                .run(lines, inbox, emit)
            })?;
        Ok(Connection {
            machine,
            handle: ConnectionHandle {
                dialect,
                commands,
                job_active,
                firing,
                homed,
            },
            owner: Some(owner),
        })
    }

    pub fn machine(&self) -> &MachineInfo {
        &self.machine
    }

    pub fn dialect(&self) -> &'static dyn Dialect {
        self.handle.dialect
    }

    /// A handle for sending from other threads, such as a query that must
    /// not hold the lock this connection is kept under.
    pub fn handle(&self) -> ConnectionHandle {
        self.handle.clone()
    }

    /// Queues a command line; the dialect's line ending is added.
    pub fn send_line(&self, line: &str) -> Result<(), SendError> {
        self.handle.send_line(line)
    }

    /// Queues a realtime command byte such as `?` or a soft reset.
    pub fn send_realtime(&self, byte: u8) -> Result<(), SendError> {
        self.handle.send_realtime(byte)
    }

    /// Queues the dialect's soft reset, which aborts motion and makes the
    /// controller print its banner again.
    pub fn soft_reset(&self) -> Result<(), SendError> {
        self.handle
            .send(Command::Write(self.handle.dialect.soft_reset().to_vec()))
    }

    /// Stops the owner thread and closes the port.
    pub fn close(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.handle.send(Command::Close);
        if let Some(owner) = self.owner.take() {
            // Never join from the owner thread itself.
            if owner.thread().id() != thread::current().id() {
                let _ = owner.join();
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Owner<T, C> {
    transport: T,
    clock: C,
    dialect: &'static dyn Dialect,
    reader: LineReader,
    status: StatusTracker,
    /// One entry per command line awaiting its reply, oldest first.
    pending: VecDeque<Sent>,
    /// Command lines waiting for room in the controller's RX buffer.
    outbox: VecDeque<(Vec<u8>, Awaiting)>,
    /// Bytes of unanswered lines the controller can buffer.
    rx_buffer: usize,
    /// Status queries sent so far...
    polls: u64,
    /// ...and answered (see [`Tick`]).
    reports: u64,
    job: Option<ActiveJob>,
    job_active: Arc<AtomicBool>,
    /// When the job last reported, and how many lines were acknowledged.
    job_reported: (Duration, u32),
    /// What the job asked for while replies were read.
    actions: Vec<JobAction>,
    held_jog: Option<HeldJog>,
    /// Status polls sent before the last jog cancel: a held jog starts only
    /// on a report polled after it.
    jog_cancelled_at: u64,
    /// When a test fire's off command is due.
    laser_off_at: Option<Duration>,
    firing: Arc<AtomicBool>,
    homed: Arc<AtomicBool>,
    overrides: OverrideTargets,
}

impl<T: Transport, C: Clock> Owner<T, C> {
    fn run(
        mut self,
        pending: Vec<String>,
        inbox: Receiver<Command>,
        mut on_event: impl FnMut(MachineEvent),
    ) {
        self.emit_lines(pending, &mut on_event);
        let mut buf = [0; 1024];
        // The first poll goes out straight away.
        let mut next_poll = self.clock.now();
        let failure = 'owner: loop {
            loop {
                let written = match inbox.try_recv() {
                    Ok(Command::Write(bytes)) => self.transport.write(&bytes),
                    Ok(Command::Line(bytes)) => {
                        self.outbox.push_back((bytes, Awaiting::Event));
                        Ok(())
                    }
                    Ok(Command::Query(bytes, reply)) => {
                        let query = Pending {
                            lines: Vec::new(),
                            reply,
                        };
                        self.outbox.push_back((bytes, Awaiting::Query(query)));
                        Ok(())
                    }
                    Ok(Command::StartJob(job, reply)) => {
                        let started = self.start_job(job);
                        let ok = started.is_ok();
                        let _ = reply.send(started);
                        if ok {
                            // Overrides left from earlier would apply to the
                            // whole job; these go out ahead of its first line.
                            self.overrides = OverrideTargets::default();
                            self.transport.write(self.dialect.override_resets())
                        } else {
                            Ok(())
                        }
                    }
                    Ok(Command::PauseJob) => self.job_command(ActiveJob::pause),
                    Ok(Command::ResumeJob) => self.job_command(ActiveJob::resume),
                    Ok(Command::StopJob) => {
                        let tick = self.tick();
                        self.job_command(|job| job.stop(tick))
                    }
                    Ok(Command::RxBuffer(bytes)) => {
                        self.rx_buffer = bytes;
                        Ok(())
                    }
                    Ok(Command::Override(kind, percent)) => {
                        self.overrides.set(kind, percent);
                        Ok(())
                    }
                    Ok(Command::TestFire(on, duration, reply)) => {
                        let _ = reply.send(self.test_fire(on, duration));
                        Ok(())
                    }
                    Ok(Command::JogHold(direction, feed)) => self.jog_hold(direction, feed),
                    Ok(Command::JogStop) => self.jog_stop(),
                    Ok(Command::Close) | Err(TryRecvError::Disconnected) => {
                        // Closing under a job would leave the controller
                        // running what it has buffered, laser and all.
                        if self.job.as_ref().is_some_and(|job| !job.is_done()) {
                            let _ = self.transport.write(self.dialect.soft_reset());
                        }
                        // Nor may a test fire or a held jog outlive us.
                        if self.laser_off_at.take().is_some() {
                            let _ = self.transport.write(&self.line(self.dialect.laser_off()));
                        }
                        if self.held_jog.is_some() {
                            let _ = self.transport.write(&[self.dialect.jog_cancel()]);
                        }
                        break 'owner None;
                    }
                    Err(TryRecvError::Empty) => break,
                };
                if let Err(error) = written {
                    break 'owner Some(error);
                }
            }
            if let Err(error) = self.manual_timers() {
                break Some(error);
            }
            if let Err(error) = self.nudge_overrides() {
                break Some(error);
            }
            if let Err(error) = self.send_lines() {
                break Some(error);
            }
            let now = self.clock.now();
            if now >= next_poll {
                if let Err(error) = self.transport.write(&[self.dialect.status_query()]) {
                    break Some(error);
                }
                self.polls += 1;
                next_poll = now + self.poll_interval();
            }
            match self.transport.read(&mut buf, POLL) {
                Ok(read) => {
                    let lines = self.reader.push(&buf[..read]);
                    self.emit_lines(lines, &mut on_event);
                }
                Err(error) => break Some(error),
            }
            // Send what those replies asked for, such as a stop's `M5`,
            // before the job reports done: from then on the app allows a
            // disconnect.
            if let Err(error) = self.send_lines() {
                break Some(error);
            }
            self.report_job(&mut on_event);
        };
        let reason = failure.map(|error| format!("Connection lost: {error}"));
        if let Some(job) = &mut self.job {
            job.on_disconnect(reason.as_deref().unwrap_or("The connection was closed."));
            self.report_job(&mut on_event);
        }
        // Refuse further commands before anyone hears about the disconnect.
        drop(inbox);
        drop(self.transport);
        on_event(MachineEvent::Disconnected { reason });
    }

    fn tick(&self) -> Tick {
        Tick {
            now: self.clock.now(),
            polls: self.polls,
            reports: self.reports,
        }
    }

    fn start_job(&mut self, job: Job) -> Result<(), JobError> {
        if self.job.is_some() {
            return Err(JobError::AlreadyRunning);
        }
        if self.laser_off_at.is_some() || self.held_jog.is_some() {
            return Err(JobError::NotReady(
                "The machine is busy. Wait for it to be idle.".into(),
            ));
        }
        match self.status.last().map(|status| status.state) {
            Some(MachineState::Alarm) => {
                return Err(JobError::NotReady(
                    "The machine is in alarm. Home or unlock it first.".into(),
                ));
            }
            // Check mode (`$C`) runs a job without moving, to test it.
            None | Some(MachineState::Idle | MachineState::Check) => {}
            Some(_) => {
                return Err(JobError::NotReady(
                    "The machine is busy. Wait for it to be idle.".into(),
                ));
            }
        }
        self.job = Some(ActiveJob::new(job, self.clock.now()));
        self.job_active.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// `line` with the dialect's line ending.
    fn line(&self, line: &str) -> Vec<u8> {
        let mut bytes = line.as_bytes().to_vec();
        bytes.extend_from_slice(self.dialect.line_ending());
        bytes
    }

    /// Queues the test fire's `on` line and schedules its off command.
    fn test_fire(&mut self, on: Vec<u8>, duration: Duration) -> Result<(), SendError> {
        let refusal = if self.job.is_some() {
            Some(SendError::JobRunning)
        } else if self.held_jog.is_some() {
            Some(SendError::NotReady("The machine is jogging.".into()))
        } else if self.status.last().map(|status| status.state) != Some(MachineState::Idle) {
            Some(SendError::NotReady(
                "The machine must be idle to test fire.".into(),
            ))
        } else {
            None
        };
        if let Some(error) = refusal {
            self.firing.store(false, Ordering::SeqCst);
            return Err(error);
        }
        self.outbox.push_back((on, Awaiting::Quiet));
        self.laser_off_at = Some(self.clock.now() + duration);
        Ok(())
    }

    fn jog_hold(&mut self, direction: JogDirection, feed: f64) -> io::Result<()> {
        if self.job.is_some() || self.laser_off_at.is_some() {
            return Ok(());
        }
        let now = self.clock.now();
        match &mut self.held_jog {
            Some(held) if held.is(direction, feed) => held.renew(now),
            // A new direction is a new hold. The UI releases one before
            // pressing another, so this only follows a lost release: stop,
            // and the next renewal starts the new one once the head is still.
            Some(_) => return self.jog_stop(),
            None => self.held_jog = Some(HeldJog::new(direction, feed, now)),
        }
        Ok(())
    }

    /// Cancels any jog and forgets the held one, with its unsent segments.
    fn jog_stop(&mut self) -> io::Result<()> {
        if self.held_jog.take().is_some() {
            self.outbox
                .retain(|(_, awaiting)| !matches!(awaiting, Awaiting::HeldJog));
        }
        self.jog_cancelled_at = self.polls;
        self.transport.write(&[self.dialect.jog_cancel()])
    }

    /// Sends a due test-fire off command, and keeps a held jog fed or
    /// cancels it once the UI stops renewing it.
    fn manual_timers(&mut self) -> io::Result<()> {
        let now = self.clock.now();
        if self.laser_off_at.is_some_and(|at| now >= at) {
            self.laser_off_at = None;
            let off = self.line(self.dialect.laser_off());
            // Ahead of anything else still waiting to go out.
            self.outbox.push_front((off, Awaiting::Quiet));
            self.firing.store(false, Ordering::SeqCst);
        }
        let Some(held) = &mut self.held_jog else {
            return Ok(());
        };
        if held.expired(now) {
            return self.jog_stop();
        }
        let settled = self.reports > self.jog_cancelled_at;
        let feed = held.feed();
        let segments = held.segments(self.status.last(), settled);
        for (dx, dy) in segments {
            let bytes = self.line(&self.dialect.jog_by(dx, dy, feed));
            self.outbox.push_back((bytes, Awaiting::HeldJog));
        }
        Ok(())
    }

    /// Sends the override bytes that move the reported overrides towards
    /// their targets, if any are due.
    fn nudge_overrides(&mut self) -> io::Result<()> {
        let current = self.status.last().and_then(|status| status.overrides);
        let dialect = self.dialect;
        let bytes = self
            .overrides
            .next(current, self.tick(), |kind| dialect.override_bytes(kind));
        if bytes.is_empty() {
            return Ok(());
        }
        self.transport.write(&bytes)
    }

    fn job_command(
        &mut self,
        command: impl FnOnce(&mut ActiveJob) -> Option<JobAction>,
    ) -> io::Result<()> {
        match self.job.as_mut().and_then(command) {
            Some(action) => self.act(action),
            None => Ok(()),
        }
    }

    fn act(&mut self, action: JobAction) -> io::Result<()> {
        match action {
            JobAction::FeedHold => self.transport.write(&[self.dialect.feed_hold()]),
            JobAction::CycleStart => self.transport.write(&[self.dialect.cycle_start()]),
            JobAction::SoftReset => self.transport.write(self.dialect.soft_reset()),
            JobAction::SpindleStop => self.transport.write(&[self.dialect.spindle_stop()]),
            JobAction::LaserOff { air } => {
                let off = self.line(self.dialect.laser_off());
                self.outbox.push_back((off, Awaiting::Quiet));
                if air {
                    let air_off = self.line(self.dialect.air_assist_off());
                    self.outbox.push_back((air_off, Awaiting::Quiet));
                }
                Ok(())
            }
        }
    }

    /// Writes queued lines, then job lines, while they fit the controller's
    /// RX buffer: GRBL's character-counting protocol. A line longer than the
    /// whole buffer goes out once nothing else is in flight.
    fn send_lines(&mut self) -> io::Result<()> {
        let tick = self.tick();
        let mut actions = std::mem::take(&mut self.actions);
        actions.extend(self.job.as_mut().and_then(|job| job.on_tick(tick)));
        for action in actions {
            self.act(action)?;
        }
        let ending = self.dialect.line_ending();
        loop {
            let in_flight: usize = self.pending.iter().map(|sent| sent.len).sum();
            let fits = |len: usize| in_flight == 0 || in_flight + len <= self.rx_buffer;
            if let Some((bytes, _)) = self.outbox.front() {
                if !fits(bytes.len()) {
                    return Ok(());
                }
                let (bytes, awaiting) = self.outbox.pop_front().unwrap();
                self.pending.push_back(Sent {
                    len: bytes.len(),
                    awaiting,
                    homing: bytes == self.line(self.dialect.home()),
                });
                self.transport.write(&bytes)?;
                continue;
            }
            let Some(job) = self.job.as_mut() else {
                return Ok(());
            };
            match job.peek() {
                Some(line) if fits(line.len() + ending.len()) => {}
                _ => return Ok(()),
            }
            let (number, command) = job.take().unwrap();
            let mut bytes = command.clone().into_bytes();
            bytes.extend_from_slice(ending);
            self.pending.push_back(Sent {
                len: bytes.len(),
                awaiting: Awaiting::Job { number, command },
                homing: false,
            });
            self.transport.write(&bytes)?;
        }
    }

    /// Reports the job's progress if its state changed or enough time has
    /// passed, and lets go of it once it is done.
    fn report_job(&mut self, on_event: &mut impl FnMut(MachineEvent)) {
        let Some(job) = &mut self.job else { return };
        let now = self.clock.now();
        let (progress, changed) = job.progress(now);
        let (reported_at, reported_acks) = self.job_reported;
        let moved = progress.acknowledged != reported_acks
            && now.saturating_sub(reported_at) >= JOB_PROGRESS_INTERVAL;
        if job.is_done() {
            self.job = None;
            self.job_active.store(false, Ordering::SeqCst);
        }
        if changed || moved {
            self.job_reported = (now, progress.acknowledged);
            on_event(MachineEvent::Job(progress));
        }
    }

    fn poll_interval(&self) -> Duration {
        match self.status.last() {
            Some(status) if status.state.is_moving() => MOVING_STATUS_INTERVAL,
            _ => STATUS_INTERVAL,
        }
    }

    fn emit_lines(&mut self, lines: Vec<String>, on_event: &mut impl FnMut(MachineEvent)) {
        for line in lines.into_iter().filter(|line| !line.is_empty()) {
            // A malformed report still reaches the console as a line.
            if let Some(Ok(report)) = self.dialect.parse_status(&line) {
                let fresh = report.overrides;
                // GRBL reports `A:` with `Ov:`, and leaves it out when
                // nothing is on; a report with neither says nothing.
                let accessories = (report.overrides.is_some() || report.accessories.is_some())
                    .then(|| report.accessories.unwrap_or_default());
                let status = self.status.update(report);
                self.reports = (self.reports + 1).min(self.polls);
                if status.state == MachineState::Alarm {
                    self.homed.store(false, Ordering::SeqCst);
                }
                let tick = self.tick();
                self.overrides.on_report(fresh, tick);
                let job = self.job.as_mut();
                if let Some(action) =
                    job.and_then(|job| job.on_status(status.state, accessories, tick))
                {
                    self.actions.push(action);
                }
                on_event(MachineEvent::Status(status));
            } else if let Some(line) = self.take_reply_line(line) {
                on_event(MachineEvent::Line(line));
            }
        }
    }

    /// Matches `line` to the command line it answers. Returns it unless a
    /// query took it.
    fn take_reply_line(&mut self, line: String) -> Option<String> {
        if self.dialect.parse_banner(&line).is_some() {
            // A reset discards whatever the controller hadn't answered,
            // status polls included.
            self.reports = self.polls;
            self.overrides.on_reset();
            for sent in self.pending.drain(..) {
                if let Awaiting::Query(query) = sent.awaiting {
                    let _ = query.reply.send(Err(QueryError::Reset));
                }
            }
            let tick = self.tick();
            if let Some(job) = &mut self.job {
                job.on_reset(tick);
            }
            return Some(line);
        }
        if let Some(reply) = self.dialect.parse_reply(&line) {
            let sent = self.pending.pop_front();
            if reply.is_ok() && sent.as_ref().is_some_and(|sent| sent.homing) {
                self.homed.store(true, Ordering::SeqCst);
            }
            return match sent.map(|sent| sent.awaiting) {
                Some(Awaiting::Query(query)) => {
                    let result = reply.map(|()| query.lines).map_err(QueryError::Rejected);
                    let _ = query.reply.send(result);
                    None
                }
                Some(Awaiting::Job { number, command }) => {
                    let shown = reply.is_err().then_some(line);
                    let tick = self.tick();
                    let job = self.job.as_mut();
                    if let Some(action) =
                        job.and_then(|job| job.answered(number, command, reply, tick))
                    {
                        self.actions.push(action);
                    }
                    shown
                }
                Some(Awaiting::Quiet) => reply.is_err().then_some(line),
                Some(Awaiting::HeldJog) => {
                    let first = reply.is_err() && self.held_jog.as_mut().is_some_and(HeldJog::fail);
                    first.then_some(line)
                }
                Some(Awaiting::Event) | None => Some(line),
            };
        }
        if let Reply::Alarm(alarm) = self.dialect.decode_reply(&line) {
            // An alarm may mean the position is lost: home again.
            self.homed.store(false, Ordering::SeqCst);
            if let Some(job) = &mut self.job {
                job.on_alarm(&format!("{line} ({})", alarm.title));
            }
        }
        match self.pending.front_mut() {
            Some(Sent {
                awaiting: Awaiting::Query(query),
                ..
            }) if !self.dialect.is_push_message(&line) => {
                query.lines.push(line);
                None
            }
            _ => Some(line),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc::Receiver;

    use bf_sim::{Event, Fault, Profile, Simulator};

    use super::*;
    use crate::machine::sim::{Lockstep, SimLink};
    use crate::machine::{JobState, MachineProfile, MachineState, OverrideKind, job_line};

    const WAIT: Duration = Duration::from_secs(5);

    /// The test's end of the event stream. In lockstep, the simulated clock
    /// waits at each status report until the test comes back for the next
    /// event, so whatever the test checks after a report, the simulator is
    /// still where the report says.
    struct Events {
        inbox: Receiver<MachineEvent>,
        lockstep: Option<Lockstep>,
        /// The test has a status report it hasn't finished with.
        holding: Cell<bool>,
    }

    impl Events {
        fn recv_timeout(&self, timeout: Duration) -> Result<MachineEvent, RecvTimeoutError> {
            if self.holding.replace(false)
                && let Some(lockstep) = &self.lockstep
            {
                lockstep.handled();
            }
            let event = self.inbox.recv_timeout(timeout)?;
            self.holding.set(matches!(event, MachineEvent::Status(_)));
            Ok(event)
        }

        /// Every event until the connection's handler ends.
        fn iter(&self) -> impl Iterator<Item = MachineEvent> + '_ {
            std::iter::from_fn(|| self.recv_timeout(WAIT).ok())
        }
    }

    /// Opens a connection in lockstep with the test thread (see [`Events`]).
    fn open(link: &SimLink) -> (Connection, Events) {
        open_events(link, Some(link.lockstep()))
    }

    /// Opens a connection whose clock runs free of the test thread, for
    /// tests that wait on simulated time without reading events meanwhile.
    fn open_free(link: &SimLink) -> (Connection, Events) {
        open_events(link, None)
    }

    fn open_events(link: &SimLink, lockstep: Option<Lockstep>) -> (Connection, Events) {
        let (events, inbox) = mpsc::channel();
        let connection = Connection::open(link.transport(), link.clock(), move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        let events = Events {
            inbox,
            lockstep,
            holding: Cell::new(false),
        };
        (connection, events)
    }

    impl Events {
        /// Events from a handler of the test's own, clock running free.
        fn free(inbox: Receiver<MachineEvent>) -> Self {
            Events {
                inbox,
                lockstep: None,
                holding: Cell::new(false),
            }
        }
    }

    /// The next line, skipping status reports.
    fn next_line(inbox: &Events) -> String {
        loop {
            match inbox.recv_timeout(WAIT).unwrap() {
                MachineEvent::Line(line) => return line,
                MachineEvent::Status(_) | MachineEvent::Job(_) => {}
                other => panic!("expected a line, got {other:?}"),
            }
        }
    }

    fn next_status(inbox: &Events) -> MachineStatus {
        loop {
            match inbox.recv_timeout(WAIT).unwrap() {
                MachineEvent::Status(status) => return status,
                MachineEvent::Line(_) | MachineEvent::Job(_) => {}
                other => panic!("expected a status, got {other:?}"),
            }
        }
    }

    /// Simulated times at which `?` reached the controller.
    fn status_queries(link: &SimLink) -> Vec<Duration> {
        link.sim()
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::Received { at, bytes } if bytes == b"?" => Some(*at),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sends_lines_with_newline_only_and_streams_replies() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, inbox) = open(&link);
        assert_eq!(connection.machine().version, "1.1h");

        connection.send_line("$G").unwrap();

        assert!(next_line(&inbox).starts_with("[GC:"));
        assert_eq!(next_line(&inbox), "ok");
        let sent: Vec<_> = link
            .sim()
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::Received { bytes, .. } if bytes != b"?" => Some(bytes.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(sent, [b"$G\n".to_vec()]);
    }

    #[test]
    fn polls_status_at_five_hertz_while_idle() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (_connection, inbox) = open(&link);

        let first = next_status(&inbox);
        assert_eq!(first.state, MachineState::Idle);
        assert_eq!(first.machine_position, Some(Default::default()));
        assert_eq!(
            first.work_position,
            Some(Default::default()),
            "first report has WCO"
        );
        for _ in 0..4 {
            next_status(&inbox);
        }

        let queries = status_queries(&link);
        let gaps: Vec<_> = queries.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.len() >= 4, "{queries:?}");
        for gap in gaps {
            assert!(
                gap >= STATUS_INTERVAL && gap < STATUS_INTERVAL + 2 * POLL,
                "{gap:?}"
            );
        }
    }

    #[test]
    fn position_updates_live_while_the_machine_moves() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, inbox) = open(&link);
        next_status(&inbox);

        // 20 mm at 600 mm/min takes about two seconds.
        connection.send_line("G1 X20 F600").unwrap();
        let mut moving = Vec::new();
        let last = loop {
            let status = next_status(&inbox);
            if status.state == MachineState::Run {
                moving.push(status);
            } else if !moving.is_empty() {
                break status;
            }
        };

        let xs: Vec<f64> = moving
            .iter()
            .map(|status| status.work_position.unwrap().x)
            .collect();
        assert!(xs.len() >= 10, "{xs:?}");
        assert!(xs.windows(2).all(|w| w[0] <= w[1]), "{xs:?}");
        assert!(xs.iter().any(|&x| x > 5.0 && x < 15.0), "{xs:?}");
        assert!(moving.iter().any(|status| status.feed > 0.0));
        assert_eq!(last.state, MachineState::Idle);
        assert_eq!(last.machine_position.unwrap().x, 20.0);

        // Polls run twice as fast while the machine moves.
        let queries = status_queries(&link);
        let fastest = queries.windows(2).map(|w| w[1] - w[0]).min().unwrap();
        assert!(fastest < STATUS_INTERVAL, "{fastest:?}");
    }

    #[test]
    fn derives_work_position_after_an_offset_change() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, inbox) = open(&link);
        next_status(&inbox);

        connection.send_line("G10 L20 P1 X5 Y-5").unwrap();
        let status = loop {
            let status = next_status(&inbox);
            if status.work_position.is_some_and(|p| p.x == 5.0) {
                break status;
            }
        };
        assert_eq!(status.machine_position.unwrap().x, 0.0);
        assert_eq!(status.work_offset.unwrap().x, -5.0);
        assert_eq!(status.work_position.unwrap().y, -5.0);
    }

    #[test]
    fn forwards_lines_that_followed_the_banner() {
        let mut profile = Profile::grbl_defaults();
        profile.settings.set(22, 1.0).unwrap();
        let link = SimLink::new(Simulator::new(profile));
        let (_connection, inbox) = open(&link);

        assert_eq!(next_line(&inbox), "[MSG:'$H'|'$X' to unlock]");
    }

    #[test]
    fn queries_collect_their_reply_without_reaching_the_events() {
        let mut profile = Profile::grbl_defaults();
        profile.settings.set(32, 1.0).unwrap();
        let link = SimLink::new(Simulator::new(profile));
        let (connection, inbox) = open(&link);
        let handle = connection.handle();

        connection.send_line("$G").unwrap();
        let settings = handle.query("$$", WAIT).unwrap();

        assert_eq!(settings.len(), 34);
        assert!(settings.contains(&"$32=1".to_string()), "{settings:?}");
        // The plain line's reply still arrives as events, in order.
        assert!(next_line(&inbox).starts_with("[GC:"));
        assert_eq!(next_line(&inbox), "ok");
        handle.send_line("$I").unwrap();
        assert!(next_line(&inbox).starts_with("[VER:"), "no $$ lines leaked");
    }

    #[test]
    fn writing_a_setting_round_trips() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, _inbox) = open(&link);
        let handle = connection.handle();

        assert_eq!(handle.query("$32=1", WAIT).unwrap(), Vec::<String>::new());

        let settings = handle.query("$$", WAIT).unwrap();
        assert!(settings.contains(&"$32=1".to_string()), "{settings:?}");
        assert_eq!(link.sim().settings().get(32), Some(1.0));
    }

    #[test]
    fn rejected_queries_explain_the_error() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, _inbox) = open(&link);

        let error = connection.handle().query("$110=-5", WAIT).unwrap_err();

        assert_eq!(
            error,
            QueryError::Rejected(
                "GRBL error 4: Negative value received for an expected positive value.".into()
            )
        );
    }

    #[test]
    fn a_reset_fails_queries_waiting_for_a_reply() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, _inbox) = open(&link);
        // A long dwell holds back the `ok` until the reset.
        let handle = connection.handle();
        let waiting = thread::spawn(move || handle.query("G4 P1000", WAIT));
        thread::sleep(Duration::from_millis(50));
        connection.send_realtime(0x18).unwrap();

        assert_eq!(waiting.join().unwrap(), Err(QueryError::Reset));
    }

    #[test]
    fn refuses_lines_with_line_breaks() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, _inbox) = open(&link);

        assert_eq!(connection.send_line("G0 X1\r\n"), Err(SendError::LineBreak));
    }

    #[test]
    fn reports_a_pulled_cable_as_a_disconnect() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, inbox) = open(&link);

        link.unplug();

        match inbox.recv_timeout(WAIT).unwrap() {
            MachineEvent::Disconnected {
                reason: Some(reason),
            } => {
                assert!(reason.contains("unplugged"), "{reason}");
            }
            other => panic!("expected a disconnect, got {other:?}"),
        }
        assert_eq!(connection.send_line("?"), Err(SendError::Closed));
        assert_eq!(
            connection.handle().query("$$", WAIT),
            Err(QueryError::Send(SendError::Closed))
        );
    }

    #[test]
    fn closing_reports_a_disconnect_without_a_reason() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let (connection, inbox) = open(&link);

        connection.close();

        // A status polled before the close may arrive first.
        let last = inbox.iter().last();
        assert_eq!(last, Some(MachineEvent::Disconnected { reason: None }));
    }

    // ---- jobs ----

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../bf-sim/tests/fixtures");

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(FIXTURES).join(name)).unwrap()
    }

    fn laser_link() -> SimLink {
        SimLink::new(Simulator::new(Profile::diode_laser()))
    }

    /// Opens the connection and waits for the first status report, so the
    /// job's readiness check has something to go on.
    fn open_ready(link: &SimLink) -> (Connection, Events) {
        let (connection, inbox) = open(link);
        next_status(&inbox);
        (connection, inbox)
    }

    /// The next job progress, skipping other events.
    fn next_progress(inbox: &Events) -> JobProgress {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            assert!(std::time::Instant::now() < deadline, "no job progress");
            match inbox.recv_timeout(WAIT).unwrap() {
                MachineEvent::Job(progress) => return progress,
                MachineEvent::Disconnected { reason } => panic!("disconnected: {reason:?}"),
                _ => {}
            }
        }
    }

    /// Progress until the job is done. `on` sees each one first, to pause,
    /// resume or stop the job.
    fn follow_job(inbox: &Events, mut on: impl FnMut(&JobProgress)) -> Vec<JobProgress> {
        let mut seen = Vec::new();
        loop {
            let progress = next_progress(inbox);
            on(&progress);
            let done = progress.state.is_done();
            seen.push(progress);
            if done {
                return seen;
            }
        }
    }

    /// The first status report for which `until` holds.
    fn status_where(inbox: &Events, until: impl Fn(&MachineStatus) -> bool) -> MachineStatus {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            let status = next_status(inbox);
            if until(&status) {
                return status;
            }
            assert!(std::time::Instant::now() < deadline, "last {status:?}");
        }
    }

    /// Progress until `until` holds for one.
    fn wait_for_progress(inbox: &Events, until: impl Fn(&JobProgress) -> bool) -> JobProgress {
        loop {
            let progress = next_progress(inbox);
            if until(&progress) {
                return progress;
            }
            assert!(!progress.state.is_done(), "{progress:?}");
        }
    }

    /// Bytes of command lines the controller received, without realtime
    /// bytes such as `?`, `!`, `~` and the soft reset.
    fn received_lines(link: &SimLink) -> String {
        let sim = link.sim();
        let mut lines = String::new();
        for event in sim.events() {
            if let Event::Received { bytes, .. } = event
                && bytes.ends_with(b"\n")
            {
                lines.push_str(&String::from_utf8_lossy(bytes));
            }
        }
        lines
    }

    /// Realtime bytes received, other than status queries.
    fn received_realtime(link: &SimLink) -> Vec<u8> {
        let sim = link.sim();
        let events = sim.events().iter();
        events
            .filter_map(|event| match event {
                Event::Received { bytes, .. } if bytes.len() == 1 && bytes[0] != b'?' => {
                    Some(bytes[0])
                }
                _ => None,
            })
            .collect()
    }

    /// The most bytes of command lines that were ever in the controller's
    /// buffer awaiting `ok`/`error:`, rebuilt from the wire.
    fn peak_in_flight(link: &SimLink) -> usize {
        let sim = link.sim();
        let mut in_flight = VecDeque::new();
        let mut peak = 0;
        let mut replies = String::new();
        for event in sim.events() {
            match event {
                Event::Received { bytes, .. } if bytes.ends_with(b"\n") => {
                    in_flight.push_back(bytes.len());
                    peak = peak.max(in_flight.iter().sum());
                }
                Event::Sent { bytes, .. } => {
                    replies.push_str(&String::from_utf8_lossy(bytes));
                    while let Some(end) = replies.find("\r\n") {
                        let line: String = replies.drain(..end + 2).collect();
                        if line == "ok\r\n" || line.starts_with("error:") {
                            in_flight.pop_front();
                        } else if line.starts_with("Grbl ") {
                            in_flight.clear();
                        }
                    }
                }
                _ => {}
            }
        }
        peak
    }

    fn overflows(link: &SimLink) -> usize {
        let sim = link.sim();
        let events = sim.events().iter();
        events
            .filter(|event| matches!(event, Event::RxOverflow { .. }))
            .count()
    }

    /// Cut distance, travel distance and peak power from the simulator's
    /// motion trace, as `bf_sim::check` reports them.
    fn traced(link: &SimLink) -> (f64, f64, f64) {
        let sim = link.sim();
        let (mut cut, mut travel, mut power) = (0.0f64, 0.0f64, 0.0f64);
        for segment in sim.trace() {
            let length = (0..3)
                .map(|axis| (segment.end[axis] - segment.start[axis]).powi(2))
                .sum::<f64>()
                .sqrt();
            if segment.power > 0.0 {
                cut += length;
            } else {
                travel += length;
            }
            power = power.max(segment.power);
        }
        let round = |mm: f64| (mm * 1000.0).round() / 1000.0;
        (round(cut), round(travel), power)
    }

    /// A burn: one GCode line's lased motion, from the trace.
    #[derive(Debug, Clone, Copy)]
    struct Burn {
        from: [f64; 2],
        to: [f64; 2],
        /// Peak laser power, 0 to 1.
        power: f64,
    }

    /// The trace's lased moves, one per GCode line: feed moves grouped by
    /// the line that queued them, so a move cut short by a hold and resumed
    /// is still one burn.
    fn burns(link: &SimLink) -> Vec<Burn> {
        let sim = link.sim();
        let mut burns: Vec<(u64, Burn)> = Vec::new();
        for segment in sim.trace() {
            if segment.rapid || segment.is_stationary() {
                continue;
            }
            let (from, to) = (
                [segment.start[0], segment.start[1]],
                [segment.end[0], segment.end[1]],
            );
            match burns.last_mut() {
                Some((line, burn)) if *line == segment.line => {
                    burn.to = to;
                    burn.power = burn.power.max(segment.power);
                }
                _ => burns.push((
                    segment.line,
                    Burn {
                        from,
                        to,
                        power: segment.power,
                    },
                )),
            }
        }
        burns
            .into_iter()
            .map(|(_, burn)| burn)
            .filter(|burn| burn.power > 0.0)
            .collect()
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    /// Override bytes (0x90-0x9D) the controller received, in order.
    fn received_overrides(link: &SimLink) -> Vec<u8> {
        let sim = link.sim();
        let mut bytes = Vec::new();
        for event in sim.events() {
            if let Event::Received {
                bytes: received, ..
            } = event
            {
                bytes.extend(received.iter().filter(|b| (0x90..=0x9D).contains(*b)));
            }
        }
        bytes
    }

    /// A job that burns along X for about `seconds`, a second per line.
    fn long_burn(seconds: u32) -> Job {
        let mut lines = vec!["G21 G90".to_string(), "M4 S500".to_string()];
        for step in 1..=seconds {
            lines.push(format!("G1 X{} F600", step * 10));
        }
        lines.extend(["M5".to_string(), "G0 X0".to_string()]);
        Job::from_lines("burn.gcode", lines)
    }

    /// A job that runs `setup` (such as `M8` and `M4 S500`), then burns
    /// along X for about `seconds`, a second per line, then rapids home. It
    /// leaves the laser and air as `setup` set them, so whatever turns them
    /// off afterwards wasn't the job.
    fn burn(setup: &[&str], seconds: u32) -> Job {
        let mut lines = vec!["G21 G90".to_string()];
        lines.extend(setup.iter().map(ToString::to_string));
        for step in 1..=seconds {
            lines.push(format!("G1 X{} F600", step * 10));
        }
        lines.push("G0 X0".to_string());
        Job::from_lines("burn.gcode", lines)
    }

    /// How many times the controller received `byte` on its own.
    fn realtime_count(link: &SimLink, byte: u8) -> usize {
        received_realtime(link)
            .iter()
            .filter(|&&b| b == byte)
            .count()
    }

    fn parser_state(connection: &Connection) -> String {
        connection.handle().query("$G", WAIT).unwrap().join("")
    }

    #[test]
    fn streams_the_sample_job_exactly() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let gcode = fixture("sample-job.gc");
        let commands: Vec<String> = gcode.lines().filter_map(job_line).collect();

        connection
            .handle()
            .start_job(Job::from_lines("sample-job.gc", gcode.lines()))
            .unwrap();
        let progress = follow_job(&inbox, |_| {});

        let last = progress.last().unwrap();
        assert_eq!(last.state, JobState::Finished, "{progress:?}");
        assert_eq!(last.name, "sample-job.gc");
        assert_eq!(last.total, Some(commands.len() as u32));
        assert_eq!(last.acknowledged, commands.len() as u32);
        assert!(last.elapsed_ms > 10_000, "{last:?}");
        assert_eq!(progress[0].state, JobState::Running);
        assert!(
            progress
                .windows(2)
                .all(|w| w[0].acknowledged <= w[1].acknowledged),
            "{progress:?}"
        );
        // Every command line went out once, in order, without comments.
        assert_eq!(received_lines(&link), format!("{}\n", commands.join("\n")));
        // Character counting kept several lines in the buffer, never more
        // than it holds.
        assert_eq!(overflows(&link), 0);
        let peak = peak_in_flight(&link);
        assert!(peak > 64 && peak <= 128, "{peak}");
        // The machine did what the headless check says the job does.
        let report = bf_sim::check(&gcode, Profile::diode_laser());
        assert!(report.passed(), "{report}");
        assert_eq!(
            traced(&link),
            (
                report.cut_distance_mm,
                report.travel_distance_mm,
                report.max_power
            )
        );
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(!connection.handle().job_active());
    }

    #[test]
    fn streams_every_fixture_without_overflowing_the_buffer() {
        let mut names: Vec<_> = std::fs::read_dir(FIXTURES)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".gc"))
            .collect();
        names.sort();
        assert!(names.len() >= 8, "{names:?}");
        for name in names {
            let link = laser_link();
            let (connection, inbox) = open_ready(&link);
            let handle = connection.handle();
            handle
                .start_job(Job::from_lines(&name, fixture(&name).lines()))
                .unwrap();

            // Continue past rejected lines, as the user may.
            let progress = follow_job(&inbox, |progress| {
                if matches!(progress.state, JobState::Rejected { .. }) {
                    handle.resume_job().unwrap();
                }
            });

            let state = &progress.last().unwrap().state;
            match name.as_str() {
                "bad-alarm.gc" => assert!(matches!(state, JobState::Failed { .. }), "{state:?}"),
                _ => assert_eq!(*state, JobState::Finished, "{name}"),
            }
            assert_eq!(overflows(&link), 0, "{name}");
        }
    }

    #[test]
    fn pauses_with_a_feed_hold_and_resumes_with_cycle_start() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.start_job(long_burn(5)).unwrap();
        wait_for_progress(&inbox, |p| p.acknowledged >= 3);
        // An `ok` means a line was buffered, not that the head moved; on a
        // slow runner the hold could land before any motion.
        status_where(&inbox, |s| s.machine_position.is_some_and(|p| p.x > 0.0));

        handle.pause_job().unwrap();
        wait_for_progress(&inbox, |p| p.state == JobState::Paused);
        let held = status_where(&inbox, |s| s.state == MachineState::Hold { code: 0 })
            .machine_position
            .unwrap()
            .x;
        for _ in 0..5 {
            let status = next_status(&inbox);
            assert_eq!(status.state, MachineState::Hold { code: 0 });
            assert_eq!(status.machine_position.unwrap().x, held);
        }
        assert!(held > 0.0 && held < 50.0, "{held}");

        handle.resume_job().unwrap();
        let progress = follow_job(&inbox, |_| {});

        assert_eq!(progress[0].state, JobState::Running);
        let last = progress.last().unwrap();
        assert_eq!(last.state, JobState::Finished);
        assert_eq!(last.acknowledged, 9);
        assert_eq!(received_realtime(&link), b"!~");
        assert_eq!(link.sim().machine_position()[0], 0.0);
    }

    #[test]
    fn pause_and_resume_keep_the_head_and_laser_state() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.start_job(long_burn(6)).unwrap();
        // Burning at full speed mid-line: M4 S500 is half power at cruise.
        status_where(&inbox, |s| {
            s.state == MachineState::Run
                && s.machine_position.is_some_and(|p| p.x > 12.0 && p.x < 18.0)
        });
        assert!((link.sim().laser_power() - 0.5).abs() < 1e-9);

        handle.pause_job().unwrap();
        let held = status_where(&inbox, |s| s.state == MachineState::Hold { code: 0 });
        let at = held.machine_position.unwrap();
        // GRBL's laser mode turns the laser off for the hold...
        assert_eq!(link.sim().laser_power(), 0.0);
        for _ in 0..10 {
            let status = next_status(&inbox);
            assert_eq!(status.state, MachineState::Hold { code: 0 });
            assert_eq!(status.machine_position.unwrap(), at);
            assert_eq!(link.sim().laser_power(), 0.0);
        }

        // GRBL's reports show it off, so the spindle stop override, which
        // would toggle it back on, never went out.
        assert_eq!(realtime_count(&link, 0x9E), 0);

        // ...and resume picks up where it stopped, at the same power.
        handle.resume_job().unwrap();
        status_where(&inbox, |s| {
            s.state == MachineState::Run && s.machine_position.is_some_and(|p| p.x > at.x + 3.0)
        });
        assert!((link.sim().laser_power() - 0.5).abs() < 1e-9);
        let progress = follow_job(&inbox, |_| {});
        assert_eq!(progress.last().unwrap().state, JobState::Finished);

        // One unbroken burn per line, each at the programmed power: the
        // held line resumed from the point it stopped at.
        let burns = burns(&link);
        assert_eq!(burns.len(), 6, "{burns:?}");
        let mut x = 0.0;
        for burn in &burns {
            assert!(near(burn.from, [x, 0.0]), "{burn:?}");
            x += 10.0;
            assert!(near(burn.to, [x, 0.0]), "{burn:?}");
            assert!((burn.power - 0.5).abs() < 1e-9, "{burn:?}");
        }
        // The feed moves join up end to start, through the hold.
        let trace = link.sim().trace().to_vec();
        let paused = trace
            .iter()
            .filter(|s| !s.rapid && !s.is_stationary())
            .collect::<Vec<_>>();
        assert!(
            paused
                .windows(2)
                .all(|w| near([w[0].end[0], w[0].end[1]], [w[1].start[0], w[1].start[1]])),
            "the burn has a gap"
        );
        assert!(
            paused
                .iter()
                .any(|s| near([s.end[0], s.end[1]], [at.x, at.y])),
            "no move ends where the hold stopped"
        );
    }

    #[test]
    fn overrides_converge_on_their_targets_and_read_back() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.start_job(long_burn(300)).unwrap();
        status_where(&inbox, |s| s.state == MachineState::Run);

        handle.set_override(OverrideKind::Feed, 137).unwrap();
        handle.set_override(OverrideKind::Spindle, 45).unwrap();
        let status = status_where(&inbox, |s| {
            s.overrides
                .is_some_and(|o| o.feed == 137 && o.spindle == 45)
        });
        assert_eq!(status.overrides.unwrap().rapid, 100);
        // The job's start reset them all, then the fewest nudges: tens then
        // ones.
        let mut expected = vec![0x90, 0x95, 0x99];
        expected.extend([0x91; 4]);
        expected.extend([0x94; 3]);
        expected.extend([0x9B; 5]);
        expected.extend([0x9D; 5]);
        assert_eq!(received_overrides(&link), expected);
        let sim_overrides = link.sim().overrides();
        assert_eq!((sim_overrides.feed, sim_overrides.spindle), (137, 45));
        // Laser power follows the spindle override live.
        status_where(&inbox, |_| {
            let power = link.sim().laser_power();
            power > 0.0 && power < 0.4
        });

        // A slider dragged faster than GRBL reports still lands on the last
        // value, never past the range it swept.
        for feed in [150, 170, 190, 120] {
            handle.set_override(OverrideKind::Feed, feed).unwrap();
        }
        status_where(&inbox, |s| s.overrides.is_some_and(|o| o.feed == 120));
        for _ in 0..5 {
            assert_eq!(next_status(&inbox).overrides.unwrap().feed, 120);
        }
        handle.set_override(OverrideKind::Spindle, 100).unwrap();
        status_where(&inbox, |s| s.overrides.is_some_and(|o| o.spindle == 100));
        assert_eq!(received_overrides(&link).last(), Some(&0x99), "a reset");

        // Stopping resets the controller, overrides and all; nothing
        // fights that afterwards.
        handle.stop_job().unwrap();
        follow_job(&inbox, |_| {});
        let sent = received_overrides(&link).len();
        status_where(&inbox, |s| {
            s.overrides
                .is_some_and(|o| o.feed == 100 && o.spindle == 100)
        });
        for _ in 0..5 {
            next_status(&inbox);
        }
        assert_eq!(received_overrides(&link).len(), sent);
    }

    #[test]
    fn a_generated_design_burns_its_toolpath() {
        use crate::gcode::{JobPlacement, run};
        use crate::plan::Move;
        use crate::project::{CutLayer, DesignDocument, SceneObject};

        let mut engrave = CutLayer::new("eng", "Engrave", "#0000ff");
        engrave.speed = 600.0;
        engrave.max_power = 40.0;
        engrave.passes = 2;
        let mut mark = CutLayer::new("mark", "Mark", "#ff0000");
        mark.speed = 900.0;
        mark.max_power = 15.0;
        mark.constant_power = true;
        let rect = |id: &str, layer: &str, x: f64, y: f64, w: f64, h: f64| SceneObject::Rect {
            id: id.into(),
            name: id.into(),
            layer_id: layer.into(),
            transform: [1.0, 0.0, 0.0, 1.0, x, y],
            width: w,
            height: h,
            corner_radius: 0.0,
        };
        let document = DesignDocument {
            version: 1,
            units: "mm".into(),
            layers: vec![engrave, mark],
            objects: vec![
                rect("a", "eng", 10.0, 10.0, 20.0, 15.0),
                rect("b", "mark", 40.0, 10.0, 12.0, 12.0),
            ],
        };
        let profile = MachineProfile {
            max_spindle: 1000.0,
            laser_mode: true,
            ..MachineProfile::default()
        };
        let design = run(&document, &profile, JobPlacement::default(), None, "Tag").unwrap();

        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        connection
            .handle()
            .start_job(Job::from_lines("Tag", design.job.lines()))
            .unwrap();
        let progress = follow_job(&inbox, |_| {});
        let last = progress.last().unwrap();
        assert_eq!(last.state, JobState::Finished, "{last:?}");
        assert_eq!(last.acknowledged, design.job.summary.lines);
        assert_eq!(
            *design.plan.move_lines.last().unwrap(),
            design.job.summary.lines - 2,
            "only M5 and M9 follow the last move"
        );

        // Every cut in the toolpath was burned, in order, from where it
        // starts to where it ends, at its power; nothing else was.
        let placement = design.plan.placement;
        let mut at = placement.to_work(design.plan.toolpath.plan.start);
        let mut planned = Vec::new();
        for step in design.plan.toolpath.plan.moves() {
            let (to, power) = match *step {
                Move::Rapid { to } => (placement.to_work(to), None),
                Move::Cut { to, power, .. } => (placement.to_work(to), Some(power / 100.0)),
            };
            if let Some(power) = power {
                planned.push(Burn {
                    from: [at.x, at.y],
                    to: [to.x, to.y],
                    power,
                });
            }
            at = to;
        }
        let burned = burns(&link);
        assert_eq!(burned.len(), planned.len(), "{burned:?}");
        assert_eq!(planned.len(), 4 * 2 + 4);
        for (burn, plan) in burned.iter().zip(&planned) {
            assert!(near(burn.from, plan.from), "{burn:?} vs {plan:?}");
            assert!(near(burn.to, plan.to), "{burn:?} vs {plan:?}");
            assert!(
                (burn.power - plan.power).abs() < 1e-9,
                "{burn:?} vs {plan:?}"
            );
        }
        assert_eq!(link.sim().laser_power(), 0.0);
    }

    #[test]
    fn stop_turns_the_laser_off_and_leaves_the_machine_idle_and_unlocked() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.start_job(long_burn(30)).unwrap();
        // Burning mid-move.
        status_where(&inbox, |_| link.sim().laser_power() > 0.0);

        handle.stop_job().unwrap();
        let progress = follow_job(&inbox, |_| {});

        let states: Vec<_> = progress.iter().map(|p| p.state.clone()).collect();
        assert!(states.contains(&JobState::Stopping), "{states:?}");
        let last = progress.last().unwrap();
        assert_eq!(last.state, JobState::Stopped { warning: None });
        assert!(last.acknowledged < 30, "{last:?}");
        // Hold, then reset at rest so GRBL raises no alarm, then M5.
        assert_eq!(received_realtime(&link), [b'!', 0x18]);
        assert!(received_lines(&link).ends_with("M5\n"));
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(parser_state(&connection).contains("M5"));
        assert_eq!(next_status(&inbox).state, MachineState::Idle);
        // Unlocked: motion is accepted straight away.
        assert_eq!(handle.query("G0 X1", WAIT), Ok(Vec::new()));
    }

    #[test]
    fn a_stopped_job_has_sent_its_m5_before_it_reports_stopped() {
        let link = laser_link();
        // Look at the wire from the owner thread, as the event is emitted:
        // once a job reports done, the app allows a disconnect.
        let wire = link.clone();
        let (events, inbox) = mpsc::channel();
        let (sent_when_done, done) = mpsc::channel();
        let connection = Connection::open(link.transport(), link.clock(), move |event| {
            if let MachineEvent::Job(progress) = &event
                && progress.state.is_done()
            {
                let _ = sent_when_done.send(received_lines(&wire));
            }
            let _ = events.send(event);
        })
        .unwrap();
        let inbox = Events::free(inbox);
        next_status(&inbox);
        let handle = connection.handle();
        handle.start_job(long_burn(30)).unwrap();
        status_where(&inbox, |_| link.sim().laser_power() > 0.0);

        handle.stop_job().unwrap();
        let sent = done.recv_timeout(WAIT).unwrap();
        assert!(sent.ends_with("M5\n"), "reported stopped after {sent:?}");
    }

    #[test]
    fn an_error_mid_stream_holds_the_job_until_continued() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        let job = Job::from_lines(
            "error.gcode",
            ["G21 G90", "G1 X10 F600", "G99", "G1 X20", "G0 X0"],
        );
        handle.start_job(job).unwrap();

        // The error reaches the console too.
        let mut lines = Vec::new();
        let rejected = loop {
            match inbox.recv_timeout(WAIT).unwrap() {
                MachineEvent::Line(line) => lines.push(line),
                MachineEvent::Job(p) if matches!(p.state, JobState::Rejected { .. }) => break p,
                _ => {}
            }
        };
        assert_eq!(lines, ["error:20"]);
        let JobState::Rejected {
            line,
            command,
            message,
        } = rejected.state
        else {
            unreachable!()
        };
        assert_eq!((line, command.as_str()), (3, "G99"));
        assert!(message.contains("error 20"), "{message}");
        let held = status_where(&inbox, |s| s.state == MachineState::Hold { code: 0 });
        assert!(held.machine_position.unwrap().x < 20.0);

        handle.resume_job().unwrap();
        let progress = follow_job(&inbox, |_| {});

        let last = progress.last().unwrap();
        assert_eq!(last.state, JobState::Finished);
        assert_eq!(last.acknowledged, 5);
        assert_eq!(link.sim().machine_position()[0], 0.0);
    }

    #[test]
    fn a_scenario_fault_mid_job_reaches_the_job() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        link.sim()
            .run_scenario("after 2: error 20".parse().unwrap());
        let job = Job::from_lines("fault.gcode", ["G21 G90", "G1 X10 F600", "G1 X20"]);
        handle.start_job(job).unwrap();

        let rejected = wait_for_progress(&inbox, |p| matches!(p.state, JobState::Rejected { .. }));
        let JobState::Rejected { line, command, .. } = rejected.state else {
            unreachable!()
        };
        assert_eq!((line, command.as_str()), (3, "G1 X20"));
    }

    #[test]
    fn an_injected_disconnect_unplugs_the_link() {
        let link = laser_link();
        let (_connection, inbox) = open_ready(&link);

        link.sim().inject(Fault::Disconnect);

        let reason = loop {
            if let MachineEvent::Disconnected { reason } = inbox.recv_timeout(WAIT).unwrap() {
                break reason;
            }
        };
        assert!(reason.unwrap().contains("unplugged"));
    }

    #[test]
    fn stopping_a_rejected_job_aborts_it() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        let job = Job::from_lines("error.gcode", ["M4 S500", "G1 X10 F600", "G99", "G1 X20"]);
        handle.start_job(job).unwrap();
        wait_for_progress(&inbox, |p| matches!(p.state, JobState::Rejected { .. }));

        handle.stop_job().unwrap();
        let progress = follow_job(&inbox, |_| {});

        assert_eq!(
            progress.last().unwrap().state,
            JobState::Stopped { warning: None }
        );
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(link.sim().machine_position()[0] < 20.0);
        assert!(parser_state(&connection).contains("M5"));
    }

    #[test]
    fn polls_and_queries_pair_their_replies_during_a_job() {
        let link = laser_link();
        // The query waits behind job lines, which takes simulated time, while
        // status reports go unread.
        let (connection, inbox) = open_free(&link);
        next_status(&inbox);
        let handle = connection.handle();
        handle.start_job(long_burn(30)).unwrap();
        wait_for_progress(&inbox, |p| p.acknowledged >= 2);

        assert_eq!(connection.send_line("G0 X0"), Err(SendError::JobRunning));
        assert_eq!(
            handle.start_job(long_burn(1)).unwrap_err(),
            JobError::AlreadyRunning
        );
        let state = handle.query("$G", WAIT).unwrap();
        // It went out behind job lines, so it sees the job's modes. Where it
        // lands depends on how much of the job was already buffered: on a slow
        // runner the whole job can be ahead of it, ending in G0 and M5. S500
        // and F600 hold either way.
        assert_eq!(state.len(), 1, "{state:?}");
        assert!(
            state[0].starts_with("[GC:")
                && state[0].contains(" S500")
                && state[0].contains(" F600"),
            "{state:?}"
        );
        assert_eq!(next_status(&inbox).state, MachineState::Run);
        let progress = follow_job(&inbox, |_| {});

        let last = progress.last().unwrap();
        assert_eq!(last.state, JobState::Finished);
        assert_eq!(last.acknowledged, 34);
        assert_eq!(link.sim().machine_position()[0], 0.0);
        // Afterwards the console works again.
        connection.send_line("$I").unwrap();
        assert!(next_line(&inbox).starts_with("[VER:"));
    }

    #[test]
    fn refuses_to_start_a_job_in_alarm() {
        let mut profile = Profile::diode_laser();
        profile.settings.set(22, 1.0).unwrap();
        let link = SimLink::new(Simulator::new(profile));
        let (connection, _inbox) = open_ready(&link);

        let refused = connection.handle().start_job(long_burn(1));

        assert!(matches!(refused, Err(JobError::NotReady(_))), "{refused:?}");
        assert!(!connection.handle().job_active());
    }

    #[test]
    fn closing_under_a_job_resets_the_controller() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        connection.handle().start_job(long_burn(30)).unwrap();
        wait_for_progress(&inbox, |p| p.acknowledged >= 3);

        connection.close();

        let progress = follow_job(&inbox, |_| {});
        assert!(matches!(
            progress.last().unwrap().state,
            JobState::Failed { .. }
        ));
        let mut sim = link.sim();
        sim.advance(Duration::from_millis(100));
        assert_eq!(sim.laser_power(), 0.0);
        assert!(sim.events().iter().any(
            |event| matches!(event, Event::Received { bytes, .. } if bytes.as_slice() == [0x18])
        ));
    }

    // ---- laser-off guarantees ----

    fn sim_link(settings: &[(u16, f64)]) -> SimLink {
        let mut profile = Profile::diode_laser();
        for &(id, value) in settings {
            profile.settings.set(id, value).unwrap();
        }
        SimLink::new(Simulator::new(profile))
    }

    #[test]
    fn pausing_without_laser_mode_turns_the_laser_off_with_the_spindle_stop_override() {
        // Without laser mode GRBL keeps an M3 laser on through a hold.
        let link = sim_link(&[(32, 0.0)]);
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.start_job(burn(&["M3 S500"], 6)).unwrap();
        status_where(&inbox, |s| {
            s.state == MachineState::Run
                && s.machine_position.is_some_and(|p| p.x > 12.0 && p.x < 18.0)
        });
        assert!(link.sim().laser_power() > 0.0);

        handle.pause_job().unwrap();
        status_where(&inbox, |s| s.state == MachineState::Hold { code: 0 });
        // Once a report shows the laser still on at rest, 0x9E turns it off.
        status_where(&inbox, |_| link.sim().laser_power() == 0.0);
        for _ in 0..10 {
            assert_eq!(next_status(&inbox).state, MachineState::Hold { code: 0 });
            assert_eq!(link.sim().laser_power(), 0.0);
        }
        assert_eq!(realtime_count(&link, 0x9E), 1, "sent once per hold");

        // The program is still M3: resuming turns the laser back on. (GRBL
        // parses no lines, `$G` included, until a completed hold resumes.)
        handle.resume_job().unwrap();
        let progress = follow_job(&inbox, |_| {});
        assert_eq!(progress.last().unwrap().state, JobState::Finished);
        assert_eq!(received_realtime(&link), [b'!', 0x9E, b'~']);
        let burns = burns(&link);
        assert_eq!(burns.len(), 6, "{burns:?}");
        assert!(
            burns.iter().all(|b| (b.power - 0.5).abs() < 1e-9),
            "{burns:?}"
        );
    }

    #[test]
    fn stopping_turns_off_the_laser_and_the_air_assist_the_job_turned_on() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.start_job(burn(&["M8", "M4 S500"], 30)).unwrap();
        status_where(&inbox, |_| link.sim().laser_power() > 0.0);
        assert!(link.sim().air_assist());

        handle.stop_job().unwrap();
        let progress = follow_job(&inbox, |_| {});

        assert_eq!(
            progress.last().unwrap().state,
            JobState::Stopped { warning: None }
        );
        assert!(
            received_lines(&link).ends_with("M5\nM9\n"),
            "{}",
            received_lines(&link)
        );
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(!link.sim().air_assist());
        let state = parser_state(&connection);
        assert!(state.contains("M5") && state.contains("M9"), "{state}");

        // Without air assist only M5 follows the stop.
        handle.start_job(burn(&["M4 S500"], 30)).unwrap();
        status_where(&inbox, |_| link.sim().laser_power() > 0.0);
        let before = received_lines(&link).len();
        handle.stop_job().unwrap();
        follow_job(&inbox, |_| {});
        let after = received_lines(&link);
        assert!(after[before..].ends_with("M5\n"), "{}", &after[before..]);
        assert!(!after[before..].contains("M9"), "{}", &after[before..]);
    }

    #[test]
    fn a_rejected_line_holds_with_the_laser_off_and_aborting_sends_m5_and_m9() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        let job = Job::from_lines(
            "error.gcode",
            [
                "G21 G90",
                "M8",
                "M4 S500",
                "G1 X10 F600",
                "G99",
                "G1 X20",
                "G0 X0",
            ],
        );
        handle.start_job(job).unwrap();
        wait_for_progress(&inbox, |p| matches!(p.state, JobState::Rejected { .. }));
        status_where(&inbox, |s| s.state == MachineState::Hold { code: 0 });
        assert_eq!(link.sim().laser_power(), 0.0);

        handle.stop_job().unwrap();
        let progress = follow_job(&inbox, |_| {});

        assert_eq!(
            progress.last().unwrap().state,
            JobState::Stopped { warning: None }
        );
        assert!(received_lines(&link).ends_with("M5\nM9\n"));
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(!link.sim().air_assist());
    }

    #[test]
    fn a_job_that_cant_be_read_stops_with_the_laser_and_air_off() {
        let link = laser_link();
        let (_connection, inbox) = open_ready(&link);
        let mut lines: Vec<io::Result<String>> = ["G21 G90", "M8", "M4 S500"]
            .into_iter()
            .map(|line| Ok(line.to_string()))
            .collect();
        lines.extend((1..=5).map(|step| Ok(format!("G1 X{} F600", step * 10))));
        lines.push(Err(io::Error::other("the disk went away")));
        let job = Job {
            name: "broken.gcode".into(),
            total: None,
            lines: Box::new(lines.into_iter()),
        };
        _connection.handle().start_job(job).unwrap();

        let progress = follow_job(&inbox, |_| {});

        let JobState::Failed { reason } = &progress.last().unwrap().state else {
            panic!("{progress:?}");
        };
        assert!(reason.contains("the disk went away"), "{reason}");
        assert!(received_lines(&link).ends_with("M5\nM9\n"));
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(!link.sim().air_assist());
    }

    #[test]
    fn an_alarm_mid_job_leaves_the_laser_and_air_off_to_the_controller() {
        let link = laser_link();
        let (_connection, inbox) = open_ready(&link);
        link.sim().run_scenario("after 5: alarm 1".parse().unwrap());
        _connection
            .handle()
            .start_job(burn(&["M8", "M4 S500"], 30))
            .unwrap();

        let progress = follow_job(&inbox, |_| {});

        let JobState::Failed { reason } = &progress.last().unwrap().state else {
            panic!("{progress:?}");
        };
        assert!(reason.contains("ALARM:1"), "{reason}");
        // GRBL's alarm reset turned both off; in alarm it would reject M5.
        assert_eq!(link.sim().laser_power(), 0.0);
        assert!(!link.sim().air_assist());
        assert!(!received_lines(&link).contains("M5"));
    }

    #[test]
    fn a_dropped_connection_leaves_the_controller_to_drain_and_reconnecting_resets_it() {
        // Nothing can reach the controller over a pulled cable: it runs out
        // what it has buffered. What happens to the laser then is GRBL's.
        for (mode, lit_at_rest) in [("M4 S500", false), ("M3 S500", true)] {
            let link = laser_link();
            let (connection, inbox) = open_ready(&link);
            connection
                .handle()
                .start_job(burn(&["M8", mode], 30))
                .unwrap();
            status_where(&inbox, |_| link.sim().laser_power() > 0.0);
            let sent = received_lines(&link);

            link.sim().inject(Fault::Disconnect);
            let progress = follow_job(&inbox, |_| {});

            let JobState::Failed { reason } = &progress.last().unwrap().state else {
                panic!("{progress:?}");
            };
            assert!(reason.contains("Connection lost"), "{reason}");
            let mut sim = link.sim();
            for _ in 0..600 {
                if sim.state() == bf_sim::MachineState::Idle {
                    break;
                }
                sim.advance(Duration::from_millis(100));
            }
            assert_eq!(sim.state(), bf_sim::MachineState::Idle);
            assert!(sim.machine_position()[0] < 300.0, "it stopped short");
            // Dynamic power (M4) goes dark at rest; constant power (M3) in
            // laser mode doesn't, and air assist stays on either way.
            assert_eq!(sim.laser_power() > 0.0, lit_at_rest, "{mode}");
            assert!(sim.air_assist());
            drop(sim);
            assert_eq!(received_lines(&link), sent, "{mode}: nothing more arrived");
            drop(connection);

            // A board that doesn't reset when its port opens is still
            // running: the handshake's soft reset turns both off.
            link.sim().replug();
            let (_again, _inbox) = open_ready(&link);
            assert_eq!(link.sim().laser_power(), 0.0, "{mode}");
            assert!(!link.sim().air_assist(), "{mode}");
        }
    }

    #[test]
    fn a_frozen_event_handler_cannot_stall_or_block_stopping_the_stream() {
        let link = laser_link();
        let (events, inbox) = mpsc::channel();
        let (thaw, frozen) = mpsc::channel::<()>();
        let freeze = Arc::new(AtomicBool::new(false));
        let frozen_now = freeze.clone();
        let connection = Connection::open(link.transport(), link.clock(), move |event| {
            if frozen_now.load(Ordering::SeqCst) {
                // Like a UI that stopped responding: until thawed.
                let _ = frozen.recv();
            }
            let _ = events.send(event);
        })
        .unwrap();
        let inbox = Events::free(inbox);
        next_status(&inbox);
        let handle = connection.handle();
        let until = |done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "timed out");
                thread::sleep(Duration::from_millis(5));
            }
        };

        freeze.store(true, Ordering::SeqCst);
        handle.start_job(long_burn(5)).unwrap();
        // Nobody hears about it, yet the job streams to the end.
        until(&|| !handle.job_active());
        assert_eq!(link.sim().machine_position()[0], 0.0);
        assert_eq!(link.sim().laser_power(), 0.0);
        assert_eq!(overflows(&link), 0);

        // And a stop asked for meanwhile still goes through to M5. The clock
        // runs free here (the handler is frozen, so there's no lockstep):
        // the job runs back and forth for most of an hour of simulated time,
        // so a slow test thread still catches it running, and the wait is
        // for a burn in the trace, which stays there once it happens.
        let mut lines = vec!["G21 G90".to_string(), "M4 S500".to_string()];
        lines.extend((0..3000).map(|i| format!("G1 X{} F600", if i % 2 == 0 { 10 } else { 0 })));
        handle
            .start_job(Job::from_lines("back-and-forth.gcode", lines))
            .unwrap();
        until(&|| link.sim().trace().iter().any(|segment| segment.power > 0.0));
        handle.stop_job().unwrap();
        until(&|| !handle.job_active());
        assert!(received_lines(&link).ends_with("M5\n"));
        assert_eq!(link.sim().laser_power(), 0.0);

        // Thawed, the handler catches up on every event, in order.
        drop(thaw);
        let done: Vec<JobState> = inbox
            .iter()
            .filter_map(|event| match event {
                MachineEvent::Job(progress) if progress.state.is_done() => Some(progress.state),
                _ => None,
            })
            .take(2)
            .collect();
        assert_eq!(
            done,
            [JobState::Finished, JobState::Stopped { warning: None }]
        );
    }

    #[test]
    fn starting_a_job_resets_overrides_left_from_earlier() {
        let link = laser_link();
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        handle.set_override(OverrideKind::Spindle, 50).unwrap();
        handle.set_override(OverrideKind::Feed, 150).unwrap();
        status_where(&inbox, |s| {
            s.overrides
                .is_some_and(|o| o.spindle == 50 && o.feed == 150)
        });
        link.sim().feed(&[0x96]); // rapids at 50%, from the machine's buttons
        status_where(&inbox, |s| s.overrides.is_some_and(|o| o.rapid == 50));

        handle.start_job(long_burn(3)).unwrap();
        let progress = follow_job(&inbox, |_| {});

        assert_eq!(progress.last().unwrap().state, JobState::Finished);
        let overrides = link.sim().overrides();
        assert_eq!(
            (overrides.feed, overrides.rapid, overrides.spindle),
            (100, 100, 100)
        );
        // The resets went out before the job's first line...
        let sim = link.sim();
        let position = |wanted: &[u8]| {
            sim.events()
                .iter()
                .position(|e| matches!(e, Event::Received { bytes, .. } if bytes == wanted))
                .unwrap()
        };
        assert!(position(&[0x90, 0x95, 0x99]) < position(b"G21 G90\n"));
        drop(sim);
        // ...so every cut burned at the programmed power.
        let burns = burns(&link);
        assert_eq!(burns.len(), 3);
        assert!(
            burns.iter().all(|b| (b.power - 0.5).abs() < 1e-9),
            "{burns:?}"
        );
    }

    #[test]
    fn homing_marks_the_machine_homed_until_an_alarm() {
        let link = sim_link(&[(22, 1.0)]);
        let (connection, inbox) = open_ready(&link);
        let handle = connection.handle();
        assert!(!handle.homed(), "GRBL powers up unhomed, in alarm");
        handle.query("$X", WAIT).unwrap();
        assert!(!handle.homed(), "unlocking isn't homing");

        assert_eq!(handle.query("$H", WAIT), Ok(Vec::new()));
        assert!(handle.homed());
        status_where(&inbox, |s| s.state == MachineState::Idle);
        assert!(handle.homed());

        link.sim().inject(Fault::Alarm(3));
        status_where(&inbox, |s| s.state == MachineState::Alarm);
        assert!(!handle.homed());
    }
}
