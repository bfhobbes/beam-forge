use std::fmt;
use std::io;
use std::time::Duration;

use super::dialect::{DIALECTS, Dialect, dialect};
use super::{Clock, LineReader, MachineInfo, Transport};

/// How long to wait for the welcome banner, before and after a soft reset.
pub const BANNER_TIMEOUT: Duration = Duration::from_secs(2);

/// A controller that introduced itself.
pub struct Handshake {
    pub machine: MachineInfo,
    pub dialect: &'static dyn Dialect,
    /// Lines that followed the banner in the same read, such as GRBL's
    /// `[MSG:'$H'|'$X' to unlock]` when it powers up in alarm.
    pub lines: Vec<String>,
    /// Holds any partial line that followed them.
    pub reader: LineReader,
}

#[derive(Debug)]
pub enum HandshakeError {
    /// Nothing recognisable arrived, even after a soft reset.
    NoBanner,
    /// The port failed or closed during the handshake.
    Io(io::Error),
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::NoBanner => write!(
                f,
                "No GRBL welcome banner, even after a soft reset. Check that the controller is \
                 powered and that the port and baud rate are right."
            ),
            HandshakeError::Io(error) => write!(f, "The port failed during the handshake: {error}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

impl From<io::Error> for HandshakeError {
    fn from(error: io::Error) -> Self {
        HandshakeError::Io(error)
    }
}

/// Waits up to `timeout` for a welcome banner (`Grbl 1.1h`, `GrblHAL 1.1f`).
/// Boards that don't reset when the port opens stay quiet, so if none arrives
/// this sends a soft reset and waits again.
pub fn handshake<T: Transport + ?Sized>(
    transport: &mut T,
    clock: &dyn Clock,
    timeout: Duration,
) -> Result<Handshake, HandshakeError> {
    let mut reader = LineReader::new();
    if let Some(found) = wait_for_banner(transport, clock, &mut reader, timeout)? {
        return Ok(found);
    }
    // Every supported dialect resets with the same byte; try the first.
    transport.write(DIALECTS[0].soft_reset())?;
    wait_for_banner(transport, clock, &mut reader, timeout)?.ok_or(HandshakeError::NoBanner)
}

fn wait_for_banner<T: Transport + ?Sized>(
    transport: &mut T,
    clock: &dyn Clock,
    reader: &mut LineReader,
    timeout: Duration,
) -> Result<Option<Handshake>, HandshakeError> {
    let deadline = clock.now() + timeout;
    let mut buf = [0; 256];
    loop {
        let now = clock.now();
        if now >= deadline {
            return Ok(None);
        }
        let read = transport.read(&mut buf, deadline - now)?;
        let lines = reader.push(&buf[..read]);
        for (index, line) in lines.iter().enumerate() {
            let Some((machine, found_by)) = DIALECTS
                .iter()
                .find_map(|d| d.parse_banner(line).map(|machine| (machine, *d)))
            else {
                continue;
            };
            return Ok(Some(Handshake {
                dialect: dialect(machine.firmware).unwrap_or(found_by),
                machine,
                lines: lines[index + 1..].to_vec(),
                reader: std::mem::take(reader),
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bf_sim::{Event, Profile, Simulator};

    use super::*;
    use crate::Firmware;
    use crate::machine::sim::SimLink;

    fn received(sim: &Simulator) -> Vec<(Duration, Vec<u8>)> {
        sim.events()
            .iter()
            .filter_map(|event| match event {
                Event::Received { at, bytes } => Some((*at, bytes.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn detects_the_banner_printed_when_the_port_opens() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        let found = handshake(&mut link.transport(), &link.clock(), BANNER_TIMEOUT).unwrap();

        assert_eq!(found.machine.firmware, Firmware::Grbl);
        assert_eq!(found.machine.name, "Grbl");
        assert_eq!(found.machine.version, "1.1h");
        assert_eq!(found.machine.banner, "Grbl 1.1h ['$' for help]");
        assert_eq!(found.dialect.firmware(), Firmware::Grbl);
        let sim = link.sim();
        assert!(received(&sim).is_empty(), "no soft reset was needed");
        assert!(sim.now() < BANNER_TIMEOUT);
    }

    #[test]
    fn soft_resets_a_controller_that_stays_quiet() {
        // A board that doesn't reset on port open has printed its banner long ago.
        let mut sim = Simulator::new(Profile::grbl_defaults());
        sim.take_output();
        let link = SimLink::new(sim);

        let found = handshake(&mut link.transport(), &link.clock(), BANNER_TIMEOUT).unwrap();

        assert_eq!(found.machine.version, "1.1h");
        let sent = received(&link.sim());
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1, [0x18]);
        assert!(sent[0].0 >= BANNER_TIMEOUT, "reset only after the timeout");
    }

    #[test]
    fn keeps_the_lines_that_follow_the_banner() {
        let mut profile = Profile::grbl_defaults();
        profile.settings.set(22, 1.0).unwrap(); // homing on: powers up in alarm
        let link = SimLink::new(Simulator::new(profile));

        let found = handshake(&mut link.transport(), &link.clock(), BANNER_TIMEOUT).unwrap();

        assert_eq!(found.lines, ["[MSG:'$H'|'$X' to unlock]"]);
    }

    #[test]
    fn reports_no_banner_when_the_baud_rate_is_wrong() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        link.garble(true);

        let error = handshake(&mut link.transport(), &link.clock(), BANNER_TIMEOUT)
            .err()
            .unwrap();

        assert!(matches!(error, HandshakeError::NoBanner));
        assert!(error.to_string().contains("baud rate"));
        let sim = link.sim();
        assert_eq!(received(&sim).len(), 1, "one soft reset, then give up");
        assert!(sim.now() >= BANNER_TIMEOUT * 2);
    }

    #[test]
    fn reports_a_port_that_fails() {
        let link = SimLink::new(Simulator::new(Profile::grbl_defaults()));
        link.unplug();

        let error = handshake(&mut link.transport(), &link.clock(), BANNER_TIMEOUT)
            .err()
            .unwrap();

        assert!(matches!(error, HandshakeError::Io(_)), "{error}");
    }
}
