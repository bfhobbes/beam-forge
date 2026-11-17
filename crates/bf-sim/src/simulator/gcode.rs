//! The G-code interpreter: GRBL 1.1's `gc_execute_line`.
//!
//! Accepts what stock GRBL 1.1 does: G0-G3 (I/J/K and R forms in the G17,
//! G18 and G19 planes), G4, G10 L2/L20, G17-G19, G20/G21, G28/G30 and
//! G28.1/G30.1, G40, G43.1/G49, G53, G54-G59, G61, G80, G90/G91, G91.1, G92
//! and G92.1, G93/G94, M0/M1/M2/M30, M3/M4/M5, M7/M8/M9, and the F, N, S and
//! T words. M7 is a compile-time option in GRBL; this models a build with it
//! on. M1 is accepted and ignored, as in GRBL, and T only records the number.
//!
//! G38.x probing returns `error:20`: it needs a probe pin, which the
//! simulator does not model.
//!
//! Like GRBL, a line is fully validated before any of it takes effect, and
//! errors are checked in GRBL's order so the same bad line gets the same code.

use super::{
    ERR_BAD_NUMBER, ERR_EXPECTED_LETTER, ERR_LOCKED, ERR_NO_AXIS_WORDS, ERR_UNDEFINED_FEED,
    MachineState, Op, Reply, STORED_G28, STORED_G30, Simulator, parse_number,
};
use crate::motion::{LineData, Plane, Spindle, arc_points};

const ERR_NEGATIVE_VALUE: u8 = 4;
const ERR_UNSUPPORTED_COMMAND: u8 = 20;
const ERR_MODAL_GROUP_VIOLATION: u8 = 21;
const ERR_COMMAND_VALUE_NOT_INTEGER: u8 = 23;
const ERR_AXIS_COMMAND_CONFLICT: u8 = 24;
const ERR_WORD_REPEATED: u8 = 25;
const ERR_INVALID_LINE_NUMBER: u8 = 27;
const ERR_VALUE_WORD_MISSING: u8 = 28;
const ERR_UNSUPPORTED_COORD_SYS: u8 = 29;
const ERR_G53_INVALID_MOTION_MODE: u8 = 30;
const ERR_AXIS_WORDS_EXIST: u8 = 31;
const ERR_NO_AXIS_WORDS_IN_PLANE: u8 = 32;
const ERR_INVALID_TARGET: u8 = 33;
const ERR_ARC_RADIUS: u8 = 34;
const ERR_NO_OFFSETS_IN_PLANE: u8 = 35;
const ERR_UNUSED_WORDS: u8 = 36;
const ERR_G43_DYNAMIC_AXIS: u8 = 37;
const ERR_MAX_VALUE_EXCEEDED: u8 = 38;

const MAX_LINE_NUMBER: f64 = 10_000_000.0;
const MAX_TOOL_NUMBER: f64 = 255.0;
const MM_PER_INCH: f64 = 25.4;
/// G54-G59.
const N_COORDINATE_SYSTEM: f64 = 6.0;
/// `G80`'s motion mode.
const MOTION_NONE: u8 = 80;
/// GRBL's `TOOL_LENGTH_OFFSET_AXIS`.
const TOOL_LENGTH_AXIS: usize = 2;

// Value words, tracked as bits like GRBL's `value_words`.
const WORD_F: u16 = 1 << 0;
const WORD_I: u16 = 1 << 1;
const WORD_J: u16 = 1 << 2;
const WORD_K: u16 = 1 << 3;
const WORD_L: u16 = 1 << 4;
const WORD_N: u16 = 1 << 5;
const WORD_P: u16 = 1 << 6;
const WORD_R: u16 = 1 << 7;
const WORD_S: u16 = 1 << 8;
const WORD_T: u16 = 1 << 9;
const WORD_X: u16 = 1 << 10;
const WORD_Y: u16 = 1 << 11;
const WORD_Z: u16 = 1 << 12;

// Modal groups, for `error:21` when a line has two commands from one group.
const GROUP_NON_MODAL: u16 = 1 << 0;
const GROUP_MOTION: u16 = 1 << 1;
const GROUP_PLANE: u16 = 1 << 2;
const GROUP_DISTANCE: u16 = 1 << 3;
const GROUP_ARC_DISTANCE: u16 = 1 << 4;
const GROUP_FEED_RATE: u16 = 1 << 5;
const GROUP_UNITS: u16 = 1 << 6;
const GROUP_CUTTER_COMP: u16 = 1 << 7;
const GROUP_TOOL_LENGTH: u16 = 1 << 8;
const GROUP_COORD_SYSTEM: u16 = 1 << 9;
const GROUP_PATH_CONTROL: u16 = 1 << 10;
const GROUP_PROGRAM_FLOW: u16 = 1 << 11;
const GROUP_SPINDLE: u16 = 1 << 12;
const GROUP_COOLANT: u16 = 1 << 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisCommand {
    None,
    NonModal,
    Motion,
    /// G43.1 and G49 count as axis commands, so no motion may share the line.
    ToolLengthOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NonModal {
    None,
    Dwell,
    SetCoordinateData,
    /// G28/G30, by stored-data index.
    GoHome(usize),
    /// G28.1/G30.1, by stored-data index.
    SetHome(usize),
    AbsoluteOverride,
    SetCoordinateOffset,
    ResetCoordinateOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramFlow {
    Running,
    Paused,
    Completed,
}

/// Word values from one line.
#[derive(Debug, Default)]
struct Values {
    f: f64,
    ijk: [f64; 3],
    l: f64,
    n: f64,
    p: f64,
    r: f64,
    s: f64,
    t: f64,
    xyz: [f64; 3],
}

impl Simulator {
    pub(super) fn gcode_line(&mut self, line: &str) -> Result<Reply, u8> {
        if matches!(self.state, MachineState::Alarm | MachineState::Jog) {
            return Err(ERR_LOCKED);
        }
        self.execute_gcode(line)?;
        self.ops.push_back(Op::Ok);
        self.run_ops();
        Ok(Reply::Sent)
    }

    /// Parses, checks and executes one line. On error nothing has changed.
    fn execute_gcode(&mut self, line: &str) -> Result<(), u8> {
        // ---- step 1: start from the current modes ----
        let mut motion = self.modal.motion;
        let mut coord_select = self.modal.coord_select;
        let mut plane = self.modal.plane;
        let mut inches = self.modal.inches;
        let mut incremental = self.modal.incremental;
        let mut inverse_time = self.modal.inverse_time;
        let mut spindle = self.modal.spindle;
        let (mut flood, mut mist) = (self.modal.flood, self.modal.mist);
        let mut program_flow = ProgramFlow::Running;
        let mut cancel_tool_length = false;
        let mut non_modal = NonModal::None;
        let mut axis_command = AxisCommand::None;
        let mut commands = 0u16;
        let mut words = 0u16;
        let mut axis_words = 0u8;
        let mut ijk_words = 0u8;
        let mut v = Values::default();

        // ---- step 2: read words, checking repeats and modal groups ----
        let mut rest = line;
        while !rest.is_empty() {
            let (letter, value, tail) = next_word(rest)?;
            rest = tail;
            let int_value = value.trunc();
            let mut mantissa = (100.0 * (value - int_value)).round() as i64;
            let int_value = int_value as i64;
            match letter {
                'G' => {
                    let group = match int_value {
                        10 | 28 | 30 | 92 => {
                            // G28.1, G30.1 and G92.1 use no axis words.
                            if mantissa == 0 {
                                if axis_command != AxisCommand::None {
                                    return Err(ERR_AXIS_COMMAND_CONFLICT);
                                }
                                axis_command = AxisCommand::NonModal;
                            }
                            non_modal = match (int_value, mantissa) {
                                (10, _) => NonModal::SetCoordinateData,
                                (28, 0) => NonModal::GoHome(STORED_G28),
                                (30, 0) => NonModal::GoHome(STORED_G30),
                                (28, 10) => NonModal::SetHome(STORED_G28),
                                (30, 10) => NonModal::SetHome(STORED_G30),
                                (92, 0) => NonModal::SetCoordinateOffset,
                                (92, 10) => NonModal::ResetCoordinateOffset,
                                _ => return Err(ERR_UNSUPPORTED_COMMAND),
                            };
                            if int_value != 10 {
                                mantissa = 0;
                            }
                            GROUP_NON_MODAL
                        }
                        4 => {
                            non_modal = NonModal::Dwell;
                            GROUP_NON_MODAL
                        }
                        53 => {
                            non_modal = NonModal::AbsoluteOverride;
                            GROUP_NON_MODAL
                        }
                        0..=3 | 38 => {
                            if axis_command != AxisCommand::None {
                                return Err(ERR_AXIS_COMMAND_CONFLICT);
                            }
                            axis_command = AxisCommand::Motion;
                            if int_value == 38 {
                                // G38.x probing: no probe pin is modelled.
                                return Err(ERR_UNSUPPORTED_COMMAND);
                            }
                            motion = int_value as u8;
                            GROUP_MOTION
                        }
                        80 => {
                            motion = MOTION_NONE;
                            GROUP_MOTION
                        }
                        17..=19 => {
                            plane = (int_value - 17) as u8;
                            GROUP_PLANE
                        }
                        90 | 91 if mantissa == 0 => {
                            incremental = int_value == 91;
                            GROUP_DISTANCE
                        }
                        90 | 91 => {
                            // G91.1 (incremental arc centres) is GRBL's only
                            // arc mode, so it does nothing; G90.1 is refused.
                            if mantissa != 10 || int_value == 90 {
                                return Err(ERR_UNSUPPORTED_COMMAND);
                            }
                            mantissa = 0;
                            GROUP_ARC_DISTANCE
                        }
                        93 | 94 => {
                            inverse_time = int_value == 93;
                            GROUP_FEED_RATE
                        }
                        20 | 21 => {
                            inches = int_value == 20;
                            GROUP_UNITS
                        }
                        // Cutter compensation is never on; G40 only appears in
                        // program headers.
                        40 => GROUP_CUTTER_COMP,
                        43 | 49 => {
                            if axis_command != AxisCommand::None {
                                return Err(ERR_AXIS_COMMAND_CONFLICT);
                            }
                            axis_command = AxisCommand::ToolLengthOffset;
                            if int_value == 49 {
                                cancel_tool_length = true;
                            } else if mantissa != 10 {
                                return Err(ERR_UNSUPPORTED_COMMAND);
                            }
                            mantissa = 0;
                            GROUP_TOOL_LENGTH
                        }
                        54..=59 => {
                            coord_select = (int_value - 54) as usize;
                            GROUP_COORD_SYSTEM
                        }
                        61 => {
                            if mantissa != 0 {
                                return Err(ERR_UNSUPPORTED_COMMAND);
                            }
                            GROUP_PATH_CONTROL
                        }
                        _ => return Err(ERR_UNSUPPORTED_COMMAND),
                    };
                    if mantissa != 0 {
                        return Err(ERR_COMMAND_VALUE_NOT_INTEGER);
                    }
                    if commands & group != 0 {
                        return Err(ERR_MODAL_GROUP_VIOLATION);
                    }
                    commands |= group;
                }
                'M' => {
                    if mantissa != 0 {
                        return Err(ERR_COMMAND_VALUE_NOT_INTEGER);
                    }
                    let group = match int_value {
                        0 | 1 | 2 | 30 => {
                            // M1 (optional stop) is ignored.
                            match int_value {
                                0 => program_flow = ProgramFlow::Paused,
                                1 => {}
                                _ => program_flow = ProgramFlow::Completed,
                            }
                            GROUP_PROGRAM_FLOW
                        }
                        3..=5 => {
                            spindle = match int_value {
                                3 => Spindle::Cw,
                                4 => Spindle::Ccw,
                                _ => Spindle::Off,
                            };
                            GROUP_SPINDLE
                        }
                        7 => {
                            mist = true;
                            GROUP_COOLANT
                        }
                        8 => {
                            flood = true;
                            GROUP_COOLANT
                        }
                        9 => {
                            flood = false;
                            mist = false;
                            GROUP_COOLANT
                        }
                        _ => return Err(ERR_UNSUPPORTED_COMMAND),
                    };
                    if commands & group != 0 {
                        return Err(ERR_MODAL_GROUP_VIOLATION);
                    }
                    commands |= group;
                }
                _ => {
                    let word = match letter {
                        'F' => {
                            v.f = value;
                            WORD_F
                        }
                        'I' | 'J' | 'K' => {
                            let axis = (letter as u8 - b'I') as usize;
                            v.ijk[axis] = value;
                            ijk_words |= 1 << axis;
                            WORD_I << axis
                        }
                        'L' => {
                            v.l = value.trunc();
                            WORD_L
                        }
                        'N' => {
                            v.n = value.trunc();
                            WORD_N
                        }
                        'P' => {
                            v.p = value;
                            WORD_P
                        }
                        'R' => {
                            v.r = value;
                            WORD_R
                        }
                        'S' => {
                            v.s = value;
                            WORD_S
                        }
                        'T' => {
                            if value > MAX_TOOL_NUMBER {
                                return Err(ERR_MAX_VALUE_EXCEEDED);
                            }
                            v.t = value.trunc();
                            WORD_T
                        }
                        'X' | 'Y' | 'Z' => {
                            let axis = (letter as u8 - b'X') as usize;
                            v.xyz[axis] = value;
                            axis_words |= 1 << axis;
                            WORD_X << axis
                        }
                        _ => return Err(ERR_UNSUPPORTED_COMMAND),
                    };
                    if words & word != 0 {
                        return Err(ERR_WORD_REPEATED);
                    }
                    if word & (WORD_F | WORD_N | WORD_P | WORD_T | WORD_S) != 0 && value < 0.0 {
                        return Err(ERR_NEGATIVE_VALUE);
                    }
                    words |= word;
                }
            }
        }

        // ---- step 3: check the block and convert its values ----
        if axis_words != 0 && axis_command == AxisCommand::None {
            axis_command = AxisCommand::Motion;
        }
        if words & WORD_N != 0 && v.n > MAX_LINE_NUMBER {
            return Err(ERR_INVALID_LINE_NUMBER);
        }
        if inverse_time {
            // Every G1-G3 move needs its own F, which is never converted
            // from inches. Without one the feed rate becomes undefined.
            if axis_command == AxisCommand::Motion
                && !matches!(motion, 0 | MOTION_NONE)
                && words & WORD_F == 0
            {
                return Err(ERR_UNDEFINED_FEED);
            }
        } else if !self.modal.inverse_time {
            if words & WORD_F != 0 {
                if inches {
                    v.f *= MM_PER_INCH;
                }
            } else {
                v.f = self.modal.feed;
            }
        }
        // Else switching from G93 to G94: the feed is this line's F as given,
        // unconverted, or undefined.
        if words & WORD_S == 0 {
            v.s = self.modal.speed;
        }
        if non_modal == NonModal::Dwell {
            if words & WORD_P == 0 {
                return Err(ERR_VALUE_WORD_MISSING);
            }
            words &= !WORD_P;
        }
        let arc_plane = Plane::select(plane);
        if inches {
            for (axis, value) in v.xyz.iter_mut().enumerate() {
                if axis_words & (1 << axis) != 0 {
                    *value *= MM_PER_INCH;
                }
            }
        }
        if axis_command == AxisCommand::ToolLengthOffset
            && !cancel_tool_length
            && axis_words != 1 << TOOL_LENGTH_AXIS
        {
            return Err(ERR_G43_DYNAMIC_AXIS);
        }

        let position = self.gc_position;
        let coord_system = self.coordinates.systems[coord_select];
        let g92 = self.coordinates.g92;
        let tool_length = self.modal.tool_length_offset;
        let given = |axis: usize| axis_words & (1 << axis) != 0;
        let mut target = position;
        let mut coordinate_data = None;
        let mut home = None;
        let mut new_g92 = None;
        match non_modal {
            NonModal::SetCoordinateData => {
                if axis_words == 0 {
                    return Err(ERR_NO_AXIS_WORDS);
                }
                // GRBL only insists on one of P and L here; the other defaults to 0.
                if words & (WORD_P | WORD_L) == 0 {
                    return Err(ERR_VALUE_WORD_MISSING);
                }
                let p = v.p.trunc();
                if p > N_COORDINATE_SYSTEM {
                    return Err(ERR_UNSUPPORTED_COORD_SYS);
                }
                if v.l != 20.0 && (v.l != 2.0 || words & WORD_R != 0) {
                    return Err(ERR_UNSUPPORTED_COMMAND);
                }
                words &= !(WORD_L | WORD_P);
                // P0 means the system active after this line.
                let index = if p > 0.0 {
                    p as usize - 1
                } else {
                    coord_select
                };
                let mut data = self.coordinates.systems[index];
                for axis in (0..3).filter(|&axis| given(axis)) {
                    data[axis] = if v.l == 20.0 {
                        // WPos = MPos - WCS - G92 - TLO, solved for WCS.
                        let mut wcs = position[axis] - g92[axis] - v.xyz[axis];
                        if axis == TOOL_LENGTH_AXIS {
                            wcs -= tool_length;
                        }
                        wcs
                    } else {
                        v.xyz[axis]
                    };
                }
                coordinate_data = Some((index, data));
            }
            NonModal::SetCoordinateOffset => {
                if axis_words == 0 {
                    return Err(ERR_NO_AXIS_WORDS);
                }
                new_g92 = Some(std::array::from_fn(|axis| {
                    if given(axis) {
                        let mut offset = position[axis] - coord_system[axis] - v.xyz[axis];
                        if axis == TOOL_LENGTH_AXIS {
                            offset -= tool_length;
                        }
                        offset
                    } else {
                        g92[axis]
                    }
                }));
            }
            _ => {
                if axis_command != AxisCommand::ToolLengthOffset {
                    for axis in (0..3).filter(|&axis| given(axis)) {
                        target[axis] = if non_modal == NonModal::AbsoluteOverride {
                            v.xyz[axis]
                        } else if incremental {
                            position[axis] + v.xyz[axis]
                        } else {
                            let mut machine = v.xyz[axis] + coord_system[axis] + g92[axis];
                            if axis == TOOL_LENGTH_AXIS {
                                machine += tool_length;
                            }
                            machine
                        };
                    }
                }
                match non_modal {
                    NonModal::GoHome(index) => {
                        // Axis words name an intermediate point, and only
                        // those axes then go home.
                        let mut stored = self.coordinates.stored(index);
                        if axis_words != 0 {
                            for axis in (0..3).filter(|&axis| !given(axis)) {
                                stored[axis] = position[axis];
                            }
                        } else {
                            axis_command = AxisCommand::None;
                        }
                        home = Some(stored);
                    }
                    NonModal::AbsoluteOverride if motion > 1 => {
                        return Err(ERR_G53_INVALID_MOTION_MODE);
                    }
                    _ => {}
                }
            }
        }

        let mut arc = None;
        if motion == MOTION_NONE {
            // Even non-modal and tool length commands may not have axis words.
            if axis_words != 0 {
                return Err(ERR_AXIS_WORDS_EXIST);
            }
        } else if axis_command == AxisCommand::Motion {
            if motion == 0 {
                if axis_words == 0 {
                    axis_command = AxisCommand::None;
                }
            } else {
                if v.f == 0.0 {
                    return Err(ERR_UNDEFINED_FEED);
                }
                if motion == 1 {
                    if axis_words == 0 {
                        axis_command = AxisCommand::None;
                    }
                } else {
                    let Plane { axis_0, axis_1, .. } = arc_plane;
                    if axis_words == 0 {
                        return Err(ERR_NO_AXIS_WORDS);
                    }
                    if axis_words & (1 << axis_0 | 1 << axis_1) == 0 {
                        return Err(ERR_NO_AXIS_WORDS_IN_PLANE);
                    }
                    let x = target[axis_0] - position[axis_0];
                    let y = target[axis_1] - position[axis_1];
                    if words & WORD_R != 0 {
                        words &= !WORD_R;
                        let (offset, radius) =
                            radius_arc(position, target, x, y, v.r, inches, motion)?;
                        let mut ijk = v.ijk;
                        ijk[axis_0] = offset[0];
                        ijk[axis_1] = offset[1];
                        arc = Some((ijk, radius));
                    } else {
                        if ijk_words & (1 << axis_0 | 1 << axis_1) == 0 {
                            return Err(ERR_NO_OFFSETS_IN_PLANE);
                        }
                        words &= !(WORD_I | WORD_J | WORD_K);
                        let ijk = if inches {
                            v.ijk.map(|o| o * MM_PER_INCH)
                        } else {
                            v.ijk
                        };
                        let target_radius = (x - ijk[axis_0]).hypot(y - ijk[axis_1]);
                        let radius = ijk[axis_0].hypot(ijk[axis_1]);
                        let delta = (target_radius - radius).abs();
                        if delta > 0.005 && (delta > 0.5 || delta > 0.001 * radius) {
                            return Err(ERR_INVALID_TARGET);
                        }
                        arc = Some((ijk, radius));
                    }
                }
            }
        }

        words &= !(WORD_N | WORD_F | WORD_S | WORD_T);
        if axis_command != AxisCommand::None {
            words &= !(WORD_X | WORD_Y | WORD_Z);
        }
        if words != 0 {
            return Err(ERR_UNUSED_WORDS);
        }

        // ---- step 4: execute, in GRBL's order ----
        // In laser mode only G1/G2/G3 may fire the laser, and an M3 laser must
        // be resynchronised when a line without motion moves between G0 and
        // G1-G3.
        let laser = self.profile.settings.laser_mode();
        let laser_disable = laser && !matches!(motion, 1..=3);
        let laser_is_motion = laser && axis_words != 0 && axis_command == AxisCommand::Motion;
        let force_sync = laser
            && !laser_is_motion
            && self.modal.spindle == Spindle::Cw
            && if matches!(self.modal.motion, 1..=3) {
                laser_disable
            } else {
                !laser_disable
            };
        let check = self.state == MachineState::Check;

        // Feed rate mode and feed rate.
        self.modal.inverse_time = inverse_time;
        self.modal.feed = v.f;
        // Spindle speed: a new S with the spindle on, outside a laser move,
        // syncs and applies it now.
        if self.modal.speed != v.s || force_sync {
            if self.modal.spindle != Spindle::Off && !laser_is_motion {
                let rpm = if laser_disable { 0.0 } else { v.s };
                self.spindle_sync(self.modal.spindle, rpm, check);
            }
            self.modal.speed = v.s;
        }
        let rpm = if laser_disable { 0.0 } else { self.modal.speed };
        // Tool select: GRBL only records it, and a line without T sets 0.
        self.modal.tool = v.t as u8;
        // Spindle control.
        if self.modal.spindle != spindle {
            self.spindle_sync(spindle, rpm, check);
            self.modal.spindle = spindle;
        }
        // Coolant control.
        if (self.modal.flood, self.modal.mist) != (flood, mist) {
            if !check {
                self.ops.push_back(Op::Sync);
                self.ops.push_back(Op::Coolant { flood, mist });
            }
            self.modal.flood = flood;
            self.modal.mist = mist;
        }
        // Dwell.
        if non_modal == NonModal::Dwell && !check {
            self.ops.push_back(Op::Sync);
            self.ops.push_back(Op::Dwell(v.p));
        }
        // Plane and units.
        self.modal.plane = plane;
        self.modal.inches = inches;
        // Tool length offset.
        if axis_command == AxisCommand::ToolLengthOffset {
            let offset = if cancel_tool_length {
                0.0
            } else {
                v.xyz[TOOL_LENGTH_AXIS]
            };
            if self.modal.tool_length_offset != offset {
                self.modal.tool_length_offset = offset;
                self.wco_counter = 0;
            }
        }
        // Coordinate system selection.
        if self.modal.coord_select != coord_select {
            self.modal.coord_select = coord_select;
            self.wco_counter = 0;
        }
        // Distance mode.
        self.modal.incremental = incremental;
        // Coordinate data, predefined positions and offsets.
        let data = LineData {
            feed: v.f,
            rapid: false,
            no_feed_override: false,
            inverse_time,
            spindle: self.modal.spindle,
            rpm,
            line: self.lines_read,
        };
        match non_modal {
            NonModal::SetCoordinateData => {
                let (index, data) = coordinate_data.expect("checked above");
                self.ops.push_back(Op::Sync);
                self.ops.push_back(Op::Coordinates(index, data));
            }
            NonModal::GoHome(_) => {
                let home = home.expect("checked above");
                let rapid = LineData {
                    rapid: true,
                    ..data
                };
                if axis_command != AxisCommand::None {
                    self.ops.push_back(Op::Line(target, rapid));
                }
                self.ops.push_back(Op::Line(home, rapid));
                self.gc_position = home;
            }
            NonModal::SetHome(index) => {
                self.ops.push_back(Op::Sync);
                self.ops.push_back(Op::Coordinates(index, position));
            }
            NonModal::SetCoordinateOffset => {
                self.coordinates.g92 = new_g92.expect("checked above");
                self.wco_counter = 0;
            }
            NonModal::ResetCoordinateOffset => {
                self.coordinates.g92 = [0.0; 3];
                self.wco_counter = 0;
            }
            _ => {}
        }
        // Motion.
        self.modal.motion = motion;
        if motion != MOTION_NONE && axis_command == AxisCommand::Motion {
            let data = LineData {
                rapid: motion == 0,
                ..data
            };
            match arc {
                None => self.ops.push_back(Op::Line(target, data)),
                Some((offset, radius)) => {
                    let tolerance = self.profile.settings.arc_tolerance();
                    let (points, segments) = arc_points(
                        position,
                        target,
                        offset,
                        radius,
                        motion == 2,
                        tolerance,
                        arc_plane,
                    );
                    // GRBL scales an inverse-time F by the segment count and
                    // runs the segments as mm/min.
                    let data = if data.inverse_time && segments > 0 {
                        LineData {
                            feed: data.feed * f64::from(segments),
                            inverse_time: false,
                            ..data
                        }
                    } else {
                        data
                    };
                    self.ops
                        .extend(points.into_iter().map(|point| Op::Line(point, data)));
                }
            }
            self.gc_position = target;
        }
        // Program flow. M0 finishes motion, then holds until cycle start.
        match program_flow {
            ProgramFlow::Running => {}
            ProgramFlow::Paused => {
                self.ops.push_back(Op::Sync);
                if !check {
                    self.ops.push_back(Op::Pause);
                    self.ops.push_back(Op::AwaitResume);
                }
            }
            ProgramFlow::Completed => {
                // Finish motion, then reset modes, spindle and coolant. Units,
                // tool length offset, F and S are kept.
                self.ops.push_back(Op::Sync);
                self.modal.motion = 1;
                self.modal.plane = 0;
                self.modal.incremental = false;
                self.modal.inverse_time = false;
                self.modal.coord_select = 0;
                self.modal.spindle = Spindle::Off;
                self.modal.flood = false;
                self.modal.mist = false;
                self.ops.push_back(Op::ProgramEnd);
            }
        }
        Ok(())
    }

    /// GRBL's `spindle_sync`: wait for motion, then set the spindle. Check
    /// mode skips both.
    fn spindle_sync(&mut self, dir: Spindle, rpm: f64, check: bool) {
        if !check {
            self.ops.push_back(Op::Sync);
            self.ops.push_back(Op::Spindle(dir, rpm));
        }
    }
}

/// The centre offset (in the arc's plane) and radius of an R-form arc, as
/// GRBL computes them. A negative R asks for the long way round.
fn radius_arc(
    position: [f64; 3],
    target: [f64; 3],
    x: f64,
    y: f64,
    r: f64,
    inches: bool,
    motion: u8,
) -> Result<([f64; 2], f64), u8> {
    if position == target {
        return Err(ERR_INVALID_TARGET);
    }
    let mut r = if inches { r * MM_PER_INCH } else { r };
    let h_sqr = 4.0 * r * r - x * x - y * y;
    if h_sqr < 0.0 {
        return Err(ERR_ARC_RADIUS);
    }
    let mut h_x2_div_d = -h_sqr.sqrt() / x.hypot(y);
    if motion == 3 {
        h_x2_div_d = -h_x2_div_d;
    }
    if r < 0.0 {
        h_x2_div_d = -h_x2_div_d;
        r = -r;
    }
    let offset = [0.5 * (x - y * h_x2_div_d), 0.5 * (y + x * h_x2_div_d)];
    Ok((offset, r))
}

/// Reads one `X10.5` word from the front of a line, GRBL's way: a letter
/// then a number.
fn next_word(text: &str) -> Result<(char, f64, &str), u8> {
    let mut chars = text.chars();
    let letter = chars.next().ok_or(ERR_EXPECTED_LETTER)?;
    if !letter.is_ascii_uppercase() {
        return Err(ERR_EXPECTED_LETTER);
    }
    let rest = chars.as_str();
    let sign = usize::from(rest.starts_with(['-', '+']));
    let end = rest[sign..]
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(rest.len(), |i| i + sign);
    let value = parse_number(&rest[..end]).ok_or(ERR_BAD_NUMBER)?;
    Ok((letter, value, &rest[end..]))
}
