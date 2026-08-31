//! Pure DOM window math for the first-pass queue (host tests + UI formula).

/// Inclusive-exclusive index range of rows that should be mounted in the DOM.
///
/// `end` is exclusive. Overscroll clamps so `start ≤ end ≤ total`.
pub fn visible_range(
    scroll_top: f64,
    viewport_h: f64,
    row_h: f64,
    total: usize,
    overscan: usize,
) -> (usize, usize) {
    if total == 0 || row_h <= 0.0 {
        return (0, 0);
    }
    let scroll_top = scroll_top.max(0.0);
    let viewport_h = viewport_h.max(0.0);
    let first = (scroll_top / row_h).floor() as isize;
    let last = ((scroll_top + viewport_h) / row_h).ceil() as isize;
    let over = overscan as isize;
    let start = (first - over).max(0) as usize;
    let end_raw = (last + over).max(0) as usize;
    let end = end_raw.min(total);
    let start = start.min(end);
    (start, end)
}

/// Offset of the last SQL page (`PAGE_LIMIT` chunks). Total 0 → 0.
pub fn last_page_offset(total: u64, page_limit: u64) -> u64 {
    if total == 0 || page_limit == 0 {
        return 0;
    }
    (total.saturating_sub(1) / page_limit) * page_limit
}

/// `Some(new_offset)` when an empty fetch is past the end of the corpus (clamp).
/// `None` for a real empty corpus or a data gap (`offset < total`).
pub fn offset_after_empty_page(
    offset: u64,
    total: u64,
    fetched_len: usize,
    page_limit: u64,
) -> Option<u64> {
    if fetched_len != 0 || total == 0 {
        return None;
    }
    if offset >= total {
        Some(last_page_offset(total, page_limit))
    } else {
        None
    }
}

/// Apply `offset_after_empty_page` only when `last_fetch_meta` is for `current_offset`.
///
/// Stale empty-page metadata must not clamp a newer pager offset (Next from a gap
/// would otherwise snap back to 0 before the fetch returns).
pub fn clamp_offset_for_fetch_meta(
    current_offset: u64,
    meta_offset: u64,
    total: u64,
    fetched_len: usize,
    page_limit: u64,
) -> Option<u64> {
    if current_offset != meta_offset {
        return None;
    }
    offset_after_empty_page(current_offset, total, fetched_len, page_limit)
}

/// Next pager disable. A data gap (`fetched_len == 0 && total > 0 && offset < total`)
/// keeps Next enabled so the operator can leave the gap (spec §3.2).
pub fn next_page_disabled(offset: u64, total: u64, fetched_len: usize, page_limit: u64) -> bool {
    if total == 0 || page_limit == 0 {
        return true;
    }
    if fetched_len == 0 && offset < total {
        return false;
    }
    offset.saturating_add(page_limit) >= total
}

/// Scroll position that keeps `idx` inside the viewport. Unchanged when already visible.
pub fn scroll_top_to_reveal(idx: usize, row_h: f64, viewport_h: f64, scroll_top: f64) -> f64 {
    if row_h <= 0.0 {
        return scroll_top.max(0.0);
    }
    let viewport_h = viewport_h.max(0.0);
    let scroll_top = scroll_top.max(0.0);
    let row_top = idx as f64 * row_h;
    let row_bottom = row_top + row_h;
    let view_bottom = scroll_top + viewport_h;
    if row_top < scroll_top {
        row_top
    } else if row_bottom > view_bottom {
        (row_bottom - viewport_h).max(0.0)
    } else {
        scroll_top
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_at_top() {
        let (start, end) = visible_range(0.0, 640.0, 32.0, 1000, 8);
        assert_eq!(start, 0);
        // viewport rows = 640/32 = 20; + overscan 8 → end ≤ 28
        assert_eq!(end, 28);
        assert!(end - start <= 20 + 16);
    }

    #[test]
    fn visible_range_near_bottom() {
        // Near bottom: last visible index around 980 with 20-row viewport.
        let scroll_top = 980.0 * 32.0;
        let (start, end) = visible_range(scroll_top, 640.0, 32.0, 1000, 8);
        assert!(start <= end);
        assert!(end <= 1000);
        assert!(end - start <= 20 + 16);
        assert!(start >= 980 - 8);
    }

    #[test]
    fn visible_range_overscroll_clamps() {
        let scroll_top = 1000.0 * 32.0 + 500.0;
        let (start, end) = visible_range(scroll_top, 640.0, 32.0, 1000, 8);
        assert!(start <= end);
        assert!(end <= 1000);
        assert_eq!(end, 1000);
    }

    #[test]
    fn visible_range_empty_total() {
        assert_eq!(visible_range(0.0, 640.0, 32.0, 0, 8), (0, 0));
    }

    #[test]
    fn visible_range_span_cap() {
        let (start, end) = visible_range(100.0, 640.0, 32.0, 1000, 8);
        assert!(end - start <= 20 + 16);
        assert!(start <= end);
        assert!(end <= 1000);
    }

    #[test]
    fn last_page_offset_cases() {
        assert_eq!(last_page_offset(0, 500), 0);
        assert_eq!(last_page_offset(500, 500), 0);
        assert_eq!(last_page_offset(501, 500), 500);
    }

    #[test]
    fn offset_after_empty_page_clamp_gap_corpus() {
        assert_eq!(offset_after_empty_page(500, 400, 0, 500), Some(0));
        assert_eq!(offset_after_empty_page(0, 10, 0, 500), None);
        assert_eq!(offset_after_empty_page(0, 0, 0, 500), None);
        assert_eq!(offset_after_empty_page(0, 10, 3, 500), None);
    }

    #[test]
    fn clamp_offset_for_fetch_meta_ignores_stale_gap() {
        // Normative gap at offset 0; operator clicks Next (offset 500) before refetch.
        assert_eq!(clamp_offset_for_fetch_meta(500, 0, 10, 0, 500), None);
        assert_eq!(clamp_offset_for_fetch_meta(0, 0, 10, 0, 500), None);
        // Empty fetch that actually landed past the corpus still clamps.
        assert_eq!(clamp_offset_for_fetch_meta(500, 500, 10, 0, 500), Some(0));
        assert_eq!(clamp_offset_for_fetch_meta(500, 500, 400, 0, 500), Some(0));
    }

    #[test]
    fn next_page_disabled_gap_keeps_next_usable() {
        assert!(!next_page_disabled(0, 10, 0, 500));
        assert!(next_page_disabled(0, 10, 10, 500));
        assert!(!next_page_disabled(0, 501, 500, 500));
        assert!(next_page_disabled(500, 501, 1, 500));
        assert!(next_page_disabled(0, 0, 0, 500));
        assert!(!next_page_disabled(500, 600, 0, 500));
    }

    #[test]
    fn scroll_top_to_reveal_past_window_in_window_and_top() {
        let vh = 640.0;
        let rh = 32.0;
        assert_eq!(scroll_top_to_reveal(25, rh, vh, 0.0), 192.0);
        assert_eq!(scroll_top_to_reveal(5, rh, vh, 0.0), 0.0);
        assert_eq!(scroll_top_to_reveal(0, rh, vh, 0.0), 0.0);
    }

    fn pub_fn_body<'a>(src: &'a str, name: &str) -> &'a str {
        let needle = format!("pub fn {name}");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("missing {name}"));
        let rest = &src[start..];
        let rel = rest[needle.len()..]
            .find("\npub fn ")
            .or_else(|| rest[needle.len()..].find("\npub const "))
            .or_else(|| rest[needle.len()..].find("\n#[cfg(test)]"))
            .unwrap_or(rest.len() - needle.len());
        rest[..needle.len() + rel].trim_end()
    }

    #[test]
    fn clamp_helper_single_production_consumer_in_queue_page() {
        let src = include_str!("../ui/src/pages/queue.rs");
        assert_eq!(
            src.matches("offset_after_empty_page(").count(),
            0,
            "fetch/render must not call offset_after_empty_page"
        );
        assert_eq!(
            src.matches("clamp_offset_for_fetch_meta(").count(),
            1,
            "clamp helper must be consumed only by the dedicated Effect"
        );
    }

    #[test]
    fn ui_queue_window_twin_matches_host() {
        let host = include_str!("queue_window.rs");
        let ui = include_str!("../ui/src/queue_window.rs");
        for name in [
            "visible_range",
            "last_page_offset",
            "offset_after_empty_page",
            "clamp_offset_for_fetch_meta",
            "next_page_disabled",
            "scroll_top_to_reveal",
        ] {
            assert_eq!(
                pub_fn_body(host, name),
                pub_fn_body(ui, name),
                "twin drift on {name}"
            );
        }
    }
}
