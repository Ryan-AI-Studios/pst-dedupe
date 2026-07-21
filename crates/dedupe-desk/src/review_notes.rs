//! Notes / highlights helpers for the Review screen (track 0030).
//!
//! Pure builders for highlight create inputs and body layout paint — unit-tested
//! without egui interaction.

use eframe::egui::{text::LayoutJob, text::TextFormat, text::TextWrapping, Color32, FontId};
use matter_core::{
    display_body_digest, highlight_status, utf8_char_slice, CreateHighlightInput, ItemHighlight,
    ResolvedHighlight, UpsertNoteInput,
};

/// Yellow paint for active user highlights.
pub const HIGHLIGHT_PAINT: Color32 = Color32::from_rgb(0xFF, 0xF5, 0x9D);
/// Gray paint for stale ranges (optional dashed-like dim).
pub const HIGHLIGHT_STALE_PAINT: Color32 = Color32::from_rgb(0xE0, 0xE0, 0xE0);
/// Light blue temporary paint for AI citation (track 0052).
pub const AI_CITATION_PAINT: Color32 = Color32::from_rgb(0xBB, 0xDE, 0xFB);

/// Char-range selection on the display body (`start` inclusive, `end` exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodySelection {
    pub start: usize,
    pub end: usize,
}

impl BodySelection {
    pub fn new(start: usize, end: usize) -> Option<Self> {
        if end > start {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn quote<'a>(&self, body: &'a str) -> Option<&'a str> {
        utf8_char_slice(body, self.start, self.end)
    }
}

/// Build a [`CreateHighlightInput`] from a body selection (desk → matter-core).
pub fn highlight_input_from_selection(
    item_id: &str,
    body: &str,
    body_digest: &str,
    sel: BodySelection,
    actor: &str,
    color: Option<String>,
) -> Result<CreateHighlightInput, String> {
    let quote = sel
        .quote(body)
        .ok_or_else(|| "selection does not map to a valid body slice".to_string())?;
    if quote.trim().is_empty() {
        return Err("selection is empty or whitespace-only".into());
    }
    let digest = if body_digest.trim().is_empty() {
        display_body_digest(body)
    } else {
        body_digest.to_string()
    };
    Ok(CreateHighlightInput {
        item_id: item_id.to_string(),
        start_utf8: sel.start as i64,
        end_utf8: sel.end as i64,
        exact_quote: quote.to_string(),
        display_body: body.to_string(),
        body_digest: digest,
        color,
        actor: actor.to_string(),
    })
}

/// Prefer item `text_sha256`, then `html_sha256`, else synthetic digest of display text.
///
/// Aligns with redaction regenerate bookkeeping: when plain text CAS is absent,
/// `redacted_source_digest` is stored as `html_sha256` (track 0032).
pub fn body_digest_for_item(
    text_sha256: Option<&str>,
    html_sha256: Option<&str>,
    display_body: &str,
) -> String {
    if let Some(t) = text_sha256.map(str::trim).filter(|s| !s.is_empty()) {
        return t.to_string();
    }
    if let Some(h) = html_sha256.map(str::trim).filter(|s| !s.is_empty()) {
        return h.to_string();
    }
    display_body_digest(display_body)
}

/// Convert UTF-8 **byte** offsets to char indices for body paint/selection.
///
/// AI citations store entity-style byte offsets; egui body paint uses char indices.
pub fn byte_offsets_to_char_range(
    text: &str,
    start_byte: i64,
    end_byte: i64,
) -> Option<(usize, usize)> {
    if start_byte < 0 || end_byte <= start_byte {
        return None;
    }
    let s = start_byte as usize;
    let e = end_byte as usize;
    if e > text.len() || !text.is_char_boundary(s) || !text.is_char_boundary(e) {
        return None;
    }
    let start_char = text[..s].chars().count();
    let end_char = start_char + text[s..e].chars().count();
    if end_char > start_char {
        Some((start_char, end_char))
    } else {
        None
    }
}

/// Pure helper: citation click → highlight char range (unit-testable).
///
/// Returns `None` when offsets are missing or OOB (do not invent a highlight).
pub fn citation_highlight_target(
    text: &str,
    start_offset: Option<i64>,
    end_offset: Option<i64>,
    verify_status: &str,
) -> Option<(usize, usize)> {
    if verify_status != "matched" {
        return None;
    }
    let start = start_offset?;
    let end = end_offset?;
    byte_offsets_to_char_range(text, start, end)
}

/// Re-verify one citation against the current body text (Desk display space).
///
/// When `text_sha_stale` is true (suggestion fingerprint ≠ current item
/// `text_sha256`), stored offsets are ignored and the quote is re-found only.
/// Returns repaired status + offsets for in-memory UI (badge / highlight).
pub fn reverify_citation_against_body(
    quote: &str,
    start_offset: Option<i64>,
    end_offset: Option<i64>,
    body: &str,
    text_sha_stale: bool,
) -> (String, Option<i64>, Option<i64>) {
    let (so, eo) = if text_sha_stale {
        (None, None)
    } else {
        (start_offset, end_offset)
    };
    let v = matter_ai::verify_ai_citation_against_text(quote, so, eo, body);
    (v.status, v.start_offset, v.end_offset)
}

/// Approximate vertical scroll offset (pixels) so that `char_off` is near the top
/// of a monospace body viewer.
///
/// Counts newlines before `char_off` and multiplies by `line_height`. Does **not**
/// account for soft wrap; the Review UI prefers galley `pos_from_cursor` when a
/// layout is available, and falls back to this for ScrollArea offset wiring.
pub fn approx_scroll_y_for_char(text: &str, char_off: usize, line_height: f32) -> f32 {
    if line_height <= 0.0 || text.is_empty() {
        return 0.0;
    }
    let line = text.chars().take(char_off).filter(|c| *c == '\n').count();
    (line as f32) * line_height
}

/// Build a monospace [`LayoutJob`] with yellow backgrounds on active resolved ranges.
///
/// Optional `ai_cite` is a temporary char-range paint for AI citation highlight (0052).
pub fn body_layout_job(body: &str, resolved: &[ResolvedHighlight]) -> LayoutJob {
    body_layout_job_ex(body, resolved, None)
}

/// Like [`body_layout_job`] with optional temporary AI citation paint (char range).
pub fn body_layout_job_ex(
    body: &str,
    resolved: &[ResolvedHighlight],
    ai_cite: Option<(usize, usize)>,
) -> LayoutJob {
    // kind: 0=active hl, 1=stale hl, 2=ai citation
    let mut ranges: Vec<(usize, usize, u8)> = resolved
        .iter()
        .filter(|r| r.end_utf8 > r.start_utf8 && r.start_utf8 >= 0)
        .map(|r| {
            (
                r.start_utf8 as usize,
                r.end_utf8 as usize,
                if r.status == "active" { 0u8 } else { 1u8 },
            )
        })
        .collect();
    if let Some((s, e)) = ai_cite {
        if e > s {
            ranges.push((s, e, 2));
        }
    }
    ranges.sort_by_key(|(s, _, _)| *s);

    // Merge/paint non-overlapping by walking char indices (skip overlaps: first wins).
    let char_len = body.chars().count();
    let mut job = LayoutJob {
        wrap: TextWrapping {
            max_width: f32::INFINITY,
            ..Default::default()
        },
        ..Default::default()
    };
    let font = FontId::monospace(13.0);
    let base = TextFormat {
        font_id: font.clone(),
        color: Color32::PLACEHOLDER,
        ..Default::default()
    };

    if ranges.is_empty() {
        job.append(body, 0.0, base);
        return job;
    }

    let chars: Vec<char> = body.chars().collect();
    let mut i = 0usize;
    let mut ri = 0usize;
    while i < char_len {
        // Skip past ranges that end before i.
        while ri < ranges.len() && ranges[ri].1 <= i {
            ri += 1;
        }
        if ri < ranges.len() && ranges[ri].0 <= i && i < ranges[ri].1 {
            let (rs, re, kind) = ranges[ri];
            let start = i.max(rs);
            let end = re.min(char_len);
            if start < end {
                let slice: String = chars[start..end].iter().collect();
                let mut fmt = base.clone();
                fmt.background = match kind {
                    0 => HIGHLIGHT_PAINT,
                    2 => AI_CITATION_PAINT,
                    _ => HIGHLIGHT_STALE_PAINT,
                };
                job.append(&slice, 0.0, fmt);
                i = end;
            } else {
                i += 1;
            }
            continue;
        }
        // Plain run until next range or end.
        let next = ranges
            .get(ri)
            .map(|(s, _, _)| *s)
            .unwrap_or(char_len)
            .max(i + 1);
        let end = next.min(char_len);
        if i < end {
            let slice: String = chars[i..end].iter().collect();
            job.append(&slice, 0.0, base.clone());
        }
        i = end;
    }
    job
}

/// Whether digit coding shortcuts should fire given focus + note-editor flag.
///
/// `no_widget_focus` is `ctx.memory(|m| m.focused().is_none())`.
/// `note_editor_focused` is true when a notes panel TextEdit has focus.
pub fn focus_allows_coding_shortcuts(no_widget_focus: bool, note_editor_focused: bool) -> bool {
    no_widget_focus && !note_editor_focused
}

/// Build resolved paint list from stored highlights + current body (in-memory).
///
/// UI banners / header stale counts / list labels **must** use this (or
/// [`count_stale_resolved`]) rather than raw SQLite `ItemHighlight.status`,
/// which can lag until optional `persist_stale` runs.
pub fn resolve_for_paint(
    highlights: &[ItemHighlight],
    body: &str,
    digest: &str,
) -> Vec<ResolvedHighlight> {
    highlights
        .iter()
        .map(|hl| matter_core::resolve_highlight_against_body(hl, body, digest))
        .collect()
}

/// Count highlights whose **resolved** status is stale (body re-resolve failed).
pub fn count_stale_resolved(resolved: &[ResolvedHighlight]) -> usize {
    resolved
        .iter()
        .filter(|r| r.status == highlight_status::STALE)
        .count()
}

/// Prefer in-memory resolve status for UI labels; fall back to stored row when
/// the display body is not ready yet.
pub fn highlight_ui_status<'a>(
    hl: &'a ItemHighlight,
    resolved: Option<&'a ResolvedHighlight>,
) -> &'a str {
    resolved
        .map(|r| r.status.as_str())
        .unwrap_or(hl.status.as_str())
}

/// Look up a paint-ready resolve row by highlight id.
pub fn find_resolved<'a>(
    resolved: &'a [ResolvedHighlight],
    highlight_id: &str,
) -> Option<&'a ResolvedHighlight> {
    resolved.iter().find(|r| r.highlight_id == highlight_id)
}

/// Stale count for header / banner: use re-resolve when body is ready, else
/// provisional stored SQLite status.
pub fn stale_count_for_ui(
    highlights: &[ItemHighlight],
    resolved: Option<&[ResolvedHighlight]>,
) -> usize {
    match resolved {
        Some(r) => count_stale_resolved(r),
        None => highlights
            .iter()
            .filter(|h| h.status == highlight_status::STALE)
            .count(),
    }
}

/// egui TextEdit layouter factory is not stored here — paint uses [`body_layout_job`].
pub fn body_job_for_ui(body: &str, resolved: &[ResolvedHighlight], wrap_width: f32) -> LayoutJob {
    body_job_for_ui_ex(body, resolved, wrap_width, None)
}

/// Like [`body_job_for_ui`] with optional AI citation char-range paint.
pub fn body_job_for_ui_ex(
    body: &str,
    resolved: &[ResolvedHighlight],
    wrap_width: f32,
    ai_cite: Option<(usize, usize)>,
) -> LayoutJob {
    let mut job = match ai_cite {
        None => body_layout_job(body, resolved),
        Some(range) => body_layout_job_ex(body, resolved, Some(range)),
    };
    job.wrap.max_width = wrap_width;
    job
}

/// Extract non-empty selection from egui char range.
pub fn selection_from_char_range(range: std::ops::Range<usize>) -> Option<BodySelection> {
    BodySelection::new(range.start, range.end)
}

/// Build a create-note [`UpsertNoteInput`] from desk draft text + optional passage highlight.
///
/// Rejects empty/whitespace bodies (mirrors matter-core). Never invents synthetic
/// `"Note on: …"` placeholder text — callers must pass attorney-entered `draft_body`.
pub fn note_upsert_from_draft(
    item_id: &str,
    draft_body: &str,
    pending_highlight_id: Option<&str>,
    actor: &str,
) -> Result<UpsertNoteInput, String> {
    let body = draft_body.trim();
    if body.is_empty() {
        return Err("note body cannot be empty or whitespace-only".into());
    }
    Ok(UpsertNoteInput {
        id: None,
        item_id: item_id.to_string(),
        body: body.to_string(),
        highlight_id: pending_highlight_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        actor: actor.to_string(),
    })
}

/// Hint-only quote snippet for the passage-note draft editor (not persisted).
pub fn passage_note_hint_from_quote(exact_quote: &str) -> String {
    let snippet: String = exact_quote.chars().take(80).collect();
    if snippet.is_empty() {
        "Type a passage note linked to the highlight…".into()
    } else {
        format!("Passage note on “{snippet}”…")
    }
}

/// Find an existing highlight for a body selection among **active resolved** ranges.
///
/// Reuse is only valid when a highlight resolves `active` against the current
/// body/digest **and** its resolved range (stored or remapped) equals `sel`.
/// A stale highlight that still has matching stored offsets must not be reused —
/// callers should create a new highlight instead.
pub fn find_highlight_for_selection<'a>(
    highlights: &'a [ItemHighlight],
    resolved: &[ResolvedHighlight],
    sel: BodySelection,
) -> Option<&'a ItemHighlight> {
    let start = sel.start as i64;
    let end = sel.end as i64;
    let id = resolved
        .iter()
        .find(|r| {
            r.status == highlight_status::ACTIVE && r.start_utf8 == start && r.end_utf8 == end
        })
        .map(|r| r.highlight_id.as_str())?;
    highlights.iter().find(|h| h.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::highlight_status;

    #[test]
    fn selection_builder_produces_valid_highlight_input() {
        let body = "Hello yellow world";
        let sel = BodySelection::new(6, 12).expect("sel");
        assert_eq!(sel.quote(body), Some("yellow"));
        let input = highlight_input_from_selection("itm_1", body, "digest_abc", sel, "alice", None)
            .expect("input");
        assert_eq!(input.item_id, "itm_1");
        assert_eq!(input.start_utf8, 6);
        assert_eq!(input.end_utf8, 12);
        assert_eq!(input.exact_quote, "yellow");
        assert_eq!(input.body_digest, "digest_abc");
        assert_eq!(input.actor, "alice");
    }

    #[test]
    fn selection_builder_rejects_empty() {
        let body = "abc";
        assert!(BodySelection::new(1, 1).is_none());
        let sel = BodySelection { start: 0, end: 0 };
        let err = highlight_input_from_selection("i", body, "", sel, "a", None).expect_err("e");
        assert!(err.contains("empty") || err.contains("valid"), "{err}");
    }

    #[test]
    fn focus_gate_blocks_when_note_editor_focused() {
        assert!(focus_allows_coding_shortcuts(true, false));
        assert!(!focus_allows_coding_shortcuts(false, false));
        assert!(!focus_allows_coding_shortcuts(true, true));
        assert!(!focus_allows_coding_shortcuts(false, true));
    }

    #[test]
    fn layout_job_includes_body_text() {
        let body = "aaa bbb ccc";
        let resolved = vec![ResolvedHighlight {
            highlight_id: "h1".into(),
            start_utf8: 4,
            end_utf8: 7,
            status: highlight_status::ACTIVE.into(),
            remapped: false,
        }];
        let job = body_layout_job(body, &resolved);
        let painted: String = job
            .sections
            .iter()
            .map(|s| body[s.byte_range.clone()].to_string())
            .collect();
        // LayoutJob sections reference job text, not original body indices.
        let full: String = job.text.clone();
        assert_eq!(full, body);
        assert!(!painted.is_empty() || !job.sections.is_empty());
        assert!(job.sections.len() >= 2, "plain + highlight sections");
    }

    #[test]
    fn body_digest_prefers_text_sha() {
        assert_eq!(
            body_digest_for_item(Some("abc"), Some("html"), "hello"),
            "abc"
        );
        assert_eq!(
            body_digest_for_item(None, Some("htmlsha"), "hello"),
            "htmlsha"
        );
        let syn = body_digest_for_item(None, None, "hello");
        assert_eq!(syn, display_body_digest("hello"));
    }

    #[test]
    fn citation_click_yields_char_highlight_range() {
        let body = "prefix hot suffix";
        let start = body.find("hot").expect("pos") as i64;
        let end = start + 3;
        let r = citation_highlight_target(body, Some(start), Some(end), "matched");
        assert_eq!(r, Some((7, 10))); // "prefix " = 7 chars
                                      // Unverified → no invent highlight
        assert!(
            citation_highlight_target(body, Some(start), Some(end), "quote_not_found").is_none()
        );
        // OOB
        assert!(citation_highlight_target(body, Some(999), Some(1005), "matched").is_none());
    }

    #[test]
    fn reverify_repairs_offsets_and_rejects_missing() {
        let body = "alpha unique_evidence_token beta";
        let quote = "unique_evidence_token";
        // Wrong stored offsets — re-find must repair.
        let (status, start, end) =
            reverify_citation_against_body(quote, Some(0), Some(5), body, false);
        assert_eq!(status, matter_core::VERIFY_MATCHED);
        let expected = body.find(quote).expect("pos") as i64;
        assert_eq!(start, Some(expected));
        assert_eq!(end, Some(expected + quote.len() as i64));
        // Highlight only when re-verified matched.
        assert!(citation_highlight_target(body, start, end, &status).is_some());

        let (st2, s2, e2) =
            reverify_citation_against_body("not in body", Some(0), Some(3), body, false);
        assert_eq!(st2, matter_core::VERIFY_QUOTE_NOT_FOUND);
        assert!(citation_highlight_target(body, s2, e2, &st2).is_none());
    }

    #[test]
    fn reverify_stale_text_sha_ignores_offsets() {
        let body = "new body with hot material only once";
        let quote = "hot material";
        // Offsets that would match a different span under old body — stale forces re-find.
        let (status, start, end) =
            reverify_citation_against_body(quote, Some(0), Some(3), body, true);
        assert_eq!(status, matter_core::VERIFY_MATCHED);
        let expected = body.find(quote).expect("pos") as i64;
        assert_eq!(start, Some(expected));
        assert_eq!(end, Some(expected + quote.len() as i64));
    }

    #[test]
    fn approx_scroll_y_counts_newlines_before_char() {
        let text = "line0\nline1\nline2 target here\nline3";
        let char_off = text.chars().position(|c| c == 't').expect("target char");
        // Two newlines before "target"
        assert_eq!(approx_scroll_y_for_char(text, char_off, 16.0), 32.0);
        assert_eq!(approx_scroll_y_for_char(text, 0, 16.0), 0.0);
        assert_eq!(approx_scroll_y_for_char("", 5, 16.0), 0.0);
        assert_eq!(approx_scroll_y_for_char(text, char_off, 0.0), 0.0);
    }

    #[test]
    fn byte_offsets_multibyte_chars() {
        let body = "café hot"; // é is 2 bytes
        let start = body.find("hot").expect("pos") as i64;
        let end = start + 3;
        let (cs, ce) = byte_offsets_to_char_range(body, start, end).expect("range");
        assert_eq!(
            &body.chars().skip(cs).take(ce - cs).collect::<String>(),
            "hot"
        );
    }

    fn sample_hl(id: &str, quote: &str, digest: &str, status: &str) -> ItemHighlight {
        ItemHighlight {
            id: id.into(),
            item_id: "itm".into(),
            matter_id: "mat".into(),
            start_utf8: 0,
            end_utf8: quote.chars().count() as i64,
            exact_quote: quote.into(),
            prefix: None,
            suffix: None,
            body_digest: digest.into(),
            color: "#FFF59D".into(),
            status: status.into(),
            created_at: "t".into(),
            updated_at: "t".into(),
            created_by: "t".into(),
        }
    }

    #[test]
    fn resolve_drives_stale_ui_when_quote_missing() {
        // Stored row still says active (DB not yet persisted stale).
        let body_old = "The secret clause is here.";
        let body_new = "Completely different body text.";
        let hl = sample_hl(
            "hlt_stale",
            "secret",
            &display_body_digest(body_old),
            highlight_status::ACTIVE,
        );
        let digest_new = display_body_digest(body_new);
        let resolved = resolve_for_paint(std::slice::from_ref(&hl), body_new, &digest_new);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].status, highlight_status::STALE);
        // UI helpers must report stale from resolve, not stored active.
        assert_eq!(count_stale_resolved(&resolved), 1);
        assert_eq!(
            stale_count_for_ui(std::slice::from_ref(&hl), Some(&resolved)),
            1
        );
        assert_eq!(
            highlight_ui_status(&hl, find_resolved(&resolved, "hlt_stale")),
            highlight_status::STALE
        );
        // Without body resolve, fall back to stored (still active).
        assert_eq!(stale_count_for_ui(std::slice::from_ref(&hl), None), 0);
    }

    #[test]
    fn resolve_keeps_active_when_digest_and_quote_match() {
        let body = "hello yellow world";
        let digest = display_body_digest(body);
        let hl = ItemHighlight {
            id: "hlt_ok".into(),
            item_id: "itm".into(),
            matter_id: "mat".into(),
            start_utf8: 6,
            end_utf8: 12,
            exact_quote: "yellow".into(),
            prefix: Some("hello ".into()),
            suffix: Some(" world".into()),
            body_digest: digest.clone(),
            color: "#FFF59D".into(),
            status: highlight_status::ACTIVE.into(),
            created_at: "t".into(),
            updated_at: "t".into(),
            created_by: "t".into(),
        };
        let resolved = resolve_for_paint(std::slice::from_ref(&hl), body, &digest);
        assert_eq!(count_stale_resolved(&resolved), 0);
        assert_eq!(
            highlight_ui_status(&hl, find_resolved(&resolved, "hlt_ok")),
            highlight_status::ACTIVE
        );
    }

    #[test]
    fn note_on_selection_path_uses_user_text_and_highlight_id() {
        // Empty draft must not save (and never auto-fake "Note on: …").
        let empty = note_upsert_from_draft("itm_1", "   ", Some("hlt_abc"), "alice");
        assert!(empty.is_err(), "empty body rejected");

        let quote = "important clause";
        let fake = format!("Note on: {}", quote.chars().take(80).collect::<String>());
        let input = note_upsert_from_draft(
            "itm_1",
            "  attorney observation about privilege  ",
            Some("hlt_abc"),
            "alice",
        )
        .expect("user text");
        assert_eq!(input.item_id, "itm_1");
        assert_eq!(input.body, "attorney observation about privilege");
        assert_eq!(input.highlight_id.as_deref(), Some("hlt_abc"));
        assert!(input.id.is_none());
        assert_ne!(input.body, fake);
        assert!(!input.body.starts_with("Note on:"));

        // Document note path: no highlight_id.
        let doc = note_upsert_from_draft("itm_1", "doc note", None, "bob").expect("doc");
        assert!(doc.highlight_id.is_none());
        assert_eq!(doc.body, "doc note");
    }

    #[test]
    fn passage_hint_is_not_auto_saved_body() {
        let hint = passage_note_hint_from_quote("secret sauce");
        assert!(hint.contains("secret sauce"));
        assert!(hint.starts_with("Passage note"));
        // Empty draft still fails — quote hint is never used as the body.
        assert!(note_upsert_from_draft("i", "", Some("h"), "a").is_err());
        assert!(note_upsert_from_draft("i", "   ", Some("h"), "a").is_err());
    }

    #[test]
    fn find_highlight_matches_active_resolved_selection() {
        let body = "hello yellow world";
        let digest = display_body_digest(body);
        let mut hl = sample_hl("hlt_1", "yellow", &digest, highlight_status::ACTIVE);
        hl.start_utf8 = 6;
        hl.end_utf8 = 12;
        hl.exact_quote = "yellow".into();
        let resolved = resolve_for_paint(std::slice::from_ref(&hl), body, &digest);
        assert_eq!(resolved[0].status, highlight_status::ACTIVE);
        let sel = BodySelection::new(6, 12).expect("sel");
        assert_eq!(
            find_highlight_for_selection(std::slice::from_ref(&hl), &resolved, sel)
                .map(|h| h.id.as_str()),
            Some("hlt_1")
        );
        let other = BodySelection::new(0, 5).expect("other");
        assert!(
            find_highlight_for_selection(std::slice::from_ref(&hl), &resolved, other).is_none()
        );
    }

    #[test]
    fn find_highlight_rejects_stale_offset_collision() {
        // Body drifted: stored offsets still land on same-length slice, but quote
        // no longer matches → resolve is stale. Reuse must return None so the
        // note-on-selection path creates a new highlight.
        let body_old = "hello yellow world";
        let body_new = "hello secret world"; // same length at 6..12, different text
        let mut hl = sample_hl(
            "hlt_stale",
            "yellow",
            &display_body_digest(body_old),
            highlight_status::ACTIVE, // stored status still active
        );
        hl.start_utf8 = 6;
        hl.end_utf8 = 12;
        hl.exact_quote = "yellow".into();
        hl.prefix = Some("hello ".into());
        hl.suffix = Some(" world".into());

        let digest_new = display_body_digest(body_new);
        let resolved = resolve_for_paint(std::slice::from_ref(&hl), body_new, &digest_new);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].status, highlight_status::STALE);

        let sel = BodySelection::new(6, 12).expect("sel");
        // Offset-only match would have returned the stale row; resolved path must not.
        assert!(
            find_highlight_for_selection(std::slice::from_ref(&hl), &resolved, sel).is_none(),
            "stale highlight with matching stored offsets must not be reused"
        );
    }
}
