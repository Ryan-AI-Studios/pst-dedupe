//! MRU recent matters list (paths + display name only; injectable dir for tests).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CommandError;

pub const MAX_RECENTS: usize = 20;
const FILE_NAME: &str = "recents.json";
const APP_DIR: &str = "com.dedupe.desk.chrome";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentMatter {
    pub root: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RecentsFile {
    #[serde(default)]
    matters: Vec<RecentMatter>,
}

/// Production app-data directory for recents. Never falls back to temp.
pub fn production_recents_dir() -> Result<PathBuf, CommandError> {
    dirs::data_local_dir()
        .map(|p| p.join(APP_DIR))
        .ok_or_else(|| {
            CommandError::failed(
                "Local app data directory is unavailable; cannot store recent matters.",
            )
        })
}

fn normalize_matters(mut matters: Vec<RecentMatter>) -> Vec<RecentMatter> {
    if matters.len() > MAX_RECENTS {
        matters.truncate(MAX_RECENTS);
    }
    matters
}

pub fn recent_matters_list_in(dir: &Path) -> Result<Vec<RecentMatter>, CommandError> {
    let path = dir.join(FILE_NAME);
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let file: RecentsFile =
                serde_json::from_str(&raw).map_err(|e| CommandError::failed(e.to_string()))?;
            Ok(normalize_matters(file.matters))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CommandError::failed(e.to_string())),
    }
}

pub fn recent_matters_remember_in(
    dir: &Path,
    root: &str,
    name: &str,
) -> Result<Vec<RecentMatter>, CommandError> {
    let mut matters = recent_matters_list_in(dir)?;
    matters.retain(|m| m.root != root);
    matters.insert(
        0,
        RecentMatter {
            root: root.to_string(),
            name: name.to_string(),
        },
    );
    let matters = normalize_matters(matters);
    fs::create_dir_all(dir).map_err(|e| CommandError::failed(e.to_string()))?;
    let path = dir.join(FILE_NAME);
    let raw = serde_json::to_string_pretty(&RecentsFile {
        matters: matters.clone(),
    })
    .map_err(|e| CommandError::failed(e.to_string()))?;
    fs::write(&path, raw).map_err(|e| CommandError::failed(e.to_string()))?;
    Ok(matters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mru_front_cap_20_and_inject_dir() {
        let tmp = tempdir().expect("tempdir");
        let dir = tmp.path();
        // Production LocalAppData must not be touched — only `dir`.
        for i in 0..25 {
            recent_matters_remember_in(dir, &format!("root-{i}"), &format!("n{i}")).expect("rem");
        }
        let list = recent_matters_list_in(dir).expect("list");
        assert_eq!(list.len(), MAX_RECENTS);
        assert_eq!(list[0].root, "root-24");
        assert_eq!(list[19].root, "root-5");
        // Spec: missing roots remain so overview can return not_found (no silent drop).
        assert!(!Path::new("root-24").exists());
        assert!(
            list.iter().any(|m| m.root == "root-24"),
            "MRU entry must remain even when the path is absent on disk"
        );
        recent_matters_remember_in(dir, "root-10", "n10-again").expect("promote");
        let list = recent_matters_list_in(dir).expect("list2");
        assert_eq!(list[0].root, "root-10");
        assert_eq!(list.iter().filter(|m| m.root == "root-10").count(), 1);
        assert_eq!(list.len(), MAX_RECENTS);
    }

    #[test]
    fn list_truncates_oversized_file_on_load() {
        let tmp = tempdir().expect("tempdir");
        let dir = tmp.path();
        let matters: Vec<RecentMatter> = (0..25)
            .map(|i| RecentMatter {
                root: format!("root-{i}"),
                name: format!("n{i}"),
            })
            .collect();
        let raw = serde_json::to_string_pretty(&RecentsFile { matters }).expect("json");
        fs::write(dir.join(FILE_NAME), raw).expect("write");
        let list = recent_matters_list_in(dir).expect("list");
        assert_eq!(list.len(), MAX_RECENTS);
        assert_eq!(list[0].root, "root-0");
        assert_eq!(list[19].root, "root-19");
        assert!(!list.iter().any(|m| m.root == "root-24"));
    }

    #[test]
    fn production_dir_is_result_not_temp_fallback() {
        // API is Result — callers must handle Err; no unwrap_or(temp_dir).
        match production_recents_dir() {
            Ok(p) => {
                let s = p.to_string_lossy();
                assert!(
                    s.contains(APP_DIR),
                    "production path must use app id dir: {s}"
                );
            }
            Err(e) => {
                assert_eq!(e.kind, "failed");
                assert!(e.message.to_lowercase().contains("local app data"));
            }
        }
    }
}
