//! The toolpath preview: the moves a job's GCode is written from, in the
//! design's coordinates, so the canvas can draw them over the design, and
//! how long each takes on the machine ([`crate::estimate`]).
//!
//! [`toolpath`] places the job exactly as [`super::generate`] does and
//! returns the plan before it is moved into work coordinates. Placing only
//! mirrors and moves points, so the preview has the same moves, in the same
//! order, at the same power and speed as the GCode, and the same lengths.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{GcodeError, JobPlacement, PlacedJob};
use crate::estimate::JobEstimate;
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
    /// Estimated seconds each move takes, in the same order, with the
    /// machine's acceleration.
    pub move_seconds: Vec<f64>,
    /// The job's estimated time, by layer.
    pub estimate: JobEstimate,
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
    let (_, _, stops) = placed.write(document, &profile);
    Ok(placed.toolpath(&stops, &profile))
}

/// How long the job `document` would take on `profile`'s machine, placed
/// as `placement` says: [`toolpath`]'s estimate.
pub fn estimate(
    document: &DesignDocument,
    profile: &MachineProfile,
    placement: JobPlacement,
    current: Option<Point>,
) -> Result<JobEstimate, GcodeError> {
    Ok(toolpath(document, profile, placement, current)?.estimate)
}
