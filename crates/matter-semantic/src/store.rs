//! Model-namespaced on-disk vector store under `{matter}/semantic/{model_id}/`.

use std::fs;
use std::io::{Read, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SemanticError};

/// Directory name under the matter root.
pub const SEMANTIC_DIR_NAME: &str = "semantic";

/// Store format version for meta.json / item files.
pub const STORE_FORMAT_VERSION: u32 = 1;

/// Sanitize `model_id` for a single path segment under `semantic/`.
///
/// Replaces `:`, `/`, `\` with `_`. Rejects empty, `..`, absolute, or residual
/// path separators after sanitize.
pub fn sanitize_model_id(model_id: &str) -> Result<String> {
    let raw = model_id.trim();
    if raw.is_empty() {
        return Err(SemanticError::PathRejected(
            "model_id is empty after trim".into(),
        ));
    }
    if raw.contains("..") {
        return Err(SemanticError::PathRejected(format!(
            "model_id must not contain '..': {raw}"
        )));
    }
    // Reject absolute Windows / Unix paths before sanitize.
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err(SemanticError::PathRejected(format!(
            "model_id must not be absolute: {raw}"
        )));
    }
    if raw.len() >= 2 && raw.as_bytes()[1] == b':' && raw.as_bytes()[0].is_ascii_alphabetic() {
        // Drive letter absolute path like C:\...
        if raw.len() >= 3 && (raw.as_bytes()[2] == b'\\' || raw.as_bytes()[2] == b'/') {
            return Err(SemanticError::PathRejected(format!(
                "model_id must not be absolute: {raw}"
            )));
        }
    }

    let sanitized: String = raw
        .chars()
        .map(|c| match c {
            ':' | '/' | '\\' => '_',
            c => c,
        })
        .collect();

    if sanitized.contains("..")
        || sanitized.contains('/')
        || sanitized.contains('\\')
        || sanitized.is_empty()
    {
        return Err(SemanticError::PathRejected(format!(
            "model_id sanitizes to invalid path segment: {raw} → {sanitized}"
        )));
    }
    Ok(sanitized)
}

/// Namespace directory: `{matter_root}/semantic/{sanitized_model_id}`.
pub fn namespace_dir(matter_root: &Utf8Path, model_id: &str) -> Result<Utf8PathBuf> {
    let seg = sanitize_model_id(model_id)?;
    Ok(matter_root.join(SEMANTIC_DIR_NAME).join(seg))
}

/// Store meta written to `meta.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreMeta {
    pub format_version: u32,
    pub model_id: String,
    pub dims: usize,
    pub chunk_chars: u32,
    pub chunk_overlap: u32,
    pub max_chunks_per_item: u32,
    pub engine_tag: String,
    pub fingerprint: String,
}

/// One chunk vector on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredChunk {
    pub ordinal: u32,
    pub start_offset: usize,
    pub end_offset: usize,
    pub vector: Vec<f32>,
}

/// Per-item vector file (`items/{item_id}.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemVectorFile {
    pub format_version: u32,
    pub item_id: String,
    pub text_sha256: String,
    pub model_id: String,
    pub dims: usize,
    /// Job fingerprint at embed time (model + dims + chunk params + engine).
    /// Query only scores files whose fingerprint matches the active matter
    /// fingerprint so mid-rebuild mixed old/new vectors are excluded (Codex P1).
    #[serde(default)]
    pub fingerprint: String,
    pub chunks: Vec<StoredChunk>,
}

/// Handle for a model-namespaced semantic store.
pub struct SemanticStore {
    root: Utf8PathBuf,
    model_id: String,
    dims: usize,
}

impl SemanticStore {
    /// Open (create dirs) for an active model namespace.
    pub fn open(matter_root: &Utf8Path, model_id: &str, dims: usize) -> Result<Self> {
        let root = namespace_dir(matter_root, model_id)?;
        fs::create_dir_all(root.join("items").as_std_path())?;
        Ok(Self {
            root,
            model_id: model_id.to_string(),
            dims,
        })
    }

    /// Path of the namespace directory.
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    fn items_dir(&self) -> Utf8PathBuf {
        self.root.join("items")
    }

    fn item_path(&self, item_id: &str) -> Result<Utf8PathBuf> {
        // Prevent traversal via item_id.
        if item_id.contains("..")
            || item_id.contains('/')
            || item_id.contains('\\')
            || item_id.is_empty()
        {
            return Err(SemanticError::PathRejected(format!(
                "invalid item_id for store path: {item_id}"
            )));
        }
        Ok(self.items_dir().join(format!("{item_id}.json")))
    }

    fn meta_path(&self) -> Utf8PathBuf {
        self.root.join("meta.json")
    }

    /// Write / overwrite store meta.
    pub fn write_meta(&self, meta: &StoreMeta) -> Result<()> {
        if meta.model_id != self.model_id {
            return Err(SemanticError::other(format!(
                "meta model_id {} != store {}",
                meta.model_id, self.model_id
            )));
        }
        if meta.dims != self.dims {
            return Err(SemanticError::other(format!(
                "meta dims {} != store {}",
                meta.dims, self.dims
            )));
        }
        let json = serde_json::to_string_pretty(meta)?;
        fs::write(self.meta_path().as_std_path(), json)?;
        Ok(())
    }

    /// Read store meta if present.
    pub fn read_meta(&self) -> Result<Option<StoreMeta>> {
        let p = self.meta_path();
        if !p.as_std_path().exists() {
            return Ok(None);
        }
        let s = fs::read_to_string(p.as_std_path())?;
        let meta: StoreMeta = serde_json::from_str(&s)?;
        if meta.model_id != self.model_id {
            return Err(SemanticError::ModelMismatch {
                embedder: self.model_id.clone(),
                active: meta.model_id,
            });
        }
        Ok(Some(meta))
    }

    /// Delete one item's vector file (if any).
    pub fn delete_item(&self, item_id: &str) -> Result<bool> {
        let p = self.item_path(item_id)?;
        if p.as_std_path().exists() {
            fs::remove_file(p.as_std_path())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Write item vectors (delete-before-write).
    pub fn write_item(&self, file: &ItemVectorFile) -> Result<()> {
        if file.model_id != self.model_id {
            return Err(SemanticError::ModelMismatch {
                embedder: file.model_id.clone(),
                active: self.model_id.clone(),
            });
        }
        if file.dims != self.dims {
            return Err(SemanticError::other(format!(
                "item dims {} != store dims {}",
                file.dims, self.dims
            )));
        }
        for c in &file.chunks {
            if c.vector.len() != self.dims {
                return Err(SemanticError::other(format!(
                    "chunk ordinal {} vector len {} != dims {}",
                    c.ordinal,
                    c.vector.len(),
                    self.dims
                )));
            }
        }
        let p = self.item_path(&file.item_id)?;
        if p.as_std_path().exists() {
            fs::remove_file(p.as_std_path())?;
        }
        let json = serde_json::to_string(file)?;
        let mut f = fs::File::create(p.as_std_path())?;
        f.write_all(json.as_bytes())?;
        Ok(())
    }

    /// Load one item's vectors if present.
    pub fn read_item(&self, item_id: &str) -> Result<Option<ItemVectorFile>> {
        let p = self.item_path(item_id)?;
        if !p.as_std_path().exists() {
            return Ok(None);
        }
        let mut s = String::new();
        fs::File::open(p.as_std_path())?.read_to_string(&mut s)?;
        let file: ItemVectorFile = serde_json::from_str(&s)?;
        if file.model_id != self.model_id {
            return Err(SemanticError::ModelMismatch {
                embedder: self.model_id.clone(),
                active: file.model_id,
            });
        }
        Ok(Some(file))
    }

    /// Load vectors for many items (skips missing).
    pub fn load_items(&self, item_ids: &[String]) -> Result<Vec<ItemVectorFile>> {
        let mut out = Vec::new();
        for id in item_ids {
            if let Some(f) = self.read_item(id)? {
                out.push(f);
            }
        }
        Ok(out)
    }

    /// Wipe the entire model namespace directory.
    pub fn reset_namespace(matter_root: &Utf8Path, model_id: &str) -> Result<()> {
        let dir = namespace_dir(matter_root, model_id)?;
        if dir.as_std_path().exists() {
            fs::remove_dir_all(dir.as_std_path())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_and_rejects_traversal() {
        assert_eq!(sanitize_model_id("mock:hash_v1").unwrap(), "mock_hash_v1");
        assert_eq!(
            sanitize_model_id("local:minilm-l6-v2").unwrap(),
            "local_minilm-l6-v2"
        );
        assert!(sanitize_model_id("../evil").is_err());
        assert!(sanitize_model_id("/abs/path").is_err());
        assert!(sanitize_model_id(r"C:\windows").is_err());
        assert!(sanitize_model_id("").is_err());
    }

    #[test]
    fn write_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let store = SemanticStore::open(&root, "mock:hash_v1", 4).unwrap();
        store
            .write_meta(&StoreMeta {
                format_version: STORE_FORMAT_VERSION,
                model_id: "mock:hash_v1".into(),
                dims: 4,
                chunk_chars: 800,
                chunk_overlap: 120,
                max_chunks_per_item: 48,
                engine_tag: "mock".into(),
                fingerprint: "fp".into(),
            })
            .unwrap();
        let item = ItemVectorFile {
            format_version: STORE_FORMAT_VERSION,
            item_id: "itm1".into(),
            text_sha256: "abc".into(),
            model_id: "mock:hash_v1".into(),
            dims: 4,
            fingerprint: "fp".into(),
            chunks: vec![StoredChunk {
                ordinal: 0,
                start_offset: 0,
                end_offset: 10,
                vector: vec![0.5, 0.5, 0.5, 0.5],
            }],
        };
        store.write_item(&item).unwrap();
        let back = store.read_item("itm1").unwrap().unwrap();
        assert_eq!(back.chunks.len(), 1);
        store.delete_item("itm1").unwrap();
        assert!(store.read_item("itm1").unwrap().is_none());
    }
}
