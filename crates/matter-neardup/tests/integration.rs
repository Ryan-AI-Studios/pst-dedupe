//! Synthetic matter near-dup tests (spec §3.11).

use matter_core::{item_dedup_role, item_near_dup_role, item_role, item_status, ItemInput, Matter};
use matter_neardup::{
    expand_shingle_hashes, run_neardup, text_to_shingles, NearDupOutcome, NearDupParams,
    SplitMix64, JOB_KIND_NEARDUP, NEARDUP_STAGE, NEAR_DUP_METHOD,
};

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = camino::Utf8Path::from_path(tmp.path())
        .expect("utf8")
        .to_path_buf();
    (tmp, path)
}

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let (tmp, base) = utf8_tempdir();
    let root = base.join(name);
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

/// Pad text to exceed default min_chars (80) and produce enough word
/// *k*-shingles so a single-word edit still estimates Jaccard ≥ 0.80.
///
/// With k=5, one word change touches ≤5 shingles. Need S ≳ 45 shingles
/// so (S−5)/(S+5) ≥ 0.80.
fn pad(s: &str) -> String {
    let mut t = s.to_string();
    // Shared long boilerplate (identical across near-identical variants).
    let boilerplate = " alpha bravo charlie delta echo foxtrot golf hotel india juliet \
kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey \
xray yankee zulu one two three four five six seven eight nine ten eleven twelve \
thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone \
twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight \
twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix \
thirtyseven thirtyeight thirtynine forty fortyone fortytwo fortythree fortyfour \
fortyfive fortysix fortyseven fortyeight fortynine fifty";
    t.push_str(boilerplate);
    while t.chars().count() < 200 {
        t.push_str(" more shared padding vocabulary tokens ");
    }
    t
}

fn put_text(matter: &Matter, text: &str) -> String {
    matter.put_bytes(text.as_bytes()).expect("cas")
}

fn insert_doc(matter: &Matter, path: &str, text: &str, dedup_role: Option<&str>) -> String {
    let digest = put_text(matter, text);
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some(path.into()),
            text_sha256: Some(digest),
            dedup_role: dedup_role.map(|s| s.into()),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

fn run_default(matter: &Matter, job_id: &str) -> NearDupOutcome {
    let params = NearDupParams::default();
    run_neardup(matter, job_id, &params, None, |_| {}).expect("run")
}

#[test]
fn near_identical_one_word_diff_same_group() {
    let (_tmp, matter) = temp_matter("near-id");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    let base = "the quick brown fox jumps over the lazy dog while reviewing the contract draft carefully with counsel present";
    let a = pad(base);
    let b = pad(
        "the quick brown fox jumps over the lazy dog while reviewing the contract draft carefully with lawyers present",
    );
    let id_a = insert_doc(&matter, "a.txt", &a, Some(item_dedup_role::UNIQUE));
    let id_b = insert_doc(&matter, "b.txt", &b, Some(item_dedup_role::UNIQUE));

    let outcome = run_default(&matter, &job.id);
    assert!(
        matches!(outcome, NearDupOutcome::Succeeded(_)),
        "{outcome:?}"
    );

    let ia = matter.get_item(&id_a).unwrap();
    let ib = matter.get_item(&id_b).unwrap();
    assert_eq!(ia.near_dup_group_id, ib.near_dup_group_id);
    assert!(ia.near_dup_group_id.is_some());
    let roles: std::collections::HashSet<_> =
        [ia.near_dup_role.as_deref(), ib.near_dup_role.as_deref()]
            .into_iter()
            .collect();
    assert!(roles.contains(&Some(item_near_dup_role::PIVOT)));
    assert!(roles.contains(&Some(item_near_dup_role::MEMBER)));
    for item in [&ia, &ib] {
        if item.near_dup_role.as_deref() == Some(item_near_dup_role::MEMBER) {
            let sim = item.near_dup_similarity.expect("sim");
            assert!(sim > 0.80 && sim <= 1.0, "sim={sim}");
        }
        if item.near_dup_role.as_deref() == Some(item_near_dup_role::PIVOT) {
            assert_eq!(item.near_dup_similarity, Some(1.0));
        }
        assert_eq!(item.near_dup_method.as_deref(), Some(NEAR_DUP_METHOD));
    }
}

#[test]
fn completely_different_both_unique() {
    let (_tmp, matter) = temp_matter("diff");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    // Distinct long bodies with no shared pad boilerplate.
    let a = "astronomy nebula quasar pulsar galaxy telescope spectrum photon orbit gravity \
relativity singularity blackhole eventhorizon redshift blueshift supernova neutronstar \
cosmicmicrowave background radiation darkmatter darkenergy multiverse spacetime continuum \
exoplanet asteroid meteor comet satellite constellation celestial equatorial ecliptic \
parallax magnitude luminosity photometry spectroscopy interferometry coronagraph adaptive \
optics observatory planetarium astrophysics cosmology nucleosynthesis baryon lepton quark \
hadron meson boson fermion neutrino muon tau lepton flavor oscillation";
    let b = "culinary recipe kitchen saucepan skillet spatula whisk blender oven broiler \
grill roast braise simmer saute blanch confit emulsion reduction mirepoix bouquet garni \
roux bechamel veloute espagnole hollandaise stock broth consomme julienne brunoise \
chiffonade dice mince puree glaze caramelize deglaze temper bloom proof ferment pickle \
cure smoke dryage charcuterie chargrill sousvide immersion thermocouple pastry baguette \
croissant brioche sourdough preferment autolyse laminated dough laminated pastry filo";
    assert!(a.chars().count() >= 80 && b.chars().count() >= 80);
    let id_a = insert_doc(&matter, "a.txt", a, None);
    let id_b = insert_doc(&matter, "b.txt", b, None);

    let _ = run_default(&matter, &job.id);
    let ia = matter.get_item(&id_a).unwrap();
    let ib = matter.get_item(&id_b).unwrap();
    assert_eq!(
        ia.near_dup_role.as_deref(),
        Some(item_near_dup_role::UNIQUE)
    );
    assert_eq!(
        ib.near_dup_role.as_deref(),
        Some(item_near_dup_role::UNIQUE)
    );
    assert!(ia.near_dup_group_id.is_none());
    assert!(ib.near_dup_group_id.is_none());
}

#[test]
fn exact_same_body_same_group_high_sim() {
    let (_tmp, matter) = temp_matter("exact-body");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");
    let body =
        pad("identical body text for both items used in near duplicate detection validation suite");
    let id_a = insert_doc(&matter, "a.txt", &body, None);
    let id_b = insert_doc(&matter, "b.txt", &body, None);

    let _ = run_default(&matter, &job.id);
    let ia = matter.get_item(&id_a).unwrap();
    let ib = matter.get_item(&id_b).unwrap();
    assert_eq!(ia.near_dup_group_id, ib.near_dup_group_id);
    assert!(ia.near_dup_group_id.is_some());
    for item in [&ia, &ib] {
        if item.near_dup_role.as_deref() == Some(item_near_dup_role::MEMBER) {
            assert!((item.near_dup_similarity.unwrap() - 1.0).abs() < 1e-9);
        }
    }
}

#[test]
fn skip_exact_duplicates_marks_skipped() {
    let (_tmp, matter) = temp_matter("skip-dup");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");
    let body = pad("document that was already marked as exact duplicate by matter dedupe job");
    let id = insert_doc(&matter, "dup.txt", &body, Some(item_dedup_role::DUPLICATE));

    let _ = run_default(&matter, &job.id);
    let item = matter.get_item(&id).unwrap();
    assert_eq!(
        item.near_dup_role.as_deref(),
        Some(item_near_dup_role::SKIPPED)
    );
}

#[test]
fn missing_text_sha256_skipped() {
    let (_tmp, matter) = temp_matter("no-text");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("empty.txt".into()),
            text_sha256: None,
            ..Default::default()
        })
        .expect("insert");

    let _ = run_default(&matter, &job.id);
    let got = matter.get_item(&item.id).unwrap();
    assert_eq!(
        got.near_dup_role.as_deref(),
        Some(item_near_dup_role::SKIPPED)
    );
}

#[test]
fn below_min_chars_skipped() {
    let (_tmp, matter) = temp_matter("short");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");
    let id = insert_doc(&matter, "short.txt", "too short", None);

    let params = NearDupParams {
        min_chars: 80,
        ..Default::default()
    };
    let _ = run_neardup(&matter, &job.id, &params, None, |_| {}).unwrap();
    let got = matter.get_item(&id).unwrap();
    assert_eq!(
        got.near_dup_role.as_deref(),
        Some(item_near_dup_role::SKIPPED)
    );
}

#[test]
fn pivot_is_longer_token_count() {
    let (_tmp, matter) = temp_matter("pivot-len");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    // Shared core + extra tokens on B → higher token_count
    let core =
        "the board approved the merger agreement after lengthy negotiation sessions with advisors";
    let short = pad(core);
    let long = pad(&format!(
        "{core} and additional schedule exhibits appendix notes tables figures references citations"
    ));
    let id_short = insert_doc(&matter, "short.txt", &short, None);
    let id_long = insert_doc(&matter, "long.txt", &long, None);

    let _ = run_default(&matter, &job.id);
    let short_item = matter.get_item(&id_short).unwrap();
    let long_item = matter.get_item(&id_long).unwrap();
    // Fixture must group; pivot is the longer-token item.
    assert!(
        short_item.near_dup_group_id.is_some(),
        "expected short+long to form a near-dup group; short role={:?} long role={:?}",
        short_item.near_dup_role,
        long_item.near_dup_role
    );
    assert_eq!(
        short_item.near_dup_group_id, long_item.near_dup_group_id,
        "both items must share the group id"
    );
    assert_eq!(
        long_item.near_dup_role.as_deref(),
        Some(item_near_dup_role::PIVOT)
    );
    assert_eq!(
        short_item.near_dup_role.as_deref(),
        Some(item_near_dup_role::MEMBER)
    );
    assert_eq!(
        short_item.near_dup_pivot_item_id.as_deref(),
        Some(id_long.as_str())
    );
}

#[test]
fn single_link_demotion_weak_member() {
    // Forced demotion (synthetic sigs A~B, B~C, A vs pivot C weak) is covered by
    // unit tests `single_link_demotion_weak_vs_pivot` and
    // `demoted_items_never_remain_members_below_threshold` in cluster.rs.
    // This integration fixture only asserts the post-condition invariant on real texts:
    // no member may have similarity below the job threshold.
    let (_tmp, matter) = temp_matter("demote");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    let a = pad(
        "version one of the financial model assumptions revenue growth margin expansion plan alpha",
    );
    let b = pad(
        "version one of the financial model assumptions revenue growth margin expansion plan beta",
    );
    // C is related to B wording but drifts further from A
    let c = pad(
        "version two financial model different assumptions cost structure headcount plan gamma delta",
    );
    let id_a = insert_doc(&matter, "a.txt", &a, None);
    let id_b = insert_doc(&matter, "b.txt", &b, None);
    let id_c = insert_doc(&matter, "c.txt", &c, None);

    let params = NearDupParams {
        threshold: 0.80,
        ..Default::default()
    };
    let _ = run_neardup(&matter, &job.id, &params, None, |_| {}).unwrap();

    let ia = matter.get_item(&id_a).unwrap();
    let ib = matter.get_item(&id_b).unwrap();
    let ic = matter.get_item(&id_c).unwrap();

    for item in [&ia, &ib, &ic] {
        if item.near_dup_role.as_deref() == Some(item_near_dup_role::MEMBER) {
            let sim = item.near_dup_similarity.expect("sim");
            assert!(
                sim >= 0.80,
                "member sim {sim} must be >= threshold after demotion"
            );
        }
    }
}

#[test]
fn deterministic_two_reset_runs() {
    let (_tmp, matter) = temp_matter("det");
    let body_a =
        pad("deterministic fixture alpha for near duplicate clustering validation pass number one");
    let body_b =
        pad("deterministic fixture alpha for near duplicate clustering validation pass number two");
    let id_a = insert_doc(&matter, "a.txt", &body_a, None);
    let id_b = insert_doc(&matter, "b.txt", &body_b, None);

    let job1 = matter.create_job(JOB_KIND_NEARDUP).expect("job1");
    let _ = run_default(&matter, &job1.id);
    let g1a = matter.get_item(&id_a).unwrap().near_dup_group_id.clone();
    let r1a = matter.get_item(&id_a).unwrap().near_dup_role.clone();
    let r1b = matter.get_item(&id_b).unwrap().near_dup_role.clone();
    let p1 = matter
        .get_item(&id_a)
        .unwrap()
        .near_dup_pivot_item_id
        .clone();

    let job2 = matter.create_job(JOB_KIND_NEARDUP).expect("job2");
    let _ = run_default(&matter, &job2.id);
    let g2a = matter.get_item(&id_a).unwrap().near_dup_group_id.clone();
    let r2a = matter.get_item(&id_a).unwrap().near_dup_role.clone();
    let r2b = matter.get_item(&id_b).unwrap().near_dup_role.clone();
    let p2 = matter
        .get_item(&id_a)
        .unwrap()
        .near_dup_pivot_item_id
        .clone();

    assert_eq!(g1a, g2a);
    assert_eq!(r1a, r2a);
    assert_eq!(r1b, r2b);
    assert_eq!(p1, p2);
}

#[test]
fn cjk_near_identical_same_group() {
    let (_tmp, matter) = temp_matter("cjk-near");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    let base = "这是一份非常重要的合同文件内容用于测试近似重复检测功能是否能够正确识别中文文本的相似性情况并且生成非空的字符";
    // Repeat to clear min_chars=80 and keep high bigram overlap.
    let a = format!("{base}{base}二元组扩展段落继续描述合同条款与附件清单明细");
    let b = format!("{base}{base}三元组扩展段落继续描述合同条款与附件清单明细");
    assert!(a.chars().count() >= 80);
    let id_a = insert_doc(&matter, "a.txt", &a, None);
    let id_b = insert_doc(&matter, "b.txt", &b, None);

    // Verify shingles non-empty
    let (_, shingles, _) = text_to_shingles(&a, 5, 2, true);
    assert!(!shingles.is_empty(), "CJK must produce shingles");

    let _ = run_default(&matter, &job.id);
    let ia = matter.get_item(&id_a).unwrap();
    let ib = matter.get_item(&id_b).unwrap();
    assert_eq!(ia.near_dup_group_id, ib.near_dup_group_id);
    assert!(
        ia.near_dup_group_id.is_some(),
        "CJK near-identical should group, roles={:?}/{:?}",
        ia.near_dup_role,
        ib.near_dup_role
    );
    assert_ne!(
        ia.near_dup_role.as_deref(),
        Some(item_near_dup_role::SKIPPED)
    );
}

#[test]
fn cjk_unrelated_not_same_group() {
    let (_tmp, matter) = temp_matter("cjk-diff");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    let a = "今天天气晴朗适合外出散步游览城市公园欣赏美丽风景呼吸新鲜空气放松心情享受生活美好时光";
    let b = "财务报表显示本季度营业收入大幅增长主要受益于新产品线的成功推广和市场份额的持续扩大";
    assert!(a.chars().count() >= 40);
    // pad with more CJK to clear min_chars
    let a = format!("{a}{a}");
    let b = format!("{b}{b}");
    let id_a = insert_doc(&matter, "a.txt", &a, None);
    let id_b = insert_doc(&matter, "b.txt", &b, None);

    let _ = run_default(&matter, &job.id);
    let ia = matter.get_item(&id_a).unwrap();
    let ib = matter.get_item(&id_b).unwrap();
    assert!(
        ia.near_dup_group_id.is_none() || ia.near_dup_group_id != ib.near_dup_group_id,
        "unrelated CJK must not share a group"
    );
    // Prefer both unique
    if ia.near_dup_role.as_deref() != Some(item_near_dup_role::SKIPPED) {
        assert_eq!(
            ia.near_dup_role.as_deref(),
            Some(item_near_dup_role::UNIQUE)
        );
    }
    if ib.near_dup_role.as_deref() != Some(item_near_dup_role::SKIPPED) {
        assert_eq!(
            ib.near_dup_role.as_deref(),
            Some(item_near_dup_role::UNIQUE)
        );
    }
}

#[test]
fn hash_family_not_km_and_splitmix_golden() {
    // Hard-coded first outputs for seed 0xDEAD_BEEF_CAFE_BABE (absolute freeze).
    let mut rng = SplitMix64::new(0xDEAD_BEEF_CAFE_BABE);
    let got: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
    assert_eq!(
        got,
        vec![
            0x0d7d_9356_0d19_29d2,
            0x491d_fb74_0e50_d43f,
            0x4272_2bf4_473e_5e7d,
            0xd6ca_8a07_90ff_fc45,
        ]
    );
    assert_ne!(got[0], got[1]);

    let shingle = b"sample\x1fshingle\x1ffor\x1fhash\x1ftest";
    let ours = expand_shingle_hashes(shingle, 0x4E44_5F6D_685F_7631, 32);
    // KM style from same SHA-256 first two u64s
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(shingle);
    let mut h1b = [0u8; 8];
    let mut h2b = [0u8; 8];
    h1b.copy_from_slice(&digest[0..8]);
    h2b.copy_from_slice(&digest[8..16]);
    let h1 = u64::from_be_bytes(h1b);
    let h2 = u64::from_be_bytes(h2b) | 1;
    let km: Vec<u64> = (0..32u64)
        .map(|i| h1.wrapping_add(i.wrapping_mul(h2)))
        .collect();
    assert_ne!(ours, km);
}

#[test]
fn cancel_pauses_resume_completes() {
    let (_tmp, matter) = temp_matter("cancel");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");

    for i in 0..12 {
        let body = pad(&format!(
            "unique document number {i} with distinct vocabulary words set for cancel testing path"
        ));
        insert_doc(&matter, &format!("d{i:02}.txt"), &body, None);
    }

    let cancel_after = std::sync::atomic::AtomicU64::new(0);
    let params = NearDupParams {
        batch_size: 1,
        ..Default::default()
    };
    let outcome = run_neardup(
        &matter,
        &job.id,
        &params,
        Some(&|| cancel_after.load(std::sync::atomic::Ordering::SeqCst) > 0),
        |completed| {
            if completed > 0 {
                cancel_after.store(1, std::sync::atomic::Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    let NearDupOutcome::Paused(s) = outcome else {
        panic!("expected Paused, got {outcome:?}");
    };
    assert!(s.completed_count > 0, "must have committed progress");
    let cp = matter
        .get_checkpoint(&job.id, NEARDUP_STAGE)
        .expect("cp")
        .expect("checkpoint after pause");
    assert!(!cp.cursor_json.is_empty());

    let outcome2 = run_neardup(&matter, &job.id, &params, None, |_| {}).expect("resume");
    assert!(
        matches!(outcome2, NearDupOutcome::Succeeded(_)),
        "resume must succeed: {outcome2:?}"
    );
}

#[test]
fn audit_neardup_start_and_complete() {
    let (_tmp, matter) = temp_matter("audit");
    let job = matter.create_job(JOB_KIND_NEARDUP).expect("job");
    let body = pad("audit trail document for near duplicate detection job lifecycle events");
    insert_doc(&matter, "a.txt", &body, None);

    let out = run_default(&matter, &job.id);
    assert!(matches!(out, NearDupOutcome::Succeeded(_)));

    let mut stmt = matter
        .connection()
        .prepare("SELECT action FROM audit_events WHERE action LIKE 'neardup.%' ORDER BY seq ASC")
        .expect("prepare");
    let actions: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    assert!(
        actions.iter().any(|a| a == "neardup.start"),
        "expected neardup.start in {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a == "neardup.complete"),
        "expected neardup.complete in {actions:?}"
    );
    let start_pos = actions.iter().position(|a| a == "neardup.start").unwrap();
    let complete_pos = actions
        .iter()
        .position(|a| a == "neardup.complete")
        .unwrap();
    assert!(start_pos < complete_pos);
}

#[test]
fn audit_neardup_fail_on_bad_job_id() {
    // Force write-phase failure after `neardup.start` by using a job id that
    // does not exist (checkpoint FK on jobs). Asserts `neardup.fail` is audited.
    let (_tmp, matter) = temp_matter("audit-fail");
    let body = pad("audit fail path document for near duplicate detection job lifecycle events");
    insert_doc(&matter, "a.txt", &body, None);

    let params = NearDupParams::default();
    let result = run_neardup(
        &matter,
        "missing-job-id-for-fail-audit",
        &params,
        None,
        |_| {},
    );
    assert!(
        result.is_err(),
        "expected Err from missing job FK, got {result:?}"
    );

    let mut stmt = matter
        .connection()
        .prepare("SELECT action FROM audit_events WHERE action LIKE 'neardup.%' ORDER BY seq ASC")
        .expect("prepare");
    let actions: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    assert!(
        actions.iter().any(|a| a == "neardup.start"),
        "expected neardup.start in {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a == "neardup.fail"),
        "expected neardup.fail in {actions:?}"
    );
    let start_pos = actions.iter().position(|a| a == "neardup.start").unwrap();
    let fail_pos = actions.iter().position(|a| a == "neardup.fail").unwrap();
    assert!(start_pos < fail_pos);
}
