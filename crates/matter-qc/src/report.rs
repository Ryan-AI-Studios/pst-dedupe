//! QC findings report pack (summary.csv + findings.csv).

use std::fs;
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use matter_core::EXPORTS_DIR;

use crate::error::{QcError, Result};
use crate::params::QcSeverity;
use crate::rules::QcFinding;

const SUMMARY_FILE: &str = "summary.csv";
const FINDINGS_FILE: &str = "findings.csv";
const README_FILE: &str = "README.txt";

/// Default stamped QC report directory under `matter_root/exports/qc/`.
///
/// Stamp includes sub-second millis so rapid successive runs do not collide.
pub fn default_qc_report_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
    let stamp = Utc::now().format("%Y%m%d_%H%M%S_%3f");
    matter_root
        .join(EXPORTS_DIR)
        .join("qc")
        .join(format!("qc_{stamp}"))
}

/// Summary fields written into the QC report pack.
#[derive(Debug, Clone)]
pub struct QcReportMeta<'a> {
    pub matter_id: &'a str,
    pub profile: &'a str,
    pub scope: &'a str,
    pub passed: bool,
    pub error_count: u64,
    pub warn_count: u64,
    pub candidate_count: u64,
    pub selection_fingerprint: &'a str,
}

/// Write findings + summary CSV under `output_dir` via atomic tmp+rename.
///
/// Returns the final output directory path as a string.
pub fn write_qc_report(
    output_dir: &Utf8Path,
    meta: &QcReportMeta<'_>,
    findings: &[QcFinding],
) -> Result<String> {
    if output_dir.as_str().is_empty() {
        return Err(QcError::Other("report output_dir is empty".into()));
    }
    if output_dir.exists() {
        return Err(QcError::Other(format!(
            "QC report directory already exists: {output_dir} (refuse overwrite)"
        )));
    }

    let tmp_dir = Utf8PathBuf::from(format!("{output_dir}.tmp"));
    if tmp_dir.exists() {
        fs::remove_dir_all(tmp_dir.as_std_path())?;
    }
    if let Some(parent) = tmp_dir.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    fs::create_dir_all(tmp_dir.as_std_path())?;

    let write_result = (|| -> Result<()> {
        write_summary(&tmp_dir, meta)?;
        write_findings(&tmp_dir, findings)?;
        write_readme(&tmp_dir)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_dir_all(tmp_dir.as_std_path());
        return Err(e);
    }

    // Parent of final must exist
    if let Some(parent) = output_dir.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    fs::rename(tmp_dir.as_std_path(), output_dir.as_std_path()).map_err(|e| {
        let _ = fs::remove_dir_all(tmp_dir.as_std_path());
        QcError::Io(e)
    })?;

    Ok(output_dir.to_string())
}

fn write_summary(dir: &Utf8Path, meta: &QcReportMeta<'_>) -> Result<()> {
    let path = dir.join(SUMMARY_FILE);
    let mut f = fs::File::create(path.as_std_path())?;
    writeln!(f, "metric,value")?;
    writeln!(f, "matter_id,{}", csv_escape(meta.matter_id))?;
    writeln!(f, "profile,{}", csv_escape(meta.profile))?;
    writeln!(f, "scope,{}", csv_escape(meta.scope))?;
    writeln!(f, "candidate_count,{}", meta.candidate_count)?;
    writeln!(f, "error_count,{}", meta.error_count)?;
    writeln!(f, "warn_count,{}", meta.warn_count)?;
    writeln!(f, "passed,{}", if meta.passed { "1" } else { "0" })?;
    writeln!(
        f,
        "selection_fingerprint,{}",
        csv_escape(meta.selection_fingerprint)
    )?;
    writeln!(f, "generated_at,{}", csv_escape(&Utc::now().to_rfc3339()))?;
    Ok(())
}

fn write_findings(dir: &Utf8Path, findings: &[QcFinding]) -> Result<()> {
    let path = dir.join(FINDINGS_FILE);
    let mut wtr = csv::Writer::from_path(path.as_std_path())?;
    wtr.write_record(["rule_id", "severity", "item_id", "message"])?;
    if findings.is_empty() {
        // Always at least header; no sentinel needed for findings.
    } else {
        for f in findings {
            wtr.write_record([
                f.rule_id.as_str(),
                f.severity.as_str(),
                f.item_id.as_deref().unwrap_or(""),
                f.message.as_str(),
            ])?;
        }
    }
    wtr.flush()?;
    Ok(())
}

fn write_readme(dir: &Utf8Path) -> Result<()> {
    let path = dir.join(README_FILE);
    let mut f = fs::File::create(path.as_std_path())?;
    writeln!(
        f,
        "Production QC report pack (track 0041).\n\
         \n\
         Privacy: findings.csv contains rule_id, severity, item_id, and short messages only.\n\
         No subjects, bodies, paths, or privilege descriptions.\n\
         \n\
         Files:\n\
         - summary.csv: metric,value KPIs\n\
         - findings.csv: rule_id,severity,item_id,message\n"
    )?;
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Count errors / warns from findings (Off should never appear).
pub fn count_severities(findings: &[QcFinding]) -> (u64, u64) {
    let mut errors = 0u64;
    let mut warns = 0u64;
    for f in findings {
        match f.severity {
            QcSeverity::Error => errors += 1,
            QcSeverity::Warn => warns += 1,
            QcSeverity::Off => {}
        }
    }
    (errors, warns)
}
