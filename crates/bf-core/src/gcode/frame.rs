//! Framing: tracing the outline of where a job will burn, so the user can
//! check the material is under it before starting.
//!
//! [`frame`] places the job exactly as [`super::generate`] does (same start
//! from, job origin and origin corner), then traces its bounding box or, for
//! a rubber-band frame, the convex hull of everywhere the head goes. The
//! laser is off throughout unless the profile turns on a low-power frame
//! (`M4` at [`MachineProfile::frame_power`] percent, at most
//! [`MAX_FRAME_POWER`]). Either way it ends with `M5`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    GcodeError, JobBounds, JobPlacement, PlacedJob, StartFrom, command_lines, grbl::number,
};
use crate::machine::MachineProfile;
use crate::project::{DesignDocument, Point};

/// Frame feed rate, mm/min.
pub const FRAME_SPEED: f64 = 3000.0;

/// The most a low-power frame may use, percent of full power: enough to see
/// the dot, not enough to mark most materials at frame speed.
pub const MAX_FRAME_POWER: f64 = 5.0;

/// What the frame traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub enum FrameShape {
    /// The job's bounding box.
    #[default]
    Box,
    /// The convex hull of the job: a rubber band stretched around it.
    RubberBand,
}

impl FrameShape {
    fn label(self) -> &'static str {
        match self {
            FrameShape::Box => "bounding box",
            FrameShape::RubberBand => "rubber band",
        }
    }
}

/// What a dialect writes to frame a job.
#[derive(Debug, Clone, Copy)]
pub struct FrameProgram<'a> {
    /// Comment lines that open the file, without comment markers.
    pub header: &'a [String],
    /// The closed outline in work coordinates; the last point is the first.
    pub path: &'a [Point],
    /// Where the head goes afterwards, or `None` to stay at the path's end.
    pub park: Option<Point>,
    /// The `S` value of a low-power frame, or `None` with the laser off.
    pub power: Option<f64>,
    /// Feed rate along the path, mm/min.
    pub speed: f64,
}

/// Frame GCode and the outline it traces.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedFrame {
    pub gcode: String,
    /// The traced outline in work coordinates, closed (last point = first).
    pub path: Vec<Point>,
    /// The job's bounds, which the frame's bounds equal.
    pub bounds: JobBounds,
    /// Why the job doesn't fit the bed, if it doesn't.
    pub warning: Option<String>,
    /// Command lines in the GCode.
    pub lines: u32,
}

impl GeneratedFrame {
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.gcode.lines()
    }
}

/// GCode that traces `shape` around the job `document` would be, placed as
/// `placement` says. `current` is as for [`super::generate`]. When the job
/// starts from the current position or user origin, the head returns there
/// afterwards, so the job can start from where it was.
pub fn frame(
    document: &DesignDocument,
    profile: &MachineProfile,
    placement: JobPlacement,
    current: Option<Point>,
    shape: FrameShape,
) -> Result<GeneratedFrame, GcodeError> {
    let placed = PlacedJob::new(document, profile, placement, current)?;
    let mut path = match shape {
        FrameShape::Box => {
            let JobBounds { min, max } = placed.bounds;
            vec![
                min,
                Point { x: max.x, y: min.y },
                max,
                Point { x: min.x, y: max.y },
            ]
        }
        FrameShape::RubberBand => convex_hull(placed.points()),
    };
    path.push(path[0]);

    let percent = profile.frame_power.clamp(0.0, MAX_FRAME_POWER);
    let power = (percent > 0.0).then(|| percent / 100.0 * profile.max_spindle);
    let mut header = vec!["Beam Forge frame".to_string()];
    header.extend(placed.header());
    header.push(match power {
        None => format!("Frame: {}, laser off", shape.label()),
        Some(s) => format!(
            "Frame: {}, low power {}% (M4 S{})",
            shape.label(),
            number(percent),
            number(s)
        ),
    });
    let park = match placement.start_from {
        StartFrom::Absolute => None,
        StartFrom::CurrentPosition | StartFrom::UserOrigin => Some(placed.plan.start),
    };
    let gcode = placed.dialect.write_frame(&FrameProgram {
        header: &header,
        path: &path,
        park,
        power,
        speed: FRAME_SPEED,
    });
    Ok(GeneratedFrame {
        lines: command_lines(&gcode),
        gcode,
        path,
        bounds: placed.bounds,
        warning: placed.warning,
    })
}

/// The convex hull of `points`, counter-clockwise from the leftmost (then
/// lowest) point, without collinear points or a repeated first point. Empty
/// for no points; one or two points for a dot or a line.
pub fn convex_hull(points: impl IntoIterator<Item = Point>) -> Vec<Point> {
    let mut points: Vec<Point> = points.into_iter().collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross =
        |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    // Andrew's monotone chain: the lower hull left to right, then the upper
    // hull right to left.
    let mut hull: Vec<Point> = Vec::with_capacity(points.len() * 2);
    for pass in [
        &points[..],
        &points.iter().rev().copied().collect::<Vec<_>>()[..],
    ] {
        let floor = hull.len();
        for &p in pass {
            while hull.len() >= floor + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        // The last point starts the other chain.
        hull.pop();
    }
    hull
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn hull_drops_inside_and_collinear_points() {
        let points = [
            p(0.0, 0.0),
            p(5.0, 0.0),
            p(10.0, 0.0),
            p(10.0, 10.0),
            p(3.0, 4.0),
            p(0.0, 10.0),
            p(0.0, 10.0),
            p(5.0, 12.0),
        ];
        assert_eq!(
            convex_hull(points),
            [
                p(0.0, 0.0),
                p(10.0, 0.0),
                p(10.0, 10.0),
                p(5.0, 12.0),
                p(0.0, 10.0)
            ]
        );
    }

    #[test]
    fn hull_of_a_dot_or_a_line() {
        assert_eq!(convex_hull([]), Vec::<Point>::new());
        assert_eq!(convex_hull([p(1.0, 1.0), p(1.0, 1.0)]), [p(1.0, 1.0)]);
        assert_eq!(
            convex_hull([p(2.0, 2.0), p(1.0, 1.0), p(0.0, 0.0)]),
            [p(0.0, 0.0), p(2.0, 2.0)]
        );
    }
}
