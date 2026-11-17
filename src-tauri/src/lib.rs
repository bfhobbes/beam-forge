//! Desktop shell: Tauri commands and plugins. Product logic lives in `bf-core`.

mod files;
mod gcode;
mod import;
mod machine;
mod project;
mod text;
#[cfg(test)]
mod update_tests;

use bf_core::CoreInfo;
use serde::Serialize;
use ts_rs::TS;

/// Application and platform details for the About panel and status bar.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub app_version: String,
    pub platform: String,
    pub arch: String,
    pub core: CoreInfo,
}

#[tauri::command]
fn app_info<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppInfo {
    AppInfo {
        app_version: app.package_info().version.to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        core: bf_core::core_info(),
    }
}

/// State and commands, shared by the app and the IPC tests.
fn commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .manage(machine::Machine::default())
        .manage(machine::settings::Profiles::default())
        .manage(machine::safety::RealHardwareGate::default())
        .manage(machine::job::JobPicker::default())
        .manage(gcode::ExportPicker::default())
        .manage(project::ProjectFiles::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            import::import_pick,
            import::import_file,
            import::import_svg_text,
            machine::machine_ports,
            machine::machine_state,
            machine::machine_connect,
            machine::machine_disconnect,
            machine::safety::machine_real_hardware,
            machine::safety::machine_set_real_hardware,
            machine::settings::machine_settings,
            machine::settings::machine_write_setting,
            machine::settings::machine_profiles,
            machine::settings::machine_profile_save,
            machine::machine_send,
            machine::machine_soft_reset,
            machine::control::machine_jog,
            machine::control::machine_jog_hold,
            machine::control::machine_jog_stop,
            machine::control::machine_command,
            machine::control::machine_test_fire,
            machine::job::machine_run_file,
            machine::job::machine_preflight,
            machine::job::machine_run_document,
            machine::job::machine_frame,
            machine::job::machine_job,
            machine::job::machine_job_pause,
            machine::job::machine_job_resume,
            machine::job::machine_job_stop,
            machine::job::machine_job_stop_wait,
            machine::job::machine_override,
            gcode::gcode_summary,
            gcode::gcode_toolpath,
            gcode::gcode_export,
            project::project_open,
            project::project_open_dialog,
            project::project_save,
            project::project_save_as,
            project::project_recent,
            project::project_recovery,
            project::project_recovery_save,
            project::project_recovery_clear,
            project::project_launch_file,
            text::text_fonts,
            text::text_layout,
        ])
}

/// The updater. Releases are public GitHub releases, so it sends no
/// credentials; packages are trusted by their signatures alone.
fn updater_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config>
{
    tauri_plugin_updater::Builder::new().build()
}

pub fn run() {
    text::warm();
    commands(tauri::Builder::default())
        .plugin(updater_plugin())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .build(tauri::generate_context!())
        .expect("Unable to start Beam Forge")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // However the app ends up exiting, a job must not outlive it:
                // stop it with the laser off first.
                machine::shutdown(app);
                // A clean exit leaves no recovery copy; one found at launch means a crash.
                if let Ok(recovery) = project::ProjectFiles::recovery(app) {
                    recovery.clear();
                }
            }
        });
}

#[cfg(test)]
mod ipc_tests {
    use super::*;
    use serde_json::{Value, json};
    use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};

    fn request(cmd: &str, url: tauri::Url, body: Value) -> tauri::webview::InvokeRequest {
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url,
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        }
    }

    // Loads the real tauri.conf.json and capabilities, so a missing permission
    // fails here rather than silently in the shipped app.
    fn main_window() -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
        let app = commands(mock_builder())
            .build(tauri::generate_context!())
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        (app, window)
    }

    #[test]
    fn main_window_can_complete_the_sdk_close_request() {
        // onCloseRequested calls window.destroy once the handler allows closing.
        let (_app, window) = main_window();
        let url = window.url().unwrap();
        let response = get_ipc_response(
            &window,
            request("plugin:window|destroy", url, json!({ "label": "main" })),
        );
        assert!(
            response.is_ok(),
            "SDK close completion must be authorized: {response:?}"
        );
    }

    #[test]
    fn main_window_can_show_the_project_in_its_title() {
        let (_app, window) = main_window();
        let url = window.url().unwrap();
        let response = get_ipc_response(
            &window,
            request(
                "plugin:window|set_title",
                url,
                json!({ "label": "main", "value": "● Coaster – Beam Forge" }),
            ),
        );
        assert!(
            response.is_ok(),
            "set_title must be authorized: {response:?}"
        );
    }

    #[test]
    fn app_info_round_trips_typed_core_details() {
        let (_app, window) = main_window();
        let url = window.url().unwrap();
        let info = get_ipc_response(&window, request("app_info", url, json!({})))
            .map(|body| body.deserialize::<Value>().unwrap())
            .unwrap();
        assert_eq!(info["core"]["version"], bf_core::VERSION);
        assert_eq!(info["core"]["firmware"][0]["id"], "grbl");
        assert_eq!(info["platform"], std::env::consts::OS);
    }

    fn call(
        window: &tauri::WebviewWindow<MockRuntime>,
        cmd: &str,
        body: Value,
    ) -> Result<Value, Value> {
        let url = window.url().unwrap();
        get_ipc_response(window, request(cmd, url, body))
            .map(|body| body.deserialize::<Value>().unwrap())
    }

    /// Held while binding a port, so a port one test freed on purpose
    /// can't be handed to another test's simulator before it is used.
    static PORTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A listener on a free loopback port. Tests bind through this, not
    /// directly, so they respect [`PORTS`].
    fn bind_local() -> std::net::TcpListener {
        let _ports = PORTS.lock().unwrap_or_else(|e| e.into_inner());
        std::net::TcpListener::bind("127.0.0.1:0").unwrap()
    }

    /// Serves one `bf-sim` controller over TCP, as the simulator app does,
    /// running its clock in real time.
    fn serve_simulator() -> u16 {
        serve_shared_simulator(bf_sim::Profile::grbl_defaults()).0
    }

    /// [`serve_simulator`] with `profile`, sharing the simulator so a test
    /// can read its burn trace.
    fn serve_shared_simulator(
        profile: bf_sim::Profile,
    ) -> (u16, std::sync::Arc<std::sync::Mutex<bf_sim::Simulator>>) {
        use bf_sim::Simulator;
        use std::io::{Read, Write};
        use std::sync::mpsc::{self, RecvTimeoutError};
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};

        let listener = bind_local();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Mutex::new(Simulator::new(profile)));
        let simulator = shared.clone();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Blocking reads on their own thread, as the simulator app does:
            // a timed-out socket read on Windows can fail the next one.
            let mut reader = stream.try_clone().unwrap();
            let (chunks, incoming) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = [0; 256];
                while let Ok(read @ 1..) = reader.read(&mut buf) {
                    if chunks.send(buf[..read].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let started = Instant::now();
            loop {
                let output = simulator.lock().unwrap().take_output();
                if stream.write_all(&output).is_err() {
                    break;
                }
                let incoming = incoming.recv_timeout(Duration::from_millis(5));
                let mut sim = simulator.lock().unwrap();
                match incoming {
                    Ok(bytes) => sim.feed(&bytes),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                let now = started.elapsed();
                let behind = now.saturating_sub(sim.now());
                sim.advance(behind);
            }
        });
        (port, shared)
    }

    /// Where a burn started and ended, and its peak power (0 to 1).
    type Burn = ([f64; 2], [f64; 2], f64);

    /// The simulator's lased moves, one per GCode line.
    fn burns(sim: &bf_sim::Simulator) -> Vec<Burn> {
        let mut burns: Vec<(u64, Burn)> = Vec::new();
        for segment in sim.trace() {
            if segment.rapid || segment.is_stationary() {
                continue;
            }
            let (from, to) = (
                [segment.start[0], segment.start[1]],
                [segment.end[0], segment.end[1]],
            );
            match burns.last_mut() {
                Some((line, burn)) if *line == segment.line => {
                    burn.1 = to;
                    burn.2 = burn.2.max(segment.power);
                }
                _ => burns.push((segment.line, (from, to, segment.power))),
            }
        }
        burns
            .into_iter()
            .map(|(_, burn)| burn)
            .filter(|burn| burn.2 > 0.0)
            .collect()
    }

    fn tcp_port(port: u16) -> Value {
        json!({
            "key": format!("tcp:127.0.0.1:{port}"),
            "address": { "kind": "tcp", "host": "127.0.0.1", "port": port },
            "label": format!("Test ({port})"),
            "description": null,
            "vid": null,
            "pid": null,
        })
    }

    #[test]
    fn lists_fonts_and_checks_text_styles() {
        let (_app, window) = main_window();
        let fonts = call(&window, "text_fonts", json!({})).unwrap();
        assert!(fonts.is_array());
        let style = json!({
            "fontFamily": "Arial", "fontWeight": 400, "italic": false, "height": 0,
            "letterSpacing": 0, "lineSpacing": 1, "align": "left"
        });
        let error = call(
            &window,
            "text_layout",
            json!({ "text": "Hi", "style": style }),
        )
        .unwrap_err();
        assert_eq!(error, json!("Height must be above zero"));
    }

    #[test]
    fn imports_svg_text_and_svg_and_dxf_files() {
        let (_app, window) = main_window();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><rect x="5" y="2" width="10" height="4" stroke="#ff0000" fill="none"/></svg>"##;
        let drawing = call(&window, "import_svg_text", json!({ "text": svg })).unwrap();
        assert_eq!(drawing["width"], 20.0);
        assert_eq!(drawing["paths"][0]["color"], "#ff0000");
        assert_eq!(
            drawing["paths"][0]["subpaths"][0]["start"],
            json!({ "x": 5.0, "y": 8.0 })
        );
        assert_eq!(drawing["segments"], 3, "the closing edge is implicit");

        let invalid = call(&window, "import_svg_text", json!({ "text": "<html/>" })).unwrap_err();
        assert!(
            invalid
                .as_str()
                .unwrap()
                .starts_with("This isn't a valid SVG"),
            "{invalid}"
        );

        let dir = std::env::temp_dir().join(format!("bf-ipc-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Coaster.svg");
        std::fs::write(&file, svg).unwrap();
        let imported = call(&window, "import_file", json!({ "path": file })).unwrap();
        assert_eq!(imported["kind"], "imported");
        assert_eq!(imported["file"]["name"], "Coaster");
        assert_eq!(imported["file"]["drawing"]["height"], 10.0);
        let not_svg = call(
            &window,
            "import_file",
            json!({ "path": dir.join("notes.txt") }),
        );
        assert_eq!(not_svg.unwrap_err(), "notes is not an SVG or DXF file.");
        let missing = call(
            &window,
            "import_file",
            json!({ "path": dir.join("gone.svg") }),
        );
        assert!(
            missing
                .unwrap_err()
                .as_str()
                .unwrap()
                .starts_with("Couldn't read the file")
        );

        // A DXF without $INSUNITS asks for units, then imports in them.
        let dxf = "0
SECTION
2
ENTITIES
0
LINE
8
0
10
0
20
0
11
2
21
1
0
ENDSEC
0
EOF
";
        let file = dir.join("Bracket.DXF");
        std::fs::write(&file, dxf).unwrap();
        let ask = call(&window, "import_file", json!({ "path": file })).unwrap();
        assert_eq!(ask["kind"], "needsUnits");
        assert_eq!(ask["name"], "Bracket");
        assert_eq!(
            (ask["width"].clone(), ask["height"].clone()),
            (json!(2.0), json!(1.0))
        );
        let imported = call(
            &window,
            "import_file",
            json!({ "path": ask["path"], "units": "in" }),
        )
        .unwrap();
        assert_eq!(imported["file"]["drawing"]["width"], 50.8);
        assert_eq!(imported["file"]["drawing"]["format"], "dxf");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn port_list_always_offers_the_simulator() {
        let (_app, window) = main_window();
        let ports = call(&window, "machine_ports", json!({})).unwrap();
        let last = ports.as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["label"], "Simulator (127.0.0.1:23232)");
        assert_eq!(last["address"]["kind"], "tcp");
    }

    #[test]
    fn unreachable_tcp_port_fails_with_a_clear_error() {
        let (_app, window) = main_window();
        let _ports = PORTS.lock().unwrap_or_else(|e| e.into_inner());
        let unused = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let error = call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(unused) }),
        )
        .unwrap_err();
        assert!(
            error
                .as_str()
                .unwrap()
                .starts_with(&format!("Couldn't reach 127.0.0.1:{unused}")),
            "{error}"
        );
    }

    #[test]
    fn connects_over_tcp_loopback_to_the_simulator() {
        use std::sync::{Arc, Mutex};
        use tauri::Listener;

        let (app, window) = main_window();
        let states = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = states.clone();
        app.listen(machine::STATE_EVENT, move |event| {
            let state: Value = serde_json::from_str(event.payload()).unwrap();
            seen.lock().unwrap().push(state["status"].clone());
        });
        let port = serve_simulator();

        let machine = call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        assert_eq!(machine["firmware"], "grbl");
        assert_eq!(machine["name"], "Grbl");
        assert_eq!(machine["version"], "1.1h");

        let state = call(&window, "machine_state", json!({})).unwrap();
        assert_eq!(state["status"], "connected");
        assert_eq!(state["machine"]["version"], "1.1h");
        let again = call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        );
        assert!(again.is_err(), "a second connect is refused");

        call(&window, "machine_disconnect", json!({})).unwrap();
        let state = call(&window, "machine_state", json!({})).unwrap();
        assert_eq!(state, json!({ "status": "disconnected", "reason": null }));
        assert_eq!(
            *states.lock().unwrap(),
            ["connecting", "connected", "disconnected"]
        );
    }

    #[test]
    fn emits_polled_status_while_connected_to_the_simulator() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use tauri::Listener;

        let (app, window) = main_window();
        let statuses = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = statuses.clone();
        app.listen(machine::STATUS_EVENT, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        let port = serve_simulator();
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while statuses.lock().unwrap().len() < 3 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        call(&window, "machine_disconnect", json!({})).unwrap();
        let count = statuses.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(300));

        let statuses = statuses.lock().unwrap();
        assert_eq!(statuses.len(), count, "no status after disconnecting");
        assert!(statuses.len() >= 3, "{statuses:?}");
        let origin = json!({ "x": 0.0, "y": 0.0, "z": 0.0 });
        assert_eq!(statuses[0]["state"], json!({ "name": "idle" }));
        assert_eq!(statuses[0]["machinePosition"], origin);
        assert_eq!(statuses[0]["workPosition"], origin);
    }

    #[test]
    fn jogs_sets_the_origin_and_test_fires_on_the_simulator() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use tauri::Listener;

        let (app, window) = main_window();
        let statuses = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = statuses.clone();
        app.listen(machine::STATUS_EVENT, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        let wait_for = |what: &str, until: &dyn Fn(&Value) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if statuses.lock().unwrap().last().is_some_and(until) {
                    return;
                }
                assert!(Instant::now() < deadline, "{what}: {statuses:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let idle_at = |x: f64| {
            move |s: &Value| s["state"]["name"] == "idle" && s["machinePosition"]["x"] == x
        };
        let right = json!({ "x": 1, "y": 0 });
        let refused = call(
            &window,
            "machine_jog",
            json!({ "direction": right, "distance": 5.0, "feed": 3000.0 }),
        );
        assert_eq!(refused.unwrap_err(), "The machine is not connected");
        let port = serve_simulator();
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        wait_for("first status", &|s| s["state"]["name"] == "idle");

        // A step jog, then the origin set there.
        call(
            &window,
            "machine_jog",
            json!({ "direction": right, "distance": 5.0, "feed": 3000.0 }),
        )
        .unwrap();
        wait_for("jogged", &idle_at(5.0));
        let command = |command: Value| {
            call(
                &window,
                "machine_command",
                json!({ "command": command, "profile": null }),
            )
        };
        command(json!({ "command": "setOrigin" })).unwrap();
        wait_for("origin set", &|s| s["workPosition"]["x"] == 0.0);

        // A held jog, renewed like the UI does, then released.
        let up = json!({ "direction": { "x": 0, "y": 1 }, "feed": 600.0 });
        for _ in 0..5 {
            call(&window, "machine_jog_hold", up.clone()).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        }
        call(&window, "machine_jog_stop", json!({})).unwrap();
        wait_for("held jog stopped", &|s| {
            s["state"]["name"] == "idle" && s["machinePosition"]["y"].as_f64().unwrap() > 0.0
        });

        command(json!({ "command": "goToOrigin", "feed": 3000.0 })).unwrap();
        wait_for("back at the origin", &|s| {
            s["state"]["name"] == "idle" && s["workPosition"]["y"] == 0.0
        });
        command(json!({ "command": "clearOrigin" })).unwrap();
        wait_for("origin cleared", &|s| s["workPosition"]["x"] == 5.0);

        // The caps hold whatever the UI asks for.
        let fired = call(
            &window,
            "machine_test_fire",
            json!({ "fire": { "power": 50.0, "durationMs": 5000 }, "profile": null }),
        )
        .unwrap();
        assert_eq!(fired, json!({ "power": 10.0, "durationMs": 1000 }));
        let busy = call(&window, "machine_send", json!({ "line": "$I" })).unwrap_err();
        assert_eq!(busy, "The laser is test firing. Try again in a moment.");
        // Disconnecting mid-fire sends the off command on the way out.
        call(&window, "machine_disconnect", json!({})).unwrap();
    }

    #[test]
    fn console_sends_lines_and_receives_decoded_replies() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use tauri::Listener;

        let (app, window) = main_window();
        let lines = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = lines.clone();
        app.listen(machine::LINE_EVENT, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        let wait_for = |find: &dyn Fn(&Value) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(line) = lines.lock().unwrap().iter().find(|l| find(l)) {
                    return line.clone();
                }
                assert!(Instant::now() < deadline, "{:?}", lines.lock().unwrap());
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let refused = call(&window, "machine_send", json!({ "line": "$I" })).unwrap_err();
        assert_eq!(refused, "The machine is not connected");
        let port = serve_simulator();
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();

        call(&window, "machine_send", json!({ "line": "$I" })).unwrap();
        let version = wait_for(&|l| l["line"].as_str().unwrap().starts_with("[VER:"));
        assert_eq!(version["reply"]["kind"], "feedback");

        call(&window, "machine_send", json!({ "line": "G99" })).unwrap();
        let error = wait_for(&|l| l["reply"]["kind"] == "error");
        assert_eq!(error["line"], "error:20");
        assert_eq!(error["reply"]["title"], "Unsupported command");

        call(&window, "machine_soft_reset", json!({})).unwrap();
        wait_for(&|l| l["line"].as_str().unwrap().starts_with("Grbl 1.1h"));
        call(&window, "machine_disconnect", json!({})).unwrap();
    }

    #[test]
    fn real_hardware_needs_the_switch_and_the_current_safety_notice() {
        use tauri::Manager;

        let (app, window) = main_window();
        let dir = std::env::temp_dir().join(format!("bf-ipc-safety-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.state::<machine::safety::RealHardwareGate>()
            .use_dir(dir.clone());
        let serial = json!({
            "key": "serial:BF-NO-SUCH-PORT",
            "address": { "kind": "serial", "path": "BF-NO-SUCH-PORT" },
            "label": "BF-NO-SUCH-PORT",
            "description": null,
            "vid": null,
            "pid": null,
        });
        let version = machine::safety::NOTICE_VERSION;

        let state = call(&window, "machine_real_hardware", json!({})).unwrap();
        assert_eq!(state, json!({ "enabled": false, "noticeVersion": version }));
        let refused = call(&window, "machine_connect", json!({ "port": serial })).unwrap_err();
        assert!(
            refused
                .as_str()
                .unwrap()
                .starts_with("Real laser control is off"),
            "{refused}"
        );
        let state = call(&window, "machine_state", json!({})).unwrap();
        assert_eq!(state, json!({ "status": "disconnected", "reason": null }));

        // An older (or missing) notice version doesn't switch it on.
        for accepted in [json!(null), json!(version - 1)] {
            let stale = call(
                &window,
                "machine_set_real_hardware",
                json!({ "enabled": true, "acceptedNotice": accepted }),
            );
            assert!(stale.is_err(), "{stale:?}");
        }
        let on = call(
            &window,
            "machine_set_real_hardware",
            json!({ "enabled": true, "acceptedNotice": version }),
        )
        .unwrap();
        assert_eq!(on["enabled"], true);
        // Through the gate, a missing port fails with a clear error.
        let missing = call(
            &window,
            "machine_connect",
            json!({ "port": serial, "baud": 115200 }),
        )
        .unwrap_err();
        assert!(
            missing
                .as_str()
                .unwrap()
                .starts_with("Couldn't open BF-NO-SUCH-PORT"),
            "{missing}"
        );
        let state = call(&window, "machine_state", json!({})).unwrap();
        assert_eq!(state["status"], "disconnected");
        assert_eq!(state["reason"], missing);

        // Switching off forgets the acceptance.
        call(
            &window,
            "machine_set_real_hardware",
            json!({ "enabled": false }),
        )
        .unwrap();
        let state = call(&window, "machine_real_hardware", json!({})).unwrap();
        assert_eq!(state["enabled"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_and_sync_the_profile_with_the_simulator() {
        use tauri::Manager;

        let (app, window) = main_window();
        let dir = std::env::temp_dir().join(format!("bf-ipc-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.state::<machine::settings::Profiles>()
            .use_dir(dir.clone());
        let not_connected = call(&window, "machine_settings", json!({})).unwrap_err();
        assert_eq!(not_connected, "Connect to the machine first.");

        let port = serve_simulator();
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();

        let read =
            call(&window, "machine_settings", json!({})).unwrap_or_else(|error| panic!("{error}"));
        let setting = |read: &Value, number: u64| {
            read["settings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["number"] == number)
                .unwrap()
                .clone()
        };
        let laser = setting(&read, 32);
        assert_eq!(laser["value"], "0");
        assert_eq!(laser["name"], "Laser-mode enable");
        assert_eq!(setting(&read, 110)["unit"], "mm/min");
        let id = format!("tcp-127-0-0-1-{port}");
        assert_eq!(read["profile"]["id"], id);
        assert_eq!(read["profile"]["laserMode"], false);
        assert_eq!(
            read["profile"]["bed"],
            json!({ "width": 200.0, "height": 200.0 })
        );
        assert!(dir.join(format!("{id}.json")).exists());

        let rejected = call(
            &window,
            "machine_write_setting",
            json!({ "number": 32, "value": "2" }),
        )
        .unwrap_err();
        assert_eq!(rejected, "$32 (Laser-mode enable) must be 0 or 1.");
        let written = call(
            &window,
            "machine_write_setting",
            json!({ "number": 32, "value": "1" }),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(setting(&written, 32)["value"], "1");
        assert_eq!(written["profile"]["laserMode"], true);

        let mut profile = written["profile"].clone();
        profile["name"] = json!("Phecda");
        call(
            &window,
            "machine_profile_save",
            json!({ "profile": profile }),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        profile["airAssist"] = json!("");
        let invalid = call(
            &window,
            "machine_profile_save",
            json!({ "profile": profile }),
        );
        assert!(
            invalid
                .unwrap_err()
                .as_str()
                .unwrap()
                .contains("air-assist")
        );
        let profiles = call(&window, "machine_profiles", json!({})).unwrap();
        assert_eq!(profiles[0]["name"], "Phecda");
        assert_eq!(profiles[0]["laserMode"], true);

        call(&window, "machine_disconnect", json!({})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn runs_pauses_and_stops_a_gcode_file_on_the_simulator() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use tauri::{Listener, Manager};

        let (app, window) = main_window();
        let jobs = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = jobs.clone();
        app.listen(machine::job::JOB_EVENT, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        // The first progress after `from` in `state`, and where it was.
        let wait_for = |from: usize, state: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let jobs = jobs.lock().unwrap();
                let found = jobs.iter().enumerate().skip(from);
                if let Some((at, job)) = found
                    .into_iter()
                    .find(|(_, j)| j["state"]["state"] == state)
                {
                    return (at + 1, job.clone());
                }
                assert!(Instant::now() < deadline, "no {state} in {jobs:?}");
                drop(jobs);
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let dir = std::env::temp_dir().join(format!("bf-ipc-jobs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let short = dir.join("short.gcode");
        std::fs::write(&short, "; quick\nG21 G90\nG1 X5 F3000\nG0 X0\n").unwrap();
        let long = dir.join("long.nc");
        std::fs::write(&long, "G1 X50 F600\nG1 X0\n").unwrap();
        let picker = || app.state::<machine::job::JobPicker>();

        let refused = call(&window, "machine_run_file", json!({})).unwrap_err();
        assert_eq!(refused, "Connect to the machine first.");
        let port = serve_simulator();
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        // Let the first status report arrive, so the machine is known idle.
        std::thread::sleep(Duration::from_millis(300));

        picker().pick(short);
        let name = call(&window, "machine_run_file", json!({})).unwrap();
        assert_eq!(name, "short.gcode");
        let (_, finished) = wait_for(0, "finished");
        assert_eq!(finished["acknowledged"], 3);
        assert_eq!(finished["total"], 3);
        let job = call(&window, "machine_job", json!({})).unwrap();
        assert_eq!(job["state"]["state"], "finished");

        let from = jobs.lock().unwrap().len();
        picker().pick(long);
        call(&window, "machine_run_file", json!({})).unwrap();
        let (from, _) = wait_for(from, "running");
        let again = call(&window, "machine_run_file", json!({})).unwrap_err();
        assert_eq!(again, "A job is already running.");
        let disconnect = call(&window, "machine_disconnect", json!({})).unwrap_err();
        assert_eq!(disconnect, "A job is running. Stop it first.");
        let console = call(&window, "machine_send", json!({ "line": "G0 X0" })).unwrap_err();
        assert_eq!(console, "A job is running. Stop it first.");

        call(&window, "machine_job_pause", json!({})).unwrap();
        let (from, _) = wait_for(from, "paused");
        call(&window, "machine_job_resume", json!({})).unwrap();
        let (from, _) = wait_for(from, "running");
        call(&window, "machine_job_stop", json!({})).unwrap();
        let (_, stopped) = wait_for(from, "stopped");
        assert_eq!(stopped["state"]["warning"], Value::Null, "{stopped}");
        assert_eq!(stopped["name"], "long.nc");

        call(&window, "machine_disconnect", json!({})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generates_exports_and_runs_the_design_on_the_simulator() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use tauri::{Listener, Manager};

        let (app, window) = main_window();
        let jobs = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = jobs.clone();
        app.listen(machine::job::JOB_EVENT, move |event| {
            seen.lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        let document = json!({ "version": 1, "units": "mm", "layers": [{
            "id": "C00", "name": "Cut", "color": "#ff0000", "mode": "line",
            "speed": 3000.0, "maxPower": 50.0, "minPower": 0.0, "passes": 1,
            "constantPower": false, "airAssist": true, "output": true, "shown": true,
            "lineInterval": 0.1, "scanAngle": 0.0, "bidirectional": true, "overscan": 2.5,
            "fillGap": 1.0
        }], "objects": [{
            "type": "rect", "id": "r1", "name": "Rectangle", "layerId": "C00",
            "transform": [1.0, 0.0, 0.0, 1.0, 10.0, 20.0],
            "width": 2.0, "height": 1.0, "cornerRadius": 0.0
        }] });
        let absolute = json!({ "startFrom": "absolute", "anchor": "center" });
        let here = json!({ "startFrom": "currentPosition", "anchor": "center" });
        let body = |placement: &Value| json!({ "document": document, "profile": null, "placement": placement, "name": "Tag" });

        let summary = call(&window, "gcode_summary", body(&absolute)).unwrap();
        assert_eq!(summary["bounds"]["min"], json!({ "x": 10.0, "y": 20.0 }));
        assert_eq!(summary["insideBed"], true);
        let refused = call(&window, "gcode_summary", body(&here)).unwrap_err();
        assert!(
            refused.as_str().unwrap().contains("connected machine"),
            "{refused}"
        );
        let toolpath = call(&window, "gcode_toolpath", body(&absolute)).unwrap();
        let moves = &toolpath["plan"]["layers"][0]["moves"];
        assert_eq!(
            moves[0],
            json!({ "kind": "rapid", "to": { "x": 10.0, "y": 20.0 } })
        );
        assert_eq!(moves.as_array().unwrap().len(), 5);

        let dir = std::env::temp_dir().join(format!("bf-ipc-gcode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Tag.gcode");
        app.state::<gcode::ExportPicker>().pick(file.clone());
        let saved = call(&window, "gcode_export", body(&absolute)).unwrap();
        assert_eq!(saved, file.display().to_string());
        let gcode = std::fs::read_to_string(&file).unwrap();
        assert!(gcode.starts_with("; Beam Forge job\n"), "{gcode}");
        let report = bf_sim::check(&gcode, bf_sim::Profile::diode_laser());
        assert!(report.passed(), "{report}");

        let refused = call(&window, "machine_run_document", body(&absolute)).unwrap_err();
        assert_eq!(refused, "Connect to the machine first.");
        let (port, sim) = serve_shared_simulator(bf_sim::Profile::diode_laser());
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        // Let the first status report arrive, so the head's position is known.
        std::thread::sleep(Duration::from_millis(300));
        let summary = call(&window, "gcode_summary", body(&here)).unwrap();
        assert_eq!(summary["bounds"]["min"], json!({ "x": -1.0, "y": -0.5 }));

        // Without a profile the machine's settings are unknown: the pre-run
        // checks say so, and the run refuses until that is acknowledged.
        let checks = call(&window, "machine_preflight", body(&here)).unwrap();
        let flagged: Vec<&Value> = checks["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|check| check["level"] != "pass")
            .map(|check| &check["id"])
            .collect();
        // Starting from the head at X0 Y0 puts part of it off the bed, too.
        assert_eq!(flagged, [&json!("laserMode"), &json!("bounds")], "{checks}");
        let refused = call(&window, "machine_run_document", body(&here)).unwrap_err();
        assert!(
            refused
                .as_str()
                .unwrap()
                .starts_with("Machine settings not read."),
            "{refused}"
        );
        let mut acknowledged = body(&here);
        acknowledged["acknowledged"] = json!(["laserMode"]);
        let refused = call(&window, "machine_run_document", acknowledged.clone()).unwrap_err();
        assert!(
            refused.as_str().unwrap().starts_with("Job leaves the bed."),
            "{refused}"
        );
        acknowledged["acknowledged"] = json!(["laserMode", "bounds"]);
        let plan = call(&window, "machine_run_document", acknowledged).unwrap();
        assert_eq!(plan["name"], "Tag");
        assert_eq!(plan["moveLines"], json!([4, 5, 6, 7, 8]), "{plan}");
        assert_eq!(
            plan["placement"]["offset"],
            json!({ "x": -11.0, "y": -20.5 })
        );
        assert_eq!(
            plan["toolpath"],
            call(&window, "gcode_toolpath", body(&here)).unwrap()
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let finished = loop {
            let done = jobs
                .lock()
                .unwrap()
                .iter()
                .find(|job| job["state"]["state"] != "running")
                .cloned();
            if let Some(job) = done {
                break job;
            }
            assert!(Instant::now() < deadline, "the job didn't finish");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(finished["state"]["state"], "finished", "{finished}");
        assert_eq!(finished["name"], "Tag");
        assert_eq!(finished["acknowledged"], summary["lines"]);

        // The burn matches the design: each cut in the run plan's toolpath,
        // placed by its offset, was lased from its start to its end.
        let offset = |p: &Value| {
            [
                p["x"].as_f64().unwrap() + plan["placement"]["offset"]["x"].as_f64().unwrap(),
                p["y"].as_f64().unwrap() + plan["placement"]["offset"]["y"].as_f64().unwrap(),
            ]
        };
        let mut at = offset(&plan["toolpath"]["plan"]["start"]);
        let mut planned = Vec::new();
        for step in plan["toolpath"]["plan"]["layers"][0]["moves"]
            .as_array()
            .unwrap()
        {
            let to = offset(&step["to"]);
            if step["kind"] == "cut" {
                planned.push((at, to, step["power"].as_f64().unwrap() / 100.0));
            }
            at = to;
        }
        let burned = burns(&sim.lock().unwrap());
        assert_eq!(burned.len(), planned.len(), "{burned:?}");
        let near =
            |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        for (burn, plan) in burned.iter().zip(&planned) {
            assert!(
                near(burn.0, plan.0) && near(burn.1, plan.1),
                "{burn:?} vs {plan:?}"
            );
            // M4: power follows speed, up to the programmed power.
            assert!(
                burn.2 > 0.0 && burn.2 <= plan.2 + 1e-9,
                "{burn:?} vs {plan:?}"
            );
        }
        assert_eq!(sim.lock().unwrap().laser_power(), 0.0);

        // Overrides reach the controller by their realtime bytes.
        call(
            &window,
            "machine_override",
            json!({ "kind": "spindle", "percent": 73 }),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while sim.lock().unwrap().overrides().spindle != 73 {
            assert!(Instant::now() < deadline, "the override didn't converge");
            std::thread::sleep(Duration::from_millis(20));
        }
        call(
            &window,
            "machine_override",
            json!({ "kind": "spindle", "percent": 100 }),
        )
        .unwrap();

        // Frame the same job, as a rubber band.
        let from = jobs.lock().unwrap().len();
        let mut framing = body(&here);
        framing["shape"] = json!("rubberBand");
        let name = call(&window, "machine_frame", framing).unwrap();
        assert_eq!(name, "Frame");
        let deadline = Instant::now() + Duration::from_secs(10);
        let framed = loop {
            let done = jobs.lock().unwrap()[from..]
                .iter()
                .find(|job| job["state"]["state"] != "running")
                .cloned();
            if let Some(job) = done {
                break job;
            }
            assert!(Instant::now() < deadline, "the frame didn't finish");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(framed["state"]["state"], "finished", "{framed}");
        assert_eq!(framed["name"], "Frame");

        call(&window, "machine_disconnect", json!({})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_mid_job_stops_the_machine_with_the_laser_off_first() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        use tauri::Manager;

        let (app, window) = main_window();
        // With nothing connected there is nothing to stop.
        call(&window, "machine_job_stop_wait", json!({})).unwrap();
        let (port, sim) = serve_shared_simulator(bf_sim::Profile::diode_laser());
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        // Let the first status report arrive, so the machine is known idle.
        std::thread::sleep(Duration::from_millis(300));
        let dir = std::env::temp_dir().join(format!("bf-ipc-close-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("burn.gcode");
        std::fs::write(&file, "G21 G90\nM8\nM4 S500\nG1 X100 F600\nG1 X0\n").unwrap();
        let burning = |sim: &Arc<Mutex<bf_sim::Simulator>>| {
            app.state::<machine::job::JobPicker>().pick(file.clone());
            call(&window, "machine_run_file", json!({})).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while sim.lock().unwrap().laser_power() == 0.0 {
                assert!(Instant::now() < deadline, "the laser never came on");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let lines = |sim: &Arc<Mutex<bf_sim::Simulator>>| {
            let sim = sim.lock().unwrap();
            let mut lines = String::new();
            for event in sim.events() {
                if let bf_sim::Event::Received { bytes, .. } = event
                    && bytes.ends_with(b"\n")
                {
                    lines.push_str(&String::from_utf8_lossy(bytes));
                }
            }
            lines
        };

        // The close guard's "Stop job and close": the window closes only
        // once this returns, with the laser and air assist off.
        burning(&sim);
        call(&window, "machine_job_stop_wait", json!({})).unwrap();
        assert_eq!(sim.lock().unwrap().laser_power(), 0.0);
        assert!(!sim.lock().unwrap().air_assist());
        assert!(lines(&sim).ends_with("M5\nM9\n"), "{}", lines(&sim));
        let disconnected = call(&window, "machine_disconnect", json!({}));
        assert_eq!(disconnected, Ok(Value::Null), "the job is over");

        // However else the app exits, its exit hook does the same before
        // closing the connection.
        let (port, sim) = serve_shared_simulator(bf_sim::Profile::diode_laser());
        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        burning(&sim);
        machine::shutdown(app.handle());
        assert_eq!(sim.lock().unwrap().laser_power(), 0.0);
        assert!(!sim.lock().unwrap().air_assist());
        assert!(lines(&sim).ends_with("M5\nM9\n"), "{}", lines(&sim));
        assert_eq!(
            call(&window, "machine_job_pause", json!({})),
            Err(json!("Connect to the machine first."))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn simulator_closing_its_socket_reports_a_disconnect() {
        let (_app, window) = main_window();
        // A server that sends the banner, then hangs up.
        let listener = bind_local();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::Write;
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .write_all(b"\r\nGrbl 1.1h ['$' for help]\r\n")
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
        });

        call(
            &window,
            "machine_connect",
            json!({ "port": tcp_port(port) }),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let state = loop {
            let state = call(&window, "machine_state", json!({})).unwrap();
            if state["status"] == "disconnected" || std::time::Instant::now() > deadline {
                break state;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(state["status"], "disconnected");
        assert!(
            state["reason"]
                .as_str()
                .unwrap()
                .starts_with("Connection lost"),
            "{state}"
        );
    }

    #[test]
    fn projects_save_reopen_identically_and_bad_files_fail_clearly() {
        use tauri::Manager;

        let (app, window) = main_window();
        let dir = std::env::temp_dir().join(format!("bf-ipc-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        app.state::<project::ProjectFiles>().use_dir(dir.clone());

        let project = json!({
            "formatVersion": 2,
            "machineProfile": { "id": "serial-com3", "name": "Phecda" },
            "document": { "version": 1, "units": "mm", "layers": [{
                "id": "C00", "name": "Cut", "color": "#ff0000", "mode": "line",
                "speed": 600.0, "maxPower": 90.0, "minPower": 5.0, "passes": 2,
                "constantPower": true, "airAssist": true, "output": true, "shown": false,
                "lineInterval": 0.1, "scanAngle": 0.0, "bidirectional": true, "overscan": 2.5,
                "fillGap": 1.0
            }], "objects": [{
                "type": "ellipse", "id": "e1", "name": "Ellipse", "layerId": "C00",
                "transform": [1.0, 0.0, 0.0, 1.0, 12.5, -3.25], "rx": 4.0, "ry": 2.5
            }] }
        });
        let path = dir.join("Coaster.bfp").display().to_string();
        let saved = call(
            &window,
            "project_save",
            json!({ "project": project, "path": path }),
        )
        .unwrap();
        assert_eq!(saved["name"], "Coaster");
        assert_eq!(saved["recent"][0]["path"], path);

        let opened = call(&window, "project_open", json!({ "path": path })).unwrap();
        assert_eq!(opened["project"], project);
        let recent = call(&window, "project_recent", json!({})).unwrap();
        assert_eq!(recent, json!([{ "path": path, "name": "Coaster" }]));

        std::fs::write(&path, "{ \"formatVersion\": 1, \"document\": 7 }").unwrap();
        let error = call(&window, "project_open", json!({ "path": path })).unwrap_err();
        assert!(
            error
                .as_str()
                .unwrap()
                .starts_with("This isn't a valid Beam Forge project"),
            "{error}"
        );
        let refused = call(&window, "project_open", json!({ "path": "C:/notes.txt" }));
        assert!(
            refused
                .unwrap_err()
                .as_str()
                .unwrap()
                .contains("isn't a Beam Forge project")
        );

        assert_eq!(
            call(&window, "project_recovery", json!({})).unwrap(),
            Value::Null
        );
        call(
            &window,
            "project_recovery_save",
            json!({ "project": project, "path": null }),
        )
        .unwrap();
        let recovery = call(&window, "project_recovery", json!({})).unwrap();
        assert_eq!(recovery["project"], project);
        assert_eq!(recovery["name"], "Untitled");
        call(&window, "project_recovery_clear", json!({})).unwrap();
        assert_eq!(
            call(&window, "project_recovery", json!({})).unwrap(),
            Value::Null
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
