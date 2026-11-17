//! Running the design: the job's GCode, generated in memory, and what the
//! UI needs to follow it on the canvas while it streams.
//!
//! [`run`] places the job once and returns both the GCode and its
//! [`RunPlan`]: the toolpath (the same moves, in document coordinates),
//! which command line makes each move, and how the machine's work
//! coordinates map back onto the design. The UI turns the job's
//! acknowledged lines and the head's reported position into "moves done"
//! with them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{GcodeError, GeneratedJob, JobPlacement, PlacedJob, Toolpath};
use crate::machine::MachineProfile;
use crate::project::{DesignDocument, Point};

/// How a placed job's document coordinates become the machine's work
/// coordinates: mirrored across the bed on an axis measured from the far
/// side (when the width or height is set), then moved by `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub struct WorkPlacement {
    /// Bed width when X is measured from the right.
    pub mirror_width: Option<f64>,
    /// Bed height when Y is measured from the rear.
    pub mirror_height: Option<f64>,
    pub offset: Point,
}

impl WorkPlacement {
    pub fn to_work(&self, p: Point) -> Point {
        Point {
            x: self.mirror_width.map_or(p.x, |w| w - p.x) + self.offset.x,
            y: self.mirror_height.map_or(p.y, |h| h - p.y) + self.offset.y,
        }
    }

    pub fn to_document(&self, p: Point) -> Point {
        let (x, y) = (p.x - self.offset.x, p.y - self.offset.y);
        Point {
            x: self.mirror_width.map_or(x, |w| w - x),
            y: self.mirror_height.map_or(y, |h| h - y),
        }
    }
}

/// What the UI follows a running design with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub struct RunPlan {
    /// The job's name, as its progress reports it.
    pub name: String,
    /// The moves the GCode is written from, in document coordinates.
    pub toolpath: Toolpath,
    /// For each move, in order, the 1-based command line that makes it, as
    /// the job's progress counts acknowledged lines. Never decreasing.
    pub move_lines: Vec<u32>,
    /// Maps the head's work position onto the toolpath.
    pub placement: WorkPlacement,
}

/// A design ready to stream, and how to follow it.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignRun {
    pub job: GeneratedJob,
    pub plan: RunPlan,
}

/// Generates the job `document` would be, placed as `placement` says, as
/// [`super::generate`] does, and the plan to follow it with while it runs.
pub fn run(
    document: &DesignDocument,
    profile: &MachineProfile,
    placement: JobPlacement,
    current: Option<Point>,
    name: impl Into<String>,
) -> Result<DesignRun, GcodeError> {
    let placed = PlacedJob::new(document, profile, placement, current)?;
    let (job, move_lines, stops) = placed.write(document, profile);
    let plan = RunPlan {
        name: name.into(),
        toolpath: placed.toolpath(&stops, profile),
        move_lines,
        placement: WorkPlacement {
            mirror_width: placed.corner.width,
            mirror_height: placed.corner.height,
            offset: placed.offset,
        },
    };
    Ok(DesignRun { job, plan })
}
