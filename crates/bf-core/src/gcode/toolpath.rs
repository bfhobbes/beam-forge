//! The toolpath preview: the moves a job's GCode is written from, in the
//! design's coordinates, so the canvas can draw them over the design.
//!
//! [`toolpath`] places the job exactly as [`super::generate`] does and
//! returns the plan before it is moved into work coordinates. Placing only
//! mirrors and moves points, so the preview has the same moves, in the same
//! order, at the same power and speed as the GCode, and the same lengths.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{GcodeError, JobPlacement, PlacedJob};
use crate::machine::MachineProfile;
use crate::plan::Plan;
use crate::project::{DesignDocument, Point};

/// What the toolpath preview draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub struct Toolpath {
    /// The job's moves in document coordinates (where the design sits in
    /// the editor), in the order the GCode makes them.
    pub plan: Plan,
    /// How fast rapids travel, mm/min: the slower of the profile's X and Y
    /// maximum rates (`$110`, `$111`).
    pub rapid_rate: f64,
}

/// The toolpath the job `document` would be, placed as `placement` says,
/// and refused for the same documents as [`super::generate`].
///
/// Where the head is (`current`) or the user origin only moves the placed
/// job across the bed: the moves in document coordinates don't depend on
/// them. So the preview needs neither, and works with no machine connected.
pub fn toolpath(
    document: &DesignDocument,
    profile: &MachineProfile,
    placement: JobPlacement,
    current: Option<Point>,
) -> Result<Toolpath, GcodeError> {
    const ANYWHERE: Point = Point { x: 0.0, y: 0.0 };
    let profile = MachineProfile {
        user_origin: profile.user_origin.or(Some(ANYWHERE)),
        ..profile.clone()
    };
    let current = current.or(Some(ANYWHERE));
    let placed = PlacedJob::new(document, &profile, placement, current)?;
    Ok(Toolpath {
        rapid_rate: profile.max_rate.x.min(profile.max_rate.y),
        plan: placed.design,
    })
}
