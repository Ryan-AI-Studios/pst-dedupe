//! Keyword search over a matter Tantivy index.

use std::collections::HashSet;
use std::path::Path;

use camino::Utf8Path;
use matter_core::Matter;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{TantivyDocument, Value};
use tantivy::Index;

use crate::error::{Result, SearchError, CODE_FTS_LANG_PACK_STALE};
use crate::index::{register_pack_tokenizers, stored_item_id, MatterIndex};
use crate::pack::LangPack;
use crate::schema::FtsSchema;
use crate::tokenizer::rewrite_cjk_query_phrases;

/// Default max FTS hit ids fetched for Review compose / large queries.
///
/// Beyond this window, results are truncated (documented; streaming deferred).
pub const DEFAULT_FTS_FETCH_LIMIT: usize = 50_000;

/// Keyword query parameters.
#[derive(Debug, Clone)]
pub struct KeywordQuery {
    pub query: String,
    pub limit: usize,
    pub offset: usize,
}

/// Unique item ids from a keyword search (de-duped).
#[derive(Debug, Clone, Default)]
pub struct KeywordHits {
    pub item_ids: Vec<String>,
    /// Approximate total hits before limit (if available).
    pub total_approx: Option<u64>,
}

/// Matter-aware keyword search with language-pack stale gate (production path).
///
/// 1. Load matter lang config
/// 2. Compute expected fingerprint for active pack
/// 3. If index missing → [`SearchError::IndexMissing`]
/// 4. If fingerprint is `None` or ≠ expected → [`SearchError::LangPackStale`]
///    (hard fail — never query mismatched index)
/// 5. Open index with pack tokenizer registered
/// 6. For `cjk_ngram_v1`, rewrite consecutive CJK runs to Tantivy phrases
/// 7. Parse + search
pub fn search_keyword_for_matter(matter: &Matter, q: &KeywordQuery) -> Result<KeywordHits> {
    let lang = matter.get_lang_config()?;
    let pack = LangPack::parse(&lang.lang_pack_id)?;
    if lang.lang_pack_version != pack.version() {
        return Err(SearchError::LangPackStale(format!(
            "lang_pack_version {} != pack {} version {} ({CODE_FTS_LANG_PACK_STALE})",
            lang.lang_pack_version,
            pack.id(),
            pack.version()
        )));
    }
    let expected = pack.fingerprint();

    let index_dir = MatterIndex::index_dir(matter.root());
    if !index_dir.as_std_path().exists() {
        return Err(SearchError::IndexMissing);
    }
    if is_effectively_empty(index_dir.as_std_path())? {
        return Err(SearchError::IndexMissing);
    }

    match lang.fts_lang_fingerprint.as_deref() {
        Some(fp) if fp == expected.as_str() => {}
        Some(fp) => {
            return Err(SearchError::LangPackStale(format!(
                "stored fingerprint '{fp}' != expected '{expected}' for pack {}",
                pack.id()
            )));
        }
        None => {
            return Err(SearchError::LangPackStale(format!(
                "no fts_lang_fingerprint; rebuild FTS for pack {} ({CODE_FTS_LANG_PACK_STALE})",
                pack.id()
            )));
        }
    }

    let dek = matter.dek_arc();
    let handle = MatterIndex::open_or_create_with_crypto(
        matter.root(),
        pack,
        dek.as_deref(),
        matter.crypto_chunk_bytes(),
    )?;
    let hits = search_index_with_pack(handle.index(), pack, q)?;
    handle.shutdown();
    Ok(hits)
}

/// Low-level search by path (latin_default tokenizer registration only).
///
/// Prefer [`search_keyword_for_matter`] for production (stale gate + pack).
pub fn search_keyword(matter_root: &Utf8Path, q: &KeywordQuery) -> Result<KeywordHits> {
    search_keyword_with_pack(matter_root, LangPack::LatinDefault, q)
}

/// Search by path with an explicit pack (no SQLite fingerprint gate).
///
/// Used by tests that manage fingerprints manually, and as a low-level path.
pub fn search_keyword_with_pack(
    matter_root: &Utf8Path,
    pack: LangPack,
    q: &KeywordQuery,
) -> Result<KeywordHits> {
    let index_dir = MatterIndex::index_dir(matter_root);
    if !index_dir.as_std_path().exists() {
        return Err(SearchError::IndexMissing);
    }
    if is_effectively_empty(index_dir.as_std_path())? {
        return Err(SearchError::IndexMissing);
    }

    let index = Index::open_in_dir(index_dir.as_std_path()).map_err(|e| {
        SearchError::Index(format!(
            "failed to open FTS index at {index_dir}: {e} — try Rebuild index"
        ))
    })?;
    register_pack_tokenizers(&index, pack)?;
    search_index_with_pack(&index, pack, q)
}

/// Search an already-open [`Index`] with the latin_default query path.
pub fn search_index(index: &Index, q: &KeywordQuery) -> Result<KeywordHits> {
    search_index_with_pack(index, LangPack::LatinDefault, q)
}

/// Search an open index with pack-aware query rewrite.
pub fn search_index_with_pack(
    index: &Index,
    pack: LangPack,
    q: &KeywordQuery,
) -> Result<KeywordHits> {
    let mut query_str = q.query.trim().to_string();
    if query_str.is_empty() {
        return Ok(KeywordHits::default());
    }

    if pack.is_cjk() {
        query_str = rewrite_cjk_query_phrases(&query_str);
    }

    let fts = FtsSchema::build_for_pack(pack);
    // Prefer schema from the open index (field ids must match).
    let schema = index.schema();
    let item_id_field = schema.get_field("item_id").map_err(|_| {
        SearchError::Index("index schema missing item_id — rebuild required".into())
    })?;
    let subject = schema
        .get_field("subject")
        .map_err(|_| SearchError::Index("index schema missing subject".into()))?;
    let body = schema
        .get_field("body")
        .map_err(|_| SearchError::Index("index schema missing body".into()))?;
    let path = schema
        .get_field("path")
        .map_err(|_| SearchError::Index("index schema missing path".into()))?;
    let attach_names = schema
        .get_field("attach_names")
        .map_err(|_| SearchError::Index("index schema missing attach_names".into()))?;

    let mut parser = QueryParser::for_index(index, vec![subject, body, path, attach_names]);
    // Multi-term default: AND (Tantivy default is OR in some versions — set conjunction).
    parser.set_conjunction_by_default();

    let query = parser
        .parse_query(&query_str)
        .map_err(|e| SearchError::InvalidQuery(format!("{e}")))?;

    let reader = index
        .reader_builder()
        .reload_policy(tantivy::ReloadPolicy::Manual)
        .try_into()?;
    reader.reload()?;
    let searcher = reader.searcher();

    let num_docs = searcher.num_docs();
    if num_docs == 0 {
        return Err(SearchError::IndexMissing);
    }

    // Fetch enough docs to cover offset + limit after de-dupe.
    // Cap at DEFAULT_FTS_FETCH_LIMIT (50k) so Review compose ∩ filter can see
    // the full requested candidate window — not an undocumented 10k clamp.
    let need = q.limit.saturating_add(q.offset).max(1);
    let desired = need.saturating_mul(2).max(need.saturating_add(64));
    let fetch = desired
        .min(DEFAULT_FTS_FETCH_LIMIT)
        .min((num_docs as usize).max(1))
        .max(1);
    let top = searcher.search(&query, &TopDocs::with_limit(fetch).order_by_score())?;

    let mut seen = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();
    for (_score, addr) in top {
        let doc: TantivyDocument = searcher.doc(addr)?;
        let Some(id) = doc
            .get_first(item_id_field)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
        else {
            // Fallback via helper with rebuilt schema field (may not match).
            if let Some(id) = stored_item_id(&fts, &doc) {
                if seen.insert(id.clone()) {
                    ordered.push(id);
                }
            }
            continue;
        };
        if seen.insert(id.clone()) {
            ordered.push(id);
        }
    }

    // Approximate unique hits in the fetch window (not global count if capped).
    let total_approx = Some(ordered.len() as u64);
    let item_ids = ordered.into_iter().skip(q.offset).take(q.limit).collect();

    Ok(KeywordHits {
        item_ids,
        total_approx,
    })
}

fn is_effectively_empty(path: &Path) -> Result<bool> {
    let mut entries = std::fs::read_dir(path)?;
    // Only meta.json without segments still counts as openable but empty-ish;
    // we treat "no files" as missing.
    Ok(entries.next().is_none())
}
