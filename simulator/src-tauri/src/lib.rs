//! Beam Forge Simulator: one `bf-sim` GRBL controller on a real-time clock,
//! reachable over a TCP "fake USB" link. Tauri commands here are thin; the
//! clock is in [`machine`] and the link in [`link`]. [`launch`] covers running
//! as Beam Forge's built-in simulator.

pub mod burn;
pub mod inspector;
pub mod launch;
pub mod link;
pub mod machine;

use burn::BurnBatch;
use inspector::InspectorBatch;
use link::{DEFAULT_PORT, Host, HostSnapshot};
use machine::{Preset, PresetInfo, SettingRow, Speed};
use tauri::{AppHandle, Manager, Runtime, State};

/// `BF_SIM_PORT` overrides the default port, e.g. to run two simulators.
fn initial_port() -> u16 {
    std::env::var("BF_SIM_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[tauri::command]
fn snapshot(host: State<'_, Host>) -> HostSnapshot {
    host.snapshot()
}

#[tauri::command]
fn inspector(host: State<'_, Host>, since: u64) -> InspectorBatch {
    host.inspector(since)
}

/// Writes a new file in Downloads named for the time; returns its path.
fn save_download<R: Runtime>(
    app: &AppHandle<R>,
    extension: &str,
    contents: &[u8],
) -> Result<String, String> {
    let folder = app.path().download_dir().map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let path = folder.join(format!("beam-forge-simulator-{stamp}.{extension}"));
    std::fs::write(&path, contents)
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

/// Saves the inspector log as shown to a new file in Downloads; returns its path.
#[tauri::command]
fn export_log<R: Runtime>(app: AppHandle<R>, text: String) -> Result<String, String> {
    save_download(&app, "log", text.as_bytes())
}

/// Saves the burn view as a PNG in Downloads; returns its path.
#[tauri::command]
fn export_png<R: Runtime>(app: AppHandle<R>, png: Vec<u8>) -> Result<String, String> {
    if !png.starts_with(b"\x89PNG") {
        return Err("Not a PNG image".into());
    }
    save_download(&app, "png", &png)
}

#[tauri::command]
fn burns(host: State<'_, Host>, since: u32) -> BurnBatch {
    host.with_machine(|machine, _| machine.burns(since))
}

#[tauri::command]
fn clear_burns(host: State<'_, Host>) {
    host.with_machine(|machine, _| machine.clear_burns());
}

#[tauri::command]
fn listen(host: State<'_, Host>, port: u16) -> Result<String, String> {
    host.listen(port).map(|address| address.to_string())
}

#[tauri::command]
fn set_speed(host: State<'_, Host>, speed: Speed) {
    host.with_machine(|machine, now| machine.set_speed(speed, now));
}

#[tauri::command]
fn reset_machine(host: State<'_, Host>) {
    host.with_machine(|machine, now| machine.power_up(now));
}

#[tauri::command]
fn settings(host: State<'_, Host>) -> Vec<SettingRow> {
    host.with_machine(|machine, _| machine.settings())
}

#[tauri::command]
fn set_setting(host: State<'_, Host>, id: u16, value: f64) -> Result<(), String> {
    host.with_machine(|machine, now| machine.set_setting(id, value, now))
}

/// Injects one fault, in `bf_sim`'s text form (e.g. `alarm 1`).
#[tauri::command]
fn inject_fault(host: State<'_, Host>, fault: String) -> Result<(), String> {
    host.with_machine(|machine, _| machine.inject(&fault))
}

/// Starts a scenario script; its lines count from now.
#[tauri::command]
fn run_scenario(host: State<'_, Host>, script: String) -> Result<(), String> {
    host.with_machine(|machine, _| machine.run_scenario(&script))
}

#[tauri::command]
fn clear_faults(host: State<'_, Host>) {
    host.with_machine(|machine, _| machine.clear_faults());
}

#[tauri::command]
fn presets() -> Vec<PresetInfo> {
    machine::presets()
}

#[tauri::command]
fn load_preset(host: State<'_, Host>, preset: Preset) {
    host.with_machine(|machine, now| machine.load_preset(preset, now));
}

macro_rules! handlers {
    () => {
        tauri::generate_handler![
            snapshot,
            inspector,
            export_log,
            export_png,
            burns,
            clear_burns,
            listen,
            set_speed,
            reset_machine,
            settings,
            set_setting,
            presets,
            load_preset,
            inject_fault,
            run_scenario,
            clear_faults
        ]
    };
}

pub fn run() {
    let launched = launch::launched_by_app(std::env::args().skip(1));
    // Launched by the app: a free port, so it never clashes with a simulator
    // the user started on the default port.
    let port = if launched { 0 } else { initial_port() };
    let host = Host::start(Preset::DiodeLaser.profile(), port);
    if launched {
        // If the app is already gone, stdin is closed too and setup exits.
        let _ = launch::announce(&host, &mut std::io::stdout().lock());
    }
    tauri::Builder::default()
        .manage(host)
        .setup(move |app| {
            if launched {
                let app = app.handle().clone();
                std::thread::Builder::new()
                    .name("sim-stdin".into())
                    .spawn(move || launch::wait_for_close(std::io::stdin(), move || app.exit(0)))?;
            }
            Ok(())
        })
        .invoke_handler(handlers!())
        .run(tauri::generate_context!())
        .expect("Unable to start Beam Forge Simulator");
}

#[cfg(test)]
mod ipc_tests {
    use super::*;
    use serde_json::{Value, json};
    use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};

    // Loads the real tauri.conf.json and capabilities, so a missing permission
    // fails here rather than silently in the shipped app.
    fn main_window() -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
        let app = mock_builder()
            .manage(Host::start(Preset::DiodeLaser.profile(), 0))
            .invoke_handler(handlers!())
            .build(tauri::generate_context!())
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        (app, window)
    }

    fn invoke(
        window: &tauri::WebviewWindow<MockRuntime>,
        cmd: &str,
        body: Value,
    ) -> Result<Value, Value> {
        let request = tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: window.url().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        get_ipc_response(window, request).map(|body| body.deserialize::<Value>().unwrap())
    }

    #[test]
    fn snapshot_reports_the_link_and_machine() {
        let (_app, window) = main_window();
        let snapshot = invoke(&window, "snapshot", json!({})).unwrap();
        assert!(
            snapshot["link"]["address"]
                .as_str()
                .unwrap()
                .starts_with("127.0.0.1:")
        );
        assert_eq!(snapshot["link"]["client"], Value::Null);
        assert_eq!(snapshot["machine"]["state"], "Idle");
        assert_eq!(snapshot["machine"]["speed"], "x1");
    }

    #[test]
    fn settings_presets_and_speed_round_trip() {
        let (_app, window) = main_window();
        invoke(&window, "set_setting", json!({ "id": 110, "value": 900 })).unwrap();
        let settings = invoke(&window, "settings", json!({})).unwrap();
        let x_rate = settings
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == 110)
            .unwrap();
        assert_eq!(x_rate["text"], "900.000");
        let rejected = invoke(&window, "set_setting", json!({ "id": 7, "value": 1 }));
        assert_eq!(rejected, Err(json!("$7=1 rejected with error:3")));

        invoke(&window, "load_preset", json!({ "preset": "grblDefaults" })).unwrap();
        invoke(&window, "set_speed", json!({ "speed": "instant" })).unwrap();
        invoke(&window, "reset_machine", json!({})).unwrap();
        let snapshot = invoke(&window, "snapshot", json!({})).unwrap();
        assert_eq!(snapshot["machine"]["profile"], "GRBL 1.1 defaults");
        assert_eq!(snapshot["machine"]["speed"], "instant");
        let presets = invoke(&window, "presets", json!({})).unwrap();
        assert_eq!(presets[0]["id"], "diodeLaser");
    }

    #[test]
    fn inspector_returns_the_wire_log() {
        let (_app, window) = main_window();
        invoke(&window, "reset_machine", json!({})).unwrap();
        let batch = invoke(&window, "inspector", json!({ "since": 0 })).unwrap();
        let events = batch["events"].as_array().unwrap();
        assert!(events.iter().any(|e| e["event"]["kind"] == "sent"));
        assert!(events.iter().any(|e| e["event"]["text"] == "Power-cycled"));
        assert_eq!(batch["rxCapacity"], 128);
        let later = invoke(&window, "inspector", json!({ "since": batch["next"] })).unwrap();
        assert_eq!(later["events"], json!([]));
    }

    #[test]
    fn burns_are_fetched_and_cleared() {
        let (_app, window) = main_window();
        let batch = invoke(&window, "burns", json!({ "since": 0 })).unwrap();
        assert_eq!(batch["segments"], json!([]));
        invoke(&window, "clear_burns", json!({})).unwrap();
        let cleared = invoke(&window, "burns", json!({ "since": 0 })).unwrap();
        assert_ne!(cleared["generation"], batch["generation"]);
        let rejected = invoke(&window, "export_png", json!({ "png": [1, 2, 3] }));
        assert_eq!(rejected, Err(json!("Not a PNG image")));
    }

    #[test]
    fn faults_and_scenarios_are_injected() {
        let (_app, window) = main_window();
        invoke(&window, "inject_fault", json!({ "fault": "door open" })).unwrap();
        let snapshot = invoke(&window, "snapshot", json!({})).unwrap();
        assert_eq!(snapshot["machine"]["state"], "Door:1");
        let rejected = invoke(&window, "inject_fault", json!({ "fault": "door ajar" }));
        assert_eq!(rejected, Err(json!("unknown fault \"door ajar\"")));

        let script = "after 2: alarm 1\nafter 3: disconnect";
        invoke(&window, "run_scenario", json!({ "script": script })).unwrap();
        let snapshot = invoke(&window, "snapshot", json!({})).unwrap();
        assert_eq!(snapshot["machine"]["scenarioPending"], 2);
        invoke(&window, "clear_faults", json!({})).unwrap();
        let snapshot = invoke(&window, "snapshot", json!({})).unwrap();
        assert_eq!(snapshot["machine"]["scenarioPending"], 0);
        let batch = invoke(&window, "inspector", json!({ "since": 0 })).unwrap();
        let events = batch["events"].as_array().unwrap();
        assert!(
            events
                .iter()
                .any(|e| e["event"] == json!({ "kind": "fault", "text": "door open" }))
        );
    }

    #[test]
    fn listen_moves_to_another_port() {
        let (_app, window) = main_window();
        let address = invoke(&window, "listen", json!({ "port": 0 })).unwrap();
        let snapshot = invoke(&window, "snapshot", json!({})).unwrap();
        assert_eq!(snapshot["link"]["address"], address);
    }
}
