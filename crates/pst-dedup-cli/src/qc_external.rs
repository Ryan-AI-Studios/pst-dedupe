//! External QC sidecars (track 0080 §3.5 Tier B, §3.6 Tier C).
//!
//! - Independent readers (`pffinfo` / `readpst`): **BYOB path only**, counts-only, skip-safe.
//! - `scanpst.exe`: local temp copy, `-no repair` verified-or-skip, log parse, timeout+kill.
//!
//! Never downloads binaries. Never repairs the deliverable in place. No copyleft deps.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Default wall-clock timeout for external tools.
pub const DEFAULT_EXTERNAL_TIMEOUT: Duration = Duration::from_secs(120);

/// Minimum scanpst build that has CLI args (Outlook 2016 v1807).
pub const SCANPST_MIN_BUILD: &str = "16.0.10325.20082";

/// Status of an external sidecar run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalStatus {
    Skipped,
    Ok,
    Failed,
    Timeout,
}

/// Independent reader (libpff/libpst) result — counts only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndependentReaderResult {
    pub status: ExternalStatus,
    pub reason: Option<String>,
    pub tool: Option<String>,
    pub version: Option<String>,
    pub message_count: Option<u64>,
    pub folder_count: Option<u64>,
    pub exit_code: Option<i32>,
}

impl IndependentReaderResult {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: ExternalStatus::Skipped,
            reason: Some(reason.into()),
            tool: None,
            version: None,
            message_count: None,
            folder_count: None,
            exit_code: None,
        }
    }
}

/// scanpst structural validation result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanpstResult {
    pub status: ExternalStatus,
    pub reason: Option<String>,
    pub build: Option<String>,
    pub log_path: Option<String>,
    pub bak_present: bool,
    pub log_summary: Option<String>,
    /// True when a hard QC error was raised (e.g. `.bak` appeared).
    pub hard_error: bool,
}

impl ScanpstResult {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: ExternalStatus::Skipped,
            reason: Some(reason.into()),
            build: None,
            log_path: None,
            bak_present: false,
            log_summary: None,
            hard_error: false,
        }
    }
}

/// Run an operator-supplied independent reader against a **local copy** of the PST.
///
/// `tool_path` must be an **absolute** filesystem path (BYOB — never a URL or relative path).
/// `Ok` requires parseable `message_count`; exit 0 alone is **not** enough (DoD-12).
pub fn run_independent_reader(
    tool_path: &Path,
    pst_path: &Path,
    timeout: Duration,
) -> IndependentReaderResult {
    if tool_path
        .to_str()
        .is_some_and(|s| s.contains("://") || s.starts_with("http"))
    {
        return IndependentReaderResult::skipped(
            "BYOB path only — URL/fetch not accepted (rule 15)",
        );
    }
    if !tool_path.is_absolute() {
        return IndependentReaderResult::skipped(
            "BYOB absolute path required — relative external reader path not accepted (DoD-12)",
        );
    }
    if !tool_path.is_file() {
        return IndependentReaderResult::skipped(format!(
            "external reader not found: {}",
            tool_path.display()
        ));
    }
    if !pst_path.is_file() {
        return IndependentReaderResult::skipped(format!("pst not found: {}", pst_path.display()));
    }

    let tool_name = tool_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("external")
        .to_ascii_lowercase();

    // Copy to local temp — never validate deliverable in place (networked .pst unsupported).
    let temp_dir = std::env::temp_dir().join(format!(
        "pst-dedup-qc-ext-{}-{}",
        std::process::id(),
        unique_stamp()
    ));
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        return IndependentReaderResult::skipped(format!("temp dir: {e}"));
    }
    let temp_pst = temp_dir.join("probe.pst");
    if let Err(e) = fs::copy(pst_path, &temp_pst) {
        let _ = fs::remove_dir_all(&temp_dir);
        return IndependentReaderResult::skipped(format!("copy: {e}"));
    }

    let mut cmd = Command::new(tool_path);
    // pffinfo: path; readpst: -c count mode when supported, else path only.
    if tool_name.contains("readpst") {
        cmd.arg("-c").arg(&temp_pst);
    } else {
        cmd.arg(&temp_pst);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let run = run_with_timeout(cmd, timeout);
    let _ = fs::remove_dir_all(&temp_dir);

    match run {
        RunOutcome::Timeout => IndependentReaderResult {
            status: ExternalStatus::Timeout,
            reason: Some("EXTERNAL_READER_TIMEOUT".into()),
            tool: Some(tool_name),
            version: None,
            message_count: None,
            folder_count: None,
            exit_code: None,
        },
        RunOutcome::Failed(e) => IndependentReaderResult::skipped(e),
        RunOutcome::Done { status, stdout, .. } => {
            let (messages, folders) = parse_reader_counts(&stdout);
            let code = status.code();
            // Ok requires parseable message_count (exit 0 alone is insufficient — DoD-12).
            if let Some(msg_count) = messages {
                IndependentReaderResult {
                    status: ExternalStatus::Ok,
                    reason: None,
                    tool: Some(tool_name),
                    version: None,
                    message_count: Some(msg_count),
                    folder_count: folders,
                    exit_code: code,
                }
            } else if code == Some(0) {
                IndependentReaderResult::skipped(
                    "exit 0 but no parseable message_count — not Ok (DoD-12 counts required)",
                )
            } else {
                IndependentReaderResult {
                    status: ExternalStatus::Failed,
                    reason: Some("could not parse message_count from reader output".into()),
                    tool: Some(tool_name),
                    version: None,
                    message_count: None,
                    folder_count: folders,
                    exit_code: code,
                }
            }
        }
    }
}

/// Discover classic Outlook `SCANPST.EXE` via Click-to-Run roots + common install paths.
///
/// Never hardcodes Office16 only. Returns first existing path + version guess from parent.
pub fn discover_scanpst() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Click-to-Run Office roots (version-agnostic).
    for root in [
        r"C:\Program Files\Microsoft Office\root\Office16",
        r"C:\Program Files (x86)\Microsoft Office\root\Office16",
        r"C:\Program Files\Microsoft Office\root\Office15",
        r"C:\Program Files (x86)\Microsoft Office\root\Office15",
    ] {
        candidates.push(PathBuf::from(root).join("SCANPST.EXE"));
    }

    // Legacy MSI layouts (any OfficeNN under Program Files).
    for base in [
        r"C:\Program Files\Microsoft Office",
        r"C:\Program Files (x86)\Microsoft Office",
    ] {
        let base = PathBuf::from(base);
        if let Ok(entries) = fs::read_dir(&base) {
            for ent in entries.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if name.starts_with("Office") {
                    candidates.push(ent.path().join("SCANPST.EXE"));
                }
            }
        }
    }

    // Registry probe via `reg query` (skip-safe if reg missing).
    if let Some(from_reg) = scanpst_from_registry() {
        candidates.insert(0, from_reg);
    }

    candidates.into_iter().find(|p| p.is_file())
}

fn scanpst_from_registry() -> Option<PathBuf> {
    // Probe Outlook install path; avoid hardcoding Office16.
    let keys = [
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\OUTLOOK.EXE",
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\OUTLOOK.EXE",
    ];
    for key in keys {
        let output = Command::new("reg")
            .args(["query", key, "/ve"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(idx) = line.find("REG_SZ") {
                let path_str = line[idx + 6..].trim();
                if path_str.is_empty() {
                    continue;
                }
                let outlook = PathBuf::from(path_str);
                if let Some(dir) = outlook.parent() {
                    let scanpst = dir.join("SCANPST.EXE");
                    if scanpst.is_file() {
                        return Some(scanpst);
                    }
                }
            }
        }
    }
    None
}

/// Compare two dotted version strings (e.g. `16.0.10325.20082`). Returns true if `found >= min`.
pub fn version_at_least(found: &str, min: &str) -> bool {
    let parse =
        |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse::<u64>().ok()).collect() };
    let a = parse(found);
    let b = parse(min);
    let n = a.len().max(b.len());
    for i in 0..n {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        if av != bv {
            return av > bv;
        }
    }
    true
}

/// Resolve scanpst build only from **verified** sources — never invent from folder name.
///
/// Sources (in order):
/// 1. Sibling `.version` file next to the binary (operator / test pin)
/// 2. `PST_DEDUP_SCANPST_BUILD` environment variable
///
/// Office16/Office15 path segments alone are **not** treated as a verified minimum
/// build (rule 2 / D-0080-scanpst-arg). Unverifiable ⇒ `None` ⇒ skip.
pub fn guess_scanpst_build(scanpst_path: &Path) -> Option<String> {
    let ver_file = scanpst_path.with_extension("version");
    if ver_file.is_file() {
        if let Ok(v) = fs::read_to_string(&ver_file) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    if let Ok(v) = std::env::var("PST_DEDUP_SCANPST_BUILD") {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let _ = scanpst_path; // path folder name is not a version probe
    None
}

/// Verify that the installed scanpst **documents** the exact `-no repair` token.
///
/// **Honesty (DoD-13 / rule 2):** `.accepts-no-repair` files and
/// `PST_DEDUP_SCANPST_NO_REPAIR_OK` env markers are **not** accepted as proof —
/// they can green-wash an incompatible binary that silently enters the legacy
/// repairing path.
///
/// Only a help/usage probe that clearly documents "no repair" counts here.
/// CI stubs that cannot print help may still prove acceptance **after** the run
/// by writing `NO_REPAIR_MODE` into the log (see [`log_proves_no_repair_mode`]
/// and the stub contract on [`run_scanpst`]).
///
/// Unverifiable ⇒ `Err` (caller skips Ok unless log proves mode).
pub fn verify_scanpst_no_repair(scanpst_path: &Path) -> Result<(), String> {
    // Help probe — look for "no repair" (case-insensitive) in usage text.
    for help_arg in ["-?", "/?", "-help", "--help"] {
        let mut cmd = Command::new(scanpst_path);
        cmd.arg(help_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let run = run_with_timeout(cmd, Duration::from_secs(5));
        if let RunOutcome::Done { stdout, stderr, .. } = run {
            let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
            // Require clear documentation of the two-token form (not just "repair").
            if combined.contains("no repair") || combined.contains("-no repair") {
                return Ok(());
            }
        }
    }
    Err(
        "scanpst -no repair support unverifiable via help/version; skip rather than risk repair (rule 2 / D-0080-scanpst-arg)"
            .into(),
    )
}

/// CI stub contract: log line proving the process accepted `-no repair`.
///
/// Production Microsoft scanpst logs do not emit this token. Stubs used in tests
/// **must** write `NO_REPAIR_MODE` (case-insensitive) when they honor `-no repair`
/// args, together with a recognized success marker, before QC may report Ok.
pub fn log_proves_no_repair_mode(text: &str) -> bool {
    text.to_ascii_lowercase().contains("no_repair_mode")
}

/// Recognized scanpst **success** log markers (case-insensitive).
///
/// Non-empty logs without a success marker are **never** treated as Ok (DoD-13).
/// Documented patterns: "no errors", "no problems found", "complete", "0 errors".
pub fn log_indicates_scanpst_success(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Explicit success phrases (checked before error heuristics so "no errors found" wins).
    if lower.contains("no errors")
        || lower.contains("no problems found")
        || lower.contains("0 errors")
        || lower.contains("zero errors")
    {
        return true;
    }
    // "complete" alone is success only when no hard error markers are present.
    if lower.contains("complete") && !lower.contains("incomplete") {
        return !(lower.contains("is corrupt") || lower.contains("serious error"));
    }
    false
}

/// Run scanpst `-no repair` on a **local temp copy** of the deliverable.
///
/// - Build must be ≥ [`SCANPST_MIN_BUILD`] from a **real** version source
///   (sibling `.version` or `PST_DEDUP_SCANPST_BUILD`) — folder names alone never count.
/// - Ok requires: no `.bak`, recognized success markers, **and** proof that `-no repair`
///   was honored via either (a) help/usage that documents it, or (b) CI stub log line
///   `NO_REPAIR_MODE` ([`log_proves_no_repair_mode`]). Never Ok from bare env/flag markers.
/// - When `-no repair` cannot be proven, status is **Skipped** (D-0080-scanpst-arg residual).
/// - `.bak` next to the copy ⇒ hard error (proves repair ran). Deliverable path is never mutated.
pub fn run_scanpst(scanpst_path: &Path, pst_path: &Path, timeout: Duration) -> ScanpstResult {
    if !scanpst_path.is_file() {
        return ScanpstResult::skipped(format!("scanpst not found: {}", scanpst_path.display()));
    }
    if !pst_path.is_file() {
        return ScanpstResult::skipped(format!("pst not found: {}", pst_path.display()));
    }

    let build = guess_scanpst_build(scanpst_path);
    if let Some(ref b) = build {
        if !version_at_least(b, SCANPST_MIN_BUILD) {
            return ScanpstResult {
                status: ExternalStatus::Skipped,
                reason: Some(format!(
                    "scanpst build {b} < {SCANPST_MIN_BUILD} (pre-CLI / GUI-only risk)"
                )),
                build: Some(b.clone()),
                log_path: None,
                bak_present: false,
                log_summary: None,
                hard_error: false,
            };
        }
    } else {
        // Cannot verify build ⇒ skip rather than guess (rule 2 / D-0080-scanpst-arg).
        return ScanpstResult::skipped(
            "scanpst build unverifiable; skip rather than risk repair path (D-0080-scanpst-arg)",
        );
    }

    // Help-based preverify (not flag/env). Stubs may still prove via NO_REPAIR_MODE log.
    let no_repair_help_ok = verify_scanpst_no_repair(scanpst_path).is_ok();
    // Safe default: if help does not document -no repair, do not invoke the binary
    // (asymmetric failure: unrecognized flags fall into the legacy repairing path).
    // Exception: CI stubs prove acceptance post-run via NO_REPAIR_MODE — those stubs
    // must still be invokable. We only skip pre-run when help fails **and** the binary
    // is not a cmd/bat test stub (extension heuristic for local CI only).
    let looks_like_test_stub = scanpst_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let el = e.to_ascii_lowercase();
            el == "cmd" || el == "bat"
        })
        .unwrap_or(false);
    if !no_repair_help_ok && !looks_like_test_stub {
        return ScanpstResult {
            status: ExternalStatus::Skipped,
            reason: Some(
                "scanpst -no repair support unverifiable via help; skip rather than risk repair (rule 2 / D-0080-scanpst-arg)"
                    .into(),
            ),
            build,
            log_path: None,
            bak_present: false,
            log_summary: None,
            hard_error: false,
        };
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "pst-dedup-scanpst-{}-{}",
        std::process::id(),
        unique_stamp()
    ));
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        return ScanpstResult::skipped(format!("temp dir: {e}"));
    }
    let temp_pst = temp_dir.join("validate.pst");
    if let Err(e) = fs::copy(pst_path, &temp_pst) {
        let _ = fs::remove_dir_all(&temp_dir);
        return ScanpstResult::skipped(format!("copy: {e}"));
    }

    // Mandatory: -no repair (read-only validation). Never omit.
    let no_repair_token = "-no";
    let repair_token = "repair";
    let mut cmd = Command::new(scanpst_path);
    cmd.arg("-file")
        .arg(&temp_pst)
        .arg(no_repair_token)
        .arg(repair_token)
        .arg("-silent")
        .arg("-log")
        .arg("replace")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let run = run_with_timeout(cmd, timeout);

    let bak = temp_dir.join("validate.bak");
    // Also common: same stem as pst.
    let bak_alt = temp_pst.with_extension("bak");
    let bak_present = bak.is_file() || bak_alt.is_file();

    // Prefer log next to copy.
    let log_candidates = [
        temp_pst.with_extension("log"),
        temp_dir.join("validate.log"),
        temp_dir.join("SCANPST.LOG"),
    ];
    let mut log_path: Option<PathBuf> = None;
    let mut log_text = String::new();
    for c in &log_candidates {
        if c.is_file() {
            if let Ok(t) = fs::read_to_string(c) {
                log_text = t;
                log_path = Some(c.clone());
                break;
            }
        }
    }

    let no_repair_proven = no_repair_help_ok || log_proves_no_repair_mode(&log_text);

    let result = if bak_present {
        ScanpstResult {
            status: ExternalStatus::Failed,
            reason: Some(
                "scanpst produced .bak — repair ran; hard QC error (deliverable untouched)".into(),
            ),
            build,
            log_path: log_path.map(|p| p.display().to_string()),
            bak_present: true,
            log_summary: summarize_scanpst_log(&log_text),
            hard_error: true,
        }
    } else {
        match run {
            RunOutcome::Timeout => ScanpstResult {
                status: ExternalStatus::Timeout,
                reason: Some("SCANPST_TIMEOUT".into()),
                build,
                log_path: log_path.map(|p| p.display().to_string()),
                bak_present: false,
                log_summary: summarize_scanpst_log(&log_text),
                hard_error: false,
            },
            RunOutcome::Failed(e) => ScanpstResult {
                status: ExternalStatus::Skipped,
                reason: Some(e),
                build,
                log_path: None,
                bak_present: false,
                log_summary: None,
                hard_error: false,
            },
            RunOutcome::Done { .. } => {
                // Missing/empty log after run ⇒ never Ok (exit code is not trusted).
                if log_text.trim().is_empty() {
                    ScanpstResult {
                        status: ExternalStatus::Skipped,
                        reason: Some(
                            "scanpst finished but log empty or missing; not Ok (exit not trusted)"
                                .into(),
                        ),
                        build,
                        log_path: log_path.map(|p| p.display().to_string()),
                        bak_present: false,
                        log_summary: None,
                        hard_error: false,
                    }
                } else if log_suggests_errors(&log_text) {
                    ScanpstResult {
                        status: ExternalStatus::Failed,
                        reason: Some("scanpst log indicates errors".into()),
                        build,
                        log_path: log_path.map(|p| p.display().to_string()),
                        bak_present: false,
                        log_summary: summarize_scanpst_log(&log_text),
                        hard_error: true,
                    }
                } else if log_indicates_scanpst_success(&log_text) {
                    if no_repair_proven {
                        ScanpstResult {
                            status: ExternalStatus::Ok,
                            reason: None,
                            build,
                            log_path: log_path.map(|p| p.display().to_string()),
                            bak_present: false,
                            log_summary: summarize_scanpst_log(&log_text),
                            hard_error: false,
                        }
                    } else {
                        // Success markers without -no repair proof ⇒ never Ok.
                        ScanpstResult {
                            status: ExternalStatus::Skipped,
                            reason: Some(
                                "scanpst log has success markers but -no repair not proven (help docs or NO_REPAIR_MODE stub line required; D-0080-scanpst-arg)"
                                    .into(),
                            ),
                            build,
                            log_path: log_path.map(|p| p.display().to_string()),
                            bak_present: false,
                            log_summary: summarize_scanpst_log(&log_text),
                            hard_error: false,
                        }
                    }
                } else {
                    // Non-empty but unrecognized — never Ok (DoD-13).
                    ScanpstResult {
                        status: ExternalStatus::Skipped,
                        reason: Some(
                            "scanpst log content unrecognized; not Ok without success markers (no errors / complete / 0 errors)"
                                .into(),
                        ),
                        build,
                        log_path: log_path.map(|p| p.display().to_string()),
                        bak_present: false,
                        log_summary: summarize_scanpst_log(&log_text),
                        hard_error: false,
                    }
                }
            }
        }
    };

    // Quarantine copy dir on bak; always clean temp when possible.
    if bak_present {
        // Leave temp for operator inspection under a quarantine name.
        let q = temp_dir.with_extension("quarantine");
        let _ = fs::rename(&temp_dir, &q);
    } else {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    result
}

/// Auto-discover and run scanpst, or skip with reason.
pub fn run_scanpst_auto(pst_path: &Path, timeout: Duration) -> ScanpstResult {
    match discover_scanpst() {
        Some(path) => run_scanpst(&path, pst_path, timeout),
        None => ScanpstResult::skipped(
            "scanpst not found (Click-to-Run/registry probe); only new Outlook? skip",
        ),
    }
}

/// Monotonic-ish unique stamp for temp dirs (never use Instant::elapsed ≈ 0).
fn unique_stamp() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    nanos.saturating_add(u128::from(n))
}

fn summarize_scanpst_log(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    Some(line.chars().take(200).collect())
}

fn log_suggests_errors(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Success phrases take precedence ("no errors found" contains "errors found").
    if lower.contains("no errors")
        || lower.contains("no problems found")
        || lower.contains("0 errors")
        || lower.contains("zero errors")
    {
        return false;
    }
    // Conservative: explicit error markers only.
    lower.contains("errors found")
        || lower.contains("error found")
        || lower.contains("is corrupt")
        || lower.contains("serious error")
        || lower.contains("errors were found")
}

/// Parse message/folder counts from pffinfo/readpst-like stdout (best-effort).
pub fn parse_reader_counts(stdout: &str) -> (Option<u64>, Option<u64>) {
    let mut messages = None;
    let mut folders = None;
    for line in stdout.lines() {
        let lower = line.to_ascii_lowercase();
        if messages.is_none() {
            if let Some(n) = extract_count_after(&lower, &["number of items", "messages", "items"])
            {
                messages = Some(n);
            }
        }
        if folders.is_none() {
            if let Some(n) = extract_count_after(&lower, &["number of folders", "folders"]) {
                folders = Some(n);
            }
        }
    }
    // Fallback: first bare integer on a line with "message".
    if messages.is_none() {
        for line in stdout.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.contains("message") {
                if let Some(n) = first_integer(line) {
                    messages = Some(n);
                    break;
                }
            }
        }
    }
    (messages, folders)
}

fn extract_count_after(lower_line: &str, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(idx) = lower_line.find(key) {
            let rest = &lower_line[idx + key.len()..];
            if let Some(n) = first_integer(rest) {
                return Some(n);
            }
        }
    }
    None
}

fn first_integer(s: &str) -> Option<u64> {
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else if !num.is_empty() {
            break;
        }
    }
    if num.is_empty() {
        None
    } else {
        num.parse().ok()
    }
}

enum RunOutcome {
    Done {
        status: std::process::ExitStatus,
        stdout: String,
        #[allow(dead_code)]
        stderr: String,
    },
    Timeout,
    Failed(String),
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> RunOutcome {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RunOutcome::Failed(format!("spawn: {e}")),
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = std::io::Read::read_to_string(&mut out, &mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err, &mut stderr);
                }
                return RunOutcome::Done {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return RunOutcome::Timeout;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return RunOutcome::Failed(format!("wait: {e}")),
        }
    }
}

/// Write a tiny stub executable script for tests (Windows `.cmd`).
#[cfg(test)]
pub fn write_stub_cmd(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::File::create(path)?;
    writeln!(f, "@echo off")?;
    write!(f, "{body}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn byob_rejects_url() {
        let r = run_independent_reader(
            Path::new("https://example.com/pffinfo.exe"),
            Path::new("nope.pst"),
            Duration::from_secs(1),
        );
        assert_eq!(r.status, ExternalStatus::Skipped);
        assert!(r.reason.as_deref().unwrap_or("").contains("BYOB"));
    }

    #[test]
    fn missing_reader_skips() {
        let r = run_independent_reader(
            Path::new(r"C:\nonexistent\pffinfo.exe"),
            Path::new(r"C:\nonexistent\x.pst"),
            Duration::from_secs(1),
        );
        assert_eq!(r.status, ExternalStatus::Skipped);
    }

    #[test]
    fn parse_pffinfo_like_counts() {
        let out = "Number of folders\t: 5\nNumber of items\t: 42\n";
        let (m, f) = parse_reader_counts(out);
        assert_eq!(m, Some(42));
        assert_eq!(f, Some(5));
    }

    #[test]
    fn version_gate() {
        assert!(version_at_least("16.0.10325.20082", SCANPST_MIN_BUILD));
        assert!(version_at_least("16.0.15000.0", SCANPST_MIN_BUILD));
        assert!(!version_at_least("15.0.0.0", SCANPST_MIN_BUILD));
        assert!(!version_at_least("16.0.10324.0", SCANPST_MIN_BUILD));
    }

    #[test]
    fn stub_reader_counts() {
        let dir = TempDir::new().expect("tmp");
        let stub = dir.path().join("pffinfo.cmd");
        write_stub_cmd(
            &stub,
            "echo Number of folders : 2\r\necho Number of items : 7\r\nexit /b 0\r\n",
        )
        .expect("stub");
        let pst = dir.path().join("x.pst");
        fs::write(&pst, b"not-a-real-pst-but-file-exists").expect("pst");
        let r = run_independent_reader(&stub, &pst, Duration::from_secs(10));
        assert_eq!(r.status, ExternalStatus::Ok, "{r:?}");
        assert_eq!(r.message_count, Some(7));
        assert_eq!(r.folder_count, Some(2));
    }

    fn pin_scanpst_build(cmd_path: &Path) {
        // Verified build via sibling .version — never invent from Office16 folder alone.
        // Do **not** write `.accepts-no-repair` (removed as Ok path — DoD-13 honesty).
        fs::write(
            cmd_path.with_extension("version"),
            format!("{SCANPST_MIN_BUILD}\n"),
        )
        .expect("version pin");
    }

    /// CI stub contract body: prove `-no repair` via `NO_REPAIR_MODE` log line + success marker.
    /// `%2` is the pst path after `-file` (production argv shape).
    fn stub_scanpst_ok_body() -> &'static str {
        "if not \"%~2\"==\"\" (\r\n\
         echo NO_REPAIR_MODE> \"%~dpn2.log\"\r\n\
         echo No errors found>> \"%~dpn2.log\"\r\n\
         )\r\nexit /b 0\r\n"
    }

    #[test]
    fn stub_scanpst_ok_no_bak() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("Office16");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        write_stub_cmd(&named_cmd, stub_scanpst_ok_body()).expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("deliverable.pst");
        fs::write(&pst, b"fake-pst-bytes").expect("pst");

        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        // May be Ok or Failed depending on log write; must not hard_error without bak.
        assert!(!r.bak_present, "{r:?}");
        assert!(!r.hard_error || r.status == ExternalStatus::Failed, "{r:?}");
        // Stub contract: NO_REPAIR_MODE + success marker ⇒ Ok.
        assert_eq!(
            r.status,
            ExternalStatus::Ok,
            "stub with NO_REPAIR_MODE + success must be Ok: {r:?}"
        );
    }

    #[test]
    fn stub_success_without_no_repair_proof_is_not_ok() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("tools");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        // Success markers only — no NO_REPAIR_MODE, help has no "no repair".
        write_stub_cmd(
            &named_cmd,
            "if not \"%~2\"==\"\" (echo No errors found> \"%~dpn2.log\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        assert_ne!(
            r.status,
            ExternalStatus::Ok,
            "success log without -no repair proof must not be Ok: {r:?}"
        );
        assert_eq!(r.status, ExternalStatus::Skipped, "{r:?}");
    }

    #[test]
    fn accepts_no_repair_flag_file_alone_is_not_ok() {
        // Regression: bare .accepts-no-repair must never green-wash Ok.
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("tools");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        write_stub_cmd(
            &named_cmd,
            "if not \"%~2\"==\"\" (echo No errors found> \"%~dpn2.log\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        pin_scanpst_build(&named_cmd);
        fs::write(named_cmd.with_extension("accepts-no-repair"), b"1\n").expect("flag");
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        assert_ne!(
            r.status,
            ExternalStatus::Ok,
            ".accepts-no-repair alone must not yield Ok: {r:?}"
        );
    }

    #[test]
    fn stub_scanpst_bak_is_hard_error() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("Office16");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        // Production always passes: -file <pst> -no repair ...
        // Write .bak next to %2 (the pst path after -file) — most reliable on cmd.
        write_stub_cmd(
            &named_cmd,
            "if not \"%~2\"==\"\" (echo NO_REPAIR_MODE> \"%~dpn2.log\" & echo repaired> \"%~dpn2.bak\")\r\nif not \"%~3\"==\"\" (echo repaired> \"%~dpn3.bak\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("deliverable.pst");
        fs::write(&pst, b"fake").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        assert!(
            r.bak_present || r.hard_error,
            "expected bak hard error, got {r:?}"
        );
        if r.bak_present {
            assert!(r.hard_error);
            assert_eq!(r.status, ExternalStatus::Failed);
        }
    }

    #[test]
    fn scanpst_timeout() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("Office16");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        write_stub_cmd(&named_cmd, "ping -n 30 127.0.0.1 >nul\r\nexit /b 0\r\n").expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_millis(500));
        assert!(
            matches!(r.status, ExternalStatus::Timeout)
                || r.reason.as_deref() == Some("SCANPST_TIMEOUT"),
            "{r:?}"
        );
    }

    #[test]
    fn unknown_build_skips_not_ok() {
        let dir = TempDir::new().expect("tmp");
        // No .version pin and no env — Office16 folder alone must skip.
        let office = dir.path().join("Office16");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        write_stub_cmd(&named_cmd, "exit /b 0\r\n").expect("stub");
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        // Ensure env does not leak a pin from the environment.
        let r = {
            // Temporarily clear env if set.
            let prev = std::env::var("PST_DEDUP_SCANPST_BUILD").ok();
            std::env::remove_var("PST_DEDUP_SCANPST_BUILD");
            let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(5));
            if let Some(v) = prev {
                std::env::set_var("PST_DEDUP_SCANPST_BUILD", v);
            }
            r
        };
        assert_eq!(r.status, ExternalStatus::Skipped, "{r:?}");
        assert_ne!(r.status, ExternalStatus::Ok);
        assert!(
            r.reason
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("unverif")
                || r.reason
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains("skip"),
            "{r:?}"
        );
        assert!(guess_scanpst_build(&named_cmd).is_none());
    }

    #[test]
    fn empty_log_not_ok() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("tools");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        // Finish successfully without writing any log.
        write_stub_cmd(&named_cmd, "exit /b 0\r\n").expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        assert_ne!(
            r.status,
            ExternalStatus::Ok,
            "empty log must not be Ok: {r:?}"
        );
        assert!(
            matches!(r.status, ExternalStatus::Skipped | ExternalStatus::Failed),
            "{r:?}"
        );
        assert!(
            r.reason
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("log"),
            "{r:?}"
        );
    }

    #[test]
    fn unrecognized_log_content_not_ok() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("tools");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        // Write a non-empty log without recognized success/error markers (%2 = pst after -file).
        write_stub_cmd(
            &named_cmd,
            "if not \"%~2\"==\"\" (echo lorem ipsum random banner> \"%~dpn2.log\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        assert_ne!(
            r.status,
            ExternalStatus::Ok,
            "unrecognized log must not be Ok: {r:?}"
        );
        let reason = r.reason.as_deref().unwrap_or("").to_ascii_lowercase();
        assert!(
            reason.contains("unrecog") || reason.contains("success") || reason.contains("log"),
            "{r:?}"
        );
    }

    #[test]
    fn missing_no_repair_verification_skips() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("tools");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        // Version pin only — help has no "no repair", run writes no NO_REPAIR_MODE.
        write_stub_cmd(
            &named_cmd,
            "echo usage: SCANPST -file path\r\nexit /b 0\r\n",
        )
        .expect("stub");
        fs::write(
            named_cmd.with_extension("version"),
            format!("{SCANPST_MIN_BUILD}\n"),
        )
        .expect("version");
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(10));
        // Stub may run (cmd) but must not be Ok without proof.
        assert_ne!(r.status, ExternalStatus::Ok, "{r:?}");
        assert!(
            matches!(r.status, ExternalStatus::Skipped | ExternalStatus::Failed),
            "{r:?}"
        );
        let reason = r.reason.as_deref().unwrap_or("").to_ascii_lowercase();
        assert!(
            reason.contains("no repair")
                || reason.contains("unverif")
                || reason.contains("log")
                || reason.contains("success"),
            "{r:?}"
        );
    }

    #[test]
    fn env_no_repair_marker_alone_is_not_ok() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("tools");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        write_stub_cmd(
            &named_cmd,
            "if not \"%~2\"==\"\" (echo No errors found> \"%~dpn2.log\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        pin_scanpst_build(&named_cmd);
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let prev = std::env::var("PST_DEDUP_SCANPST_NO_REPAIR_OK").ok();
        std::env::set_var("PST_DEDUP_SCANPST_NO_REPAIR_OK", "1");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        if let Some(v) = prev {
            std::env::set_var("PST_DEDUP_SCANPST_NO_REPAIR_OK", v);
        } else {
            std::env::remove_var("PST_DEDUP_SCANPST_NO_REPAIR_OK");
        }
        assert_ne!(
            r.status,
            ExternalStatus::Ok,
            "env PST_DEDUP_SCANPST_NO_REPAIR_OK alone must not yield Ok: {r:?}"
        );
    }

    #[test]
    fn relative_reader_path_skipped() {
        let r = run_independent_reader(
            Path::new("pffinfo.cmd"),
            Path::new("nope.pst"),
            Duration::from_secs(1),
        );
        assert_eq!(r.status, ExternalStatus::Skipped);
        assert!(
            r.reason
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("absolute"),
            "{r:?}"
        );
    }

    #[test]
    fn exit_zero_without_counts_skipped_not_ok() {
        let dir = TempDir::new().expect("tmp");
        let stub = dir.path().join("pffinfo.cmd");
        write_stub_cmd(&stub, "echo hello world\r\nexit /b 0\r\n").expect("stub");
        let pst = dir.path().join("x.pst");
        fs::write(&pst, b"not-real").expect("pst");
        let r = run_independent_reader(&stub, &pst, Duration::from_secs(10));
        assert_ne!(r.status, ExternalStatus::Ok, "{r:?}");
        assert_eq!(r.status, ExternalStatus::Skipped, "{r:?}");
        assert!(r.message_count.is_none());
    }

    #[test]
    fn log_success_markers() {
        assert!(log_indicates_scanpst_success("No errors found"));
        assert!(log_indicates_scanpst_success("No problems found."));
        assert!(log_indicates_scanpst_success("Scan complete. 0 errors."));
        assert!(!log_indicates_scanpst_success("lorem ipsum banner"));
        assert!(!log_indicates_scanpst_success(""));
    }
}
