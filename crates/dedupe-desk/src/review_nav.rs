//! Pure review navigation helpers (unit-tested; no egui dependency).

/// Next index in a list of `n` items (0-based). **No wrap** at the end.
pub fn next_index(i: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    if i + 1 < n {
        Some(i + 1)
    } else {
        None
    }
}

/// Previous index in a list of `n` items (0-based). **No wrap** at the start.
pub fn prev_index(i: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    if i > 0 {
        Some(i - 1)
    } else {
        None
    }
}

/// Whether keyboard shortcuts for next/prev should fire.
///
/// Pass `true` when egui reports no focused widget
/// (`ctx.memory(|m| m.focused().is_none())` in egui 0.34).
pub fn focus_allows_shortcuts(no_widget_focus: bool) -> bool {
    no_widget_focus
}

/// 1-based position string: `Item {i} of {n}` (empty list → `Item 0 of 0`).
pub fn position_label(index_0based: Option<usize>, n: usize) -> String {
    match (index_0based, n) {
        (None, 0) | (Some(_), 0) => "Item 0 of 0".into(),
        (None, n) => format!("Item — of {n}"),
        (Some(i), n) => format!("Item {} of {n}", i + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_clamps_at_end() {
        assert_eq!(next_index(0, 0), None);
        assert_eq!(next_index(0, 1), None);
        assert_eq!(next_index(0, 3), Some(1));
        assert_eq!(next_index(1, 3), Some(2));
        assert_eq!(next_index(2, 3), None);
    }

    #[test]
    fn prev_clamps_at_start() {
        assert_eq!(prev_index(0, 0), None);
        assert_eq!(prev_index(0, 3), None);
        assert_eq!(prev_index(1, 3), Some(0));
        assert_eq!(prev_index(2, 3), Some(1));
    }

    #[test]
    fn position_is_one_based() {
        assert_eq!(position_label(Some(0), 10), "Item 1 of 10");
        assert_eq!(position_label(Some(9), 10), "Item 10 of 10");
        assert_eq!(position_label(None, 0), "Item 0 of 0");
    }

    #[test]
    fn focus_gate() {
        assert!(focus_allows_shortcuts(true));
        assert!(!focus_allows_shortcuts(false));
    }
}
