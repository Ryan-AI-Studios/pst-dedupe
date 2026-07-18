//! Redaction helpers for the Review screen (track 0032).
//!
//! Pure builders for redaction create inputs and body layout paint — unit-tested
//! without egui interaction. Redactions paint **black on top of** yellow highlights.

use eframe::egui::{text::LayoutJob, text::TextFormat, text::TextWrapping, Color32, FontId};
use matter_core::{
    display_body_digest, redaction_reason, redaction_status, CreateRedactionInput, ItemRedaction,
    ResolvedHighlight, ResolvedRedaction,
};

use crate::review_notes::{body_job_for_ui, BodySelection, HIGHLIGHT_PAINT, HIGHLIGHT_STALE_PAINT};

/// Solid black paint for active redaction ranges.
pub const REDACTION_PAINT: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
/// Dim/warning paint for stale redaction ranges.
pub const REDACTION_STALE_PAINT: Color32 = Color32::from_rgb(0x40, 0x40, 0x40);

/// Default reason for Redact mode ComboBox.
pub const DEFAULT_REDACTION_REASON: &str = redaction_reason::PRIVILEGE;

/// Reasons shown in the UI ComboBox (order stable).
pub const REDACTION_REASON_CHOICES: &[&str] = redaction_reason::ALL;

/// Build a [`CreateRedactionInput`] from a body selection (desk → matter-core).
pub fn redaction_input_from_selection(
    item_id: &str,
    body: &str,
    body_digest: &str,
    sel: BodySelection,
    reason: &str,
    label: Option<String>,
    actor: &str,
) -> Result<CreateRedactionInput, String> {
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
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("redaction reason is required".into());
    }
    Ok(CreateRedactionInput {
        item_id: item_id.to_string(),
        start_utf8: sel.start as i64,
        end_utf8: sel.end as i64,
        exact_quote: quote.to_string(),
        display_body: body.to_string(),
        body_digest: digest,
        reason: reason.to_string(),
        label: label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        actor: actor.to_string(),
    })
}

/// Build resolved paint list from stored redactions + current body (in-memory).
pub fn resolve_redactions_for_paint(
    redactions: &[ItemRedaction],
    body: &str,
    digest: &str,
) -> Vec<ResolvedRedaction> {
    redactions
        .iter()
        .map(|r| matter_core::resolve_redaction_against_body(r, body, digest))
        .collect()
}

/// Count redactions whose **resolved** status is stale.
pub fn count_stale_redactions(resolved: &[ResolvedRedaction]) -> usize {
    resolved
        .iter()
        .filter(|r| r.status == redaction_status::STALE)
        .count()
}

/// Prefer in-memory resolve status for UI labels.
pub fn redaction_ui_status<'a>(
    red: &'a ItemRedaction,
    resolved: Option<&'a ResolvedRedaction>,
) -> &'a str {
    resolved
        .map(|r| r.status.as_str())
        .unwrap_or(red.status.as_str())
}

/// Look up a paint-ready resolve row by redaction id.
pub fn find_resolved_redaction<'a>(
    resolved: &'a [ResolvedRedaction],
    redaction_id: &str,
) -> Option<&'a ResolvedRedaction> {
    resolved.iter().find(|r| r.redaction_id == redaction_id)
}

/// Stale redaction count for header / banner.
pub fn stale_redaction_count_for_ui(
    redactions: &[ItemRedaction],
    resolved: Option<&[ResolvedRedaction]>,
) -> usize {
    match resolved {
        Some(r) => count_stale_redactions(r),
        None => redactions
            .iter()
            .filter(|h| h.status == redaction_status::STALE)
            .count(),
    }
}

/// Whether redacted produce artifact is missing/outdated while regions exist.
///
/// Stale when count>0 AND (sha NULL OR source digest mismatches the body CAS
/// used as source: prefer `text_sha256`; else `html_sha256` when plain text is
/// absent). Matches matter-core `redacted_text_stale` filter.
pub fn redacted_artifact_is_stale(
    redaction_count: i64,
    redacted_text_sha256: Option<&str>,
    redacted_source_digest: Option<&str>,
    text_sha256: Option<&str>,
    html_sha256: Option<&str>,
) -> bool {
    if redaction_count <= 0 {
        return false;
    }
    let sha_missing = redacted_text_sha256
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none();
    if sha_missing {
        return true;
    }
    let src = redacted_source_digest
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let text = text_sha256.map(str::trim).filter(|s| !s.is_empty());
    let html = html_sha256.map(str::trim).filter(|s| !s.is_empty());
    match (src, text, html) {
        (Some(src), Some(txt), _) => src != txt,
        (Some(src), None, Some(h)) => src != h,
        _ => false,
    }
}

/// Gate regenerate against a truncated Review display pane.
///
/// When `text_sha256` is present, matter-core loads the **full** plain-text CAS
/// and ignores display bytes — truncated UI is OK.
///
/// When only a truncated display pane is available (no text CAS), fail closed so
/// we never write a partial redacted artifact labeled as full-body source.
pub fn refuse_truncated_regenerate(
    truncated: bool,
    text_sha256: Option<&str>,
) -> Result<(), String> {
    let has_text = text_sha256
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some();
    if truncated && !has_text {
        return Err(
            "Body is truncated and no full text CAS (text_sha256) is available; \
             refuse redacted regenerate from partial display. Re-extract text or \
             open a smaller body before regenerating."
                .into(),
        );
    }
    Ok(())
}

/// Status line after regenerate.
pub fn regenerate_status_message(region_count: u64, stale_count: u64, sha: Option<&str>) -> String {
    match sha {
        Some(s) if region_count > 0 => {
            let short: String = s.chars().take(12).collect();
            if stale_count > 0 {
                format!(
                    "Redacted text regenerated ({region_count} region(s), sha {short}…) — \
                     {stale_count} stale skipped"
                )
            } else {
                format!("Redacted text regenerated ({region_count} region(s), sha {short}…)")
            }
        }
        _ => {
            if stale_count > 0 {
                format!("Redacted artifact cleared (no active regions; {stale_count} stale)")
            } else {
                "Redacted artifact cleared (no active redactions)".into()
            }
        }
    }
}

/// Whether digit coding shortcuts should fire given focus + note/privilege/redact flags.
pub fn focus_allows_coding_with_redact(
    no_widget_focus: bool,
    note_editor_focused: bool,
    privilege_focused: bool,
    redact_reason_focused: bool,
) -> bool {
    crate::review_privilege::focus_allows_coding_with_privilege(
        no_widget_focus,
        note_editor_focused,
        privilege_focused,
    ) && !redact_reason_focused
}

/// Build a monospace [`LayoutJob`] with yellow highlights and **black redactions on top**.
///
/// Paint priority at each char: redaction > highlight > plain.
pub fn body_layout_job_with_redactions(
    body: &str,
    highlights: &[ResolvedHighlight],
    redactions: &[ResolvedRedaction],
) -> LayoutJob {
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

    if highlights.is_empty() && redactions.is_empty() {
        job.append(body, 0.0, base);
        return job;
    }

    // Per-char coverage: 0=plain, 1=hl active, 2=hl stale, 3=red active, 4=red stale.
    // Higher wins so redactions paint over highlights.
    let mut cover: Vec<u8> = vec![0; char_len];
    for r in highlights {
        if r.end_utf8 <= r.start_utf8 || r.start_utf8 < 0 {
            continue;
        }
        let s = (r.start_utf8 as usize).min(char_len);
        let e = (r.end_utf8 as usize).min(char_len);
        let v = if r.status == "active" { 1u8 } else { 2u8 };
        for c in cover.iter_mut().take(e).skip(s) {
            if *c < v {
                *c = v;
            }
        }
    }
    for r in redactions {
        if r.end_utf8 <= r.start_utf8 || r.start_utf8 < 0 {
            continue;
        }
        let s = (r.start_utf8 as usize).min(char_len);
        let e = (r.end_utf8 as usize).min(char_len);
        let v = if r.status == redaction_status::ACTIVE {
            3u8
        } else {
            4u8
        };
        for c in cover.iter_mut().take(e).skip(s) {
            *c = v; // redactions always win
        }
    }

    let chars: Vec<char> = body.chars().collect();
    let mut i = 0usize;
    while i < char_len {
        let kind = cover[i];
        let mut j = i + 1;
        while j < char_len && cover[j] == kind {
            j += 1;
        }
        let slice: String = chars[i..j].iter().collect();
        let mut fmt = base.clone();
        match kind {
            1 => fmt.background = HIGHLIGHT_PAINT,
            2 => fmt.background = HIGHLIGHT_STALE_PAINT,
            3 => {
                fmt.background = REDACTION_PAINT;
                fmt.color = Color32::from_rgb(0xFF, 0xFF, 0xFF);
            }
            4 => {
                fmt.background = REDACTION_STALE_PAINT;
                fmt.color = Color32::from_rgb(0xCC, 0xCC, 0xCC);
            }
            _ => {}
        }
        job.append(&slice, 0.0, fmt);
        i = j;
    }
    job
}

/// Wrap-width variant for egui TextEdit layouter.
pub fn body_job_for_ui_with_redactions(
    body: &str,
    highlights: &[ResolvedHighlight],
    redactions: &[ResolvedRedaction],
    wrap_width: f32,
) -> LayoutJob {
    if redactions.is_empty() {
        // Fast path: reuse highlight-only painter.
        return body_job_for_ui(body, highlights, wrap_width);
    }
    let mut job = body_layout_job_with_redactions(body, highlights, redactions);
    job.wrap.max_width = wrap_width;
    job
}

/// Selection → redaction path when redact mode is on (vs highlight mode).
pub fn selection_creates_redaction(redact_mode: bool) -> bool {
    redact_mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::highlight_status;

    #[test]
    fn selection_builder_produces_valid_redaction_input() {
        let body = "Hello SECRET world";
        let sel = BodySelection::new(6, 12).expect("sel");
        assert_eq!(sel.quote(body), Some("SECRET"));
        let input = redaction_input_from_selection(
            "itm_1",
            body,
            "digest_abc",
            sel,
            redaction_reason::PRIVILEGE,
            Some("PRIV".into()),
            "alice",
        )
        .expect("input");
        assert_eq!(input.item_id, "itm_1");
        assert_eq!(input.start_utf8, 6);
        assert_eq!(input.end_utf8, 12);
        assert_eq!(input.exact_quote, "SECRET");
        assert_eq!(input.reason, redaction_reason::PRIVILEGE);
        assert_eq!(input.label.as_deref(), Some("PRIV"));
        assert_eq!(input.actor, "alice");
    }

    #[test]
    fn selection_builder_rejects_empty() {
        let body = "abc";
        let sel = BodySelection { start: 0, end: 0 };
        let err =
            redaction_input_from_selection("i", body, "", sel, "other", None, "a").expect_err("e");
        assert!(err.contains("empty") || err.contains("valid"), "{err}");
    }

    #[test]
    fn mode_routing_redact_vs_highlight() {
        assert!(selection_creates_redaction(true));
        assert!(!selection_creates_redaction(false));
    }

    #[test]
    fn layout_paints_redaction_black_over_highlight() {
        let body = "aaa bbb ccc";
        // Highlight covers 4..11, redaction covers 4..7 — redaction wins on overlap.
        let highlights = vec![ResolvedHighlight {
            highlight_id: "h1".into(),
            start_utf8: 4,
            end_utf8: 11,
            status: highlight_status::ACTIVE.into(),
            remapped: false,
        }];
        let redactions = vec![ResolvedRedaction {
            redaction_id: "r1".into(),
            start_utf8: 4,
            end_utf8: 7,
            status: redaction_status::ACTIVE.into(),
            remapped: false,
            reason: redaction_reason::PII.into(),
        }];
        let job = body_layout_job_with_redactions(body, &highlights, &redactions);
        assert_eq!(job.text, body);
        // Find section covering "bbb" (chars 4..7) — must be black.
        let mut found_black = false;
        let mut found_yellow = false;
        for sec in &job.sections {
            let slice = &job.text[sec.byte_range.clone()];
            if slice == "bbb" {
                assert_eq!(sec.format.background, REDACTION_PAINT);
                found_black = true;
            }
            if slice.contains('c') {
                // " ccc" or part may be yellow
                if sec.format.background == HIGHLIGHT_PAINT {
                    found_yellow = true;
                }
            }
        }
        assert!(found_black, "redacted span should paint black");
        assert!(
            found_yellow || job.sections.len() >= 2,
            "non-redacted highlight remainder should still paint"
        );
    }

    #[test]
    fn artifact_stale_helpers() {
        assert!(redacted_artifact_is_stale(1, None, None, Some("abc"), None));
        assert!(!redacted_artifact_is_stale(
            0,
            None,
            None,
            Some("abc"),
            None
        ));
        assert!(!redacted_artifact_is_stale(
            1,
            Some("sha"),
            Some("abc"),
            Some("abc"),
            None
        ));
        assert!(redacted_artifact_is_stale(
            1,
            Some("sha"),
            Some("old"),
            Some("new"),
            None
        ));
        // HTML-only source mismatch.
        assert!(redacted_artifact_is_stale(
            1,
            Some("sha"),
            Some("old_html"),
            None,
            Some("new_html")
        ));
        assert!(!redacted_artifact_is_stale(
            1,
            Some("sha"),
            Some("html"),
            None,
            Some("html")
        ));
        // text_sha present wins over html for mismatch check.
        assert!(!redacted_artifact_is_stale(
            1,
            Some("sha"),
            Some("txt"),
            Some("txt"),
            Some("different_html")
        ));
    }

    #[test]
    fn refuse_truncated_regenerate_gate() {
        assert!(refuse_truncated_regenerate(false, None).is_ok());
        assert!(refuse_truncated_regenerate(false, Some("abc")).is_ok());
        // Truncated + full text CAS available → OK (core loads CAS).
        assert!(refuse_truncated_regenerate(true, Some("abc")).is_ok());
        // Truncated with no text CAS → fail closed.
        let err = refuse_truncated_regenerate(true, None).expect_err("fail closed");
        assert!(err.contains("truncated"), "{err}");
        assert!(refuse_truncated_regenerate(true, Some("  ")).is_err());
    }

    #[test]
    fn focus_gate_blocks_when_redact_focused() {
        assert!(focus_allows_coding_with_redact(true, false, false, false));
        assert!(!focus_allows_coding_with_redact(true, false, false, true));
        assert!(!focus_allows_coding_with_redact(true, true, false, false));
    }

    #[test]
    fn utf8_slice_helper_available() {
        assert_eq!(matter_core::utf8_char_slice("ab©cd", 2, 3), Some("©"));
    }
}
