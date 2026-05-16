//! Shared utility helpers for dedup-engine.

/// Convert a FILETIME (100ns intervals since 1601-01-01) to Unix seconds.
pub fn filetime_to_unix(ft: i64) -> i64 {
    (ft / 10_000_000) - 11_644_473_600
}

/// Format a byte count as a human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Truncate a string safely by Unicode character count, adding "..." if truncated.
pub fn truncate_utf8(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        result.push_str("...");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filetime_to_unix_epoch() {
        // 1970-01-01 00:00:00 UTC in FILETIME = 11644473600 * 10_000_000
        let ft = 11_644_473_600i64 * 10_000_000;
        assert_eq!(filetime_to_unix(ft), 0);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2_097_152), "2.00 MB");
        assert_eq!(format_bytes(2_147_483_648), "2.00 GB");
    }

    #[test]
    fn test_truncate_utf8_no_truncation() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_utf8_ascii() {
        assert_eq!(truncate_utf8("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_utf8_multibyte() {
        let s = "日本語の長い文字列"; // Japanese characters
                                      // Each char is 3 bytes, but we truncate by char count.
                                      // max=5 → take(2) + "..." = 5 chars total.
        let result = truncate_utf8(s, 5);
        assert_eq!(result, "日本...");
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn test_truncate_utf8_exact_boundary() {
        // Multi-byte char at exact boundary
        let s = "aéiou"; // é is 2 bytes
        assert_eq!(truncate_utf8(s, 5), "aéiou");
        // max=4 → take(1) + "..." = 4 chars total
        assert_eq!(truncate_utf8(s, 4), "a...");
    }
}
