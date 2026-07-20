//! Path resolution (track 0045 §3.2.4).
//!
//! - CLI argument paths: relative to process CWD, normalized absolute when possible.
//! - Data paths inside JSON params: must already be absolute.

use std::path::{Component, Path, PathBuf};

use camino::Utf8PathBuf;
use serde_json::Value;

use crate::error::{CliError, Result};

/// Known JSON object keys that hold filesystem data paths (must be absolute).
const PATH_KEYS: &[&str] = &[
    "path",
    "source_path",
    "output_dir",
    "out",
    "pst_path",
    "package_path",
    "file",
    "report_dir",
    "export_dir",
    "dat_path",
    "opposing_dat",
    "denist_path",
    "hash_list_path",
];

/// Resolve a CLI argument path relative to CWD and normalize to absolute.
pub fn resolve_cli_path(path: impl AsRef<Path>) -> Result<Utf8PathBuf> {
    let p = path.as_ref();
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| CliError::Usage(format!("cannot resolve path relative to CWD: {e}")))?;
        cwd.join(p)
    };
    let canonical = std::fs::canonicalize(&abs).unwrap_or(abs);
    path_buf_to_utf8(canonical)
}

/// Resolve a CLI path that may not exist yet (create parent may be needed later).
///
/// Uses CWD join + `dunce`-style absolute without requiring the path to exist.
pub fn resolve_cli_path_maybe_missing(path: impl AsRef<Path>) -> Result<Utf8PathBuf> {
    let p = path.as_ref();
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| CliError::Usage(format!("cannot resolve path relative to CWD: {e}")))?;
        cwd.join(p)
    };
    // Prefer canonicalize when path exists; else absolutize components.
    let resolved = if abs.exists() {
        std::fs::canonicalize(&abs).unwrap_or_else(|_| normalize_abs(&abs))
    } else {
        normalize_abs(&abs)
    };
    path_buf_to_utf8(resolved)
}

fn normalize_abs(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn path_buf_to_utf8(p: PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(p)
        .map_err(|p| CliError::Usage(format!("path is not valid UTF-8: {}", p.display())))
}

/// True when `s` looks like an absolute filesystem path on this platform.
pub fn is_absolute_path_str(s: &str) -> bool {
    let p = Path::new(s);
    p.is_absolute()
}

/// Walk JSON and reject relative values under known path keys.
///
/// Only string leaves under documented keys are checked; unknown keys are left
/// to the engine (spec residual).
pub fn validate_params_paths_absolute(value: &Value) -> Result<()> {
    validate_value(value)
}

fn validate_value(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if let Value::String(s) = v {
                    if PATH_KEYS.contains(&k.as_str()) && !s.is_empty() && !is_absolute_path_str(s)
                    {
                        return Err(CliError::Usage(format!(
                            "data path in params for key '{k}' must be absolute (got '{s}'); \
                             use absolute path (CLI args may be CWD-relative)"
                        )));
                    }
                } else {
                    validate_value(v)?;
                }
            }
            Ok(())
        }
        Value::Array(arr) => {
            for item in arr {
                validate_value(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Load params JSON from inline string or `@file` form.
pub fn load_params_json(raw: Option<&str>) -> Result<Value> {
    match raw {
        None => Ok(serde_json::json!({})),
        Some(s) if s.trim().is_empty() => Ok(serde_json::json!({})),
        Some(s) if s.starts_with('@') => {
            let file = s.trim_start_matches('@');
            if file.is_empty() {
                return Err(CliError::Usage("empty @file path for --params-json".into()));
            }
            let path = resolve_cli_path(file)?;
            let text = std::fs::read_to_string(path.as_std_path())
                .map_err(|e| CliError::Usage(format!("cannot read params file {path}: {e}")))?;
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| CliError::Usage(format!("invalid JSON in params file {path}: {e}")))?;
            if !v.is_object() {
                return Err(CliError::Usage("params JSON must be a JSON object".into()));
            }
            validate_params_paths_absolute(&v)?;
            Ok(v)
        }
        Some(s) => {
            let v: Value = serde_json::from_str(s)
                .map_err(|e| CliError::Usage(format!("invalid --params-json: {e}")))?;
            if !v.is_object() {
                return Err(CliError::Usage("params JSON must be a JSON object".into()));
            }
            validate_params_paths_absolute(&v)?;
            Ok(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_windows_style() {
        assert!(is_absolute_path_str(r"C:\Matters\m1"));
        assert!(is_absolute_path_str(r"\\server\share\x"));
        assert!(!is_absolute_path_str(r"relative\path"));
        assert!(!is_absolute_path_str("foo.json"));
    }

    #[test]
    fn rejects_relative_path_key() {
        let v = serde_json::json!({ "path": "relative/pkg.zip" });
        let err = validate_params_paths_absolute(&v).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn accepts_absolute_path_key() {
        let v = serde_json::json!({ "path": r"C:\data\pkg.zip", "force": true });
        validate_params_paths_absolute(&v).unwrap();
    }

    #[test]
    fn nested_relative_rejected() {
        let v = serde_json::json!({ "nested": { "output_dir": "out/here" } });
        assert!(validate_params_paths_absolute(&v).is_err());
    }
}
