fn main() {
    let attrs =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "matter_overview",
            "create_matter",
            "recent_matters_list",
            "recent_matters_remember",
            "review_queue_page",
            "review_code_catalog",
            "saved_searches_list",
            "saved_search_upsert",
            "review_codes_preview",
            "review_apply_codes",
        ]));
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("tauri-build failed: {e}");
    }
}
