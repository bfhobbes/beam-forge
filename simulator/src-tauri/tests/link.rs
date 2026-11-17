//! The fake USB link, driven by a real TCP client.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bf_sim::Profile;
use bf_simulator_lib::inspector::Wire;
use bf_simulator_lib::link::{BUSY_MESSAGE, Host};

const BANNER: &str = "\r\nGrbl 1.1h ['$' for help]\r\n";
const TIMEOUT: Duration = Duration::from_secs(5);

fn host() -> (Host, SocketAddr) {
    let host = Host::start(Profile::diode_laser(), 0);
    let address = host.address().expect("listening on a free port");
    (host, address)
}

/// A test client. It reads on its own thread with plain blocking reads, as the
/// app's transport does: on Windows a read that times out can leave the socket
/// unusable (os error 997), so the socket never gets a read timeout. Dropping
/// the client shuts the socket, which ends the reader.
struct Client {
    stream: TcpStream,
    received: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
}

impl Client {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(bytes)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn connect(address: SocketAddr) -> Client {
    let stream = TcpStream::connect(address).unwrap();
    let mut socket = stream.try_clone().unwrap();
    let (sender, received) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut buffer = [0; 1024];
        // Ends when the server closes, on the shutdown from `Drop`, or once
        // nobody is listening.
        while let Ok(n @ 1..) = socket.read(&mut buffer) {
            if sender.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Client {
        stream,
        received,
        reader: Some(reader),
    }
}

/// Reads until `done` accepts what has arrived, or the connection closes.
fn read_until(client: &mut Client, done: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut received = Vec::new();
    while !done(&String::from_utf8_lossy(&received)) {
        let left = deadline.saturating_duration_since(Instant::now());
        match client.received.recv_timeout(left) {
            Ok(bytes) => received.extend_from_slice(&bytes),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!(
                "timed out; received {:?}",
                String::from_utf8_lossy(&received)
            ),
        }
    }
    String::from_utf8(received).unwrap()
}

fn read_banner(client: &mut Client) {
    assert_eq!(read_until(client, |s| s.ends_with(BANNER)), BANNER);
}

fn status(client: &mut Client) -> String {
    client.write_all(b"?").unwrap();
    read_until(client, |s| s.ends_with(">\r\n"))
}

fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_client_gets_the_banner_and_a_status_report() {
    let (host, address) = host();
    let mut client = connect(address);
    read_banner(&mut client);
    let report = status(&mut client);
    assert!(
        report.starts_with("<Idle|MPos:0.000,0.000,0.000|"),
        "{report}"
    );
    assert!(host.snapshot().link.client.is_some());
}

#[test]
fn bytes_pass_through_like_a_serial_line() {
    let (_host, address) = host();
    let mut client = connect(address);
    read_banner(&mut client);
    client.write_all(b"$I\n").unwrap();
    let reply = read_until(&mut client, |s| s.ends_with("ok\r\n"));
    assert!(reply.starts_with("[VER:1.1h"), "{reply}");
}

#[test]
fn reconnecting_resets_the_controller() {
    let (host, address) = host();
    let mut first = connect(address);
    read_banner(&mut first);
    first.write_all(b"$J=G91X50F6000\n").unwrap();
    read_until(&mut first, |s| s.ends_with("ok\r\n"));
    wait_for("the jog to move", || {
        host.snapshot().machine.machine_position[0] > 0.0
    });
    drop(first);
    wait_for("the pulled cable", || host.snapshot().link.client.is_none());

    let mut second = connect(address);
    read_banner(&mut second);
    let report = status(&mut second);
    assert!(
        report.starts_with("<Idle|MPos:0.000,0.000,0.000|"),
        "{report}"
    );
}

#[test]
fn a_second_client_is_refused_cleanly() {
    let (host, address) = host();
    let mut first = connect(address);
    read_banner(&mut first);

    let mut second = connect(address);
    let refusal = read_until(&mut second, |_| false);
    assert_eq!(refusal.as_bytes(), BUSY_MESSAGE);
    wait_for("the refusal count", || host.snapshot().link.refused == 1);

    // The first client is untouched: no reset, still answering.
    assert!(status(&mut first).starts_with("<Idle|"));
    assert_eq!(host.snapshot().link.refused, 1);
}

#[test]
fn a_taken_port_is_reported_not_fatal() {
    let (_host, address) = host();
    let other = Host::start(Profile::diode_laser(), address.port());
    let link = other.snapshot().link;
    assert_eq!(link.address, None);
    assert!(link.error.unwrap().contains(&address.port().to_string()));
    assert!(other.listen(0).is_ok());
    assert_eq!(other.snapshot().link.error, None);
}

#[test]
fn an_rx_overflow_reaches_the_inspector_log() {
    let (host, address) = host();
    let mut client = connect(address);
    read_banner(&mut client);
    // A dwell stops GRBL reading the RX buffer, so what follows backs up.
    client.write_all(b"G4P5\n").unwrap();
    wait_for("the dwell to start", || {
        host.inspector(0).events.iter().any(|event| {
            matches!(&event.event, Wire::Received { bytes, .. } if bytes == b"G4P5
")
        })
    });
    let flood = "G1X1F100\n".repeat(30);
    client.write_all(flood.as_bytes()).unwrap();
    wait_for("the overflow", || {
        host.inspector(0)
            .events
            .iter()
            .any(|event| matches!(event.event, Wire::Received { dropped, .. } if dropped > 0))
    });
    let batch = host.inspector(0);
    let dropped: u32 = batch
        .events
        .iter()
        .map(|event| match event.event {
            Wire::Received { dropped, .. } => dropped,
            _ => 0,
        })
        .sum();
    // 270 bytes into a 128-byte buffer the dwell left empty.
    assert_eq!(dropped, 270 - 128);
    assert_eq!(batch.rx_capacity, 128);
    let notes: Vec<_> = batch
        .events
        .iter()
        .filter_map(|event| match &event.event {
            Wire::Note { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        notes.iter().any(|text| text.ends_with("connected")),
        "{notes:?}"
    );
    wait_for("a full RX buffer sample", || {
        host.inspector(0)
            .samples
            .iter()
            .any(|sample| sample.rx == 128)
    });
}

#[test]
fn a_disconnect_fault_drops_the_client_and_a_new_one_gets_a_fresh_board() {
    let (host, address) = host();
    let mut client = connect(address);
    read_banner(&mut client);
    host.with_machine(|machine, _| machine.inject("disconnect").unwrap());
    // The server closes the socket, so the read ends.
    assert_eq!(read_until(&mut client, |_| false), "");
    wait_for("the client to detach", || {
        host.snapshot().link.client.is_none()
    });
    let events = host.inspector(0).events;
    assert!(events.iter().any(|event| matches!(
        &event.event,
        Wire::Fault { text } if text == "disconnect"
    )));

    let mut again = connect(address);
    read_banner(&mut again);
    assert!(status(&mut again).starts_with("<Idle|"));
}

/// The scenario and job `bf-sim`'s fault tests run directly on the emulator.
const SCENARIO: &str = include_str!("../../../crates/bf-sim/tests/fixtures/fault-scenario.txt");
const JOB: &str = include_str!("../../../crates/bf-sim/tests/fixtures/fault-job.gc");

#[test]
fn a_scenario_runs_identically_from_the_panel_and_from_a_test() {
    // Straight on the emulator, as a test in another crate runs it.
    let mut sim = bf_sim::Simulator::new(Profile::diode_laser());
    sim.take_output();
    sim.run_scenario(SCENARIO.parse().unwrap());
    sim.feed(JOB.as_bytes());
    sim.advance(Duration::from_secs(30));
    let direct = String::from_utf8(sim.take_output()).unwrap();
    assert!(direct.ends_with("ALARM:1\r\n[MSG:Reset to continue]\r\n"));

    // Through the panel's command, with the job streamed over the link.
    let (host, address) = host();
    let mut client = connect(address);
    read_banner(&mut client);
    host.with_machine(|machine, _| machine.run_scenario(SCENARIO).unwrap());
    client.write_all(JOB.as_bytes()).unwrap();
    let panel = read_until(&mut client, |s| s.ends_with("[MSG:Reset to continue]\r\n"));
    assert_eq!(panel, direct);
}
