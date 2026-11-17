fn main() {
    // Embed the manifest via the linker for both the app and Cargo's test harness,
    // and skip the resource copy so the release executable gets only one manifest.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("Unable to generate Tauri build resources");
    // Tauri's IPC test runtime links TaskDialogIndirect, which needs Common Controls v6;
    // without it the lib-test executable fails with STATUS_ENTRYPOINT_NOT_FOUND.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
