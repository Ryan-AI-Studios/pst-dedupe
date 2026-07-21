//! Pure AI citation grounding verify (track 0052).
//!
//! Offsets are UTF-8 **byte** indices into the scanned text (entity-track style).
//! Compare uses whitespace-collapse + case-fold. Never panics on OOB offsets.
//!
//! Note: [`VERIFY_OFFSET_MISMATCH`](crate::VERIFY_OFFSET_MISMATCH) is reserved for
//! future intermediate reporting (offsets wrong but re-find may still succeed).
//! Current verify either repairs to `matched` or falls through to
//! `quote_not_found` — it does not emit `offset_mismatch` as a stored status.
//!
//! Lives in **matter-core** so Desk and accept-path can re-verify without depending
//! on the job crate (`matter-ai`).

use crate::{VERIFY_MATCHED, VERIFY_QUOTE_NOT_FOUND};

/// Result of verifying a citation quote against source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyCitationResult {
    pub status: String,
    /// Repaired UTF-8 byte start when matched (inclusive).
    pub start_offset: Option<i64>,
    /// Repaired UTF-8 byte end when matched (exclusive).
    pub end_offset: Option<i64>,
}

/// Verify a citation quote against `text`.
///
/// Rules (spec §3.3.1):
/// 1. Prefer offsets if in range and normalized slice matches → `matched`
/// 2. Else search for normalized quote; unique hit → `matched` with repaired offsets
/// 3. Ambiguous / missing / spliced ellipsis → `quote_not_found`
/// 4. OOB offsets alone → try re-search (do not crash)
pub fn verify_ai_citation_against_text(
    quote: &str,
    start_offset: Option<i64>,
    end_offset: Option<i64>,
    text: &str,
) -> VerifyCitationResult {
    let quote = quote.trim();
    if quote.is_empty() {
        return VerifyCitationResult {
            status: VERIFY_QUOTE_NOT_FOUND.into(),
            start_offset: None,
            end_offset: None,
        };
    }

    // Spliced ellipsis patterns: treat as invalid unless the ellipsis literally
    // appears in the source at the claimed span (checked via normalize path).
    if looks_like_splice_ellipsis(quote) {
        // Only accept if the full quote (including ellipsis) is a contiguous substring.
        if let Some((s, e)) = find_unique_normalized(quote, text) {
            return VerifyCitationResult {
                status: VERIFY_MATCHED.into(),
                start_offset: Some(s as i64),
                end_offset: Some(e as i64),
            };
        }
        return VerifyCitationResult {
            status: VERIFY_QUOTE_NOT_FOUND.into(),
            start_offset: None,
            end_offset: None,
        };
    }

    let norm_quote = normalize_for_verify(quote);
    if norm_quote.is_empty() {
        return VerifyCitationResult {
            status: VERIFY_QUOTE_NOT_FOUND.into(),
            start_offset: None,
            end_offset: None,
        };
    }

    // Prefer offsets when in range and normalized slice matches — but only if
    // the normalized quote is **unique** in the document (spec §3.3.1). Offsets
    // that land on one of several case/whitespace-equivalent hits are ambiguous.
    if let (Some(start), Some(end)) = (start_offset, end_offset) {
        if start >= 0 && end > start {
            let s = start as usize;
            let e = end as usize;
            if e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e) {
                let slice = &text[s..e];
                if normalize_for_verify(slice) == norm_quote {
                    if is_normalized_quote_unique(quote, text, s, e) {
                        return VerifyCitationResult {
                            status: VERIFY_MATCHED.into(),
                            start_offset: Some(start),
                            end_offset: Some(end),
                        };
                    }
                    // Ambiguous under normalize — do not invent a unique match.
                    return VerifyCitationResult {
                        status: VERIFY_QUOTE_NOT_FOUND.into(),
                        start_offset: None,
                        end_offset: None,
                    };
                }
                // Offset mismatch — re-search; unique hit → matched, else not found.
                // (VERIFY_OFFSET_MISMATCH is reserved/unused as a stored status.)
                if let Some((rs, re)) = find_unique_normalized(quote, text) {
                    return VerifyCitationResult {
                        status: VERIFY_MATCHED.into(),
                        start_offset: Some(rs as i64),
                        end_offset: Some(re as i64),
                    };
                }
                // Spec: missing after re-search → quote_not_found (not stuck on mismatch).
                return VerifyCitationResult {
                    status: VERIFY_QUOTE_NOT_FOUND.into(),
                    start_offset: Some(start),
                    end_offset: Some(end),
                };
            }
            // OOB → re-search below.
        }
    }

    if let Some((s, e)) = find_unique_normalized(quote, text) {
        return VerifyCitationResult {
            status: VERIFY_MATCHED.into(),
            start_offset: Some(s as i64),
            end_offset: Some(e as i64),
        };
    }

    VerifyCitationResult {
        status: VERIFY_QUOTE_NOT_FOUND.into(),
        start_offset: None,
        end_offset: None,
    }
}

/// Verify a citation for **storage** against the full continuous body.
///
/// When `prepared_was_truncated` is true, the model saw middle-dropped prompt text
/// and any offsets it returned are in prepared space — ignore them and re-find
/// the quote only in `full_text` so stored offsets match Desk's CAS body.
pub fn verify_citation_for_storage(
    quote: &str,
    start_offset: Option<i64>,
    end_offset: Option<i64>,
    full_text: &str,
    prepared_was_truncated: bool,
) -> VerifyCitationResult {
    if prepared_was_truncated {
        verify_ai_citation_against_text(quote, None, None, full_text)
    } else {
        verify_ai_citation_against_text(quote, start_offset, end_offset, full_text)
    }
}

/// Collapse whitespace runs + ASCII case-fold for verify compare.
pub fn normalize_for_verify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            prev_space = false;
            for c in ch.to_lowercase() {
                out.push(c);
            }
        }
    }
    out.trim().to_string()
}

/// True when quote uses `...` / `…` as a join (likely non-contiguous splice).
fn looks_like_splice_ellipsis(quote: &str) -> bool {
    // Interior ellipsis (not leading/trailing only) is treated as splice candidate.
    let t = quote.trim();
    if let Some(idx) = t.find("...") {
        // Not pure leading/trailing.
        if idx > 0 && idx + 3 < t.len() {
            return true;
        }
    }
    if let Some(idx) = t.find('…') {
        if idx > 0 && idx + '…'.len_utf8() < t.len() {
            return true;
        }
    }
    false
}

/// Find a unique occurrence of `quote` in `text` via normalized sliding window
/// over original char-boundary substrings. Returns UTF-8 **byte** range.
///
/// Uniqueness is always evaluated in **normalized** space (case-fold + whitespace
/// collapse). An exact-case unique substring is still rejected when another
/// same-length case-variant also normalizes to the same form
/// (e.g. `"hot HOT"` + quote `"hot"`).
fn find_unique_normalized(quote: &str, text: &str) -> Option<(usize, usize)> {
    let norm_quote = normalize_for_verify(quote);
    if norm_quote.is_empty() || text.is_empty() {
        return None;
    }

    // Fast path: exact byte substring (case-sensitive).
    // Uniqueness must still hold under full normalized space (case + whitespace).
    if let Some(pos) = text.find(quote) {
        if text[pos + quote.len()..].find(quote).is_none() {
            let end = pos + quote.len();
            if !is_normalized_quote_unique(quote, text, pos, end) {
                return None; // e.g. "hot"/"HOT" or "foo bar"/"foo  bar"
            }
            return Some((pos, end));
        }
        // Multiple exact hits → ambiguous (do not pick arbitrarily).
        return None;
    }

    // Normalized search: collect all spans whose normalized form equals norm_quote.
    // Strategy: for each byte start at char boundary, expand to cover enough
    // content that normalized length ≥ quote norm length, then check equality.
    // Only start at non-whitespace (or text start) so leading spaces do not
    // create a second hit for the same token via trim.
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    if boundaries.len() < 2 {
        return None;
    }

    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (i, &start) in boundaries
        .iter()
        .enumerate()
        .take(boundaries.len().saturating_sub(1))
    {
        // Skip starts that are whitespace — normalize would trim them and
        // double-count the following token.
        if text[start..]
            .chars()
            .next()
            .map(|c| c.is_whitespace())
            .unwrap_or(false)
        {
            continue;
        }
        // Expand end until normalized length reaches or exceeds target.
        for &end in boundaries.iter().skip(i + 1) {
            let slice = &text[start..end];
            let n = normalize_for_verify(slice);
            if n.len() < norm_quote.len() {
                continue;
            }
            if n == norm_quote {
                hits.push((start, end));
                break; // shortest match from this start
            }
            // Once we overshoot length without match, further expansion only grows.
            if n.len() > norm_quote.len() && !n.starts_with(&norm_quote) {
                break;
            }
            if n.len() > norm_quote.len() {
                break;
            }
        }
        if hits.len() > 1 {
            return None; // ambiguous
        }
    }

    if hits.len() == 1 {
        Some(hits[0])
    } else {
        None
    }
}

/// True when the span `[known_start, known_end)` is the **only** normalized
/// occurrence of `quote` in `text` (case/whitespace-tolerant).
fn is_normalized_quote_unique(
    quote: &str,
    text: &str,
    known_start: usize,
    known_end: usize,
) -> bool {
    let norm_quote = normalize_for_verify(quote);
    if norm_quote.is_empty() {
        return false;
    }
    // Exact same-string duplicates elsewhere?
    if let Some(pos) = text.find(quote) {
        if pos != known_start {
            return false;
        }
        if text[pos + quote.len()..].find(quote).is_some() {
            return false;
        }
    }
    // Case/whitespace-variant duplicates (normalized space).
    !has_same_charlen_casefold_duplicate(quote, text, known_start, &norm_quote)
        && !has_normalized_whitespace_variant_duplicate(
            quote,
            text,
            known_start,
            known_end,
            &norm_quote,
        )
}

/// True when another window (possibly different raw length due to whitespace)
/// normalizes to the same form as `quote`.
fn has_normalized_whitespace_variant_duplicate(
    quote: &str,
    text: &str,
    known_start: usize,
    known_end: usize,
    norm_quote: &str,
) -> bool {
    let _ = (quote, known_end);
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    for (i, &start) in boundaries
        .iter()
        .enumerate()
        .take(boundaries.len().saturating_sub(1))
    {
        if start == known_start {
            continue;
        }
        if text[start..]
            .chars()
            .next()
            .map(|c| c.is_whitespace())
            .unwrap_or(false)
        {
            continue;
        }
        for &end in boundaries.iter().skip(i + 1) {
            let slice = &text[start..end];
            let n = normalize_for_verify(slice);
            if n.len() < norm_quote.len() {
                continue;
            }
            if n == norm_quote {
                return true;
            }
            if n.len() > norm_quote.len() {
                break;
            }
        }
    }
    false
}

/// True when another same-char-length window normalizes to `norm_quote`
/// (detects mixed-case duplicates the exact path would otherwise miss).
fn has_same_charlen_casefold_duplicate(
    quote: &str,
    text: &str,
    known_pos: usize,
    norm_quote: &str,
) -> bool {
    let q_chars = quote.chars().count();
    if q_chars == 0 {
        return false;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for i in 0..chars.len() {
        let start = chars[i].0;
        if start == known_pos {
            continue;
        }
        let end = if i + q_chars < chars.len() {
            chars[i + q_chars].0
        } else if i + q_chars == chars.len() {
            text.len()
        } else {
            continue;
        };
        let slice = &text[start..end];
        if normalize_for_verify(slice) == norm_quote {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VERIFY_MATCHED;
    use crate::VERIFY_QUOTE_NOT_FOUND;

    #[test]
    fn matched_exact_offsets() {
        let text = "Hello world, this is hot material.";
        let quote = "this is hot";
        let start = text.find(quote).expect("pos") as i64;
        let end = start + quote.len() as i64;
        let r = verify_ai_citation_against_text(quote, Some(start), Some(end), text);
        assert_eq!(r.status, VERIFY_MATCHED);
        assert_eq!(r.start_offset, Some(start));
        assert_eq!(r.end_offset, Some(end));
    }

    #[test]
    fn mismatch_refinds_unique() {
        let text = "prefix this is hot suffix";
        let quote = "this is hot";
        // Wrong offsets.
        let r = verify_ai_citation_against_text(quote, Some(0), Some(4), text);
        assert_eq!(r.status, VERIFY_MATCHED);
        let expected = text.find(quote).expect("pos");
        assert_eq!(r.start_offset, Some(expected as i64));
        assert_eq!(r.end_offset, Some((expected + quote.len()) as i64));
    }

    #[test]
    fn not_found() {
        let text = "nothing relevant here";
        let r = verify_ai_citation_against_text("invented quote", Some(0), Some(5), text);
        assert_eq!(r.status, VERIFY_QUOTE_NOT_FOUND);
    }

    #[test]
    fn whitespace_case_normalize() {
        let text = "We will Terminate   the Agreement soon.";
        let quote = "we will terminate the agreement";
        let r = verify_ai_citation_against_text(quote, None, None, text);
        assert_eq!(r.status, VERIFY_MATCHED);
        assert!(r.start_offset.is_some());
        assert!(r.end_offset.is_some());
    }

    #[test]
    fn spliced_ellipsis_not_found() {
        let text = "First sentence about cats. Later we discuss dogs and birds.";
        let quote = "First sentence about cats... discuss dogs";
        let r = verify_ai_citation_against_text(quote, None, None, text);
        assert_eq!(r.status, VERIFY_QUOTE_NOT_FOUND);
    }

    #[test]
    fn long_quote_still_verifies() {
        let words: Vec<String> = (0..80).map(|i| format!("word{i}")).collect();
        let text = words.join(" ");
        let quote = words[10..40].join(" ");
        let r = verify_ai_citation_against_text(&quote, None, None, &text);
        assert_eq!(r.status, VERIFY_MATCHED);
        assert!(r.start_offset.is_some());
    }

    #[test]
    fn oob_offsets_researches() {
        let text = "abc hot def";
        let r = verify_ai_citation_against_text("hot", Some(999), Some(1005), text);
        assert_eq!(r.status, VERIFY_MATCHED);
        assert_eq!(r.start_offset, Some(text.find("hot").unwrap() as i64));
    }

    #[test]
    fn literal_ellipsis_in_source_matches() {
        let text = "see section 1...2 for details";
        let quote = "section 1...2";
        let r = verify_ai_citation_against_text(quote, None, None, text);
        assert_eq!(r.status, VERIFY_MATCHED);
    }

    #[test]
    fn empty_quote_is_not_found() {
        let text = "some body text";
        let r = verify_ai_citation_against_text("", Some(0), Some(4), text);
        assert_eq!(r.status, VERIFY_QUOTE_NOT_FOUND);
        assert!(r.start_offset.is_none());
        let r2 = verify_ai_citation_against_text("   ", None, None, text);
        assert_eq!(r2.status, VERIFY_QUOTE_NOT_FOUND);
    }

    #[test]
    fn ambiguous_multi_hit_is_not_found() {
        let text = "alpha hot beta hot gamma";
        let r = verify_ai_citation_against_text("hot", None, None, text);
        assert_eq!(
            r.status, VERIFY_QUOTE_NOT_FOUND,
            "duplicate exact hits must not pick an arbitrary span"
        );
        assert!(r.start_offset.is_none());
        assert!(r.end_offset.is_none());
    }

    #[test]
    fn mixed_case_ambiguous_is_not_found() {
        // P2-1: uniqueness is in normalized space — exact unique "hot" still
        // collides with "HOT" under case-fold.
        let text = "hot HOT";
        let r = verify_ai_citation_against_text("hot", None, None, text);
        assert_eq!(
            r.status, VERIFY_QUOTE_NOT_FOUND,
            "case-variant duplicates must be ambiguous"
        );
        assert!(r.start_offset.is_none());
        assert!(r.end_offset.is_none());
    }

    #[test]
    fn offset_fast_path_rejects_mixed_case_ambiguity() {
        // Provider supplies offsets into the first hit; second case-variant still
        // makes the quote non-unique under normalize.
        let text = "hot HOT";
        let quote = "hot";
        let start = 0i64;
        let end = 3i64;
        let r = verify_ai_citation_against_text(quote, Some(start), Some(end), text);
        assert_eq!(
            r.status, VERIFY_QUOTE_NOT_FOUND,
            "offset prefer must not bypass normalized uniqueness"
        );
    }

    #[test]
    fn offset_fast_path_rejects_whitespace_variant_ambiguity() {
        let text = "foo bar foo  bar";
        let quote = "foo bar";
        let start = text.find(quote).expect("first") as i64;
        let end = start + quote.len() as i64;
        let r = verify_ai_citation_against_text(quote, Some(start), Some(end), text);
        assert_eq!(
            r.status, VERIFY_QUOTE_NOT_FOUND,
            "whitespace-collapsed duplicates must be ambiguous even with offsets"
        );
    }

    #[test]
    fn no_offset_rejects_whitespace_variant_ambiguity() {
        // Exact fast path must not accept when another whitespace-variant hit exists.
        let text = "foo bar foo  bar";
        let quote = "foo bar";
        let r = verify_ai_citation_against_text(quote, None, None, text);
        assert_eq!(
            r.status, VERIFY_QUOTE_NOT_FOUND,
            "no-offset path must reject whitespace-variant ambiguity"
        );
        assert!(r.start_offset.is_none());
    }

    #[test]
    fn storage_verify_full_text_when_middle_drop_applied() {
        // Large doc: distinctive quote only in the true tail (beyond default prompt cap).
        let head = "HEAD_PREFIX_AAA ".repeat(300); // ~4.8k
        let mid = "MIDDLE_FILL_XXX ".repeat(2_000); // ~32k
        let tail = " END_UNIQUE_QUOTE_HOT_MATERIAL confidential.";
        let full = format!("{head}{mid}{tail}");
        assert!(full.len() > 8_000);

        // Simulate prepared-space offsets (small indices into huge full body).
        let quote = "END_UNIQUE_QUOTE_HOT_MATERIAL";
        let prepared_pos = 100i64; // deliberate wrong full-space offset
        let wrong_full_end = prepared_pos + quote.len() as i64;

        let r = verify_citation_for_storage(
            quote,
            Some(prepared_pos),
            Some(wrong_full_end),
            &full,
            true, // prepared_was_truncated
        );
        assert_eq!(r.status, VERIFY_MATCHED);
        let expected = full.find(quote).expect("quote in full") as i64;
        assert_eq!(r.start_offset, Some(expected));
        assert_eq!(r.end_offset, Some(expected + quote.len() as i64));
        assert_ne!(r.start_offset, Some(prepared_pos));
    }

    #[test]
    fn storage_verify_uses_offsets_when_not_truncated() {
        let text = "prefix this is hot suffix";
        let quote = "this is hot";
        let start = text.find(quote).expect("pos") as i64;
        let end = start + quote.len() as i64;
        let r = verify_citation_for_storage(quote, Some(start), Some(end), text, false);
        assert_eq!(r.status, VERIFY_MATCHED);
        assert_eq!(r.start_offset, Some(start));
        assert_eq!(r.end_offset, Some(end));
    }
}
