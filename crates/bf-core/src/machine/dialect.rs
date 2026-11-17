use crate::Firmware;
use crate::gcode::{FrameProgram, JobProgram, WrittenJob, number, write_grbl, write_grbl_frame};

use super::MachineInfo;
use super::overrides::{OverrideBytes, OverrideKind, grbl_override_bytes};
use super::reply::{Reply, decode_reply, error_text};
use super::status::{StatusError, StatusReport, is_status_report, parse_status};

/// How one firmware family speaks. GRBL is implemented; grblHAL, FluidNC,
/// Marlin and Smoothie plug in here later.
pub trait Dialect: Send + Sync {
    fn firmware(&self) -> Firmware;

    /// Recognises this firmware's welcome banner.
    fn parse_banner(&self, line: &str) -> Option<MachineInfo>;

    /// Bytes that soft-reset the controller, which makes it print its banner.
    fn soft_reset(&self) -> &'static [u8];

    /// The realtime byte that asks for a status report.
    fn status_query(&self) -> u8;

    /// The realtime byte that decelerates to a hold.
    fn feed_hold(&self) -> u8 {
        b'!'
    }

    /// The realtime byte that resumes from a hold.
    fn cycle_start(&self) -> u8 {
        b'~'
    }

    /// The realtime bytes that reset and nudge an override.
    fn override_bytes(&self, kind: OverrideKind) -> OverrideBytes {
        grbl_override_bytes(kind)
    }

    /// Realtime bytes that put the feed, rapid and spindle (laser power)
    /// overrides back to 100%.
    fn override_resets(&self) -> &'static [u8] {
        &[0x90, 0x95, 0x99]
    }

    /// The realtime byte for GRBL's spindle stop override: during a hold it
    /// turns the laser off, or back on if it is already off, without
    /// changing the program. Resuming restores the laser.
    fn spindle_stop(&self) -> u8 {
        0x9E
    }

    /// Bytes of command lines the controller can buffer before it has
    /// acknowledged them. GRBL's serial RX buffer holds 128; grblHAL's is
    /// larger, so 128 is safe for both.
    fn rx_buffer_size(&self) -> usize {
        128
    }

    /// Parses a status report. `None` when `line` isn't one at all.
    fn parse_status(&self, line: &str) -> Option<Result<StatusReport, StatusError>>;

    /// Classifies a line for the console, decoding error and alarm codes.
    fn decode_reply(&self, line: &str) -> Reply;

    /// Ends a command line. GRBL treats `\r` and `\n` each as an end of line,
    /// so `\r\n` would be acknowledged twice.
    fn line_ending(&self) -> &'static [u8] {
        b"\n"
    }

    /// Recognises the line that ends the reply to a command line: `Ok` for
    /// success, `Err` with a message for the user when it was rejected.
    /// `None` for any other line.
    fn parse_reply(&self, line: &str) -> Option<Result<(), String>>;

    /// True for a line the controller sends unprompted (an alarm, a
    /// message) rather than as part of a reply.
    fn is_push_message(&self, line: &str) -> bool;

    /// The command that prints every setting.
    fn settings_query(&self) -> &'static str;

    /// The command that writes `value` to setting `number`.
    fn write_setting(&self, number: u16, value: &str) -> String;

    /// Writes a placed job as this firmware's GCode, one command per line,
    /// saying which line makes each move. The same job must always give the
    /// same bytes.
    fn write_job(&self, job: &JobProgram) -> WrittenJob;

    /// Writes a job's frame: its outline traced with the laser off, or at
    /// the frame's low power, ending with the laser off.
    fn write_frame(&self, frame: &FrameProgram) -> String;

    // ---- manual control ----

    /// A jog by `dx`, `dy` mm from where the head is, at `feed` mm/min.
    /// An axis that doesn't move is left out.
    fn jog_by(&self, dx: f64, dy: f64, feed: f64) -> String;

    /// A jog to work position X0 Y0 at `feed` mm/min.
    fn jog_to_origin(&self, feed: f64) -> String;

    /// The realtime byte that stops a jog and drops the jogs queued after it.
    fn jog_cancel(&self) -> u8;

    /// Runs the homing cycle.
    fn home(&self) -> &'static str;

    /// Clears an alarm lock without homing.
    fn unlock(&self) -> &'static str;

    /// Makes the head's current position work X0 Y0.
    fn set_work_origin(&self) -> &'static str;

    /// Puts work X0 Y0 back on machine X0 Y0.
    fn clear_work_origin(&self) -> &'static str;

    /// Turns the laser on at `s` with the head still, for a test fire.
    fn laser_on(&self, s: f64) -> String;

    /// Turns the laser off.
    fn laser_off(&self) -> &'static str;

    /// Turns air assist off; the profile's command turns it on.
    fn air_assist_off(&self) -> &'static str;
}

/// GRBL 1.1, and for now grblHAL, which speaks the same protocol.
pub struct Grbl;

impl Dialect for Grbl {
    fn firmware(&self) -> Firmware {
        Firmware::Grbl
    }

    /// `Grbl 1.1h ['$' for help]` or `GrblHAL 1.1f ['$' or '$HELP' for help]`.
    fn parse_banner(&self, line: &str) -> Option<MachineInfo> {
        let line = line.trim();
        let (name, rest) = line.split_once(' ')?;
        let firmware = match name {
            "Grbl" => Firmware::Grbl,
            "GrblHAL" => Firmware::GrblHal,
            _ => return None,
        };
        let version = rest.split_whitespace().next()?;
        if !version.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        Some(MachineInfo {
            firmware,
            name: name.to_string(),
            version: version.to_string(),
            banner: line.to_string(),
        })
    }

    fn soft_reset(&self) -> &'static [u8] {
        &[0x18]
    }

    fn status_query(&self) -> u8 {
        b'?'
    }

    fn parse_status(&self, line: &str) -> Option<Result<StatusReport, StatusError>> {
        is_status_report(line).then(|| parse_status(line))
    }

    fn decode_reply(&self, line: &str) -> Reply {
        decode_reply(line)
    }

    fn parse_reply(&self, line: &str) -> Option<Result<(), String>> {
        let line = line.trim();
        if line == "ok" {
            return Some(Ok(()));
        }
        let code = line.strip_prefix("error:")?;
        Some(Err(match code.parse().ok().and_then(error_text) {
            Some((_, description)) => format!("GRBL error {code}: {description}"),
            None => format!("GRBL error {code}"),
        }))
    }

    fn is_push_message(&self, line: &str) -> bool {
        line.starts_with("ALARM:") || line.starts_with("[MSG:")
    }

    fn settings_query(&self) -> &'static str {
        "$$"
    }

    fn write_setting(&self, number: u16, value: &str) -> String {
        format!("${number}={value}")
    }

    fn write_job(&self, job: &JobProgram) -> WrittenJob {
        write_grbl(job)
    }

    fn write_frame(&self, frame: &FrameProgram) -> String {
        write_grbl_frame(frame)
    }

    /// `$J=G91 G21 X10 Y-10 F3000`: incremental and in mm whatever the
    /// parser's modes, which a jog line doesn't change.
    fn jog_by(&self, dx: f64, dy: f64, feed: f64) -> String {
        let mut line = "$J=G91 G21".to_string();
        for (axis, delta) in [("X", dx), ("Y", dy)] {
            if number(delta) != "0" {
                line.push_str(&format!(" {axis}{}", number(delta)));
            }
        }
        line.push_str(&format!(" F{}", number(feed)));
        line
    }

    fn jog_to_origin(&self, feed: f64) -> String {
        format!("$J=G90 G21 X0 Y0 F{}", number(feed))
    }

    fn jog_cancel(&self) -> u8 {
        0x85
    }

    fn home(&self) -> &'static str {
        "$H"
    }

    fn unlock(&self) -> &'static str {
        "$X"
    }

    fn set_work_origin(&self) -> &'static str {
        "G10 L20 P1 X0 Y0"
    }

    fn clear_work_origin(&self) -> &'static str {
        "G10 L2 P1 X0 Y0"
    }

    /// `G1 F1000 M3 S10`. In laser mode GRBL holds the laser off unless the
    /// motion mode is G1-G3 (so `M4`, or `M3` under `G0`, stays dark with the
    /// head still), and G1 needs a feed rate even though nothing moves.
    fn laser_on(&self, s: f64) -> String {
        format!("G1 F1000 M3 S{}", number(s))
    }

    fn laser_off(&self) -> &'static str {
        "M5"
    }

    fn air_assist_off(&self) -> &'static str {
        "M9"
    }
}

/// Dialects tried, in order, when looking for a banner.
pub(crate) const DIALECTS: &[&dyn Dialect] = &[&Grbl];

/// The dialect that drives `firmware`, if Beam Forge supports it yet.
pub fn dialect(firmware: Firmware) -> Option<&'static dyn Dialect> {
    match firmware {
        // grblHAL is GRBL 1.1-compatible until it gets its own dialect.
        Firmware::Grbl | Firmware::GrblHal => Some(&Grbl),
        Firmware::FluidNc | Firmware::Marlin | Firmware::Smoothie => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grbl_and_grblhal_banners() {
        let grbl = Grbl.parse_banner("Grbl 1.1h ['$' for help]").unwrap();
        assert_eq!(
            (grbl.firmware, grbl.name.as_str(), grbl.version.as_str()),
            (Firmware::Grbl, "Grbl", "1.1h")
        );
        let hal = Grbl
            .parse_banner("GrblHAL 1.1f ['$' or '$HELP' for help]")
            .unwrap();
        assert_eq!(
            (hal.firmware, hal.version.as_str()),
            (Firmware::GrblHal, "1.1f")
        );
    }

    #[test]
    fn ignores_lines_that_are_not_banners() {
        for line in [
            "",
            "ok",
            "[MSG:'$H'|'$X' to unlock]",
            "Grbl",
            "Grbl is great",
        ] {
            assert_eq!(Grbl.parse_banner(line), None, "{line:?}");
        }
    }

    #[test]
    fn every_dialect_is_keyed_by_its_firmware() {
        for d in DIALECTS {
            assert_eq!(dialect(d.firmware()).unwrap().firmware(), d.firmware());
        }
        assert!(dialect(Firmware::Marlin).is_none());
    }

    #[test]
    fn grbl_recognises_status_reports() {
        assert!(Grbl.parse_status("ok").is_none());
        let report = Grbl.parse_status("<Idle|MPos:0,0,0|FS:0,0>").unwrap();
        assert!(report.is_ok());
        assert_eq!(Grbl.status_query(), b'?');
    }

    #[test]
    fn grbl_replies_end_with_ok_or_an_explained_error() {
        assert_eq!(Grbl.parse_reply("ok"), Some(Ok(())));
        assert_eq!(
            Grbl.parse_reply("error:4"),
            Some(Err(
                "GRBL error 4: Negative value received for an expected positive value.".into()
            ))
        );
        assert_eq!(
            Grbl.parse_reply("error:99"),
            Some(Err("GRBL error 99".into()))
        );
        assert_eq!(Grbl.parse_reply("$32=1"), None);
        assert!(Grbl.is_push_message("ALARM:1"));
        assert!(!Grbl.is_push_message("$32=1"));
        assert_eq!(Grbl.write_setting(32, "1"), "$32=1");
    }

    #[test]
    fn grbl_manual_control_commands() {
        assert_eq!(Grbl.jog_to_origin(3000.0), "$J=G90 G21 X0 Y0 F3000");
        assert_eq!(Grbl.jog_cancel(), 0x85);
        assert_eq!(Grbl.home(), "$H");
        assert_eq!(Grbl.unlock(), "$X");
        assert_eq!(Grbl.set_work_origin(), "G10 L20 P1 X0 Y0");
        assert_eq!(Grbl.clear_work_origin(), "G10 L2 P1 X0 Y0");
        assert_eq!(Grbl.laser_on(10.0), "G1 F1000 M3 S10");
        assert_eq!(Grbl.laser_off(), "M5");
        assert_eq!(Grbl.air_assist_off(), "M9");
    }

    #[test]
    fn grbl_ends_lines_with_newline_only() {
        assert_eq!(Grbl.line_ending(), b"\n");
    }
}
