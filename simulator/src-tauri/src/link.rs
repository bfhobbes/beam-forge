//! The fake USB link: a TCP server that stands in for the controller's serial
//! port.
//!
//! Bytes pass through unchanged, like a serial line. One client at a time:
//! connecting resets the controller and sends its banner, as opening the USB
//! port resets an Arduino-based GRBL board, and disconnecting is a pulled
//! cable. The controller keeps running either way; output with no client
//! attached is lost, as it would be on a real wire.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bf_sim::Profile;
use serde::Serialize;
use ts_rs::TS;

use crate::inspector::InspectorBatch;
use crate::machine::{Machine, MachineSnapshot};

pub const DEFAULT_PORT: u16 = 23232;
/// How often the clock runs the controller between host bytes.
const TICK: Duration = Duration::from_millis(5);
/// How often the listener checks for new connections and shutdown.
const ACCEPT_POLL: Duration = Duration::from_millis(20);
/// Sent to a second client before closing it. A GRBL-style message, so a host
/// that logs controller output shows why the connection ended.
pub const BUSY_MESSAGE: &[u8] = b"[MSG:Simulator busy: another client is connected]\r\n";

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct LinkSnapshot {
    /// Where the server listens, e.g. `127.0.0.1:23232`; `None` if not listening.
    pub address: Option<String>,
    /// Why the server isn't listening, e.g. the port is in use.
    pub error: Option<String>,
    pub client: Option<ClientInfo>,
    /// Connections turned away because a client was already attached.
    pub refused: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct ClientInfo {
    pub peer: String,
    /// Unix time in milliseconds.
    pub connected_at: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../simulator/src/lib/bindings/")]
pub struct HostSnapshot {
    pub link: LinkSnapshot,
    pub machine: MachineSnapshot,
}

struct Client {
    id: u64,
    info: ClientInfo,
    stream: TcpStream,
    /// To the writer thread, so output reaches the socket in the order the
    /// controller produced it.
    output: Sender<Vec<u8>>,
}

struct Inner {
    machine: Machine,
    client: Option<Client>,
    next_client: u64,
    refused: u32,
    error: Option<String>,
}

impl Inner {
    /// Runs the controller up to now and forwards its output to the client.
    /// A pulled cable ([`bf_sim::Fault::Disconnect`]) drops the client once
    /// the output from before it has been written.
    fn pump(&mut self) {
        let output = self.machine.pump(Instant::now());
        if let Some(client) = &self.client
            && !output.is_empty()
        {
            // A closed channel means the client is going; its reader cleans up.
            let _ = client.output.send(output);
        }
        if self.machine.is_disconnected()
            && let Some(client) = self.client.take()
        {
            // Dropping the client closes its output channel: the writer
            // finishes, then shuts the socket, which ends the reader.
            self.machine.note(format!(
                "Client {} disconnected: cable pulled",
                client.info.peer
            ));
        }
    }
}

struct Listener {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One emulated controller, its clock, and the TCP link to it.
pub struct Host {
    inner: Arc<Mutex<Inner>>,
    listener: Mutex<Option<Listener>>,
    stop: Arc<AtomicBool>,
    clock: Option<JoinHandle<()>>,
}

impl Host {
    /// Starts the controller and its clock, and listens on `127.0.0.1:port`.
    /// The host runs even if the port is taken; [`Host::snapshot`] reports why
    /// it isn't listening and [`Host::listen`] can try another port.
    pub fn start(profile: Profile, port: u16) -> Host {
        let inner = Arc::new(Mutex::new(Inner {
            machine: Machine::new(profile, Instant::now()),
            client: None,
            next_client: 0,
            refused: 0,
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let clock = {
            let inner = inner.clone();
            let stop = stop.clone();
            thread::Builder::new()
                .name("sim-clock".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        lock(&inner).pump();
                        thread::sleep(TICK);
                    }
                })
                .expect("Unable to start the simulator clock")
        };
        let host = Host {
            inner,
            listener: Mutex::new(None),
            stop,
            clock: Some(clock),
        };
        let _ = host.listen(port);
        host
    }

    /// (Re)binds the server to `127.0.0.1:port`; port 0 picks a free one. An
    /// attached client stays connected. On failure the old listener is kept.
    pub fn listen(&self, port: u16) -> Result<SocketAddr, String> {
        let mut current = self.listener.lock().unwrap_or_else(|e| e.into_inner());
        let socket = TcpListener::bind(("127.0.0.1", port))
            .and_then(|socket| socket.set_nonblocking(true).map(|()| socket));
        let socket = match socket {
            Ok(socket) => socket,
            Err(error) => {
                let message = format!("Couldn't listen on 127.0.0.1:{port}: {error}");
                if current.is_none() {
                    lock(&self.inner).error = Some(message.clone());
                }
                return Err(message);
            }
        };
        let address = socket.local_addr().map_err(|e| e.to_string())?;
        // Stop the old listener first so it can't accept on the old port.
        current.take();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let inner = self.inner.clone();
            let stop = stop.clone();
            thread::Builder::new()
                .name("sim-listen".into())
                .spawn(move || accept_loop(socket, inner, stop))
                .map_err(|e| e.to_string())?
        };
        lock(&self.inner).error = None;
        *current = Some(Listener {
            address,
            stop,
            thread: Some(thread),
        });
        Ok(address)
    }

    pub fn address(&self) -> Option<SocketAddr> {
        let listener = self.listener.lock().unwrap_or_else(|e| e.into_inner());
        listener.as_ref().map(|listener| listener.address)
    }

    /// Runs `f` on the controller, then forwards anything it printed (for
    /// example the banner after a reset) to the client.
    pub fn with_machine<T>(&self, f: impl FnOnce(&mut Machine, Instant) -> T) -> T {
        let mut inner = lock(&self.inner);
        let result = f(&mut inner.machine, Instant::now());
        inner.pump();
        result
    }

    /// Wire traffic from `since` on, for the protocol inspector.
    pub fn inspector(&self, since: u64) -> InspectorBatch {
        lock(&self.inner).machine.inspector(since)
    }

    pub fn snapshot(&self) -> HostSnapshot {
        let address = self.address().map(|address| address.to_string());
        let inner = lock(&self.inner);
        HostSnapshot {
            link: LinkSnapshot {
                address,
                error: inner.error.clone(),
                client: inner.client.as_ref().map(|client| client.info.clone()),
                refused: inner.refused,
            },
            machine: inner.machine.snapshot(),
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.listener
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(clock) = self.clock.take() {
            let _ = clock.join();
        }
        if let Some(client) = lock(&self.inner).client.take() {
            let _ = client.stream.shutdown(Shutdown::Both);
        }
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    // A panicked client thread must not take the controller down with it.
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

fn accept_loop(socket: TcpListener, inner: Arc<Mutex<Inner>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match socket.accept() {
            Ok((stream, peer)) => {
                if let Err(error) = attach(stream, peer, &inner) {
                    eprintln!("Simulator: couldn't attach {peer}: {error}");
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(error) => {
                eprintln!("Simulator: accept failed: {error}");
                thread::sleep(ACCEPT_POLL);
            }
        }
    }
}

/// Plugs a new client in, or turns it away if one is already attached.
fn attach(
    mut stream: TcpStream,
    peer: SocketAddr,
    inner: &Arc<Mutex<Inner>>,
) -> std::io::Result<()> {
    // Accepted sockets inherit the listener's non-blocking mode on Windows.
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    let mut guard = lock(inner);
    if guard.client.is_some() {
        guard.refused += 1;
        drop(guard);
        let _ = stream.write_all(BUSY_MESSAGE);
        let _ = stream.shutdown(Shutdown::Both);
        return Ok(());
    }
    let reader = stream.try_clone()?;
    let writer = stream.try_clone()?;
    let (output, outgoing) = mpsc::channel::<Vec<u8>>();
    let id = guard.next_client;
    guard.next_client += 1;
    thread::Builder::new()
        .name("sim-write".into())
        .spawn(move || {
            let mut writer = writer;
            for bytes in outgoing {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
            }
            // The client was detached or its socket failed.
            let _ = writer.shutdown(Shutdown::Both);
        })?;
    {
        let inner = inner.clone();
        thread::Builder::new()
            .name("sim-read".into())
            .spawn(move || read_loop(reader, id, inner))?;
    }
    // Opening the port resets the board; the banner goes to the new client.
    guard.machine.note(format!("Client {peer} connected"));
    guard.machine.power_up(Instant::now());
    guard.client = Some(Client {
        id,
        info: ClientInfo {
            peer: peer.to_string(),
            connected_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |since| since.as_secs_f64() * 1000.0),
        },
        stream,
        output,
    });
    guard.pump();
    Ok(())
}

/// Feeds host bytes to the controller until the client goes away.
fn read_loop(mut stream: TcpStream, id: u64, inner: Arc<Mutex<Inner>>) {
    let mut buffer = [0; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut inner = lock(&inner);
                inner.machine.feed(&buffer[..n]);
                // Answer straight away rather than on the next tick.
                inner.pump();
            }
        }
    }
    // Pulled cable: detach, unless this client was already replaced.
    let mut inner = lock(&inner);
    if let Some(client) = inner.client.take_if(|client| client.id == id) {
        inner.pump();
        inner
            .machine
            .note(format!("Client {} disconnected", client.info.peer));
    }
    let _ = stream.shutdown(Shutdown::Both);
}
