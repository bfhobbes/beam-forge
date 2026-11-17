//! The real transports: USB serial ports and TCP (for the simulator app).

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use bf_core::machine::{PortAddress, PortInfo, SIMULATOR_HOST, SIMULATOR_PORT, Transport};
use serialport::{SerialPort, SerialPortType};

use super::launch::{self, Launched};

/// How long a write may stall (a full USB buffer) before the port counts as failed.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often an idle serial port checks it still exists. Some USB-serial
/// drivers report a pulled cable only by going quiet.
const PRESENCE_CHECK: Duration = Duration::from_secs(1);

/// Serial ports the OS reports, then the built-in and standalone simulators.
pub fn list_ports() -> Result<Vec<PortInfo>, String> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .map_err(|error| format!("Couldn't list serial ports: {error}"))?
        .into_iter()
        .map(|port| {
            let address = PortAddress::Serial {
                path: port.port_name.clone(),
            };
            let (description, vid, pid) = match port.port_type {
                SerialPortType::UsbPort(usb) => {
                    let description = usb.product.or(usb.manufacturer);
                    (description, Some(usb.vid), Some(usb.pid))
                }
                SerialPortType::BluetoothPort => (Some("Bluetooth".to_string()), None, None),
                SerialPortType::PciPort | SerialPortType::Unknown => (None, None, None),
            };
            let label = match &description {
                Some(description) => format!("{} · {description}", port.port_name),
                None => port.port_name,
            };
            PortInfo {
                key: address.key(),
                address,
                label,
                description,
                vid,
                pid,
            }
        })
        .collect();
    ports.sort_by(|a, b| a.label.cmp(&b.label));
    ports.push(PortInfo::built_in_simulator());
    ports.push(PortInfo::simulator());
    Ok(ports)
}

/// Opens `address`, with an error message fit to show the user.
pub fn open(address: &PortAddress, baud: u32) -> Result<Box<dyn Transport>, String> {
    match address {
        PortAddress::Serial { path } => SerialTransport::open(path, baud)
            .map(|port| Box::new(port) as Box<dyn Transport>)
            .map_err(|error| serial_open_error(path, &error)),
        PortAddress::Tcp { host, port } => TcpTransport::connect(host, *port)
            .map(|stream| Box::new(stream) as Box<dyn Transport>)
            .map_err(|error| {
                let hint = if host == SIMULATOR_HOST && *port == SIMULATOR_PORT {
                    " Is the simulator app running?"
                } else {
                    ""
                };
                format!("Couldn't reach {host}:{port}: {error}.{hint}")
            }),
        PortAddress::Launch => connect_launched(launch::launch_simulator()?),
    }
}

/// Why `path` didn't open. On Linux, a refused serial port usually means the
/// user isn't in the group that owns it, so say how to fix that.
fn serial_open_error(path: &str, error: &serialport::Error) -> String {
    let denied = error.kind() == serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied);
    if denied && cfg!(target_os = "linux") {
        format!(
            "Couldn't open {path}: {error}. Add yourself to the group that owns it              (usually dialout: `sudo usermod -aG dialout $USER`), then log out and back in."
        )
    } else {
        format!("Couldn't open {path}: {error}")
    }
}

/// Connects to a simulator the app started; the transport owns it from here.
fn connect_launched(simulator: Launched) -> Result<Box<dyn Transport>, String> {
    let address = simulator.address();
    TcpTransport::connect_to(address, Some(simulator))
        .map(|stream| Box::new(stream) as Box<dyn Transport>)
        .map_err(|error| format!("Couldn't reach the built-in simulator at {address}: {error}"))
}

pub struct SerialTransport {
    path: String,
    port: Box<dyn SerialPort>,
    timeout: Duration,
    last_seen: Instant,
}

impl SerialTransport {
    fn open(path: &str, baud: u32) -> serialport::Result<Self> {
        let timeout = Duration::from_millis(10);
        let port = serialport::new(path, baud).timeout(timeout).open()?;
        Ok(SerialTransport {
            path: path.to_string(),
            port,
            timeout,
            last_seen: Instant::now(),
        })
    }

    fn check_present(&mut self) -> io::Result<()> {
        if self.last_seen.elapsed() < PRESENCE_CHECK {
            return Ok(());
        }
        let present = serialport::available_ports()
            .map(|ports| ports.iter().any(|port| port.port_name == self.path))
            .unwrap_or(true);
        if !present {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                format!("{} disappeared (cable unplugged?)", self.path),
            ));
        }
        self.last_seen = Instant::now();
        Ok(())
    }
}

impl Transport for SerialTransport {
    fn write(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        let started = Instant::now();
        while !bytes.is_empty() {
            match self.port.write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => bytes = &bytes[written..],
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    if started.elapsed() > WRITE_TIMEOUT {
                        return Err(error);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        self.port.flush()
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let timeout = timeout.max(Duration::from_millis(1));
        if timeout != self.timeout {
            self.port.set_timeout(timeout)?;
            self.timeout = timeout;
        }
        match self.port.read(buf) {
            Ok(0) => self.check_present().map(|()| 0),
            Ok(read) => {
                self.last_seen = Instant::now();
                Ok(read)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) =>
            {
                self.check_present().map(|()| 0)
            }
            Err(error) => Err(error),
        }
    }
}

pub struct TcpTransport {
    stream: TcpStream,
    /// What the reader thread received; an error is its last message.
    incoming: Receiver<io::Result<Vec<u8>>>,
    /// The rest of a chunk that didn't fit the last read.
    unread: Vec<u8>,
    /// The simulator the app started for this connection, closed (after the
    /// socket, by field order) when the transport is dropped.
    _simulator: Option<Launched>,
}

impl TcpTransport {
    fn connect(host: &str, port: u16) -> io::Result<Self> {
        let address = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address"))?;
        Self::connect_to(address, None)
    }

    fn connect_to(address: SocketAddr, simulator: Option<Launched>) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
        stream.set_nodelay(true)?;
        // Reads block on their own thread rather than time out: on Windows a
        // timed-out socket read leaves the socket in an indeterminate state,
        // and under load the next read can fail with ERROR_IO_PENDING (os
        // error 997), which would drop the connection.
        let reader = stream.try_clone()?;
        let (chunks, incoming) = mpsc::channel();
        thread::Builder::new()
            .name("bf-tcp-read".into())
            .spawn(move || read_loop(reader, chunks))?;
        Ok(TcpTransport {
            stream,
            incoming,
            unread: Vec::new(),
            _simulator: simulator,
        })
    }
}

/// Forwards what arrives until the connection closes or fails.
fn read_loop(mut stream: TcpStream, chunks: Sender<io::Result<Vec<u8>>>) {
    let mut buf = [0; 4096];
    loop {
        let chunk = match stream.read(&mut buf) {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the other end closed the connection",
            )),
            Ok(read) => Ok(buf[..read].to_vec()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => Err(error),
        };
        let failed = chunk.is_err();
        if chunks.send(chunk).is_err() || failed {
            break;
        }
    }
}

impl Transport for TcpTransport {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.write_all(bytes)
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.unread.is_empty() {
            self.unread = match self.incoming.recv_timeout(timeout) {
                Ok(chunk) => chunk?,
                Err(RecvTimeoutError::Timeout) => return Ok(0),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "the connection is closed",
                    ));
                }
            };
        }
        let read = buf.len().min(self.unread.len());
        buf[..read].copy_from_slice(&self.unread[..read]);
        self.unread.drain(..read);
        Ok(read)
    }
}

impl Drop for TcpTransport {
    fn drop(&mut self) {
        // Unblocks the reader thread, whose clone would keep the socket open.
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bf_core::machine::{Connection, SystemClock};
    use std::process::Command;

    fn tcp_pair() -> (TcpTransport, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let transport = TcpTransport::connect_to(listener.local_addr().unwrap(), None).unwrap();
        let (peer, _) = listener.accept().unwrap();
        (transport, peer)
    }

    fn read_some(transport: &mut TcpTransport, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0; len];
        let read = transport.read(&mut buf, Duration::from_secs(5))?;
        buf.truncate(read);
        Ok(buf)
    }

    #[test]
    fn a_refused_serial_port_says_how_to_get_access_on_linux() {
        let denied = serialport::Error::new(
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied),
            "Permission denied",
        );
        let message = serial_open_error("/dev/ttyUSB0", &denied);
        assert!(message.starts_with("Couldn't open /dev/ttyUSB0: Permission denied"));
        assert_eq!(message.contains("dialout"), cfg!(target_os = "linux"));
        let missing = serialport::Error::new(serialport::ErrorKind::NoDevice, "No such device");
        assert_eq!(
            serial_open_error("/dev/ttyUSB0", &missing),
            "Couldn't open /dev/ttyUSB0: No such device"
        );
    }

    #[test]
    fn tcp_reads_wait_hand_over_what_arrived_and_report_a_close() {
        let (mut transport, mut peer) = tcp_pair();
        let mut buf = [0; 8];
        let quiet = transport.read(&mut buf, Duration::from_millis(20));
        assert_eq!(quiet.unwrap(), 0, "nothing sent yet");

        peer.write_all(b"ok\r\nGrbl").unwrap();
        // A chunk bigger than the caller's buffer comes out over several reads.
        let mut received = Vec::new();
        while received.len() < 8 {
            let chunk = read_some(&mut transport, 3).unwrap();
            assert!(!chunk.is_empty(), "timed out after {received:?}");
            received.extend(chunk);
        }
        assert_eq!(received, b"ok\r\nGrbl");

        drop(peer);
        let closed = read_some(&mut transport, 8).unwrap_err();
        assert_eq!(closed.kind(), io::ErrorKind::UnexpectedEof);
        let after = read_some(&mut transport, 8).unwrap_err();
        assert_eq!(after.kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn dropping_a_tcp_transport_closes_the_socket() {
        let (mut transport, mut peer) = tcp_pair();
        transport.write(b"?").unwrap();
        drop(transport);
        // The reader thread's clone must not keep the connection open.
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut received = Vec::new();
        peer.read_to_end(&mut received).unwrap();
        assert_eq!(received, b"?");
    }

    /// Connects to a real simulator twice in a row, as the "Built-in
    /// simulator" port does. Opens simulator windows, so it only runs on
    /// request: `BF_SIMULATOR_EXE=target/release/bf-simulator.exe cargo test
    /// -p beam-forge -- --ignored launched_simulator`.
    #[test]
    #[ignore = "needs a built simulator and opens its window"]
    fn launched_simulator_handshakes_and_closes() {
        let exe = std::env::var("BF_SIMULATOR_EXE").expect("set BF_SIMULATOR_EXE");
        let mut addresses = Vec::new();
        for _ in 0..2 {
            let mut command = Command::new(&exe);
            command.arg(launch::LAUNCH_FLAG);
            let simulator = launch::launch(command, launch::REPORT_TIMEOUT).unwrap();
            addresses.push(simulator.address());
            let transport = connect_launched(simulator).unwrap();
            let connection = Connection::open(transport, SystemClock::new(), |_| {}).unwrap();
            assert_eq!(connection.machine().version, "1.1h");
            // Joins the owner thread, which drops the transport and closes
            // the simulator.
            connection.close();
            let address = addresses.last().unwrap();
            assert!(
                TcpStream::connect_timeout(address, Duration::from_millis(500)).is_err(),
                "the simulator at {address} should have exited"
            );
        }
        assert_ne!(addresses[0], addresses[1], "a fresh port each time");
    }
}
