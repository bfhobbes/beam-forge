//! Summarising a GCode file before it runs: where the head goes, how far it
//! cuts and travels, and whether it cuts at constant power (`M3`).
//!
//! [`scan`] follows the GRBL 1.1 modal state a file sets up (`G0`-`G3`,
//! `G90`/`G91`, `G20`/`G21`, `G17`-`G19`, `G54`-`G59`, `M3`/`M4`/`M5`, `S`)
//! without running it, and gives the same [`JobSummary`] a generated design
//! has, so the pre-run checks treat both alike. It is its own reading of
//! GRBL, independent of the simulator, which only cross-checks it in tests.
//!
//! Bounds are in work coordinates and compared with the bed as design jobs
//! are (`0..width`, `0..height`). Where the head is before the file runs is
//! unknown, so an axis counts towards the bounds once the file has put it
//! somewhere; an axis the file never moves stays where the head already is.
//! Constructs that move the work coordinates or the head in ways the scanner
//! doesn't follow (`G92`, `G53`, `G28`, ...) set
//! [`JobSummary::unknown_bounds`] instead of claiming the file fits.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::io::{self, BufRead};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{JobBounds, JobSummary, bed_warning};
use crate::machine::{MachineProfile, job_line};
use crate::project::Point;

const MM_PER_INCH: f64 = 25.4;
/// GRBL's `ARC_ANGULAR_TRAVEL_EPSILON`: an arc whose end is this close to
/// its start (in radians) is a full circle.
const ARC_EPSILON: f64 = 5e-7;

/// Why a file's bounds couldn't be worked out: the first thing it does that
/// the scanner can't place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "gcode/")]
pub struct UnknownBounds {
    /// What the file uses, worded to follow "the file uses", e.g. "G92".
    pub construct: String,
    /// 1-based line in the file, counting every line as an editor does.
    pub line: u32,
}

/// Summarises `gcode` for `profile`'s bed. See the module docs.
pub fn scan(gcode: &str, profile: &MachineProfile) -> JobSummary {
    let mut scanner = Scanner::new();
    for (index, line) in gcode.lines().enumerate() {
        scanner.line(line_number(index), line);
    }
    scanner.finish(profile)
}

/// [`scan`], reading the file a line at a time so a large raster job never
/// sits in memory.
pub fn scan_reader(reader: impl BufRead, profile: &MachineProfile) -> io::Result<JobSummary> {
    let mut scanner = Scanner::new();
    for (index, line) in reader.lines().enumerate() {
        scanner.line(line_number(index), &line?);
    }
    Ok(scanner.finish(profile))
}

fn line_number(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Rapid,
    Linear,
    Clockwise,
    CounterClockwise,
    /// `G80`: axis words alone don't move.
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spindle {
    Off,
    /// `M3`: the laser stays lit at `S` even if the head stops.
    Constant,
    /// `M4`: power follows speed, off when the head stops.
    Dynamic,
}

/// A G or M code times ten, so `G92.1` is 921.
fn code(value: f64) -> Option<u32> {
    let tenths = (value * 10.0).round();
    ((value * 10.0 - tenths).abs() < 1e-6 && (0.0..=10_000.0).contains(&tenths))
        .then_some(tenths as u32)
}

fn code_name(letter: char, code: u32) -> String {
    if code.is_multiple_of(10) {
        format!("{letter}{}", code / 10)
    } else {
        format!("{letter}{}.{}", code / 10, code % 10)
    }
}

/// The words of a command line, upper case, e.g. `[('G', 1.0), ('X', 5.0)]`.
/// `None` if GRBL couldn't read it either.
fn words(command: &str) -> Option<Vec<(char, f64)>> {
    let mut words = Vec::new();
    let mut chars = command.chars().filter(|c| !c.is_whitespace()).peekable();
    while let Some(letter) = chars.next() {
        let letter = letter.to_ascii_uppercase();
        if !letter.is_ascii_alphabetic() {
            return None;
        }
        let mut number = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && number.is_empty()) {
                number.push(c);
                chars.next();
            } else {
                break;
            }
        }
        words.push((letter, number.parse().ok()?));
    }
    Some(words)
}

/// What one command line asks for, before it is applied.
#[derive(Default)]
struct Block {
    motion: Option<Motion>,
    /// `X`, `Y`, in the file's units.
    axes: [Option<f64>; 2],
    /// `I`, `J`, in the file's units.
    offsets: [Option<f64>; 2],
    radius: Option<f64>,
    l: Option<f64>,
    /// `G10`, `G28`, `G30` or `G92` (times ten), which use the axis words
    /// themselves instead of moving to them.
    axis_command: Option<u32>,
    /// `G53`: this line's move is in machine coordinates.
    machine_coordinates: bool,
    /// Other codes that leave the bounds unknown, e.g. `G28.1` is harmless
    /// but `G92.1` isn't.
    unknown: Option<String>,
    program_end: bool,
}

struct Scanner {
    /// Work position, per axis, once the file has put the head somewhere.
    position: [Option<f64>; 2],
    motion: Motion,
    relative: bool,
    inches: bool,
    /// `G17`, `G18` or `G19`.
    plane: u32,
    /// `G54`..`G59`.
    coordinate_system: u32,
    spindle: Spindle,
    power: f64,
    commands: u32,
    cut_length: f64,
    travel_length: f64,
    /// Range of each axis over every position moved to.
    range: [Option<(f64, f64)>; 2],
    unknown: Option<UnknownBounds>,
    constant_power_line: Option<u32>,
}

impl Scanner {
    /// GRBL's state after a reset, which is what a job streamed on connect
    /// starts from.
    fn new() -> Self {
        Scanner {
            position: [None; 2],
            motion: Motion::Rapid,
            relative: false,
            inches: false,
            plane: 17,
            coordinate_system: 54,
            spindle: Spindle::Off,
            power: 0.0,
            commands: 0,
            cut_length: 0.0,
            travel_length: 0.0,
            range: [None; 2],
            unknown: None,
            constant_power_line: None,
        }
    }

    /// Remembers the first construct the bounds can't follow.
    fn unknown(&mut self, line: u32, construct: impl Into<String>) {
        if self.unknown.is_none() {
            self.unknown = Some(UnknownBounds {
                construct: construct.into(),
                line,
            });
        }
    }

    fn line(&mut self, number: u32, raw: &str) {
        let Some(command) = job_line(raw) else {
            return;
        };
        self.commands = self.commands.saturating_add(1);
        if let Some(system) = command.strip_prefix('$') {
            // Homing and jogging move the head; other `$` commands don't.
            let system = system.to_ascii_uppercase();
            if system == "H" || system.starts_with("HX") || system.starts_with("HY") {
                self.unknown(number, "$H");
            } else if system.starts_with("J=") {
                self.unknown(number, "$J");
            }
            return;
        }
        // GRBL rejects a line it can't read, so it doesn't move.
        let Some(words) = words(&command) else {
            return;
        };
        let mut block = Block::default();
        for (letter, value) in words {
            match letter {
                'G' => match code(value) {
                    Some(0) => block.motion = Some(Motion::Rapid),
                    Some(10) => block.motion = Some(Motion::Linear),
                    Some(20) => block.motion = Some(Motion::Clockwise),
                    Some(30) => block.motion = Some(Motion::CounterClockwise),
                    Some(800) => block.motion = Some(Motion::Cancelled),
                    Some(c @ (170 | 180 | 190)) => self.plane = c / 10,
                    Some(200) => self.inches = true,
                    Some(210) => self.inches = false,
                    Some(900) => self.relative = false,
                    Some(910) => self.relative = true,
                    Some(c @ (540 | 550 | 560 | 570 | 580 | 590)) => {
                        let system = c / 10;
                        if system != self.coordinate_system {
                            self.coordinate_system = system;
                            self.position = [None; 2];
                            self.unknown(number, code_name('G', c));
                        }
                    }
                    Some(530) => block.machine_coordinates = true,
                    Some(c @ (100 | 280 | 300 | 920)) => block.axis_command = Some(c),
                    Some(c @ (921 | 382 | 383 | 384 | 385)) => {
                        block.unknown = Some(code_name('G', c));
                    }
                    _ => {}
                },
                'M' => match code(value) {
                    Some(30) => self.spindle = Spindle::Constant,
                    Some(40) => self.spindle = Spindle::Dynamic,
                    Some(50) => self.spindle = Spindle::Off,
                    Some(20 | 300) => block.program_end = true,
                    _ => {}
                },
                'S' => self.power = value,
                'X' => block.axes[0] = Some(value),
                'Y' => block.axes[1] = Some(value),
                'I' => block.offsets[0] = Some(value),
                'J' => block.offsets[1] = Some(value),
                'R' => block.radius = Some(value),
                'L' => block.l = Some(value),
                _ => {}
            }
        }
        if let Some(motion) = block.motion {
            self.motion = motion;
        }
        // GRBL changes units before it reads the line's coordinates.
        let scale = if self.inches { MM_PER_INCH } else { 1.0 };
        let mm = |value: Option<f64>| value.map(|v| v * scale);
        let axes = block.axes.map(mm);
        let offsets = block.offsets.map(mm);
        let radius = mm(block.radius);

        if let Some(construct) = block.unknown {
            self.unknown(number, construct);
            self.position = [None; 2];
        }
        match block.axis_command {
            Some(100) => {
                let construct = match block.l.and_then(code) {
                    Some(l) => format!("G10 L{}", l / 10),
                    None => "G10".to_string(),
                };
                self.unknown(number, construct);
            }
            Some(920) => {
                // The head stays put and its coordinates change, so moves
                // after it still measure the right lengths.
                self.unknown(number, "G92");
                for (position, axis) in self.position.iter_mut().zip(axes) {
                    if axis.is_some() {
                        *position = axis;
                    }
                }
            }
            Some(c) => {
                // G28, G30: to a stored position.
                self.unknown(number, code_name('G', c));
                self.position = [None; 2];
            }
            None if axes.iter().any(Option::is_some) => {
                if block.machine_coordinates {
                    self.unknown(number, "G53");
                    for (position, axis) in self.position.iter_mut().zip(axes) {
                        if axis.is_some() {
                            *position = None;
                        }
                    }
                } else {
                    self.motion(number, axes, offsets, radius);
                }
            }
            None => {}
        }
        if block.program_end {
            // `M2`/`M30` reset these modes, as in GRBL's `gc_execute_line`.
            self.motion = Motion::Linear;
            self.plane = 17;
            self.relative = false;
            self.spindle = Spindle::Off;
            if self.coordinate_system != 54 {
                self.coordinate_system = 54;
                self.position = [None; 2];
            }
        }
    }

    /// The line's move to `axes` (mm, as written: absolute or relative).
    fn motion(
        &mut self,
        number: u32,
        axes: [Option<f64>; 2],
        offsets: [Option<f64>; 2],
        radius: Option<f64>,
    ) {
        let from = self.position;
        let mut target = from;
        for (axis, value) in axes.into_iter().enumerate() {
            let Some(value) = value else { continue };
            target[axis] = if self.relative {
                match from[axis] {
                    Some(at) => Some(at + value),
                    None => {
                        self.unknown(number, "G91 from wherever the head starts");
                        None
                    }
                }
            } else {
                Some(value)
            };
        }
        let lit = self.spindle != Spindle::Off && self.power > 0.0;
        match self.motion {
            Motion::Cancelled => return,
            Motion::Rapid | Motion::Linear => {
                let moves = (0..2).any(|axis| {
                    axes[axis].is_some() && (from[axis].is_none() || from[axis] != target[axis])
                });
                // Axes this move doesn't name stay put, known or not.
                let length = (0..2)
                    .filter(|&axis| axes[axis].is_some())
                    .map(|axis| Some(target[axis]? - from[axis]?))
                    .try_fold(0.0, |sum, d: Option<f64>| d.map(|d| sum + d * d))
                    .map(f64::sqrt);
                self.add_length(length.unwrap_or(0.0), self.motion == Motion::Linear && lit);
                if self.motion == Motion::Linear && moves {
                    self.cutting(number, lit);
                }
                self.include(target);
            }
            Motion::Clockwise | Motion::CounterClockwise => {
                let clockwise = self.motion == Motion::Clockwise;
                if self.plane != 17 {
                    self.unknown(number, format!("arcs in the G{} plane", self.plane));
                } else if let ([Some(x0), Some(y0)], [Some(x1), Some(y1)]) = (from, target) {
                    let start = Point { x: x0, y: y0 };
                    let end = Point { x: x1, y: y1 };
                    let center = match radius {
                        Some(r) => radius_center(start, end, r, clockwise),
                        None => Some(Point {
                            x: x0 + offsets[0].unwrap_or(0.0),
                            y: y0 + offsets[1].unwrap_or(0.0),
                        }),
                    };
                    // GRBL rejects an arc it can't fit, so the head stays.
                    let Some(center) = center else { return };
                    let arc = Arc::new(start, end, center, clockwise);
                    self.add_length(arc.length(), lit);
                    self.cutting(number, lit);
                    for point in arc.extremes() {
                        self.include([Some(point.x), Some(point.y)]);
                    }
                } else {
                    let name = if clockwise { "G2" } else { "G3" };
                    self.unknown(number, format!("{name} from wherever the head starts"));
                }
                self.include(target);
            }
        }
        self.position = target;
    }

    fn add_length(&mut self, length: f64, lit: bool) {
        if lit {
            self.cut_length += length;
        } else {
            self.travel_length += length;
        }
    }

    /// A cutting move (`G1`-`G3`) on `line`: constant power if `M3` is on
    /// and lit.
    fn cutting(&mut self, line: u32, lit: bool) {
        if lit && self.spindle == Spindle::Constant && self.constant_power_line.is_none() {
            self.constant_power_line = Some(line);
        }
    }

    /// Widens the bounds to the axes of `point` that are known.
    fn include(&mut self, point: [Option<f64>; 2]) {
        for (range, value) in self.range.iter_mut().zip(point) {
            if let Some(v) = value {
                *range = Some(match *range {
                    None => (v, v),
                    Some((lo, hi)) => (lo.min(v), hi.max(v)),
                });
            }
        }
    }

    fn finish(self, profile: &MachineProfile) -> JobSummary {
        let [x, y] = self.range.map(|range| range.unwrap_or((0.0, 0.0)));
        let bounds = JobBounds {
            min: Point { x: x.0, y: y.0 },
            max: Point { x: x.1, y: y.1 },
        };
        let warning = match &self.unknown {
            Some(unknown) => Some(format!(
                "Couldn't check bounds: the file uses {} (line {}). Make sure the job fits on \
                 the bed before running it.",
                unknown.construct, unknown.line
            )),
            None => bed_warning(&bounds, profile),
        };
        JobSummary {
            bounds,
            inside_bed: warning.is_none(),
            warning,
            lines: self.commands,
            cut_length: self.cut_length,
            travel_length: self.travel_length,
            constant_power_layers: Vec::new(),
            constant_power_line: self.constant_power_line,
            unknown_bounds: self.unknown,
        }
    }
}

/// The center of an `R` arc from `start` to `end`, as GRBL picks it: a
/// negative radius takes the longer way round. `None` if the ends are too
/// far apart for the radius.
fn radius_center(start: Point, end: Point, radius: f64, clockwise: bool) -> Option<Point> {
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let chord = dx.hypot(dy);
    let h_squared = 4.0 * radius * radius - dx * dx - dy * dy;
    if h_squared < 0.0 || chord == 0.0 {
        return None;
    }
    let mut h = -h_squared.sqrt() / chord;
    if !clockwise {
        h = -h;
    }
    if radius < 0.0 {
        h = -h;
    }
    Some(Point {
        x: start.x + 0.5 * (dx - dy * h),
        y: start.y + 0.5 * (dy + dx * h),
    })
}

/// A circular arc in the XY plane.
struct Arc {
    center: Point,
    radius: f64,
    start_angle: f64,
    /// Signed: negative clockwise.
    sweep: f64,
    end: Point,
}

impl Arc {
    fn new(start: Point, end: Point, center: Point, clockwise: bool) -> Arc {
        let (r0x, r0y) = (start.x - center.x, start.y - center.y);
        let (r1x, r1y) = (end.x - center.x, end.y - center.y);
        let mut sweep = (r0x * r1y - r0y * r1x).atan2(r0x * r1x + r0y * r1y);
        // As GRBL's `mc_arc`: an end on the start is a full circle.
        if clockwise {
            if sweep >= -ARC_EPSILON {
                sweep -= TAU;
            }
        } else if sweep <= ARC_EPSILON {
            sweep += TAU;
        }
        Arc {
            center,
            radius: r0x.hypot(r0y),
            start_angle: r0y.atan2(r0x),
            sweep,
            end,
        }
    }

    fn length(&self) -> f64 {
        self.radius * self.sweep.abs()
    }

    /// The end, and the arc's leftmost, rightmost, lowest and highest points
    /// where it passes them.
    fn extremes(&self) -> Vec<Point> {
        let mut points = vec![self.end];
        for quarter in 0..4 {
            let angle = f64::from(quarter) * FRAC_PI_2;
            let along = if self.sweep > 0.0 {
                angle - self.start_angle
            } else {
                self.start_angle - angle
            };
            if along.rem_euclid(TAU) <= self.sweep.abs() {
                points.push(Point {
                    x: self.center.x + self.radius * angle.cos(),
                    y: self.center.y + self.radius * angle.sin(),
                });
            }
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::{BedSize, CheckLevel, check_bounds, check_constant_power};

    fn profile() -> MachineProfile {
        MachineProfile {
            bed: BedSize {
                width: 100.0,
                height: 100.0,
            },
            ..MachineProfile::default()
        }
    }

    fn summary(gcode: &str) -> JobSummary {
        scan(gcode, &profile())
    }

    fn bounds(summary: &JobSummary) -> [f64; 4] {
        let b = summary.bounds;
        [b.min.x, b.min.y, b.max.x, b.max.y].map(|v| (v * 1e6).round() / 1e6)
    }

    fn unknown(gcode: &str) -> Option<(String, u32)> {
        summary(gcode)
            .unknown_bounds
            .map(|unknown| (unknown.construct, unknown.line))
    }

    #[test]
    fn a_file_inside_the_bed_passes_with_its_lengths_and_command_lines() {
        let s = summary("; header\nG21 G90\n\nM4 S0\nG0 X10 Y10\nG1 X40 S500 F1000\nG1 Y50\nM5\n");
        assert_eq!(bounds(&s), [10.0, 10.0, 40.0, 50.0]);
        assert!(s.inside_bed);
        assert_eq!(s.warning, None);
        assert_eq!(s.lines, 6);
        assert!((s.cut_length - 70.0).abs() < 1e-9);
        assert!((s.travel_length).abs() < 1e-9, "from an unknown start");
        assert_eq!(check_bounds(&s).level, CheckLevel::Pass);
    }

    #[test]
    fn going_past_the_bed_warns_by_how_much() {
        let s = summary("G0 X10 Y10\nG1 X105 S100\n");
        assert!(!s.inside_bed);
        assert_eq!(
            s.warning.as_deref(),
            Some("The job goes up to 5 mm outside the 100 × 100 mm bed.")
        );
        let below = summary("G0 X10 Y10\nG0 Y-2\n");
        assert!(below.warning.unwrap().contains("up to 2 mm outside"));
    }

    #[test]
    fn travel_counts_rapids_and_unlit_cuts() {
        let s = summary("G0 X0 Y0\nG0 X30 Y40\nM4 S0\nG1 X60 Y40\nG1 X60 Y50 S10\nG0 X0 Y0");
        assert!((s.travel_length - (50.0 + 30.0 + 60f64.hypot(50.0))).abs() < 1e-9);
        assert!((s.cut_length - 10.0).abs() < 1e-9);
    }

    #[test]
    fn relative_moves_are_bounded_and_g90_returns_to_absolute() {
        let s = summary("G90 G0 X50 Y50\nG91 G1 X10 Y-20\nG1 X10\nG90 G0 X5 Y60\n");
        assert_eq!(bounds(&s), [5.0, 30.0, 70.0, 60.0]);
        assert_eq!(s.unknown_bounds, None);
    }

    #[test]
    fn relative_moves_from_wherever_the_head_starts_are_unknown() {
        assert_eq!(
            unknown("G21\nG91\nG1 X10 Y10\n"),
            Some(("G91 from wherever the head starts".into(), 3))
        );
        // Once an axis is placed, relative moves along it are fine.
        assert_eq!(unknown("G0 X10\nG91 G0 X5\n"), None);
    }

    #[test]
    fn an_axis_the_file_never_moves_stays_out_of_the_bounds() {
        let s = summary("G0 X20\nG1 X80\n");
        assert_eq!(bounds(&s), [20.0, 0.0, 80.0, 0.0]);
        assert!(s.inside_bed);
    }

    #[test]
    fn inches_scale_coordinates_and_arc_offsets() {
        let s = summary("G20\nG0 X1 Y1\nG1 X2 Y1\nG21\nG0 X10 Y10");
        assert_eq!(bounds(&s), [10.0, 10.0, 50.8, 25.4]);
        let arc = summary("G20 G0 X1 Y1\nG2 X1 Y1 I1 J0\n");
        assert_eq!(bounds(&arc), [25.4, 0.0, 76.2, 50.8]);
    }

    #[test]
    fn arcs_include_the_axis_extremes_they_pass() {
        // Counter-clockwise from the bottom of a circle round (50, 50) to its
        // top passes its rightmost point only.
        let ccw = summary("G0 X50 Y40\nG3 X50 Y60 I0 J10\n");
        assert_eq!(bounds(&ccw), [50.0, 40.0, 60.0, 60.0]);
        let cw = summary("G0 X50 Y40\nG2 X50 Y60 I0 J10\n");
        assert_eq!(bounds(&cw), [40.0, 40.0, 50.0, 60.0]);
        // A quarter arc passes no extreme beyond its ends.
        let quarter = summary("G0 X60 Y50\nG3 X50 Y60 I-10 J0\n");
        assert_eq!(bounds(&quarter), [50.0, 50.0, 60.0, 60.0]);
        // An arc ending where it starts is a full circle.
        let circle = summary("G0 X60 Y50\nM4 S100\nG2 X60 Y50 I-10 J0\n");
        assert_eq!(bounds(&circle), [40.0, 40.0, 60.0, 60.0]);
        assert!((circle.cut_length - TAU * 10.0).abs() < 1e-9);
    }

    #[test]
    fn an_arc_bulging_past_the_bed_edge_is_caught() {
        let s = summary("G0 X95 Y40\nG3 X95 Y60 I0 J10\n");
        assert_eq!(
            s.warning.as_deref(),
            Some("The job goes up to 5 mm outside the 100 × 100 mm bed.")
        );
    }

    #[test]
    fn radius_arcs_take_the_short_way_unless_the_radius_is_negative() {
        let short = summary("G0 X40 Y50\nG2 X60 Y50 R10\n");
        assert_eq!(bounds(&short), [40.0, 50.0, 60.0, 60.0]);
        let long = summary(
            "G0 X40 Y50
G2 X60 Y50 R-14.142136
",
        );
        // Center (50, 60), radius 14.14: clockwise the long way, over the top.
        let r = 14.142136;
        let [min_x, min_y, max_x, max_y] = bounds(&long);
        assert!((min_x - (50.0 - r)).abs() < 1e-5, "{min_x}");
        assert!((min_y - 50.0).abs() < 1e-5, "{min_y}");
        assert!((max_x - (50.0 + r)).abs() < 1e-5, "{max_x}");
        assert!((max_y - (60.0 + r)).abs() < 1e-5, "{max_y}");
        // Too short a radius for the ends: GRBL rejects it, the head stays.
        let rejected = summary("G0 X40 Y50\nG2 X60 Y50 R5\n");
        assert_eq!(bounds(&rejected), [40.0, 50.0, 40.0, 50.0]);
    }

    #[test]
    fn constant_power_needs_m3_lit_on_a_cutting_move() {
        // A header `M3 S0` and an M4 job: no constant power.
        let m4 = summary("M3 S0\nG0 X10 Y10\nM4\nG1 X50 S700\nM5\n");
        assert_eq!(m4.constant_power_line, None);
        assert_eq!(check_constant_power(&m4).level, CheckLevel::Pass);
        // Lit rapids don't burn in laser mode; S0 cuts don't either.
        let dark = summary("M3 S500\nG0 X10 Y10\nG1 X20 S0\nM5\n");
        assert_eq!(dark.constant_power_line, None);
        // The first lit M3 cut names its line.
        let m3 = summary("G21\nM3 S0\nG0 X10 Y10\nG1 X20 S300\nG1 X30\n");
        assert_eq!(m3.constant_power_line, Some(4));
        let check = check_constant_power(&m3);
        assert_eq!(check.level, CheckLevel::Warn);
        assert_eq!(check.title, "Constant power (M3) from line 4");
        let detail = check.detail.unwrap();
        assert_eq!(
            detail,
            "If the USB connection drops during the M3 cutting, the laser stays on where the "
                .to_owned()
                + "head stops, until the machine is reset or switched off. Stay at the machine "
                + "with its power switch in reach."
        );
        // M5 then M4 later doesn't hide an earlier M3 cut; arcs count too.
        let arc = summary("G0 X10 Y10\nM3 S200\nG2 X10 Y10 I5 J0\n");
        assert_eq!(arc.constant_power_line, Some(3));
    }

    #[test]
    fn coordinate_changes_leave_the_bounds_unknown() {
        assert_eq!(unknown("G0 X1 Y1\nG92 X0 Y0\n"), Some(("G92".into(), 2)));
        assert_eq!(unknown("G92.1\n"), Some(("G92.1".into(), 1)));
        assert_eq!(unknown("G53 G0 X0 Y0\n"), Some(("G53".into(), 1)));
        assert_eq!(unknown("G28\n"), Some(("G28".into(), 1)));
        assert_eq!(unknown("G30 X5\n"), Some(("G30".into(), 1)));
        assert_eq!(unknown("G10 L20 P1 X0\n"), Some(("G10 L20".into(), 1)));
        assert_eq!(unknown("G10 L2 P1 X5\n"), Some(("G10 L2".into(), 1)));
        assert_eq!(unknown("G55\nG0 X1 Y1\n"), Some(("G55".into(), 1)));
        assert_eq!(unknown("$H\nG0 X1 Y1\n"), Some(("$H".into(), 1)));
        assert_eq!(unknown("$J=G91 X10 F100\n"), Some(("$J".into(), 1)));
        assert_eq!(
            unknown("G18\nG0 X1 Y1\nG2 X3 Y1 I1\n"),
            Some(("arcs in the G18 plane".into(), 3))
        );
        // Re-selecting the default system, storing positions and settings
        // are fine.
        assert_eq!(
            unknown("G54\nG28.1\n$32=1\nG0 X1 Y1\nG17 G2 X3 Y1 I1\n"),
            None
        );
    }

    #[test]
    fn unknown_bounds_warn_in_the_bounds_check_and_keep_the_first_reason() {
        let s = summary("G21\nG0 X10 Y10\nG92 X0 Y0\nG28\n");
        let reason = s.unknown_bounds.clone().unwrap();
        assert_eq!((reason.construct.as_str(), reason.line), ("G92", 3));
        assert!(!s.inside_bed);
        let check = check_bounds(&s);
        assert_eq!(check.level, CheckLevel::Warn);
        assert_eq!(check.title, "Couldn't check bounds");
        assert_eq!(
            check.detail.as_deref(),
            Some(
                "Couldn't check bounds: the file uses G92 (line 3). Make sure the job fits on \
                 the bed before running it."
            )
        );
    }

    #[test]
    fn g92_keeps_lengths_right_after_it() {
        let s = summary("G0 X10 Y10\nG92 X0 Y0\nM4 S100\nG1 X10\n");
        assert!((s.cut_length - 10.0).abs() < 1e-9);
    }

    #[test]
    fn program_end_turns_the_laser_off_and_returns_to_absolute() {
        let s = summary("G0 X10 Y10\nM3 S100\nG91\nM2\nG1 X20 Y20\n");
        assert_eq!(s.constant_power_line, None);
        assert_eq!(bounds(&s), [10.0, 10.0, 20.0, 20.0]);
    }

    #[test]
    fn words_read_like_grbl_and_unreadable_lines_are_skipped() {
        let s = summary("g0x10y10\nG1 X 20 Y-5.5\nG1 X?\n(comment) G0 X30 ; trailing\n%\n");
        assert_eq!(bounds(&s), [10.0, -5.5, 30.0, 10.0]);
        assert_eq!(s.lines, 4);
    }

    #[test]
    fn scan_reader_matches_scan() {
        let gcode = "G0 X10 Y10\nM3 S100\nG1 X20\n";
        let read = scan_reader(io::Cursor::new(gcode), &profile()).unwrap();
        assert_eq!(read, summary(gcode));
    }

    #[test]
    fn an_empty_file_stays_on_the_bed() {
        let s = summary("; nothing\n");
        assert!(s.inside_bed);
        assert_eq!(s.lines, 0);
    }
}
