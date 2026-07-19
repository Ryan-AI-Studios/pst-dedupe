//! Gap report pack under `exports/gap/gap_<stamp>/`.

use std::fs;
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use matter_core::{CustodianInventoryRow, GapExpectedDoc, EXPORTS_DIR};

use crate::compare::{CompareHit, MatchKey};
use crate::date_coverage::{DateBucketRow, DateFinding, GapSeverity};
use crate::error::{GapError, Result};
use crate::roster::RosterFinding;

const SUMMARY_FILE: &str = "summary.csv";
const MISSING_CUSTODIANS_FILE: &str = "missing_custodians.csv";
const CUSTODIAN_INVENTORY_FILE: &str = "custodian_inventory.csv";
const DATE_COVERAGE_FILE: &str = "date_coverage.csv";
const OPPOSING_SUMMARY_FILE: &str = "opposing_summary.csv";
const EXPECTED_NOT_IN_MATTER_FILE: &str = "expected_not_in_matter.csv";
const MATCHED_FILE: &str = "matched.csv";
const README_FILE: &str = "README.txt";

/// Default stamped gap report directory under `matter_root/exports/gap/`.
pub fn default_gap_report_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
    let stamp = Utc::now().format("%Y%m%d_%H%M%S_%3f");
    matter_root
        .join(EXPORTS_DIR)
        .join("gap")
        .join(format!("gap_{stamp}"))
}

/// Inputs for the report pack.
#[derive(Debug, Clone, Default)]
pub struct GapReportMeta<'a> {
    pub matter_id: &'a str,
    pub kind: &'a str,
    pub error_count: u64,
    pub warn_count: u64,
    pub finding_count: u64,
    pub expected_custodian_count: u64,
    pub missing_custodian_count: u64,
    pub unexpected_custodian_count: u64,
    pub expected_doc_count: u64,
    pub matched_count: u64,
    pub expected_not_in_matter_count: u64,
}

/// Write the full gap report pack via atomic tmp+rename.
///
/// Returns the final output directory path as a string.
#[allow(clippy::too_many_arguments)]
pub fn write_gap_report(
    output_dir: &Utf8Path,
    meta: &GapReportMeta<'_>,
    roster_findings: &[RosterFinding],
    inventory: &[CustodianInventoryRow],
    date_findings: &[DateFinding],
    date_buckets: &[DateBucketRow],
    unmatched_expected: &[GapExpectedDoc],
    matched: &[CompareHit],
) -> Result<String> {
    if output_dir.as_str().is_empty() {
        return Err(GapError::Other("report output_dir is empty".into()));
    }
    if output_dir.exists() {
        return Err(GapError::Other(format!(
            "gap report directory already exists: {output_dir} (refuse overwrite)"
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
        write_missing_custodians(&tmp_dir, roster_findings)?;
        write_inventory(&tmp_dir, inventory)?;
        if !date_buckets.is_empty() || !date_findings.is_empty() {
            write_date_coverage(&tmp_dir, date_buckets, date_findings)?;
        }
        write_opposing_summary(&tmp_dir, meta, unmatched_expected, matched)?;
        write_expected_not_in_matter(&tmp_dir, unmatched_expected)?;
        write_matched(&tmp_dir, matched)?;
        write_readme(&tmp_dir)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_dir_all(tmp_dir.as_std_path());
        return Err(e);
    }

    if let Some(parent) = output_dir.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    fs::rename(tmp_dir.as_std_path(), output_dir.as_std_path()).map_err(|e| {
        let _ = fs::remove_dir_all(tmp_dir.as_std_path());
        GapError::Io(e)
    })?;

    Ok(output_dir.to_string())
}

fn write_summary(dir: &Utf8Path, meta: &GapReportMeta<'_>) -> Result<()> {
    let path = dir.join(SUMMARY_FILE);
    let mut f = fs::File::create(path.as_std_path())?;
    writeln!(f, "metric,value")?;
    writeln!(f, "matter_id,{}", csv_escape(meta.matter_id))?;
    writeln!(f, "kind,{}", csv_escape(meta.kind))?;
    writeln!(f, "error_count,{}", meta.error_count)?;
    writeln!(f, "warn_count,{}", meta.warn_count)?;
    writeln!(f, "finding_count,{}", meta.finding_count)?;
    writeln!(
        f,
        "expected_custodian_count,{}",
        meta.expected_custodian_count
    )?;
    writeln!(
        f,
        "missing_custodian_count,{}",
        meta.missing_custodian_count
    )?;
    writeln!(
        f,
        "unexpected_custodian_count,{}",
        meta.unexpected_custodian_count
    )?;
    writeln!(f, "expected_doc_count,{}", meta.expected_doc_count)?;
    writeln!(f, "matched_count,{}", meta.matched_count)?;
    writeln!(
        f,
        "expected_not_in_matter_count,{}",
        meta.expected_not_in_matter_count
    )?;
    writeln!(f, "generated_at,{}", csv_escape(&Utc::now().to_rfc3339()))?;
    Ok(())
}

fn write_missing_custodians(dir: &Utf8Path, findings: &[RosterFinding]) -> Result<()> {
    let path = dir.join(MISSING_CUSTODIANS_FILE);
    let mut wtr = csv::Writer::from_path(path.as_std_path())?;
    wtr.write_record([
        "finding_id",
        "severity",
        "custodian",
        "name_norm",
        "message",
    ])?;
    for f in findings
        .iter()
        .filter(|f| f.finding_id == crate::roster::FINDING_MISSING_CUSTODIAN)
    {
        wtr.write_record([
            f.finding_id.as_str(),
            f.severity.as_str(),
            f.custodian.as_str(),
            f.name_norm.as_str(),
            f.message.as_str(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_inventory(dir: &Utf8Path, inventory: &[CustodianInventoryRow]) -> Result<()> {
    let path = dir.join(CUSTODIAN_INVENTORY_FILE);
    let mut wtr = csv::Writer::from_path(path.as_std_path())?;
    wtr.write_record(["custodian", "name_norm", "item_count"])?;
    for r in inventory {
        wtr.write_record([
            r.custodian.as_str(),
            r.name_norm.as_str(),
            &r.item_count.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_date_coverage(
    dir: &Utf8Path,
    buckets: &[DateBucketRow],
    findings: &[DateFinding],
) -> Result<()> {
    let path = dir.join(DATE_COVERAGE_FILE);
    let mut wtr = csv::Writer::from_path(path.as_std_path())?;
    wtr.write_record([
        "bucket_start",
        "bucket_end",
        "item_count",
        "is_hole",
        "finding_id",
        "severity",
        "message",
    ])?;
    for b in buckets {
        wtr.write_record([
            b.bucket_start.as_str(),
            b.bucket_end.as_str(),
            &b.item_count.to_string(),
            if b.is_hole { "1" } else { "0" },
            "",
            "",
            "",
        ])?;
    }
    for f in findings {
        wtr.write_record([
            f.bucket_start.as_deref().unwrap_or(""),
            f.bucket_end.as_deref().unwrap_or(""),
            &f.item_count.to_string(),
            "",
            f.finding_id.as_str(),
            f.severity.as_str(),
            f.message.as_str(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_opposing_summary(
    dir: &Utf8Path,
    meta: &GapReportMeta<'_>,
    unmatched: &[GapExpectedDoc],
    matched: &[CompareHit],
) -> Result<()> {
    let path = dir.join(OPPOSING_SUMMARY_FILE);
    let mut f = fs::File::create(path.as_std_path())?;
    writeln!(f, "metric,value")?;
    writeln!(f, "expected_doc_count,{}", meta.expected_doc_count)?;
    writeln!(f, "matched_count,{}", matched.len())?;
    writeln!(f, "expected_not_in_matter_count,{}", unmatched.len())?;
    Ok(())
}

fn write_expected_not_in_matter(dir: &Utf8Path, unmatched: &[GapExpectedDoc]) -> Result<()> {
    let path = dir.join(EXPECTED_NOT_IN_MATTER_FILE);
    let mut wtr = csv::Writer::from_path(path.as_std_path())?;
    // No subject column by design.
    wtr.write_record([
        "expected_id",
        "control_number",
        "sha256",
        "message_id",
        "item_id",
        "custodian",
        "file_name",
        "file_category",
        "file_ext",
    ])?;
    for d in unmatched {
        wtr.write_record([
            d.id.as_str(),
            d.control_number.as_deref().unwrap_or(""),
            d.sha256.as_deref().unwrap_or(""),
            d.message_id.as_deref().unwrap_or(""),
            d.item_id.as_deref().unwrap_or(""),
            d.custodian.as_deref().unwrap_or(""),
            d.file_name.as_deref().unwrap_or(""),
            d.file_category.as_deref().unwrap_or(""),
            d.file_ext.as_deref().unwrap_or(""),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_matched(dir: &Utf8Path, matched: &[CompareHit]) -> Result<()> {
    let path = dir.join(MATCHED_FILE);
    let mut wtr = csv::Writer::from_path(path.as_std_path())?;
    wtr.write_record(["expected_id", "matter_item_id", "match_key"])?;
    for m in matched {
        let key = match m.key {
            MatchKey::MessageId => "message_id",
            MatchKey::ItemId => "item_id",
            MatchKey::LogicalHash => "logical_hash",
            MatchKey::NativeSha256 => "native_sha256",
            MatchKey::ControlNumber => "control_number",
        };
        wtr.write_record([m.expected_id.as_str(), m.matter_item_id.as_str(), key])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_readme(dir: &Utf8Path) -> Result<()> {
    let path = dir.join(README_FILE);
    let mut f = fs::File::create(path.as_std_path())?;
    writeln!(
        f,
        "Gap analysis report pack (track 0042).\n\
         \n\
         Privacy: subjects are omitted by default. Reports use control numbers, hashes,\n\
         message-ids, custodians, and file names only.\n\
         \n\
         Files:\n\
         - summary.csv\n\
         - missing_custodians.csv\n\
         - custodian_inventory.csv\n\
         - date_coverage.csv (when window used)\n\
         - opposing_summary.csv\n\
         - expected_not_in_matter.csv\n\
         - matched.csv (thin ids)\n"
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

/// Count error/warn from mixed findings.
pub fn count_severities(
    roster: &[RosterFinding],
    date: &[DateFinding],
    expected_not_in_matter: u64,
) -> (u64, u64) {
    let mut errors = 0u64;
    let mut warns = 0u64;
    for f in roster {
        match f.severity {
            GapSeverity::Error => errors += 1,
            GapSeverity::Warn => warns += 1,
        }
    }
    for f in date {
        match f.severity {
            GapSeverity::Error => errors += 1,
            GapSeverity::Warn => warns += 1,
        }
    }
    // expected_not_in_matter treated as warn (noise control)
    warns += expected_not_in_matter;
    (errors, warns)
}
