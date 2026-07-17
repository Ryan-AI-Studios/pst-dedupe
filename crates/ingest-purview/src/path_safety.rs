//! Logical path sanitization after name decoding.

use crate::error::{codes, Error, Result};

/// Normalize and validate a decoded ZIP/package-relative path.
///
/// - Converts `\` → `/`
/// - Rejects empty paths, `.` / `..` segments, absolute Unix/Windows paths
/// - Rejects NUL and other C0 controls (except TAB is also rejected)
/// - Returns a package-relative UTF-8 path using `/` separators
pub fn sanitize_logical_path(raw: &str) -> Result<String> {
    // Never panic on arbitrary Unicode input.
    if raw.contains('\0') {
        return Err(Error::PathRejected {
            code: codes::ZIP_UNSAFE_PATH,
            message: "path contains NUL".into(),
        });
    }
    if raw.chars().any(|c| c.is_control()) {
        return Err(Error::PathRejected {
            code: codes::ZIP_UNSAFE_PATH,
            message: "path contains control characters".into(),
        });
    }

    let normalized = raw.replace('\\', "/");
    let trimmed = normalized.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(Error::PathRejected {
            code: codes::ZIP_EMPTY_PATH,
            message: "empty path".into(),
        });
    }

    // Absolute Unix.
    if trimmed.starts_with('/') {
        return Err(Error::PathRejected {
            code: codes::ZIP_ABSOLUTE_PATH,
            message: format!("absolute path: {trimmed}"),
        });
    }

    // UNC or protocol-ish.
    if trimmed.starts_with("//") || trimmed.starts_with("\\\\") {
        return Err(Error::PathRejected {
            code: codes::ZIP_ABSOLUTE_PATH,
            message: format!("UNC/absolute path: {trimmed}"),
        });
    }

    // Windows drive absolute: "C:/" or "C:"
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let drive = bytes[0];
        if drive.is_ascii_alphabetic() {
            return Err(Error::PathRejected {
                code: codes::ZIP_ABSOLUTE_PATH,
                message: format!("drive-absolute path: {trimmed}"),
            });
        }
    }

    let mut out_parts: Vec<&str> = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(Error::PathRejected {
                code: codes::ZIP_PATH_TRAVERSAL,
                message: format!("path traversal segment in: {trimmed}"),
            });
        }
        // Reject Windows device names (CON, PRN, …) as a whole segment.
        if is_reserved_device_name(part) {
            return Err(Error::PathRejected {
                code: codes::ZIP_UNSAFE_PATH,
                message: format!("reserved device name: {part}"),
            });
        }
        out_parts.push(part);
    }

    if out_parts.is_empty() {
        return Err(Error::PathRejected {
            code: codes::ZIP_EMPTY_PATH,
            message: "path resolved empty".into(),
        });
    }

    Ok(out_parts.join("/"))
}

/// Join archive stack prefix with an inner logical path using `!/` separators.
///
/// Example: stack `["files.zip", "inner.zip"]` + `note.txt`
/// → `files.zip!/inner.zip!/note.txt`
pub fn join_logical_path(archive_stack: &[String], entry_path: &str) -> String {
    if archive_stack.is_empty() {
        return entry_path.to_string();
    }
    let mut out = String::new();
    for (i, arch) in archive_stack.iter().enumerate() {
        if i > 0 {
            out.push_str("!/");
        }
        out.push_str(arch);
    }
    out.push_str("!/");
    out.push_str(entry_path);
    out
}

fn is_reserved_device_name(segment: &str) -> bool {
    // Strip trailing extension-like suffix: CON.txt still reserved on Windows.
    let base = segment.split('.').next().unwrap_or(segment);
    let upper = base.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn accepts_simple_relative() {
        assert_eq!(
            sanitize_logical_path(r"mail\Inbox\a.eml").unwrap(),
            "mail/Inbox/a.eml"
        );
    }

    #[test]
    fn rejects_traversal() {
        let err = sanitize_logical_path("../etc/passwd").unwrap_err();
        assert_eq!(err.code(), codes::ZIP_PATH_TRAVERSAL);
        let err = sanitize_logical_path("a/../../b").unwrap_err();
        assert_eq!(err.code(), codes::ZIP_PATH_TRAVERSAL);
    }

    #[test]
    fn rejects_absolute_unix_and_windows() {
        assert_eq!(
            sanitize_logical_path("/etc/passwd").unwrap_err().code(),
            codes::ZIP_ABSOLUTE_PATH
        );
        assert_eq!(
            sanitize_logical_path(r"C:\Windows\system32")
                .unwrap_err()
                .code(),
            codes::ZIP_ABSOLUTE_PATH
        );
        assert_eq!(
            sanitize_logical_path("//server/share").unwrap_err().code(),
            codes::ZIP_ABSOLUTE_PATH
        );
    }

    #[test]
    fn join_stack() {
        let stack = vec!["files.zip".into(), "inner.zip".into()];
        assert_eq!(
            join_logical_path(&stack, "note.txt"),
            "files.zip!/inner.zip!/note.txt"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        #[test]
        fn sanitizer_no_panic_and_rejects_traversal(s in ".*") {
            let result = std::panic::catch_unwind(|| sanitize_logical_path(&s));
            assert!(result.is_ok(), "sanitizer panicked on input");
            if let Ok(Ok(path)) = result {
                assert!(!path.split('/').any(|p| p == ".."));
                assert!(!path.starts_with('/'));
            }
            // Explicit traversal forms always rejected when decodeable as such.
            if s.replace('\\', "/").split('/').any(|p| p == "..")
                && !s.contains('\0')
                && !s.chars().any(|c| c.is_control())
            {
                // May still be absolute-first; either way not Ok with ..
                if let Ok(Ok(path)) = std::panic::catch_unwind(|| sanitize_logical_path(&s)) {
                    assert!(!path.split('/').any(|p| p == ".."));
                }
            }
        }
    }
}
