//! Safe ZIP expand into matter CAS + inventory, with leaf checkpoints.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use camino::Utf8Path;
use matter_core::{ItemErrorInput, ItemInput, Matter};
use serde::{Deserialize, Serialize};
use zip::read::ZipFile;
use zip::ZipArchive;

use crate::encoding::decode_zip_name;
use crate::error::{codes, Error, Result};
use crate::limits::ExpandLimits;
use crate::path_safety::{join_logical_path, reject_symlink_or_reparse, sanitize_logical_path};

/// Opaque expand cursor (job checkpoint stage `expand`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandCursor {
    pub source_id: String,
    pub package_root: String,
    pub archive_stack: Vec<String>,
    pub last_successfully_extracted_logical_path: Option<String>,
    pub completed_count: u64,
    pub bytes_extracted: u64,
    pub completed_top_level: Vec<String>,
    pub nested_depth_max_seen: u32,
    pub entries_err: u64,
    pub psts_found: u64,
    pub nested_zips: u64,
    pub entries_skipped: u64,
}

impl ExpandCursor {
    pub fn new(source_id: &str, package_root: &str) -> Self {
        Self {
            source_id: source_id.to_string(),
            package_root: package_root.to_string(),
            archive_stack: Vec::new(),
            last_successfully_extracted_logical_path: None,
            completed_count: 0,
            bytes_extracted: 0,
            completed_top_level: Vec::new(),
            nested_depth_max_seen: 0,
            entries_err: 0,
            psts_found: 0,
            nested_zips: 0,
            entries_skipped: 0,
        }
    }
}

/// Runtime expand session state.
pub(crate) struct ExpandSession<'a> {
    pub matter: &'a Matter,
    pub source_id: &'a str,
    pub job_id: &'a str,
    pub limits: &'a ExpandLimits,
    pub cancel: Option<&'a dyn Fn() -> bool>,
    pub cursor: ExpandCursor,
    /// Successful leaf commits since last checkpoint flush.
    pub since_cp_entries: u64,
    pub since_cp_bytes: u64,
    /// Optional: count of CAS put_bytes invocations (tests).
    pub cas_puts: u64,
}

impl<'a> ExpandSession<'a> {
    pub fn cancelled(&self) -> bool {
        self.cancel.map(|f| f()).unwrap_or(false)
    }

    pub fn flush_checkpoint(&mut self) -> Result<()> {
        let json = serde_json::to_string(&self.cursor)?;
        self.matter.put_checkpoint(
            self.job_id,
            "expand",
            &json,
            self.cursor.completed_count as i64,
        )?;
        // Mirror to source cursor for convenience.
        self.matter
            .update_source(self.source_id, "importing", Some(&json))?;
        self.since_cp_entries = 0;
        self.since_cp_bytes = 0;
        Ok(())
    }

    fn maybe_checkpoint(&mut self) -> Result<()> {
        let n = self.limits.checkpoint_every_n_entries.max(1);
        let b = self.limits.checkpoint_every_bytes.max(1);
        if self.since_cp_entries >= n || self.since_cp_bytes >= b {
            self.flush_checkpoint()?;
        }
        Ok(())
    }

    /// Whether inventory already has this leaf with a digest (resume skip).
    pub fn already_inventoried(&self, logical_path: &str) -> Result<bool> {
        if let Some(item) = self
            .matter
            .item_by_source_path(self.source_id, logical_path)?
        {
            // Authoritative resume key: (source_id, path) + native_sha256 present.
            if item.native_sha256.is_some()
                && (item.status == "expanded" || item.status == "discovered")
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn record_entry_error(&mut self, logical_path: &str, err: &Error) -> Result<()> {
        self.cursor.entries_err += 1;
        self.matter.record_item_error(ItemErrorInput {
            item_id: None,
            source_id: Some(self.source_id.to_string()),
            job_id: Some(self.job_id.to_string()),
            stage: "expand".into(),
            code: err.code().into(),
            message: err.to_string(),
            detail: Some(serde_json::json!({ "path": logical_path }).to_string()),
        })?;
        Ok(())
    }

    /// Commit leaf bytes to CAS + inventory; update cursor.
    pub fn commit_leaf(&mut self, logical_path: &str, data: &[u8], status: &str) -> Result<()> {
        if self.cancelled() {
            return Err(Error::Cancelled);
        }

        if self.already_inventoried(logical_path)? {
            self.cursor.entries_skipped += 1;
            return Ok(());
        }

        let size = data.len() as u64;
        if self.cursor.completed_count.saturating_add(1) > self.limits.max_entries {
            return Err(Error::ZipBomb {
                code: codes::ZIP_BOMB_ENTRIES,
                message: format!(
                    "entry count would exceed max_entries={}",
                    self.limits.max_entries
                ),
            });
        }
        let new_total = self.cursor.bytes_extracted.saturating_add(size);
        if new_total > self.limits.max_uncompressed_bytes {
            return Err(Error::ZipBomb {
                code: codes::ZIP_BOMB_SIZE,
                message: format!(
                    "total uncompressed {} exceeds max {}",
                    new_total, self.limits.max_uncompressed_bytes
                ),
            });
        }
        if size > self.limits.max_entry_buffer_bytes {
            return Err(Error::ZipBomb {
                code: codes::ZIP_BOMB_SIZE,
                message: format!(
                    "entry size {size} exceeds max_entry_buffer_bytes {}",
                    self.limits.max_entry_buffer_bytes
                ),
            });
        }

        let digest = self.matter.put_bytes(data)?;
        self.cas_puts += 1;

        // Path-only classify at insert so inventory is immediately filterable (0037).
        let category = if logical_path.trim().is_empty() {
            None
        } else {
            Some(
                ::file_category::classify_path_mime(Some(logical_path), None)
                    .category
                    .as_str()
                    .to_string(),
            )
        };

        self.matter.insert_item(ItemInput {
            source_id: Some(self.source_id.to_string()),
            path: Some(logical_path.to_string()),
            native_sha256: Some(digest),
            status: status.into(),
            size_bytes: Some(data.len() as i64),
            file_category: category,
            // Inventory-only row (0016); Normalized Item fields filled by 0018+.
            ..Default::default()
        })?;

        self.cursor.completed_count += 1;
        self.cursor.bytes_extracted = new_total;
        self.cursor.last_successfully_extracted_logical_path = Some(logical_path.to_string());
        self.since_cp_entries += 1;
        self.since_cp_bytes = self.since_cp_bytes.saturating_add(size);

        if logical_path.to_ascii_lowercase().ends_with(".pst") {
            self.cursor.psts_found += 1;
        }

        self.maybe_checkpoint()?;
        Ok(())
    }
}

/// Expand a top-level package path (file or directory) into the session.
pub(crate) fn expand_package(session: &mut ExpandSession<'_>, package: &Utf8Path) -> Result<()> {
    reject_symlink_or_reparse(package)?;

    let meta = std::fs::symlink_metadata(package.as_std_path()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::PackageNotFound(package.to_string())
        } else {
            Error::Io(e)
        }
    })?;

    if meta.is_file() {
        expand_top_level_file(session, package, package.file_name().unwrap_or("file"))?;
    } else if meta.is_dir() {
        expand_directory(session, package, package)?;
    } else {
        return Err(Error::UnsupportedPackage(package.to_string()));
    }
    session.flush_checkpoint()?;
    Ok(())
}

fn expand_directory(
    session: &mut ExpandSession<'_>,
    package_root: &Utf8Path,
    dir: &Utf8Path,
) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir.as_std_path())?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if session.cancelled() {
            return Err(Error::Cancelled);
        }
        let path = entry.path();
        let name = entry.file_name();
        let _name_str = name.to_string_lossy();
        let rel = path
            .strip_prefix(package_root.as_std_path())
            .unwrap_or(path.as_path());
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        // Never follow filesystem symlinks / reparse points (evidence boundary).
        if let Ok(utf) = camino::Utf8PathBuf::from_path_buf(path.clone()) {
            if let Err(e) = reject_symlink_or_reparse(&utf) {
                session.record_entry_error(&rel_str, &e)?;
                continue;
            }
        } else {
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_symlink() => {
                    let err = Error::PathRejected {
                        code: codes::ZIP_UNSAFE_PATH,
                        message: format!("symlink/reparse point rejected: {rel_str}"),
                    };
                    session.record_entry_error(&rel_str, &err)?;
                    continue;
                }
                Ok(_) => {}
                Err(e) => {
                    session.record_entry_error(&rel_str, &Error::Io(e))?;
                    continue;
                }
            }
        }

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                session.record_entry_error(&rel_str, &Error::Io(e))?;
                continue;
            }
        };
        if meta.is_dir() {
            if let Some(s) = path.to_str() {
                expand_directory(session, package_root, Utf8Path::new(s))?;
            }
            continue;
        }
        let logical = match sanitize_logical_path(&rel_str) {
            Ok(p) => p,
            Err(e) => {
                session.record_entry_error(&rel_str, &e)?;
                continue;
            }
        };
        if let Some(s) = path.to_str() {
            expand_top_level_file(session, Utf8Path::new(s), &logical)?;
        }
    }
    Ok(())
}

fn expand_top_level_file(
    session: &mut ExpandSession<'_>,
    file_path: &Utf8Path,
    logical_name: &str,
) -> Result<()> {
    reject_symlink_or_reparse(file_path)?;
    let lower = logical_name.to_ascii_lowercase();
    if lower.ends_with(".7z") {
        let err = Error::UnsupportedContainer(logical_name.to_string());
        session.record_entry_error(logical_name, &err)?;
        return Ok(());
    }
    if lower.ends_with(".zip") {
        expand_zip_file(
            session,
            file_path.as_std_path(),
            &[logical_name.to_string()],
            1,
        )?;
        if !session
            .cursor
            .completed_top_level
            .iter()
            .any(|x| x == logical_name)
        {
            session
                .cursor
                .completed_top_level
                .push(logical_name.to_string());
        }
        return Ok(());
    }

    // Loose file (including .pst): read + CAS.
    let data = read_file_capped(
        file_path.as_std_path(),
        session.limits.max_entry_buffer_bytes,
    )?;
    let status = if lower.ends_with(".pst") {
        "discovered"
    } else {
        "expanded"
    };
    session.commit_leaf(logical_name, &data, status)?;
    if !session
        .cursor
        .completed_top_level
        .iter()
        .any(|x| x == logical_name)
    {
        session
            .cursor
            .completed_top_level
            .push(logical_name.to_string());
    }
    Ok(())
}

fn expand_zip_file(
    session: &mut ExpandSession<'_>,
    zip_path: &Path,
    archive_stack: &[String],
    depth: u32,
) -> Result<()> {
    if depth > session.limits.max_zip_depth {
        return Err(Error::ZipDepth {
            max: session.limits.max_zip_depth,
        });
    }
    if depth > session.cursor.nested_depth_max_seen {
        session.cursor.nested_depth_max_seen = depth;
    }
    if depth > 1 {
        session.cursor.nested_zips += 1;
    }

    let file = File::open(zip_path).map_err(Error::Io)?;
    let mut archive = ZipArchive::new(file).map_err(|e| match e {
        zip::result::ZipError::Io(io) => Error::Io(io),
        other => Error::Zip(other),
    })?;

    // Collect indices sorted by name for deterministic resume walks.
    let mut indices: Vec<usize> = (0..archive.len()).collect();
    indices.sort_by_key(|&i| {
        archive
            .by_index(i)
            .map(|f| f.name().to_string())
            .unwrap_or_default()
    });

    for i in indices {
        if session.cancelled() {
            return Err(Error::Cancelled);
        }
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                let err = Error::Zip(e);
                session.record_entry_error(
                    &join_logical_path(archive_stack, &format!("index_{i}")),
                    &err,
                )?;
                continue;
            }
        };

        if entry.is_dir() {
            continue;
        }

        // Symlink / external attributes: zip crate exposes unix mode when present.
        if is_symlink_entry(&entry) {
            let name = entry.name().to_string();
            let err = Error::PathRejected {
                code: codes::ZIP_UNSAFE_PATH,
                message: format!("symlink entry rejected: {name}"),
            };
            session.record_entry_error(&name, &err)?;
            continue;
        }

        let raw_name_bytes = entry.name_raw();
        let utf8_flag = entry_utf8_flag(&entry);
        let (decoded, _decode_path) = decode_zip_name(raw_name_bytes, utf8_flag);
        let sanitized = match sanitize_logical_path(&decoded) {
            Ok(p) => p,
            Err(e) => {
                let logical = join_logical_path(archive_stack, &decoded);
                session.record_entry_error(&logical, &e)?;
                continue;
            }
        };

        let logical = join_logical_path(archive_stack, &sanitized);
        let lower = sanitized.to_ascii_lowercase();
        let is_nested_zip = lower.ends_with(".zip");

        // Resume: inventoried non-container leaves are skipped; inventoried
        // containers re-walk children (do not re-put / re-insert the container).
        if session.already_inventoried(&logical)? {
            if is_nested_zip {
                let data = load_inventoried_or_reread(session, &logical, &mut entry)?;
                let mut child_stack = archive_stack.to_vec();
                child_stack.push(sanitized.clone());
                expand_zip_bytes(session, &data, &child_stack, depth + 1)?;
                continue;
            }
            session.cursor.entries_skipped += 1;
            continue;
        }

        let compressed = entry.compressed_size();
        let uncompressed = entry.size();

        // Bomb limits are fatal for the job (fail closed).
        check_entry_bomb(session, compressed, uncompressed)?;

        // Nested zip: materialize to temp, recurse; inventory the zip blob too.
        if is_nested_zip {
            let data = read_zip_entry(&mut entry, session.limits.max_entry_buffer_bytes)?;
            check_ratio(session, compressed, data.len() as u64)?;
            session.commit_leaf(&logical, &data, "expanded")?;

            let mut child_stack = archive_stack.to_vec();
            child_stack.push(sanitized.clone());
            expand_zip_bytes(session, &data, &child_stack, depth + 1)?;
            continue;
        }

        if lower.ends_with(".7z") {
            let err = Error::UnsupportedContainer(logical.clone());
            session.record_entry_error(&logical, &err)?;
            continue;
        }

        let data = read_zip_entry(&mut entry, session.limits.max_entry_buffer_bytes)?;
        check_ratio(session, compressed, data.len() as u64)?;

        let status = if lower.ends_with(".pst") {
            "discovered"
        } else {
            "expanded"
        };
        session.commit_leaf(&logical, &data, status)?;
    }
    Ok(())
}

fn expand_zip_bytes(
    session: &mut ExpandSession<'_>,
    data: &[u8],
    archive_stack: &[String],
    depth: u32,
) -> Result<()> {
    // nested_zips is incremented once in expand_zip_file when depth > 1.
    // Write to a temp file so ZipArchive can seek.
    // Prefer matter workspace/temp (encryption boundary) over OS %TEMP%.
    let temp_root = session.matter.workspace_temp_dir();
    std::fs::create_dir_all(temp_root.as_std_path()).map_err(Error::Io)?;
    let mut tmp = tempfile::Builder::new()
        .prefix("purview-nested-")
        .suffix(".zip")
        .tempfile_in(temp_root.as_std_path())
        .map_err(Error::Io)?;
    tmp.write_all(data).map_err(Error::Io)?;
    tmp.flush().map_err(Error::Io)?;
    expand_zip_file(session, tmp.path(), archive_stack, depth)
}

/// Load container bytes from CAS for an already-inventoried path; fall back to
/// re-reading the ZIP entry body without re-inventorying.
fn load_inventoried_or_reread<R: Read>(
    session: &ExpandSession<'_>,
    logical_path: &str,
    entry: &mut ZipFile<'_, R>,
) -> Result<Vec<u8>> {
    if let Some(item) = session
        .matter
        .item_by_source_path(session.source_id, logical_path)?
    {
        if let Some(ref digest) = item.native_sha256 {
            if let Ok(bytes) = session.matter.get_bytes(digest) {
                return Ok(bytes);
            }
        }
    }
    // CAS miss or missing digest: re-read entry (still no re-put / re-insert).
    let compressed = entry.compressed_size();
    let uncompressed = entry.size();
    check_entry_bomb(session, compressed, uncompressed)?;
    let data = read_zip_entry(entry, session.limits.max_entry_buffer_bytes)?;
    check_ratio(session, compressed, data.len() as u64)?;
    Ok(data)
}

fn check_entry_bomb(session: &ExpandSession<'_>, compressed: u64, uncompressed: u64) -> Result<()> {
    let projected = session.cursor.bytes_extracted.saturating_add(uncompressed);
    if projected > session.limits.max_uncompressed_bytes {
        return Err(Error::ZipBomb {
            code: codes::ZIP_BOMB_SIZE,
            message: format!(
                "projected uncompressed {projected} exceeds max {}",
                session.limits.max_uncompressed_bytes
            ),
        });
    }
    if session.cursor.completed_count.saturating_add(1) > session.limits.max_entries {
        return Err(Error::ZipBomb {
            code: codes::ZIP_BOMB_ENTRIES,
            message: format!(
                "entry count exceeds max_entries={}",
                session.limits.max_entries
            ),
        });
    }
    check_ratio(session, compressed, uncompressed)
}

fn check_ratio(session: &ExpandSession<'_>, compressed: u64, uncompressed: u64) -> Result<()> {
    if compressed > 0 {
        let ratio = uncompressed as f64 / compressed as f64;
        if ratio > session.limits.max_compression_ratio {
            return Err(Error::ZipBomb {
                code: codes::ZIP_BOMB_RATIO,
                message: format!(
                    "compression ratio {ratio:.1} exceeds max {}",
                    session.limits.max_compression_ratio
                ),
            });
        }
    } else if uncompressed > 1024 * 1024 {
        // Zero compressed size with large uncompressed is suspicious.
        return Err(Error::ZipBomb {
            code: codes::ZIP_BOMB_RATIO,
            message: "zero compressed size with large uncompressed payload".into(),
        });
    }
    Ok(())
}

fn read_zip_entry<R: Read>(entry: &mut ZipFile<'_, R>, max_bytes: u64) -> Result<Vec<u8>> {
    let size = entry.size();
    if size > max_bytes {
        return Err(Error::ZipBomb {
            code: codes::ZIP_BOMB_SIZE,
            message: format!("entry size {size} exceeds buffer cap {max_bytes}"),
        });
    }
    let mut buf = Vec::with_capacity(size as usize);
    entry.read_to_end(&mut buf).map_err(Error::Io)?;
    if buf.len() as u64 > max_bytes {
        return Err(Error::ZipBomb {
            code: codes::ZIP_BOMB_SIZE,
            message: "entry read exceeded buffer cap".into(),
        });
    }
    Ok(buf)
}

fn read_file_capped(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path)?;
    let len = meta.len();
    if len > max_bytes {
        return Err(Error::ZipBomb {
            code: codes::ZIP_BOMB_SIZE,
            message: format!("file size {len} exceeds buffer cap {max_bytes}"),
        });
    }
    let mut f = File::open(path)?;
    let mut buf = Vec::with_capacity(len as usize);
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

fn is_symlink_entry<R: Read>(entry: &ZipFile<'_, R>) -> bool {
    // Unix symlink: high 4 bits of mode == 0o12 (when external attrs present).
    if let Some(mode) = entry.unix_mode() {
        return (mode & 0o170000) == 0o120000;
    }
    false
}

fn entry_utf8_flag<R: Read>(entry: &ZipFile<'_, R>) -> bool {
    // Prefer true when name_raw is valid UTF-8 and matches the decoded name().
    // Full general-purpose bit 11 is not always exposed; decode policy still
    // falls back CP437 → Win-1252 for non-UTF-8 raw names.
    let raw = entry.name_raw();
    match std::str::from_utf8(raw) {
        Ok(s) => entry.name() == s,
        Err(_) => false,
    }
}
