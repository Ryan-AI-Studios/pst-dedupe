//! Dedupe Desk chrome host — Tauri 2 commands over matter-core overview.

mod create;
mod error;
mod matter_cmd;
mod params;
#[allow(dead_code)] // Host mirror of UI encode helpers; covered by unit tests in CI.
mod path_id;
mod recents;

use camino::Utf8PathBuf;
use create::create_matter_under;
use error::CommandError;
use matter_cmd::{matter_overview_blocking, MatterOverviewResponse};
use recents::{
    production_recents_dir, recent_matters_list_in, recent_matters_remember_in, RecentMatter,
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

/// Launch the Tauri app. Returns `Err` instead of panicking on run failure.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            matter_overview,
            create_matter,
            recent_matters_list,
            recent_matters_remember
        ])
        .run(tauri::generate_context!())
        .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
}
