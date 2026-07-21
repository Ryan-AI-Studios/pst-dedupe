//! Compose keyword FTS hits with metadata [`FilterSpec`] (track 0029 + 0028).

use matter_core::{FilterSpec, Matter, ReviewListRow};

use crate::error::Result;
use crate::query::{search_keyword_for_matter, KeywordHits, KeywordQuery, DEFAULT_FTS_FETCH_LIMIT};

/// Compose keyword search with a metadata filter for the Review list.
///
/// - `keyword` empty / `None` → metadata-only [`Matter::list_items_filtered_thin`]
/// - otherwise: FTS (matter-aware, pack stale gate) → unique ids → intersect filter
///
/// Returns `(count, rows)`.
pub fn compose_keyword_filter(
    matter: &Matter,
    _matter_root: &camino::Utf8Path,
    keyword: Option<&str>,
    filter: &FilterSpec,
    limit: u64,
    offset: u64,
) -> Result<(u64, Vec<ReviewListRow>)> {
    let kw = keyword.map(str::trim).filter(|s| !s.is_empty());
    let Some(qstr) = kw else {
        let count = matter.count_items_filtered(filter)?;
        let rows = matter.list_items_filtered_thin(filter, limit, offset)?;
        return Ok((count, rows));
    };

    let hits = search_keyword_for_matter(
        matter,
        &KeywordQuery {
            query: qstr.to_string(),
            limit: DEFAULT_FTS_FETCH_LIMIT,
            offset: 0,
        },
    )?;
    compose_with_hits(matter, filter, &hits, limit, offset)
}

/// Intersect precomputed FTS hits with a filter (test helper / advanced).
pub fn compose_with_hits(
    matter: &Matter,
    filter: &FilterSpec,
    hits: &KeywordHits,
    limit: u64,
    offset: u64,
) -> Result<(u64, Vec<ReviewListRow>)> {
    if hits.item_ids.is_empty() {
        return Ok((0, Vec::new()));
    }
    let count = matter.count_items_filtered_in_ids(filter, &hits.item_ids)?;
    let rows = matter.list_items_filtered_thin_in_ids(filter, &hits.item_ids, limit, offset)?;
    Ok((count, rows))
}
