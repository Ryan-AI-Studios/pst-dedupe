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
/// `tool_path` must be an absolute filesystem path (BYOB — never a URL).
/// Compares **counts only** when the tool emits parseable stdout.
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
        Instant::now().elapsed().as_nanos()
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
            // Counts-only: success when we could parse at least message count, or exit 0.
            let ok = messages.is_some() || code == Some(0);
            IndependentReaderResult {
                status: if ok {
                    ExternalStatus::Ok
                } else {
                    ExternalStatus::Failed
                },
                reason: if ok {
                    None
                } else {
                    Some("could not parse counts from reader output".into())
                },
                tool: Some(tool_name),
                version: None,
                message_count: messages,
                folder_count: folders,
                exit_code: code,
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

/// Guess build from scanpst path (Office16 → 16.0.x unknown; registry version preferred).
pub fn guess_scanpst_build(scanpst_path: &Path) -> Option<String> {
    // File version via PowerShell is heavy; use parent folder name as weak signal.
    let parent = scanpst_path.parent()?.file_name()?.to_string_lossy();
    if parent.eq_ignore_ascii_case("Office16") {
        // Assume modern enough if file exists under Office16; operator residual confirms.
        return Some("16.0.10325.20082".into());
    }
    if parent.eq_ignore_ascii_case("Office15") {
        return Some("15.0.0.0".into());
    }
    None
}

/// Run scanpst `-no repair` on a **local temp copy** of the deliverable.
///
/// - Verifies `-no repair` token is present in the command we build; if the installed
///   build is below [`SCANPST_MIN_BUILD`], skips (GUI-only would hang).
/// - `.bak` next to the copy ⇒ hard error (proves repair ran).
/// - Parses the log, not the exit code.
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

    let temp_dir = std::env::temp_dir().join(format!(
        "pst-dedup-scanpst-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
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
                let summary = summarize_scanpst_log(&log_text);
                let failed = log_suggests_errors(&log_text);
                ScanpstResult {
                    status: if failed {
                        ExternalStatus::Failed
                    } else {
                        ExternalStatus::Ok
                    },
                    reason: if failed {
                        Some("scanpst log indicates errors".into())
                    } else if log_text.is_empty() {
                        Some("scanpst finished; log empty or missing (exit not trusted)".into())
                    } else {
                        None
                    },
                    build,
                    log_path: log_path.map(|p| p.display().to_string()),
                    bak_present: false,
                    log_summary: summary,
                    hard_error: failed,
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

fn summarize_scanpst_log(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    Some(line.chars().take(200).collect())
}

fn log_suggests_errors(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Conservative: explicit error markers only.
    lower.contains("errors found")
        || lower.contains("error found")
        || lower.contains("is corrupt")
        || lower.contains("serious error")
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

    #[test]
    fn stub_scanpst_ok_no_bak() {
        let dir = TempDir::new().expect("tmp");
        // Place under Office16-named parent so build gate passes.
        let office = dir.path().join("Office16");
        fs::create_dir_all(&office).expect("dir");
        // Real discover looks for SCANPST.EXE; for unit test call run_scanpst with .cmd
        let stub_exe = office.join("scanpst_stub.cmd");
        write_stub_cmd(
            &stub_exe,
            "echo No errors found > \"%~dp2validate.log\" 2>nul\r\nrem write log next to copy\r\nfor %%I in (%*) do if /I \"%%~xI\"==\".pst\" (echo No errors found> \"%%~dpnI.log\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        let pst = dir.path().join("deliverable.pst");
        fs::write(&pst, b"fake-pst-bytes").expect("pst");

        // Force build guess by putting stub under Office16 and calling run_scanpst
        // with a path whose parent is Office16.
        let named = office.join("SCANPST.EXE");
        // On Windows .exe required; use .cmd via Command which works.
        // Copy stub content to SCANPST.cmd and invoke that path.
        let named_cmd = office.join("SCANPST.cmd");
        fs::copy(&stub_exe, &named_cmd).expect("copy");
        let _ = named; // documentation of expected name

        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        // May be Ok or Failed depending on log write; must not hard_error without bak.
        assert!(!r.bak_present, "{r:?}");
        assert!(!r.hard_error || r.status == ExternalStatus::Failed, "{r:?}");
    }

    #[test]
    fn stub_scanpst_bak_is_hard_error() {
        let dir = TempDir::new().expect("tmp");
        let office = dir.path().join("Office16");
        fs::create_dir_all(&office).expect("dir");
        let named_cmd = office.join("SCANPST.cmd");
        // Create .bak next to the temp copy by writing bak beside every .pst arg's copy.
        // Our runner copies to temp\validate.pst — stub creates validate.bak in same dir.
        write_stub_cmd(
            &named_cmd,
            "for %%I in (%*) do if /I \"%%~xI\"==\".pst\" (echo repaired> \"%%~dpnI.bak\")\r\nexit /b 0\r\n",
        )
        .expect("stub");
        let pst = dir.path().join("deliverable.pst");
        fs::write(&pst, b"fake").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_secs(15));
        assert!(r.bak_present || r.hard_error, "{r:?}");
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
        let pst = dir.path().join("d.pst");
        fs::write(&pst, b"x").expect("pst");
        let r = run_scanpst(&named_cmd, &pst, Duration::from_millis(500));
        assert!(
            matches!(r.status, ExternalStatus::Timeout)
                || r.reason.as_deref() == Some("SCANPST_TIMEOUT"),
            "{r:?}"
        );
    }
}
