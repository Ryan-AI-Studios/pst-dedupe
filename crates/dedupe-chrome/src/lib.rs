//! Dedupe Desk chrome host — Tauri 2 commands over matter-core overview + review queue.

mod body;
mod codes;
mod create;
mod document;
mod error;
mod html_strip;
mod matter_cmd;
mod notes;
mod open_root;
mod params;
#[allow(dead_code)] // Host mirror of UI encode helpers; covered by unit tests in CI.
mod path_id;
mod privilege_cmd;
mod process;
mod process_params;
mod produce;
mod queue;
#[allow(dead_code)] // Pure helper mirrored in UI; exercised by unit tests in CI.
mod queue_window;
mod raster;
mod recents;
mod saved;

use body::{review_document_body_blocking, ReviewDocumentBodyArgs};
use camino::Utf8PathBuf;
use codes::{
    review_apply_codes_blocking, review_code_catalog_blocking, review_codes_preview_blocking,
    review_window_apply_blocking, ReviewApplyCodesArgs, ReviewCodesPreviewArgs,
    ReviewWindowApplyArgs, RootOnlyArgs,
};
use create::create_matter_under;
use document::{review_document_blocking, ReviewDocumentArgs};
use error::CommandError;
use matter_cmd::{matter_overview_blocking, MatterOverviewResponse};
use notes::{review_upsert_note_blocking, ReviewUpsertNoteArgs};
use privilege_cmd::{review_upsert_privilege_blocking, ReviewUpsertPrivilegeArgs};
use process_runner::ProcessRunner;
use produce::{
    produce_page_blocking, produce_qc_findings_blocking, produce_qc_run_blocking,
    produce_start_blocking, ProduceQcRunArgs, ProduceStartArgs,
};
use queue::{review_queue_page_blocking, ReviewQueuePageArgs};
use raster::{
    produce_burn_set_blocking, review_burn_native_blocking, review_geom_delete_blocking,
    review_geom_from_hits_blocking, review_geom_list_blocking, review_geom_upsert_blocking,
    review_raster_page_blocking, ProduceBurnSetArgs, ReviewBurnNativeArgs, ReviewGeomDeleteArgs,
    ReviewGeomFromHitsArgs, ReviewGeomListArgs, ReviewGeomUpsertArgs, ReviewRasterPageArgs,
};
use recents::{
    production_recents_dir, recent_matters_list_in, recent_matters_remember_in, RecentMatter,
};
use saved::{
    saved_search_upsert_blocking, saved_searches_list_blocking, SavedSearchUpsertArgs,
    SavedSearchesListArgs,
};

fn join_worker<T>(
    label: &str,
    handle: std::thread::JoinHandle<Result<T, CommandError>>,
) -> Result<T, CommandError> {
    match handle.join() {
        Ok(inner) => inner,
        Err(_) => Err(CommandError::failed(format!("{label} worker panicked"))),
    }
}

#[tauri::command]
fn matter_overview(root: String) -> Result<MatterOverviewResponse, CommandError> {
    // Never run SQLite on the WebView thread — same contract as Desk OverviewLoadState.
    join_worker(
        "overview",
        std::thread::spawn(move || matter_overview_blocking(&root)),
    )
}

#[tauri::command]
fn create_matter(parent: String, name: String) -> Result<String, CommandError> {
    join_worker(
        "create",
        std::thread::spawn(move || {
            let parent = Utf8PathBuf::from(parent);
            let root = create_matter_under(&parent, &name)?;
            Ok(root.into_string())
        }),
    )
}

#[tauri::command]
fn recent_matters_list() -> Result<Vec<RecentMatter>, CommandError> {
    let dir = production_recents_dir()?;
    recent_matters_list_in(&dir)
}

#[tauri::command]
fn recent_matters_remember(root: String, name: String) -> Result<Vec<RecentMatter>, CommandError> {
    let dir = production_recents_dir()?;
    recent_matters_remember_in(&dir, &root, &name)
}

#[tauri::command]
fn review_queue_page(
    root: String,
    filter_json: Option<String>,
    keyword: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
    extras: Option<bool>,
) -> Result<queue::ReviewQueuePageResponse, CommandError> {
    join_worker(
        "review_queue_page",
        std::thread::spawn(move || {
            review_queue_page_blocking(ReviewQueuePageArgs {
                root,
                filter_json,
                keyword,
                limit,
                offset,
                extras,
            })
        }),
    )
}

#[tauri::command]
fn review_code_catalog(root: String) -> Result<Vec<codes::CodeCatalogEntry>, CommandError> {
    join_worker(
        "review_code_catalog",
        std::thread::spawn(move || review_code_catalog_blocking(RootOnlyArgs { root })),
    )
}

#[tauri::command]
fn saved_searches_list(root: String) -> Result<Vec<saved::SavedSearchDto>, CommandError> {
    join_worker(
        "saved_searches_list",
        std::thread::spawn(move || saved_searches_list_blocking(SavedSearchesListArgs { root })),
    )
}

#[tauri::command]
fn saved_search_upsert(
    root: String,
    name: String,
    filter_json: String,
    keyword: Option<String>,
    description: Option<String>,
    id: Option<String>,
) -> Result<saved::SavedSearchDto, CommandError> {
    join_worker(
        "saved_search_upsert",
        std::thread::spawn(move || {
            saved_search_upsert_blocking(SavedSearchUpsertArgs {
                root,
                name,
                filter_json,
                keyword,
                description,
                id,
            })
        }),
    )
}

#[tauri::command]
fn review_codes_preview(
    root: String,
    item_ids: Vec<String>,
    add_code_ids: Vec<String>,
    remove_code_ids: Vec<String>,
) -> Result<codes::ReviewCodesPreviewResponse, CommandError> {
    join_worker(
        "review_codes_preview",
        std::thread::spawn(move || {
            review_codes_preview_blocking(ReviewCodesPreviewArgs {
                root,
                item_ids,
                add_code_ids,
                remove_code_ids,
            })
        }),
    )
}

#[tauri::command]
fn review_apply_codes(
    root: String,
    item_ids: Vec<String>,
    add_code_ids: Vec<String>,
    remove_code_ids: Vec<String>,
    propagate_family: Option<bool>,
) -> Result<matter_core::ApplyCodesResult, CommandError> {
    join_worker(
        "review_apply_codes",
        std::thread::spawn(move || {
            review_apply_codes_blocking(ReviewApplyCodesArgs {
                root,
                item_ids,
                add_code_ids,
                remove_code_ids,
                propagate_family,
            })
        }),
    )
}

#[tauri::command]
fn review_document(
    root: String,
    item_id: String,
    filter_json: Option<String>,
    keyword: Option<String>,
) -> Result<document::ReviewDocumentResponse, CommandError> {
    join_worker(
        "review_document",
        std::thread::spawn(move || {
            review_document_blocking(ReviewDocumentArgs {
                root,
                item_id,
                filter_json,
                keyword,
            })
        }),
    )
}

#[tauri::command]
fn review_document_body(
    root: String,
    item_id: String,
    pane: String,
) -> Result<body::ReviewDocumentBodyResponse, CommandError> {
    join_worker(
        "review_document_body",
        std::thread::spawn(move || {
            review_document_body_blocking(ReviewDocumentBodyArgs {
                root,
                item_id,
                pane,
            })
        }),
    )
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn review_window_apply(
    root: String,
    item_ids: Vec<String>,
    add_code_ids: Vec<String>,
    remove_code_ids: Vec<String>,
    propagate_family: Option<bool>,
    privilege_basis: Option<String>,
    withhold: Option<bool>,
    include_on_log: Option<bool>,
    privilege_description: Option<String>,
) -> Result<matter_core::ApplyCodesResult, CommandError> {
    join_worker(
        "review_window_apply",
        std::thread::spawn(move || {
            review_window_apply_blocking(ReviewWindowApplyArgs {
                root,
                item_ids,
                add_code_ids,
                remove_code_ids,
                propagate_family,
                privilege_basis,
                withhold,
                include_on_log,
                privilege_description,
            })
        }),
    )
}

#[tauri::command]
fn review_upsert_note(
    root: String,
    item_id: String,
    body: String,
    id: Option<String>,
) -> Result<matter_core::ItemNote, CommandError> {
    join_worker(
        "review_upsert_note",
        std::thread::spawn(move || {
            review_upsert_note_blocking(ReviewUpsertNoteArgs {
                root,
                item_id,
                body,
                id,
            })
        }),
    )
}

#[tauri::command]
fn review_raster_page(
    root: String,
    item_id: String,
    page_index: Option<u32>,
    dpi: Option<u32>,
    generation: Option<u64>,
) -> Result<raster::ReviewRasterPageResponse, CommandError> {
    join_worker(
        "review_raster_page",
        std::thread::spawn(move || {
            review_raster_page_blocking(ReviewRasterPageArgs {
                root,
                item_id,
                page_index,
                dpi,
                generation,
            })
        }),
    )
}

#[tauri::command]
fn review_geom_list(
    root: String,
    item_id: String,
    generation: Option<u64>,
) -> Result<raster::ReviewGeomListResponse, CommandError> {
    join_worker(
        "review_geom_list",
        std::thread::spawn(move || {
            review_geom_list_blocking(ReviewGeomListArgs {
                root,
                item_id,
                generation,
            })
        }),
    )
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn review_geom_upsert(
    root: String,
    item_id: String,
    page_index: u32,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    raster_width: f64,
    raster_height: f64,
    reason: Option<String>,
    label: Option<String>,
    source: Option<String>,
    generation: Option<u64>,
) -> Result<raster::ReviewGeomUpsertResponse, CommandError> {
    join_worker(
        "review_geom_upsert",
        std::thread::spawn(move || {
            review_geom_upsert_blocking(ReviewGeomUpsertArgs {
                root,
                item_id,
                page_index,
                px,
                py,
                pw,
                ph,
                raster_width,
                raster_height,
                reason,
                label,
                source,
                generation,
            })
        }),
    )
}

#[tauri::command]
fn review_geom_delete(root: String, geom_id: String) -> Result<(), CommandError> {
    join_worker(
        "review_geom_delete",
        std::thread::spawn(move || {
            review_geom_delete_blocking(ReviewGeomDeleteArgs { root, geom_id })
        }),
    )
}

#[tauri::command]
fn review_geom_from_hits(
    root: String,
    item_id: String,
    query: Option<String>,
    reason: Option<String>,
    generation: Option<u64>,
) -> Result<raster::ReviewGeomFromHitsResponse, CommandError> {
    join_worker(
        "review_geom_from_hits",
        std::thread::spawn(move || {
            review_geom_from_hits_blocking(ReviewGeomFromHitsArgs {
                root,
                item_id,
                query,
                reason,
                generation,
            })
        }),
    )
}

#[tauri::command]
fn review_burn_native(
    root: String,
    item_id: String,
) -> Result<raster::ReviewBurnNativeResponse, CommandError> {
    join_worker(
        "review_burn_native",
        std::thread::spawn(move || {
            review_burn_native_blocking(ReviewBurnNativeArgs { root, item_id })
        }),
    )
}

#[tauri::command]
fn produce_burn_set(
    root: String,
    item_ids: Option<Vec<String>>,
) -> Result<raster::ProduceBurnSetResponse, CommandError> {
    join_worker(
        "produce_burn_set",
        std::thread::spawn(move || {
            produce_burn_set_blocking(ProduceBurnSetArgs { root, item_ids })
        }),
    )
}

#[tauri::command]
fn produce_page(root: String) -> Result<produce::ProducePageResponse, CommandError> {
    join_worker(
        "produce_page",
        std::thread::spawn(move || produce_page_blocking(&root)),
    )
}

#[tauri::command]
fn produce_qc_run(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    root: String,
    filter_json: Option<String>,
    item_ids: Option<Vec<String>>,
    production_profile: Option<String>,
    source_entire_corpus: Option<bool>,
) -> Result<produce::ProduceQcRunResponse, CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "produce_qc_run",
        std::thread::spawn(move || {
            produce_qc_run_blocking(
                &runner,
                ProduceQcRunArgs {
                    root,
                    filter_json,
                    item_ids,
                    production_profile,
                    source_entire_corpus,
                },
            )
        }),
    )
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn produce_start(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    root: String,
    filter_json: Option<String>,
    item_ids: Option<Vec<String>>,
    production_profile: Option<String>,
    source_entire_corpus: Option<bool>,
    bates_prefix: Option<String>,
    bates_start: Option<u64>,
    warning_overrides: Option<Vec<produce::WarningOverride>>,
    log_format: Option<String>,
    last_findings: Option<Vec<produce::ChromeQcFinding>>,
) -> Result<produce::ProduceStartResponse, CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "produce_start",
        std::thread::spawn(move || {
            produce_start_blocking(
                &runner,
                ProduceStartArgs {
                    root,
                    filter_json,
                    item_ids,
                    production_profile,
                    source_entire_corpus,
                    bates_prefix,
                    bates_start,
                    warning_overrides,
                    log_format,
                    last_findings,
                },
            )
        }),
    )
}

#[tauri::command]
fn produce_qc_findings(
    root: String,
    job_id: Option<String>,
) -> Result<produce::ProduceQcRunResponse, CommandError> {
    join_worker(
        "produce_qc_findings",
        std::thread::spawn(move || produce_qc_findings_blocking(&root, job_id)),
    )
}

#[tauri::command]
fn process_page(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    root: String,
) -> Result<process::ProcessPageResponse, CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "process_page",
        std::thread::spawn(move || process::process_page_blocking(&runner, &root)),
    )
}

#[tauri::command]
fn process_start(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    root: String,
    kind: String,
    params_json: String,
) -> Result<process::ProcessStartResponse, CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "process_start",
        std::thread::spawn(move || {
            process::process_start_blocking(&runner, &root, &kind, &params_json)
        }),
    )
}

#[tauri::command]
fn process_progress(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    root: String,
) -> Result<process_runner::JobProgressSnapshot, CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "process_progress",
        std::thread::spawn(move || process::process_progress_blocking(&runner, &root)),
    )
}

#[tauri::command]
fn process_cancel(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    job_id: String,
) -> Result<(), CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "process_cancel",
        std::thread::spawn(move || process::process_cancel_blocking(&runner, &job_id)),
    )
}

#[tauri::command]
fn process_resume(
    runner: tauri::State<std::sync::Arc<ProcessRunner>>,
    root: String,
    job_id: String,
) -> Result<(), CommandError> {
    let runner = runner.inner().clone();
    join_worker(
        "process_resume",
        std::thread::spawn(move || process::process_resume_blocking(&runner, &root, &job_id)),
    )
}

#[tauri::command]
fn review_upsert_privilege(
    root: String,
    item_id: String,
    basis: String,
    withhold: Option<bool>,
    description: Option<String>,
) -> Result<matter_core::ItemPrivilege, CommandError> {
    join_worker(
        "review_upsert_privilege",
        std::thread::spawn(move || {
            review_upsert_privilege_blocking(ReviewUpsertPrivilegeArgs {
                root,
                item_id,
                basis,
                withhold,
                description,
            })
        }),
    )
}

/// Launch the Tauri app. Returns `Err` instead of panicking on run failure.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runner = process::new_managed_runner();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(runner)
        .invoke_handler(tauri::generate_handler![
            matter_overview,
            create_matter,
            recent_matters_list,
            recent_matters_remember,
            review_queue_page,
            review_code_catalog,
            saved_searches_list,
            saved_search_upsert,
            review_codes_preview,
            review_apply_codes,
            review_document,
            review_document_body,
            review_window_apply,
            review_upsert_note,
            review_upsert_privilege,
            produce_page,
            produce_qc_run,
            produce_start,
            produce_qc_findings,
            process_page,
            process_start,
            process_progress,
            process_cancel,
            process_resume,
            review_raster_page,
            review_geom_list,
            review_geom_upsert,
            review_geom_delete,
            review_geom_from_hits,
            review_burn_native,
            produce_burn_set
        ])
        .build(tauri::generate_context!())?
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                app_handle
                    .state::<std::sync::Arc<ProcessRunner>>()
                    .shutdown();
            }
        });
    Ok(())
}
