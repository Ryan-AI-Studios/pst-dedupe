//! DOM window math — keep in sync with host `queue_window::visible_range`.

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

pub const ROW_HEIGHT: f64 = 32.0;
pub const OVERSCAN: usize = 8;
