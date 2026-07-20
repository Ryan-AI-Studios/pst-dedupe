//! Date window emptiness + week/month bucket hole detection.
//!
//! **P0 forbids day-level buckets** (weekend spam). Only `week` and `month`.

use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{GapError, Result};
use crate::params::{BUCKET_MONTH, BUCKET_WEEK};

/// Finding ids.
pub const FINDING_DATE_WINDOW_EMPTY: &str = "date_window_empty";
pub const FINDING_DATE_BUCKET_HOLE: &str = "date_bucket_hole";

/// Severity labels (locked: empty window = error; hole = warn).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSeverity {
    Warn,
    Error,
}

impl GapSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// One date-related finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateFinding {
    pub finding_id: String,
    pub severity: GapSeverity,
    pub message: String,
    pub bucket_start: Option<String>,
    pub bucket_end: Option<String>,
    pub item_count: u64,
}

/// One coverage bucket row for date_coverage.csv.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateBucketRow {
    pub bucket_start: String,
    pub bucket_end: String,
    pub item_count: u64,
    pub is_hole: bool,
}

/// Parse an operator window bound (RFC3339 preferred; also bare YYYY-MM-DD).
pub fn parse_window_bound(s: &str) -> Result<chrono::DateTime<Utc>> {
    let t = s.trim();
    if t.is_empty() {
        return Err(GapError::InvalidParams("empty window bound".into()));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        let naive = d
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| GapError::InvalidParams(format!("invalid date '{t}'")))?;
        return Ok(Utc.from_utc_datetime(&naive));
    }
    Err(GapError::InvalidParams(format!(
        "unparseable window bound '{t}' (use RFC3339 or YYYY-MM-DD)"
    )))
}

/// Best-effort parse of an item date string to UTC day.
pub fn parse_item_date(s: &str) -> Option<chrono::DateTime<Utc>> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc));
    }
    // Truncate fractional / space forms common in extracts
    if let Ok(d) = NaiveDate::parse_from_str(&t[..t.len().min(10)], "%Y-%m-%d") {
        return Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?));
    }
    None
}

/// Start of ISO week (Monday) for a date.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    let wd = d.weekday();
    let days_from_mon = wd.num_days_from_monday() as i64;
    d - Duration::days(days_from_mon)
}

/// Start of calendar month.
pub fn month_start(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

/// Next bucket start after `start` for the given bucket kind.
pub fn next_bucket(start: NaiveDate, bucket: &str) -> Result<NaiveDate> {
    match bucket {
        BUCKET_WEEK => Ok(start + Duration::days(7)),
        BUCKET_MONTH => {
            let (y, m) = if start.month() == 12 {
                (start.year() + 1, 1)
            } else {
                (start.year(), start.month() + 1)
            };
            Ok(NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(start + Duration::days(30)))
        }
        "day" => Err(GapError::InvalidParams("day bucket forbidden in P0".into())),
        other => Err(GapError::InvalidParams(format!("unknown bucket '{other}'"))),
    }
}

/// Analyze date coverage inside an optional window.
///
/// - If both window bounds set and zero items fall inside → `date_window_empty` (error).
/// - Interior zero buckets with non-zero neighbors → `date_bucket_hole` (warn).
/// - Bucket size is week (default) or month only.
pub fn analyze_date_coverage(
    item_dates: &[(String, String)],
    window_start: Option<&str>,
    window_end: Option<&str>,
    bucket: &str,
) -> Result<(Vec<DateFinding>, Vec<DateBucketRow>)> {
    // Reject day explicitly.
    if bucket == "day" {
        return Err(GapError::InvalidParams(
            "date bucket 'day' is forbidden in P0 (use week or month)".into(),
        ));
    }
    if bucket != BUCKET_WEEK && bucket != BUCKET_MONTH {
        return Err(GapError::InvalidParams(format!(
            "unknown date bucket '{bucket}' (expected week or month)"
        )));
    }

    let win_start = window_start
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_window_bound)
        .transpose()?;
    let win_end = window_end
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_window_bound)
        .transpose()?;

    // If no window, skip date rules entirely.
    if win_start.is_none() && win_end.is_none() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Epoch fallback when only window_end is set (never panic: from_timestamp(0) is always Some).
    let start = win_start
        .unwrap_or_else(|| chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(Utc::now));
    let end = win_end.unwrap_or_else(Utc::now);

    let mut in_window_dates: Vec<NaiveDate> = Vec::new();
    for (_id, ds) in item_dates {
        if let Some(dt) = parse_item_date(ds) {
            if dt >= start && dt <= end {
                in_window_dates.push(dt.date_naive());
            }
        }
    }

    let mut findings = Vec::new();
    if in_window_dates.is_empty() {
        findings.push(DateFinding {
            finding_id: FINDING_DATE_WINDOW_EMPTY.into(),
            severity: GapSeverity::Error,
            message: format!(
                "no items with dates in window {} .. {}",
                start.to_rfc3339(),
                end.to_rfc3339()
            ),
            bucket_start: Some(start.to_rfc3339()),
            bucket_end: Some(end.to_rfc3339()),
            item_count: 0,
        });
        return Ok((findings, Vec::new()));
    }

    // Build buckets covering [start_day, end_day]
    let start_day = start.date_naive();
    let end_day = end.date_naive();
    let mut b0 = match bucket {
        BUCKET_WEEK => week_start(start_day),
        _ => month_start(start_day),
    };

    let mut buckets: Vec<(NaiveDate, NaiveDate, u64)> = Vec::new();
    while b0 <= end_day {
        let b1 = next_bucket(b0, bucket)?;
        let count = in_window_dates
            .iter()
            .filter(|d| **d >= b0 && **d < b1)
            .count() as u64;
        buckets.push((b0, b1, count));
        b0 = b1;
        if buckets.len() > 10_000 {
            break; // safety
        }
    }

    // Hole: interior zero with non-zero neighbors
    let mut rows = Vec::new();
    for (i, (bs, be, count)) in buckets.iter().enumerate() {
        let mut is_hole = false;
        if *count == 0 && i > 0 && i + 1 < buckets.len() {
            let prev = buckets[i - 1].2;
            let next = buckets[i + 1].2;
            if prev > 0 && next > 0 {
                is_hole = true;
                findings.push(DateFinding {
                    finding_id: FINDING_DATE_BUCKET_HOLE.into(),
                    severity: GapSeverity::Warn,
                    message: format!(
                        "zero items in {bucket} bucket {} .. {} (neighbors have volume)",
                        bs, be
                    ),
                    bucket_start: Some(bs.to_string()),
                    bucket_end: Some(be.to_string()),
                    item_count: 0,
                });
            }
        }
        rows.push(DateBucketRow {
            bucket_start: bs.to_string(),
            bucket_end: be.to_string(),
            item_count: *count,
            is_hole,
        });
    }

    Ok((findings, rows))
}

/// Public helper: allowed buckets for docs/tests.
pub fn allowed_buckets() -> &'static [&'static str] {
    &[BUCKET_WEEK, BUCKET_MONTH]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_day_bucket() {
        assert!(!allowed_buckets().contains(&"day"));
        let err =
            analyze_date_coverage(&[], Some("2020-01-01"), Some("2020-01-31"), "day").unwrap_err();
        assert!(err.to_string().contains("day"));
    }

    #[test]
    fn empty_window_is_error() {
        let (findings, _) = analyze_date_coverage(
            &[],
            Some("2020-01-01T00:00:00Z"),
            Some("2020-12-31T23:59:59Z"),
            BUCKET_WEEK,
        )
        .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_id, FINDING_DATE_WINDOW_EMPTY);
        assert_eq!(findings[0].severity, GapSeverity::Error);
    }

    #[test]
    fn week_start_is_monday() {
        // 2020-01-15 is Wednesday → week start Monday 2020-01-13
        let d = NaiveDate::from_ymd_opt(2020, 1, 15).unwrap();
        assert_eq!(week_start(d).weekday().num_days_from_monday(), 0);
    }

    /// Dates only in week N-1 and N+1 → interior week N is a hole (warn).
    #[test]
    fn date_bucket_hole_warn_for_middle_week() {
        // Window spans three ISO weeks in Jan 2020:
        //   week of Mon 2020-01-06, Mon 2020-01-13, Mon 2020-01-20
        // Items only on 2020-01-08 (week N-1) and 2020-01-22 (week N+1).
        let items = vec![
            ("a".into(), "2020-01-08T12:00:00Z".into()),
            ("b".into(), "2020-01-22T12:00:00Z".into()),
        ];
        let (findings, rows) = analyze_date_coverage(
            &items,
            Some("2020-01-06T00:00:00Z"),
            Some("2020-01-26T23:59:59Z"),
            BUCKET_WEEK,
        )
        .expect("analyze");
        let holes: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_id == FINDING_DATE_BUCKET_HOLE)
            .collect();
        assert!(
            !holes.is_empty(),
            "expected date_bucket_hole for middle week; findings={findings:?} rows={rows:?}"
        );
        assert!(holes.iter().all(|f| f.severity == GapSeverity::Warn));
        assert!(rows.iter().any(|r| r.is_hole && r.item_count == 0));
    }

    #[test]
    fn open_start_window_uses_epoch_not_panic() {
        // Only window_end set → start defaults to Unix epoch without unwrap panic.
        let (findings, _) =
            analyze_date_coverage(&[], None, Some("1970-01-02T00:00:00Z"), BUCKET_WEEK)
                .expect("analyze");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_id, FINDING_DATE_WINDOW_EMPTY);
    }
}
