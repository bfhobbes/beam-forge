//! Machine connection commands and events over `bf_core::machine`.
//!
//! Events: `machine:state` carries a [`ConnectionState`] whenever it changes;
//! `machine:line` carries each [`MachineLine`] the controller sends, decoded
//! for the console;
//! `machine:status` carries each polled [`MachineStatus`].
//!
//! [`settings`] reads and writes `$` settings and keeps the machine profile;
//! [`job`] runs GCode files and generated jobs; [`control`] jogs, homes,
//! sets the work origin, toggles air assist and test fires.

pub mod control;
pub mod job;
mod launch;
pub mod settings;
mod transport;

use std::sync::{Mutex, MutexGuard};

use bf_core::machine::{
    Connection, ConnectionState, DEFAULT_BAUD, Dialect, Grbl, JobProgress, MachineEvent,
    MachineInfo, MachineLine, MachineStatus, PortInfo, SendError, SystemClock,
};
use bf_core::project::Point;
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const STATE_EVENT: &str = "machine:state";
pub const LINE_EVENT: &str = "machine:line";
pub const STATUS_EVENT: &str = "machine:status";

/// The one machine connection, shared by commands and the owner thread.
pub struct Machine(Mutex<Inner>);

struct Inner {
    state: ConnectionState,
    connection: Option<Connection>,
    /// The running job's latest progress, or the last job's final one.
    job: Option<JobProgress>,
    /// The latest status report while connected.
    status: Option<MachineStatus>,
    /// Bumped whenever a connection attempt starts or ends, so a stale
    /// attempt or owner thread can't overwrite a newer state.
    generation: u64,
}

impl Default for Machine {
    fn default() -> Self {
        Machine(Mutex::new(Inner {
            state: ConnectionState::Disconnected { reason: None },
            connection: None,
            job: None,
            status: None,
            generation: 0,
        }))
    }
}

impl Machine {
    fn of<R: Runtime>(app: &AppHandle<R>) -> &Machine {
        app.state::<Machine>().inner()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Machine {
    /// The head's work position from the latest status report, if connected.
    pub fn work_position<R: Runtime>(app: &AppHandle<R>) -> Option<Point> {
        let inner = Machine::of(app).lock();
        let position = inner.status.as_ref()?.work_position?;
        Some(Point {
            x: position.x,
            y: position.y,
        })
    }
}

impl Inner {
    fn set_state<R: Runtime>(&mut self, app: &AppHandle<R>, state: ConnectionState) {
        let _ = app.emit(STATE_EVENT, &state);
        if !matches!(state, ConnectionState::Connected { .. }) {
            self.status = None;
        }
        self.state = state;
    }
}

#[tauri::command]
pub fn machine_ports() -> Result<Vec<PortInfo>, String> {
    transport::list_ports()
}

#[tauri::command]
pub fn machine_state<R: Runtime>(app: AppHandle<R>) -> ConnectionState {
    Machine::of(&app).lock().state.clone()
}

/// Opens `port`, runs the handshake and reports the firmware. Fails with a
/// message for the user when the port can't be opened or nothing answers.
#[tauri::command]
pub async fn machine_connect<R: Runtime>(
    app: AppHandle<R>,
    port: PortInfo,
    baud: Option<u32>,
) -> Result<MachineInfo, String> {
    let baud = baud.unwrap_or(DEFAULT_BAUD);
    let generation = {
        let mut inner = Machine::of(&app).lock();
        if !matches!(inner.state, ConnectionState::Disconnected { .. }) {
            return Err("Already connected. Disconnect first.".to_string());
        }
        inner.generation += 1;
        inner.job = None;
        inner.set_state(&app, ConnectionState::Connecting { port: port.clone() });
        inner.generation
    };

    let events = app.clone();
    let address = port.address.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || {
        let transport = transport::open(&address, baud)?;
        Connection::open(transport, SystemClock::new(), move |event| {
            on_event(&events, generation, event)
        })
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|opened| opened);

    let mut inner = Machine::of(&app).lock();
    if inner.generation != generation {
        // Disconnected meanwhile, by the user or a failing port.
        return Err(match &inner.state {
            ConnectionState::Disconnected {
                reason: Some(reason),
            } => reason.clone(),
            _ => "Connection cancelled".to_string(),
        });
    }
    match opened {
        Ok(connection) => {
            let info = connection.machine().clone();
            inner.connection = Some(connection);
            inner.set_state(
                &app,
                ConnectionState::Connected {
                    port,
                    machine: info.clone(),
                },
            );
            Ok(info)
        }
        Err(error) => {
            inner.generation += 1;
            inner.set_state(
                &app,
                ConnectionState::Disconnected {
                    reason: Some(error.clone()),
                },
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn machine_disconnect<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let connection = {
        let mut inner = Machine::of(&app).lock();
        // Disconnecting mid-job would leave the laser to finish whatever
        // the controller has buffered.
        if inner
            .connection
            .as_ref()
            .is_some_and(|c| c.handle().job_active())
        {
            return Err("A job is running. Stop it first.".to_string());
        }
        inner.generation += 1;
        inner.set_state(&app, ConnectionState::Disconnected { reason: None });
        inner.connection.take()
    };
    if let Some(connection) = connection {
        // Joins the owner thread, which takes up to one poll interval.
        tauri::async_runtime::spawn_blocking(move || connection.close())
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// For when the app exits: stops any running job, waiting until the laser
/// is off and the machine idle (see [`job::stop_and_wait`]), then closes the
/// connection. Closing under a job that didn't stop in time soft-resets the
/// controller, which also turns the laser off.
pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    let handle = Machine::of(app)
        .lock()
        .connection
        .as_ref()
        .map(Connection::handle);
    if let Some(handle) = handle {
        let _ = job::stop_and_wait(&handle, job::STOP_WAIT);
    }
    let connection = {
        let mut inner = Machine::of(app).lock();
        inner.generation += 1;
        inner.connection.take()
    };
    if let Some(connection) = connection {
        connection.close();
    }
}

/// Sends one command line from the console, such as `$I` or `G0 X10`.
#[tauri::command]
pub fn machine_send<R: Runtime>(app: AppHandle<R>, line: String) -> Result<(), String> {
    with_connection(&app, |connection| connection.send_line(&line))
}

/// Soft-resets the controller (Ctrl-X for GRBL), aborting any motion.
#[tauri::command]
pub fn machine_soft_reset<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    with_connection(&app, Connection::soft_reset)
}

fn with_connection<R: Runtime>(
    app: &AppHandle<R>,
    send: impl FnOnce(&Connection) -> Result<(), SendError>,
) -> Result<(), String> {
    let inner = Machine::of(app).lock();
    let connection = inner.connection.as_ref().ok_or(SendError::Closed);
    connection.and_then(send).map_err(|error| error.to_string())
}

/// Runs on the connection's owner thread.
fn on_event<R: Runtime>(app: &AppHandle<R>, generation: u64, event: MachineEvent) {
    match event {
        MachineEvent::Line(line) => {
            // Lines that follow the banner arrive before the connection is
            // stored; GRBL is the only dialect that gets that far today.
            let reply = match &Machine::of(app).lock().connection {
                Some(connection) => connection.dialect().decode_reply(&line),
                None => Grbl.decode_reply(&line),
            };
            let _ = app.emit(LINE_EVENT, MachineLine { line, reply });
        }
        MachineEvent::Status(status) => {
            // Hold the lock so a disconnect can't slip in between the
            // check and the emit and leave a stale readout behind.
            let mut inner = Machine::of(app).lock();
            if inner.generation == generation {
                let _ = app.emit::<MachineStatus>(STATUS_EVENT, status.clone());
                inner.status = Some(status);
            }
        }
        MachineEvent::Job(progress) => {
            let mut inner = Machine::of(app).lock();
            if inner.generation == generation {
                let _ = app.emit(job::JOB_EVENT, &progress);
                inner.job = Some(progress);
            }
        }
        MachineEvent::Disconnected { reason } => {
            let stale = {
                let mut inner = Machine::of(app).lock();
                if inner.generation != generation {
                    return;
                }
                inner.generation += 1;
                inner.set_state(app, ConnectionState::Disconnected { reason });
                inner.connection.take()
            };
            drop(stale);
        }
    }
}
