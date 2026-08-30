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
}
