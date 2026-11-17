//! The "Built-in simulator" port: starts the simulator shipped next to the
//! app, on a port the OS picks.
//!
//! The simulator, started with [`LAUNCH_FLAG`], prints one line with its
//! address on stdout and exits when its stdin closes. The app keeps stdin
//! open for as long as it owns the [`Launched`] simulator, so dropping it, or
//! the app exiting for any reason, closes the simulator. The simulator has
//! its own copy of this line format (the two must not share code); change
//! both together.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

pub const LAUNCH_FLAG: &str = "--launched-by-app";
const LISTENING: &str = "BF_SIM_LISTENING";
const FAILED: &str = "BF_SIM_FAILED";
/// How long the simulator may take to report its address. Generous, as the
/// first launch on a machine can wait on WebView2 starting up.
pub const REPORT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a closing simulator may take to exit before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(3);

/// The simulator's file name, as bundled next to the app (Tauri strips the
/// target triple from sidecars when it installs them).
pub fn simulator_file_name() -> String {
    format!("bf-simulator{}", std::env::consts::EXE_SUFFIX)
}

/// The simulator in `dir`, or a message for the user saying how to get it.
pub fn locate(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(simulator_file_name());
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "The built-in simulator isn't installed ({} is missing). \
             In development, run `npm run sim:dev` first: it builds the simulator \
             and serves its window.",
            path.display()
        ))
    }
}

/// Starts the simulator next to the running app and waits for its address.
pub fn launch_simulator() -> Result<Launched, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("Couldn't find the app's folder: {error}"))?;
    let dir = exe.parent().ok_or("Couldn't find the app's folder")?;
    let mut command = Command::new(locate(dir)?);
    command.arg(LAUNCH_FLAG);
    launch(command, REPORT_TIMEOUT)
}

/// A simulator the app started. Dropping it closes the simulator.
pub struct Launched {
    process: Process,
    address: SocketAddr,
}

impl Launched {
    /// Where the simulator listens.
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

impl std::fmt::Debug for Launched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launched")
            .field("pid", &self.process.child.id())
            .field("address", &self.address)
            .finish()
    }
}

/// The child process. Dropping it closes stdin, which asks the simulator to
/// exit, and kills it if it hasn't within [`EXIT_GRACE`].
struct Process {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Drop for Process {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + EXIT_GRACE;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Ok(Some(_)) | Err(_) => return,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What the simulator reported on stdout.
#[derive(Debug, PartialEq)]
enum Report {
    Listening(SocketAddr),
    Failed(String),
}

/// Reads one stdout line; anything other than a report is ignored.
fn parse_report(line: &str) -> Option<Report> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(LISTENING) {
        let rest = rest.trim();
        Some(match rest.parse() {
            Ok(address) => Report::Listening(address),
            Err(_) => Report::Failed(format!("it reported an unreadable address \"{rest}\"")),
        })
    } else {
        line.strip_prefix(FAILED)
            .map(|reason| Report::Failed(reason.trim().to_string()))
    }
}

/// Starts `command` with piped stdin and stdout and waits up to `timeout` for
/// it to report where it listens.
pub fn launch(mut command: Command, timeout: Duration) -> Result<Launched, String> {
    let name = Path::new(command.get_program())
        .file_name()
        .unwrap_or(OsStr::new("simulator"))
        .to_string_lossy()
        .into_owned();
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Couldn't start the simulator ({name}): {error}"))?;
    let stdout = child.stdout.take();
    let process = Process {
        stdin: child.stdin.take(),
        child,
    };
    let (reports, received) = mpsc::channel();
    if let Some(stdout) = stdout {
        thread::Builder::new()
            .name("sim-stdout".into())
            .spawn(move || {
                // Keep reading after the report so the simulator can never
                // block on a full pipe.
                let mut reports = Some(reports);
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if let Some(report) = parse_report(&line)
                        && let Some(reports) = reports.take()
                    {
                        let _ = reports.send(report);
                    }
                }
            })
            .map_err(|error| error.to_string())?;
    }
    match received.recv_timeout(timeout) {
        Ok(Report::Listening(address)) => Ok(Launched { process, address }),
        Ok(Report::Failed(reason)) => Err(format!("The simulator couldn't start: {reason}")),
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "The simulator didn't report its address within {} seconds.",
            timeout.as_secs_f32()
        )),
        Err(RecvTimeoutError::Disconnected) => {
            Err("The simulator exited before reporting its address.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A stand-in for the simulator: this test binary, re-run with
    /// `BF_STAND_IN` set to how it should behave. Does nothing in a normal run.
    #[test]
    fn stand_in() {
        let Ok(mode) = std::env::var("BF_STAND_IN") else {
            return;
        };
        match mode.as_str() {
            "listening" => {
                println!("starting up");
                println!("BF_SIM_LISTENING 127.0.0.1:4567");
            }
            "failed" => println!("BF_SIM_FAILED Couldn't listen on 127.0.0.1:0"),
            "garbled" => println!("BF_SIM_LISTENING somewhere"),
            "exit" => return,
            _ => {}
        }
        // Like the simulator: run until stdin closes, then say so.
        let _ = std::io::stdin().read_to_end(&mut Vec::new());
        if let Ok(done) = std::env::var("BF_STAND_IN_DONE") {
            std::fs::write(done, "closed").unwrap();
        }
    }

    fn stand_in_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "machine::launch::tests::stand_in", "--nocapture"])
            .env("BF_STAND_IN", mode)
            .stderr(Stdio::null());
        command
    }

    #[test]
    fn reports_are_parsed_and_other_lines_ignored() {
        assert_eq!(
            parse_report("BF_SIM_LISTENING 127.0.0.1:54321\r"),
            Some(Report::Listening("127.0.0.1:54321".parse().unwrap()))
        );
        assert_eq!(
            parse_report("BF_SIM_FAILED port in use"),
            Some(Report::Failed("port in use".into()))
        );
        assert!(matches!(
            parse_report("BF_SIM_LISTENING nowhere"),
            Some(Report::Failed(_))
        ));
        assert_eq!(parse_report("running 1 test"), None);
    }

    #[test]
    fn launch_reads_the_address_and_dropping_closes_the_child() {
        let done = std::env::temp_dir().join(format!("bf-stand-in-{}", std::process::id()));
        let _ = std::fs::remove_file(&done);
        let mut command = stand_in_command("listening");
        command.env("BF_STAND_IN_DONE", &done);
        let launched = launch(command, Duration::from_secs(20)).unwrap();
        assert_eq!(launched.address(), "127.0.0.1:4567".parse().unwrap());
        assert!(!done.exists(), "the child should still be running");
        drop(launched);
        // Closed through stdin, not killed: the child wrote its marker.
        assert_eq!(std::fs::read_to_string(&done).unwrap(), "closed");
        let _ = std::fs::remove_file(&done);
    }

    #[test]
    fn a_silent_child_times_out() {
        let started = Instant::now();
        let error = launch(stand_in_command("silent"), Duration::from_millis(300)).unwrap_err();
        assert_eq!(
            error,
            "The simulator didn't report its address within 0.3 seconds."
        );
        assert!(started.elapsed() < EXIT_GRACE, "closed through stdin");
    }

    #[test]
    fn a_reported_failure_is_shown() {
        let error = launch(stand_in_command("failed"), Duration::from_secs(20)).unwrap_err();
        assert_eq!(
            error,
            "The simulator couldn't start: Couldn't listen on 127.0.0.1:0"
        );
        let error = launch(stand_in_command("garbled"), Duration::from_secs(20)).unwrap_err();
        assert!(
            error.contains("unreadable address \"somewhere\""),
            "{error}"
        );
    }

    #[test]
    fn a_child_that_exits_without_reporting_fails() {
        let error = launch(stand_in_command("exit"), Duration::from_secs(20)).unwrap_err();
        assert_eq!(error, "The simulator exited before reporting its address.");
    }

    #[test]
    fn a_missing_simulator_says_how_to_build_it() {
        let dir = std::env::temp_dir().join(format!("bf-no-sim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let error = locate(&dir).unwrap_err();
        assert!(error.contains(&simulator_file_name()), "{error}");
        assert!(error.contains("npm run sim:dev"), "{error}");
        let error = launch(Command::new(dir.join("absent.exe")), REPORT_TIMEOUT).unwrap_err();
        assert!(
            error.starts_with("Couldn't start the simulator (absent.exe)"),
            "{error}"
        );

        std::fs::write(dir.join(simulator_file_name()), "").unwrap();
        assert_eq!(locate(&dir), Ok(dir.join(simulator_file_name())));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
