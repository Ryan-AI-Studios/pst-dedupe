//! Semantic query: pre-filter FilterSpec → cosine on eligible → group-before-limit.

use matter_core::{FilterSpec, Matter, SemanticMatterMeta};
use serde::{Deserialize, Serialize};

use crate::embedder::{cosine_similarity, Embedder};
use crate::error::{Result, SemanticError};
use crate::store::SemanticStore;

/// Default max eligible items to score (safety). Beyond this we still score
/// what we load but warn via honest message when truncated.
pub const DEFAULT_MAX_ELIGIBLE_ITEMS: u64 = 50_000;

/// Input for [`search_semantic`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticQuery {
    pub text: String,
    /// Top **items** after group-by max chunk score (not top chunks).
    pub top_n_items: usize,
    /// Optional minimum best-item cosine score.
    pub min_score: Option<f32>,
}

impl Default for SemanticQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            top_n_items: 50,
            min_score: None,
        }
    }
}

/// One ranked item hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticHit {
    pub item_id: String,
    /// Best (max) chunk cosine score for this item.
    pub score: f32,
    /// Winning chunk ordinal (if known).
    pub best_ordinal: Option<u32>,
    pub best_start_offset: Option<usize>,
    pub best_end_offset: Option<usize>,
}

/// Search result package.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchResult {
    pub hits: Vec<SemanticHit>,
    /// True when eligible set was truncated by safety cap.
    pub eligible_truncated: bool,
    pub eligible_count: usize,
}

/// Run semantic search with **mandatory pre-filter**.
///
/// Algorithm (LOCKED):
/// 1. Fail if embedder model_id ≠ active matter meta model_id
/// 2. Resolve FilterSpec → eligible item_ids **first**
/// 3. Load vectors only for those items in active namespace
/// 4. Score all eligible chunks
/// 5. best_score(item) = max chunk score
/// 6. Sort desc, take top_n_items
pub fn search_semantic(
    matter: &Matter,
    matter_root: &camino::Utf8Path,
    query: &SemanticQuery,
    filter: &FilterSpec,
    embedder: &dyn Embedder,
) -> Result<SemanticSearchResult> {
    search_semantic_capped(
        matter,
        matter_root,
        query,
        filter,
        embedder,
        DEFAULT_MAX_ELIGIBLE_ITEMS,
    )
}

/// Same as [`search_semantic`] with explicit eligible-item safety cap.
pub fn search_semantic_capped(
    matter: &Matter,
    matter_root: &camino::Utf8Path,
    query: &SemanticQuery,
    filter: &FilterSpec,
    embedder: &dyn Embedder,
    max_eligible: u64,
) -> Result<SemanticSearchResult> {
    let meta = matter.get_semantic_meta()?;
    ensure_index_ready(&meta, embedder)?;

    let model_id = meta
        .semantic_model_id
        .as_deref()
        .ok_or(SemanticError::IndexNotBuilt)?;
    let dims = meta.semantic_dims.ok_or(SemanticError::IndexNotBuilt)? as usize;

    let qtext = query.text.trim();
    if qtext.is_empty() {
        return Ok(SemanticSearchResult {
            hits: Vec::new(),
            eligible_truncated: false,
            eligible_count: 0,
        });
    }

    // PRE-FILTER first (never global top_k then post-filter).
    let eligible = matter.list_filtered_item_ids(filter, max_eligible)?;
    let eligible_truncated = eligible.len() as u64 >= max_eligible && max_eligible > 0;
    if eligible.is_empty() {
        return Ok(SemanticSearchResult {
            hits: Vec::new(),
            eligible_truncated: false,
            eligible_count: 0,
        });
    }

    let store = SemanticStore::open(matter_root, model_id, dims)?;
    // Refuse to read if on-disk meta points elsewhere (namespace isolation).
    if let Some(sm) = store.read_meta()? {
        if sm.model_id != embedder.model_id() {
            return Err(SemanticError::ModelMismatch {
                embedder: embedder.model_id().to_string(),
                active: sm.model_id,
            });
        }
        // Store meta fingerprint must match active matter fingerprint so a
        // half-written store for a different param set is not silently mixed.
        if let Some(active_fp) = meta.semantic_fingerprint.as_deref() {
            if !active_fp.is_empty() && sm.fingerprint != active_fp {
                return Err(SemanticError::other(format!(
                    "semantic store fingerprint '{}' does not match active matter fingerprint '{active_fp}' — re-run semantic_index",
                    sm.fingerprint
                )));
            }
        }
    }

    let active_fp = meta
        .semantic_fingerprint
        .as_deref()
        .filter(|s| !s.is_empty());

    let qvec = embedder.embed_query(qtext)?;
    if qvec.len() != dims {
        return Err(SemanticError::embedder(format!(
            "query vector dims {} != index dims {dims}",
            qvec.len()
        )));
    }

    let files = store.load_items(&eligible)?;
    let mut best: Vec<SemanticHit> = Vec::new();

    for file in files {
        // Mid-rebuild safety: only score items embedded under the active
        // fingerprint. Stale vectors from a prior chunk/model param set are
        // excluded until re-embedded (Codex P1).
        if let Some(fp) = active_fp {
            if file.fingerprint != fp {
                continue;
            }
        }
        let mut item_best: Option<SemanticHit> = None;
        for ch in &file.chunks {
            let score = cosine_similarity(&qvec, &ch.vector);
            let better = match &item_best {
                None => true,
                Some(h) => score > h.score,
            };
            if better {
                item_best = Some(SemanticHit {
                    item_id: file.item_id.clone(),
                    score,
                    best_ordinal: Some(ch.ordinal),
                    best_start_offset: Some(ch.start_offset),
                    best_end_offset: Some(ch.end_offset),
                });
            }
        }
        if let Some(h) = item_best {
            if query.min_score.map(|m| h.score >= m).unwrap_or(true) {
                best.push(h);
            }
        }
    }

    // Group already done (max per item). Sort → top_n_items.
    best.sort_by(|a, b| match b.score.partial_cmp(&a.score) {
        Some(ord) => ord.then_with(|| a.item_id.cmp(&b.item_id)),
        None => a.item_id.cmp(&b.item_id),
    });
    let top_n = query.top_n_items.max(1);
    if best.len() > top_n {
        best.truncate(top_n);
    }

    let eligible_count = eligible.len();

    Ok(SemanticSearchResult {
        hits: best,
        eligible_truncated,
        eligible_count,
    })
}

fn ensure_index_ready(meta: &SemanticMatterMeta, embedder: &dyn Embedder) -> Result<()> {
    if !meta.semantic_enabled {
        return Err(SemanticError::IndexNotBuilt);
    }
    let Some(active) = meta.semantic_model_id.as_deref() else {
        return Err(SemanticError::IndexNotBuilt);
    };
    if active != embedder.model_id() {
        return Err(SemanticError::ModelMismatch {
            embedder: embedder.model_id().to_string(),
            active: active.to_string(),
        });
    }
    if meta.semantic_dims.is_none() {
        return Err(SemanticError::IndexNotBuilt);
    }
    Ok(())
}
