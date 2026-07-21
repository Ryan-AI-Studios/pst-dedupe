//! PLATFORM_STORAGE_ROOT path sandbox.

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Env var for the allowed storage root (single root for P0).
pub const ENV_PLATFORM_STORAGE_ROOT: &str = "PLATFORM_STORAGE_ROOT";

/// Canonicalize and require `path` to be a **strict subdirectory** of `allowed_root`.
///
/// Rejects: missing root, `..` escape, path equal to root, foreign absolute paths.
pub fn assert_path_under_root(path: &Path, allowed_root: &Path) -> Result<PathBuf> {
    let root = canonicalize_strict(allowed_root).map_err(|e| {
        Error::PathNotSandboxed(format!(
            "PLATFORM_STORAGE_ROOT invalid ({}): {e}",
            allowed_root.display()
        ))
    })?;

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
                out.push(c.as_os_str());
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
}
