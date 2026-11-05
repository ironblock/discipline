fn main() {
    // The app's own commands are named, so each is granted by a capability
    // (`capabilities/default.json`) rather than open to every window.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["drive_authorization", "notify"]),
    ))
    .expect("the shell's build script");
}
