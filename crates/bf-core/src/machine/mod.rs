//! Machine driver: everything above the byte transport, identical for a USB
//! serial port and the simulator's TCP socket.
//!
//! [`Connection::open`] runs the [`handshake`] on a [`Transport`], then hands
//! the transport to a single owner thread that writes queued commands and
//! reports inbound lines, polled [`status`] reports and disconnects as
//! [`MachineEvent`]s.

mod connection;
mod control;
mod dialect;
mod handshake;
mod job;
mod lines;
mod overrides;
mod preflight;
mod profile;
mod reply;
mod settings;
mod status;
mod transport;

#[cfg(test)]
mod sim;

pub use connection::{
    Connection, ConnectionHandle, JOB_PROGRESS_INTERVAL, MachineEvent, QueryError, SendError,
};
pub use control::{
    JOG_HOLD_TIMEOUT, JOG_STEPS, JogDirection, TEST_FIRE_MAX_MS, TEST_FIRE_MAX_POWER, TestFire,
};
pub use dialect::{Dialect, Grbl, dialect};
pub use handshake::{BANNER_TIMEOUT, Handshake, HandshakeError, handshake};
pub use job::{Job, JobError, JobLines, JobProgress, JobState, job_line};
pub use lines::LineReader;
pub use overrides::{
    OVERRIDE_DEFAULT, OVERRIDE_MAX, OVERRIDE_MIN, OverrideBytes, OverrideKind, clamp_override,
    grbl_override_bytes, override_steps,
};
pub use preflight::{
    CheckLevel, Preflight, PreflightCheck, PreflightCheckId, check_bounds, check_homed,
    check_laser_mode, check_machine_ready, check_overrides, preflight,
};
pub(crate) use profile::valid_id;
pub use profile::{
    AxisValues, BedSize, MachineProfile, MachineSetup, OriginCorner, ProfileError, ProfileStore,
    SetupStep, StepOutcome, profile_id,
};
pub use reply::{Reply, ReplyCode, alarm_text, decode_reply, error_text};
pub use settings::{
    DEFINITIONS, MachineSetting, SettingDefinition, SettingError, SettingKind, Settings,
    definition, parse_setting_line, validate_setting,
};
pub use status::{
    Accessories, BufferState, MachineState, MachineStatus, Overrides, Pins, Position, StatusError,
    StatusReport, StatusTracker, is_status_report, parse_status,
};
pub use transport::{Clock, SystemClock, Transport};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Firmware;

/// Default baud rate for GRBL controllers, the Phecda included.
pub const DEFAULT_BAUD: u32 = 115_200;

/// Where the simulator app listens for a TCP connection.
pub const SIMULATOR_HOST: &str = "127.0.0.1";
pub const SIMULATOR_PORT: u16 = 23232;

/// How to reach a controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum PortAddress {
    /// A serial port such as `COM3` or `/dev/ttyUSB0`.
    Serial { path: String },
    /// A TCP socket, used for the simulator.
    Tcp { host: String, port: u16 },
    /// The simulator bundled with the app, started on connect on a port the
    /// OS picks and closed on disconnect.
    Launch,
}

impl PortAddress {
    pub fn simulator() -> Self {
        PortAddress::Tcp {
            host: SIMULATOR_HOST.to_string(),
            port: SIMULATOR_PORT,
        }
    }

    /// Whether this address may reach a real laser rather than a simulator:
    /// any serial port, and TCP to anything but this computer (a network
    /// bridge such as an ESP32 board). The bundled simulator and loopback
    /// TCP (the simulator app) are taken to be simulators.
    pub fn may_be_hardware(&self) -> bool {
        match self {
            PortAddress::Serial { .. } => true,
            PortAddress::Tcp { host, .. } => !is_loopback(host),
            PortAddress::Launch => false,
        }
    }

    /// A stable key for remembering the last port used.
    pub fn key(&self) -> String {
        match self {
            PortAddress::Serial { path } => format!("serial:{path}"),
            PortAddress::Tcp { host, port } => format!("tcp:{host}:{port}"),
            PortAddress::Launch => "launch:simulator".to_string(),
        }
    }
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A port the user can pick, with USB details when the OS reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PortInfo {
    pub key: String,
    pub address: PortAddress,
    pub label: String,
    pub description: Option<String>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

impl PortInfo {
    pub fn simulator() -> Self {
        let address = PortAddress::simulator();
        PortInfo {
            key: address.key(),
            label: format!("Simulator ({SIMULATOR_HOST}:{SIMULATOR_PORT})"),
            description: Some("Beam Forge simulator app".to_string()),
            address,
            vid: None,
            pid: None,
        }
    }

    /// The bundled simulator, started by the app when it connects.
    pub fn built_in_simulator() -> Self {
        let address = PortAddress::Launch;
        PortInfo {
            key: address.key(),
            label: "Built-in simulator".to_string(),
            description: Some("Starts the simulator bundled with Beam Forge".to_string()),
            address,
            vid: None,
            pid: None,
        }
    }
}

/// What the handshake learned from the welcome banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MachineInfo {
    pub firmware: Firmware,
    /// Firmware name as the banner spells it, e.g. `Grbl` or `GrblHAL`.
    pub name: String,
    /// Version as the banner spells it, e.g. `1.1h`.
    pub version: String,
    pub banner: String,
}

/// Connection state shown in the status bar and the connection panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum ConnectionState {
    /// Not connected. `reason` explains an unexpected disconnect or a failed
    /// connect; it is absent after the user disconnects.
    Disconnected {
        reason: Option<String>,
    },
    Connecting {
        port: PortInfo,
    },
    Connected {
        port: PortInfo,
        machine: MachineInfo,
    },
}

/// An inbound line from the controller, emitted to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MachineLine {
    pub line: String,
    /// What the line means, decoded by the connection's dialect.
    pub reply: Reply,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulator_port_is_labelled_with_its_address() {
        let port = PortInfo::simulator();
        assert_eq!(port.label, "Simulator (127.0.0.1:23232)");
        assert_eq!(port.key, "tcp:127.0.0.1:23232");
        let built_in = PortInfo::built_in_simulator();
        assert_eq!(built_in.label, "Built-in simulator");
        assert_eq!(built_in.key, "launch:simulator");
    }

    #[test]
    fn only_simulator_addresses_skip_the_real_hardware_gate() {
        let tcp = |host: &str| PortAddress::Tcp {
            host: host.into(),
            port: 23,
        };
        assert!(
            PortAddress::Serial {
                path: "/dev/ttyUSB0".into()
            }
            .may_be_hardware()
        );
        assert!(tcp("192.168.1.40").may_be_hardware());
        assert!(tcp("laser.local").may_be_hardware());
        assert!(!tcp("127.0.0.1").may_be_hardware());
        assert!(!tcp("127.8.0.1").may_be_hardware());
        assert!(!tcp("localhost").may_be_hardware());
        assert!(!tcp("::1").may_be_hardware());
        assert!(!tcp("[::1]").may_be_hardware());
        assert!(!PortAddress::simulator().may_be_hardware());
        assert!(!PortAddress::Launch.may_be_hardware());
    }

    #[test]
    fn contract_types_serialize_with_tags_for_the_ui() {
        let state = ConnectionState::Disconnected { reason: None };
        assert_eq!(
            serde_json::to_string(&state).unwrap(),
            r#"{"status":"disconnected","reason":null}"#
        );
        let address = PortAddress::Serial {
            path: "COM3".into(),
        };
        assert_eq!(
            serde_json::to_string(&address).unwrap(),
            r#"{"kind":"serial","path":"COM3"}"#
        );
        assert_eq!(
            serde_json::to_string(&PortAddress::Launch).unwrap(),
            r#"{"kind":"launch"}"#
        );
    }
}
