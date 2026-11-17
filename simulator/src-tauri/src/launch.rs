//! Running as Beam Forge's built-in simulator.
//!
//! The app starts this process with [`LAUNCH_FLAG`]. The simulator then
//! listens on a port the OS picks, reports it as one line on stdout, and
//! exits when its stdin closes, so it can't outlive the app even if the app
//! crashes. The app has its own copy of this line format (the two must not
//! share code); change both together.

use std::io::{self, Read, Write};

use crate::link::Host;

/// Passed by the app when it launches the simulator.
pub const LAUNCH_FLAG: &str = "--launched-by-app";
/// Starts the line reporting the bound address, e.g. `BF_SIM_LISTENING 127.0.0.1:54321`.
pub const LISTENING: &str = "BF_SIM_LISTENING";
/// Starts the line reporting why the simulator couldn't listen.
pub const FAILED: &str = "BF_SIM_FAILED";

pub fn launched_by_app(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|arg| arg == LAUNCH_FLAG)
}

/// Writes the one line the app waits for: where `host` listens, or why it
/// isn't listening.
pub fn announce(host: &Host, out: &mut impl Write) -> io::Result<()> {
    match host.address() {
        Some(address) => writeln!(out, "{LISTENING} {address}")?,
        None => {
            let error = host.snapshot().link.error;
            let error = error.unwrap_or_else(|| "not listening".to_string());
            writeln!(out, "{FAILED} {error}")?;
        }
    }
    out.flush()
}

/// Blocks until `input` closes or fails, then calls `closed`.
pub fn wait_for_close(mut input: impl Read, closed: impl FnOnce()) {
    let mut buffer = [0; 256];
    while matches!(input.read(&mut buffer), Ok(n) if n > 0) {}
    closed();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    use bf_sim::Profile;

    #[test]
    fn the_flag_is_found_among_other_arguments() {
        let args = |list: &[&str]| list.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        assert!(launched_by_app(args(&["--x", LAUNCH_FLAG]).into_iter()));
        assert!(!launched_by_app(args(&["--launched"]).into_iter()));
    }

    #[test]
    fn port_zero_announces_the_bound_address() {
        let host = Host::start(Profile::diode_laser(), 0);
        let mut out = Vec::new();
        announce(&host, &mut out).unwrap();
        let line = String::from_utf8(out).unwrap();
        let address: SocketAddr = line
            .strip_prefix("BF_SIM_LISTENING ")
            .and_then(|rest| rest.strip_suffix('\n'))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(Some(address), host.address());
        assert_ne!(address.port(), 0);
        // The announced address really is the simulator.
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut banner = Vec::new();
        let mut buffer = [0; 64];
        while !String::from_utf8_lossy(&banner).contains("Grbl 1.1h") {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0, "connection closed before the banner");
            banner.extend_from_slice(&buffer[..n]);
        }
    }

    #[test]
    fn a_port_in_use_announces_the_failure() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let host = Host::start(Profile::diode_laser(), port);
        let mut out = Vec::new();
        announce(&host, &mut out).unwrap();
        let line = String::from_utf8(out).unwrap();
        assert!(line.starts_with("BF_SIM_FAILED Couldn't listen"), "{line}");
    }

    #[test]
    fn closing_input_calls_back_once_it_is_drained() {
        let mut called = false;
        wait_for_close(&b"anything"[..], || called = true);
        assert!(called);
    }
}
