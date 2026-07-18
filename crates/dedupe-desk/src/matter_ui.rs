//! Create / open matter helpers (short, non-blocking for empty open; errors to UI).

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::Matter;

use crate::params::validate_matter_name;

/// Create a new matter under `parent / name`.
pub fn create_matter(parent: &Utf8Path, name: &str) -> Result<Utf8PathBuf, String> {
    let name = validate_matter_name(name)?;
    let root = parent.join(name);
    Matter::create(&root, name).map_err(|e| e.to_string())?;
    Ok(root)
}

/// Open an existing matter root; returns matter display name on success.
///
/// When `cleanup_temp` is true, uses [`Matter::open`] (wipes orphaned
/// `workspace/temp/`). Only safe when **no** process-runner job is writing.
/// When false, uses [`Matter::open_for_read`] (no temp wipe).
pub fn open_matter(root: &Utf8Path, cleanup_temp: bool) -> Result<String, String> {
    let matter = if cleanup_temp {
        Matter::open(root).map_err(|e| e.to_string())?
    } else {
        Matter::open_for_read(root).map_err(|e| e.to_string())?
    };
    let info = matter.info().map_err(|e| e.to_string())?;
    Ok(info.name)
}

/// Read-only refresh snapshot for the workspace panels.
#[derive(Debug, Clone, Default)]
pub struct MatterSnapshot {
    pub matter_name: String,
    pub matter_id: String,
    pub sources: Vec<SourceRow>,
    pub psts: Vec<PstRow>,
    pub jobs: Vec<JobRow>,
    pub item_count: u64,
    pub journal_mode: String,
    /// Items with `dedup_role = unique` (0 if never run).
    pub dedup_unique: u64,
    /// Items with `dedup_role = duplicate`.
    pub dedup_duplicate: u64,
    /// Matter-saved user cull presets (`cull_presets` table).
    pub cull_presets: Vec<CullPresetRow>,
}

/// Compact cull preset row for the desk dropdown (id + display name).
#[derive(Debug, Clone)]
pub struct CullPresetRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PstRow {
    pub item_id: String,
    pub source_id: String,
    pub path: String,
    pub status: String,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub error_summary: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Load lists via [`Matter::open_for_read`] (WAL-safe; no workspace/temp wipe).
pub fn refresh_snapshot(matter_root: &Utf8Path) -> Result<MatterSnapshot, String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let info = matter.info().map_err(|e| e.to_string())?;

    let journal_mode: String = matter
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|_| "unknown".into());

    let sources = matter
        .list_sources()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| SourceRow {
            id: s.id,
            path: s.path,
            kind: s.kind,
            status: s.status,
        })
        .collect();

    let psts = matter
        .list_items_by_file_category("pst")
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|i| PstRow {
            item_id: i.id,
            source_id: i.source_id.unwrap_or_default(),
            path: i.path.unwrap_or_else(|| "(no path)".into()),
            status: i.status,
            size_bytes: i.size_bytes,
        })
        .collect();

    let jobs = matter
        .list_jobs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|j| JobRow {
            id: j.id,
            kind: j.kind,
            state: j.state.as_str().to_string(),
            error_summary: j.error_summary,
            started_at: j.started_at,
            finished_at: j.finished_at,
        })
        .collect();

    let item_count = matter.count_items().map_err(|e| e.to_string())?;
    let dedup_counts = matter.count_by_dedup_role().map_err(|e| e.to_string())?;
    let cull_presets = matter
        .list_cull_presets()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| CullPresetRow {
            id: p.id,
            name: p.name,
        })
        .collect();

    Ok(MatterSnapshot {
        matter_name: info.name,
        matter_id: info.id,
        sources,
        psts,
        jobs,
        item_count,
        journal_mode,
        dedup_unique: dedup_counts.unique,
        dedup_duplicate: dedup_counts.duplicate,
        cull_presets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn utf8_temp() -> (TempDir, Utf8PathBuf) {
        let tmp = TempDir::new().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8 temp");
        (tmp, p)
    }

    #[test]
    fn create_open_refresh_and_wal() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "SmokeCase").expect("create");
        let name = open_matter(&root, true).expect("open");
        assert_eq!(name, "SmokeCase");
        let name_ro = open_matter(&root, false).expect("open_for_read");
        assert_eq!(name_ro, "SmokeCase");

        let snap = refresh_snapshot(&root).expect("snap");
        assert_eq!(snap.matter_name, "SmokeCase");
        assert!(snap.sources.is_empty());
        assert_eq!(snap.item_count, 0);
        assert_eq!(snap.journal_mode.to_lowercase(), "wal");
        assert!(
            snap.cull_presets.is_empty(),
            "fresh matter has no user cull presets"
        );
    }

    #[test]
    fn refresh_includes_user_cull_presets() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "CullPresetCase").expect("create");

        let matter = Matter::open(&root).expect("open");
        matter
            .upsert_cull_preset(matter_core::CullPresetInput {
                id: None,
                name: "my_rules".into(),
                description: Some("desk smoke".into()),
                rules_json: r#"[{"type":"dedup_unique"}]"#.into(),
                created_by: None,
            })
            .expect("upsert");
        drop(matter);

        let snap = refresh_snapshot(&root).expect("snap");
        assert_eq!(snap.cull_presets.len(), 1);
        assert_eq!(snap.cull_presets[0].name, "my_rules");
        assert!(!snap.cull_presets[0].id.is_empty());
    }

    #[test]
    fn bad_name_rejected() {
        let (_t, base) = utf8_temp();
        assert!(create_matter(&base, "").is_err());
        assert!(create_matter(&base, "a/b").is_err());
    }

    /// Concurrent reader during a held writer connection (WAL / open_for_read).
    #[test]
    fn open_for_read_while_writer_connected() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "Concurrent").expect("create");
        let barrier = Arc::new(Barrier::new(2));

        let writer_root = root.clone();
        let b_w = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            let matter = Matter::open(&writer_root).expect("writer open");
            // Hold connection open; insert a source while reader runs.
            b_w.wait();
            let _ = matter
                .insert_source(r"C:\exports\pkg", "folder", "importing", None)
                .expect("insert");
            // Keep alive briefly so reader overlaps.
            thread::sleep(std::time::Duration::from_millis(50));
            drop(matter);
        });

        let reader_root = root.clone();
        let b_r = Arc::clone(&barrier);
        let reader = thread::spawn(move || {
            b_r.wait();
            // May see empty or one source depending on race; must not hard-fail.
            let mut ok = false;
            for _ in 0..20 {
                match refresh_snapshot(&reader_root) {
                    Ok(snap) => {
                        assert_eq!(snap.matter_name, "Concurrent");
                        assert_eq!(snap.journal_mode.to_lowercase(), "wal");
                        ok = true;
                        break;
                    }
                    Err(e) => {
                        assert!(
                            crate::params::is_transient_sqlite_lock(&e),
                            "unexpected refresh error: {e}"
                        );
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
            assert!(ok, "open_for_read refresh never succeeded under writer");
        });

        writer.join().expect("writer");
        reader.join().expect("reader");
    }
}
