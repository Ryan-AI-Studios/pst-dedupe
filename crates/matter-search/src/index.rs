//! Per-matter Tantivy index open/create, writer, delete-before-add.

use std::fs;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::INDEX_DIR;
use tantivy::schema::{TantivyDocument, Term, Value as _};
use tantivy::{doc, Index, IndexWriter, Opstamp, ReloadPolicy};

use crate::error::{Result, SearchError};
use crate::schema::FtsSchema;

/// Directory name under the matter root (`index/`).
pub const INDEX_DIR_NAME: &str = INDEX_DIR;

/// Default IndexWriter heap budget (~50 MiB).
pub const DEFAULT_WRITER_HEAP_BYTES: usize = 50 * 1024 * 1024;

/// Handle to a matter's on-disk Tantivy index.
///
/// Owns the [`Index`]. Call [`MatterIndex::shutdown`] (or drop) before
/// `remove_dir_all(index/)` on Windows — mmap holds OS file locks.
pub struct MatterIndex {
    index: Index,
    fts_schema: FtsSchema,
    index_dir: Utf8PathBuf,
}

impl MatterIndex {
    /// Absolute path to `<matter_root>/index`.
    pub fn index_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
        matter_root.join(INDEX_DIR_NAME)
    }

    /// Open an existing index or create one with the P0 schema.
    pub fn open_or_create(matter_root: &Utf8Path) -> Result<Self> {
        let index_dir = Self::index_dir(matter_root);
        let fts_schema = FtsSchema::build();
        let path = index_dir.as_std_path();
        if path.exists() {
            // Existing directory: try open; if empty/corrupt, recreate.
            if is_empty_dir(path)? {
                fs::create_dir_all(path)?;
                let index = Index::create_in_dir(path, fts_schema.schema.clone())?;
                return Ok(Self {
                    index,
                    fts_schema,
                    index_dir,
                });
            }
            let index = Index::open_in_dir(path).map_err(|e| {
                SearchError::Index(format!(
                    "failed to open FTS index at {index_dir}: {e} — try Rebuild index"
                ))
            })?;
            Ok(Self {
                index,
                fts_schema,
                index_dir,
            })
        } else {
            fs::create_dir_all(path)?;
            let index = Index::create_in_dir(path, fts_schema.schema.clone())?;
            Ok(Self {
                index,
                fts_schema,
                index_dir,
            })
        }
    }

    /// Explicit shutdown: drop all index handles (Windows mmap rebuild prep).
    pub fn shutdown(self) {
        // Fields drop in order; explicit for API clarity.
        drop(self);
    }

    /// Path of the index directory.
    pub fn dir(&self) -> &Utf8Path {
        &self.index_dir
    }

    /// Shared Tantivy schema helpers.
    pub fn fts_schema(&self) -> &FtsSchema {
        &self.fts_schema
    }

    /// Borrow the underlying Tantivy [`Index`].
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// Open an [`IndexWriter`] with the given heap budget.
    pub fn writer(&self, heap_bytes: usize) -> Result<IndexWriter> {
        let heap = heap_bytes.max(15_000_000);
        Ok(self.index.writer(heap)?)
    }

    /// Open a reader that reloads on commit (for search).
    pub fn reader(&self) -> Result<tantivy::IndexReader> {
        Ok(self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?)
    }
}

/// Delete-by-`item_id` then add a document (required crash-recovery path).
///
/// Always delete first (no-op if absent) so re-index never leaves duplicates.
pub fn delete_then_add(
    writer: &mut IndexWriter,
    fts: &FtsSchema,
    item_id: &str,
    subject: &str,
    body: &str,
    path: &str,
    attach_names: &str,
) -> Result<Opstamp> {
    let term = Term::from_field_text(fts.item_id, item_id);
    writer.delete_term(term);
    let doc = doc!(
        fts.item_id => item_id,
        fts.subject => subject,
        fts.body => body,
        fts.path => path,
        fts.attach_names => attach_names,
    );
    Ok(writer.add_document(doc)?)
}

/// Remove the on-disk index directory after all handles are dropped.
///
/// Caller **must** call [`MatterIndex::shutdown`] (and drop any readers) first.
pub fn remove_index_dir(matter_root: &Utf8Path) -> Result<()> {
    let dir = MatterIndex::index_dir(matter_root);
    let path = dir.as_std_path();
    if path.exists() {
        fs::remove_dir_all(path).map_err(|e| {
            SearchError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "failed to remove index dir {dir} (ensure all Index/Reader handles are dropped first): {e}"
                ),
            ))
        })?;
    }
    Ok(())
}

/// Read `item_id` from a stored document.
pub fn stored_item_id(fts: &FtsSchema, doc: &TantivyDocument) -> Option<String> {
    doc.get_first(fts.item_id)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)?;
    Ok(entries.next().is_none())
}
