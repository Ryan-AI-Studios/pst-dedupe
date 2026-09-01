use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::invoke::{
    tauri_invoke, BuiltinProfileFlags, JobProgressSnapshot, ProcessCancelArgs, ProcessErrorGroup,
    ProcessJobRow, ProcessPageArgs, ProcessPageResponse, ProcessPstRow, ProcessResumeArgs,
    ProcessSourceRow, ProcessStartArgs, ProcessStartResponse, RootArgs,
};
use crate::path_id::encode_matter_id;

const DEFAULT_PROFILE: &str = "builtin:standard";

fn ingest_params(path: &str) -> String {
    serde_json::json!({ "path": path }).to_string()
}

fn extract_params(source_id: &str, pst_item_id: &str) -> String {
    serde_json::json!({
        "source_id": source_id,
        "pst_item_id": pst_item_id,
    })
    .to_string()
}

fn profile_params(profile_id: &str) -> String {
    serde_json::json!({
        "profile_id": profile_id,
        "stop_on_stage_failure": true
    })
    .to_string()
}

#[derive(Clone, PartialEq)]
struct ExtractWork {
    source_id: String,
    pst_item_id: String,
    name: String,
}

async fn pick_path(
    directory: bool,
    filters: Option<Vec<(String, Vec<String>)>>,
) -> Result<Option<String>, String> {
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &"directory".into(), &JsValue::from_bool(directory))
        .map_err(|e| format!("{e:?}"))?;
    js_sys::Reflect::set(&opts, &"multiple".into(), &JsValue::FALSE)
        .map_err(|e| format!("{e:?}"))?;
    if let Some(filters) = filters {
        let arr = js_sys::Array::new();
        for (name, exts) in filters {
            let f = js_sys::Object::new();
            js_sys::Reflect::set(&f, &"name".into(), &JsValue::from_str(&name))
                .map_err(|e| format!("{e:?}"))?;
            let ext_arr = js_sys::Array::new();
            for e in exts {
                ext_arr.push(&JsValue::from_str(&e));
            }
            js_sys::Reflect::set(&f, &"extensions".into(), &ext_arr.into())
                .map_err(|e| format!("{e:?}"))?;
            arr.push(&f.into());
        }
        js_sys::Reflect::set(&opts, &"filters".into(), &arr.into())
            .map_err(|e| format!("{e:?}"))?;
    }
    let promise = dialog_open(opts.into()).map_err(|e| format!("{e:?}"))?;
    let value = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| format!("{e:?}"))?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    if let Some(s) = value.as_string() {
        return Ok(Some(s));
    }
    let arr = js_sys::Array::from(&value);
    Ok(arr.get(0).as_string())
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "dialog"], js_name = open, catch)]
    fn dialog_open(options: JsValue) -> Result<js_sys::Promise, JsValue>;
}

fn dash(v: Option<u64>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "—".into(),
    }
}

fn job_indent(parent: &Option<String>) -> &'static str {
    if parent.is_some() {
        "job-row child"
    } else {
        "job-row"
    }
}

fn snapshot_busy(snap: &JobProgressSnapshot) -> bool {
    !snap.job_id.is_empty()
        && snap.state != "idle"
        && snap.state != "succeeded"
        && snap.state != "failed"
        && snap.state != "cancelled"
        && snap.state != "paused"
}

fn spawn_cancel(job_id: String) {
    if job_id.is_empty() {
        return;
    }
    leptos::task::spawn_local(async move {
        let _ = tauri_invoke::<(), _>("process_cancel", &ProcessCancelArgs { job_id }).await;
    });
}

fn spawn_resume(root: String, job_id: String) {
    if root.is_empty() || job_id.is_empty() {
        return;
    }
    leptos::task::spawn_local(async move {
        let _ = tauri_invoke::<(), _>("process_resume", &ProcessResumeArgs { root, job_id }).await;
    });
}

fn is_orphan_running(job: &ProcessJobRow, snap: &JobProgressSnapshot) -> bool {
    job.state == "running"
        && (snap.job_id.is_empty() || snap.state == "idle" || snap.job_id != job.id)
}

fn extract_all_should_start(queue_len: usize, snapshot_busy: bool) -> bool {
    queue_len == 0 && !snapshot_busy
}

fn is_busy_invoke_err(e: &str) -> bool {
    e.starts_with("busy:") || e.contains("matter is busy")
}

fn should_clear_queue_on_start_err(err: &str) -> bool {
    !is_busy_invoke_err(err)
}

fn should_set_busy_retry(err: &str) -> bool {
    is_busy_invoke_err(err)
}

fn should_fire_busy_retry(pending: bool, snapshot_busy: bool) -> bool {
    pending && !snapshot_busy
}

fn take_busy_retry_fire(pending: &mut bool, snapshot_busy: bool) -> bool {
    if should_fire_busy_retry(*pending, snapshot_busy) {
        *pending = false;
        true
    } else {
        false
    }
}

fn should_clear_busy_retry(started_ok: bool, finished_paused: bool, non_busy_clear: bool) -> bool {
    started_ok || finished_paused || non_busy_clear
}

fn snapshot_clears_busy_retry(state: &str) -> bool {
    state == "paused" || state == "cancelled"
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractStartErrEffect {
    clear_queue: bool,
    zero_total: bool,
    set_retry: bool,
    clear_retry: bool,
    surface_error: bool,
}

fn extract_start_err_effect(err: &str, zero_total_on_clear: bool) -> ExtractStartErrEffect {
    if should_set_busy_retry(err) {
        ExtractStartErrEffect {
            clear_queue: false,
            zero_total: false,
            set_retry: true,
            clear_retry: false,
            surface_error: false,
        }
    } else {
        ExtractStartErrEffect {
            clear_queue: should_clear_queue_on_start_err(err),
            zero_total: zero_total_on_clear,
            set_retry: false,
            clear_retry: should_clear_busy_retry(false, false, true),
            surface_error: true,
        }
    }
}

fn apply_extract_start_err(
    err: String,
    extract_queue: RwSignal<Vec<ExtractWork>>,
    extract_total: RwSignal<u64>,
    busy_retry_pending: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    zero_total: bool,
) {
    let effect = extract_start_err_effect(&err, zero_total);
    if effect.set_retry {
        busy_retry_pending.set(true);
    }
    if effect.clear_queue {
        extract_queue.set(Vec::new());
        if effect.zero_total {
            extract_total.set(0);
        }
    }
    if effect.clear_retry {
        busy_retry_pending.set(false);
    }
    if effect.surface_error {
        error.set(Some(err));
    }
}

#[component]
pub fn ProcessPage() -> impl IntoView {
    let params = use_params_map();
    let navigate = StoredValue::new(use_navigate());
    let root_sig = RwSignal::new(String::new());
    let error = RwSignal::new(Option::<String>::None);
    let page = RwSignal::new(Option::<ProcessPageResponse>::None);
    let progress = RwSignal::new(JobProgressSnapshot {
        job_id: String::new(),
        kind: String::new(),
        matter_id: String::new(),
        state: "idle".into(),
        stage: None,
        completed_count: 0,
        total_hint: None,
        message: None,
        error_summary: None,
        updated_at: String::new(),
    });
    let selected_profile = RwSignal::new(DEFAULT_PROFILE.to_string());
    let selected_pst = RwSignal::new(Option::<String>::None);
    let extract_queue = RwSignal::new(Vec::<ExtractWork>::new());
    let extract_done = RwSignal::new(0u64);
    let extract_total = RwSignal::new(0u64);
    let extract_note = RwSignal::new(Option::<String>::None);
    let extract_current_name = RwSignal::new(String::new());
    let busy_retry_pending = RwSignal::new(false);

    let id_encoded = move || encode_matter_id(&root_sig.get());

    let reload = move |root: String| {
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ProcessPageResponse, _>("process_page", &ProcessPageArgs { root })
                .await
            {
                Ok(resp) => {
                    page.set(Some(resp));
                    error.set(None);
                }
                Err(e) => {
                    page.set(None);
                    error.set(Some(e));
                }
            }
        });
    };

    Effect::new(move |_| {
        let root = params.with(|p| p.get("id").unwrap_or_default());
        if root.is_empty() {
            error.set(Some("Missing matter id in route.".into()));
            return;
        }
        root_sig.set(root.clone());
        reload(root);
    });

    Effect::new(move |_| {
        let root = root_sig.get();
        if root.is_empty() {
            return;
        }
        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let cb = Closure::wrap(Box::new(move || {
            let root = root_sig.get_untracked();
            if root.is_empty() {
                return;
            }
            leptos::task::spawn_local(async move {
                match tauri_invoke::<JobProgressSnapshot, _>(
                    "process_progress",
                    &RootArgs { root: root.clone() },
                )
                .await
                {
                    Ok(snap) => {
                        let was_busy = snapshot_busy(&progress.get_untracked());
                        let finished_failed =
                            was_busy && !snapshot_busy(&snap) && snap.state == "failed";
                        let finished_paused = was_busy
                            && !snapshot_busy(&snap)
                            && (snap.state == "paused" || snap.state == "cancelled");
                        let finished_ok = was_busy && !snapshot_busy(&snap);
                        let missing_job = snapshot_busy(&snap)
                            && page
                                .get_untracked()
                                .map(|p| !p.jobs.iter().any(|j| j.id == snap.job_id))
                                .unwrap_or(true);
                        let mut pending = busy_retry_pending.get_untracked();
                        if should_clear_busy_retry(
                            false,
                            snapshot_clears_busy_retry(&snap.state),
                            false,
                        ) {
                            pending = false;
                        }
                        let fire_retry = take_busy_retry_fire(&mut pending, snapshot_busy(&snap));
                        busy_retry_pending.set(pending);
                        progress.set(snap.clone());
                        if finished_ok || missing_job {
                            let prev_exceptions =
                                page.get_untracked().map(|p| p.exceptions).unwrap_or(0);
                            match tauri_invoke::<ProcessPageResponse, _>(
                                "process_page",
                                &ProcessPageArgs { root: root.clone() },
                            )
                            .await
                            {
                                Ok(resp) => {
                                    let raised = resp.exceptions.saturating_sub(prev_exceptions);
                                    page.set(Some(resp));
                                    if finished_ok {
                                        let total = extract_total.get_untracked();
                                        if total > 0 && snap.kind == "extract_pst" {
                                            let done =
                                                (extract_done.get_untracked() + 1).min(total);
                                            extract_done.set(done);
                                            let name = extract_current_name.get_untracked();
                                            if finished_failed {
                                                extract_note.set(Some(format!(
                                                    "{done} of {total} extracted; {name} raised {raised} exceptions."
                                                )));
                                            } else {
                                                extract_note.set(Some(format!(
                                                    "{done} of {total} extracted."
                                                )));
                                            }
                                        }
                                    }
                                }
                                Err(e) => error.set(Some(e)),
                            }
                            let mut q = extract_queue.get_untracked();
                            if finished_ok
                                && snap.kind == "extract_pst"
                                && !finished_paused
                                && !q.is_empty()
                                && !fire_retry
                            {
                                q.remove(0);
                                extract_queue.set(q.clone());
                                if let Some(work) = q.first().cloned() {
                                    extract_current_name.set(work.name.clone());
                                    let params_json =
                                        extract_params(&work.source_id, &work.pst_item_id);
                                    match tauri_invoke::<ProcessStartResponse, _>(
                                        "process_start",
                                        &ProcessStartArgs {
                                            root: root.clone(),
                                            kind: "extract_pst".into(),
                                            params_json,
                                        },
                                    )
                                    .await
                                    {
                                        Ok(_) => {
                                            if should_clear_busy_retry(true, false, false) {
                                                busy_retry_pending.set(false);
                                            }
                                        }
                                        Err(e) => apply_extract_start_err(
                                            e,
                                            extract_queue,
                                            extract_total,
                                            busy_retry_pending,
                                            error,
                                            false,
                                        ),
                                    }
                                }
                            }
                        }
                        if fire_retry {
                            if let Some(work) = extract_queue.get_untracked().first().cloned() {
                                extract_current_name.set(work.name.clone());
                                let params_json =
                                    extract_params(&work.source_id, &work.pst_item_id);
                                match tauri_invoke::<ProcessStartResponse, _>(
                                    "process_start",
                                    &ProcessStartArgs {
                                        root: root.clone(),
                                        kind: "extract_pst".into(),
                                        params_json,
                                    },
                                )
                                .await
                                {
                                    Ok(_) => {
                                        if should_clear_busy_retry(true, false, false) {
                                            busy_retry_pending.set(false);
                                        }
                                        error.set(None);
                                    }
                                    Err(e) => apply_extract_start_err(
                                        e,
                                        extract_queue,
                                        extract_total,
                                        busy_retry_pending,
                                        error,
                                        false,
                                    ),
                                }
                            } else if should_clear_busy_retry(false, false, true) {
                                busy_retry_pending.set(false);
                            }
                        }
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }) as Box<dyn FnMut()>);
        let id = window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                400,
            )
            .ok();
        cb.forget();
        on_cleanup(move || {
            if let (Some(w), Some(id)) = (web_sys::window(), id) {
                w.clear_interval_with_handle(id);
            }
        });
    });

    let start_kind = move |kind: String, params_json: String| {
        let root = root_sig.get();
        if root.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ProcessStartResponse, _>(
                "process_start",
                &ProcessStartArgs {
                    root: root.clone(),
                    kind,
                    params_json,
                },
            )
            .await
            {
                Ok(_) => {
                    error.set(None);
                    reload(root);
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let add_folder = move |_| {
        leptos::task::spawn_local(async move {
            match pick_path(true, None).await {
                Ok(Some(path)) => start_kind("ingest".into(), ingest_params(&path)),
                Ok(None) => {}
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let add_zip_pst = move |_| {
        leptos::task::spawn_local(async move {
            match pick_path(
                false,
                Some(vec![(
                    "ZIP or PST".into(),
                    vec!["zip".into(), "pst".into()],
                )]),
            )
            .await
            {
                Ok(Some(path)) => start_kind("ingest".into(), ingest_params(&path)),
                Ok(None) => {}
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let extract_selected = move |_| {
        let Some(pg) = page.get() else {
            return;
        };
        let Some(id) = selected_pst.get() else {
            error.set(Some("Select a PST in inventory first.".into()));
            return;
        };
        let Some(row) = pg.pst_inventory.iter().find(|p| p.id == id) else {
            return;
        };
        let sid = row.source_id.clone().unwrap_or_default();
        start_kind("extract_pst".into(), extract_params(&sid, &row.id));
    };

    let extract_all = move |_| {
        if !extract_all_should_start(extract_queue.get().len(), snapshot_busy(&progress.get())) {
            return;
        }
        let Some(pg) = page.get() else {
            return;
        };
        if pg.pst_inventory.is_empty() {
            error.set(Some("No PST inventory leaves to extract.".into()));
            return;
        }
        let q: Vec<ExtractWork> = pg
            .pst_inventory
            .iter()
            .map(|p| ExtractWork {
                source_id: p.source_id.clone().unwrap_or_default(),
                pst_item_id: p.id.clone(),
                name: p.path.clone().unwrap_or_else(|| p.id.clone()),
            })
            .collect();
        let total = q.len() as u64;
        let first = q[0].clone();
        extract_total.set(total);
        extract_done.set(0);
        extract_note.set(None);
        extract_current_name.set(first.name.clone());
        extract_queue.set(q);
        let root = root_sig.get();
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ProcessStartResponse, _>(
                "process_start",
                &ProcessStartArgs {
                    root: root.clone(),
                    kind: "extract_pst".into(),
                    params_json: extract_params(&first.source_id, &first.pst_item_id),
                },
            )
            .await
            {
                Ok(_) => {
                    error.set(None);
                    if should_clear_busy_retry(true, false, false) {
                        busy_retry_pending.set(false);
                    }
                    reload(root);
                }
                Err(e) => apply_extract_start_err(
                    e,
                    extract_queue,
                    extract_total,
                    busy_retry_pending,
                    error,
                    true,
                ),
            }
        });
    };

    let run_profile = move |_| {
        start_kind(
            "profile_run".into(),
            profile_params(&selected_profile.get()),
        );
    };

    view! {
        <section class="process-page">
            <h1>"Process"</h1>
            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <div class="process-layout">
                <aside class="process-pane">
                    <h2>"Sources"</h2>
                    <div class="cta-row">
                        <button on:click=add_folder>"Add folder"</button>
                        <button on:click=add_zip_pst>"Add ZIP / PST"</button>
                    </div>
                    <Show when=move || page.get().map(|p| p.sources.is_empty()).unwrap_or(true)>
                        <p class="empty">"No sources yet. Add a Purview folder, ZIP, or PST."</p>
                    </Show>
                    <For
                        each=move || page.get().map(|p| p.sources).unwrap_or_default()
                        key=|s: &ProcessSourceRow| s.id.clone()
                        children=move |s| {
                            view! {
                                <div class="set-row">
                                    <div class="name">{s.path}</div>
                                    <div class="empty">{format!("{} · {}", s.kind, s.status)}</div>
                                </div>
                            }
                        }
                    />
                    <h2>"PST inventory"</h2>
                    <Show when=move || page.get().map(|p| p.pst_inventory.is_empty()).unwrap_or(true)>
                        <p class="empty">"No PST leaves. Ingest a PST or skip extract."</p>
                    </Show>
                    <For
                        each=move || page.get().map(|p| p.pst_inventory).unwrap_or_default()
                        key=|p: &ProcessPstRow| p.id.clone()
                        children=move |p| {
                            let id = p.id.clone();
                            let id_sel = id.clone();
                            view! {
                                <label class="set-row">
                                    <input
                                        type="radio"
                                        name="pst-inv"
                                        prop:checked=move || selected_pst.get().as_deref() == Some(id.as_str())
                                        on:change=move |_| selected_pst.set(Some(id_sel.clone()))
                                    />
                                    <span>{p.path.clone().unwrap_or_else(|| p.id.clone())}</span>
                                </label>
                            }
                        }
                    />
                    <div class="cta-row">
                        <button on:click=extract_selected>"Extract selected"</button>
                        <button
                            on:click=extract_all
                            disabled=move || {
                                snapshot_busy(&progress.get()) || busy_retry_pending.get()
                            }
                        >
                            "Extract all"
                        </button>
                    </div>
                    <Show when=move || extract_note.get().is_some()>
                        <p class="empty">{move || extract_note.get().unwrap_or_default()}</p>
                    </Show>
                    <h2>"Profile"</h2>
                    <For
                        each=move || page.get().map(|p| p.builtins).unwrap_or_default()
                        key=|b: &BuiltinProfileFlags| b.id.clone()
                        children=move |b| {
                            let id = b.id.clone();
                            let id_click = id.clone();
                            let checks = [
                                ("classify", b.classify),
                                ("office/pdf/ics", b.office_extract || b.pdf_extract || b.ics_extract),
                                ("OCR", b.ocr),
                                ("FTS", b.fts),
                                ("dedupe", b.dedupe),
                                ("thread", b.thread),
                                ("neardup", b.neardup),
                                ("cull", b.cull),
                                ("promote", b.promote),
                            ];
                            view! {
                                <button
                                    class=move || if selected_profile.get() == id { "chip-btn active" } else { "chip-btn" }
                                    on:click=move |_| selected_profile.set(id_click.clone())
                                >
                                    {b.name.clone()}
                                </button>
                                <p class="empty">
                                    {checks.into_iter().map(|(label, on)| {
                                        format!("{}{label}  ", if on { "✓ " } else { "○ " })
                                    }).collect::<String>()}
                                </p>
                            }
                        }
                    />
                    <button class="primary" on:click=run_profile>"Run profile"</button>
                </aside>
                <div class="process-pane">
                    <h2>"Jobs"</h2>
                    <Show when=move || page.get().map(|p| p.jobs.is_empty()).unwrap_or(true)>
                        <p class="empty">"No jobs yet. Ingest or run a profile."</p>
                    </Show>
                    <For
                        each=move || page.get().map(|p| p.jobs).unwrap_or_default()
                        key=|j: &ProcessJobRow| j.id.clone()
                        children=move |j| {
                            let job_for_orphan = j.clone();
                            let job_id_for_counts = j.id.clone();
                            let job_id_for_active = j.id.clone();
                            let job_id = StoredValue::new(j.id.clone());
                            view! {
                                <div class=job_indent(&j.parent_job_id)>
                                    <div class="name">{format!("{} · {}", j.kind, j.state)}</div>
                                    <div class="empty">{move || {
                                        let snap = progress.get();
                                        if snap.job_id == job_id_for_counts {
                                            format!(
                                                "{}/{}",
                                                snap.completed_count,
                                                snap.total_hint.map(|n| n.to_string()).unwrap_or_else(|| "—".into())
                                            )
                                        } else {
                                            "—".into()
                                        }
                                    }}</div>
                                    <Show when=move || {
                                        let snap = progress.get();
                                        snap.job_id == job_id_for_active && snapshot_busy(&snap)
                                    }>
                                        <button on:click=move |_| {
                                            spawn_cancel(job_id.get_value());
                                        }>"Pause"</button>
                                    </Show>
                                    <Show when=move || {
                                        is_orphan_running(&job_for_orphan, &progress.get())
                                    }>
                                        <button class="primary" on:click=move |_| {
                                            spawn_resume(root_sig.get(), job_id.get_value());
                                        }>"Resume"</button>
                                        <button on:click=move |_| {
                                            spawn_cancel(job_id.get_value());
                                        }>"Cancel"</button>
                                    </Show>
                                </div>
                            }
                        }
                    />
                    <Show when=move || snapshot_busy(&progress.get())>
                        <p class="empty">{move || {
                            let s = progress.get();
                            format!(
                                "{} {} · {} · {}",
                                s.kind,
                                s.state,
                                s.stage.unwrap_or_else(|| "—".into()),
                                s.message.unwrap_or_default()
                            )
                        }}</p>
                        <button on:click=move |_| {
                            spawn_cancel(progress.get_untracked().job_id);
                        }>"Pause"</button>
                    </Show>
                    <h2>"Exceptions"</h2>
                    <p class="empty">"Exceptions hold their items; they do not stall sibling extract."</p>
                    <Show when=move || page.get().map(|p| p.error_groups.is_empty()).unwrap_or(true)>
                        <p class="empty">"No item_errors recorded."</p>
                    </Show>
                    <For
                        each=move || page.get().map(|p| p.error_groups).unwrap_or_default()
                        key=|g: &ProcessErrorGroup| g.code.clone()
                        children=move |g| {
                            view! {
                                <div class="set-row">
                                    <div class="name">{format!("{} · {}", g.code, g.count)}</div>
                                    <div class="empty">{g.sample_message}</div>
                                </div>
                            }
                        }
                    />
                </div>
                <aside class="process-pane">
                    <h2>"Running report"</h2>
                    <div class="chip-strip">
                        <div class="chip">
                            <span class="label">"Discovered"</span>
                            <span class="value">{move || page.get().map(|p| p.discovered).unwrap_or(0)}</span>
                        </div>
                        <div class="chip">
                            <span class="label">"Exceptions"</span>
                            <span class="value">{move || page.get().map(|p| p.exceptions).unwrap_or(0)}</span>
                        </div>
                        <div class="chip">
                            <span class="label">"Review-ready"</span>
                            <span class="value">{move || page.get().map(|p| p.in_review).unwrap_or(0)}</span>
                        </div>
                        <div class="chip">
                            <span class="label">"Still processing"</span>
                            <span class="value">{move || page.get().map(|p| p.still_processing).unwrap_or(0)}</span>
                        </div>
                        <div class="chip">
                            <span class="label">"Unaccounted-for"</span>
                            <span class="value">{move || page.get().map(|p| p.unaccounted_for).unwrap_or(0)}</span>
                        </div>
                        <div class="chip">
                            <span class="label">"DeNIST"</span>
                            <span class="value">{move || dash(page.get().and_then(|p| p.denist))}</span>
                        </div>
                        <div class="chip">
                            <span class="label">"Dupes"</span>
                            <span class="value">{move || dash(page.get().and_then(|p| p.dupes))}</span>
                        </div>
                    </div>
                    <button
                        class="primary"
                        disabled=move || page.get().map(|p| p.in_review == 0).unwrap_or(true)
                        on:click=move |_| {
                            let id = id_encoded();
                            navigate.with_value(|nav| {
                                nav(&format!("/matters/{id}/review"), Default::default());
                            });
                        }
                    >
                        "Open review-ready"
                    </button>
                    <p class="empty">{move || format!("Profile {}", selected_profile.get())}</p>
                    <p class="empty">"Identity is SHA-256."</p>
                </aside>
            </div>
        </section>
    }
}

#[cfg(test)]
mod extract_all_busy_tests {
    use super::*;

    fn snap(job_id: &str, state: &str) -> JobProgressSnapshot {
        JobProgressSnapshot {
            job_id: job_id.into(),
            kind: "extract_pst".into(),
            matter_id: "m".into(),
            state: state.into(),
            stage: None,
            completed_count: 3,
            total_hint: Some(10),
            message: None,
            error_summary: None,
            updated_at: String::new(),
        }
    }

    fn job(id: &str, state: &str) -> ProcessJobRow {
        ProcessJobRow {
            id: id.into(),
            kind: "extract_pst".into(),
            state: state.into(),
            parent_job_id: None,
            error_summary: None,
            started_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn extract_all_should_start_only_when_idle_and_queue_empty() {
        assert!(extract_all_should_start(0, false));
        assert!(!extract_all_should_start(2, false));
        assert!(!extract_all_should_start(0, true));
        assert!(!extract_all_should_start(1, true));
    }

    #[test]
    fn is_busy_invoke_err_matches_produce_predicate() {
        assert!(is_busy_invoke_err(
            "busy: matter is busy: a job is already running (job_1)"
        ));
        assert!(is_busy_invoke_err(
            "matter is busy: a job is already running"
        ));
        assert!(!is_busy_invoke_err("failed: extract boom"));
        assert!(!is_busy_invoke_err(""));
        assert!(should_set_busy_retry(
            "busy: matter is busy: a job is already running (job_1)"
        ));
        assert!(!should_clear_queue_on_start_err(
            "busy: matter is busy: a job is already running (job_1)"
        ));
        assert!(should_clear_queue_on_start_err("failed: extract boom"));
    }

    #[test]
    fn extract_start_err_effect_keeps_queue_on_busy() {
        let work = ExtractWork {
            source_id: "s".into(),
            pst_item_id: "p1".into(),
            name: "one.pst".into(),
        };
        let mut queue = vec![work.clone(), work];
        let mut total = 2u64;
        let mut pending = false;
        let mut note: Option<String> = None;
        let busy = extract_start_err_effect(
            "busy: matter is busy: a job is already running (job_1)",
            true,
        );
        if busy.clear_queue {
            queue.clear();
            if busy.zero_total {
                total = 0;
            }
        }
        if busy.set_retry {
            pending = true;
        }
        if busy.clear_retry {
            pending = false;
        }
        if busy.surface_error {
            note = Some("failed".into());
        }
        assert_eq!(queue.len(), 2);
        assert_eq!(total, 2);
        assert!(pending);
        assert!(note.is_none());
        let fail = extract_start_err_effect("failed: extract boom", true);
        if fail.clear_queue {
            queue.clear();
            if fail.zero_total {
                total = 0;
            }
        }
        if fail.clear_retry {
            pending = false;
        }
        if fail.surface_error {
            note = Some("failed: extract boom".into());
        }
        assert!(queue.is_empty());
        assert_eq!(total, 0);
        assert!(!pending);
        assert_eq!(note.as_deref(), Some("failed: extract boom"));
    }

    #[test]
    fn busy_retry_state_machine() {
        assert!(should_set_busy_retry("busy: matter is busy"));
        assert!(!should_fire_busy_retry(true, true));
        assert!(should_fire_busy_retry(true, false));
        assert!(!should_fire_busy_retry(false, false));
        assert!(should_clear_busy_retry(true, false, false));
        assert!(should_clear_busy_retry(false, true, false));
        assert!(should_clear_busy_retry(false, false, true));
        assert!(!should_clear_busy_retry(false, false, false));
        assert!(
            !snapshot_busy(&snap("j1", "paused")),
            "paused must not look busy or Pause would auto-retry remaining PSTs"
        );
        assert!(
            should_fire_busy_retry(true, snapshot_busy(&snap("j1", "paused"))),
            "without clearing the flag, Pause would fire a retry; wire clears on paused/cancelled first"
        );
        assert!(snapshot_clears_busy_retry("paused"));
        assert!(snapshot_clears_busy_retry("cancelled"));
        assert!(!snapshot_clears_busy_retry("succeeded"));
        assert!(!snapshot_clears_busy_retry("running"));
        assert!(!snapshot_clears_busy_retry("idle"));
        let idle_then_cancelled = snap("j_block", "cancelled");
        let mut pending = true;
        if should_clear_busy_retry(
            false,
            snapshot_clears_busy_retry(&idle_then_cancelled.state),
            false,
        ) {
            pending = false;
        }
        assert!(
            !should_fire_busy_retry(pending, snapshot_busy(&idle_then_cancelled)),
            "Busy-while-idle then Cancel before first poll must not auto-start q.first()"
        );
        let mut overlapping = true;
        let first = take_busy_retry_fire(&mut overlapping, false);
        let second = take_busy_retry_fire(&mut overlapping, false);
        assert!(first);
        assert!(!second);
        assert!(!overlapping);
        assert!(!snapshot_busy(&snap("j1", "paused")));
        assert!(!snapshot_busy(&snap("j1", "cancelled")));
        assert!(!snapshot_busy(&snap("j1", "succeeded")));
        assert!(!snapshot_busy(&snap("", "idle")));
        assert!(snapshot_busy(&snap("j1", "running")));
    }

    #[test]
    fn matching_running_snap_is_not_orphan() {
        let running = job("job_live", "running");
        let matching = snap("job_live", "running");
        assert!(!is_orphan_running(&running, &matching));
        assert!(snapshot_busy(&matching));
        let idle = snap("", "idle");
        assert!(is_orphan_running(&running, &idle));
        let other = snap("job_other", "running");
        assert!(is_orphan_running(&running, &other));
        let succeeded = job("job_live", "succeeded");
        assert!(!is_orphan_running(&succeeded, &idle));
    }

    #[test]
    fn extract_all_and_rows_wire_helpers_not_one_shot_bools() {
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let extract_all = prod
            .split("let extract_all = move |_|")
            .nth(1)
            .unwrap_or("");
        let guard = extract_all
            .find("extract_all_should_start")
            .expect("extract_all must call extract_all_should_start");
        let queue_write = extract_all
            .find("extract_queue.set(q)")
            .expect("extract_all still writes the queue after the guard");
        assert!(
            guard < queue_write,
            "extract_all must guard before rebuilding extract_queue"
        );
        assert!(prod.contains("busy_retry_pending"));
        assert!(prod.contains("should_fire_busy_retry"));
        assert!(prod.contains("apply_extract_start_err"));
        let poller = prod.split("Ok(snap) =>").nth(1).unwrap_or("");
        let clear_paused = poller
            .find("snapshot_clears_busy_retry(&snap.state)")
            .expect("poller must clear busy_retry on paused/cancelled snapshots");
        let consume = poller
            .find("take_busy_retry_fire")
            .expect("poller must consume busy_retry_pending before any retry await");
        let page_await = poller
            .find("if finished_ok || missing_job")
            .expect("poller still reloads the page after a terminal job");
        assert!(
            clear_paused < consume,
            "Pause/Cancel must clear busy_retry_pending before a retry can be taken"
        );
        assert!(
            consume < page_await,
            "retry flag must be consumed before process_page/process_start awaits"
        );
        assert!(
            prod.matches("extract_queue.set(Vec::new())").count() == 1,
            "queue wipe must live only in apply_extract_start_err (Busy keep-queue)"
        );
        let drain = prod.split("q.remove(0)").nth(1).unwrap_or("");
        assert!(
            drain.contains("apply_extract_start_err"),
            "drain next-start Err must keep-queue via apply_extract_start_err"
        );
        assert!(
            prod.contains("extract_start_err_effect"),
            "Busy keep-queue must go through extract_start_err_effect"
        );
        assert!(
            !prod.contains("let orphan = is_orphan_running"),
            "job For must not freeze orphan at child create"
        );
        assert!(
            !prod.contains("let active = snap.job_id == j.id"),
            "job For must not freeze active at child create"
        );
        assert!(
            !prod.contains("let counts = if active"),
            "job For must not freeze counts at child create"
        );
        assert!(
            prod.contains("is_orphan_running(&job_for_orphan, &progress.get())"),
            "orphan Show must read live progress"
        );
    }
}
