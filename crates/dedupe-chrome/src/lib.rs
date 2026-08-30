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
mod queue;
#[allow(dead_code)] // Pure helper mirrored in UI; exercised by unit tests in CI.
mod queue_window;
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
use queue::{review_queue_page_blocking, ReviewQueuePageArgs};
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            review_upsert_privilege
        ])
        .run(tauri::generate_context!())
        .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
}
