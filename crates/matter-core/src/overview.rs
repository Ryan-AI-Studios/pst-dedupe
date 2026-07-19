//! Case overview aggregations (track **0038**).
//!
//! Read-only SQL rollups for the desk Overview panel and for **0039** exportable
//! reports. Metrics contract:
//!
//! - **Size** = top-level only (`role IS NULL OR role != 'attachment'`)
//! - **No** `COUNT(DISTINCT family_id)` — use top-level counts / parent count
//! - **Review progress** = coded vs uncoded within the default review set
//! - **Errors** = matter-scoped total + top-N by `code`
//!
//! Load path: [`load_case_overview`] fans out independent rollups via multiple
//! short-lived [`Matter::open_for_read`] connections (`std::thread`, WAL readers).

use std::thread;

use camino::Utf8Path;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::jobs::JobState;
use crate::matter::{item_cull_status, item_role, DedupRoleCounts, Matter};
use crate::privilege::privilege_status;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Options for [`load_case_overview`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewOptions {
    /// Max `file_category` buckets (default 25).
    pub top_categories: usize,
    /// Max custodian buckets (default 25).
    pub top_custodians: usize,
    /// Max error-code buckets (default 15).
    pub top_error_codes: usize,
    /// Max recent jobs in the jobs strip (default 5).
    pub recent_jobs: usize,
}

impl Default for OverviewOptions {
    fn default() -> Self {
        Self {
            top_categories: 25,
            top_custodians: 25,
            top_error_codes: 15,
            recent_jobs: 5,
        }
    }
}

/// One label → count row for rollup tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelCount {
    /// Raw label (may be empty). UI maps empty category → `(uncategorized)`,
    /// empty custodian → `(none)`.
    pub label: String,
    pub count: u64,
}

/// KPI totals for the case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewTotals {
    /// All item rows (including attachments).
    pub items_total: u64,
    /// `SUM(COALESCE(size_bytes,0))` where `role IS NULL OR role != 'attachment'`.
    /// Never a naive sum over every row (would double-count PST/parent + children).
    pub size_bytes_top_level: u64,
    pub sources_total: u64,
    /// Count where `role IS NULL OR role != 'attachment'` (standalone + parent).
    pub top_level_items: u64,
    /// Optional parent-only count (`role = 'parent'`). **Not** `COUNT(DISTINCT family_id)`.
    pub families_total: u64,
}

/// Cull posture for the matter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CullOverview {
    /// True when no item has a non-null `cull_status` (cull never run).
    pub never_run: bool,
    pub included: u64,
    pub culled: u64,
    /// Residual non-null statuses other than included/culled.
    pub other: u64,
}

/// Review progress within the default review set (same membership as
/// [`Matter::count_in_review`] with `set_id = None`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewOverview {
    pub in_review: u64,
    /// Subset of `in_review` with ≥1 row in `item_codes`.
    pub reviewed_count: u64,
    /// Subset of `in_review` with zero codes.
    pub unreviewed_count: u64,
}

/// Thin privilege posture (counts only — no descriptions).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegeOverview {
    /// Active claim rows in `item_privilege` (asserted / under_review / partial_redaction).
    pub claimed: u64,
    /// Distinct items withheld: `items.privilege_withhold = 1` **or** an
    /// `item_privilege` row with `withhold = 1` (matches filter / production hold).
    pub withhold: u64,
}

/// OCR / extract health chips.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrOverview {
    pub pdf_needs_ocr: u64,
    /// Optional: items with `text_sha256 IS NOT NULL`.
    pub has_text: u64,
    /// Optional: items with `native_sha256 IS NOT NULL`.
    pub has_native: u64,
}

/// Matter-scoped item errors: total + top-N by code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorOverview {
    pub total: u64,
    pub by_code: Vec<LabelCount>,
    /// Count of error rows whose code is outside the top-N list.
    pub other_codes_count: u64,
}

/// Compact job row for the overview jobs strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewJobRow {
    pub id: String,
    pub kind: String,
    pub state: String,
    /// Max `completed_count` across checkpoints for this job, if any.
    pub completed_count: Option<i64>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_summary: Option<String>,
}

/// Jobs summary: counts by state + last N jobs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobsOverview {
    pub pending: u64,
    pub running: u64,
    pub paused: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub succeeded: u64,
    pub recent: Vec<OverviewJobRow>,
}

/// Full case overview snapshot.
///
/// Privacy: counts and labels only — never subject/body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseOverview {
    /// RFC3339 generation timestamp.
    pub generated_at: String,
    pub totals: OverviewTotals,
    pub by_status: Vec<LabelCount>,
    pub by_file_category: Vec<LabelCount>,
    /// Sum of category counts outside the top-N (0 if none or all fit).
    pub other_categories_count: u64,
    pub by_custodian: Vec<LabelCount>,
    pub other_custodians_count: u64,
    pub dedup: DedupRoleCounts,
    pub cull: CullOverview,
    pub review: ReviewOverview,
    pub privilege: PrivilegeOverview,
    pub ocr: OcrOverview,
    pub errors: ErrorOverview,
    pub jobs: JobsOverview,
}

// ---------------------------------------------------------------------------
// Public load entry
// ---------------------------------------------------------------------------

/// Load a full [`CaseOverview`] with concurrent fan-out of independent rollups.
///
/// Each slice opens its own short-lived [`Matter::open_for_read`] (WAL-safe).
/// No write transaction; never wipes `workspace/temp`.
///
/// Slice partition: totals · status · category · custodian · dedup+cull ·
/// review · privilege+OCR · errors · jobs.
pub fn load_case_overview(matter_root: &Utf8Path, opts: &OverviewOptions) -> Result<CaseOverview> {
    let root = matter_root.to_path_buf();
    let opts = opts.clone();

    // Capture results from worker threads. Each thread opens its own reader.
    let root_totals = root.clone();
    let h_totals = thread::Builder::new()
        .name("overview-totals".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_totals)?;
            m.overview_totals()
        })?;

    let root_status = root.clone();
    let h_status = thread::Builder::new()
        .name("overview-status".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_status)?;
            m.overview_by_status()
        })?;

    let root_cat = root.clone();
    let top_cat = opts.top_categories;
    let h_cat = thread::Builder::new()
        .name("overview-category".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_cat)?;
            m.overview_by_file_category(top_cat)
        })?;

    let root_cust = root.clone();
    let top_cust = opts.top_custodians;
    let h_cust = thread::Builder::new()
        .name("overview-custodian".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_cust)?;
            m.overview_by_custodian(top_cust)
        })?;

    let root_dedup_cull = root.clone();
    let h_dedup_cull = thread::Builder::new()
        .name("overview-dedup-cull".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_dedup_cull)?;
            let dedup = m.count_by_dedup_role()?;
            let cull = m.overview_cull()?;
            Ok::<_, crate::error::Error>((dedup, cull))
        })?;

    let root_review = root.clone();
    let h_review = thread::Builder::new()
        .name("overview-review".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_review)?;
            m.overview_review()
        })?;

    let root_priv_ocr = root.clone();
    let h_priv_ocr = thread::Builder::new()
        .name("overview-priv-ocr".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_priv_ocr)?;
            let privilege = m.overview_privilege()?;
            let ocr = m.overview_ocr()?;
            Ok::<_, crate::error::Error>((privilege, ocr))
        })?;

    let root_err = root.clone();
    let top_err = opts.top_error_codes;
    let h_err = thread::Builder::new()
        .name("overview-errors".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_err)?;
            m.overview_errors(top_err)
        })?;

    let root_jobs = root.clone();
    let recent_n = opts.recent_jobs;
    let h_jobs = thread::Builder::new()
        .name("overview-jobs".into())
        .spawn(move || {
            let m = Matter::open_for_read(&root_jobs)?;
            m.overview_jobs(recent_n)
        })?;

    let totals = join_thread(h_totals, "totals")?;
    let by_status = join_thread(h_status, "status")?;
    let (by_file_category, other_categories_count) = join_thread(h_cat, "category")?;
    let (by_custodian, other_custodians_count) = join_thread(h_cust, "custodian")?;
    let (dedup, cull) = join_thread(h_dedup_cull, "dedup/cull")?;
    let review = join_thread(h_review, "review")?;
    let (privilege, ocr) = join_thread(h_priv_ocr, "privilege/ocr")?;
    let errors = join_thread(h_err, "errors")?;
    let jobs = join_thread(h_jobs, "jobs")?;

    Ok(CaseOverview {
        generated_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        totals,
        by_status,
        by_file_category,
        other_categories_count,
        by_custodian,
        other_custodians_count,
        dedup,
        cull,
        review,
        privilege,
        ocr,
        errors,
        jobs,
    })
}

/// Sequential load on a single open [`Matter`] (useful for tests / small matters).
pub fn load_case_overview_on(matter: &Matter, opts: &OverviewOptions) -> Result<CaseOverview> {
    let totals = matter.overview_totals()?;
    let by_status = matter.overview_by_status()?;
    let (by_file_category, other_categories_count) =
        matter.overview_by_file_category(opts.top_categories)?;
    let (by_custodian, other_custodians_count) =
        matter.overview_by_custodian(opts.top_custodians)?;
    let dedup = matter.count_by_dedup_role()?;
    let cull = matter.overview_cull()?;
    let review = matter.overview_review()?;
    let privilege = matter.overview_privilege()?;
    let ocr = matter.overview_ocr()?;
    let errors = matter.overview_errors(opts.top_error_codes)?;
    let jobs = matter.overview_jobs(opts.recent_jobs)?;

    Ok(CaseOverview {
        generated_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        totals,
        by_status,
        by_file_category,
        other_categories_count,
        by_custodian,
        other_custodians_count,
        dedup,
        cull,
        review,
        privilege,
        ocr,
        errors,
        jobs,
    })
}

fn join_thread<T>(handle: thread::JoinHandle<Result<T>>, label: &str) -> Result<T> {
    match handle.join() {
        Ok(inner) => inner,
        Err(_) => Err(crate::error::Error::Other(format!(
            "overview {label} thread panicked"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Matter slice methods
// ---------------------------------------------------------------------------

impl Matter {
    /// Totals + top-level size / counts (no `COUNT(DISTINCT family_id)`).
    pub fn overview_totals(&self) -> Result<OverviewTotals> {
        let mid = self.id();
        let items_total: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items WHERE matter_id = ?1",
            params![mid],
            |row| row.get(0),
        )?;
        let sources_total: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM sources WHERE matter_id = ?1",
            params![mid],
            |row| row.get(0),
        )?;

        // Top-level: role IS NULL OR role != 'attachment' (NULL treated as standalone).
        let (top_level_items, size_bytes_top_level): (i64, i64) = self.connection().query_row(
            "SELECT COUNT(*), COALESCE(SUM(COALESCE(size_bytes, 0)), 0) \
             FROM items \
             WHERE matter_id = ?1 \
               AND (role IS NULL OR role != ?2)",
            params![mid, item_role::ATTACHMENT],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let families_total: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items WHERE matter_id = ?1 AND role = ?2",
            params![mid, item_role::PARENT],
            |row| row.get(0),
        )?;

        Ok(OverviewTotals {
            items_total: items_total as u64,
            size_bytes_top_level: size_bytes_top_level as u64,
            sources_total: sources_total as u64,
            top_level_items: top_level_items as u64,
            families_total: families_total as u64,
        })
    }

    /// `GROUP BY status` ordered by count DESC.
    pub fn overview_by_status(&self) -> Result<Vec<LabelCount>> {
        let mut stmt = self.connection().prepare(
            "SELECT COALESCE(status, ''), COUNT(*) \
             FROM items WHERE matter_id = ?1 \
             GROUP BY COALESCE(status, '') \
             ORDER BY COUNT(*) DESC, COALESCE(status, '') ASC",
        )?;
        let rows = stmt.query_map(params![self.id()], |row| {
            Ok(LabelCount {
                label: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Top-N file categories + remainder count.
    ///
    /// Empty/null category labels are returned as `""` (UI → `(uncategorized)`).
    pub fn overview_by_file_category(&self, top_n: usize) -> Result<(Vec<LabelCount>, u64)> {
        group_by_label_top_n(self, "COALESCE(file_category, '')", top_n)
    }

    /// Top-N custodians + remainder count.
    ///
    /// Empty/null labels returned as `""` (UI → `(none)`).
    pub fn overview_by_custodian(&self, top_n: usize) -> Result<(Vec<LabelCount>, u64)> {
        group_by_label_top_n(self, "COALESCE(custodian, '')", top_n)
    }

    /// Cull posture.
    pub fn overview_cull(&self) -> Result<CullOverview> {
        let mid = self.id();
        let any_set: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 AND cull_status IS NOT NULL",
            params![mid],
            |row| row.get(0),
        )?;
        if any_set == 0 {
            return Ok(CullOverview {
                never_run: true,
                ..Default::default()
            });
        }

        let mut stmt = self.connection().prepare(
            "SELECT cull_status, COUNT(*) FROM items \
             WHERE matter_id = ?1 AND cull_status IS NOT NULL \
             GROUP BY cull_status",
        )?;
        let rows = stmt.query_map(params![mid], |row| {
            let status: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((status, count as u64))
        })?;
        let mut included = 0u64;
        let mut culled = 0u64;
        let mut other = 0u64;
        for row in rows {
            let (status, count) = row?;
            match status.as_str() {
                s if s == item_cull_status::INCLUDED => included += count,
                s if s == item_cull_status::CULLED => culled += count,
                _ => other += count,
            }
        }
        Ok(CullOverview {
            never_run: false,
            included,
            culled,
            other,
        })
    }

    /// Review progress: in_review / reviewed (has codes) / unreviewed.
    ///
    /// Membership matches [`Matter::count_in_review`] with `set_id = None`
    /// (default set if present, else all `in_review = 1`).
    pub fn overview_review(&self) -> Result<ReviewOverview> {
        let set_id = self.get_default_review_set_id()?;
        let mid = self.id();
        let (in_review, reviewed_count): (i64, i64) = match set_id.as_deref() {
            Some(sid) => self.connection().query_row(
                "SELECT \
                    COUNT(*), \
                    COALESCE(SUM(CASE WHEN EXISTS ( \
                        SELECT 1 FROM item_codes ic WHERE ic.item_id = i.id \
                    ) THEN 1 ELSE 0 END), 0) \
                 FROM items i \
                 WHERE i.matter_id = ?1 AND i.in_review = 1 AND i.review_set_id = ?2",
                params![mid, sid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?,
            None => self.connection().query_row(
                "SELECT \
                    COUNT(*), \
                    COALESCE(SUM(CASE WHEN EXISTS ( \
                        SELECT 1 FROM item_codes ic WHERE ic.item_id = i.id \
                    ) THEN 1 ELSE 0 END), 0) \
                 FROM items i \
                 WHERE i.matter_id = ?1 AND i.in_review = 1",
                params![mid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?,
        };
        let in_review = in_review as u64;
        let reviewed_count = reviewed_count as u64;
        Ok(ReviewOverview {
            in_review,
            reviewed_count,
            unreviewed_count: in_review.saturating_sub(reviewed_count),
        })
    }

    /// Privilege claimed (active rows) + withhold flag counts.
    pub fn overview_privilege(&self) -> Result<PrivilegeOverview> {
        let mid = self.id();
        // Active statuses: asserted, under_review, partial_redaction.
        let claimed: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM item_privilege p \
             INNER JOIN items i ON i.id = p.item_id \
             WHERE i.matter_id = ?1 \
               AND p.status IN (?2, ?3, ?4)",
            params![
                mid,
                privilege_status::ASSERTED,
                privilege_status::UNDER_REVIEW,
                privilege_status::PARTIAL_REDACTION,
            ],
            |row| row.get(0),
        )?;
        // Union of denormalized item flag and privilege-table withhold (cache may drift).
        let withhold: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items i \
             WHERE i.matter_id = ?1 \
               AND ( \
                 i.privilege_withhold = 1 \
                 OR EXISTS ( \
                   SELECT 1 FROM item_privilege ip \
                   WHERE ip.item_id = i.id AND ip.matter_id = i.matter_id \
                     AND ip.withhold = 1 \
                 ) \
               )",
            params![mid],
            |row| row.get(0),
        )?;
        Ok(PrivilegeOverview {
            claimed: claimed as u64,
            withhold: withhold as u64,
        })
    }

    /// OCR / extract health.
    pub fn overview_ocr(&self) -> Result<OcrOverview> {
        let mid = self.id();
        let pdf_needs_ocr: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 AND IFNULL(pdf_needs_ocr, 0) = 1",
            params![mid],
            |row| row.get(0),
        )?;
        let has_text: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 AND text_sha256 IS NOT NULL",
            params![mid],
            |row| row.get(0),
        )?;
        let has_native: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 AND native_sha256 IS NOT NULL",
            params![mid],
            |row| row.get(0),
        )?;
        Ok(OcrOverview {
            pdf_needs_ocr: pdf_needs_ocr as u64,
            has_text: has_text as u64,
            has_native: has_native as u64,
        })
    }

    /// Matter-scoped errors: total + top-N by code.
    ///
    /// Scope: error rows linked to an item, source, or job belonging to this matter.
    pub fn overview_errors(&self, top_n: usize) -> Result<ErrorOverview> {
        let mid = self.id();
        let total: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM item_errors e \
             WHERE \
               EXISTS (SELECT 1 FROM items i WHERE i.id = e.item_id AND i.matter_id = ?1) \
               OR EXISTS (SELECT 1 FROM sources s WHERE s.id = e.source_id AND s.matter_id = ?1) \
               OR EXISTS (SELECT 1 FROM jobs j WHERE j.id = e.job_id AND j.matter_id = ?1)",
            params![mid],
            |row| row.get(0),
        )?;
        let total = total as u64;

        // Honor top_n == 0: empty list, full remainder (do not force min 1).
        let limit = top_n as i64;
        let mut by_code = Vec::new();
        let mut top_sum = 0u64;
        if limit > 0 {
            let mut stmt = self.connection().prepare(
                "SELECT COALESCE(e.code, ''), COUNT(*) AS c \
                 FROM item_errors e \
                 WHERE \
                   EXISTS (SELECT 1 FROM items i WHERE i.id = e.item_id AND i.matter_id = ?1) \
                   OR EXISTS (SELECT 1 FROM sources s WHERE s.id = e.source_id AND s.matter_id = ?1) \
                   OR EXISTS (SELECT 1 FROM jobs j WHERE j.id = e.job_id AND j.matter_id = ?1) \
                 GROUP BY COALESCE(e.code, '') \
                 ORDER BY c DESC, COALESCE(e.code, '') ASC \
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![mid, limit], |row| {
                Ok(LabelCount {
                    label: row.get(0)?,
                    count: row.get::<_, i64>(1)? as u64,
                })
            })?;
            for row in rows {
                let lc = row?;
                top_sum = top_sum.saturating_add(lc.count);
                by_code.push(lc);
            }
        }
        let other_codes_count = total.saturating_sub(top_sum);
        Ok(ErrorOverview {
            total,
            by_code,
            other_codes_count,
        })
    }

    /// Jobs counts by state + last N jobs (with optional checkpoint completed_count).
    pub fn overview_jobs(&self, recent_n: usize) -> Result<JobsOverview> {
        let jobs = self.list_jobs()?;
        let mut out = JobsOverview::default();
        for j in &jobs {
            match j.state {
                JobState::Pending => out.pending += 1,
                JobState::Running => out.running += 1,
                JobState::Paused => out.paused += 1,
                JobState::Failed => out.failed += 1,
                JobState::Cancelled => out.cancelled += 1,
                JobState::Succeeded => out.succeeded += 1,
            }
        }

        let take = recent_n.min(jobs.len());
        for j in jobs.into_iter().take(take) {
            // MAX() always returns one row (NULL when no checkpoints); do not
            // swallow real SQL/lock/schema errors with unwrap_or.
            let completed_count: Option<i64> = self.connection().query_row(
                "SELECT MAX(completed_count) FROM job_checkpoints WHERE job_id = ?1",
                params![j.id],
                |row| row.get::<_, Option<i64>>(0),
            )?;
            out.recent.push(OverviewJobRow {
                id: j.id,
                kind: j.kind,
                state: j.state.as_str().to_string(),
                completed_count,
                started_at: j.started_at,
                finished_at: j.finished_at,
                error_summary: j.error_summary,
            });
        }
        Ok(out)
    }
}

fn group_by_label_top_n(
    matter: &Matter,
    expr: &str,
    top_n: usize,
) -> Result<(Vec<LabelCount>, u64)> {
    let mid = matter.id();
    // Total item count across all groups (for remainder).
    let total_sql = format!(
        "SELECT COALESCE(SUM(c), 0) FROM ( \
            SELECT COUNT(*) AS c FROM items WHERE matter_id = ?1 GROUP BY {expr} \
         )"
    );
    let total_items: i64 = matter
        .connection()
        .query_row(&total_sql, params![mid], |row| row.get(0))?;

    // Honor top_n == 0: empty list, full remainder (do not force min 1).
    let limit = top_n as i64;
    let mut out = Vec::new();
    let mut top_sum = 0u64;
    if limit > 0 {
        let sql = format!(
            "SELECT {expr} AS lbl, COUNT(*) AS c \
             FROM items WHERE matter_id = ?1 \
             GROUP BY {expr} \
             ORDER BY c DESC, lbl ASC \
             LIMIT ?2"
        );
        let mut stmt = matter.connection().prepare(&sql)?;
        let rows = stmt.query_map(params![mid, limit], |row| {
            Ok(LabelCount {
                label: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
            })
        })?;
        for row in rows {
            let lc = row?;
            top_sum = top_sum.saturating_add(lc.count);
            out.push(lc);
        }
    }
    let other = (total_items as u64).saturating_sub(top_sum);
    Ok((out, other))
}

#[cfg(test)]
mod unit_tests {
    /// Guard: overview SQL paths must never use SQLite DISTINCT-count traps.
    #[test]
    fn overview_source_forbids_count_distinct_family_id() {
        let src = include_str!("overview.rs");
        // Build the needle so this test body does not contain the forbidden token.
        let forbidden = format!("count({inner}", inner = "distinct");
        let mut bad_lines = Vec::new();
        for (i, line) in src.lines().enumerate() {
            let t = line.trim();
            // Skip rustdoc / comments (they document the prohibition).
            if t.starts_with("//") || t.starts_with("///") || t.starts_with('*') {
                continue;
            }
            // Skip this unit-test module (string assembly only).
            if t.contains("forbidden") || t.contains("unit_tests") {
                continue;
            }
            let lower = t.to_ascii_lowercase();
            if lower.contains(&forbidden) {
                bad_lines.push(format!("{}: {t}", i + 1));
            }
        }
        assert!(
            bad_lines.is_empty(),
            "overview must not use distinct-count aggregates; found:\n{}",
            bad_lines.join("\n")
        );
    }
}
