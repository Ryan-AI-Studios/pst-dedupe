//! PST structure inspection (folders / counts).

use std::path::Path;

use pst_reader::PstFile;
use serde::Serialize;

use crate::error::{CliError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct FolderRow {
    pub path: String,
    pub messages: u32,
    pub children: usize,
    pub nid: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectReport {
    pub path: String,
    pub file_size: u64,
    pub crypt: String,
    pub folders: u64,
    pub total_messages: u64,
    pub folder_rows: Vec<FolderRow>,
}

/// Open a PST and list folders with message counts.
pub fn inspect_pst(path: &Path, max_folders: Option<usize>) -> Result<InspectReport> {
    if !path.exists() {
        return Err(CliError::PathNotFound(path.to_path_buf()));
    }

    let mut pst = PstFile::open(path).map_err(|source| CliError::PstOpen {
        path: path.to_path_buf(),
        source,
    })?;

    let crypt = format!("{:?}", pst.crypt_method());
    let file_size = pst.file_size();

    let folders = pst.folders().map_err(|source| CliError::Folders {
        path: path.to_path_buf(),
        source,
    })?;

    let total_messages: u64 = folders.iter().map(|f| f.message_count as u64).sum();
    let folder_count = folders.len() as u64;

    let mut folder_rows: Vec<FolderRow> = folders
        .iter()
        .map(|f| FolderRow {
            path: f.path.clone(),
            messages: f.message_count,
            children: f.child_folder_nids.len(),
            nid: f.nid.0,
        })
        .collect();

    // Prefer folders that actually have mail when truncating.
    folder_rows.sort_by(|a, b| b.messages.cmp(&a.messages).then(a.path.cmp(&b.path)));
    if let Some(max) = max_folders {
        folder_rows.truncate(max);
    }

    Ok(InspectReport {
        path: path.display().to_string(),
        file_size,
        crypt,
        folders: folder_count,
        total_messages,
        folder_rows,
    })
}
