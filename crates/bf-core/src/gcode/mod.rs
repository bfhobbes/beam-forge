//! GCode generation: a planned job, placed on the bed, written in the
//! machine's firmware [`Dialect`].
//!
//! [`generate`] plans the document ([`crate::plan`]), places the job where
//! the [`JobPlacement`] says, checks it against the bed and has the dialect
//! write it. The output depends only on its inputs, so the same document,
//! profile and placement always give the same bytes.
//!
//! Coordinates: the document's origin is the bed's front-left corner, seen
//! from the front, with Y towards the rear. A profile whose origin is another
//! corner measures from that corner instead, so X and/or Y are mirrored.

mod frame;
mod grbl;
mod run;
mod toolpath;

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::estimate::{JobEstimate, MotionLimits, move_times};
use crate::machine::{Dialect, MachineProfile, OriginCorner, dialect};
use crate::plan::{LayerPlan, Move, Plan, PlanOptions, plan};
use crate::project::{CutLayer, DesignDocument, Point};

pub use frame::{
    FRAME_SPEED, FrameProgram, FrameShape, GeneratedFrame, MAX_FRAME_POWER, convex_hull, frame,
};
pub(crate) use grbl::{number, write as write_grbl, write_frame as write_grbl_frame};
pub use run::{DesignRun, RunPlan, WorkPlacement, run};
pub use toolpath::{Toolpath, estimate, toolpath};

/// Positions closer to the bed edge than this still count as on the bed.
const BED_TOLERANCE: f64 = 1e-6;

/// Where a job's coordinates are measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub enum StartFrom {
    /// Where the design sits on the bed in the editor.
    Absolute,
    /// The job's anchor goes where the head is now (its work position).
    CurrentPosition,
    /// The job's anchor goes to the machine profile's user origin.
    UserOrigin,
}

impl StartFrom {
    pub fn label(self) -> &'static str {
        match self {
            StartFrom::Absolute => "absolute coordinates",
            StartFrom::CurrentPosition => "current position",
            StartFrom::UserOrigin => "user origin",
        }
    }
}

/// Which point of the job's bounding box goes to the start position, as the
/// design is seen in the editor. Unused for [`StartFrom::Absolute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub enum JobAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl JobAnchor {
    /// The anchor's fraction of the way across (x) and up (y) the box.
    fn fractions(self) -> (f64, f64) {
        match self {
            JobAnchor::TopLeft => (0.0, 1.0),
            JobAnchor::Top => (0.5, 1.0),
            JobAnchor::TopRight => (1.0, 1.0),
            JobAnchor::Left => (0.0, 0.5),
            JobAnchor::Center => (0.5, 0.5),
            JobAnchor::Right => (1.0, 0.5),
            JobAnchor::BottomLeft => (0.0, 0.0),
            JobAnchor::Bottom => (0.5, 0.0),
            JobAnchor::BottomRight => (1.0, 0.0),
        }
    }

    fn label(self) -> &'static str {
        match self {
            JobAnchor::TopLeft => "top left",
            JobAnchor::Top => "top",
            JobAnchor::TopRight => "top right",
            JobAnchor::Left => "left",
            JobAnchor::Center => "center",
            JobAnchor::Right => "right",
            JobAnchor::BottomLeft => "bottom left",
            JobAnchor::Bottom => "bottom",
            JobAnchor::BottomRight => "bottom right",
        }
    }
}

/// Where the job goes on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub struct JobPlacement {
    pub start_from: StartFrom,
    pub anchor: JobAnchor,
}

impl Default for JobPlacement {
    fn default() -> Self {
        JobPlacement {
            start_from: StartFrom::Absolute,
            anchor: JobAnchor::BottomLeft,
        }
    }
}

/// An axis-aligned box, mm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "gcode/")]
pub struct JobBounds {
    pub min: Point,
    pub max: Point,
}

impl JobBounds {
    fn around(points: impl IntoIterator<Item = Point>) -> Option<JobBounds> {
        points.into_iter().fold(None, |bounds, p| {
            Some(match bounds {
                None => JobBounds { min: p, max: p },
                Some(JobBounds { min, max }) => JobBounds {
                    min: Point {
                        x: min.x.min(p.x),
                        y: min.y.min(p.y),
                    },
                    max: Point {
                        x: max.x.max(p.x),
                        y: max.y.max(p.y),
                    },
                },
            })
        })
    }

    fn at(&self, anchor: JobAnchor) -> Point {
        let (fx, fy) = anchor.fractions();
        Point {
            x: self.min.x + (self.max.x - self.min.x) * fx,
            y: self.min.y + (self.max.y - self.min.y) * fy,
        }
    }
}

/// What a generated job does, for the UI to show before it is exported or
/// run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub struct JobSummary {
    /// Everywhere the head goes after it leaves the start, in the machine's
    /// work coordinates.
    pub bounds: JobBounds,
    /// False when the job leaves the bed; `warning` then says by how much.
    pub inside_bed: bool,
    pub warning: Option<String>,
    /// Command lines in the GCode.
    pub lines: u32,
    /// Distance lased, mm.
    pub cut_length: f64,
    /// Distance travelled with the laser off, mm.
    pub travel_length: f64,
    /// Names of the layers that cut at constant power (`M3`), in cut order.
    /// If the link drops during one, the laser stays on where the head stops.
    pub constant_power_layers: Vec<String>,
}

/// GCode and what it does.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedJob {
    pub gcode: String,
    pub summary: JobSummary,
}

impl GeneratedJob {
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.gcode.lines()
    }
}

/// What a dialect writes for a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenJob {
    pub gcode: String,
    /// For each move in plan order, the 1-based command line that makes it,
    /// counted as a job's progress counts acknowledged lines.
    pub move_lines: Vec<u32>,
    /// Moves, by index in plan order, that start from rest because the
    /// firmware waits for motion to finish before the commands ahead of them.
    pub stops: Vec<usize>,
}

/// Why a job couldn't be generated, worded for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcodeError(String);

impl fmt::Display for GcodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GcodeError {}

/// What a dialect writes: a placed plan in work coordinates and the layers
/// its moves name.
#[derive(Debug, Clone, Copy)]
pub struct JobProgram<'a> {
    /// Comment lines that open the file, without comment markers.
    pub header: &'a [String],
    /// The moves, in the machine's work coordinates.
    pub plan: &'a Plan,
    /// The document's layers; [`LayerPlan::layer_id`] names one of them.
    pub layers: &'a [CutLayer],
    pub profile: &'a MachineProfile,
}

impl JobProgram<'_> {
    /// The layer `plan` cuts with.
    pub fn layer(&self, plan: &LayerPlan) -> Option<&CutLayer> {
        self.layers.iter().find(|layer| layer.id == plan.layer_id)
    }
}

/// Names of the planned layers that cut at constant power (`M3`), in cut
/// order, each once.
fn constant_power_layers(plan: &Plan, layers: &[CutLayer]) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut names = Vec::new();
    for planned in &plan.layers {
        let Some(layer) = layers.iter().find(|layer| layer.id == planned.layer_id) else {
            continue;
        };
        if layer.constant_power && !seen.contains(&layer.id.as_str()) {
            seen.push(&layer.id);
            names.push(layer.name.clone());
        }
    }
    names
}

/// Plans `document` and writes it as GCode for `profile`'s firmware, placed
/// as `placement` says. `current` is the head's work position, which
/// [`StartFrom::CurrentPosition`] needs; [`StartFrom::UserOrigin`] reads the
/// profile's user origin.
pub fn generate(
    document: &DesignDocument,
    profile: &MachineProfile,
    placement: JobPlacement,
    current: Option<Point>,
) -> Result<GeneratedJob, GcodeError> {
    let placed = PlacedJob::new(document, profile, placement, current)?;
    Ok(placed.write(document, profile).0)
}

/// GCode command lines in `gcode`: not blank, not comments.
fn command_lines(gcode: &str) -> u32 {
    let lines = gcode
        .lines()
        .filter(|line| crate::machine::job_line(line).is_some())
        .count();
    u32::try_from(lines).unwrap_or(u32::MAX)
}

/// A planned job placed on the machine: what both the job and its frame are
/// made from, so the frame traces exactly where the job burns.
struct PlacedJob {
    dialect: &'static dyn Dialect,
    placement: JobPlacement,
    /// The planned moves in document coordinates, before placing: what the
    /// toolpath preview draws over the design.
    design: Plan,
    /// How document coordinates become work coordinates.
    corner: Corner,
    offset: Point,
    /// `design` in work coordinates, the moves the GCode is written from;
    /// `plan.start` is where the head is assumed to be before the first move.
    plan: Plan,
    /// Everywhere the head goes after it leaves the start.
    bounds: JobBounds,
    warning: Option<String>,
}

impl PlacedJob {
    fn new(
        document: &DesignDocument,
        profile: &MachineProfile,
        placement: JobPlacement,
        current: Option<Point>,
    ) -> Result<PlacedJob, GcodeError> {
        document
            .validate()
            .map_err(|error| GcodeError(error.to_string()))?;
        let dialect = dialect(profile.firmware).ok_or_else(|| {
            GcodeError(format!(
                "Beam Forge can't write GCode for {} yet.",
                profile.firmware.display_name()
            ))
        })?;
        let corner = Corner::of(profile);

        // Plan from where the head is: the bed origin, or the start position.
        let mut options = PlanOptions {
            start: corner.map(Point { x: 0.0, y: 0.0 }),
            ..PlanOptions::default()
        };
        let mut planned = plan(document, &options);
        let design = JobBounds::around(planned.moves().map(move_to)).ok_or_else(|| {
            GcodeError(
                "There is nothing to cut: no shapes are on a layer with output on.".to_string(),
            )
        })?;
        let offset = match placement.start_from {
            StartFrom::Absolute => Point { x: 0.0, y: 0.0 },
            relative => {
                let origin = match relative {
                    StartFrom::CurrentPosition => current.ok_or_else(|| {
                        GcodeError(
                            "Starting from the current position needs a connected machine \
                             that has reported where the head is."
                                .to_string(),
                        )
                    })?,
                    _ => profile.user_origin.ok_or_else(|| {
                        GcodeError(
                            "No user origin is set. Connect, move the head and choose \
                             \"Set user origin here\"."
                                .to_string(),
                        )
                    })?,
                };
                let anchor = design.at(placement.anchor);
                let mapped = corner.map(anchor);
                options.start = anchor;
                planned = plan(document, &options);
                Point {
                    x: origin.x - mapped.x,
                    y: origin.y - mapped.y,
                }
            }
        };
        let mut job = PlacedJob {
            dialect,
            placement,
            design: planned,
            corner,
            offset,
            plan: Plan {
                start: Point { x: 0.0, y: 0.0 },
                layers: Vec::new(),
            },
            bounds: design,
            warning: None,
        };
        job.plan = place(&job.design, |p| job.to_work(p));
        job.bounds = JobBounds::around(job.points()).unwrap_or(design);
        job.warning = bed_warning(&job.bounds, profile);
        Ok(job)
    }

    /// A document point in the machine's work coordinates, as the job is
    /// placed: mirrored for the origin corner, then moved by the offset.
    fn to_work(&self, p: Point) -> Point {
        let q = self.corner.map(p);
        Point {
            x: q.x + self.offset.x,
            y: q.y + self.offset.y,
        }
    }

    /// Writes the job in its dialect. Also says which command line makes
    /// each move, and which moves start from rest ([`WrittenJob::stops`]).
    fn write(
        &self,
        document: &DesignDocument,
        profile: &MachineProfile,
    ) -> (GeneratedJob, Vec<u32>, Vec<usize>) {
        let mut header = vec!["Beam Forge job".to_string()];
        header.extend(self.header());
        let written = self.dialect.write_job(&JobProgram {
            header: &header,
            plan: &self.plan,
            layers: &document.layers,
            profile,
        });
        let job = GeneratedJob {
            summary: JobSummary {
                bounds: self.bounds,
                inside_bed: self.warning.is_none(),
                lines: command_lines(&written.gcode),
                cut_length: self.plan.cut_length(),
                travel_length: self.plan.travel_length(),
                warning: self.warning.clone(),
                constant_power_layers: constant_power_layers(&self.plan, &document.layers),
            },
            gcode: written.gcode,
        };
        (job, written.move_lines, written.stops)
    }

    /// The toolpath preview of the job, timed on `profile`'s machine.
    /// `stops` are the written job's [`WrittenJob::stops`].
    fn toolpath(&self, stops: &[usize], profile: &MachineProfile) -> Toolpath {
        let move_seconds = move_times(&self.plan, &MotionLimits::of(profile), stops);
        Toolpath {
            estimate: JobEstimate::new(&self.plan, &move_seconds),
            plan: self.design.clone(),
            move_seconds,
        }
    }

    /// Comment lines saying where the job goes, and any bed warning.
    fn header(&self) -> Vec<String> {
        let bounds = &self.bounds;
        let mut header = vec![
            match self.placement.start_from {
                StartFrom::Absolute => "Start from: absolute coordinates".to_string(),
                other => format!(
                    "Start from: {}, job origin {}",
                    other.label(),
                    self.placement.anchor.label()
                ),
            },
            format!(
                "Bounds: X{} Y{} to X{} Y{}",
                grbl::number(bounds.min.x),
                grbl::number(bounds.min.y),
                grbl::number(bounds.max.x),
                grbl::number(bounds.max.y)
            ),
        ];
        if let Some(warning) = &self.warning {
            header.push(format!("Warning: {warning}"));
        }
        header
    }

    /// Every point the head moves to, in order.
    fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.plan.moves().map(move_to)
    }
}

fn move_to(step: &Move) -> Point {
    match *step {
        Move::Rapid { to } | Move::Cut { to, .. } => to,
    }
}

/// `plan` with every point moved by `map`.
fn place(plan: &Plan, map: impl Fn(Point) -> Point) -> Plan {
    Plan {
        start: map(plan.start),
        layers: plan
            .layers
            .iter()
            .map(|layer| LayerPlan {
                moves: layer
                    .moves
                    .iter()
                    .map(|step| match *step {
                        Move::Rapid { to } => Move::Rapid { to: map(to) },
                        Move::Cut { to, power, speed } => Move::Cut {
                            to: map(to),
                            power,
                            speed,
                        },
                    })
                    .collect(),
                ..layer.clone()
            })
            .collect(),
    }
}

/// How document coordinates become work coordinates for the profile's
/// origin corner: mirrored across the bed on the axes measured from the far
/// side.
#[derive(Debug, Clone, Copy)]
struct Corner {
    width: Option<f64>,
    height: Option<f64>,
}

impl Corner {
    fn of(profile: &MachineProfile) -> Corner {
        let (right, rear) = match profile.origin {
            OriginCorner::FrontLeft => (false, false),
            OriginCorner::FrontRight => (true, false),
            OriginCorner::RearLeft => (false, true),
            OriginCorner::RearRight => (true, true),
        };
        Corner {
            width: right.then_some(profile.bed.width),
            height: rear.then_some(profile.bed.height),
        }
    }

    /// Its own inverse.
    fn map(self, p: Point) -> Point {
        Point {
            x: self.width.map_or(p.x, |w| w - p.x),
            y: self.height.map_or(p.y, |h| h - p.y),
        }
    }
}

/// Why `bounds` don't fit on the bed, if they don't.
fn bed_warning(bounds: &JobBounds, profile: &MachineProfile) -> Option<String> {
    let (width, height) = (profile.bed.width, profile.bed.height);
    let over = [
        -bounds.min.x,
        bounds.max.x - width,
        -bounds.min.y,
        bounds.max.y - height,
    ]
    .into_iter()
    .fold(0.0_f64, f64::max);
    (over > BED_TOLERANCE).then(|| {
        format!(
            "The job goes up to {} mm outside the {} × {} mm bed.",
            grbl::number(over),
            grbl::number(width),
            grbl::number(height)
        )
    })
}

#[cfg(test)]
mod tests;
