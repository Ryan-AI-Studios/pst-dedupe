//! DOM window math — keep in sync with host `queue_window`.

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

pub const ROW_HEIGHT: f64 = 32.0;
pub const OVERSCAN: usize = 8;
