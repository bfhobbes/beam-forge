//! GRBL 1.1 GCode.
//!
//! `G21 G90`, then each layer: air assist on (the profile's command) or off
//! (`M9`) as the layer asks, the laser enabled in dynamic (`M4`) or constant
//! (`M3`) power mode with `S0`, then its moves. Rapids are `G0` with `S0`
//! whenever the power was on; cuts are `G1` with `S` scaled from percent to
//! the profile's `$30`, and `S` and `F` only when they change. The job ends
//! with `M5` and `M9`.

use super::{FrameProgram, JobProgram, WrittenJob};
use crate::plan::Move;

/// Writes `job` as GRBL GCode, one command per line, `\n` line endings,
/// one line per move.
pub(crate) fn write(job: &JobProgram) -> WrittenJob {
    let mut out = Writer::default();
    for line in job.header {
        out.line(format!("; {}", one_line(line)));
    }
    out.line("G21 G90".to_string());

    // GRBL powers up with the laser off, S0 and air off.
    let mut laser: Option<&str> = None;
    let mut power = number(0.0);
    let mut feed: Option<String> = None;
    let mut air = false;
    let mut move_lines = Vec::with_capacity(job.plan.moves().count());
    // GRBL waits for motion to finish before it changes the laser or air
    // assist (`spindle_sync`, `coolant_sync`).
    let mut stops = Vec::new();
    for plan in &job.plan.layers {
        let layer = job.layer(plan);
        let name = layer.map_or(plan.layer_id.as_str(), |layer| layer.name.as_str());
        out.line(format!("; Layer {}", one_line(name)));
        if plan.air_assist != air {
            if stops.last() != Some(&move_lines.len()) {
                stops.push(move_lines.len());
            }
            air = plan.air_assist;
            out.line(if air {
                job.profile.air_assist.trim().to_string()
            } else {
                "M9".to_string()
            });
        }
        let mode = if layer.is_some_and(|layer| layer.constant_power) {
            "M3"
        } else {
            "M4"
        };
        if laser != Some(mode) {
            // S0 first, so switching to M3 can't fire at the last power.
            if stops.last() != Some(&move_lines.len()) {
                stops.push(move_lines.len());
            }
            laser = Some(mode);
            power = number(0.0);
            out.line(format!("{mode} S0"));
        }
        for step in &plan.moves {
            match *step {
                Move::Rapid { to } => {
                    let mut line = format!("G0 X{} Y{}", number(to.x), number(to.y));
                    if power != "0" {
                        power = number(0.0);
                        line.push_str(" S0");
                    }
                    out.line(line);
                    move_lines.push(out.commands);
                }
                Move::Cut {
                    to,
                    power: percent,
                    speed,
                } => {
                    let mut line = format!("G1 X{} Y{}", number(to.x), number(to.y));
                    let s = number(percent / 100.0 * job.profile.max_spindle);
                    if s != power {
                        line.push_str(&format!(" S{s}"));
                        power = s;
                    }
                    let f = number(speed);
                    if feed.as_deref() != Some(f.as_str()) {
                        line.push_str(&format!(" F{f}"));
                        feed = Some(f);
                    }
                    out.line(line);
                    move_lines.push(out.commands);
                }
            }
        }
    }
    out.line("M5".to_string());
    out.line("M9".to_string());
    WrittenJob {
        gcode: out.text,
        move_lines,
        stops,
    }
}

/// Writes `frame` as GRBL GCode. With the laser off it is `M5 S0`, then the
/// outline at the frame speed; a low-power frame is `M4 S0`, then the
/// outline at its `S`, so GRBL's laser mode keeps it dark on the rapids. The
/// head parks with a rapid, and the frame ends with `M5`.
pub(crate) fn write_frame(frame: &FrameProgram) -> String {
    let mut out = Writer::default();
    for line in frame.header {
        out.line(format!("; {}", one_line(line)));
    }
    out.line("G21 G90".to_string());
    out.line(
        if frame.power.is_some() {
            "M4 S0"
        } else {
            "M5 S0"
        }
        .to_string(),
    );
    if let Some((first, rest)) = frame.path.split_first() {
        out.line(format!("G0 X{} Y{}", number(first.x), number(first.y)));
        for (index, to) in rest.iter().enumerate() {
            let mut line = format!("G1 X{} Y{}", number(to.x), number(to.y));
            if index == 0 {
                if let Some(s) = frame.power {
                    line.push_str(&format!(" S{}", number(s)));
                }
                line.push_str(&format!(" F{}", number(frame.speed)));
            }
            out.line(line);
        }
    }
    if let Some(park) = frame.park {
        let off = if frame.power.is_some() { " S0" } else { "" };
        out.line(format!("G0 X{} Y{}{off}", number(park.x), number(park.y)));
    }
    out.line("M5".to_string());
    out.text
}

#[derive(Default)]
struct Writer {
    text: String,
    /// Command lines written so far: lines that aren't comments, counted as
    /// [`crate::machine::job_line`] and a job's progress count them.
    commands: u32,
}

impl Writer {
    fn line(&mut self, line: String) {
        if crate::machine::job_line(&line).is_some() {
            self.commands += 1;
        }
        self.text.push_str(&line);
        self.text.push('\n');
    }
}

/// `text` with line breaks replaced, safe inside a `;` comment.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// A GCode number: at most three decimals (a micron), no trailing zeros,
/// no `-0`.
pub(crate) fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    let text = format!("{rounded:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" | "" => "0".to_string(),
        text => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(number(0.0), "0");
        assert_eq!(number(-0.0001), "0");
        assert_eq!(number(10.0), "10");
        assert_eq!(number(12.5), "12.5");
        assert_eq!(number(1.23456), "1.235");
        assert_eq!(number(-3.1), "-3.1");
        assert_eq!(number(1000.0), "1000");
    }
}
