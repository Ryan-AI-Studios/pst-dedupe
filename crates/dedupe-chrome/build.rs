fn main() {
    let attrs =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "matter_overview",
            "create_matter",
            "recent_matters_list",
            "recent_matters_remember",
        ]));
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("tauri-build failed: {e}");
    }
}
