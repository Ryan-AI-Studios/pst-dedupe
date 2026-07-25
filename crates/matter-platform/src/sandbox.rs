//! PLATFORM_STORAGE_ROOT path sandbox.
//!
//! Production invariant: every registered/resolved matter path is checked with
//! [`assert_path_under_root`] on the **final** path (canonical, strict subdir).
//! In addition, when constructing non-existent paths under an existing ancestor,
//! each appended **Normal** name component is pre-checked with
//! [`reject_untrusted_path_component`] (blocks `..`, separators, drive markers).
//! Component rejection is also available to future path builders as an optional
//! pre-check before join.

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Env var for the allowed storage root (single root for P0).
pub const ENV_PLATFORM_STORAGE_ROOT: &str = "PLATFORM_STORAGE_ROOT";

/// Reject untrusted path *components* before any join (tenant slug segments, etc.).
///
/// Callers that build paths from tenant/operator input **must** validate each
/// segment with this helper (or equivalent) and never `root.join(absolute)` of
/// untrusted input without [`assert_path_under_root`] on the final path.
pub fn reject_untrusted_path_component(s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(Error::PathNotSandboxed(
            "path component must not be empty".into(),
        ));
    }
    if s == "." || s == ".." {
        return Err(Error::PathNotSandboxed(format!(
            "path component must not be '{s}'"
        )));
    }
    if s.contains('\0') {
        return Err(Error::PathNotSandboxed(
            "path component must not contain NUL".into(),
        ));
    }
    // Separators, drive markers, and absolute-path markers are forbidden in a component.
    if s.contains('/') || s.contains('\\') || s.contains(':') {
        return Err(Error::PathNotSandboxed(format!(
            "path component must not contain separators or drive markers: {s:?}"
        )));
    }
    // Reject Windows UNC / extended-path prefixes if somehow passed as a "component".
    if s.starts_with("//") || s.starts_with(r"\\") {
        return Err(Error::PathNotSandboxed(format!(
            "path component must not be absolute/UNC: {s:?}"
        )));
    }
    // Absolute POSIX/Windows path masquerading as a single OsStr-ish component.
    if Path::new(s).is_absolute() {
        return Err(Error::PathNotSandboxed(format!(
            "path component must not be absolute: {s:?}"
        )));
    }
    Ok(())
}

/// Canonicalize and require `path` to be a **strict subdirectory** of `allowed_root`.
///
/// Rejects: missing root, `..` escape, path equal to root, foreign absolute paths.
///
/// **Callers:** never `allowed_root.join(untrusted_absolute)` without this check —
/// `Path::join` replaces the base when the RHS is absolute.
pub fn assert_path_under_root(path: &Path, allowed_root: &Path) -> Result<PathBuf> {
    let root = canonicalize_strict(allowed_root).map_err(|e| {
        Error::PathNotSandboxed(format!(
            "PLATFORM_STORAGE_ROOT invalid ({}): {e}",
            allowed_root.display()
        ))
    })?;

    // Reject absolute paths that clearly lie outside the root before join/canonicalize work.
    if path.is_absolute() {
        // If it exists, canonicalize + subdir check below.
        // If it does not exist, reject unless its parent chain resolves under root.
        if path.exists() {
            // fall through to canonicalize path
        } else if let Some(parent) = path.parent() {
            // Non-existent absolute outside root: fail early when no ancestor is under root.
            if parent.is_absolute() && !parent.exists() {
                // Walk to see if any prefix is under root; if none exist, reject foreign abs.
                let mut cursor = parent.to_path_buf();
                let mut found_under = false;
                loop {
                    if cursor.exists() {
                        if let Ok(can) = canonicalize_strict(&cursor) {
                            if is_strict_subdir(&can, &root) || paths_equal(&can, &root) {
                                found_under = true;
                            }
                        }
                        break;
                    }
                    if !cursor.pop() {
                        break;
                    }
                }
                if !found_under {
                    return Err(Error::PathNotSandboxed(format!(
                        "absolute path outside storage root: {}",
                        path.display()
                    )));
                }
            }
        }
    }

    // Reject obvious `..` components before canonicalize (also helps when path does not exist yet).
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::PathNotSandboxed(format!(
            "path contains '..': {}",
            path.display()
        )));
    }

    let candidate = if path.exists() {
        canonicalize_strict(path).map_err(|e| {
            Error::PathNotSandboxed(format!("cannot canonicalize {}: {e}", path.display()))
        })?
    } else {
        // Parent must exist and be under root; append final component.
        let parent = path.parent().ok_or_else(|| {
            Error::PathNotSandboxed(format!("path has no parent: {}", path.display()))
        })?;
        let name = path.file_name().ok_or_else(|| {
            Error::PathNotSandboxed(format!("path has no file name: {}", path.display()))
        })?;
        if parent.as_os_str().is_empty() || parent == Path::new("") {
            return Err(Error::PathNotSandboxed(format!(
                "refusing bare name without directory: {}",
                path.display()
            )));
        }
        let parent_can = if parent.exists() {
            canonicalize_strict(parent).map_err(|e| {
                Error::PathNotSandboxed(format!(
                    "cannot canonicalize parent {}: {e}",
                    parent.display()
                ))
            })?
        } else {
            // Allow nested create under root by walking up to an existing ancestor.
            resolve_under_existing_ancestor(parent, &root)?
        };
        // Final leaf name is untrusted input when building under an ancestor.
        reject_untrusted_path_component(&name.to_string_lossy())?;
        parent_can.join(name)
    };

    if paths_equal(&candidate, &root) {
        return Err(Error::PathNotSandboxed(format!(
            "path must be a strict subdirectory of storage root, not the root itself: {}",
            candidate.display()
        )));
    }

    if !is_strict_subdir(&candidate, &root) {
        return Err(Error::PathNotSandboxed(format!(
            "path {} is not under {}",
            candidate.display(),
            root.display()
        )));
    }

    Ok(candidate)
}

fn resolve_under_existing_ancestor(path: &Path, root: &Path) -> Result<PathBuf> {
    let mut components: Vec<Component<'_>> = path.components().collect();
    let mut suffix: Vec<Component<'_>> = Vec::new();
    loop {
        let try_path: PathBuf = components.iter().collect();
        if try_path.exists() {
            let can = canonicalize_strict(&try_path).map_err(|e| {
                Error::PathNotSandboxed(format!(
                    "cannot canonicalize ancestor {}: {e}",
                    try_path.display()
                ))
            })?;
            if !is_strict_subdir(&can, root) && !paths_equal(&can, root) {
                return Err(Error::PathNotSandboxed(format!(
                    "ancestor {} is not under storage root",
                    can.display()
                )));
            }
            let mut out = can;
            for c in suffix.into_iter().rev() {
                match c {
                    Component::Normal(os) => {
                        reject_untrusted_path_component(&os.to_string_lossy())?;
                        out.push(os);
                    }
                    Component::CurDir => {
                        // Ignore `.` rather than joining a relative no-op.
                    }
                    other => {
                        return Err(Error::PathNotSandboxed(format!(
                            "path component not allowed when resolving under root: {other:?}"
                        )));
                    }
                }
            }
            return Ok(out);
        }
        match components.pop() {
            Some(c) => suffix.push(c),
            None => {
                return Err(Error::PathNotSandboxed(format!(
                    "no existing ancestor for {}",
                    path.display()
                )))
            }
        }
    }
}

fn canonicalize_strict(path: &Path) -> std::io::Result<PathBuf> {
    // std::fs::canonicalize resolves symlinks (best-effort escape mitigation).
    std::fs::canonicalize(path)
}

fn is_strict_subdir(path: &Path, root: &Path) -> bool {
    let mut rest = path.components();
    for rc in root.components() {
        match rest.next() {
            Some(pc) if component_eq(&pc, &rc) => {}
            _ => return false,
        }
    }
    // Must have at least one remaining component.
    rest.next().is_some()
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    a.components()
        .zip(b.components())
        .all(|(x, y)| component_eq(&x, &y))
        && a.components().count() == b.components().count()
}

fn component_eq(a: &Component<'_>, b: &Component<'_>) -> bool {
    match (a, b) {
        (Component::Prefix(ap), Component::Prefix(bp)) => {
            // Windows: compare prefix strings case-insensitively via OsStr equality after canonicalize.
            ap.as_os_str().eq_ignore_ascii_case(bp.as_os_str())
        }
        (Component::RootDir, Component::RootDir) => true,
        (Component::CurDir, Component::CurDir) => true,
        (Component::ParentDir, Component::ParentDir) => true,
        (Component::Normal(a), Component::Normal(b)) => {
            // Windows paths are case-insensitive.
            #[cfg(windows)]
            {
                a.eq_ignore_ascii_case(b)
            }
            #[cfg(not(windows))]
            {
                a == b
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn valid_child_ok() {
        let dir = tempdir().expect("tmp");
        let root = dir.path();
        let child = root.join("firm-a").join("case1");
        std::fs::create_dir_all(&child).expect("mkdir");
        let got = assert_path_under_root(&child, root).expect("ok");
        assert!(got.ends_with("case1") || got.file_name().is_some());
    }

    #[test]
    fn equal_root_rejected() {
        let dir = tempdir().expect("tmp");
        let root = dir.path();
        let err = assert_path_under_root(root, root).expect_err("root eq");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn parent_dir_escape_rejected() {
        let dir = tempdir().expect("tmp");
        let root = dir.path().join("matters");
        std::fs::create_dir_all(&root).expect("mkdir");
        let bad = root.join("..").join("escape");
        let err = assert_path_under_root(&bad, &root).expect_err("escape");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn mixed_separators_parent_escape_rejected() {
        let dir = tempdir().expect("tmp");
        let root = dir.path().join("matters");
        std::fs::create_dir_all(&root).expect("mkdir");
        // Mixed `..\` style: Path::new understands both separators on Windows.
        let bad = root.join(r"child\..\..\escape");
        let err = assert_path_under_root(&bad, &root).expect_err("mixed escape");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn foreign_absolute_rejected() {
        let dir = tempdir().expect("tmp");
        let root = dir.path();
        let foreign = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32")
        } else {
            PathBuf::from("/etc")
        };
        if !foreign.exists() {
            return;
        }
        let err = assert_path_under_root(&foreign, root).expect_err("foreign");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn non_existent_foreign_absolute_rejected() {
        let dir = tempdir().expect("tmp");
        let root = dir.path();
        let foreign = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\DoesNotExist-pst-dedupe-0063\case")
        } else {
            PathBuf::from("/var/does-not-exist-pst-dedupe-0063/case")
        };
        let err = assert_path_under_root(&foreign, root).expect_err("nonexist foreign");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn reject_untrusted_path_component_blocks_traversal() {
        assert!(reject_untrusted_path_component("firm-a").is_ok());
        assert!(reject_untrusted_path_component("case_1").is_ok());
        assert!(reject_untrusted_path_component("").is_err());
        assert!(reject_untrusted_path_component(".").is_err());
        assert!(reject_untrusted_path_component("..").is_err());
        assert!(reject_untrusted_path_component("a/b").is_err());
        assert!(reject_untrusted_path_component(r"a\b").is_err());
        assert!(reject_untrusted_path_component("C:").is_err());
        assert!(reject_untrusted_path_component("foo:bar").is_err());
        assert!(reject_untrusted_path_component("a\0b").is_err());
        if cfg!(windows) {
            assert!(reject_untrusted_path_component(r"C:\Windows").is_err());
        } else {
            assert!(reject_untrusted_path_component("/etc").is_err());
        }
    }

    #[test]
    fn document_join_absolute_override() {
        // Path::join replaces base when RHS is absolute — callers must not trust join alone.
        let dir = tempdir().expect("tmp");
        let root = dir.path();
        let absolute = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32")
        } else {
            PathBuf::from("/etc")
        };
        let joined = root.join(&absolute);
        // On Windows/Unix, join of absolute yields the absolute path (escape).
        assert_eq!(joined, absolute);
        // Sandbox must still reject.
        let err = assert_path_under_root(&joined, root).expect_err("joined abs");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }
}
