//! Pre-run checks: what must be true of the machine and the job before the
//! laser may fire.
//!
//! [`preflight`] looks at the machine profile (the `$$` settings it
//! mirrors), the latest status report, whether the machine was homed on this
//! connection and the generated job's summary, and returns one
//! [`PreflightCheck`] per question. Each check is a pure function, so it is
//! tested against a simulated machine state without a machine.
//!
//! A check that blocks stops the job starting at all (the machine would
//! refuse or garble it). A warning lets the job start once the user has seen
//! it and said so: the run command takes the warnings the user acknowledged
//! and refuses if any other is raised by the time the job would start.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{MachineProfile, MachineState, MachineStatus, OVERRIDE_DEFAULT};
use crate::gcode::JobSummary;

/// Which question a check answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PreflightCheckId {
    /// Idle (or in check mode), not in alarm, not busy.
    MachineReady,
    /// Laser mode (`$32=1`).
    LaserMode,
    /// Homed on this connection, when homing is enabled (`$22=1`).
    Homed,
    /// The job stays on the bed.
    Bounds,
    /// Feed, rapid and laser power overrides at 100%.
    Overrides,
}

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CheckLevel {
    Pass,
    /// The job may start once the user acknowledges it.
    Warn,
    /// The job can't start until this is fixed.
    Block,
}

/// One line of the pre-run checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PreflightCheck {
    pub id: PreflightCheckId,
    pub level: CheckLevel,
    /// A few words, e.g. "Laser mode is off ($32=0)".
    pub title: String,
    /// What it means and what to do, for anything but a pass.
    pub detail: Option<String>,
}

impl PreflightCheck {
    fn pass(id: PreflightCheckId, title: impl Into<String>) -> Self {
        PreflightCheck {
            id,
            level: CheckLevel::Pass,
            title: title.into(),
            detail: None,
        }
    }

    fn flag(
        id: PreflightCheckId,
        level: CheckLevel,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        PreflightCheck {
            id,
            level,
            title: title.into(),
            detail: Some(detail.into()),
        }
    }
}

/// The checklist for one job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Preflight {
    pub checks: Vec<PreflightCheck>,
}

impl Preflight {
    /// True when every check passed, so the job can start without asking.
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.level == CheckLevel::Pass)
    }

    /// Why the job may not start, given the warnings the user acknowledged:
    /// the first blocking check, or the first warning not acknowledged.
    pub fn refusal(&self, acknowledged: &[PreflightCheckId]) -> Option<String> {
        let explain = |check: &PreflightCheck| match &check.detail {
            Some(detail) => format!("{}. {detail}", check.title),
            None => check.title.clone(),
        };
        if let Some(block) = self.checks.iter().find(|c| c.level == CheckLevel::Block) {
            return Some(explain(block));
        }
        self.checks
            .iter()
            .find(|c| c.level == CheckLevel::Warn && !acknowledged.contains(&c.id))
            .map(|warn| format!("{} Check it before starting.", explain(warn)))
    }
}

/// Runs every check, in the order the checklist shows them.
pub fn preflight(
    profile: Option<&MachineProfile>,
    status: Option<&MachineStatus>,
    homed: bool,
    summary: &JobSummary,
) -> Preflight {
    Preflight {
        checks: vec![
            check_machine_ready(status),
            check_laser_mode(profile),
            check_homed(profile, homed),
            check_bounds(summary),
            check_overrides(status),
        ],
    }
}

/// Idle (or check mode) and not in alarm. A busy or alarmed controller
/// would refuse the job's lines or run them on top of something else.
pub fn check_machine_ready(status: Option<&MachineStatus>) -> PreflightCheck {
    use PreflightCheckId::MachineReady as Id;
    match status.map(|status| status.state) {
        None => PreflightCheck::flag(
            Id,
            CheckLevel::Block,
            "No status from the machine yet",
            "Wait a moment for it to report, then start again.",
        ),
        Some(MachineState::Idle) => PreflightCheck::pass(Id, "Machine idle"),
        Some(MachineState::Check) => PreflightCheck::pass(Id, "Check mode: the job won't move"),
        Some(MachineState::Alarm) => PreflightCheck::flag(
            Id,
            CheckLevel::Block,
            "Machine in alarm",
            "Home or unlock it first.",
        ),
        Some(state) => PreflightCheck::flag(
            Id,
            CheckLevel::Block,
            format!("Machine busy ({})", state_name(state)),
            "Wait for it to be idle.",
        ),
    }
}

/// `$32=1`. Without laser mode GRBL comes to a stop at every move, so the
/// laser dwells and burns dots at corners, and a feed hold leaves it on.
pub fn check_laser_mode(profile: Option<&MachineProfile>) -> PreflightCheck {
    use PreflightCheckId::LaserMode as Id;
    match profile {
        None => PreflightCheck::flag(
            Id,
            CheckLevel::Warn,
            "Machine settings not read",
            "Laser mode ($32) is unknown. Open the machine settings to read them.",
        ),
        Some(profile) if profile.laser_mode => PreflightCheck::pass(Id, "Laser mode on ($32=1)"),
        Some(_) => PreflightCheck::flag(
            Id,
            CheckLevel::Warn,
            "Laser mode is off ($32=0)",
            "GRBL stops at every move, leaving burn marks, and keeps the laser on while paused. \
             Turn on $32 in the machine settings.",
        ),
    }
}

/// With homing enabled (`$22=1`) positions mean nothing until the machine
/// has homed. GRBL can't say whether it has, so this trusts a `$H` that
/// succeeded on this connection, until an alarm.
pub fn check_homed(profile: Option<&MachineProfile>, homed: bool) -> PreflightCheck {
    use PreflightCheckId::Homed as Id;
    match profile {
        None => PreflightCheck::pass(Id, "Homing not checked (settings not read)"),
        Some(profile) if !profile.homing => PreflightCheck::pass(Id, "Homing off ($22=0)"),
        Some(_) if homed => PreflightCheck::pass(Id, "Homed"),
        Some(_) => PreflightCheck::flag(
            Id,
            CheckLevel::Warn,
            "Not homed since connecting",
            "Homing is on ($22=1) but the machine hasn't homed on this connection, so its \
             position may be off. Home it from the Machine panel.",
        ),
    }
}

/// The job, placed as it will run, stays on the bed.
pub fn check_bounds(summary: &JobSummary) -> PreflightCheck {
    use PreflightCheckId::Bounds as Id;
    if summary.inside_bed {
        PreflightCheck::pass(Id, "Job inside the bed")
    } else {
        PreflightCheck::flag(
            Id,
            CheckLevel::Warn,
            "Job leaves the bed",
            summary
                .warning
                .clone()
                .unwrap_or_else(|| "Part of the job is outside the bed.".to_string()),
        )
    }
}

/// GRBL keeps overrides between jobs. Starting a job resets them to 100%
/// (see [`ConnectionHandle::start_job`](super::ConnectionHandle::start_job));
/// this says so when the last report had any elsewhere.
pub fn check_overrides(status: Option<&MachineStatus>) -> PreflightCheck {
    use PreflightCheckId::Overrides as Id;
    let Some(overrides) = status.and_then(|status| status.overrides) else {
        return PreflightCheck::pass(Id, "Overrides reset to 100% at start");
    };
    let changed: Vec<String> = [
        ("feed", overrides.feed),
        ("rapids", overrides.rapid),
        ("laser power", overrides.spindle),
    ]
    .into_iter()
    .filter(|&(_, percent)| percent != OVERRIDE_DEFAULT)
    .map(|(name, percent)| format!("{name} {percent}%"))
    .collect();
    if changed.is_empty() {
        PreflightCheck::pass(Id, "Overrides at 100%")
    } else {
        PreflightCheck::flag(
            Id,
            CheckLevel::Warn,
            "Overrides left from an earlier job",
            format!(
                "The machine has {}. Starting resets them to 100%; adjust them again while the \
                 job runs if you need to.",
                changed.join(", ")
            ),
        )
    }
}

fn state_name(state: MachineState) -> &'static str {
    match state {
        MachineState::Idle => "idle",
        MachineState::Run => "running",
        MachineState::Hold { .. } => "holding",
        MachineState::Jog => "jogging",
        MachineState::Alarm => "alarm",
        MachineState::Door { .. } => "door open",
        MachineState::Check => "check mode",
        MachineState::Home => "homing",
        MachineState::Sleep => "asleep",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcode::JobBounds;
    use crate::machine::{Overrides, StatusReport, StatusTracker};
    use crate::project::Point;

    fn status(state: MachineState, overrides: Option<Overrides>) -> MachineStatus {
        StatusTracker::new().update(StatusReport {
            state: Some(state),
            overrides,
            ..StatusReport::default()
        })
    }

    fn profile(laser_mode: bool, homing: bool) -> MachineProfile {
        MachineProfile {
            laser_mode,
            homing,
            ..MachineProfile::default()
        }
    }

    fn summary(inside_bed: bool) -> JobSummary {
        let corner = Point { x: 0.0, y: 0.0 };
        JobSummary {
            bounds: JobBounds {
                min: corner,
                max: corner,
            },
            inside_bed,
            warning: (!inside_bed).then(|| "The job goes up to 5 mm outside the bed.".into()),
            lines: 3,
            cut_length: 1.0,
            travel_length: 1.0,
        }
    }

    const HUNDRED: Overrides = Overrides {
        feed: 100,
        rapid: 100,
        spindle: 100,
    };

    #[test]
    fn machine_ready_passes_idle_or_check_mode_and_blocks_otherwise() {
        let level = |state| check_machine_ready(Some(&status(state, None))).level;
        assert_eq!(level(MachineState::Idle), CheckLevel::Pass);
        assert_eq!(level(MachineState::Check), CheckLevel::Pass);
        assert_eq!(level(MachineState::Alarm), CheckLevel::Block);
        assert_eq!(level(MachineState::Run), CheckLevel::Block);
        assert_eq!(level(MachineState::Hold { code: 0 }), CheckLevel::Block);
        assert_eq!(level(MachineState::Door { code: 1 }), CheckLevel::Block);
        assert_eq!(check_machine_ready(None).level, CheckLevel::Block);
        let alarm = check_machine_ready(Some(&status(MachineState::Alarm, None)));
        assert_eq!(alarm.title, "Machine in alarm");
        let jog = check_machine_ready(Some(&status(MachineState::Jog, None)));
        assert_eq!(jog.title, "Machine busy (jogging)");
    }

    #[test]
    fn laser_mode_passes_with_32_on_and_warns_when_off_or_unknown() {
        assert_eq!(
            check_laser_mode(Some(&profile(true, false))).level,
            CheckLevel::Pass
        );
        let off = check_laser_mode(Some(&profile(false, false)));
        assert_eq!(off.level, CheckLevel::Warn);
        assert_eq!(off.title, "Laser mode is off ($32=0)");
        assert_eq!(check_laser_mode(None).level, CheckLevel::Warn);
    }

    #[test]
    fn homed_passes_when_homing_is_off_or_done_and_warns_otherwise() {
        assert_eq!(
            check_homed(Some(&profile(true, false)), false).level,
            CheckLevel::Pass
        );
        assert_eq!(
            check_homed(Some(&profile(true, true)), true).level,
            CheckLevel::Pass
        );
        let not_homed = check_homed(Some(&profile(true, true)), false);
        assert_eq!(not_homed.level, CheckLevel::Warn);
        assert_eq!(not_homed.title, "Not homed since connecting");
        assert_eq!(check_homed(None, false).level, CheckLevel::Pass);
    }

    #[test]
    fn bounds_pass_inside_the_bed_and_warn_with_the_summary_warning_outside() {
        assert_eq!(check_bounds(&summary(true)).level, CheckLevel::Pass);
        let outside = check_bounds(&summary(false));
        assert_eq!(outside.level, CheckLevel::Warn);
        assert_eq!(
            outside.detail.as_deref(),
            Some("The job goes up to 5 mm outside the bed.")
        );
    }

    #[test]
    fn overrides_pass_at_100_and_warn_that_others_will_reset() {
        let idle = |overrides| check_overrides(Some(&status(MachineState::Idle, overrides)));
        assert_eq!(idle(Some(HUNDRED)).level, CheckLevel::Pass);
        assert_eq!(idle(None).level, CheckLevel::Pass, "not reported yet");
        assert_eq!(check_overrides(None).level, CheckLevel::Pass);
        let left = idle(Some(Overrides {
            feed: 50,
            rapid: 100,
            spindle: 80,
        }));
        assert_eq!(left.level, CheckLevel::Warn);
        let detail = left.detail.unwrap();
        assert!(
            detail.contains("feed 50%, laser power 80%") && detail.contains("resets them to 100%"),
            "{detail}"
        );
    }

    #[test]
    fn a_ready_machine_passes_everything() {
        let idle = status(MachineState::Idle, Some(HUNDRED));
        let checks = preflight(
            Some(&profile(true, true)),
            Some(&idle),
            true,
            &summary(true),
        );
        assert!(checks.passed(), "{checks:?}");
        assert_eq!(checks.refusal(&[]), None);
        let ids: Vec<_> = checks.checks.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            [
                PreflightCheckId::MachineReady,
                PreflightCheckId::LaserMode,
                PreflightCheckId::Homed,
                PreflightCheckId::Bounds,
                PreflightCheckId::Overrides,
            ]
        );
    }

    #[test]
    fn warnings_need_acknowledging_and_blocks_refuse_regardless() {
        let idle = status(MachineState::Idle, Some(HUNDRED));
        let checks = preflight(
            Some(&profile(false, true)),
            Some(&idle),
            false,
            &summary(false),
        );
        assert!(!checks.passed());
        let refusal = checks.refusal(&[PreflightCheckId::LaserMode]).unwrap();
        assert!(
            refusal.starts_with("Not homed since connecting."),
            "{refusal}"
        );
        let all = [
            PreflightCheckId::LaserMode,
            PreflightCheckId::Homed,
            PreflightCheckId::Bounds,
        ];
        assert_eq!(checks.refusal(&all), None);

        let alarm = status(MachineState::Alarm, None);
        let checks = preflight(
            Some(&profile(true, false)),
            Some(&alarm),
            false,
            &summary(true),
        );
        assert_eq!(
            checks.refusal(&[PreflightCheckId::MachineReady]).as_deref(),
            Some("Machine in alarm. Home or unlock it first.")
        );
    }

    #[test]
    fn the_checklist_serializes_for_the_ui() {
        let check = check_laser_mode(Some(&profile(true, false)));
        assert_eq!(
            serde_json::to_value(&check).unwrap(),
            serde_json::json!({
                "id": "laserMode",
                "level": "pass",
                "title": "Laser mode on ($32=1)",
                "detail": null,
            })
        );
    }
}
