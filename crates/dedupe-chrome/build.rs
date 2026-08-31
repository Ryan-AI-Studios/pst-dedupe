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
            "review_document",
            "review_document_body",
            "review_window_apply",
            "review_upsert_note",
            "review_upsert_privilege",
            "produce_page",
            "produce_qc_run",
            "produce_start",
            "produce_qc_findings",
            "process_page",
            "process_start",
            "process_progress",
            "process_cancel",
            "process_resume",
            "review_raster_page",
            "review_geom_list",
            "review_geom_upsert",
            "review_geom_delete",
            "review_geom_from_hits",
            "review_burn_native",
            "produce_burn_set",
        ]));
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("tauri-build failed: {e}");
    }
}
