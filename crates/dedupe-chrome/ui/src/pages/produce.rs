use std::collections::HashMap;

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};

use crate::invoke::{
    tauri_invoke, ChromeExtra, ChromeQcFinding, JobProgressSnapshot, ProduceBurnSet,
    ProduceBurnSetArgs, ProducePageResponse, ProduceQcFindingsArgs, ProduceQcRun, ProduceQcRunArgs,
    ProduceStart, ProduceStartArgs, ProductionSetThin, RootArgs, WarningOverride,
};
use crate::path_id::{matter_home_href_from_param, review_doc_href};

fn override_key(rule_id: &str, item_id: Option<&str>) -> String {
    match item_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => format!("{rule_id}\u{1f}{id}"),
        None => format!("{rule_id}\u{1f}*"),
    }
}

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

async fn wait_process_terminal(root: &str) -> Result<JobProgressSnapshot, String> {
    loop {
        let snap = tauri_invoke::<JobProgressSnapshot, _>(
            "process_progress",
            &RootArgs {
                root: root.to_string(),
            },
        )
        .await?;
        if snap.job_id.is_empty()
            || snap.state == "idle"
            || snap.state == "succeeded"
            || snap.state == "failed"
            || snap.state == "cancelled"
            || snap.state == "paused"
        {
            return Ok(snap);
        }
        sleep_ms(400).await;
    }
}

/// Wire `process_progress.state` is success only as exact `"succeeded"`.
fn process_job_succeeded(state: &str) -> bool {
    state == "succeeded"
}

fn wait_root_is_current(captured: &str, live: &str) -> bool {
    captured == live
}

/// Not-succeeded terminals leave a prior latch; first cancel stays unlatched.
fn volume_latch_after_produce_terminal(prev: bool, succeeded: bool) -> bool {
    if succeeded {
        true
    } else {
        prev
    }
}

fn finalize_blocked_by_volume_latch(volume_succeeded: bool, start_busy: bool) -> bool {
    volume_succeeded || start_busy
}

/// Apply `next_seq_hint` only after a succeeded volume (never first-paint silent 1).
fn bates_start_from_next_seq_hint(hint: Option<u64>) -> Option<String> {
    hint.filter(|n| *n >= 1).map(|n| n.to_string())
}

fn terminal_job_error(kind: &str, snap: &JobProgressSnapshot) -> String {
    snap.error_summary
        .clone()
        .unwrap_or_else(|| format!("{kind} {}", snap.state))
}

/// Host attaches genuine post-step failures as `privilege log: …` on a succeeded snap.
fn privilege_log_post_step_banner(snap: &JobProgressSnapshot) -> Option<String> {
    snap.error_summary
        .as_ref()
        .filter(|s| s.contains("privilege log:"))
        .cloned()
}

#[cfg(test)]
mod process_job_succeeded_tests {
    use super::{
        bates_start_from_next_seq_hint, finalize_blocked_by_volume_latch,
        privilege_log_post_step_banner, process_job_succeeded, volume_latch_after_produce_terminal,
        wait_root_is_current, JobProgressSnapshot,
    };

    #[test]
    fn only_succeeded_is_success() {
        assert!(process_job_succeeded("succeeded"));
        assert!(!process_job_succeeded("Succeeded"));
        assert!(!process_job_succeeded("cancelled"));
        assert!(!process_job_succeeded("idle"));
        assert!(!process_job_succeeded("paused"));
        assert!(!process_job_succeeded("failed"));
        assert!(!process_job_succeeded(""));
        assert!(!process_job_succeeded("running"));
    }

    #[test]
    fn stale_wait_when_root_drifts() {
        assert!(wait_root_is_current(r"C:\a", r"C:\a"));
        assert!(!wait_root_is_current(r"C:\a", r"C:\b"));
    }

    #[test]
    fn latch_sets_only_on_success_and_survives_cancel() {
        assert!(volume_latch_after_produce_terminal(false, true));
        assert!(volume_latch_after_produce_terminal(true, false));
        assert!(!volume_latch_after_produce_terminal(false, false));
        // Hint refresh is independent: a failed produce_page still leaves the latch on.
        assert_eq!(bates_start_from_next_seq_hint(None), None);
        assert!(volume_latch_after_produce_terminal(false, true));
    }

    #[test]
    fn qc_cannot_rearm_finalize_via_latch() {
        assert!(finalize_blocked_by_volume_latch(true, false));
        assert!(finalize_blocked_by_volume_latch(false, true));
        assert!(!finalize_blocked_by_volume_latch(false, false));
    }

    #[test]
    fn log_post_step_error_still_succeeded_for_latch() {
        let mut snap = JobProgressSnapshot {
            job_id: "j1".into(),
            kind: "produce".into(),
            matter_id: "m".into(),
            state: "succeeded".into(),
            stage: None,
            completed_count: 1,
            total_hint: None,
            message: None,
            error_summary: Some("privilege log: disk full".into()),
            updated_at: String::new(),
        };
        assert!(process_job_succeeded(&snap.state));
        assert_eq!(
            privilege_log_post_step_banner(&snap).as_deref(),
            Some("privilege log: disk full")
        );
        snap.error_summary = None;
        assert!(privilege_log_post_step_banner(&snap).is_none());
    }

    #[test]
    fn hint_only_when_at_least_one() {
        assert_eq!(
            bates_start_from_next_seq_hint(Some(42)).as_deref(),
            Some("42")
        );
        assert_eq!(bates_start_from_next_seq_hint(Some(0)), None);
        assert_eq!(bates_start_from_next_seq_hint(None), None);
    }

    #[test]
    fn finalize_view_wires_latch_not_start_result_ok() {
        let src = include_str!("produce.rs");
        assert!(
            src.contains("finalize_blocked_by_volume_latch"),
            "Finalize disabled must call the volume latch helper"
        );
        assert!(
            src.contains("if start_busy.get() || volume_succeeded.get()"),
            "Finalize click must no-op when latched"
        );
        assert!(
            src.contains("volume_succeeded.set(false)"),
            "matter switch must clear the latch"
        );
        assert!(
            !src.contains("qc_busy.set(false);\n            start_busy.set(false)")
                && !src.contains("start_busy.set(false);\n            qc_busy.set(false)"),
            "matter switch must not force-clear busy flags as a pair"
        );
    }
}

fn is_busy_err(e: &str) -> bool {
    e.starts_with("busy:") || e.contains("matter is busy")
}

const DAT_ONLY_PROFILE: &str = "us_concordance_native_text_v1";
const IMAGE_OPT_PROFILE: &str = "us_concordance_image_opt_v1";

fn selected_profile_flags(page: &Option<ProducePageResponse>, slug: &str) -> (bool, bool) {
    let Some(p) = page
        .as_ref()
        .and_then(|pg| pg.profiles.iter().find(|x| x.slug == slug))
    else {
        return (false, false);
    };
    (p.include_images, p.bates_mode.eq_ignore_ascii_case("page"))
}

#[component]
pub fn ProducePage() -> impl IntoView {
    let params = use_params_map();
    let root_sig = RwSignal::new(String::new());
    let error = RwSignal::new(Option::<String>::None);
    let page = RwSignal::new(Option::<ProducePageResponse>::None);
    let step = RwSignal::new(1u8);
    let entire_corpus = RwSignal::new(false);
    let prefix = RwSignal::new("PROD".to_string());
    let bates_start = RwSignal::new(String::new());
    let profile = RwSignal::new("us_concordance_native_text_v1".to_string());
    let log_format = RwSignal::new("standard".to_string());
    let qc = RwSignal::new(Option::<ProduceQcRun>::None);
    let qc_busy = RwSignal::new(false);
    let start_busy = RwSignal::new(false);
    let start_result = RwSignal::new(Option::<ProduceStart>::None);
    let volume_succeeded = RwSignal::new(false);
    let overrides = RwSignal::new(HashMap::<String, (String, String)>::new());
    let busy_banner = RwSignal::new(Option::<String>::None);
    let last_root = RwSignal::new(String::new());

    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));

    Effect::new(move |_| {
        let root = params.with(|p| p.get("id").unwrap_or_default());
        if root.is_empty() {
            error.set(Some("Missing matter id in route.".into()));
            return;
        }
        let switched = last_root.get_untracked() != root;
        root_sig.set(root.clone());
        if switched {
            last_root.set(root.clone());
            qc.set(None);
            overrides.set(HashMap::new());
            start_result.set(None);
            volume_succeeded.set(false);
            step.set(1);
            entire_corpus.set(false);
            bates_start.set(String::new());
            busy_banner.set(None);
            error.set(None);
            // Do not reset start_busy / qc_busy: in-flight waits own those until they exit.
        }
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ProducePageResponse, _>(
                "produce_page",
                &RootArgs { root: root.clone() },
            )
            .await
            {
                Ok(resp) => {
                    if !wait_root_is_current(&root, &root_sig.get_untracked()) {
                        return;
                    }
                    prefix.set(resp.bates_prefix.clone());
                    if let Some(first) = resp.profiles.first() {
                        profile.set(first.slug.clone());
                    }
                    page.set(Some(resp));
                }
                Err(e) => {
                    if !wait_root_is_current(&root, &root_sig.get_untracked()) {
                        return;
                    }
                    page.set(None);
                    error.set(Some(e));
                }
            }
        });
    });

    let run_qc = move || {
        let root = root_sig.get();
        if root.is_empty() || qc_busy.get() {
            return;
        }
        qc_busy.set(true);
        error.set(None);
        let entire = entire_corpus.get();
        let prof = profile.get();
        leptos::task::spawn_local(async move {
            busy_banner.set(None);
            let res = tauri_invoke::<ProduceQcRun, _>(
                "produce_qc_run",
                &ProduceQcRunArgs {
                    root: root.clone(),
                    filter_json: None,
                    item_ids: None,
                    production_profile: Some(prof),
                    source_entire_corpus: Some(entire),
                },
            )
            .await;
            match res {
                Ok(r) => {
                    if !wait_root_is_current(&root, &root_sig.get_untracked()) {
                        qc_busy.set(false);
                        return;
                    }
                    if let Some(job_id) = r.job_id.clone() {
                        match wait_process_terminal(&root).await {
                            Err(e) => {
                                qc_busy.set(false);
                                if wait_root_is_current(&root, &root_sig.get_untracked()) {
                                    error.set(Some(e));
                                }
                                return;
                            }
                            Ok(snap) => {
                                if !wait_root_is_current(&root, &root_sig.get_untracked()) {
                                    qc_busy.set(false);
                                    return;
                                }
                                if !process_job_succeeded(&snap.state) {
                                    qc_busy.set(false);
                                    error.set(Some(terminal_job_error("QC", &snap)));
                                    return;
                                }
                            }
                        }
                        match tauri_invoke::<ProduceQcRun, _>(
                            "produce_qc_findings",
                            &ProduceQcFindingsArgs {
                                root: root.clone(),
                                job_id: Some(job_id),
                            },
                        )
                        .await
                        {
                            Ok(findings) => {
                                if wait_root_is_current(&root, &root_sig.get_untracked()) {
                                    overrides.set(HashMap::new());
                                    qc.set(Some(findings));
                                    start_result.set(None);
                                }
                            }
                            Err(e) => {
                                if wait_root_is_current(&root, &root_sig.get_untracked()) {
                                    error.set(Some(e));
                                }
                            }
                        }
                    } else if wait_root_is_current(&root, &root_sig.get_untracked()) {
                        overrides.set(HashMap::new());
                        qc.set(Some(r));
                        start_result.set(None);
                    }
                }
                Err(e) => {
                    if wait_root_is_current(&root, &root_sig.get_untracked()) {
                        if is_busy_err(&e) {
                            busy_banner.set(Some(e));
                        } else {
                            error.set(Some(e));
                        }
                    }
                }
            }
            qc_busy.set(false);
        });
    };

    let finalize = move |_| {
        let root = root_sig.get();
        let start = bates_start.get();
        let parsed: Option<u64> = start.trim().parse().ok().filter(|n| *n >= 1);
        let Some(bates_start_n) = parsed else {
            error.set(Some("bates_start is required and must be >= 1".into()));
            return;
        };
        if start_busy.get() || volume_succeeded.get() {
            return;
        }
        start_busy.set(true);
        error.set(None);
        let entire = entire_corpus.get();
        let prof = profile.get();
        let pref = prefix.get();
        let fmt = log_format.get();
        let last = qc.get().map(|r| r.findings.clone());
        let ovs = {
            let map = overrides.get();
            qc.get()
                .map(|r| {
                    r.findings
                        .iter()
                        .filter(|f| f.severity == "warn")
                        .filter_map(|f| {
                            let key = override_key(&f.rule_id, f.item_id.as_deref());
                            let (by, reason) = map.get(&key)?;
                            if by.trim().is_empty() || reason.trim().is_empty() {
                                return None;
                            }
                            Some(WarningOverride {
                                recorded_by: by.clone(),
                                reason: reason.clone(),
                                rule_id: f.rule_id.clone(),
                                item_id: f.item_id.clone(),
                                qc_run_id: r.qc_run_id.clone(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        leptos::task::spawn_local(async move {
            busy_banner.set(None);
            let res = tauri_invoke::<ProduceStart, _>(
                "produce_start",
                &ProduceStartArgs {
                    root: root.clone(),
                    filter_json: None,
                    item_ids: None,
                    production_profile: Some(prof),
                    source_entire_corpus: Some(entire),
                    bates_prefix: Some(pref),
                    bates_start: Some(bates_start_n),
                    warning_overrides: Some(ovs),
                    log_format: Some(fmt),
                    last_findings: last,
                },
            )
            .await;
            match res {
                Ok(r) => {
                    if !wait_root_is_current(&root, &root_sig.get_untracked()) {
                        start_busy.set(false);
                        return;
                    }
                    if !r.ok {
                        start_result.set(Some(r));
                        error.set(Some("Finalize blocked — see pre-flight cards.".into()));
                    } else if r.job_id.is_some() {
                        match wait_process_terminal(&root).await {
                            Err(e) => {
                                if wait_root_is_current(&root, &root_sig.get_untracked()) {
                                    error.set(Some(e));
                                }
                            }
                            Ok(snap) => {
                                if !wait_root_is_current(&root, &root_sig.get_untracked()) {
                                    start_busy.set(false);
                                    return;
                                }
                                if !process_job_succeeded(&snap.state) {
                                    error.set(Some(terminal_job_error("produce", &snap)));
                                    volume_succeeded.update(|prev| {
                                        *prev = volume_latch_after_produce_terminal(*prev, false);
                                    });
                                } else {
                                    // Latch on job success even if produce_page refresh fails —
                                    // Bates were already assigned; do not re-arm Finalize.
                                    volume_succeeded.set(volume_latch_after_produce_terminal(
                                        volume_succeeded.get_untracked(),
                                        true,
                                    ));
                                    let log_err = privilege_log_post_step_banner(&snap);
                                    match tauri_invoke::<ProducePageResponse, _>(
                                        "produce_page",
                                        &RootArgs { root: root.clone() },
                                    )
                                    .await
                                    {
                                        Ok(pg) => {
                                            if !wait_root_is_current(
                                                &root,
                                                &root_sig.get_untracked(),
                                            ) {
                                                start_busy.set(false);
                                                return;
                                            }
                                            if let Some(next) =
                                                bates_start_from_next_seq_hint(pg.next_seq_hint)
                                            {
                                                bates_start.set(next);
                                            }
                                            let mut filled = r.clone();
                                            if let Some(set) = pg.sets.iter().rev().find(|s| {
                                                s.output_root
                                                    .as_ref()
                                                    .is_some_and(|p| !p.is_empty())
                                            }) {
                                                filled.output_root = set.output_root.clone();
                                                filled.production_set_id = Some(set.id.clone());
                                                filled.produced_count = pg.produced_count;
                                            }
                                            page.set(Some(pg));
                                            start_result.set(Some(filled));
                                            if let Some(e) = log_err {
                                                error.set(Some(e));
                                            }
                                        }
                                        Err(e) => {
                                            if wait_root_is_current(
                                                &root,
                                                &root_sig.get_untracked(),
                                            ) {
                                                error.set(Some(e));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        start_result.set(Some(r));
                    }
                }
                Err(e) => {
                    if wait_root_is_current(&root, &root_sig.get_untracked()) {
                        if is_busy_err(&e) {
                            busy_banner.set(Some(e));
                        } else {
                            error.set(Some(e));
                        }
                    }
                }
            }
            start_busy.set(false);
        });
    };

    view! {
        <section class="produce-page">
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
            </div>
            <h1>"Produce"</h1>
            <Show when=move || busy_banner.get().is_some()>
                <div class="busy-banner" role="status">
                    <p>{move || busy_banner.get().unwrap_or_default()}</p>
                    <A href=move || {
                        format!("/matters/{}/process", crate::path_id::encode_matter_id(&root_sig.get()))
                    }>"Open Process tab / active job"</A>
                </div>
            </Show>
            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || page.get().is_some()>
                {move || page.get().map(|pg| {
                    let count = pg.default_count;
                    let produced = pg.produced_count;
                    let hint = pg.next_seq_hint;
                    let gate = pg.qc_gate.clone();
                    view! {
                        <div class="produce-layout">
                            <aside class="produce-sets">
                                <h2>"Production sets"</h2>
                                <p class="empty">{format!("{produced} produced item(s)")}</p>
                                <Show when=move || page.get().map(|p| p.sets.is_empty()).unwrap_or(true)>
                                    <p class="empty">"No volumes yet."</p>
                                </Show>
                                <For
                                    each=move || page.get().map(|p| p.sets).unwrap_or_default()
                                    key=|s: &ProductionSetThin| s.id.clone()
                                    children=move |s| {
                                        view! {
                                            <div class="set-row">
                                                <div class="name">{s.name}</div>
                                                <div class="empty">{format!("{} · {} ok · next {}", s.status, s.produced_ok_count, s.next_seq)}</div>
                                            </div>
                                        }
                                    }
                                />
                            </aside>
                            <div class="produce-center">
                                <ol class="produce-steps" start="1">
                                    <li class=move || if step.get() == 1 { "active" } else { "" }>
                                        <button on:click=move |_| { step.set(1); }>"1 Set"</button>
                                    </li>
                                    <li class=move || if step.get() == 2 { "active" } else { "" }>
                                        <button on:click=move |_| { step.set(2); }>"2 Number"</button>
                                    </li>
                                    <li class=move || if step.get() == 3 { "active" } else { "" }>
                                        <button on:click=move |_| { step.set(3); }>"3 Format"</button>
                                    </li>
                                    <li class=move || if step.get() == 4 { "active" } else { "" }>
                                        <button on:click=move |_| { step.set(4); }>"4 Burn"</button>
                                    </li>
                                    <li class=move || if step.get() == 5 { "active" } else { "" }>
                                        <button on:click=move |_| {
                                            step.set(5);
                                            if qc.get().is_none() && !qc_busy.get() {
                                                run_qc();
                                            }
                                        }>"5 Pre-flight"</button>
                                    </li>
                                </ol>

                                <Show when=move || step.get() == 1>
                                    <div class="produce-step">
                                        <h2>"Set"</h2>
                                        <p>"Default search: Responsive NOT withheld."</p>
                                        <p>{if entire_corpus.get() {
                                            "Source: entire review corpus (withhold = false, family together). Count refreshes at QC."
                                        } else {
                                            "Source: responsive AND NOT withheld (family together)."
                                        }}</p>
                                        <Show when=move || !entire_corpus.get()>
                                            <p>{format!("{count} item(s) in the default produce set.")}</p>
                                        </Show>
                                        <label>
                                            <input
                                                type="checkbox"
                                                prop:checked=move || entire_corpus.get()
                                                on:change=move |ev| {
                                                    if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                        entire_corpus.set(el.checked());
                                                        overrides.set(HashMap::new());
                                                        qc.set(None);
                                                    }
                                                }
                                            />
                                            " Entire review corpus (still withhold = false, include family)"
                                        </label>
                                        <p class="empty">"QC gate: " {gate.status.clone()} " — " {gate.message.clone()}</p>
                                    </div>
                                </Show>

                                <Show when=move || step.get() == 2>
                                    <div class="produce-step">
                                        <h2>"Number"</h2>
                                        <label>
                                            "Prefix "
                                            <input
                                                type="text"
                                                prop:value=move || prefix.get()
                                                on:input=move |ev| {
                                                    if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                        prefix.set(el.value());
                                                    }
                                                }
                                            />
                                        </label>
                                        <label>
                                            "Bates start "
                                            <input
                                                type="text"
                                                prop:value=move || bates_start.get()
                                                placeholder="required ≥ 1"
                                                on:input=move |ev| {
                                                    if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                        bates_start.set(el.value());
                                                    }
                                                }
                                            />
                                        </label>
                                        <p class="empty">{match hint {
                                            Some(n) => format!("Next sequence hint for PROD: {n}"),
                                            None => "No prior PROD volume.".into(),
                                        }}</p>
                                        <label>
                                            <input type="checkbox" prop:checked=true prop:disabled=true />
                                            " Family together (locked on)"
                                        </label>
                                        <div class="seg">
                                            <button
                                                class=move || {
                                                    let (_, page_bates) = selected_profile_flags(&page.get(), &profile.get());
                                                    if page_bates { String::new() } else { "primary".into() }
                                                }
                                                on:click=move |_| {
                                                    let slug = page.get().and_then(|pg| {
                                                        pg.profiles.into_iter().find(|x| !x.include_images).map(|x| x.slug)
                                                    }).unwrap_or_else(|| DAT_ONLY_PROFILE.to_string());
                                                    profile.set(slug);
                                                    overrides.set(HashMap::new());
                                                    qc.set(None);
                                                }
                                            >"Doc-level Bates"</button>
                                            <button
                                                class=move || {
                                                    let (_, page_bates) = selected_profile_flags(&page.get(), &profile.get());
                                                    if page_bates { "primary".into() } else { String::new() }
                                                }
                                                disabled=move || {
                                                    page.get().map(|pg| {
                                                        !pg.profiles.iter().any(|x| {
                                                            x.include_images || x.bates_mode.eq_ignore_ascii_case("page")
                                                        })
                                                    }).unwrap_or(true)
                                                }
                                                on:click=move |_| {
                                                    let slug = page.get().and_then(|pg| {
                                                        pg.profiles.into_iter().find(|x| {
                                                            x.slug == IMAGE_OPT_PROFILE
                                                                || (x.include_images && x.bates_mode.eq_ignore_ascii_case("page"))
                                                        }).map(|x| x.slug)
                                                    }).unwrap_or_else(|| IMAGE_OPT_PROFILE.to_string());
                                                    profile.set(slug);
                                                    overrides.set(HashMap::new());
                                                    qc.set(None);
                                                }
                                            >"Page-level Bates"</button>
                                        </div>
                                        <p class="empty">{move || {
                                            let (_, page_bates) = selected_profile_flags(&page.get(), &profile.get());
                                            if page_bates {
                                                "Page-level Bates: BEGBATES is the first page, ENDBATES the last.".to_string()
                                            } else {
                                                "DAT-only profile uses one Bates per native (BEGBATES=ENDBATES).".to_string()
                                            }
                                        }}</p>
                                    </div>
                                </Show>

                                <Show when=move || step.get() == 3>
                                    <div class="produce-step">
                                        <h2>"Format"</h2>
                                        <p>"NATIVES + TEXT + DATA/load.dat on."</p>
                                        <p class="empty">{move || {
                                            let (include_images, _) = selected_profile_flags(&page.get(), &profile.get());
                                            if include_images {
                                                "Single-page TIFF G4 + IMAGE.opt. Spreadsheets and email stay native-only. LFP is not this track.".to_string()
                                            } else {
                                                "DAT-only profile: no IMAGES/ or IMAGE.opt.".to_string()
                                            }
                                        }}</p>
                                        <p class="empty">"Slipsheets off."</p>
                                        <label>
                                            "Production profile "
                                            <select
                                                on:change=move |ev| {
                                                    if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
                                                        profile.set(el.value());
                                                        overrides.set(HashMap::new());
                                                        qc.set(None);
                                                    }
                                                }
                                            >
                                                <For
                                                    each=move || page.get().map(|p| p.profiles).unwrap_or_default()
                                                    key=|p| p.slug.clone()
                                                    children=move |p| {
                                                        let slug = p.slug.clone();
                                                        let label = format!("{} ({})", p.name, p.qc_pack_id);
                                                        view! { <option value=slug>{label}</option> }
                                                    }
                                                />
                                            </select>
                                        </label>
                                        <fieldset>
                                            <legend>"Privilege log format"</legend>
                                            <label>
                                                <input
                                                    type="radio"
                                                    name="privlog"
                                                    prop:checked=move || log_format.get() == "standard"
                                                    on:change=move |_| log_format.set("standard".into())
                                                />
                                                " standard"
                                            </label>
                                            <label>
                                                <input
                                                    type="radio"
                                                    name="privlog"
                                                    prop:checked=move || log_format.get() == "automated_metadata"
                                                    on:change=move |_| log_format.set("automated_metadata".into())
                                                />
                                                " automated_metadata"
                                            </label>
                                            <label class="empty">
                                                <input type="radio" name="privlog" disabled=true />
                                                " category — not implemented (D-0031-03)"
                                            </label>
                                        </fieldset>
                                    </div>
                                </Show>

                                <Show when=move || step.get() == 4>
                                    <div class="produce-step">
                                        <h2>"Burn"</h2>
                                        {move || {
                                            let p = page.get();
                                            let q = qc.get();
                                            let need = q
                                                .as_ref()
                                                .map(|r| r.need_burn)
                                                .or_else(|| p.as_ref().map(|x| x.need_burn))
                                                .unwrap_or(0);
                                            let fresh = q
                                                .as_ref()
                                                .map(|r| r.burned_fresh)
                                                .or_else(|| p.as_ref().map(|x| x.burned_fresh))
                                                .unwrap_or(0);
                                            let unmapped = q
                                                .as_ref()
                                                .map(|r| r.unmapped_text)
                                                .or_else(|| p.as_ref().map(|x| x.unmapped_text))
                                                .unwrap_or(0);
                                            view! {
                                                <p>{format!("Need burn: {need} · Burned fresh: {fresh} · Unmapped text: {unmapped}")}</p>
                                            }
                                        }}
                                        <p>"Highlights never burn. Draft overlays are not the produced native."</p>
                                        <button
                                            on:click=move |_| {
                                                let root = root_sig.get();
                                                let ids = qc
                                                    .get()
                                                    .map(|r| r.ordered_ids)
                                                    .filter(|v| !v.is_empty())
                                                    .unwrap_or_default();
                                                if ids.is_empty() {
                                                    error.set(Some(
                                                        "Run QC first so Burn uses the current selected set.".into(),
                                                    ));
                                                    return;
                                                }
                                                leptos::task::spawn_local(async move {
                                                    match tauri_invoke::<ProduceBurnSet, _>(
                                                        "produce_burn_set",
                                                        &ProduceBurnSetArgs {
                                                            root: root.clone(),
                                                            item_ids: Some(ids),
                                                        },
                                                    ).await {
                                                        Ok(r) => {
                                                            if !r.errors.is_empty() {
                                                                error.set(Some(r.errors.join("; ")));
                                                            }
                                                            match tauri_invoke::<ProducePageResponse, _>(
                                                                "produce_page",
                                                                &RootArgs { root },
                                                            ).await {
                                                                Ok(resp) => page.set(Some(resp)),
                                                                Err(e) => error.set(Some(e)),
                                                            }
                                                        }
                                                        Err(e) => error.set(Some(e)),
                                                    }
                                                });
                                            }
                                        >"Burn selected set"</button>
                                    </div>
                                </Show>

                                <Show when=move || step.get() == 5>
                                    <div class="produce-step">
                                        <h2>"Pre-flight"</h2>
                                        <button
                                            on:click=move |_| run_qc()
                                            disabled=move || qc_busy.get()
                                        >{move || if qc_busy.get() { "Running…" } else { "Re-run QC" }}</button>
                                        {move || qc.get().map(|r| {
                                            let findings = r.findings.clone();
                                            let extras = r.extras.clone();
                                            view! {
                                                <p>{format!("scope={} pack={} errors={} warns={} passed={}", r.scope, r.pack_id, r.error_count, r.warn_count, r.passed)}</p>
                                                <For
                                                    each=move || extras.clone()
                                                    key=|e: &ChromeExtra| format!("{}:{}", e.kind, e.item_id.clone().unwrap_or_default())
                                                    children=move |e| {
                                                        let href = e.item_id.as_ref().map(|id| review_doc_href(&root_sig.get(), id, None, None));
                                                        let is_block = e.severity == "blocker";
                                                        view! {
                                                            <div class=if is_block { "card blocker" } else { "card warn" }>
                                                                <strong>{e.kind}</strong>
                                                                <span class="empty">{e.severity.clone()}</span>
                                                                <p>{e.message}</p>
                                                                {href.map(|h| view! { <A href=h>"Open in review"</A> })}
                                                            </div>
                                                        }
                                                    }
                                                />
                                                <For
                                                    each=move || findings.clone()
                                                    key=|f: &ChromeQcFinding| format!("{}:{}:{}", f.rule_id, f.item_id.clone().unwrap_or_default(), f.severity)
                                                    children=move |f| {
                                                        let href = f.item_id.as_ref().map(|id| review_doc_href(&root_sig.get(), id, None, None));
                                                        let is_err = f.severity == "error";
                                                        let key = override_key(&f.rule_id, f.item_id.as_deref());
                                                        let key_by = key.clone();
                                                        let key_reason = key.clone();
                                                        let override_form = if is_err {
                                                            None
                                                        } else {
                                                            Some(view! {
                                                                <label>
                                                                    "Recorded by "
                                                                    <input
                                                                        type="text"
                                                                        on:input=move |ev| {
                                                                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                                                let v = el.value();
                                                                                let k = key_by.clone();
                                                                                overrides.update(move |m| {
                                                                                    let entry = m.entry(k).or_insert((String::new(), String::new()));
                                                                                    entry.0 = v;
                                                                                });
                                                                            }
                                                                        }
                                                                    />
                                                                </label>
                                                                <label>
                                                                    "Reason "
                                                                    <textarea
                                                                        on:input=move |ev| {
                                                                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()) {
                                                                                let v = el.value();
                                                                                let k = key_reason.clone();
                                                                                overrides.update(move |m| {
                                                                                    let entry = m.entry(k).or_insert((String::new(), String::new()));
                                                                                    entry.1 = v;
                                                                                });
                                                                            }
                                                                        }
                                                                    />
                                                                </label>
                                                            })
                                                        };
                                                        view! {
                                                            <div class=if is_err { "card blocker" } else { "card warn" }>
                                                                <strong>{f.rule_id.clone()}</strong>
                                                                <span class="empty">{f.severity.clone()}</span>
                                                                <p>{f.message.clone()}</p>
                                                                {href.map(|h| view! { <A href=h>"Open in review"</A> })}
                                                                {override_form}
                                                            </div>
                                                        }
                                                    }
                                                />
                                            }
                                        })}
                                    </div>
                                </Show>
                            </div>
                        </div>
                        <div class="produce-foot">
                            <button
                                class="primary"
                                disabled=move || {
                                    if finalize_blocked_by_volume_latch(
                                        volume_succeeded.get(),
                                        start_busy.get(),
                                    ) {
                                        return true;
                                    }
                                    let start_ok = bates_start.get().trim().parse::<u64>().ok().is_some_and(|n| n >= 1);
                                    if !start_ok {
                                        return true;
                                    }
                                    match qc.get() {
                                        None => true,
                                        Some(r) => {
                                            let blockers = r.extras.iter().any(|e| e.severity == "blocker")
                                                || r.findings.iter().any(|f| f.severity == "error")
                                                || r.ordered_ids.is_empty();
                                            if blockers {
                                                return true;
                                            }
                                            let map = overrides.get();
                                            r.findings.iter().filter(|f| f.severity == "warn").any(|f| {
                                                let key = override_key(&f.rule_id, f.item_id.as_deref());
                                                match map.get(&key) {
                                                    Some((by, reason)) => by.trim().is_empty() || reason.trim().is_empty(),
                                                    None => true,
                                                }
                                            })
                                        }
                                    }
                                }
                                on:click=finalize
                            >{move || if start_busy.get() { "Finalizing…" } else { "Finalize" }}</button>
                            {move || start_result.get().and_then(|r| {
                                if r.ok {
                                    Some(view! { <p>"Volume " <code>{r.output_root.unwrap_or_default()}</code></p> })
                                } else {
                                    None
                                }
                            })}
                        </div>
                    }
                })}
            </Show>
        </section>
    }
}
