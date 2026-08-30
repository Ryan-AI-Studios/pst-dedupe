//! Chrome produce checklist (track 0113): QC + DAT-only produce.

use std::collections::{HashMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::{
    FilterCondition, FilterSpec, Matter, PrivilegeLogExportParams, ProductionSetThin,
    UpsertPrivilegeProtocolInput, SCOPE_REVIEW_CORPUS,
};
use matter_produce::{
    effective_qc_pack_id, resolve_produce_config, run_produce, ProduceOutcome, ProduceParams,
    DEFAULT_BATES_PREFIX, SCOPE_ITEM_IDS,
};
use matter_qc::{
    check_qc_gate_for_pack, run_production_qc, QcGateBlock, QcOutcome, QcParams, QcSeverity,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{map_core, CommandError};
use crate::open_root::{open_matter_read, open_matter_write};

const ACTOR: &str = "chrome";
const DEFAULT_PROFILE: &str = "us_concordance_native_text_v1";
const RESPONSIVENESS_KEYS: &[&str] = &["responsive", "not_responsive", "needs_second_look"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProductionProfileThin {
    pub slug: String,
    pub name: String,
    pub qc_pack_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QcGateDto {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeQcFinding {
    pub item_id: Option<String>,
    pub rule_id: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeExtra {
    pub kind: String,
    pub severity: String,
    pub item_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WarningOverride {
    pub recorded_by: String,
    pub reason: String,
    pub rule_id: String,
    pub item_id: Option<String>,
    pub qc_run_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProducePageResponse {
    pub sets: Vec<ProductionSetThin>,
    pub default_count: u64,
    pub default_filter_json: String,
    pub qc_gate: QcGateDto,
    pub next_seq_hint: Option<u64>,
    pub produced_count: u64,
    pub profiles: Vec<ProductionProfileThin>,
    pub bates_prefix: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProduceQcRunResponse {
    pub ordered_ids: Vec<String>,
    pub pack_id: String,
    pub scope: String,
    pub findings: Vec<ChromeQcFinding>,
    pub extras: Vec<ChromeExtra>,
    pub error_count: u64,
    pub warn_count: u64,
    pub passed: bool,
    pub qc_run_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProduceStartResponse {
    pub ok: bool,
    pub blockers: Vec<ChromeExtra>,
    pub ordered_ids: Vec<String>,
    pub pack_id: String,
    pub scope: String,
    pub fail_if_withheld: bool,
    pub require_qc_pass: bool,
    pub produce_params: serde_json::Value,
    pub output_root: Option<String>,
    pub produced_count: u64,
    pub production_set_id: Option<String>,
    pub privilege_log_path: Option<String>,
}

pub struct ProduceQcRunArgs {
    pub root: String,
    pub filter_json: Option<String>,
    pub item_ids: Option<Vec<String>>,
    pub production_profile: Option<String>,
    pub source_entire_corpus: Option<bool>,
}

pub struct ProduceStartArgs {
    pub root: String,
    pub filter_json: Option<String>,
    pub item_ids: Option<Vec<String>>,
    pub production_profile: Option<String>,
    pub source_entire_corpus: Option<bool>,
    pub bates_prefix: Option<String>,
    pub bates_start: Option<u64>,
    pub warning_overrides: Option<Vec<WarningOverride>>,
    pub log_format: Option<String>,
    pub last_findings: Option<Vec<ChromeQcFinding>>,
}

fn default_produce_filter() -> FilterSpec {
    let mut spec = FilterSpec::preset_produce_responsive();
    spec.include_family = true;
    spec
}

fn entire_review_corpus_filter() -> FilterSpec {
    FilterSpec {
        include_family: true,
        conditions: vec![FilterCondition {
            field: "privilege_withhold".into(),
            op: "eq".into(),
            value: Some(serde_json::Value::Bool(false)),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    }
}

fn resolve_filter(
    filter_json: Option<&str>,
    source_entire_corpus: bool,
) -> Result<FilterSpec, CommandError> {
    if let Some(json) = filter_json.map(str::trim).filter(|s| !s.is_empty()) {
        return serde_json::from_str(json)
            .map_err(|e| CommandError::failed(format!("invalid filter_json: {e}")));
    }
    if source_entire_corpus {
        Ok(entire_review_corpus_filter())
    } else {
        Ok(default_produce_filter())
    }
}

fn effective_profile(profile: Option<&str>) -> String {
    profile
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_PROFILE)
        .to_string()
}

fn pack_for_profile(matter: &Matter, profile: &str) -> Result<String, CommandError> {
    let dummy = ProduceParams {
        production_profile: Some(profile.to_string()),
        bates_start: Some(1),
        fail_if_withheld: true,
        require_qc_pass: Some(true),
        ..ProduceParams::default()
    };
    let cfg =
        resolve_produce_config(matter, &dummy).map_err(|e| CommandError::failed(e.to_string()))?;
    Ok(effective_qc_pack_id(&cfg))
}

fn resolve_ordered_ids(
    matter: &Matter,
    spec: &FilterSpec,
    item_ids: Option<&[String]>,
) -> Result<Vec<String>, CommandError> {
    let resolved = matter.list_item_ids_filtered(spec).map_err(map_core)?;
    let raw = match item_ids {
        Some(ids) if !ids.is_empty() => {
            let allowed: HashSet<&str> = resolved.iter().map(String::as_str).collect();
            ids.iter()
                .filter(|id| allowed.contains(id.as_str()))
                .cloned()
                .collect()
        }
        _ => resolved,
    };
    matter.order_ids_family_together(&raw).map_err(map_core)
}

fn qc_gate_dto(block: Option<QcGateBlock>) -> QcGateDto {
    match block {
        None => QcGateDto {
            status: "Passed".into(),
            message: String::new(),
        },
        Some(QcGateBlock::Missing) => QcGateDto {
            status: "Missing".into(),
            message: QcGateBlock::Missing.message(),
        },
        Some(b @ QcGateBlock::Failed { .. }) => QcGateDto {
            status: "Failed".into(),
            message: b.message(),
        },
        Some(b @ QcGateBlock::Stale { .. }) => QcGateDto {
            status: "Stale".into(),
            message: b.message(),
        },
    }
}

fn extra_blocker(kind: &str, item_id: Option<String>, message: impl Into<String>) -> ChromeExtra {
    ChromeExtra {
        kind: kind.into(),
        severity: "blocker".into(),
        item_id,
        message: message.into(),
    }
}

fn uncoded_blockers(matter: &Matter, ids: &[String]) -> Result<Vec<ChromeExtra>, CommandError> {
    let codes = matter.list_item_codes(ids).map_err(map_core)?;
    let mut extras = Vec::new();
    for id in ids {
        let item_codes = codes.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let coded = item_codes.iter().any(|c| {
            RESPONSIVENESS_KEYS.contains(&c.key.as_str()) || c.group_key == "responsiveness"
        });
        if !coded {
            extras.push(extra_blocker(
                "uncoded_in_set",
                Some(id.clone()),
                "candidate lacks a responsiveness group code",
            ));
        }
    }
    Ok(extras)
}

fn withheld_in_scope_ids(matter: &Matter, spec: &FilterSpec) -> Result<Vec<String>, CommandError> {
    let mut withheld = FilterSpec::preset_withheld();
    withheld.scope = spec.scope.clone();
    withheld.include_family = false;
    matter.list_item_ids_filtered(&withheld).map_err(map_core)
}

fn privilege_log_union_ids(
    matter: &Matter,
    spec: &FilterSpec,
    candidates: &[String],
) -> Result<Vec<String>, CommandError> {
    let mut ids = candidates.to_vec();
    ids.extend(withheld_in_scope_ids(matter, spec)?);
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn privilege_log_blank_blocker(
    matter: &Matter,
    spec: &FilterSpec,
    candidates: &[String],
) -> Result<Option<ChromeExtra>, CommandError> {
    let protocol = matter.get_privilege_protocol().map_err(map_core)?;
    if protocol.description_required == 0 {
        return Ok(None);
    }
    let filter_ids = privilege_log_union_ids(matter, spec, candidates)?;
    let blank = matter
        .count_privilege_log_blank_descriptions(&spec.scope, Some(&filter_ids))
        .map_err(map_core)?;
    if blank > 0 {
        Ok(Some(extra_blocker(
            "privilege_log_blank",
            None,
            format!("privilege log has {blank} blank description(s); description_required"),
        )))
    } else {
        Ok(None)
    }
}

fn chrome_extras(
    matter: &Matter,
    spec: &FilterSpec,
    ids: &[String],
    gate: Option<&QcGateBlock>,
) -> Result<Vec<ChromeExtra>, CommandError> {
    let mut extras = Vec::new();
    if ids.is_empty() {
        extras.push(extra_blocker(
            "empty_selection",
            None,
            "produce set is empty",
        ));
        return Ok(extras);
    }
    extras.extend(uncoded_blockers(matter, ids)?);
    if let Some(blank) = privilege_log_blank_blocker(matter, spec, ids)? {
        extras.push(blank);
    }
    if let Some(block) = gate {
        extras.push(extra_blocker("qc_gate", None, block.message()));
    }
    Ok(extras)
}

fn finding_from_engine(f: &matter_qc::QcFinding) -> ChromeQcFinding {
    ChromeQcFinding {
        item_id: f.item_id.clone(),
        rule_id: f.rule_id.clone(),
        severity: f.severity.as_str().to_string(),
        message: f.message.clone(),
    }
}

const ORDERED_IDS_FILE: &str = "ordered_ids.json";

fn persist_ordered_ids(report_dir: &str, ids: &[String]) -> Result<(), CommandError> {
    let path = Utf8Path::new(report_dir).join(ORDERED_IDS_FILE);
    let bytes = serde_json::to_vec(ids)
        .map_err(|e| CommandError::failed(format!("QC ordered_ids json: {e}")))?;
    fs::write(path.as_std_path(), bytes)
        .map_err(|e| CommandError::failed(format!("QC ordered_ids write: {e}")))
}

fn load_ordered_ids(report_dir: &str) -> Result<Vec<String>, ChromeExtra> {
    let path = Utf8Path::new(report_dir).join(ORDERED_IDS_FILE);
    let bytes = fs::read(path.as_std_path())
        .map_err(|_| extra_blocker("qc_gate", None, "QC ordered_ids sidecar missing; re-run QC"))?;
    serde_json::from_slice(&bytes).map_err(|_| {
        extra_blocker(
            "qc_gate",
            None,
            "QC ordered_ids sidecar unreadable; re-run QC",
        )
    })
}

fn load_findings_csv(report_dir: &str) -> Result<Vec<ChromeQcFinding>, ChromeExtra> {
    let path = Utf8PathBuf::from(report_dir).join("findings.csv");
    let meta = fs::metadata(path.as_std_path()).map_err(|_| {
        extra_blocker(
            "qc_gate",
            None,
            "QC findings.csv missing or unreadable; re-run QC",
        )
    })?;
    if meta.len() == 0 {
        return Err(extra_blocker(
            "qc_gate",
            None,
            "QC findings.csv is empty; re-run QC",
        ));
    }
    let mut rdr = csv::Reader::from_path(path.as_std_path()).map_err(|_| {
        extra_blocker(
            "qc_gate",
            None,
            "QC findings.csv missing or unreadable; re-run QC",
        )
    })?;
    let headers = rdr.headers().map_err(|_| {
        extra_blocker(
            "qc_gate",
            None,
            "QC findings.csv header unreadable; re-run QC",
        )
    })?;
    if headers.get(0) != Some("rule_id")
        || headers.get(1) != Some("severity")
        || headers.get(2) != Some("item_id")
        || headers.get(3) != Some("message")
    {
        return Err(extra_blocker(
            "qc_gate",
            None,
            "QC findings.csv header is invalid; re-run QC",
        ));
    }
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|_| {
            extra_blocker("qc_gate", None, "QC findings.csv is malformed; re-run QC")
        })?;
        if rec.len() < 4 {
            return Err(extra_blocker(
                "qc_gate",
                None,
                "QC findings.csv is malformed; re-run QC",
            ));
        }
        let rule_id = rec.get(0).unwrap_or("").trim().to_string();
        if rule_id.is_empty() {
            return Err(extra_blocker(
                "qc_gate",
                None,
                "QC findings.csv has a blank rule_id; re-run QC",
            ));
        }
        let severity = rec.get(1).unwrap_or("").trim().to_string();
        if severity != "warn" && severity != "error" {
            return Err(extra_blocker(
                "qc_gate",
                None,
                "QC findings.csv has an unknown severity; re-run QC",
            ));
        }
        let item_id = rec
            .get(2)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let message = rec.get(3).unwrap_or("").to_string();
        out.push(ChromeQcFinding {
            item_id,
            rule_id,
            severity,
            message,
        });
    }
    Ok(out)
}

struct StoredQc {
    run_id: String,
    ordered_ids: Vec<String>,
    findings: Vec<ChromeQcFinding>,
}

fn load_stored_qc(matter: &Matter) -> Result<Result<StoredQc, ChromeExtra>, CommandError> {
    let Some(run) = matter
        .load_latest_qc_run_for_scope(Some(SCOPE_ITEM_IDS))
        .map_err(map_core)?
    else {
        return Ok(Err(extra_blocker(
            "qc_gate",
            None,
            "QC required: no production QC run found",
        )));
    };
    let Some(report) = run.report_path.as_deref() else {
        return Ok(Err(extra_blocker(
            "qc_gate",
            None,
            "QC report path missing; re-run QC",
        )));
    };
    let findings = match load_findings_csv(report) {
        Ok(f) => f,
        Err(e) => return Ok(Err(e)),
    };
    let warn_n = findings.iter().filter(|f| f.severity == "warn").count() as u64;
    let err_n = findings.iter().filter(|f| f.severity == "error").count() as u64;
    if warn_n != run.warn_count || err_n != run.error_count {
        return Ok(Err(extra_blocker(
            "qc_gate",
            None,
            "QC findings.csv does not match stored warn/error counts; re-run QC",
        )));
    }
    let ordered_ids = match load_ordered_ids(report) {
        Ok(ids) => ids,
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(StoredQc {
        run_id: run.id,
        ordered_ids,
        findings,
    }))
}

fn override_key(rule_id: &str, item_id: Option<&str>) -> String {
    match item_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => format!("{rule_id}\u{1f}{id}"),
        None => format!("{rule_id}\u{1f}*"),
    }
}

fn intended_produce_params(
    ordered: &[String],
    profile: &str,
    pack_id: &str,
    prefix: &str,
    bates_start: u64,
) -> ProduceParams {
    ProduceParams {
        scope: SCOPE_ITEM_IDS.into(),
        item_ids: ordered.to_vec(),
        bates_prefix: Some(prefix.to_string()),
        bates_start: Some(bates_start),
        production_profile: Some(profile.to_string()),
        qc_pack_id: Some(pack_id.to_string()),
        fail_if_withheld: true,
        require_qc_pass: Some(true),
        expand_family: Some(false),
        include_csv_twin: Some(true),
        export_eml_if_missing_native: Some(true),
        output_dir: None,
        ..ProduceParams::default()
    }
}

pub fn produce_page_blocking(root: &str) -> Result<ProducePageResponse, CommandError> {
    let matter = open_matter_read(root)?;
    let mut spec = default_produce_filter();
    spec.include_family = true;
    let default_filter_json = serde_json::to_string(&spec)
        .map_err(|e| CommandError::failed(format!("filter json: {e}")))?;
    let default_count = matter.count_items_filtered(&spec).map_err(map_core)?;
    let produced_count = matter.count_produced_items().map_err(map_core)?;
    let sets = matter.list_production_sets_thin().map_err(map_core)?;
    let next_seq_hint = sets
        .iter()
        .find(|s| s.bates_prefix == DEFAULT_BATES_PREFIX)
        .map(|s| s.next_seq);
    let pack_id = pack_for_profile(&matter, DEFAULT_PROFILE)?;
    let ids = matter.list_item_ids_filtered(&spec).map_err(map_core)?;
    let ordered = matter.order_ids_family_together(&ids).map_err(map_core)?;
    let gate =
        check_qc_gate_for_pack(&matter, SCOPE_ITEM_IDS, &ordered, &pack_id).map_err(map_core)?;
    let profiles = matter
        .list_production_profiles()
        .map_err(map_core)?
        .into_iter()
        .map(|p| ProductionProfileThin {
            slug: p.slug,
            name: p.label,
            qc_pack_id: matter_core::normalize_qc_pack_id(&p.body.qc.pack_id),
        })
        .collect();
    Ok(ProducePageResponse {
        sets,
        default_count,
        default_filter_json,
        qc_gate: qc_gate_dto(gate),
        next_seq_hint,
        produced_count,
        profiles,
        bates_prefix: DEFAULT_BATES_PREFIX.into(),
    })
}

pub fn produce_qc_run_blocking(
    args: ProduceQcRunArgs,
) -> Result<ProduceQcRunResponse, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let spec = resolve_filter(
        args.filter_json.as_deref(),
        args.source_entire_corpus.unwrap_or(false),
    )?;
    let profile = effective_profile(args.production_profile.as_deref());
    let pack_id = pack_for_profile(&matter, &profile)?;
    let ordered = resolve_ordered_ids(&matter, &spec, args.item_ids.as_deref())?;
    if ordered.is_empty() {
        let extras = chrome_extras(&matter, &spec, &ordered, None)?;
        return Ok(ProduceQcRunResponse {
            ordered_ids: ordered,
            pack_id,
            scope: SCOPE_ITEM_IDS.into(),
            findings: Vec::new(),
            extras,
            error_count: 0,
            warn_count: 0,
            passed: false,
            qc_run_id: String::new(),
        });
    }

    let qc_params = QcParams {
        scope: SCOPE_ITEM_IDS.into(),
        item_ids: ordered.clone(),
        expand_family_for_scan: false,
        pack_id: Some(pack_id.clone()),
        ..QcParams::default()
    };
    let job = matter.create_job("qc").map_err(map_core)?;
    let outcome = run_production_qc(&matter, &job.id, &qc_params, None, |_| {})
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let (findings, error_count, warn_count, passed, report_path, qc_run_id) = match outcome {
        QcOutcome::Succeeded(r) => (
            r.findings
                .iter()
                .map(finding_from_engine)
                .collect::<Vec<_>>(),
            r.error_count,
            r.warn_count,
            r.passed,
            r.report_path,
            r.qc_run_id,
        ),
        QcOutcome::Paused(s) => {
            return Err(CommandError::failed(format!(
                "QC paused after {} candidate(s)",
                s.completed_count
            )));
        }
        QcOutcome::Failed { message, .. } => {
            return Err(CommandError::failed(message));
        }
    };
    persist_ordered_ids(&report_path, &ordered)?;
    let gate =
        check_qc_gate_for_pack(&matter, SCOPE_ITEM_IDS, &ordered, &pack_id).map_err(map_core)?;
    let extras = chrome_extras(&matter, &spec, &ordered, gate.as_ref())?;
    Ok(ProduceQcRunResponse {
        ordered_ids: ordered,
        pack_id,
        scope: SCOPE_ITEM_IDS.into(),
        findings,
        extras,
        error_count,
        warn_count,
        passed,
        qc_run_id,
    })
}

fn validate_log_format(log_format: Option<&str>) -> Result<String, CommandError> {
    match log_format.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("standard") => Ok("standard".into()),
        Some("automated_metadata") => Ok("automated_metadata".into()),
        Some("category") => Err(CommandError::failed(
            "category privilege log is not implemented (D-0031-03)",
        )),
        Some(other) => Err(CommandError::failed(format!(
            "unknown privilege log format '{other}'"
        ))),
    }
}

pub fn produce_start_blocking(
    args: ProduceStartArgs,
) -> Result<ProduceStartResponse, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let spec = resolve_filter(
        args.filter_json.as_deref(),
        args.source_entire_corpus.unwrap_or(false),
    )?;
    let profile = effective_profile(args.production_profile.as_deref());
    let pack_id = pack_for_profile(&matter, &profile)?;
    let prefix = args
        .bates_prefix
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_BATES_PREFIX)
        .to_string();
    let bates_start = match args.bates_start {
        Some(n) if n >= 1 => n,
        _ => {
            return Err(CommandError::failed(
                "bates_start is required and must be >= 1",
            ));
        }
    };
    let log_format = validate_log_format(args.log_format.as_deref())?;
    // UI may send last_findings as a cache; the warning/error gate always
    // loads the stored QC report so an empty IPC vec cannot skip overrides.
    let _ui_findings_cache = args.last_findings;
    let ordered = resolve_ordered_ids(&matter, &spec, args.item_ids.as_deref())?;
    let produce_params =
        intended_produce_params(&ordered, &profile, &pack_id, &prefix, bates_start);
    let produce_params_json = serde_json::to_value(&produce_params).unwrap_or_else(|_| json!({}));

    let blocked = |blockers: Vec<ChromeExtra>| ProduceStartResponse {
        ok: false,
        blockers,
        ordered_ids: ordered.clone(),
        pack_id: pack_id.clone(),
        scope: SCOPE_ITEM_IDS.into(),
        fail_if_withheld: true,
        require_qc_pass: true,
        produce_params: produce_params_json.clone(),
        output_root: None,
        produced_count: 0,
        production_set_id: None,
        privilege_log_path: None,
    };

    if ordered.is_empty() {
        return Ok(blocked(vec![extra_blocker(
            "empty_selection",
            None,
            "produce set is empty",
        )]));
    }

    let gate =
        check_qc_gate_for_pack(&matter, SCOPE_ITEM_IDS, &ordered, &pack_id).map_err(map_core)?;
    if let Some(block) = gate.as_ref() {
        return Ok(blocked(vec![extra_blocker(
            "qc_gate",
            None,
            block.message(),
        )]));
    }

    let extras = chrome_extras(&matter, &spec, &ordered, None)?;
    let mut blockers: Vec<ChromeExtra> = extras
        .into_iter()
        .filter(|e| e.severity == "blocker")
        .collect();

    let stored = match load_stored_qc(&matter)? {
        Ok(s) => s,
        Err(e) => {
            blockers.push(e);
            return Ok(blocked(blockers));
        }
    };
    let resolved_set: HashSet<&str> = ordered.iter().map(String::as_str).collect();
    let stored_set: HashSet<&str> = stored.ordered_ids.iter().map(String::as_str).collect();
    if resolved_set != stored_set {
        blockers.push(extra_blocker(
            "qc_gate",
            None,
            "QC stale: selection changed since last QC; re-run QC",
        ));
        return Ok(blocked(blockers));
    }
    let stored_run_id = stored.run_id.clone();
    let ordered = stored.ordered_ids.clone();
    let produce_params =
        intended_produce_params(&ordered, &profile, &pack_id, &prefix, bates_start);
    let findings = stored.findings;
    for f in &findings {
        if f.severity == QcSeverity::Error.as_str() {
            blockers.push(extra_blocker(
                &f.rule_id,
                f.item_id.clone(),
                f.message.clone(),
            ));
        }
    }

    let warns: Vec<&ChromeQcFinding> = findings
        .iter()
        .filter(|f| f.severity == QcSeverity::Warn.as_str())
        .collect();
    let overrides = args.warning_overrides.unwrap_or_default();
    let mut override_map: HashMap<String, WarningOverride> = HashMap::new();
    for ov in overrides {
        if ov.recorded_by.trim().is_empty() || ov.reason.trim().is_empty() {
            blockers.push(extra_blocker(
                "warning_override",
                ov.item_id.clone(),
                "warning override requires non-empty recorded_by and reason",
            ));
            continue;
        }
        if ov.qc_run_id.trim() != stored_run_id {
            blockers.push(extra_blocker(
                "warning_override",
                ov.item_id.clone(),
                "warning override is not for the current QC run; re-record after Re-run",
            ));
            continue;
        }
        override_map.insert(override_key(&ov.rule_id, ov.item_id.as_deref()), ov);
    }
    for w in &warns {
        let key = override_key(&w.rule_id, w.item_id.as_deref());
        if !override_map.contains_key(&key) {
            blockers.push(extra_blocker(
                "warning_override",
                w.item_id.clone(),
                format!("warning {} requires recorded_by + reason", w.rule_id),
            ));
        }
    }

    if !blockers.is_empty() {
        return Ok(blocked(blockers));
    }

    for w in &warns {
        let key = override_key(&w.rule_id, w.item_id.as_deref());
        let Some(ov) = override_map.get(&key) else {
            continue;
        };
        matter
            .append_audit(matter_core::AuditEventInput {
                actor: ACTOR.into(),
                action: "produce.warning_override".into(),
                entity: match w.item_id.as_deref() {
                    Some(id) => format!("item:{id}"),
                    None => "produce:set".into(),
                },
                params_json: json!({
                    "rule_id": ov.rule_id,
                    "item_id": ov.item_id,
                    "recorded_by": ov.recorded_by,
                    "reason": ov.reason,
                    "qc_run_id": ov.qc_run_id,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            })
            .map_err(map_core)?;
    }

    let job = matter.create_job("produce").map_err(map_core)?;
    let outcome = run_produce(&matter, &job.id, &produce_params, None, |_| {})
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let summary = match outcome {
        ProduceOutcome::Succeeded(s) => s,
        ProduceOutcome::Paused(s) => {
            return Err(CommandError::failed(format!(
                "produce paused after {} item(s)",
                s.completed_count
            )));
        }
        ProduceOutcome::Failed { message, .. } => {
            return Err(CommandError::failed(message));
        }
    };

    let mut control_numbers: HashMap<String, String> = HashMap::new();
    let mut produced_ids: Vec<String> = Vec::new();
    for id in &ordered {
        if let Some(cn) = matter.latest_control_number(id).map_err(map_core)? {
            control_numbers.insert(id.clone(), cn);
            produced_ids.push(id.clone());
        }
    }
    let mut filter_ids = produced_ids;
    filter_ids.extend(withheld_in_scope_ids(&matter, &spec)?);
    filter_ids.sort();
    filter_ids.dedup();

    let proto = matter.get_privilege_protocol().map_err(map_core)?;
    matter
        .upsert_privilege_protocol(UpsertPrivilegeProtocolInput {
            log_format,
            fre_502d_note: proto.fre_502d_note,
            fre_502e_note: proto.fre_502e_note,
            description_required: proto.description_required != 0,
            actor: ACTOR.into(),
        })
        .map_err(map_core)?;

    let log_path = Utf8Path::new(&summary.output_root).join("privilege-log.csv");
    matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: if spec.scope == SCOPE_REVIEW_CORPUS {
                SCOPE_REVIEW_CORPUS.into()
            } else {
                spec.scope.clone()
            },
            path: log_path.clone(),
            filter_ids: Some(filter_ids),
            control_numbers: Some(control_numbers),
        })
        .map_err(map_core)?;

    Ok(ProduceStartResponse {
        ok: true,
        blockers: Vec::new(),
        ordered_ids: ordered,
        pack_id,
        scope: SCOPE_ITEM_IDS.into(),
        fail_if_withheld: true,
        require_qc_pass: true,
        produce_params: produce_params_json,
        output_root: Some(summary.output_root),
        produced_count: summary.produced_count,
        production_set_id: Some(summary.production_set_id),
        privilege_log_path: Some(log_path.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use crate::document::{review_document_blocking, ReviewDocumentArgs};
    use crate::matter_cmd::matter_overview_blocking;
    use matter_core::{
        is_encrypted_matter, item_role, item_status, ApplyCodesInput, ItemInput, ItemUpdate,
        Matter, UpsertItemPrivilegeInput, DEFAULT_REVIEW_SET_NAME,
    };
    use matter_qc::RULE_WITHHELD_IN_SELECTION;
    use std::fs;
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    fn tiny_eml() -> &'static [u8] {
        b"From: a@example.com\r\nTo: b@example.com\r\nSubject: t\r\n\r\nbody\r\n"
    }

    fn catalog_id(matter: &Matter, key: &str) -> String {
        matter
            .list_code_definitions()
            .expect("defs")
            .into_iter()
            .find(|d| d.key == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .id
    }

    fn apply_responsive(matter: &Matter, ids: &[&str]) {
        let resp = catalog_id(matter, "responsive");
        matter
            .apply_codes(ApplyCodesInput {
                item_ids: ids.iter().map(|s| (*s).to_string()).collect(),
                add_code_ids: vec![resp],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: "chrome".into(),
                expected_version: None,
            })
            .expect("apply");
    }

    fn put_native_text(matter: &Matter, id: &str) {
        let n = matter.put_bytes(tiny_eml()).expect("native");
        let t = matter.put_bytes(b"text body").expect("text");
        matter
            .update_item(
                id,
                ItemUpdate {
                    native_sha256: Some(Some(n)),
                    text_sha256: Some(Some(t)),
                    mime_type: Some(Some("message/rfc822".into())),
                    file_category: Some(Some("email".into())),
                    size_bytes: Some(Some(tiny_eml().len() as i64)),
                    ..Default::default()
                },
            )
            .expect("cas");
    }

    fn seed_family_three(root: &Utf8Path) {
        let matter = Matter::open(root).expect("open");
        let family = matter.insert_family("").expect("family");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0000".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                family_id: Some(family.id.clone()),
                subject: Some("Parent".into()),
                from_addr: Some("a@example.com".into()),
                mime_type: Some("message/rfc822".into()),
                file_category: Some("email".into()),
                path: Some("parent.eml".into()),
                ..Default::default()
            })
            .expect("parent");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0001".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                family_id: Some(family.id.clone()),
                parent_item_id: Some("itm_0000".into()),
                subject: Some("Child A".into()),
                mime_type: Some("message/rfc822".into()),
                file_category: Some("email".into()),
                path: Some("child-a.eml".into()),
                ..Default::default()
            })
            .expect("c1");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0002".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                family_id: Some(family.id),
                parent_item_id: Some("itm_0000".into()),
                subject: Some("Child B".into()),
                mime_type: Some("message/rfc822".into()),
                file_category: Some("email".into()),
                path: Some("child-b.eml".into()),
                ..Default::default()
            })
            .expect("c2");
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        for (i, id) in ["itm_0000", "itm_0001", "itm_0002"].iter().enumerate() {
            matter
                .update_item(
                    id,
                    ItemUpdate {
                        in_review: Some(Some(1)),
                        review_set_id: Some(Some(set.id.clone())),
                        review_order: Some(Some(i as i64)),
                        ..Default::default()
                    },
                )
                .expect("promote");
            put_native_text(&matter, id);
        }
        matter.seed_default_codes().expect("seed");
        apply_responsive(&matter, &["itm_0000"]);
        matter
            .upsert_item_privilege(UpsertItemPrivilegeInput {
                item_id: "itm_0001".into(),
                basis: "attorney_client".into(),
                description: "privileged attachment".into(),
                status: "asserted".into(),
                withhold: true,
                include_on_log: true,
                actor: "chrome".into(),
                expected_version: None,
            })
            .expect("withhold A");
    }

    fn qc_args(root: &str, ids: Option<Vec<String>>) -> ProduceQcRunArgs {
        ProduceQcRunArgs {
            root: root.to_string(),
            filter_json: None,
            item_ids: ids,
            production_profile: None,
            source_entire_corpus: None,
        }
    }

    fn start_args(
        root: &str,
        ids: Option<Vec<String>>,
        overrides: Option<Vec<WarningOverride>>,
        last_findings: Option<Vec<ChromeQcFinding>>,
        bates_start: Option<u64>,
    ) -> ProduceStartArgs {
        ProduceStartArgs {
            root: root.to_string(),
            filter_json: None,
            item_ids: ids,
            production_profile: None,
            source_entire_corpus: None,
            bates_prefix: Some("PROD".into()),
            bates_start,
            warning_overrides: overrides,
            log_format: Some("standard".into()),
            last_findings,
        }
    }

    #[test]
    fn dod2_default_set_privilege_in_set_and_uncoded() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Dod2").expect("create");
        seed_family_three(&root);

        let page = produce_page_blocking(root.as_str()).expect("page");
        assert_eq!(
            page.default_count, 3,
            "include_family pulls withheld child into the default set"
        );
        let spec: FilterSpec = serde_json::from_str(&page.default_filter_json).expect("json");
        assert!(spec.include_family);
        assert_eq!(spec.scope, SCOPE_REVIEW_CORPUS);

        let qc = produce_qc_run_blocking(qc_args(root.as_str(), None)).expect("qc");
        assert_eq!(qc.scope, "item_ids");
        assert_eq!(
            qc.pack_id,
            page.profiles
                .iter()
                .find(|p| p.slug == DEFAULT_PROFILE)
                .map(|p| p.qc_pack_id.as_str())
                .unwrap_or("qc_default_v1")
        );
        assert!(
            qc.findings.iter().any(|f| {
                f.rule_id == RULE_WITHHELD_IN_SELECTION
                    && f.severity == "error"
                    && f.item_id.as_deref() == Some("itm_0001")
            }),
            "withheld_in_selection error: {:?}",
            qc.findings
        );
        assert!(
            qc.extras.iter().any(|e| {
                e.kind == "uncoded_in_set" && e.item_id.as_deref() == Some("itm_0002")
            }),
            "uncoded_in_set child B: {:?}",
            qc.extras
        );

        let start = produce_start_blocking(start_args(
            root.as_str(),
            None,
            None,
            Some(qc.findings.clone()),
            Some(1),
        ))
        .expect("start blocked");
        assert!(!start.ok, "produce must fail while child A is withheld");
        assert!(start.fail_if_withheld);
        assert_eq!(start.produce_params["fail_if_withheld"], true);
        assert_eq!(start.scope, "item_ids");
        assert_eq!(start.pack_id, qc.pack_id);

        {
            let matter = Matter::open(&root).expect("open");
            apply_responsive(&matter, &["itm_0001", "itm_0002"]);
            matter
                .upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: "itm_0001".into(),
                    basis: "attorney_client".into(),
                    description: "cleared".into(),
                    status: "cleared".into(),
                    withhold: false,
                    include_on_log: false,
                    actor: "chrome".into(),
                    expected_version: None,
                })
                .expect("clear withhold");
        }
        let qc2 = produce_qc_run_blocking(qc_args(root.as_str(), None)).expect("qc2");
        assert!(qc2.passed, "engine passed after coding: {:?}", qc2.findings);
        assert!(
            !qc2.extras.iter().any(|e| e.kind == "uncoded_in_set"),
            "uncoded gone: {:?}",
            qc2.extras
        );
        let overrides: Vec<WarningOverride> = qc2
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "accepted family warning".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc2.qc_run_id.clone(),
            })
            .collect();
        let start2 = produce_start_blocking(start_args(
            root.as_str(),
            None,
            Some(overrides),
            Some(qc2.findings.clone()),
            Some(1),
        ))
        .expect("start2");
        assert!(start2.ok, "start after coding: {:?}", start2.blockers);
        assert!(start2.fail_if_withheld);
        assert_eq!(start2.produce_params["fail_if_withheld"], true);
        assert_eq!(start2.pack_id, qc2.pack_id);
    }

    #[test]
    fn dod3_warning_override_payload_and_stale_gate() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Dod3").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            for (id, order) in [("itm_w0", 0), ("itm_w1", 1)] {
                matter
                    .insert_item(ItemInput {
                        id: Some(id.into()),
                        status: item_status::EXTRACTED.into(),
                        role: Some(item_role::STANDALONE.into()),
                        subject: Some(id.into()),
                        from_addr: Some("a@example.com".into()),
                        mime_type: Some("message/rfc822".into()),
                        file_category: Some("email".into()),
                        in_review: Some(1),
                        review_set_id: Some(set.id.clone()),
                        review_order: Some(order),
                        size_bytes: Some(0),
                        path: Some(format!("{id}.eml")),
                        ..Default::default()
                    })
                    .expect("item");
                put_native_text(&matter, id);
                matter
                    .update_item(
                        id,
                        ItemUpdate {
                            size_bytes: Some(Some(0)),
                            ..Default::default()
                        },
                    )
                    .expect("zero");
            }
            apply_responsive(&matter, &["itm_w0", "itm_w1"]);
        }

        let empty = produce_qc_run_blocking(qc_args(
            root.as_str(),
            Some(vec!["itm_does_not_exist".into()]),
        ))
        .expect("empty qc");
        assert!(
            empty.extras.iter().any(|e| e.kind == "empty_selection"),
            "{:?}",
            empty.extras
        );
        let empty_start = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_does_not_exist".into()]),
            None,
            None,
            Some(1),
        ))
        .expect("empty start");
        assert!(!empty_start.ok);
        assert!(empty_start
            .blockers
            .iter()
            .any(|e| e.kind == "empty_selection"));

        let qc = produce_qc_run_blocking(qc_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
        ))
        .expect("qc");
        assert!(qc.passed, "warn-only should pass engine: {:?}", qc.findings);
        assert!(
            qc.findings.iter().any(|f| f.severity == "warn"),
            "need warn findings: {:?}",
            qc.findings
        );

        let no_ov = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
            None,
            Some(qc.findings.clone()),
            Some(1),
        ))
        .expect("no ov");
        assert!(!no_ov.ok);

        let empty_reason = qc
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "  ".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc.qc_run_id.clone(),
            })
            .collect();
        let blank = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
            Some(empty_reason),
            Some(qc.findings.clone()),
            Some(1),
        ))
        .expect("blank reason");
        assert!(!blank.ok);

        let only_one: Vec<WarningOverride> = qc
            .findings
            .iter()
            .filter(|f| f.severity == "warn" && f.item_id.as_deref() == Some("itm_w0"))
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "ok for w0".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc.qc_run_id.clone(),
            })
            .collect();
        let partial = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
            Some(only_one),
            Some(qc.findings.clone()),
            Some(1),
        ))
        .expect("partial");
        assert!(!partial.ok, "old override must not cover new item warning");

        let err_findings = vec![ChromeQcFinding {
            item_id: Some("itm_w0".into()),
            rule_id: RULE_WITHHELD_IN_SELECTION.into(),
            severity: "error".into(),
            message: "withheld item in selection".into(),
        }];
        let with_reason_on_error = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
            Some(vec![WarningOverride {
                recorded_by: "counsel".into(),
                reason: "override error".into(),
                rule_id: RULE_WITHHELD_IN_SELECTION.into(),
                item_id: Some("itm_w0".into()),
                qc_run_id: qc.qc_run_id.clone(),
            }]),
            Some(err_findings),
            Some(1),
        ))
        .expect("error not overridable");
        assert!(!with_reason_on_error.ok);

        let qc_one = produce_qc_run_blocking(qc_args(root.as_str(), Some(vec!["itm_w0".into()])))
            .expect("qc one");
        let stale = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
            None,
            Some(qc_one.findings.clone()),
            Some(1),
        ))
        .expect("stale");
        assert!(!stale.ok);
        assert!(
            stale.blockers.iter().any(|e| e.kind == "qc_gate"),
            "membership drift is stale, never silent re-QC: {:?}",
            stale.blockers
        );

        let qc_again = produce_qc_run_blocking(qc_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
        ))
        .expect("qc again");
        let all_ov: Vec<WarningOverride> = qc_again
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "accepted zero-size".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc_again.qc_run_id.clone(),
            })
            .collect();
        let ok = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_w0".into(), "itm_w1".into()]),
            Some(all_ov),
            Some(qc_again.findings.clone()),
            Some(1),
        ))
        .expect("ok");
        assert!(ok.ok, "{:?}", ok.blockers);
    }

    #[test]
    fn dod4_volume_bates_privilege_log_and_chip() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Dod4").expect("create");
        let (fam_first, fam_second) = {
            let matter = Matter::open(&root).expect("open");
            let fa = matter.insert_family("").expect("fa");
            let fb = matter.insert_family("").expect("fb");
            // First-seen family should be the lexicographically *later* id.
            let (first, second) = if fa.id > fb.id {
                (fa.id, fb.id)
            } else {
                (fb.id, fa.id)
            };
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            let rows = [
                ("itm_p1", &first, None, 0, item_role::PARENT),
                ("itm_c1", &first, Some("itm_p1"), 1, item_role::ATTACHMENT),
                ("itm_p2", &second, None, 2, item_role::PARENT),
                ("itm_c2", &second, Some("itm_p2"), 3, item_role::ATTACHMENT),
            ];
            for (id, fam, parent_id, order, role) in rows {
                matter
                    .insert_item(ItemInput {
                        id: Some(id.into()),
                        status: item_status::EXTRACTED.into(),
                        role: Some(role.into()),
                        family_id: Some(fam.clone()),
                        parent_item_id: parent_id.map(|s| s.to_string()),
                        subject: Some(id.into()),
                        from_addr: Some("a@example.com".into()),
                        mime_type: Some("message/rfc822".into()),
                        file_category: Some("email".into()),
                        in_review: Some(1),
                        review_set_id: Some(set.id.clone()),
                        review_order: Some(order),
                        path: Some(format!("{id}.eml")),
                        ..Default::default()
                    })
                    .expect("item");
                put_native_text(&matter, id);
            }
            matter
                .insert_item(ItemInput {
                    id: Some("itm_hold".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("WithheldOut".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id),
                    review_order: Some(9),
                    mime_type: Some("message/rfc822".into()),
                    file_category: Some("email".into()),
                    path: Some("hold.eml".into()),
                    ..Default::default()
                })
                .expect("hold");
            apply_responsive(&matter, &["itm_p1", "itm_c1", "itm_p2", "itm_c2"]);
            matter
                .upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: "itm_p1".into(),
                    basis: "attorney_client".into(),
                    description: "produced but logged".into(),
                    status: "asserted".into(),
                    withhold: false,
                    include_on_log: true,
                    actor: "chrome".into(),
                    expected_version: None,
                })
                .expect("log parent");
            matter
                .upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: "itm_hold".into(),
                    basis: "attorney_client".into(),
                    description: "withheld in corpus".into(),
                    status: "asserted".into(),
                    withhold: true,
                    include_on_log: true,
                    actor: "chrome".into(),
                    expected_version: None,
                })
                .expect("hold");
            (first, second)
        };
        let _ = (fam_first, fam_second);

        let qc = produce_qc_run_blocking(qc_args(root.as_str(), None)).expect("qc");
        assert!(qc.passed, "{:?}", qc.findings);
        let overrides: Vec<WarningOverride> = qc
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "accepted".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc.qc_run_id.clone(),
            })
            .collect();
        let start = produce_start_blocking(start_args(
            root.as_str(),
            None,
            Some(overrides),
            Some(qc.findings.clone()),
            Some(1),
        ))
        .expect("start");
        assert!(start.ok, "{:?}", start.blockers);
        let vol = start.output_root.clone().expect("vol");
        let dat_path = Utf8Path::new(&vol).join("DATA").join("load.dat");
        let dat = fs::read(dat_path.as_std_path()).expect("dat");
        assert!(dat.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 BOM");
        let text = String::from_utf8(dat[3..].to_vec()).expect("utf8");
        assert!(text.contains("BEGBATES"));
        assert!(Utf8Path::new(&vol).join("NATIVES").as_std_path().is_dir());
        assert!(Utf8Path::new(&vol).join("TEXT").as_std_path().is_dir());
        assert!(Utf8Path::new(&vol)
            .join("privilege-log.csv")
            .as_std_path()
            .is_file());
        assert!(!Utf8Path::new(&vol).join("IMAGES").as_std_path().exists());
        assert!(!Utf8Path::new(&vol).join("IMAGE.opt").as_std_path().exists());

        let q = matter_produce::DAT_QUALIFIER;
        let sep = matter_produce::DAT_SEPARATOR;
        let header = text.lines().next().expect("hdr");
        let cols: Vec<_> = header.split(sep).map(|c| c.trim_matches(q)).collect();
        let beg = cols.iter().position(|c| *c == "BEGBATES").expect("beg");
        let end = cols.iter().position(|c| *c == "ENDBATES").expect("end");
        let ctl = cols
            .iter()
            .position(|c| *c == "CONTROL_NUMBER")
            .expect("ctl");
        let item_col = cols.iter().position(|c| *c == "ITEM_ID").expect("item");
        let mut bates_by_item = HashMap::new();
        for line in text.lines().skip(1).filter(|l| !l.is_empty()) {
            let fields: Vec<_> = line
                .split(sep)
                .map(|c| c.trim_matches(q).to_string())
                .collect();
            assert_eq!(fields[beg], fields[end]);
            assert_eq!(fields[beg], fields[ctl]);
            bates_by_item.insert(fields[item_col].clone(), fields[ctl].clone());
        }
        let p1 = bates_by_item.get("itm_p1").expect("p1 bates");
        let c1 = bates_by_item.get("itm_c1").expect("c1 bates");
        let p2 = bates_by_item.get("itm_p2").expect("p2 bates");
        assert!(p1 < c1, "parent control < child control: {p1} vs {c1}");
        assert!(
            p1 < p2,
            "first-seen family keeps lower Bates than later family: {p1} vs {p2}"
        );

        let log = fs::read_to_string(Utf8Path::new(&vol).join("privilege-log.csv").as_std_path())
            .expect("log");
        assert!(
            log.contains(p1),
            "produced privilege-log ControlNumber is Bates: {log}"
        );
        assert!(
            log.contains("itm_hold"),
            "withheld-in-scope row stays item_id: {log}"
        );
        assert!(
            !log.lines()
                .any(|l| l.contains("itm_hold") && l.contains("PROD")),
            "withheld row must not get Bates: {log}"
        );

        let doc = review_document_blocking(ReviewDocumentArgs {
            root: root.to_string(),
            item_id: "itm_p1".into(),
            filter_json: None,
            keyword: None,
        })
        .expect("doc");
        assert_eq!(doc.bates, *p1);
        assert_ne!(doc.bates_note, "0113");
        let ov = matter_overview_blocking(root.as_str()).expect("ov");
        assert!(ov.produced >= 1);
    }

    #[test]
    fn dod5_encrypted_produce_page() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = parent.join("EncProd");
        {
            let _m =
                Matter::create_encrypted(&root, "EncProd", "test-passphrase-0113").expect("enc");
        }
        assert!(is_encrypted_matter(&root));
        let err = produce_page_blocking(root.as_str()).expect_err("encrypted");
        assert_eq!(err.kind, "encrypted");
    }

    #[test]
    fn missing_findings_csv_blocks_produce() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "MissingCsv").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_m".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("m".into()),
                    from_addr: Some("a@example.com".into()),
                    mime_type: Some("message/rfc822".into()),
                    file_category: Some("email".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id),
                    review_order: Some(0),
                    size_bytes: Some(0),
                    path: Some("m.eml".into()),
                    ..Default::default()
                })
                .expect("item");
            put_native_text(&matter, "itm_m");
            matter
                .update_item(
                    "itm_m",
                    ItemUpdate {
                        size_bytes: Some(Some(0)),
                        ..Default::default()
                    },
                )
                .expect("zero");
            apply_responsive(&matter, &["itm_m"]);
        }
        let qc = produce_qc_run_blocking(qc_args(root.as_str(), Some(vec!["itm_m".into()])))
            .expect("qc");
        assert!(qc.passed, "{:?}", qc.findings);
        {
            let matter = Matter::open_for_read(&root).expect("read");
            let run = matter
                .load_latest_qc_run_for_scope(Some("item_ids"))
                .expect("run")
                .expect("some");
            let path =
                Utf8Path::new(run.report_path.as_deref().expect("report")).join("findings.csv");
            fs::remove_file(path.as_std_path()).expect("unlink");
        }
        let ov: Vec<WarningOverride> = qc
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "accepted".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc.qc_run_id.clone(),
            })
            .collect();
        let start = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_m".into()]),
            Some(ov),
            None,
            Some(1),
        ))
        .expect("start");
        assert!(!start.ok);
        assert!(
            start.blockers.iter().any(|e| e.kind == "qc_gate"),
            "{:?}",
            start.blockers
        );
    }

    #[test]
    fn empty_findings_csv_blocks_produce() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "EmptyCsv").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_e".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("e".into()),
                    from_addr: Some("a@example.com".into()),
                    mime_type: Some("message/rfc822".into()),
                    file_category: Some("email".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id),
                    review_order: Some(0),
                    path: Some("e.eml".into()),
                    ..Default::default()
                })
                .expect("item");
            put_native_text(&matter, "itm_e");
            apply_responsive(&matter, &["itm_e"]);
        }
        let qc = produce_qc_run_blocking(qc_args(root.as_str(), Some(vec!["itm_e".into()])))
            .expect("qc");
        assert!(qc.passed, "{:?}", qc.findings);
        {
            let matter = Matter::open_for_read(&root).expect("read");
            let run = matter
                .load_latest_qc_run_for_scope(Some("item_ids"))
                .expect("run")
                .expect("some");
            let path =
                Utf8Path::new(run.report_path.as_deref().expect("report")).join("findings.csv");
            fs::write(path.as_std_path(), b"").expect("truncate");
        }
        let ov: Vec<WarningOverride> = qc
            .findings
            .iter()
            .filter(|f| f.severity == "warn")
            .map(|f| WarningOverride {
                recorded_by: "counsel".into(),
                reason: "accepted".into(),
                rule_id: f.rule_id.clone(),
                item_id: f.item_id.clone(),
                qc_run_id: qc.qc_run_id.clone(),
            })
            .collect();
        let start = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_e".into()]),
            Some(ov),
            None,
            Some(1),
        ))
        .expect("start");
        assert!(!start.ok);
        assert!(
            start.blockers.iter().any(|e| e.kind == "qc_gate"),
            "{:?}",
            start.blockers
        );
    }

    #[test]
    fn last_findings_empty_does_not_skip_warning_overrides() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "LastFind").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_z".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("zero".into()),
                    from_addr: Some("a@example.com".into()),
                    mime_type: Some("message/rfc822".into()),
                    file_category: Some("email".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id),
                    review_order: Some(0),
                    size_bytes: Some(0),
                    path: Some("z.eml".into()),
                    ..Default::default()
                })
                .expect("item");
            put_native_text(&matter, "itm_z");
            matter
                .update_item(
                    "itm_z",
                    ItemUpdate {
                        size_bytes: Some(Some(0)),
                        ..Default::default()
                    },
                )
                .expect("zero");
            apply_responsive(&matter, &["itm_z"]);
        }
        let qc = produce_qc_run_blocking(qc_args(root.as_str(), Some(vec!["itm_z".into()])))
            .expect("qc");
        assert!(qc.passed, "{:?}", qc.findings);
        assert!(
            qc.findings.iter().any(|f| f.severity == "warn"),
            "{:?}",
            qc.findings
        );
        let start = produce_start_blocking(start_args(
            root.as_str(),
            Some(vec!["itm_z".into()]),
            None,
            Some(Vec::new()),
            Some(1),
        ))
        .expect("start");
        assert!(!start.ok, "empty last_findings must not skip warning gate");
        assert!(
            start.blockers.iter().any(|e| e.kind == "warning_override"),
            "{:?}",
            start.blockers
        );
    }

    #[test]
    fn qc_run_does_not_audit_privilege_log_export() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "NoLogAudit").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_blank".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("blank".into()),
                    from_addr: Some("a@example.com".into()),
                    mime_type: Some("message/rfc822".into()),
                    file_category: Some("email".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id),
                    review_order: Some(0),
                    path: Some("blank.eml".into()),
                    ..Default::default()
                })
                .expect("item");
            put_native_text(&matter, "itm_blank");
            apply_responsive(&matter, &["itm_blank"]);
            matter
                .upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: "itm_blank".into(),
                    basis: "attorney_client".into(),
                    description: String::new(),
                    status: "asserted".into(),
                    withhold: false,
                    include_on_log: true,
                    actor: "chrome".into(),
                    expected_version: None,
                })
                .expect("blank priv");
        }
        let qc = produce_qc_run_blocking(qc_args(root.as_str(), Some(vec!["itm_blank".into()])))
            .expect("qc");
        assert!(
            qc.extras.iter().any(|e| e.kind == "privilege_log_blank"),
            "{:?}",
            qc.extras
        );
        let matter = Matter::open_for_read(&root).expect("read");
        let n: i64 = matter
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE action = 'privilege.log_export'",
                [],
                |row| row.get(0),
            )
            .expect("audit count");
        assert_eq!(n, 0, "QC extras must not write a privilege-log export");
    }

    #[test]
    fn bates_start_required_no_silent_one() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "BatesReq").expect("create");
        let err = produce_start_blocking(start_args(root.as_str(), None, None, None, None))
            .expect_err("missing start");
        assert_eq!(err.kind, "failed");
        let err0 = produce_start_blocking(start_args(root.as_str(), None, None, None, Some(0)))
            .expect_err("zero");
        assert_eq!(err0.kind, "failed");
    }

    #[test]
    fn allow_permission_files_exist() {
        let page = include_str!("../permissions/autogenerated/produce_page.toml");
        let qc = include_str!("../permissions/autogenerated/produce_qc_run.toml");
        let start = include_str!("../permissions/autogenerated/produce_start.toml");
        assert!(page.contains("allow-produce-page"));
        assert!(qc.contains("allow-produce-qc-run"));
        assert!(start.contains("allow-produce-start"));
        assert!(page.contains("deny-produce-page"));
    }
}
