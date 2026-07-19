//! Matter progress / metrics report export (track **0039**).
//!
//! Serializes a [`CaseOverview`] (from [`load_case_overview`] / [`load_case_overview_on`])
//! plus job history into a multi-file CSV pack. Metrics are **never** re-rolled with
//! ad-hoc GROUP BY SQL — the overview snapshot is the single source of truth.
//!
//! ## Pack layout (`matter_report_v1`)
//!
//! Written under an operator-chosen or default stamped directory
//! (`exports/reports/matter_report_YYYYMMDD_HHMMSS/`):
//!
//! | File | Layout |
//! |---|---|
//! | `summary.csv` | Two-column `metric,value` KPIs + matter identity + dual datetimes |
//! | `by_file_category.csv` | `label,count` (+ `(other),N` remainder) |
//! | `by_custodian.csv` | `label,count` (+ remainder) |
//! | `by_status.csv` | `label,count` |
//! | `errors_by_code.csv` | `code,count` (+ remainder) |
//! | `jobs.csv` | Job history (all jobs, newest first) with scrubbed errors |
//! | `README.txt` | Privacy + datetime notes |
//!
//! ### Empty tables
//!
//! Every rollup CSV always has a header row. When there are no data rows, exactly one
//! sentinel row is written: label/code = `(none)`, count = `0`. Never 0-byte files.
//!
//! ### Dual datetimes
//!
//! Summary always includes `generated_at` (RFC3339 UTC) and `generated_at_excel`
//! (`YYYY-MM-DD HH:MM:SS UTC`). Job times export both Excel-friendly and RFC3339 twins.
//!
//! ### Privacy
//!
//! Counts, labels, and job metadata only — **no** subjects, bodies, or privilege
//! descriptions. Job `error_summary` is scrubbed via [`scrub_error_summary`] before
//! export (paths / filenames / `file://` URIs redacted, then an allowlist keeps only
//! stable error codes and short generic phrases).
//!
//! ### PDF (D-0039-01)
//!
//! PDF summary is **deferred**. `include_pdf` is accepted but ignored; `pdf_written`
//! is always `false`. A later implementation must embed a permissive TTF and never
//! depend on host fonts.
//!
//! ### Overwrite policy
//!
//! Fail closed if `output_dir` already exists (no silent clobber). Prefer a fresh
//! stamp from [`default_matter_report_dir`].
//!
//! ### Atomic pack write
//!
//! Files are written under a sibling `{output_dir}.tmp` directory, then the pack is
//! **renamed into place**, then `report.export.complete` is audited. On failure before
//! rename, the temp directory is removed best-effort so a retry is not blocked by
//! half-written debris. If rename succeeds but audit fails, the pack remains at the
//! final path and the error is returned (honest: the published pack exists).

use std::fs;
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};

use crate::audit::{self, AuditEventInput};
use crate::error::{Error, Result};
use crate::matter::{now_rfc3339, Matter, EXPORTS_DIR};
use crate::overview::{load_case_overview_on, CaseOverview, LabelCount, OverviewOptions};
use crate::privilege::csv_escape_field;
use crate::schema::SCHEMA_VERSION;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Report pack format version written to `summary.csv` and audit detail.
pub const MATTER_REPORT_FORMAT_VERSION: &str = "matter_report_v1";

const SUMMARY_FILE: &str = "summary.csv";
const BY_CATEGORY_FILE: &str = "by_file_category.csv";
const BY_CUSTODIAN_FILE: &str = "by_custodian.csv";
const BY_STATUS_FILE: &str = "by_status.csv";
const ERRORS_FILE: &str = "errors_by_code.csv";
const JOBS_FILE: &str = "jobs.csv";
const README_FILE: &str = "README.txt";

const LABEL_NONE: &str = "(none)";
const LABEL_UNCATEGORIZED: &str = "(uncategorized)";
const LABEL_OTHER: &str = "(other)";
const REDACTED: &str = "(redacted)";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parameters for [`Matter::export_matter_report`] / [`export_matter_report`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatterReportParams {
    /// Directory that will receive the pack. Must **not** already exist.
    pub output_dir: Utf8PathBuf,
    /// Top-N options for rollup CSVs (same as Overview).
    pub overview_opts: OverviewOptions,
    /// PDF summary request. **Ignored in P0** (D-0039-01 deferred); always treated as false.
    pub include_pdf: bool,
    /// When true (default), export all jobs. When false, only recent strip from overview.
    pub export_all_jobs: bool,
}

impl Default for MatterReportParams {
    fn default() -> Self {
        Self {
            output_dir: Utf8PathBuf::new(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        }
    }
}

/// Result of a successful matter report export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatterReportResult {
    /// RFC3339 generation timestamp for the pack.
    pub generated_at: String,
    /// Absolute or matter-relative output directory written.
    pub output_dir: Utf8PathBuf,
    /// Relative file names written into `output_dir`.
    pub files_written: Vec<String>,
    /// Snapshot used for metrics (from `load_case_overview*`).
    pub overview: CaseOverview,
    /// Always `false` while PDF is deferred (D-0039-01).
    pub pdf_written: bool,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Default stamped report directory under `matter_root/exports/reports/`.
///
/// Stamp format: `matter_report_YYYYMMDD_HHMMSS` (UTC).
pub fn default_matter_report_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
    let stamp = Utc::now().format("%Y%m%d_%H%M%S");
    matter_root
        .join(EXPORTS_DIR)
        .join("reports")
        .join(format!("matter_report_{stamp}"))
}

// ---------------------------------------------------------------------------
// Datetime helpers
// ---------------------------------------------------------------------------

/// Convert an RFC3339 timestamp to Excel-friendly `YYYY-MM-DD HH:MM:SS UTC`.
///
/// Returns empty string when `rfc` is empty or unparseable.
pub fn rfc3339_to_excel_utc(rfc: &str) -> String {
    let s = rfc.trim();
    if s.is_empty() {
        return String::new();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt
            .with_timezone(&Utc)
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
    }
    // `now_rfc3339` uses chrono's to_rfc3339; also accept bare UTC forms without offset parse.
    if let Ok(dt) = s.parse::<DateTime<Utc>>() {
        return dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    }
    String::new()
}

/// Current UTC as Excel-friendly companion for a known RFC3339 string.
fn excel_now_from_rfc(rfc: &str) -> String {
    let excel = rfc3339_to_excel_utc(rfc);
    if excel.is_empty() {
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
    } else {
        excel
    }
}

// ---------------------------------------------------------------------------
// Path / error scrubber
// ---------------------------------------------------------------------------

/// Scrub client paths, filenames, and free-text from a job `error_summary`.
///
/// Pipeline:
/// 1. Redact `file://` URIs, Windows/UNC/Unix absolute paths, and path-like tokens.
/// 2. Allowlist remaining tokens against a **finite** registry of stable error codes
///    and short generic words (case-insensitive). Syntactic snake_case is **not**
///    enough — client phrases like `acme_merger_strategy` must not pass.
/// 3. If nothing remains → `(redacted)` (unless the original input was empty/whitespace
///    → empty).
pub fn scrub_error_summary(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut out = redact_file_uris(trimmed);
    out = redact_windows_and_unc_paths(&out);
    out = redact_unix_absolute_paths(&out);
    out = redact_pathish_tokens(&out);

    // Collapse leftover whitespace; drop punctuation-only islands.
    let cleaned: String = out
        .split_whitespace()
        .filter(|t| !t.is_empty() && *t != "-" && *t != "–")
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = cleaned
        .trim_matches(|c: char| c == ':' || c == ',' || c == ';' || c == '"' || c == '\'')
        .trim()
        .to_string();

    // Privacy allowlist: drop subject-like / free text that survived path redaction.
    let allowed = filter_error_summary_allowlist(&cleaned);
    if allowed.is_empty() {
        REDACTED.to_string()
    } else {
        allowed
    }
}

/// Keep only registered stable error-code tokens and short generic error words.
fn filter_error_summary_allowlist(s: &str) -> String {
    s.split_whitespace()
        .filter(|tok| token_is_allowed_error_summary(tok))
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_is_allowed_error_summary(tok: &str) -> bool {
    let bare = tok.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '.' | '!' | '?'
        )
    });
    if bare.is_empty() || bare == REDACTED {
        return false;
    }
    if is_registered_error_code(bare) {
        return true;
    }
    is_generic_error_word(bare)
}

/// Finite registry of stable machine codes known to Desk jobs / item_errors.
/// Deliberately **not** a syntactic snake_case predicate (client free text can look
/// like codes). Extend when new stable codes ship.
fn is_registered_error_code(tok: &str) -> bool {
    const STABLE_CODES: &[&str] = &[
        "parse_failed",
        "ocr_pdf_renderer_missing",
        "engine_not_found",
        "unsupported_7z",
        "pdf_needs_ocr",
        "low_text",
        "empty_text",
        "mid_logical_conflicts",
        "job_not_found",
        "worker_gone",
        "item_error",
        "fts_query_invalid",
        "privilege_log_incomplete",
    ];
    STABLE_CODES.iter().any(|c| tok.eq_ignore_ascii_case(c))
}

fn is_generic_error_word(tok: &str) -> bool {
    const GENERIC: &[&str] = &[
        "failed",
        "error",
        "errors",
        "timeout",
        "missing",
        "not",
        "found",
        "refused",
        "cancelled",
        "canceled",
        "aborted",
        "unknown",
        "unsupported",
        "invalid",
        "empty",
        "none",
        "ok",
        "skipped",
        "partial",
    ];
    let lower = tok.to_ascii_lowercase();
    GENERIC.contains(&lower.as_str())
}

fn redact_file_uris(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if looks_like_file_uri_at(s, i) {
            // Skip scheme + remainder until whitespace
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push_str(REDACTED);
        } else {
            out.push(s[i..].chars().next().unwrap_or('?'));
            i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    }
    out
}

fn looks_like_file_uri_at(s: &str, i: usize) -> bool {
    s[i..].len() >= 7
        && s[i..]
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("file://"))
}

fn redact_windows_and_unc_paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Drive letter path: C:\...
        if i + 2 < chars.len()
            && chars[i].is_ascii_alphabetic()
            && chars[i + 1] == ':'
            && (chars[i + 2] == '\\' || chars[i + 2] == '/')
        {
            i = skip_path_chars(&chars, i);
            out.push_str(REDACTED);
            continue;
        }
        // UNC: \\server\share\...
        if i + 1 < chars.len() && chars[i] == '\\' && chars[i + 1] == '\\' {
            i = skip_path_chars(&chars, i);
            out.push_str(REDACTED);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn redact_unix_absolute_paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/'
            && (i == 0 || chars[i - 1].is_whitespace() || is_path_boundary(chars[i - 1]))
            && i + 1 < chars.len()
            && !chars[i + 1].is_whitespace()
            && looks_like_unix_path_start(&chars, i)
        {
            i = skip_path_chars(&chars, i);
            out.push_str(REDACTED);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn looks_like_unix_path_start(chars: &[char], i: usize) -> bool {
    // Any absolute token starting with `/` after a boundary is treated as a path,
    // including root-level single segments (`/client_secret`) with no second `/`
    // and no file extension. Caller already requires a non-whitespace next char.
    let _ = chars;
    let _ = i;
    true
}

fn is_path_boundary(c: char) -> bool {
    matches!(c, '"' | '\'' | '(' | '[' | '{' | '=' | ':' | ',' | ';')
}

fn is_path_end_punct(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | ',' | ';' | '"' | '\'')
}

/// Once an absolute path starts (drive letter / UNC / Unix), consume aggressively for
/// privacy: through whitespace (paths with spaces) until end of string, `;`, double
/// space, sentence-ending `. `, or clear path-end punctuation.
fn skip_path_chars(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        // Hard stop: semicolon (list/detail separator in short job errors).
        if c == ';' {
            break;
        }
        // Double whitespace → end of path blob.
        if c.is_whitespace() && i + 1 < chars.len() && chars[i + 1].is_whitespace() {
            break;
        }
        // Sentence boundary: ". " (period + space). Extension dots never have a space after.
        if c == '.' && i + 1 < chars.len() && chars[i + 1].is_whitespace() {
            break;
        }
        // Closing punctuation that commonly terminates a path in prose (keep consuming
        // spaces so `C:\client data\file.pdf` is fully redacted).
        if is_path_end_punct(c) && !c.is_whitespace() {
            // Allow path-internal characters only; `)` etc. end the path.
            if matches!(c, ')' | ']' | '}' | '"' | '\'') {
                break;
            }
            if matches!(c, ',') {
                break;
            }
        }
        i += 1;
    }
    i
}

/// Redact remaining tokens that look like bare filenames with extensions
/// (e.g. leftover `super_secret_merger.pdf` after path stripping), or multi-segment
/// relative path-like tokens (`client_data\acme_deal\memo.eml`).
fn redact_pathish_tokens(s: &str) -> String {
    s.split_whitespace()
        .map(|tok| {
            let bare = tok.trim_matches(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':'
                )
            });
            if is_filename_like(bare) || is_relative_pathish(bare) {
                REDACTED
            } else {
                tok
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Multi-segment relative path tokens with `\` or `/` (even without a known extension).
fn is_relative_pathish(tok: &str) -> bool {
    if tok.is_empty() || tok == REDACTED {
        return false;
    }
    if !(tok.contains('\\') || tok.contains('/')) {
        return false;
    }
    // Skip pure Windows drive roots already handled; still treat multi-segment.
    let seps = tok.chars().filter(|c| *c == '\\' || *c == '/').count();
    if seps == 0 {
        return false;
    }
    let segments = tok.split(['\\', '/']).filter(|s| !s.is_empty()).count();
    segments >= 2
}

fn is_filename_like(tok: &str) -> bool {
    if tok.is_empty() || tok == REDACTED {
        return false;
    }
    // Strip multi-segment path down to final component for extension checks.
    let base = tok.rsplit(['\\', '/']).next().unwrap_or(tok);
    // Must contain a dot extension of 1–10 alnum chars and a basename with a letter.
    let Some((name, ext)) = base.rsplit_once('.') else {
        return false;
    };
    if name.is_empty() || ext.is_empty() || ext.len() > 10 {
        return false;
    }
    if !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    // Avoid redacting version-like tokens (e.g. v1.2) — require letter in basename
    // and prefer longer basenames or known document-ish extensions.
    let has_letter = name.chars().any(|c| c.is_ascii_alphabetic());
    if !has_letter {
        return false;
    }
    let ext_l = ext.to_ascii_lowercase();
    const DOC_EXTS: &[&str] = &[
        "pdf", "eml", "msg", "pst", "ost", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt",
        "csv", "zip", "7z", "rar", "tiff", "tif", "png", "jpg", "jpeg", "gif", "html", "htm",
        "mht", "rtf", "ics", "mbox", "nsf", "db", "sqlite", "json", "xml",
    ];
    DOC_EXTS.contains(&ext_l.as_str())
        || name.contains('_')
        || name.contains('-')
        || name.len() > 8
        || tok.contains('\\')
        || tok.contains('/')
}

// ---------------------------------------------------------------------------
// Label display (match Overview UI)
// ---------------------------------------------------------------------------

fn category_label(raw: &str) -> &str {
    if raw.is_empty() {
        LABEL_UNCATEGORIZED
    } else {
        raw
    }
}

fn custodian_label(raw: &str) -> &str {
    if raw.is_empty() {
        LABEL_NONE
    } else {
        raw
    }
}

fn status_or_code_label(raw: &str) -> &str {
    if raw.is_empty() {
        LABEL_NONE
    } else {
        raw
    }
}

// ---------------------------------------------------------------------------
// Free entry
// ---------------------------------------------------------------------------

/// Open a matter and export a progress/metrics report pack.
///
/// Opens via [`Matter::open_for_read`] (no `workspace/temp/` wipe) so desk export can
/// run safely while extract jobs materialize CAS blobs under temp. Audit append still
/// uses the normal SQLite connection; only the cleanup_temp flag differs from
/// [`Matter::open`].
///
/// Uses [`load_case_overview_on`] on the opened handle (same metrics as Overview).
pub fn export_matter_report(
    matter_root: &Utf8Path,
    params: MatterReportParams,
) -> Result<MatterReportResult> {
    let matter = Matter::open_for_read(matter_root)?;
    matter.export_matter_report(params)
}

// ---------------------------------------------------------------------------
// Matter impl
// ---------------------------------------------------------------------------

impl Matter {
    /// Build a matter progress/metrics CSV pack from [`CaseOverview`] + jobs.
    ///
    /// # Errors
    ///
    /// - Target `output_dir` already exists (fail closed; no silent overwrite).
    /// - I/O or SQLite failures while loading overview / writing files.
    ///
    /// On failure after the matter is open, attempts `report.export.fail` audit.
    pub fn export_matter_report(&self, params: MatterReportParams) -> Result<MatterReportResult> {
        // PDF deferred (D-0039-01): `include_pdf` is accepted but ignored.
        let _requested_pdf = params.include_pdf;

        match self.export_matter_report_inner(params) {
            Ok(r) => Ok(r),
            Err(e) => {
                let msg = e.to_string();
                let _ = self.audit_report_export_fail(&msg);
                Err(e)
            }
        }
    }

    fn export_matter_report_inner(&self, params: MatterReportParams) -> Result<MatterReportResult> {
        let output_dir = params.output_dir.clone();
        if output_dir.as_str().is_empty() {
            return Err(Error::Other(
                "matter report output_dir must not be empty".into(),
            ));
        }
        if output_dir.exists() {
            return Err(Error::Other(format!(
                "matter report output directory already exists (refusing overwrite): {output_dir}"
            )));
        }
        if let Some(parent) = output_dir.parent() {
            if !parent.as_str().is_empty() {
                fs::create_dir_all(parent.as_std_path())?;
            }
        }

        // Sibling temp pack dir: write fully → rename to final → audit complete.
        // Audit only after the published path exists so complete detail is truthful.
        let temp_dir = Utf8PathBuf::from(format!("{}.tmp", output_dir.as_str()));
        if temp_dir.exists() {
            return Err(Error::Other(format!(
                "matter report temp directory already exists (refusing): {temp_dir}"
            )));
        }

        match self.export_matter_report_write_temp(&temp_dir, &output_dir, &params) {
            Ok(result) => {
                if output_dir.exists() {
                    let _ = fs::remove_dir_all(temp_dir.as_std_path());
                    return Err(Error::Other(format!(
                        "matter report output directory already exists (refusing overwrite): {output_dir}"
                    )));
                }
                match fs::rename(temp_dir.as_std_path(), output_dir.as_std_path()) {
                    Ok(()) => {
                        // Publish succeeded — record complete against the final path.
                        self.audit_report_export_complete(&result)?;
                        Ok(result)
                    }
                    Err(e) => {
                        // Leave temp so the pack is not lost; operator can recover/rename.
                        Err(Error::Other(format!(
                            "matter report rename failed ({e}); pack left at {temp_dir} \
                             (intended {output_dir})"
                        )))
                    }
                }
            }
            Err(e) => {
                let _ = fs::remove_dir_all(temp_dir.as_std_path());
                Err(e)
            }
        }
    }

    /// Write pack files under `temp_dir`. Result path is the intended final `output_dir`.
    /// Caller renames then audits complete.
    fn export_matter_report_write_temp(
        &self,
        temp_dir: &Utf8Path,
        output_dir: &Utf8Path,
        params: &MatterReportParams,
    ) -> Result<MatterReportResult> {
        // create_dir fails if the path already exists (race-safe fail closed).
        fs::create_dir(temp_dir.as_std_path()).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Other(format!(
                    "matter report temp directory already exists (refusing): {temp_dir}"
                ))
            } else {
                Error::Io(e)
            }
        })?;

        let generated_at = now_rfc3339();
        let generated_at_excel = excel_now_from_rfc(&generated_at);

        let overview = load_case_overview_on(self, &params.overview_opts)?;
        let info = self.info()?;

        let mut files_written: Vec<String> = Vec::new();

        // summary.csv
        write_text_file(
            &temp_dir.join(SUMMARY_FILE),
            &build_summary_csv(
                &info.id,
                &info.name,
                &generated_at,
                &generated_at_excel,
                &overview,
            ),
        )?;
        files_written.push(SUMMARY_FILE.into());

        // Rollups
        write_text_file(
            &temp_dir.join(BY_CATEGORY_FILE),
            &build_label_count_csv(
                "label",
                &overview.by_file_category,
                category_label,
                overview.other_categories_count,
            ),
        )?;
        files_written.push(BY_CATEGORY_FILE.into());

        write_text_file(
            &temp_dir.join(BY_CUSTODIAN_FILE),
            &build_label_count_csv(
                "label",
                &overview.by_custodian,
                custodian_label,
                overview.other_custodians_count,
            ),
        )?;
        files_written.push(BY_CUSTODIAN_FILE.into());

        write_text_file(
            &temp_dir.join(BY_STATUS_FILE),
            &build_label_count_csv("label", &overview.by_status, status_or_code_label, 0),
        )?;
        files_written.push(BY_STATUS_FILE.into());

        write_text_file(
            &temp_dir.join(ERRORS_FILE),
            &build_label_count_csv(
                "code",
                &overview.errors.by_code,
                status_or_code_label,
                overview.errors.other_codes_count,
            ),
        )?;
        files_written.push(ERRORS_FILE.into());

        // jobs.csv
        let jobs_csv = if params.export_all_jobs {
            build_jobs_csv_all(self)?
        } else {
            build_jobs_csv_from_overview(&overview)
        };
        write_text_file(&temp_dir.join(JOBS_FILE), &jobs_csv)?;
        files_written.push(JOBS_FILE.into());

        // README.txt (nice-to-have)
        write_text_file(&temp_dir.join(README_FILE), &build_readme())?;
        files_written.push(README_FILE.into());

        // PDF intentionally not written (D-0039-01 deferred).
        // Result path is the intended final directory (audit + return value after rename).
        Ok(MatterReportResult {
            generated_at: generated_at.clone(),
            output_dir: output_dir.to_path_buf(),
            files_written,
            overview,
            pdf_written: false,
        })
    }

    fn audit_report_export_complete(&self, result: &MatterReportResult) -> Result<()> {
        let now = now_rfc3339();
        let params_json = serde_json::json!({
            "path": result.output_dir.as_str(),
            "files": result.files_written,
            "items_total": result.overview.totals.items_total,
            "generated_at": result.generated_at,
            "format_version": MATTER_REPORT_FORMAT_VERSION,
            "pdf_written": result.pdf_written,
        })
        .to_string();
        audit::append_event(
            self.connection(),
            &AuditEventInput {
                actor: "desk".into(),
                action: "report.export.complete".into(),
                entity: format!("matter:{}", self.id()),
                params_json,
                tool_version: env!("CARGO_PKG_VERSION").into(),
            },
            &now,
        )?;
        Ok(())
    }

    fn audit_report_export_fail(&self, error_message: &str) -> Result<()> {
        let now = now_rfc3339();
        // Truncate long I/O messages at a UTF-8 char boundary; never include subjects.
        let msg = truncate_utf8(error_message, 500);
        audit::append_event(
            self.connection(),
            &AuditEventInput {
                actor: "desk".into(),
                action: "report.export.fail".into(),
                entity: format!("matter:{}", self.id()),
                params_json: serde_json::json!({
                    "error": msg,
                    "format_version": MATTER_REPORT_FORMAT_VERSION,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            },
            &now,
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CSV builders
// ---------------------------------------------------------------------------

fn write_text_file(path: &Utf8Path, content: &str) -> Result<()> {
    let mut f = fs::File::create(path.as_std_path())?;
    f.write_all(content.as_bytes())?;
    f.flush()?;
    Ok(())
}

/// Truncate `s` to at most `max_bytes` UTF-8 bytes without splitting a codepoint.
/// Appends an ellipsis when truncation occurs.
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn csv_line(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|f| csv_escape_field(f))
        .collect::<Vec<_>>()
        .join(",")
}

fn build_summary_csv(
    matter_id: &str,
    matter_name: &str,
    generated_at: &str,
    generated_at_excel: &str,
    ov: &CaseOverview,
) -> String {
    let mut out = String::from("metric,value\n");
    let mut row = |k: &str, v: &str| {
        out.push_str(&csv_line(&[k, v]));
        out.push('\n');
    };

    row("matter_id", matter_id);
    row("matter_name", matter_name);
    row("generated_at", generated_at);
    row("generated_at_excel", generated_at_excel);
    row("schema_version", &SCHEMA_VERSION.to_string());
    row("report_format_version", MATTER_REPORT_FORMAT_VERSION);
    row("app_version", env!("CARGO_PKG_VERSION"));
    row("pdf_written", "false");

    row("items_total", &ov.totals.items_total.to_string());
    row("top_level_items", &ov.totals.top_level_items.to_string());
    row(
        "size_bytes_top_level",
        &ov.totals.size_bytes_top_level.to_string(),
    );
    row("sources_total", &ov.totals.sources_total.to_string());
    row("families_total", &ov.totals.families_total.to_string());

    row("in_review", &ov.review.in_review.to_string());
    row("reviewed_count", &ov.review.reviewed_count.to_string());
    row("unreviewed_count", &ov.review.unreviewed_count.to_string());

    row("dedup_unique", &ov.dedup.unique.to_string());
    row("dedup_duplicate", &ov.dedup.duplicate.to_string());
    row("dedup_skipped", &ov.dedup.skipped.to_string());
    row("dedup_null", &ov.dedup.null_role.to_string());

    row(
        "cull_never_run",
        if ov.cull.never_run { "true" } else { "false" },
    );
    row("cull_included", &ov.cull.included.to_string());
    row("cull_culled", &ov.cull.culled.to_string());
    row("cull_other", &ov.cull.other.to_string());

    row("privilege_claimed", &ov.privilege.claimed.to_string());
    row("privilege_withhold", &ov.privilege.withhold.to_string());

    row("pdf_needs_ocr", &ov.ocr.pdf_needs_ocr.to_string());
    row("has_text", &ov.ocr.has_text.to_string());
    row("has_native", &ov.ocr.has_native.to_string());

    row("item_errors_total", &ov.errors.total.to_string());

    row("jobs_pending", &ov.jobs.pending.to_string());
    row("jobs_running", &ov.jobs.running.to_string());
    row("jobs_paused", &ov.jobs.paused.to_string());
    row("jobs_failed", &ov.jobs.failed.to_string());
    row("jobs_cancelled", &ov.jobs.cancelled.to_string());
    row("jobs_succeeded", &ov.jobs.succeeded.to_string());

    out
}

fn build_label_count_csv(
    header_label: &str,
    rows: &[LabelCount],
    label_fn: fn(&str) -> &str,
    other_count: u64,
) -> String {
    let mut out = format!("{header_label},count\n");
    if rows.is_empty() && other_count == 0 {
        out.push_str(&csv_line(&[LABEL_NONE, "0"]));
        out.push('\n');
        return out;
    }
    for r in rows {
        let label = label_fn(&r.label);
        out.push_str(&csv_line(&[label, &r.count.to_string()]));
        out.push('\n');
    }
    if other_count > 0 {
        out.push_str(&csv_line(&[LABEL_OTHER, &other_count.to_string()]));
        out.push('\n');
    }
    out
}

fn build_jobs_csv_all(matter: &Matter) -> Result<String> {
    let jobs = matter.list_jobs()?;
    let mut out = String::from(
        "job_id,kind,state,started_at_excel,finished_at_excel,started_at_rfc3339,finished_at_rfc3339,completed_count,error_summary_safe\n",
    );
    if jobs.is_empty() {
        out.push_str(&csv_line(&[LABEL_NONE, "", "", "", "", "", "", "0", ""]));
        out.push('\n');
        return Ok(out);
    }
    for j in jobs {
        let completed: Option<i64> = matter.connection().query_row(
            "SELECT MAX(completed_count) FROM job_checkpoints WHERE job_id = ?1",
            rusqlite::params![j.id],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let started = j.started_at.as_deref().unwrap_or("");
        let finished = j.finished_at.as_deref().unwrap_or("");
        let completed_s = completed.map(|c| c.to_string()).unwrap_or_default();
        let scrubbed = j
            .error_summary
            .as_deref()
            .map(scrub_error_summary)
            .unwrap_or_default();
        out.push_str(&csv_line(&[
            j.id.as_str(),
            j.kind.as_str(),
            j.state.as_str(),
            &rfc3339_to_excel_utc(started),
            &rfc3339_to_excel_utc(finished),
            started,
            finished,
            completed_s.as_str(),
            scrubbed.as_str(),
        ]));
        out.push('\n');
    }
    Ok(out)
}

fn build_jobs_csv_from_overview(ov: &CaseOverview) -> String {
    let mut out = String::from(
        "job_id,kind,state,started_at_excel,finished_at_excel,started_at_rfc3339,finished_at_rfc3339,completed_count,error_summary_safe\n",
    );
    if ov.jobs.recent.is_empty() {
        out.push_str(&csv_line(&[LABEL_NONE, "", "", "", "", "", "", "0", ""]));
        out.push('\n');
        return out;
    }
    for j in &ov.jobs.recent {
        let started = j.started_at.as_deref().unwrap_or("");
        let finished = j.finished_at.as_deref().unwrap_or("");
        let completed_s = j.completed_count.map(|c| c.to_string()).unwrap_or_default();
        let scrubbed = j
            .error_summary
            .as_deref()
            .map(scrub_error_summary)
            .unwrap_or_default();
        out.push_str(&csv_line(&[
            j.id.as_str(),
            j.kind.as_str(),
            j.state.as_str(),
            &rfc3339_to_excel_utc(started),
            &rfc3339_to_excel_utc(finished),
            started,
            finished,
            completed_s.as_str(),
            scrubbed.as_str(),
        ]));
        out.push('\n');
    }
    out
}

fn build_readme() -> String {
    format!(
        "Matter progress/metrics report ({MATTER_REPORT_FORMAT_VERSION})\n\
         \n\
         This pack serializes the same KPIs and rollups as the live Case Overview.\n\
         Privacy: counts, labels, and job metadata only — no email subjects, bodies,\n\
         or privilege description text. Job error_summary values are path-scrubbed\n\
         and allowlist-filtered to stable codes / short generic phrases.\n\
         \n\
         Datetimes: summary.generated_at is RFC3339 (machines/audit).\n\
         summary.generated_at_excel and jobs.*_excel use 'YYYY-MM-DD HH:MM:SS UTC'\n\
         for spreadsheet sort/filter. Empty rollup tables contain header + (none),0.\n\
         PDF summary is deferred (D-0039-01).\n\
         Generated with matter-core {ver}.\n",
        ver = env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn scrub_redacts_windows_path_and_filename() {
        let raw = r"Failed to extract C:\client_data\super_secret_merger.pdf";
        let safe = scrub_error_summary(raw);
        assert!(!safe.contains("client_data"), "path segment leaked: {safe}");
        assert!(
            !safe.contains("super_secret_merger"),
            "filename leaked: {safe}"
        );
        assert!(!safe.contains(r"C:\"), "drive path leaked: {safe}");
        assert!(!safe.to_ascii_lowercase().contains("extract"));
        // Allowlist keeps generic "Failed"; free text dropped.
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "{safe}"
        );
    }

    #[test]
    fn scrub_redacts_windows_path_with_spaces() {
        let raw = r"Failed to extract C:\client data\super_secret_merger.pdf";
        let safe = scrub_error_summary(raw);
        assert!(!safe.contains("client"), "path segment leaked: {safe}");
        assert!(!safe.contains("super_secret"), "filename leaked: {safe}");
        assert!(!safe.contains("merger"), "filename leaked: {safe}");
        // Extension / path residual must not remain.
        assert!(
            !safe.to_ascii_lowercase().contains("pdf"),
            "pdf residual: {safe}"
        );
        assert!(!safe.contains(r"C:\"), "drive path leaked: {safe}");
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "{safe}"
        );
    }

    #[test]
    fn scrub_redacts_relative_multi_segment() {
        let raw = r"Failed on client_data\acme_deal\memo.eml";
        let safe = scrub_error_summary(raw);
        assert!(
            !safe.contains("client_data"),
            "relative path leaked: {safe}"
        );
        assert!(!safe.contains("acme_deal"), "relative path leaked: {safe}");
        assert!(!safe.contains("memo"), "filename leaked: {safe}");
        assert!(!safe.contains(".eml"), "extension residual: {safe}");
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "{safe}"
        );
    }

    #[test]
    fn scrub_redacts_unc_with_spaces() {
        let raw = r"copy failed \\fileserver\share\client data\deal memo.pdf";
        let safe = scrub_error_summary(raw);
        assert!(!safe.contains("fileserver"), "UNC host leaked: {safe}");
        assert!(!safe.contains("client"), "UNC path leaked: {safe}");
        assert!(!safe.contains("deal"), "UNC path leaked: {safe}");
        assert!(!safe.contains("memo"), "filename leaked: {safe}");
        assert!(
            !safe.to_ascii_lowercase().contains("pdf"),
            "pdf residual: {safe}"
        );
        // "copy" is free text — allowlist drops it; only generic "failed" remains.
        assert!(
            !safe.to_ascii_lowercase().contains("copy"),
            "free text: {safe}"
        );
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "{safe}"
        );
    }

    #[test]
    fn scrub_redacts_unix_and_file_uri() {
        let raw = "ocr failed file:///var/data/secret_memo.docx on /home/user/secret_memo.docx";
        let safe = scrub_error_summary(raw);
        assert!(!safe.contains("secret_memo"));
        assert!(!safe.contains("/var/data"));
        assert!(!safe.contains("/home/user"));
        assert!(!safe.contains("file://"));
        // Bare "ocr" is free text (no underscore); "failed" is generic allowlist.
        assert!(!safe.contains("ocr") || safe.contains("ocr_"));
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "{safe}"
        );
    }

    #[test]
    fn scrub_redacts_unix_root_level_path() {
        let safe = scrub_error_summary("failed /client_secret");
        assert!(
            !safe.contains("client_secret"),
            "root-level unix path leaked: {safe}"
        );
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "{safe}"
        );
    }

    #[test]
    fn scrub_drops_subject_like_free_text() {
        let safe = scrub_error_summary("failed while processing CONFIDENTIAL_SUBJECT_XYZ");
        assert!(
            !safe.to_ascii_uppercase().contains("CONFIDENTIAL"),
            "subject leaked: {safe}"
        );
        assert!(
            !safe.to_ascii_uppercase().contains("SUBJECT"),
            "subject leaked: {safe}"
        );
        assert!(!safe.contains("XYZ"), "subject leaked: {safe}");
        assert!(!safe.to_ascii_lowercase().contains("while"));
        assert!(!safe.to_ascii_lowercase().contains("processing"));
        assert!(
            safe.eq_ignore_ascii_case("failed") || safe == REDACTED,
            "expected only failed or (redacted), got: {safe}"
        );
    }

    #[test]
    fn scrub_keeps_short_codes() {
        let raw = "ocr_pdf_renderer_missing";
        assert_eq!(scrub_error_summary(raw), raw);
        assert_eq!(
            scrub_error_summary("parse_failed timeout"),
            "parse_failed timeout"
        );
    }

    #[test]
    fn scrub_drops_lowercase_snake_client_text() {
        // Finite allowlist only — syntactic snake_case is not enough.
        let safe = scrub_error_summary("acme_merger_strategy");
        assert!(!safe.contains("acme"));
        assert!(!safe.contains("merger"));
        assert!(!safe.contains("strategy"));
        assert_eq!(safe, REDACTED);

        let mixed = scrub_error_summary("failed acme_merger_strategy parse_failed");
        assert!(!mixed.contains("acme"));
        assert!(!mixed.contains("merger"));
        assert!(mixed.contains("failed"));
        assert!(mixed.contains("parse_failed"));
    }

    #[test]
    fn scrub_empty() {
        assert_eq!(scrub_error_summary(""), "");
        assert_eq!(scrub_error_summary("   "), "");
    }

    #[test]
    fn excel_datetime_from_rfc3339() {
        let excel = rfc3339_to_excel_utc("2026-07-19T10:00:00Z");
        assert_eq!(excel, "2026-07-19 10:00:00 UTC");
    }

    #[test]
    fn truncate_utf8_does_not_split_codepoint() {
        // Multibyte chars (é = 2 bytes) so max_bytes=5 lands mid-char without char boundary.
        let s = "éééééééééé"; // 10 * 2 = 20 bytes
        assert!(s.len() > 5);
        let t = truncate_utf8(s, 5);
        assert!(t.ends_with('…'));
        assert!(t.is_char_boundary(t.len() - '…'.len_utf8()) || t.ends_with('…'));
        // Body before ellipsis is valid UTF-8 prefix.
        let body = t.trim_end_matches('…');
        assert!(s.starts_with(body));
        assert!(body.len() <= 5);
    }

    #[test]
    fn empty_rollup_has_sentinel() {
        let csv = build_label_count_csv("code", &[], status_or_code_label, 0);
        assert!(csv.starts_with("code,count\n"));
        assert!(csv.contains("(none),0"));
        assert!(!csv.trim().is_empty());
    }

    #[test]
    fn non_empty_rollup_no_spurious_none() {
        let rows = vec![LabelCount {
            label: "pdf".into(),
            count: 3,
        }];
        let csv = build_label_count_csv("label", &rows, category_label, 2);
        assert!(csv.contains("pdf,3"));
        assert!(csv.contains("(other),2"));
        assert!(!csv.contains("(none),0"));
    }
}
