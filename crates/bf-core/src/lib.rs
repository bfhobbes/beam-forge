//! Beam Forge product logic, independent of Tauri so it can be unit-tested
//! without a window or a machine. Geometry, toolpath planning ([`plan`]),
//! GCode generation ([`gcode`]), file import ([`import`]) and the machine driver
//! ([`machine`]) live here.

pub mod gcode;
pub mod import;
pub mod machine;
pub mod plan;
pub mod project;
pub mod text;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Version of the core library, reported to the UI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GCode firmware families Beam Forge intends to drive. Each gets a dialect
/// implementation; GRBL is the first to be supported on hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Firmware {
    Grbl,
    GrblHal,
    FluidNc,
    Marlin,
    Smoothie,
}

impl Firmware {
    pub const ALL: [Firmware; 5] = [
        Firmware::Grbl,
        Firmware::GrblHal,
        Firmware::FluidNc,
        Firmware::Marlin,
        Firmware::Smoothie,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Firmware::Grbl => "GRBL",
            Firmware::GrblHal => "grblHAL",
            Firmware::FluidNc => "FluidNC",
            Firmware::Marlin => "Marlin",
            Firmware::Smoothie => "Smoothieware",
        }
    }
}

/// What the core library offers, shown in the About panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CoreInfo {
    pub version: String,
    pub firmware: Vec<FirmwareInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FirmwareInfo {
    pub id: Firmware,
    pub name: String,
}

pub fn core_info() -> CoreInfo {
    CoreInfo {
        version: VERSION.to_string(),
        firmware: Firmware::ALL
            .into_iter()
            .map(|id| FirmwareInfo {
                id,
                name: id.display_name().to_string(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_info_lists_every_firmware_once() {
        let info = core_info();
        assert_eq!(info.version, VERSION);
        let ids: Vec<_> = info.firmware.iter().map(|f| f.id).collect();
        assert_eq!(ids, Firmware::ALL);
    }

    #[test]
    fn firmware_serializes_in_camel_case_for_the_ui() {
        let json = serde_json::to_string(&Firmware::GrblHal).unwrap();
        assert_eq!(json, "\"grblHal\"");
    }
}
