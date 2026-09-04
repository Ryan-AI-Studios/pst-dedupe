use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlSelectElement;

use crate::invoke::{
    tauri_invoke, BuiltinProfileFlags, JobProgressSnapshot, ProcessCancelArgs, ProcessErrorGroup,
    ProcessExportReportArgs, ProcessExportReportResponse, ProcessJobRow, ProcessPageArgs,
    ProcessPageResponse, ProcessPstRow, ProcessResumeArgs, ProcessSourceRow, ProcessStartArgs,
    ProcessStartResponse, RootArgs,
};
use crate::path_id::{encode_matter_id, review_doc_href};
use crate::shell::ProcessChromeCtx;

const DEFAULT_PROFILE: &str = "builtin:standard";
const DROP_COPY_KINDS: &str = "PST · ZIP · Purview package · folder";
const DROP_COPY_HASH: &str = "Hashed on arrival.";
const DENIST_NSRL_NOTE: &str = "optional local hash-list (NSRL RDS not this track).";
const EXCEPTIONS_NO_VAULT: &str = "Exclude is not available. Encrypted stores fail closed (no password vault).";
const FILE_DROP_EVENT: &str = "process-file-drop";

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
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    fn tauri_event_listen(event: &str, handler: &js_sys::Function) -> Result<js_sys::Promise, JsValue>;
}

fn dash(v: Option<u64>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "—".into(),
    }
}

fn strip_extended_path(path: &str) -> String {
    const UNC: &str = r"\\?\UNC\";
    const PREFIX: &str = r"\\?\";
    if let Some(rest) = path.strip_prefix(UNC) {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(PREFIX) {
        rest.to_string()
    } else {
        path.to_string()
    }
}

fn path_basename(path: &str) -> String {
    let stripped = strip_extended_path(path);
    stripped
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(stripped.as_str())
        .to_string()
}

fn format_size(n: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    if (n as f64) >= GB {
        format!("{:.1} GB", n as f64 / GB)
    } else if (n as f64) >= MB {
        format!("{:.1} MB", n as f64 / MB)
    } else {
        format!("{n} B")
    }
}

fn retry_allowed(state: &str) -> bool {
    state == "failed" || state == "paused"
}

fn exception_title(code: &str) -> &str {
    match code {
        "zip_corrupt" => "ZIP corrupt",
        "zip_path_traversal" => "ZIP path rejected",
        "unsupported_7z" => "Unsupported 7z",
        "package_not_found" => "Package not found",
        "io_error" => "I/O error",
        other => other,
    }
}

fn snapshot_idle_or_terminal(snap: &JobProgressSnapshot) -> bool {
    snap.job_id.is_empty()
        || snap.state == "idle"
        || snap.state == "succeeded"
        || snap.state == "failed"
        || snap.state == "cancelled"
        || snap.state == "paused"
}

fn should_reload_stale_importing(sources_importing: bool, snap: &JobProgressSnapshot) -> bool {
    sources_importing && snapshot_idle_or_terminal(snap)
}

/// Completion for the 400 ms poller: classic busy→idle, or a job we accepted that
/// reached terminal before the first poll observed `running`.
fn poll_finished_ok(was_busy: bool, snap: &JobProgressSnapshot, accepted_job: &str) -> bool {
    if snapshot_busy(snap) {
        return false;
    }
    if was_busy {
        return true;
    }
    !accepted_job.is_empty() && snap.job_id == accepted_job
}

fn extract_work_from_psts(rows: &[ProcessPstRow]) -> Vec<ExtractWork> {
    rows.iter()
        .map(|p| ExtractWork {
            source_id: p.source_id.clone().unwrap_or_default(),
            pst_item_id: p.id.clone(),
            name: p.path.clone().unwrap_or_else(|| p.id.clone()),
        })
        .collect()
}

fn strings_from_js(value: &JsValue) -> Vec<String> {
    if let Some(s) = value.as_string() {
        return if s.is_empty() { Vec::new() } else { vec![s] };
    }
    let arr = js_sys::Array::from(value);
    let mut out = Vec::new();
    for i in 0..arr.length() {
        if let Some(s) = arr.get(i).as_string() {
            if !s.is_empty() {
                out.push(s);
            }
        }
    }
    out
}

fn drop_paths_from_event(ev: &JsValue) -> Vec<String> {
    let Ok(payload) = js_sys::Reflect::get(ev, &"payload".into()) else {
        return Vec::new();
    };
    if let Ok(ty) = js_sys::Reflect::get(&payload, &"type".into()) {
        if let Some(kind) = ty.as_string() {
            if kind != "drop" {
                return Vec::new();
            }
            if let Ok(paths) = js_sys::Reflect::get(&payload, &"paths".into()) {
                return strings_from_js(&paths);
            }
        }
    }
    strings_from_js(&payload)
}

fn drop_error_after_start(start_err: Option<&str>, paths: &[String]) -> Option<String> {
    match start_err {
        None => {
            if paths.len() <= 1 {
                return None;
            }
            let names: Vec<String> = paths[1..].iter().map(|p| path_basename(p)).collect();
            Some(format!("Not queued: {}", names.join(", ")))
        }
        Some(e) => {
            let names: Vec<String> = paths.iter().map(|p| path_basename(p)).collect();
            Some(format!("{e}; not queued: {}", names.join(", ")))
        }
    }
}

fn try_listen_webview_drop(handler: &js_sys::Function) -> Option<js_sys::Promise> {
    let window = web_sys::window()?;
    let tauri = js_sys::Reflect::get(&window, &"__TAURI__".into()).ok()?;
    if tauri.is_undefined() || tauri.is_null() {
        return None;
    }
    let webview_ns = js_sys::Reflect::get(&tauri, &"webview".into()).ok()?;
    if webview_ns.is_undefined() || webview_ns.is_null() {
        return None;
    }
    let get_current = js_sys::Reflect::get(&webview_ns, &"getCurrentWebview".into()).ok()?;
    let get_fn = get_current.dyn_into::<js_sys::Function>().ok()?;
    let current = get_fn.call0(&webview_ns).ok()?;
    let on_drop = js_sys::Reflect::get(&current, &"onDragDropEvent".into()).ok()?;
    let on_drop_fn = on_drop.dyn_into::<js_sys::Function>().ok()?;
    let ret = on_drop_fn.call1(&current, handler.as_ref()).ok()?;
    if ret.is_undefined() || ret.is_null() {
        return None;
    }
    Some(js_sys::Promise::resolve(&ret))
}

async fn unlisten_from_promise(promise: js_sys::Promise) -> Result<js_sys::Function, String> {
    let value = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| format!("File drop listener failed: {e:?}"))?;
    value
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "File drop listener failed: invalid unlisten".into())
}

async fn attach_drop_listener(handler: &js_sys::Function) -> Result<js_sys::Function, String> {
    if let Some(promise) = try_listen_webview_drop(handler) {
        if let Ok(unlisten) = unlisten_from_promise(promise).await {
            return Ok(unlisten);
        }
    }
    let promise = tauri_event_listen(FILE_DROP_EVENT, handler)
        .map_err(|e| format!("File drop listener failed: {e:?}"))?;
    unlisten_from_promise(promise).await
}

fn truncate_error(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let taken: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

fn job_source_class(parent: &Option<String>) -> &'static str {
    if parent.is_some() {
        "jobs-source child"
    } else {
        "jobs-source"
    }
}

fn source_shows_extract_progress(
    source: &ProcessSourceRow,
    extract_current_name: &str,
    snap: &JobProgressSnapshot,
    inventory: &[ProcessPstRow],
) -> bool {
    if extract_current_name.is_empty() || snap.kind != "extract_pst" {
        return false;
    }
    let want = strip_extended_path(extract_current_name);
    if strip_extended_path(&source.path) == want {
        return true;
    }
    inventory.iter().any(|p| {
        p.source_id.as_deref() == Some(source.id.as_str())
            && p.path
                .as_deref()
                .map(|path| strip_extended_path(path) == want)
                .unwrap_or(false)
    })
}

fn jobs_head_summary(jobs: &[ProcessJobRow], snap: &JobProgressSnapshot) -> String {
    let complete = jobs.iter().filter(|j| j.state == "succeeded").count();
    let mut running = jobs.iter().filter(|j| j.state == "running").count();
    if snapshot_busy(snap)
        && !jobs
            .iter()
            .any(|j| j.id == snap.job_id && j.state == "running")
    {
        running = running.saturating_add(1);
    }
    format!("{complete} complete · {running} running")
}

fn grouped_error_sum(groups: &[ProcessErrorGroup]) -> u64 {
    groups.iter().map(|g| g.count).sum()
}

fn selected_flags(
    page: Option<ProcessPageResponse>,
    selected: &str,
) -> Option<BuiltinProfileFlags> {
    let builtins = page?.builtins;
    builtins
        .iter()
        .find(|b| b.id == selected)
        .cloned()
        .or_else(|| builtins.iter().find(|b| b.id == DEFAULT_PROFILE).cloned())
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

fn spawn_resume(
    root: String,
    job_id: String,
    error: RwSignal<Option<String>>,
    accepted_job: RwSignal<String>,
) {
    if root.is_empty() || job_id.is_empty() {
        return;
    }
    leptos::task::spawn_local(async move {
        match tauri_invoke::<(), _>(
            "process_resume",
            &ProcessResumeArgs {
                root,
                job_id: job_id.clone(),
            },
        )
        .await
        {
            Ok(()) => {
                error.set(None);
                accepted_job.set(job_id);
            }
            Err(e) => error.set(Some(e)),
        }
    });
}

fn spawn_drop_ingest(
    root: String,
    paths: Vec<String>,
    error: RwSignal<Option<String>>,
    page: RwSignal<Option<ProcessPageResponse>>,
    accepted_job: RwSignal<String>,
) {
    if root.is_empty() || paths.is_empty() {
        return;
    }
    let first = paths[0].clone();
    leptos::task::spawn_local(async move {
        match tauri_invoke::<ProcessStartResponse, _>(
            "process_start",
            &ProcessStartArgs {
                root: root.clone(),
                kind: "ingest".into(),
                params_json: ingest_params(&first),
            },
        )
        .await
        {
            Ok(resp) => {
                accepted_job.set(resp.job_id);
                let queued_note = drop_error_after_start(None, &paths);
                error.set(queued_note.clone());
                match tauri_invoke::<ProcessPageResponse, _>(
                    "process_page",
                    &ProcessPageArgs { root },
                )
                .await
                {
                    Ok(resp) => page.set(Some(resp)),
                    Err(e) => {
                        error.set(Some(match queued_note {
                            Some(note) => format!("{e}; {note}"),
                            None => e,
                        }));
                    }
                }
            }
            Err(e) => error.set(drop_error_after_start(Some(&e), &paths)),
        }
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
    let accepted_job = RwSignal::new(String::new());
    let exporting = RwSignal::new(false);
    let export_note = RwSignal::new(Option::<String>::None);
    let selected_exception = RwSignal::new(Option::<String>::None);
    let chrome = use_context::<ProcessChromeCtx>();

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
                        let finished_ok = poll_finished_ok(
                            was_busy,
                            &snap,
                            &accepted_job.get_untracked(),
                        );
                        let finished_failed = finished_ok && snap.state == "failed";
                        let finished_paused = finished_ok
                            && (snap.state == "paused" || snap.state == "cancelled");
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
                        let importing = page
                            .get_untracked()
                            .map(|p| p.sources.iter().any(|s| s.status == "importing"))
                            .unwrap_or(false);
                        let stale_importing = should_reload_stale_importing(importing, &snap);
                        if finished_ok || missing_job || stale_importing {
                            if finished_ok {
                                accepted_job.set(String::new());
                            }
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
                                        Ok(resp) => {
                                            accepted_job.set(resp.job_id);
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
                                    Ok(resp) => {
                                        accepted_job.set(resp.job_id);
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
                Ok(resp) => {
                    accepted_job.set(resp.job_id);
                    error.set(None);
                    reload(root);
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    Effect::new(move |_| {
        let unlisten = StoredValue::new(Option::<js_sys::Function>::None);
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let paths = drop_paths_from_event(&ev);
            if paths.is_empty() {
                return;
            }
            spawn_drop_ingest(root_sig.get_untracked(), paths, error, page, accepted_job);
        }) as Box<dyn FnMut(JsValue)>);
        let handler: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        leptos::task::spawn_local(async move {
            match attach_drop_listener(&handler).await {
                Ok(f) => unlisten.set_value(Some(f)),
                Err(e) => error.set(Some(e)),
            }
        });
        cb.forget();
        on_cleanup(move || {
            if let Some(f) = unlisten.get_value() {
                let _ = f.call0(&JsValue::UNDEFINED);
            }
        });
    });

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
        let q = extract_work_from_psts(&pg.pst_inventory);
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
                Ok(resp) => {
                    accepted_job.set(resp.job_id);
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

    let extract_remaining = move |_| {
        if !extract_all_should_start(extract_queue.get().len(), snapshot_busy(&progress.get())) {
            return;
        }
        let Some(pg) = page.get() else {
            return;
        };
        if pg.unextracted_psts.is_empty() {
            error.set(Some("No unextracted PST leaves.".into()));
            return;
        }
        let q = extract_work_from_psts(&pg.unextracted_psts);
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
                Ok(resp) => {
                    accepted_job.set(resp.job_id);
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

    let export_report = move |_| {
        if exporting.get() {
            return;
        }
        let root = root_sig.get();
        if root.is_empty() {
            return;
        }
        exporting.set(true);
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ProcessExportReportResponse, _>(
                "process_export_report",
                &ProcessExportReportArgs { root },
            )
            .await
            {
                Ok(resp) => {
                    error.set(None);
                    export_note.set(Some(format!(
                        "Wrote {} file(s) to {}",
                        resp.files_written.len(),
                        resp.output_dir
                    )));
                }
                Err(e) => {
                    export_note.set(None);
                    error.set(Some(e));
                }
            }
            exporting.set(false);
        });
    };

    Effect::new(move |_| {
        let Some(ctx) = chrome else {
            return;
        };
        let jobs = page.get().map(|p| p.jobs).unwrap_or_default();
        let snap = progress.get();
        ctx.right_label.set(jobs_head_summary(&jobs, &snap));
        ctx.status_left.set(if snapshot_busy(&snap) {
            format!(
                "{} {} · {} · {}",
                snap.kind,
                snap.state,
                snap.stage.unwrap_or_else(|| "—".into()),
                snap.message.unwrap_or_default()
            )
        } else {
            String::new()
        });
    });

    view! {
        <section class="process-page">
            <h1>"Process"</h1>
            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <div class="process-layout">
                <aside class="process-pane">
                    <h2>"Sources"</h2>
                    <div class="drop-zone" aria-label="Accepted ingest kinds">
                        <p>{DROP_COPY_KINDS}</p>
                        <p class="empty">{DROP_COPY_HASH}</p>
                    </div>
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
                            let source = s.clone();
                            view! {
                                <div class="set-row">
                                    <div class="name">{path_basename(&s.path)}</div>
                                    <div class="empty">{format!("{} · {}", s.kind, s.status)}</div>
                                    {s.size_bytes.map(|n| {
                                        view! { <div class="empty">{format_size(n)}</div> }
                                    })}
                                    <Show when=move || {
                                        let snap = progress.get();
                                        let inventory = page
                                            .get()
                                            .map(|p| p.pst_inventory)
                                            .unwrap_or_default();
                                        source_shows_extract_progress(
                                            &source,
                                            &extract_current_name.get(),
                                            &snap,
                                            &inventory,
                                        ) && snap.total_hint.filter(|t| *t > 0).is_some()
                                    }>
                                        <progress
                                            max=move || progress.get().total_hint.unwrap_or(1)
                                            value=move || progress.get().completed_count
                                        />
                                    </Show>
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
                            let label = p
                                .path
                                .as_deref()
                                .map(strip_extended_path)
                                .unwrap_or_else(|| p.id.clone());
                            view! {
                                <label class="set-row">
                                    <input
                                        type="radio"
                                        name="pst-inv"
                                        prop:checked=move || selected_pst.get().as_deref() == Some(id.as_str())
                                        on:change=move |_| selected_pst.set(Some(id_sel.clone()))
                                    />
                                    <span>{label}</span>
                                </label>
                            }
                        }
                    />
                    <h2>"Profile"</h2>
                    <label>
                        "Builtin "
                        <select
                            prop:value=move || selected_profile.get()
                            on:change=move |ev| {
                                if let Some(el) = ev
                                    .target()
                                    .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
                                {
                                    selected_profile.set(el.value());
                                }
                            }
                        >
                            <For
                                each=move || page.get().map(|p| p.builtins).unwrap_or_default()
                                key=|b: &BuiltinProfileFlags| b.id.clone()
                                children=move |b| {
                                    let id = b.id.clone();
                                    view! { <option value=id>{b.name}</option> }
                                }
                            />
                        </select>
                    </label>
                    <ul class="profile-checklist">
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.classify)
                                .unwrap_or(false);
                            format!("{} classify", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.office_extract || b.pdf_extract || b.ics_extract)
                                .unwrap_or(false);
                            format!("{} office-pdf-ics", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.ocr)
                                .unwrap_or(false);
                            format!("{} OCR", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.fts)
                                .unwrap_or(false);
                            format!("{} FTS", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.dedupe)
                                .unwrap_or(false);
                            format!("{} dedupe", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.thread)
                                .unwrap_or(false);
                            format!("{} thread", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.neardup)
                                .unwrap_or(false);
                            format!("{} neardup", if on { "✓" } else { "○" })
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.cull)
                                .unwrap_or(false);
                            format!(
                                "{} DeNIST/cull — {DENIST_NSRL_NOTE}",
                                if on { "✓" } else { "○" }
                            )
                        }}</li>
                        <li>{move || {
                            let on = selected_flags(page.get(), &selected_profile.get())
                                .map(|b| b.promote)
                                .unwrap_or(false);
                            format!("{} promote", if on { "✓" } else { "○" })
                        }}</li>
                    </ul>
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
                        <button class="primary" on:click=run_profile>"Run profile"</button>
                        <Show when=move || {
                            page.get()
                                .map(|p| !p.unextracted_psts.is_empty())
                                .unwrap_or(false)
                        }>
                            <button
                                on:click=extract_remaining
                                disabled=move || {
                                    snapshot_busy(&progress.get()) || busy_retry_pending.get()
                                }
                            >
                                "Extract remaining"
                            </button>
                        </Show>
                    </div>
                    <Show when=move || extract_note.get().is_some()>
                        <p class="empty">{move || extract_note.get().unwrap_or_default()}</p>
                    </Show>
                </aside>
                <div class="process-pane">
                    <h2>"Jobs"</h2>
                    <Show when=move || page.get().map(|p| !p.jobs.is_empty()).unwrap_or(false)>
                        <p class="empty">{move || {
                            let jobs = page.get().map(|p| p.jobs).unwrap_or_default();
                            jobs_head_summary(&jobs, &progress.get())
                        }}</p>
                    </Show>
                    <Show when=move || page.get().map(|p| p.jobs.is_empty()).unwrap_or(true)>
                        <p class="empty">"No jobs yet. Ingest or run a profile."</p>
                    </Show>
                    <div class="jobs-table-wrap">
                        <table class="jobs-table">
                            <thead>
                                <tr>
                                    <th>"Source"</th>
                                    <th class="num">"Items"</th>
                                    <th class="num">"Dupes"</th>
                                    <th class="num">"NIST"</th>
                                    <th class="num">"Families"</th>
                                    <th class="num">"Except."</th>
                                    <th class="status">"Status"</th>
                                    <th></th>
                                </tr>
                            </thead>
                            <tbody>
                                <For
                                    each=move || page.get().map(|p| p.jobs).unwrap_or_default()
                                    key=|j: &ProcessJobRow| j.id.clone()
                                    children=move |j| {
                                        let job_for_orphan = j.clone();
                                        let job_for_retry = j.clone();
                                        let job_id_for_counts = j.id.clone();
                                        let job_id_for_active = j.id.clone();
                                        let job_id_for_status = j.id.clone();
                                        let job_state = j.state.clone();
                                        let job_id = StoredValue::new(j.id.clone());
                                        let err = j
                                            .error_summary
                                            .as_deref()
                                            .filter(|s| !s.is_empty())
                                            .map(|s| truncate_error(s, 80));
                                        view! {
                                            <tr>
                                                <td class=job_source_class(&j.parent_job_id)>
                                                    <div class="name">{j.source_label.clone().unwrap_or_else(|| j.kind.clone())}</div>
                                                    <div class="empty">{j.kind.clone()}</div>
                                                    {err.map(|e| view! { <div class="empty">{e}</div> })}
                                                </td>
                                                <td class="num">{move || {
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
                                                }}</td>
                                                <td class="num">"—"</td>
                                                <td class="num">"—"</td>
                                                <td class="num">"—"</td>
                                                <td class="num">"—"</td>
                                                <td class="status">{move || {
                                                    let snap = progress.get();
                                                    if snap.job_id == job_id_for_status {
                                                        if snapshot_busy(&snap) {
                                                            if let Some(total) = snap.total_hint {
                                                                if total > 0 {
                                                                    let pct = snap.completed_count.saturating_mul(100) / total;
                                                                    return format!("{} · {pct}%", snap.state);
                                                                }
                                                            }
                                                        }
                                                        snap.state
                                                    } else {
                                                        job_state.clone()
                                                    }
                                                }}</td>
                                                <td class="jobs-actions">
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
                                                            spawn_resume(root_sig.get(), job_id.get_value(), error, accepted_job);
                                                        }>"Resume"</button>
                                                        <button on:click=move |_| {
                                                            spawn_cancel(job_id.get_value());
                                                        }>"Cancel"</button>
                                                    </Show>
                                                    <Show when=move || {
                                                        retry_allowed(&job_for_retry.state)
                                                    }>
                                                        <button class="primary" on:click=move |_| {
                                                            spawn_resume(root_sig.get(), job_id.get_value(), error, accepted_job);
                                                        }>"Resume"</button>
                                                    </Show>
                                                </td>
                                            </tr>
                                        }
                                    }
                                />
                            </tbody>
                        </table>
                    </div>
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
                    <h2>{move || format!(
                        "Exceptions ({})",
                        page.get().map(|p| p.exceptions).unwrap_or(0)
                    )}</h2>
                    <p class="empty">"Exceptions hold their items; they do not stall sibling extract."</p>
                    <Show when=move || page.get().map(|p| p.error_groups.is_empty()).unwrap_or(true)>
                        <p class="empty">"No item_errors recorded."</p>
                    </Show>
                    <For
                        each=move || page.get().map(|p| p.error_groups).unwrap_or_default()
                        key=|g: &ProcessErrorGroup| g.code.clone()
                        children=move |g| {
                            let code = g.code.clone();
                            let code_sel = code.clone();
                            let title = exception_title(&g.code).to_string();
                            view! {
                                <button
                                    class="set-row exception-group"
                                    on:click=move |_| selected_exception.set(Some(code_sel.clone()))
                                >
                                    <div class="name">{format!("{} · {}", title, g.count)}</div>
                                </button>
                            }
                        }
                    />
                    <Show when=move || selected_exception.get().is_some()>
                        <div class="exception-detail">
                            {move || {
                                let code = selected_exception.get().unwrap_or_default();
                                page.get()
                                    .and_then(|p| p.error_groups.into_iter().find(|g| g.code == code))
                                    .map(|g| {
                                        let title = exception_title(&g.code).to_string();
                                        let resume_id = StoredValue::new(
                                            g.sample_job_id.clone().unwrap_or_default(),
                                        );
                                        let retry_id = resume_id.get_value();
                                        let item_id = g.sample_item_id.clone();
                                        view! {
                                            <div>
                                                <div class="name">{format!("{} ({})", title, g.code)}</div>
                                                <div class="empty">{format!("count {}", g.count)}</div>
                                                <div class="empty">{format!("sample_message {}", g.sample_message)}</div>
                                                <Show when=move || {
                                                    if retry_id.is_empty() {
                                                        return false;
                                                    }
                                                    page.get()
                                                        .and_then(|p| {
                                                            p.jobs.into_iter().find(|j| j.id == retry_id)
                                                        })
                                                        .map(|j| retry_allowed(&j.state))
                                                        .unwrap_or(false)
                                                }>
                                                    <button class="primary" on:click=move |_| {
                                                        spawn_resume(root_sig.get(), resume_id.get_value(), error, accepted_job);
                                                    }>"Retry"</button>
                                                </Show>
                                                {item_id.as_ref().filter(|id| !id.is_empty()).map(|id| {
                                                    let href = review_doc_href(&root_sig.get(), id, None, None);
                                                    view! { <A href=href>"Open in review"</A> }
                                                })}
                                            </div>
                                        }
                                    })
                            }}
                        </div>
                    </Show>
                    <Show when=move || {
                        page.get()
                            .map(|p| p.exceptions > grouped_error_sum(&p.error_groups))
                            .unwrap_or(false)
                    }>
                        <p class="empty">{move || {
                            let p = page.get();
                            let grouped = p.as_ref().map(|x| grouped_error_sum(&x.error_groups)).unwrap_or(0);
                            let total = p.map(|x| x.exceptions).unwrap_or(0);
                            format!(
                                "Grouped counts cover {grouped} of {total} exceptions (recent 100 item_errors)."
                            )
                        }}</p>
                    </Show>
                    <p class="empty">{EXCEPTIONS_NO_VAULT}</p>
                </div>
                <aside class="process-pane">
                    <h2>"Running report"</h2>
                    <dl class="minus-stack">
                        <div>
                            <dt>"Discovered"</dt>
                            <dd>{move || page.get().map(|p| p.discovered).unwrap_or(0)}</dd>
                        </div>
                        <div>
                            <dt>"− DeNIST/cull suppressed"</dt>
                            <dd>{move || dash(page.get().and_then(|p| p.denist))}</dd>
                        </div>
                        <div>
                            <dt>"− duplicate instances"</dt>
                            <dd>{move || dash(page.get().and_then(|p| p.dupes))}</dd>
                        </div>
                        <div>
                            <dt>"− quarantined"</dt>
                            <dd>{move || page.get().map(|p| p.exceptions).unwrap_or(0)}</dd>
                        </div>
                        <div>
                            <dt>"Review-ready"</dt>
                            <dd>{move || page.get().map(|p| p.in_review).unwrap_or(0)}</dd>
                        </div>
                        <div>
                            <dt>"Unaccounted-for"</dt>
                            <dd>{move || page.get().map(|p| p.unaccounted_for).unwrap_or(0)}</dd>
                        </div>
                        <div>
                            <dt>"Still processing"</dt>
                            <dd>{move || page.get().map(|p| p.still_processing).unwrap_or(0)}</dd>
                        </div>
                    </dl>
                    <Show when=move || {
                        page.get()
                            .map(|p| !p.unextracted_psts.is_empty())
                            .unwrap_or(false)
                    }>
                        <p class="empty">"Unextracted inventory (not missing messages):"</p>
                        <For
                            each=move || page.get().map(|p| p.unextracted_psts).unwrap_or_default()
                            key=|p: &ProcessPstRow| p.id.clone()
                            children=move |p| {
                                let name = p
                                    .path
                                    .as_deref()
                                    .map(path_basename)
                                    .unwrap_or_else(|| p.id.clone());
                                view! { <p class="empty">{name}</p> }
                            }
                        />
                    </Show>
                    <Show when=move || {
                        page.get().map(|p| p.failed_unlogged > 0).unwrap_or(false)
                    }>
                        <p class="empty">{move || {
                            let n = page.get().map(|p| p.failed_unlogged).unwrap_or(0);
                            format!("{n} failed job(s) without item_errors — use job Resume")
                        }}</p>
                    </Show>
                    <p class="empty">{move || format!(
                        "Families {}",
                        page.get().map(|p| p.families).unwrap_or(0)
                    )}</p>
                    <Show when=move || page.get().map(|p| p.pdf_needs_ocr > 0).unwrap_or(false)>
                        <p class="empty">{move || format!(
                            "{} items need OCR",
                            page.get().map(|p| p.pdf_needs_ocr).unwrap_or(0)
                        )}</p>
                    </Show>
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
                    <button
                        disabled=move || exporting.get()
                        on:click=export_report
                    >
                        "Download report"
                    </button>
                    <Show when=move || export_note.get().is_some()>
                        <p class="empty">{move || export_note.get().unwrap_or_default()}</p>
                    </Show>
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
            source_label: None,
        }
    }

    fn job_row_shows_resume(job: &ProcessJobRow, snap: &JobProgressSnapshot) -> bool {
        is_orphan_running(job, snap) || retry_allowed(&job.state)
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
        let failed = job("job_fail", "failed");
        let paused = job("job_pause", "paused");
        assert!(job_row_shows_resume(&failed, &idle));
        assert!(job_row_shows_resume(&paused, &idle));
        assert!(!job_row_shows_resume(&succeeded, &idle));
        assert!(!job_row_shows_resume(&running, &matching));
        assert!(job_row_shows_resume(&running, &idle));
    }

    #[test]
    fn poll_finished_ok_matches_accepted_terminal_without_was_busy() {
        let terminal = snap("job_fast", "succeeded");
        assert!(
            poll_finished_ok(false, &terminal, "job_fast"),
            "idle→terminal with accepted id must complete without observing running"
        );
        assert!(!poll_finished_ok(false, &terminal, ""));
        assert!(!poll_finished_ok(false, &terminal, "other"));
        assert!(!poll_finished_ok(
            false,
            &snap("job_fast", "running"),
            "job_fast"
        ));
        assert!(poll_finished_ok(true, &terminal, ""));
        let idle = snap("", "idle");
        assert!(
            !poll_finished_ok(false, &idle, "job_fast"),
            "empty idle must not complete a different accepted id"
        );
        assert!(poll_finished_ok(true, &idle, "job_fast"));
        let failed = snap("job_fast", "failed");
        assert!(poll_finished_ok(false, &failed, "job_fast"));
        let mut queue = 2usize;
        if poll_finished_ok(false, &terminal, "job_fast") && terminal.kind == "extract_pst" {
            queue -= 1;
        }
        assert_eq!(queue, 1);
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
            poller.contains("poll_finished_ok"),
            "poller must complete accepted jobs that skip the running poll"
        );
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
        assert!(
            prod.contains("retry_allowed(&job_for_retry.state)"),
            "failed/paused jobs must offer Resume beside the orphan lock"
        );
    }

    #[test]
    fn strip_extended_path_drive_and_unc() {
        assert_eq!(strip_extended_path(r"\\?\C:\x"), r"C:\x");
        assert_eq!(
            strip_extended_path(r"\\?\UNC\server\share"),
            r"\\server\share"
        );
        assert_eq!(strip_extended_path(r"C:\plain"), r"C:\plain");
    }

    #[test]
    fn drop_copy_names_honest_ingest_kinds() {
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(prod.contains(DROP_COPY_KINDS));
        assert!(prod.contains(DROP_COPY_HASH));
        let drop = prod.split("drop-zone").nth(1).unwrap_or("");
        let drop = drop.split("</div>").next().unwrap_or(drop);
        assert!(
            !drop.contains("OST"),
            "drop-zone copy must not advertise OST"
        );
        assert!(
            !drop.contains("MBOX"),
            "drop-zone copy must not advertise MBOX"
        );
    }

    #[test]
    fn jobs_table_emdash_per_row_columns() {
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let jobs = prod.split("\"Jobs\"").nth(1).unwrap_or("");
        assert!(jobs.contains("<table"));
        assert!(jobs.contains("Dupes"));
        assert!(jobs.contains("NIST"));
        assert!(jobs.contains("Families"));
        assert!(jobs.contains("Except."));
        let row = jobs.split("children=move |j|").nth(1).unwrap_or("");
        assert!(
            row.contains("{j.kind.clone()}"),
            "Source column must be live job kind, not a fake PST name"
        );
        let dashes = row.matches(r#""—""#).count();
        assert!(
            dashes >= 4,
            "each job row must paint em-dash for Dupes/NIST/Families/Except.; got {dashes}"
        );
        assert!(
            !row.contains("page.dupes"),
            "must not copy matter-wide dupes onto job rows"
        );
        assert!(jobs.contains("jobs-table-wrap"));
        let css = include_str!("../../styles/app.css");
        assert!(css.contains("overflow-x: auto"));
        assert!(css.contains("280px minmax(0, 1fr) 320px"));
        assert!(css.contains("white-space: nowrap"));
        assert!(!css.contains("Archivo"));
        assert!(!css.contains("#ec3013"));
    }

    #[test]
    fn minus_stack_labels_and_denist_zero_vs_dash() {
        assert_eq!(dash(None), "—");
        assert_eq!(dash(Some(0)), "0");
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(prod.contains("minus-stack"));
        assert!(prod.contains("Discovered"));
        assert!(prod.contains("DeNIST/cull suppressed"));
        assert!(prod.contains("duplicate instances"));
        assert!(prod.contains("quarantined"));
        assert!(prod.contains("Review-ready"));
        assert!(prod.contains("Unaccounted-for"));
        assert!(prod.contains("Still processing"));
        let export = prod.split("let export_report = move |_|").nth(1).unwrap_or("");
        assert!(
            export.contains("export_note.set(None)"),
            "export failure must clear a previous success note"
        );
    }

    #[test]
    fn wrap_process_and_shell_consume_process_chrome_ctx() {
        let app = include_str!("../app.rs");
        let app_prod = app.split("#[cfg(test)]").next().unwrap_or(app);
        assert!(app_prod.contains("fn WrapProcess()"));
        assert!(app_prod.contains("provide_context(ProcessChromeCtx"));
        let shell = include_str!("../shell.rs");
        let shell_prod = shell.split("#[cfg(test)]").next().unwrap_or(shell);
        let top = shell_prod.split("fn TopBar").nth(1).unwrap_or("");
        let top = top.split("fn StatusBar").next().unwrap_or(top);
        assert!(top.contains("ProcessChromeCtx"));
        let status = shell_prod.split("fn StatusBar").nth(1).unwrap_or("");
        assert!(status.contains("ProcessChromeCtx"));
    }

    #[test]
    fn retry_allowed_failed_or_paused_only() {
        assert!(retry_allowed("failed"));
        assert!(retry_allowed("paused"));
        assert!(!retry_allowed("succeeded"));
        assert!(!retry_allowed("running"));
        assert!(!retry_allowed("cancelled"));
        assert!(!retry_allowed("pending"));
        assert!(!retry_allowed("idle"));
        assert!(!retry_allowed(""));
        assert_eq!(exception_title("zip_corrupt"), "ZIP corrupt");
        assert_eq!(exception_title("unknown_bucket"), "unknown_bucket");
    }

    #[test]
    fn extract_remaining_reuses_extract_all_guard_without_extra_queue_wipe() {
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let remaining = prod
            .split("let extract_remaining = move |_|")
            .nth(1)
            .unwrap_or("");
        let guard = remaining
            .find("extract_all_should_start")
            .expect("extract remaining must call extract_all_should_start");
        let queue_write = remaining
            .find("extract_queue.set(q)")
            .expect("extract remaining still writes the queue after the guard");
        assert!(guard < queue_write);
        assert!(remaining.contains("unextracted_psts"));
        assert!(remaining.contains("apply_extract_start_err"));
        assert!(
            !remaining.contains("extract_queue.set(Vec::new())"),
            "remaining must not add a second queue wipe"
        );
        assert!(prod.contains("\"Extract remaining\""));
        assert!(prod.contains("\"Extract all\""));
    }

    #[test]
    fn poller_reloads_stale_importing_when_snapshot_idle() {
        let idle = snap("", "idle");
        assert!(should_reload_stale_importing(true, &idle));
        assert!(!should_reload_stale_importing(false, &idle));
        assert!(!should_reload_stale_importing(
            true,
            &snap("j1", "running")
        ));
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(prod.contains("should_reload_stale_importing"));
        assert!(prod.contains("if finished_ok || missing_job || stale_importing"));
        assert!(prod.contains("poll_finished_ok"));
    }

    #[test]
    fn drop_ingest_lists_unqueued_and_never_writes_extract_queue() {
        assert_eq!(
            drop_error_after_start(None, &["C:\\a.pst".into()]),
            None
        );
        let note = drop_error_after_start(
            None,
            &["C:\\a.pst".into(), "C:\\b.zip".into(), "C:\\c.pst".into()],
        );
        assert_eq!(note.as_deref(), Some("Not queued: b.zip, c.pst"));
        let busy = drop_error_after_start(
            Some("busy: matter is busy: a job is already running (job_1)"),
            &["C:\\a.pst".into(), "C:\\b.zip".into()],
        );
        assert!(busy.as_deref().unwrap_or("").contains("not queued: a.pst, b.zip"));
        let encrypted = drop_error_after_start(
            Some("encrypted PST: password required"),
            &["C:\\a.pst".into(), "C:\\b.zip".into()],
        );
        assert!(
            encrypted
                .as_deref()
                .unwrap_or("")
                .contains("not queued: a.pst, b.zip")
        );
        assert!(encrypted
            .as_deref()
            .unwrap_or("")
            .contains("encrypted PST: password required"));
        assert!(is_busy_invoke_err(
            "busy: matter is busy: a job is already running (job_1)"
        ));
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        let drop_fn = prod.split("fn spawn_drop_ingest").nth(1).unwrap_or("");
        let drop_fn = drop_fn.split("fn is_orphan_running").next().unwrap_or(drop_fn);
        assert!(drop_fn.contains("ingest"));
        assert!(!drop_fn.contains("extract_queue"));
        assert!(drop_fn.contains("accepted_job.set(resp.job_id)"));
        assert!(prod.contains("try_listen_webview_drop"));
        assert!(prod.contains("attach_drop_listener"));
        assert!(prod.contains("tauri_event_listen"));
        assert!(prod.contains(FILE_DROP_EVENT));
        assert!(prod.contains("File drop listener failed"));
        let drop_effect = prod.split("attach_drop_listener(&handler)").nth(1).unwrap_or("");
        let drop_effect = drop_effect.split("let add_folder").next().unwrap_or(drop_effect);
        assert!(
            drop_effect.contains("on_cleanup"),
            "drop listener must unlisten when Process unmounts"
        );
        assert!(prod.contains("Add folder"));
        assert!(prod.contains("Add ZIP / PST"));
        assert_eq!(path_basename(r"\\?\C:\mail\INC.pst"), "INC.pst");
        assert_eq!(format_size(1_500_000_000), "1.5 GB");
        assert_eq!(format_size(2_000_000), "2.0 MB");
        assert_eq!(format_size(12), "12 B");
    }

    #[test]
    fn exception_retry_and_no_vault_copy() {
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(prod.contains("retry_allowed"));
        assert!(prod.contains("\"Retry\""));
        assert!(prod.contains("Open in review"));
        assert!(prod.contains(EXCEPTIONS_NO_VAULT));
        assert!(!prod.contains("EXCEPTIONS_NOT_THIS_TRACK"));
        assert!(!prod.contains("password vault: not this track"));
        assert!(!prod.contains("request from custodian"));
        assert!(prod.contains("spawn_resume(root_sig.get(), job_id.get_value(), error, accepted_job)"));
        assert!(!prod.contains("let _ = tauri_invoke::<(), _>(\"process_resume\""));
    }
}
