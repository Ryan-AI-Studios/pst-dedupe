use std::collections::HashMap;

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};

use crate::invoke::{
    tauri_invoke, ChromeExtra, ChromeQcFinding, ProduceBurnSet, ProduceBurnSetArgs,
    ProducePageResponse, ProduceQcRun, ProduceQcRunArgs, ProduceStart, ProduceStartArgs,
    ProductionSetThin, RootArgs, WarningOverride,
};
use crate::path_id::{matter_home_href_from_param, review_doc_href};

fn override_key(rule_id: &str, item_id: Option<&str>) -> String {
    match item_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => format!("{rule_id}\u{1f}{id}"),
        None => format!("{rule_id}\u{1f}*"),
    }
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
    let overrides = RwSignal::new(HashMap::<String, (String, String)>::new());

    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));

    Effect::new(move |_| {
        let root = params.with(|p| p.get("id").unwrap_or_default());
        if root.is_empty() {
            error.set(Some("Missing matter id in route.".into()));
            return;
        }
        root_sig.set(root.clone());
        error.set(None);
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ProducePageResponse, _>("produce_page", &RootArgs { root }).await {
                Ok(resp) => {
                    prefix.set(resp.bates_prefix.clone());
                    if let Some(first) = resp.profiles.first() {
                        profile.set(first.slug.clone());
                    }
                    page.set(Some(resp));
                }
                Err(e) => {
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
            let res = tauri_invoke::<ProduceQcRun, _>(
                "produce_qc_run",
                &ProduceQcRunArgs {
                    root,
                    filter_json: None,
                    item_ids: None,
                    production_profile: Some(prof),
                    source_entire_corpus: Some(entire),
                },
            )
            .await;
            qc_busy.set(false);
            match res {
                Ok(r) => {
                    overrides.set(HashMap::new());
                    qc.set(Some(r));
                    start_result.set(None);
                }
                Err(e) => error.set(Some(e)),
            }
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
        if start_busy.get() {
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
            let res = tauri_invoke::<ProduceStart, _>(
                "produce_start",
                &ProduceStartArgs {
                    root,
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
            start_busy.set(false);
            match res {
                Ok(r) => {
                    start_result.set(Some(r.clone()));
                    if !r.ok {
                        error.set(Some("Finalize blocked — see pre-flight cards.".into()));
                    }
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    view! {
        <section class="produce-page">
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
            </div>
            <h1>"Produce"</h1>
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
                                            <button class="primary" disabled=true>"Doc-level Bates"</button>
                                            <button disabled=true title="Page-level Bates ships with image productions (0115).">"Page-level Bates"</button>
                                        </div>
                                        <p class="empty">"Page-level Bates ships with image productions (0115). This DAT volume uses one Bates per native (BEGBATES=ENDBATES)."</p>
                                    </div>
                                </Show>

                                <Show when=move || step.get() == 3>
                                    <div class="produce-step">
                                        <h2>"Format"</h2>
                                        <p>"NATIVES + TEXT + DATA/load.dat on."</p>
                                        <p class="empty">"TIFF / PDF image / OPT off — ships in 0115. Do not create IMAGES/ or IMAGE.opt."</p>
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
                                    if start_busy.get() {
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
