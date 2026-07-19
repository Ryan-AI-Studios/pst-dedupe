//! Produce-screen QC preflight and findings display helpers (track **0041**).
//!
//! Soft-gate readiness uses the same selection + gate logic as produce:
//! `review_corpus` scope, `expand_family` matching the produce checkbox, and
//! [`matter_qc::check_qc_gate`]. Opens matter read-only for cheap SQL only.

use std::fs::File;
use std::io::{BufRead, BufReader};

use camino::Utf8Path;
use matter_core::Matter;
use matter_qc::{check_qc_gate, select_item_ids, QcGateBlock, QcParams, SCOPE_REVIEW_CORPUS};

/// Max findings rows loaded into the desk panel.
pub const FINDINGS_DISPLAY_CAP: usize = 200;

/// One findings.csv row (privacy-safe: no subjects/paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QcFindingRow {
    pub rule_id: String,
    pub severity: String,
    pub item_id: String,
    pub message: String,
}

/// Soft-gate readiness for Start produce (when require_qc_pass is on).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProduceQcReadiness {
    /// Not yet evaluated (or require_qc off — treated as allow).
    #[default]
    Unknown,
    /// Gate allows produce (fresh pass, or require_qc off).
    Allowed,
    /// No qc_runs row.
    Missing,
    /// Last run failed (error findings).
    Failed {
        run_id: String,
        error_count: u64,
        warn_count: u64,
    },
    /// Selection fingerprint / count / scope mismatch.
    Stale {
        run_id: String,
        stored_count: u64,
        current_count: u64,
    },
    /// Could not open matter or evaluate (show message; do not soft-enable).
    Unavailable(String),
}

impl ProduceQcReadiness {
    /// Operator-facing label for chips / dialog.
    pub fn label(&self) -> String {
        match self {
            Self::Unknown => "QC status unknown".into(),
            Self::Allowed => "QC fresh pass — produce allowed".into(),
            Self::Missing => "No QC run yet — run production QC before produce".into(),
            Self::Failed {
                error_count,
                warn_count,
                ..
            } => format!(
                "Last QC failed ({error_count} error(s), {warn_count} warning(s)) — fix and re-run QC"
            ),
            Self::Stale {
                stored_count,
                current_count,
                ..
            } => format!(
                "Selection changed since last QC (was {stored_count}, now {current_count}) — re-run QC"
            ),
            Self::Unavailable(msg) => format!("QC preflight unavailable: {msg}"),
        }
    }

    /// Only a confirmed Allowed state unblocks Start produce when require is on.
    pub fn allows_produce(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Session summary hydrated from `qc_runs` (survives desk restart when still on disk).
#[derive(Debug, Clone, Default)]
pub struct HydratedQcSummary {
    pub passed: Option<bool>,
    pub error_count: Option<u64>,
    pub warn_count: Option<u64>,
    pub report_path: Option<String>,
    pub status: Option<String>,
}

/// Evaluate produce QC readiness for the desk soft-gate.
///
/// `expand_family` must match the produce dialog checkbox (same as
/// `expand_family_for_scan` on the QC job).
pub fn evaluate_produce_qc_readiness(
    matter_root: &Utf8Path,
    require_qc_pass: bool,
    expand_family: bool,
) -> ProduceQcReadiness {
    if !require_qc_pass {
        return ProduceQcReadiness::Allowed;
    }
    let matter = match Matter::open_for_read(matter_root) {
        Ok(m) => m,
        Err(e) => return ProduceQcReadiness::Unavailable(e.to_string()),
    };
    let params = QcParams {
        scope: SCOPE_REVIEW_CORPUS.into(),
        expand_family_for_scan: expand_family,
        ..Default::default()
    };
    let ids = match select_item_ids(&matter, &params) {
        Ok(ids) => ids,
        Err(e) => return ProduceQcReadiness::Unavailable(e.to_string()),
    };
    match check_qc_gate(&matter, SCOPE_REVIEW_CORPUS, &ids) {
        Ok(None) => ProduceQcReadiness::Allowed,
        Ok(Some(QcGateBlock::Missing)) => ProduceQcReadiness::Missing,
        Ok(Some(QcGateBlock::Failed {
            error_count,
            warn_count,
            run_id,
        })) => ProduceQcReadiness::Failed {
            run_id,
            error_count,
            warn_count,
        },
        Ok(Some(QcGateBlock::Stale {
            run_id,
            stored_count,
            current_count,
        })) => ProduceQcReadiness::Stale {
            run_id,
            stored_count,
            current_count,
        },
        Err(e) => ProduceQcReadiness::Unavailable(e.to_string()),
    }
}

/// Load latest `qc_runs` row into session fields (any scope; preflight re-checks freshness).
pub fn hydrate_last_qc_summary(matter_root: &Utf8Path) -> HydratedQcSummary {
    let Ok(matter) = Matter::open_for_read(matter_root) else {
        return HydratedQcSummary::default();
    };
    let Ok(Some(run)) = matter.load_latest_qc_run() else {
        return HydratedQcSummary::default();
    };
    HydratedQcSummary {
        passed: Some(run.passed),
        error_count: Some(run.error_count),
        warn_count: Some(run.warn_count),
        report_path: run.report_path.clone(),
        status: Some(format!(
            "from matter: passed={} errors={} warns={} scope={}",
            run.passed, run.error_count, run.warn_count, run.scope
        )),
    }
}

/// Parse `findings.csv` from a QC report directory (header + up to `cap` data rows).
pub fn load_findings_csv(report_path: &str, cap: usize) -> Result<Vec<QcFindingRow>, String> {
    let path = Utf8Path::new(report_path).join("findings.csv");
    if !path.as_std_path().exists() {
        return Err(format!("findings.csv not found under {report_path}"));
    }
    let file = File::open(path.as_std_path()).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    let mut lines = reader.lines();
    // Skip header
    let _ = lines.next();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if out.len() >= cap {
            break;
        }
        let cols = parse_csv_line(&line);
        let rule_id = cols.first().cloned().unwrap_or_default();
        let severity = cols.get(1).cloned().unwrap_or_default();
        let item_id = cols.get(2).cloned().unwrap_or_default();
        let message = cols.get(3).cloned().unwrap_or_default();
        out.push(QcFindingRow {
            rule_id,
            severity,
            item_id,
            message,
        });
    }
    Ok(out)
}

/// Minimal CSV line split (handles quoted fields with commas).
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_csv_line_basic() {
        let cols = parse_csv_line("zero_size,warn,itm1,zero size_bytes");
        assert_eq!(cols.len(), 4);
        assert_eq!(cols[0], "zero_size");
        assert_eq!(cols[3], "zero size_bytes");
    }

    #[test]
    fn parse_csv_line_quoted() {
        let cols = parse_csv_line(r#"rule,error,id,"msg, with comma""#);
        assert_eq!(cols[3], "msg, with comma");
    }

    #[test]
    fn readiness_allows_when_not_required() {
        // Path ignored when require_qc_pass is false.
        let r = evaluate_produce_qc_readiness(Utf8Path::new("C:\\nope"), false, false);
        assert_eq!(r, ProduceQcReadiness::Allowed);
        assert!(r.allows_produce());
    }

    #[test]
    fn blocks_when_unknown() {
        let r = ProduceQcReadiness::Unknown;
        assert!(!r.allows_produce());
    }
}
