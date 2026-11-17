//! `bf-sim check <file.gcode> [--profile <profile.json>] [--json]`: runs a
//! job on the simulated machine and prints its report. Exits 1 if the job
//! has violations and 2 if it could not be checked.

use std::process::ExitCode;

use bf_sim::Profile;

const USAGE: &str = "usage: bf-sim check <file.gcode> [--profile <profile.json>] [--json]

Streams the job through the GRBL emulator and reports its time, extent and
violations. The profile is a JSON bf_sim::Profile; the default is the
placeholder diode laser.";

struct Args {
    file: String,
    profile: Option<String>,
    json: bool,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("check") => {}
        Some(other) => return Err(format!("unknown command {other:?}")),
        None => return Err("missing command".to_string()),
    }
    let (mut file, mut profile, mut json) = (None, None, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--profile" => {
                profile = Some(args.next().ok_or("--profile needs a file")?);
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ if file.is_none() => file = Some(arg),
            _ => return Err(format!("unexpected argument {arg:?}")),
        }
    }
    Ok(Args {
        file: file.ok_or("missing G-code file")?,
        profile,
        json,
    })
}

fn run(args: Args) -> Result<bool, String> {
    let gcode =
        std::fs::read_to_string(&args.file).map_err(|e| format!("reading {}: {e}", args.file))?;
    let profile = match &args.profile {
        Some(path) => {
            let json = std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
            serde_json::from_str::<Profile>(&json).map_err(|e| format!("parsing {path}: {e}"))?
        }
        None => Profile::diode_laser(),
    };
    let report = bf_sim::check(&gcode, profile);
    if args.json {
        let json = serde_json::to_string_pretty(&report).expect("reports serialise");
        println!("{json}");
    } else {
        println!("{report}");
    }
    Ok(report.passed())
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("bf-sim: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("bf-sim: {message}");
            ExitCode::from(2)
        }
    }
}
