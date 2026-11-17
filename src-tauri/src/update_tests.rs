use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

#[test]
fn updater_plugins_initialize_with_production_configuration() {
    mock_builder()
        .plugin(crate::updater_plugin())
        .plugin(tauri_plugin_process::init())
        .build(tauri::generate_context!())
        .expect("Updater and process plugins must initialize in normal builds");
}

#[test]
fn updater_builds_with_the_release_configuration() {
    let configuration = std::fs::read_to_string("tauri.release.conf.json").unwrap();
    let configuration: serde_json::Value = serde_json::from_str(&configuration).unwrap();
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".to_string(),
        configuration["plugins"]["updater"].clone(),
    );
    let app = mock_builder()
        .plugin(crate::updater_plugin())
        .build(context)
        .unwrap();
    app.updater()
        .expect("The release public key and HTTPS endpoint must be accepted");
}

#[test]
#[ignore = "Requires updates:setup, a published newer signed package, and updates:serve"]
fn local_signed_update_downloads_and_rejects_tampering() {
    let configuration = std::fs::read_to_string("../.local/tauri.updater.json").unwrap();
    let configuration: serde_json::Value = serde_json::from_str(&configuration).unwrap();
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".to_string(),
        configuration["plugins"]["updater"].clone(),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    tauri::async_runtime::block_on(async {
        let update = app
            .updater_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap()
            .check()
            .await
            .unwrap()
            .expect("Publish a version newer than the version under test");
        let mut bytes = update.download(|_, _| {}, || {}).await.unwrap();
        assert!(!bytes.is_empty());
        update.verify(&bytes).unwrap();
        bytes[0] ^= 1;
        assert!(
            update.verify(&bytes).is_err(),
            "Tampered packages must fail"
        );
    });
}
