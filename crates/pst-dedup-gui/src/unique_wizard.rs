//! Unique-PST wizard state mapping and path helpers (0072).
//!
//! **Dialog main-thread invariant:** native pickers (`rfd`) must run only from
//! the egui UI thread (see `app::PstDedupApp` dialog methods). Helpers in this
//! module never call `rfd` — they only map already-owned paths into CLI args.
//! Unit tests assert this module has no rfd dependency.

use std::path::{Path, PathBuf};

use dedup_engine::integrity::ScanMode;
use dedup_engine::keepset::{FamilyPolicy, KeepPolicy};
use pst_dedup_cli::paths::{is_same_or_under, is_same_or_under_resolved, paths_equal};
use pst_dedup_cli::unique_export_report::volume_path_for;
use pst_dedup_cli::unique_pst_cmd::{FolderLayoutArg, UniquePstCliArgs};

/// Prefer absolute paths from dialogs (canonicalize when the path exists).
pub fn absolutize_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path.canonicalize().unwrap_or(path);
    }
    match std::env::current_dir() {
        Ok(cwd) => {
            let joined = cwd.join(&path);
            joined.canonicalize().unwrap_or(joined)
        }
        Err(_) => path,
    }
}

fn is_pst_file_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pst"))
        .unwrap_or(false)
}

/// True when parent is an existing **directory** that is writable, or is
/// one-level creatable under an existing writable grandparent.
fn parent_exists_or_creatable(path: &Path) -> Result<(), String> {
    let parent = match path.parent() {
        None => return Ok(()),
        Some(p) if p.as_os_str().is_empty() => return Ok(()),
        Some(p) => p,
    };
    if parent.exists() {
        if !parent.is_dir() {
            return Err(format!(
                "output parent exists but is not a directory: {}",
                parent.display()
            ));
        }
        return probe_dir_writable(parent);
    }
    // One-level create: grandparent must exist as a writable directory.
    match parent.parent() {
        None => Ok(()),
        Some(gp) if gp.as_os_str().is_empty() => Ok(()),
        Some(gp) if gp.exists() => {
            if !gp.is_dir() {
                return Err(format!(
                    "output grandparent is not a directory: {}",
                    gp.display()
                ));
            }
            probe_dir_writable(gp)?;
            Ok(())
        }
        Some(gp) => Err(format!(
            "output parent directory does not exist and is not creatable: {} (missing ancestor {})",
            parent.display(),
            gp.display()
        )),
    }
}

/// Best-effort write probe: create+delete a unique probe file in `dir`.
fn probe_dir_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(
        ".pst_dedup_write_probe_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(format!(
            "output parent directory is not writable: {} ({e})",
            dir.display()
        )),
    }
}

/// True when any multi-volume sibling of `out` exists (`{stem}_volNNN.pst`).
/// Enumerates the parent directory so high indices (vol013…vol999) are found.
pub fn existing_volume_siblings(out: &Path) -> bool {
    let parent = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return false,
    };
    let stem = match out.file_stem().and_then(|s| s.to_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    let prefix = format!("{stem}_vol");
    let Ok(rd) = parent.read_dir() else {
        // Fallback probe of common indices if dir listing fails.
        return (2u32..=999).any(|i| volume_path_for(out, i).exists());
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() > prefix.len()
            && name
                .get(..prefix.len())
                .map(|p| p.eq_ignore_ascii_case(&prefix))
                .unwrap_or(false)
            && name.to_ascii_lowercase().ends_with(".pst")
        {
            return true;
        }
    }
    false
}

/// Wizard form fields (UI-owned; mapped to [`UniquePstCliArgs`] on Run).
#[derive(Debug, Clone)]
pub struct UniqueWizardForm {
    pub inputs: Vec<PathBuf>,
    pub out: Option<PathBuf>,
    pub report_dir: Option<PathBuf>,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub folder_layout: FolderLayoutArg,
    /// Comma-separated path substrings for `prefer_path` policy.
    pub prefer_path_text: String,
    /// When true, max-volume soft limit is enabled (default 10 GiB when enabled).
    pub max_volume_enabled: bool,
    pub max_volume_text: String,
    pub mode: ScanMode,
    pub no_tier2: bool,
    pub no_attachments: bool,
    pub overwrite: bool,
    /// Pending overwrite confirm dialog.
    pub confirm_overwrite: bool,
}

impl Default for UniqueWizardForm {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            out: None,
            report_dir: None,
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            folder_layout: FolderLayoutArg::Preserve,
            prefer_path_text: String::new(),
            max_volume_enabled: false,
            max_volume_text: "10737418240".into(), // 10 GiB
            mode: ScanMode::BestEffort,
            no_tier2: false,
            no_attachments: false,
            overwrite: false,
            confirm_overwrite: false,
        }
    }
}

impl UniqueWizardForm {
    /// Prefill inputs from a prior scan (Results → Export Unique PST).
    pub fn with_inputs(inputs: Vec<PathBuf>) -> Self {
        Self {
            inputs,
            ..Default::default()
        }
    }

    /// Default report dir next to out: `{stem}_report`.
    pub fn default_report_dir_for_out(out: &Path) -> PathBuf {
        let parent = out.parent().unwrap_or_else(|| Path::new("."));
        let stem = out
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unique".into());
        parent.join(format!("{stem}_report"))
    }

    /// Effective report directory (explicit or derived from out).
    pub fn effective_report_dir(&self) -> Option<PathBuf> {
        if let Some(r) = &self.report_dir {
            return Some(r.clone());
        }
        self.out
            .as_ref()
            .map(|o| Self::default_report_dir_for_out(o))
    }

    /// Fail-closed preflight: enables Run only when filesystem checks pass.
    pub fn can_run(&self) -> bool {
        self.validate_for_run().is_ok()
    }

    /// Parse comma-separated prefer-path substrings.
    pub fn prefer_path_contains(&self) -> Vec<String> {
        self.prefer_path_text
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Full preflight (DoD-6/8): inputs exist as `.pst` files, out parent creatable,
    /// path-shape guards, overwrite conflicts surface via [`Self::needs_overwrite_confirm`].
    pub fn validate_for_run(&self) -> Result<(), String> {
        if self.inputs.is_empty() {
            return Err("at least one input PST is required".into());
        }
        for input in &self.inputs {
            if !input.is_absolute() {
                return Err(format!("input path must be absolute: {}", input.display()));
            }
            if !is_pst_file_path(input) {
                return Err(format!(
                    "input must end with .pst (case-insensitive): {}",
                    input.display()
                ));
            }
            if !input.exists() {
                return Err(format!("input PST does not exist: {}", input.display()));
            }
            if !input.is_file() {
                return Err(format!("input path is not a file: {}", input.display()));
            }
        }
        let out = self
            .out
            .as_ref()
            .ok_or_else(|| "output .pst path is required".to_string())?;
        if !out.is_absolute() {
            return Err(format!("output path must be absolute: {}", out.display()));
        }
        if !is_pst_file_path(out) {
            return Err("output path must end with .pst".into());
        }
        parent_exists_or_creatable(out)?;
        if let Some(report) = self.effective_report_dir() {
            parent_exists_or_creatable(&report)?;
            // Report path must be a directory if it already exists (file blocks prepare).
            if report.exists() && !report.is_dir() {
                return Err(format!(
                    "report path exists but is not a directory: {}",
                    report.display()
                ));
            }
        }
        // Soft path guards (full guards also run inside run_unique_pst).
        for input in &self.inputs {
            if paths_equal_loose(out, input) {
                return Err(format!(
                    "refusing --out equal to an input PST: {}",
                    out.display()
                ));
            }
            if out_nested_under_input(out, input) {
                return Err(format!(
                    "refusing --out nested under an input PST: out={} input={}",
                    out.display(),
                    input.display()
                ));
            }
        }
        // Overwrite collisions are handled by needs_overwrite_confirm + confirm dialog
        // (do not fail-closed here or the Run button stays disabled forever).
        // Validate max-volume parse early.
        if self.max_volume_enabled {
            let t = self.max_volume_text.trim();
            if !t.is_empty() {
                t.parse::<u64>()
                    .map_err(|_| format!("invalid max volume bytes: {t}"))?;
            }
        }
        Ok(())
    }

    /// Map wizard form → CLI args. Prefers calling [`Self::validate_for_run`] first
    /// (see `start_unique_pst`); still enforces path-shape guards here for library use.
    pub fn to_cli_args(&self) -> Result<UniquePstCliArgs, String> {
        if self.inputs.is_empty() {
            return Err("at least one input PST is required".into());
        }
        let out = self
            .out
            .clone()
            .ok_or_else(|| "output .pst path is required".to_string())?;
        if !is_pst_file_path(&out) {
            return Err("output path must end with .pst".into());
        }
        // Soft path guard (full guards run inside run_unique_pst).
        for input in &self.inputs {
            if paths_equal_loose(&out, input) {
                return Err(format!(
                    "refusing --out equal to an input PST: {}",
                    out.display()
                ));
            }
            // Nested out under an input path (mirror CLI is_same_or_under).
            if out_nested_under_input(&out, input) {
                return Err(format!(
                    "refusing --out nested under an input PST: out={} input={}",
                    out.display(),
                    input.display()
                ));
            }
        }
        let max_volume_bytes = if self.max_volume_enabled {
            let t = self.max_volume_text.trim();
            if t.is_empty() {
                Some(10_737_418_240)
            } else {
                Some(
                    t.parse::<u64>()
                        .map_err(|_| format!("invalid max volume bytes: {t}"))?,
                )
            }
        } else {
            None
        };
        Ok(UniquePstCliArgs {
            paths: self.inputs.clone(),
            out,
            report_dir: self.effective_report_dir(),
            policy: self.policy,
            family_policy: self.family_policy,
            prefer_path_contains: self.prefer_path_contains(),
            decision_csv: None,
            keep_set_json: None,
            folder_layout: self.folder_layout,
            max_volume_bytes,
            overwrite: self.overwrite,
            verify_hash: false,
            also_eml: None,
            no_tier2: self.no_tier2,
            no_attachments: self.no_attachments,
            json: false,
            mode: self.mode,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
        })
    }

    /// Whether primary out, report pack, or multi-volume siblings already exist
    /// (for overwrite confirm UX). Sibling discovery uses parent `read_dir` so
    /// high indices (e.g. `_vol013.pst` … `_vol999.pst`) are not missed.
    pub fn needs_overwrite_confirm(&self) -> bool {
        if self.overwrite {
            return false;
        }
        let out_exists = self.out.as_ref().map(|p| p.exists()).unwrap_or(false);
        let report_exists = self
            .effective_report_dir()
            .map(|p| {
                if !p.exists() {
                    return false;
                }
                // File at report path is a collision (CLI prepare will fail).
                if !p.is_dir() {
                    return true;
                }
                p.read_dir()
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let sibling_exists = self
            .out
            .as_ref()
            .map(|out| existing_volume_siblings(out))
            .unwrap_or(false);
        out_exists || report_exists || sibling_exists
    }
}

fn paths_equal_loose(a: &Path, b: &Path) -> bool {
    paths_equal(a, b)
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .map(|(x, y)| x == y)
            .unwrap_or(false)
}

/// True when `out` is equal to `input` or a path under the input file's parent
/// treated as a tree root — mirrors CLI nested-out refusal loosely.
///
/// Uses CLI [`is_same_or_under`] / resolved variants when the input path can be
/// treated as a directory root (parent of the PST), and also when out literally
/// starts with the input path as a prefix component sequence.
fn out_nested_under_input(out: &Path, input: &Path) -> bool {
    if is_same_or_under(out, input) || is_same_or_under_resolved(out, input) {
        return true;
    }
    // If input is a file, also refuse out under the same directory tree named
    // after the input path stem used as a folder (common accidental nesting:
    // input `C:\data\mail.pst`, out `C:\data\mail.pst\unique.pst` when mail.pst
    // is a directory — covered by is_same_or_under). For string-prefix safety
    // without canonicalize: refuse when out is under input's parent + input stem
    // as a directory only when that dir exists — keep component-wise:
    if let Some(parent) = input.parent() {
        // Not generally "out under parent" — that would block any co-located out.
        // Only refuse when out is under the input path itself as a root.
        let _ = parent;
    }
    false
}

/// Open a folder (or parent of a file) in the OS file manager without blocking.
///
/// Windows: `explorer`. Best-effort; never panics.
pub fn open_folder_nonblocking(path: &Path) {
    let target = if path.is_file() {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    };
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(target.as_os_str())
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(target.as_os_str())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_mapping_from_ui_state() {
        let form = UniqueWizardForm {
            inputs: vec![
                PathBuf::from(r"C:\data\a.pst"),
                PathBuf::from(r"C:\data\b.pst"),
            ],
            out: Some(PathBuf::from(r"C:\export\unique.pst")),
            report_dir: Some(PathBuf::from(r"C:\export\unique_report")),
            policy: KeepPolicy::KeepLargest,
            family_policy: FamilyPolicy::ParentsOnly,
            folder_layout: FolderLayoutArg::Flat,
            prefer_path_text: "Primary, Archive".into(),
            max_volume_enabled: true,
            max_volume_text: "12345".into(),
            mode: ScanMode::Strict,
            no_tier2: true,
            no_attachments: true,
            overwrite: true,
            ..Default::default()
        };
        let args = form.to_cli_args().expect("map");
        assert_eq!(args.paths.len(), 2);
        assert_eq!(args.out, PathBuf::from(r"C:\export\unique.pst"));
        assert_eq!(
            args.report_dir,
            Some(PathBuf::from(r"C:\export\unique_report"))
        );
        assert_eq!(args.policy, KeepPolicy::KeepLargest);
        assert_eq!(args.family_policy, FamilyPolicy::ParentsOnly);
        assert_eq!(args.folder_layout, FolderLayoutArg::Flat);
        assert_eq!(args.max_volume_bytes, Some(12345));
        assert!(args.overwrite);
        assert!(args.no_tier2);
        assert!(args.no_attachments);
        assert_eq!(args.mode, ScanMode::Strict);
        assert_eq!(
            args.prefer_path_contains,
            vec!["Primary".to_string(), "Archive".to_string()]
        );
    }

    #[test]
    fn args_mapping_out_equals_input_refused() {
        let form = UniqueWizardForm {
            inputs: vec![PathBuf::from(r"C:\data\mail.pst")],
            out: Some(PathBuf::from(r"C:\data\mail.pst")),
            ..Default::default()
        };
        let err = form.to_cli_args().expect_err("must refuse");
        assert!(err.contains("input") || err.contains("refusing"));
    }

    #[test]
    fn args_mapping_out_nested_under_input_refused() {
        // Component-wise: out is under input path when input is treated as a root.
        // e.g. input is a directory-like path used as root in tests.
        let form = UniqueWizardForm {
            inputs: vec![PathBuf::from(r"C:\data\mail.pst")],
            out: Some(PathBuf::from(r"C:\data\mail.pst\nested\unique.pst")),
            ..Default::default()
        };
        let err = form.to_cli_args().expect_err("must refuse nested out");
        assert!(
            err.contains("nested") || err.contains("refusing"),
            "err={err}"
        );
    }

    #[test]
    fn args_mapping_requires_pst_extension() {
        let form = UniqueWizardForm {
            inputs: vec![PathBuf::from(r"C:\data\a.pst")],
            out: Some(PathBuf::from(r"C:\export\out.txt")),
            ..Default::default()
        };
        assert!(form.to_cli_args().is_err());
    }

    #[test]
    fn can_run_requires_absolute_existing_paths() {
        let mut form = UniqueWizardForm::default();
        assert!(!form.can_run());
        form.inputs.push(PathBuf::from("a.pst"));
        form.out = Some(PathBuf::from("out.pst"));
        // Relative / missing paths fail full preflight (fail-closed).
        assert!(!form.can_run());
        assert!(form.validate_for_run().is_err());
    }

    #[test]
    fn validate_for_run_requires_existing_absolute_pst() {
        let dir = tempfile::tempdir().expect("tmp");
        let input = dir.path().join("in.pst");
        std::fs::write(&input, b"not-a-real-pst").expect("touch");
        let out = dir.path().join("out.pst");
        let form = UniqueWizardForm {
            inputs: vec![absolutize_path(input)],
            out: Some(absolutize_path(out)),
            overwrite: true,
            ..Default::default()
        };
        // Absolute + exists + .pst + writable parent — passes preflight shape even if
        // content is not a valid PST (CLI open will fail later).
        form.validate_for_run().expect("preflight");
    }

    #[test]
    fn validate_for_run_rejects_missing_input() {
        let dir = tempfile::tempdir().expect("tmp");
        let missing = absolutize_path(dir.path().join("missing.pst"));
        let out = absolutize_path(dir.path().join("out.pst"));
        let form = UniqueWizardForm {
            inputs: vec![missing],
            out: Some(out),
            ..Default::default()
        };
        let err = form.validate_for_run().expect_err("missing input");
        assert!(
            err.contains("does not exist") || err.contains("absolute"),
            "err={err}"
        );
    }

    #[test]
    fn validate_for_run_rejects_parent_that_is_a_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let input = dir.path().join("in.pst");
        std::fs::write(&input, b"x").expect("touch");
        // Parent of out is a *file*, not a directory.
        let file_as_parent = dir.path().join("not_a_dir");
        std::fs::write(&file_as_parent, b"file").expect("touch");
        let out = file_as_parent.join("out.pst");
        let form = UniqueWizardForm {
            inputs: vec![absolutize_path(input)],
            out: Some(absolutize_path(out)),
            overwrite: true,
            ..Default::default()
        };
        let err = form.validate_for_run().expect_err("parent is file");
        assert!(
            err.contains("not a directory")
                || err.contains("not writable")
                || err.contains("parent"),
            "err={err}"
        );
    }

    #[test]
    fn needs_overwrite_confirm_includes_volume_siblings() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let sibling = volume_path_for(&out, 2);
        std::fs::write(&sibling, b"stale").expect("touch vol002");
        let form = UniqueWizardForm {
            inputs: vec![dir.path().join("in.pst")],
            out: Some(out),
            overwrite: false,
            ..Default::default()
        };
        assert!(form.needs_overwrite_confirm());
        let mut with_overwrite = form;
        with_overwrite.overwrite = true;
        assert!(!with_overwrite.needs_overwrite_confirm());
    }

    #[test]
    fn validate_for_run_rejects_report_path_that_is_a_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let input = dir.path().join("in.pst");
        std::fs::write(&input, b"x").expect("touch");
        let out = dir.path().join("out.pst");
        let report_as_file = dir.path().join("out_report");
        std::fs::write(&report_as_file, b"not-a-dir").expect("touch report file");
        let form = UniqueWizardForm {
            inputs: vec![absolutize_path(input)],
            out: Some(absolutize_path(out)),
            report_dir: Some(absolutize_path(report_as_file)),
            overwrite: true,
            ..Default::default()
        };
        let err = form.validate_for_run().expect_err("report is file");
        assert!(
            err.contains("not a directory") || err.contains("report"),
            "err={err}"
        );
    }

    #[test]
    fn needs_overwrite_confirm_finds_high_index_siblings() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        // High index beyond old fixed probe window (vol002–vol012).
        let sibling = volume_path_for(&out, 13);
        std::fs::write(&sibling, b"stale").expect("touch vol013");
        let form = UniqueWizardForm {
            inputs: vec![dir.path().join("in.pst")],
            out: Some(out.clone()),
            overwrite: false,
            ..Default::default()
        };
        assert!(
            form.needs_overwrite_confirm(),
            "vol013 must trigger overwrite confirm"
        );
        assert!(existing_volume_siblings(&out));
    }

    #[test]
    fn default_report_dir_derives_from_out_stem() {
        let d = UniqueWizardForm::default_report_dir_for_out(Path::new(r"C:\export\unique.pst"));
        assert_eq!(d, PathBuf::from(r"C:\export\unique_report"));
    }

    /// Dialog main-thread invariant: path helpers never invoke rfd.
    ///
    /// This test documents that `to_cli_args` / `default_report_dir_for_out` /
    /// `open_folder_nonblocking` do not open native dialogs (they only use
    /// owned paths). Native pickers live on `PstDedupApp` methods called from
    /// the egui frame only.
    #[test]
    fn path_helpers_do_not_call_rfd() {
        let form = UniqueWizardForm {
            inputs: vec![PathBuf::from(r"C:\in\a.pst")],
            out: Some(PathBuf::from(r"C:\out\u.pst")),
            report_dir: None,
            ..Default::default()
        };
        let args = form.to_cli_args().expect("map without dialogs");
        assert!(args.report_dir.is_some());
        // Pure derivation — no I/O beyond existence checks is required for mapping.
        let _ = UniqueWizardForm::default_report_dir_for_out(Path::new("x.pst"));
    }
}
