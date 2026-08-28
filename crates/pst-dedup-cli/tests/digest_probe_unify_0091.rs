//! Track 0091 — digest + probe unify (record-don't-tee).
//!
//! Proves: Pass-2 skips second stream when Pass-1 Real digest seeded Full/ok;
//! tallies match empty-seed baseline; single-feature paths unchanged.

use std::fs;

use dedup_engine::integrity::ScanMode;
use dedup_engine::keepset::{FamilyPolicy, KeepPolicy};
use dedup_engine::GroupingContext;
use pst_dedup_cli::attach_probe::{
    probe_keep_set_groups, probe_scan_items, AttachProbeSummary, KeepSetProbeOpts, ProbeBudgets,
    ProbeLevel, ProbeResultCache,
};
use pst_dedup_cli::grouping_cli::parse_identity_level;
use pst_dedup_cli::scan::{run_scan, ScanOptions};
use pst_dedup_cli::unique_export_report::{AttachLedgerMode, LedgerPathMode};
use pst_writer::{write_unicode_pst, WriteAttachment, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn msg_with_attach(mid: &str, payload: Vec<u8>) -> WriteMessage {
    let mut msg = WriteMessage {
        message_id: Some(mid.into()),
        subject: "DigestProbe".into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some("body text for 0091".into()),
        source_folder_path: Some("Inbox".into()),
        submit_time: Some(100),
        ..WriteMessage::default()
    };
    msg.attachments = vec![WriteAttachment {
        filename: "blob.bin".into(),
        mime: Some("application/octet-stream".into()),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload),
        stream_available: true,
        ..WriteAttachment::default()
    }];
    msg
}

fn fixture_pst(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.path().join("digest_probe_0091.pst");
    // Payload larger than typical Head per-attach so charge-cap vs full digest differs.
    let payload = vec![0x5Au8; 8 * 1024];
    write_unicode_pst(
        &path,
        vec![msg_with_attach("<0091@ex.com>", payload)],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write fixture");
    path
}

fn attach_scan_opts() -> ScanOptions {
    let mut opts = ScanOptions {
        retain_candidates: true,
        deep_attach_preflight: false,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    opts
}

fn tallies(s: &AttachProbeSummary) -> (u64, u64, u64, bool) {
    (s.attempted, s.failed, s.bytes, s.truncated)
}

/// Seeded Full ok → probe skips stream; digest_stream_skips >= 1; tallies charged once.
#[test]
fn digest_probe_seed_skips_second_stream_and_charges_once() {
    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);

    let outcome = run_scan(std::slice::from_ref(&path), &attach_scan_opts()).expect("scan");
    assert!(
        !outcome.digest_probe_cache.is_empty(),
        "body-recip-attach Real digest must seed probe cache"
    );
    assert!(
        outcome.summary.grouping.strong_hash_attach_digested >= 1,
        "expected Real digest; stats={:?}",
        outcome.summary.grouping
    );

    let budgets = ProbeBudgets {
        per_attach_max_bytes: 1024,
        max_probe_bytes: 64 * 1024 * 1024,
        ..ProbeBudgets::default()
    };

    let mut with_seed_items = outcome.candidates.clone();
    let seed = {
        // Rebuild seed from scan outcome without consuming the original for the empty path.
        // run_scan already seeded; re-scan is heavy — take a second scan for clean seed/empty pair.
        let again = run_scan(std::slice::from_ref(&path), &attach_scan_opts()).expect("scan2");
        again.digest_probe_cache
    };
    assert!(!seed.is_empty());

    let (seeded_summary, _) = probe_scan_items(
        &mut with_seed_items,
        budgets,
        ProbeLevel::Head,
        ScanMode::BestEffort,
        None,
        None,
        Some(seed),
    );
    assert!(
        seeded_summary.digest_stream_skips >= 1,
        "expected digest_stream_skips; summary={seeded_summary:?}"
    );
    assert!(seeded_summary.attempted >= 1);
    assert_eq!(seeded_summary.failed, 0);
    // Head logical charge capped at per_attach (8 KiB digest → 1 KiB charge).
    assert_eq!(seeded_summary.bytes, 1024);

    let mut empty_items = outcome.candidates.clone();
    let (empty_summary, _) = probe_scan_items(
        &mut empty_items,
        budgets,
        ProbeLevel::Head,
        ScanMode::BestEffort,
        None,
        None,
        None,
    );
    assert_eq!(empty_summary.digest_stream_skips, 0);
    assert_eq!(
        tallies(&seeded_summary),
        tallies(&empty_summary),
        "seeded vs two-pass tallies must match; seeded={seeded_summary:?} empty={empty_summary:?}"
    );
}

/// Deep-preflight alone does not build digest seed (isolation).
#[test]
fn digest_probe_deep_preflight_alone_no_seed() {
    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);

    let opts = ScanOptions {
        retain_candidates: true,
        deep_attach_preflight: true,
        deep_attach_level: "head".into(),
        deep_attach_per_attach_max_bytes: 1024,
        ..ScanOptions::default()
    };
    // Default identity is not body-recip-attach.
    assert!(!opts.grouping.identity.includes_attach_content());

    let outcome = run_scan(std::slice::from_ref(&path), &opts).expect("scan");
    assert!(
        outcome.digest_probe_cache.is_empty(),
        "deep-preflight alone must not leave digest seeds"
    );
    assert_eq!(
        outcome.summary.grouping.strong_hash_attach_digested, 0,
        "deep-preflight alone must not run attach-content digest"
    );
    let pre = &outcome.summary.preflight.attach_probe;
    assert!(pre.enabled);
    assert!(pre.attempted >= 1 || pre.truncated);
}

/// body-recip-attach alone seeds cache but does not run probe (isolation).
#[test]
fn digest_probe_body_recip_attach_alone_seeds_unused() {
    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);

    let outcome = run_scan(std::slice::from_ref(&path), &attach_scan_opts()).expect("scan");
    assert!(!outcome.digest_probe_cache.is_empty());
    assert!(
        !outcome.summary.preflight.attach_probe.enabled,
        "probe must stay disabled when deep_attach_preflight is false"
    );
    assert!(outcome.summary.grouping.strong_hash_attach_digested >= 1);
}

/// Dual-enabled scan path: digest seed consumed; skips recorded on Pass 2.
#[test]
fn digest_probe_dual_enabled_scan_skips_second_stream() {
    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);

    let mut opts = attach_scan_opts();
    opts.deep_attach_preflight = true;
    opts.deep_attach_level = "head".into();
    opts.deep_attach_per_attach_max_bytes = 1024;

    let outcome = run_scan(std::slice::from_ref(&path), &opts).expect("dual scan");
    let pre = &outcome.summary.preflight.attach_probe;
    assert!(pre.enabled);
    assert!(pre.attempted >= 1);
    assert_eq!(pre.failed, 0);
    assert!(
        pre.digest_stream_skips >= 1,
        "dual path must surface digest_stream_skips in preflight; pre={pre:?}"
    );
    assert!(
        pre.bytes_probed >= 1,
        "dual path must surface logical bytes_probed; pre={pre:?}"
    );
    // Seed was consumed into Pass 2 (not left on ScanOutcome after dual scan).
    assert!(
        outcome.digest_probe_cache.is_empty(),
        "dual scan consumes digest seed into probe_scan_items"
    );
    assert!(
        outcome.summary.grouping.strong_hash_attach_digested >= 1,
        "digest still ran under dual path"
    );
}

/// Unit-level: empty seed vs digest seed on same candidates → equal tallies, skips differ.
#[test]
fn digest_probe_equivalence_seed_vs_empty() {
    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);
    let outcome = run_scan(std::slice::from_ref(&path), &attach_scan_opts()).expect("scan");

    let budgets = ProbeBudgets {
        per_attach_max_bytes: 2048,
        ..ProbeBudgets::default()
    };

    let mut a = outcome.candidates.clone();
    let mut b = outcome.candidates.clone();
    let seed = run_scan(std::slice::from_ref(&path), &attach_scan_opts())
        .expect("rescan")
        .digest_probe_cache;

    let (sum_seed, _) = probe_scan_items(
        &mut a,
        budgets,
        ProbeLevel::Full,
        ScanMode::BestEffort,
        None,
        None,
        Some(seed),
    );
    let (sum_empty, _) = probe_scan_items(
        &mut b,
        budgets,
        ProbeLevel::Full,
        ScanMode::BestEffort,
        None,
        None,
        Some(ProbeResultCache::new()),
    );

    assert!(sum_seed.digest_stream_skips >= 1);
    assert_eq!(sum_empty.digest_stream_skips, 0);
    assert_eq!(tallies(&sum_seed), tallies(&sum_empty));
    assert_eq!(a[0].integrity.degraded, b[0].integrity.degraded);
}

/// unique-pst path: probe_keep_set_groups seed vs empty → equal tallies, winners, recommendation.
#[test]
fn digest_probe_keep_set_groups_seed_equivalence() {
    use dedup_engine::integrity::{
        compute_preflight, IntegrityThresholds, PreflightInputs, PreflightRecommendation,
    };
    use dedup_engine::keepset::build_keep_set;

    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);
    let outcome = run_scan(std::slice::from_ref(&path), &attach_scan_opts()).expect("scan");
    let seed = run_scan(std::slice::from_ref(&path), &attach_scan_opts())
        .expect("rescan")
        .digest_probe_cache;
    assert!(!seed.is_empty());

    let budgets = ProbeBudgets {
        per_attach_max_bytes: 1024,
        ..ProbeBudgets::default()
    };
    let grouping = GroupingContext::default();
    let prefer: [String; 0] = [];

    let mut seeded_items = outcome.candidates.clone();
    let (sum_seed, _) = probe_keep_set_groups(
        &mut seeded_items,
        KeepSetProbeOpts {
            budgets,
            level: ProbeLevel::Head,
            policy: KeepPolicy::FirstSeen,
            family: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path: &prefer,
            grouping: grouping.clone(),
            mode: ScanMode::BestEffort,
            cancel: None,
            progress: None,
            seed_cache: Some(seed),
        },
    );

    let mut empty_items = outcome.candidates.clone();
    let (sum_empty, _) = probe_keep_set_groups(
        &mut empty_items,
        KeepSetProbeOpts {
            budgets,
            level: ProbeLevel::Head,
            policy: KeepPolicy::FirstSeen,
            family: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path: &prefer,
            grouping,
            mode: ScanMode::BestEffort,
            cancel: None,
            progress: None,
            seed_cache: None,
        },
    );

    assert!(sum_seed.digest_stream_skips >= 1);
    assert_eq!(sum_empty.digest_stream_skips, 0);
    assert_eq!(tallies(&sum_seed), tallies(&sum_empty));

    let (ks_seed, _) = build_keep_set(
        seeded_items.clone(),
        KeepPolicy::FirstSeen,
        FamilyPolicy::KeepAttachmentsWithParent,
        &prefer,
        true,
    )
    .expect("keep seed");
    let (ks_empty, _) = build_keep_set(
        empty_items.clone(),
        KeepPolicy::FirstSeen,
        FamilyPolicy::KeepAttachmentsWithParent,
        &prefer,
        true,
    )
    .expect("keep empty");
    let winners_seed: Vec<_> = ks_seed
        .winners
        .iter()
        .map(|w| (w.locus.source_pst.clone(), w.locus.nid))
        .collect();
    let winners_empty: Vec<_> = ks_empty
        .winners
        .iter()
        .map(|w| (w.locus.source_pst.clone(), w.locus.nid))
        .collect();
    assert_eq!(winners_seed, winners_empty);
    assert!(!winners_seed.is_empty());

    let pre_seed = compute_preflight(&PreflightInputs {
        mode: ScanMode::BestEffort,
        recoverable: seeded_items.len() as u64,
        skipped: 0,
        crc_skips: 0,
        failed_files: 0,
        input_file_count: 1,
        thresholds: IntegrityThresholds::default(),
        attach_probe_enabled: true,
        attach_probe_level: "head".into(),
        attach_attempted: sum_seed.attempted,
        attach_failed: sum_seed.failed,
        attach_probe_truncated: sum_seed.truncated,
        peer_probe_capped_groups: sum_seed.peer_probe_capped_groups,
        attach_probe_cancelled: sum_seed.cancelled,
        attach_probe_bytes: sum_seed.bytes,
        attach_digest_stream_skips: sum_seed.digest_stream_skips,
    });
    let pre_empty = compute_preflight(&PreflightInputs {
        mode: ScanMode::BestEffort,
        recoverable: empty_items.len() as u64,
        skipped: 0,
        crc_skips: 0,
        failed_files: 0,
        input_file_count: 1,
        thresholds: IntegrityThresholds::default(),
        attach_probe_enabled: true,
        attach_probe_level: "head".into(),
        attach_attempted: sum_empty.attempted,
        attach_failed: sum_empty.failed,
        attach_probe_truncated: sum_empty.truncated,
        peer_probe_capped_groups: sum_empty.peer_probe_capped_groups,
        attach_probe_cancelled: sum_empty.cancelled,
        attach_probe_bytes: sum_empty.bytes,
        attach_digest_stream_skips: sum_empty.digest_stream_skips,
    });
    assert_eq!(pre_seed.recommendation, pre_empty.recommendation);
    assert_eq!(pre_seed.recommendation, PreflightRecommendation::Ok);
}

/// End-to-end unique-pst dual flags: exit success, recommendation ok, digest skips surfaced.
#[test]
fn digest_probe_unique_pst_dual_flag_exit_and_recommendation() {
    use pst_dedup_cli::error::CliExit;
    use pst_dedup_cli::unique_pst_cmd::{
        run_unique_pst_with_options, FolderLayoutArg, UniquePstCliArgs, UniquePstRunOptions,
    };

    let dir = TempDir::new().expect("tmp");
    let path = fixture_pst(&dir);
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    fs::create_dir_all(&report).expect("report");

    let args = UniquePstCliArgs {
        paths: vec![path],
        out,
        report_dir: Some(report.clone()),
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::KeepAttachmentsWithParent,
        prefer_path_contains: vec![],
        prefer_bcc_copy: false,
        prefer_folder_class: false,
        folder_rank: vec![],
        source_rank: vec![],
        rank_folder_class_first: false,
        fidelity_rank: "binary".into(),
        decision_csv: None,
        keep_set_json: None,
        folder_layout: FolderLayoutArg::Preserve,
        max_volume_bytes: None,
        overwrite: true,
        verify_hash: false,
        also_eml: None,
        no_tier2: false,
        no_attachments: false,
        json: false,
        mode: ScanMode::BestEffort,
        max_skip_rate: 0.05,
        max_crc_skip_rate: 0.01,
        max_failed_file_rate: 0.0,
        allow_failed_files: false,
        integrity_csv: None,
        skip_limit: 10_000,
        attach_ledger: AttachLedgerMode::Off,
        attach_ledger_max_rows: 500_000,
        ledger_path_mode: LedgerPathMode::Full,
        deep_attach_preflight: true,
        deep_attach_level: "head".into(),
        deep_attach_max_attaches: 50_000,
        deep_attach_max_probe_bytes: 268_435_456,
        deep_attach_per_attach_max_bytes: 1024,
        deep_attach_max_probe_time_ms: 2000,
        deep_attach_max_open_psts: 32,
        deep_attach_max_peer_probes: 3,
        max_attach_fail_rate: 0.05,
        strong_content_hash: "body-recip-attach".into(),
        strong_hash_attach_max_attaches: 50_000,
        strong_hash_attach_max_bytes: 1_073_741_824,
        strong_hash_attach_per_attach_max_bytes: 536_870_912,
        dedupe_scope: "global".into(),
        tier1_verify: "off".into(),
        tier1_backfill: false,
        identity_ignore_inline_attachments: false,
        allow_cross_mid_tier2: false,
        allow_degenerate_tier2: false,
        allow_crc_suspect_tier2: false,
        crc_log_limit: 10,
        crc_log_interval_secs: 30,
        fail_on_partial_fidelity: false,
        allow_partial_fidelity: true,
        fail_on_export_risk: None,
        max_open_psts: 32,
        qc_level: pst_dedup_cli::unique_pst_qc::QcLevel::Off,
        qc_sample_max: 64,
        qc_external_reader: None,
        qc_scanpst: false,
        include_bcc_recipients: false,
        promote_on_attach_fail: false,
        max_embedded_depth: 3,
    };

    let outcome = run_unique_pst_with_options(
        args,
        UniquePstRunOptions {
            cancel: None,
            stderr_progress: false,
            on_progress: None,
            on_log: None,
        },
    )
    .expect("unique-pst dual");
    assert_eq!(outcome.exit, CliExit::Success);
    assert!(outcome.ok);
    assert_eq!(outcome.unique, 1);

    let summary_path = outcome.summary_path;
    let summary_text = fs::read_to_string(&summary_path).expect("read summary.json");
    let v: serde_json::Value = serde_json::from_str(&summary_text).expect("parse summary");
    let attach = &v["scan"]["preflight"]["attach_probe"];
    assert_eq!(attach["enabled"], true);
    assert!(
        attach["digest_stream_skips"].as_u64().unwrap_or(0) >= 1,
        "unique-pst dual must skip re-stream; attach={attach}"
    );
    assert_eq!(v["scan"]["preflight"]["recommendation"], "ok");
    assert_eq!(v["exit_code"].as_u64().unwrap_or(999), 0);
}
