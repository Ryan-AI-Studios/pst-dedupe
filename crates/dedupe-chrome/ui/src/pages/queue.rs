//! First-pass virtualized review queue (track 0111).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Once;

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlElement, HtmlInputElement, KeyboardEvent, MouseEvent};

use crate::shell::{QueueChromeCtx, QueueRange};

use crate::invoke::{
    tauri_invoke, CodeCatalogEntry, QueueRow, ReviewApplyCodesArgs, ReviewCodesPreview,
    ReviewCodesPreviewArgs, ReviewQueuePage, ReviewQueuePageArgs, RootArgs, SavedSearchDto,
    SavedSearchUpsertArgs,
};
use crate::path_id::review_doc_href;
use crate::queue_window::{
    clamp_offset_for_fetch_meta, next_page_disabled, scroll_top_to_reveal, visible_range, OVERSCAN,
    ROW_HEIGHT,
};

const PAGE_LIMIT: u64 = 500;

fn preset_uncoded_json() -> String {
    r#"{"version":1,"scope":"review_corpus","include_family":false,"conditions":[{"field":"code_missing","op":"eq","value":true}]}"#.into()
}

fn preset_privilege_json() -> String {
    r#"{"version":1,"scope":"review_corpus","include_family":false,"conditions":[{"field":"code","op":"any_of","values":["privilege"]}]}"#.into()
}

fn preset_responsive_json() -> String {
    r#"{"version":1,"scope":"review_corpus","include_family":false,"conditions":[{"field":"code","op":"any_of","values":["responsive"]}]}"#.into()
}

fn control_number(order: Option<i64>) -> String {
    match order {
        Some(n) => n.to_string(),
        None => "—".into(),
    }
}

fn blank_field(value: Option<&str>) -> bool {
    value.map(str::trim).filter(|s| !s.is_empty()).is_none()
}

/// Display From as stored (SMTP or X500). Do not parse `/O=` into a guessed SMTP.
fn display_from(from_addr: Option<&str>) -> String {
    from_addr
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn display_or_dash(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("—")
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FamilyCells {
    date: String,
    from: String,
    subject: String,
}

/// If the row is a child with empty date/from/subject, copy the parent when that
/// parent is on this SQL page; otherwise the subject cell is `"— attachment"`.
fn family_cell_text(
    parent_item_id: Option<&str>,
    date: Option<&str>,
    from_addr: Option<&str>,
    subject: Option<&str>,
    parent: Option<(Option<&str>, Option<&str>, Option<&str>)>,
) -> FamilyCells {
    if parent_item_id.is_some()
        && blank_field(date)
        && blank_field(from_addr)
        && blank_field(subject)
    {
        if let Some((parent_date, parent_from, parent_subject)) = parent {
            return FamilyCells {
                date: display_or_dash(parent_date),
                from: display_from(parent_from),
                subject: display_or_dash(parent_subject),
            };
        }
        return FamilyCells {
            date: "—".into(),
            from: "—".into(),
            subject: "— attachment".into(),
        };
    }
    FamilyCells {
        date: display_or_dash(date),
        from: display_from(from_addr),
        subject: display_or_dash(subject),
    }
}

fn subject_contains_json(needle: &str, include_family: bool) -> String {
    serde_json::json!({
        "version": 1,
        "scope": "review_corpus",
        "include_family": include_family,
        "conditions": [{
            "field": "subject",
            "op": "contains",
            "value": needle,
        }]
    })
    .to_string()
}

fn queue_title_text(chip: &str, total: u64, saved: &[SavedSearchDto]) -> String {
    let name = match chip {
        "unreviewed" => "Unreviewed",
        "privileged" => "Privileged",
        "responsive" => "Responsive",
        "goto-subject" => "Subject",
        other => saved
            .iter()
            .find(|s| s.id == other)
            .map(|s| s.name.as_str())
            .unwrap_or("Queue"),
    };
    format!("{name} {total} docs")
}

fn control_not_on_page(n: i64, meta: Option<(u64, u64, usize)>) -> String {
    let span = match meta {
        Some((offset, _total, fetched)) if fetched > 0 => {
            let start = offset.saturating_add(1);
            let end = offset.saturating_add(fetched as u64);
            format!("Rows {start}–{end}")
        }
        Some((_, total, _)) => format!("Rows — of {total}"),
        None => "Rows —".into(),
    };
    format!("Control# {n} not found in current page ({span})")
}

fn measure_queue_viewport(viewport_h: RwSignal<f64>) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(el) = doc.get_element_by_id("queue") else {
        return;
    };
    let Ok(html) = el.dyn_into::<HtmlElement>() else {
        return;
    };
    let height = html.client_height() as f64;
    if height > 0.0 {
        viewport_h.set(height);
    }
}

fn install_queue_resize_once() {
    static QUEUE_RESIZE_ONCE: Once = Once::new();
    QUEUE_RESIZE_ONCE.call_once(|| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let cb = Closure::<dyn FnMut()>::new(|| {
            QUEUE_VIEWPORT_H.with(|cell| {
                if let Some(vh) = cell.get() {
                    measure_queue_viewport(vh);
                }
            });
        });
        let _ = window.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
        cb.forget();
    });
}

thread_local! {
    static QUEUE_VIEWPORT_H: Cell<Option<RwSignal<f64>>> = const { Cell::new(None) };
}

fn resp_display(resp: &Option<String>) -> String {
    resp.clone().unwrap_or_else(|| "—".into())
}

/// Checkbox is authoritative: always write `include_family` true or false.
fn with_include_family(filter_json: &str, include_family: bool) -> Result<String, String> {
    let mut v: serde_json::Value =
        serde_json::from_str(filter_json).map_err(|e| format!("invalid filter_json: {e}"))?;
    v["include_family"] = serde_json::Value::Bool(include_family);
    serde_json::to_string(&v).map_err(|e| format!("filter_json serialize failed: {e}"))
}

fn read_include_family(filter_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(filter_json)
        .ok()
        .and_then(|v| v.get("include_family").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

fn queue_shortcut_blocked(ev: &KeyboardEvent) -> bool {
    let Some(target) = ev.target() else {
        return false;
    };
    let Ok(el) = target.dyn_into::<web_sys::Element>() else {
        return false;
    };
    let tag = el.tag_name().to_ascii_lowercase();
    if matches!(
        tag.as_str(),
        "input" | "textarea" | "select" | "button" | "option" | "a"
    ) {
        return true;
    }
    if el
        .get_attribute("role")
        .is_some_and(|r| r.eq_ignore_ascii_case("link"))
    {
        return true;
    }
    el.get_attribute("contenteditable")
        .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "")
}

fn set_queue_dom_scroll_top(value: f64) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(el) = doc.get_element_by_id("queue") else {
        return;
    };
    let Ok(html) = el.dyn_into::<HtmlElement>() else {
        return;
    };
    html.set_scroll_top(value as i32);
}

fn reveal_row(idx: usize, scroll_top: RwSignal<f64>, viewport_h: RwSignal<f64>) {
    let st = scroll_top.get();
    let next = scroll_top_to_reveal(idx, ROW_HEIGHT, viewport_h.get(), st);
    if next != st {
        scroll_top.set(next);
        set_queue_dom_scroll_top(next);
    }
}

fn apply_codes_async(
    root: String,
    ids: Vec<String>,
    code_id: String,
    bulk_error: RwSignal<Option<String>>,
    confirm_priv: RwSignal<Option<(String, u64)>>,
    tag_open: RwSignal<bool>,
    selected: RwSignal<HashSet<String>>,
    reload_tick: RwSignal<u32>,
) {
    leptos::task::spawn_local(async move {
        match tauri_invoke::<serde_json::Value, _>(
            "review_apply_codes",
            &ReviewApplyCodesArgs {
                root,
                item_ids: ids,
                add_code_ids: vec![code_id],
                remove_code_ids: vec![],
                propagate_family: Some(false),
            },
        )
        .await
        {
            Ok(_) => {
                confirm_priv.set(None);
                tag_open.set(false);
                selected.set(HashSet::new());
                reload_tick.update(|t| *t = t.wrapping_add(1));
            }
            Err(e) => bulk_error.set(Some(format!("Apply failed: {e}"))),
        }
    });
}

#[component]
pub fn ReviewQueue() -> impl IntoView {
    let params = use_params_map();
    let navigate = StoredValue::new(use_navigate());

    let root_sig = RwSignal::new(String::new());
    let filter_json = RwSignal::new(preset_uncoded_json());
    let active_chip = RwSignal::new(String::from("unreviewed"));
    let keyword = RwSignal::new(String::new());
    let keyword_draft = RwSignal::new(String::new());
    let include_family = RwSignal::new(false);
    let extras = RwSignal::new(false);
    let offset = RwSignal::new(0u64);
    let reload_tick = RwSignal::new(0u32);
    let fetch_gen = RwSignal::new(0u32);

    let page = RwSignal::new(Option::<ReviewQueuePage>::None);
    let error = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(false);

    let catalog = RwSignal::new(Vec::<CodeCatalogEntry>::new());
    let saved = RwSignal::new(Vec::<SavedSearchDto>::new());
    let saved_totals = RwSignal::new(HashMap::<String, u64>::new());
    let selected = RwSignal::new(HashSet::<String>::new());
    let current_idx = RwSignal::new(0usize);
    let scroll_top = RwSignal::new(0.0f64);
    let viewport_h = RwSignal::new(0.0f64);
    let last_fetch_meta = RwSignal::new(Option::<(u64, u64, usize)>::None);

    let reset_queue_navigation = move || {
        current_idx.set(0);
        scroll_top.set(0.0);
        set_queue_dom_scroll_top(0.0);
    };

    let help_open = RwSignal::new(false);
    let coding_hint = RwSignal::new(false);
    let field_focused = RwSignal::new(false);
    let save_name = RwSignal::new(String::new());
    let save_error = RwSignal::new(Option::<String>::None);
    let bulk_error = RwSignal::new(Option::<String>::None);
    let confirm_priv = RwSignal::new(Option::<(String, u64)>::None);
    let tag_open = RwSignal::new(false);

    Effect::new(move |_| {
        let root = params.with(|p| p.get("id").unwrap_or_default());
        if root.is_empty() {
            error.set(Some("Missing matter id in route.".into()));
            return;
        }
        root_sig.set(root.clone());
        selected.set(HashSet::new());
        offset.set(0);
        last_fetch_meta.set(None);
        reset_queue_navigation();
        leptos::task::spawn_local({
            let root = root.clone();
            async move {
                match tauri_invoke::<Vec<CodeCatalogEntry>, _>(
                    "review_code_catalog",
                    &RootArgs { root: root.clone() },
                )
                .await
                {
                    Ok(c) => catalog.set(c),
                    Err(e) => error.set(Some(format!("Code catalog: {e}"))),
                }
                match tauri_invoke::<Vec<SavedSearchDto>, _>(
                    "saved_searches_list",
                    &RootArgs { root },
                )
                .await
                {
                    Ok(s) => saved.set(s),
                    Err(e) => error.set(Some(format!("Saved searches: {e}"))),
                }
            }
        });
    });

    Effect::new(move |_| {
        let root = root_sig.get();
        let fj = filter_json.get();
        let kw = keyword.get();
        let off = offset.get();
        let ex = extras.get();
        let fam = include_family.get();
        let _tick = reload_tick.get();
        if root.is_empty() {
            return;
        }
        let fam_filter = match with_include_family(&fj, fam) {
            Ok(s) => s,
            Err(e) => {
                // Invalidate in-flight fetches so a late OK cannot overwrite this error.
                fetch_gen.update(|g| *g = g.wrapping_add(1));
                page.set(None);
                last_fetch_meta.set(None);
                error.set(Some(e));
                loading.set(false);
                return;
            }
        };
        let gen = fetch_gen.get_untracked().wrapping_add(1);
        fetch_gen.set(gen);
        loading.set(true);
        error.set(None);
        let chip_for_total = active_chip.get_untracked();
        let record_saved_total = saved.get_untracked().iter().any(|s| s.id == chip_for_total);
        leptos::task::spawn_local(async move {
            let result = tauri_invoke::<ReviewQueuePage, _>(
                "review_queue_page",
                &ReviewQueuePageArgs {
                    root,
                    filter_json: Some(fam_filter),
                    keyword: if kw.trim().is_empty() { None } else { Some(kw) },
                    limit: Some(PAGE_LIMIT),
                    offset: Some(off),
                    extras: Some(ex),
                },
            )
            .await;
            if fetch_gen.get_untracked() != gen {
                return;
            }
            match result {
                Ok(p) => {
                    if record_saved_total {
                        let total = p.total;
                        saved_totals.update(|m| {
                            m.insert(chip_for_total, total);
                        });
                    }
                    last_fetch_meta.set(Some((p.offset, p.total, p.rows.len())));
                    if p.rows.is_empty() && p.total > 0 {
                        // Keep last good page. Clamp Effect owns offset_after_empty_page;
                        // gap is offset < total (not a clamp). Do not call the clamp helper here.
                        if p.offset < p.total {
                            error.set(Some(
                                "This page has no rows, but the queue still has items. Use Prev/Next."
                                    .into(),
                            ));
                        }
                        loading.set(false);
                    } else {
                        page.set(Some(p));
                        loading.set(false);
                    }
                }
                Err(e) => {
                    // Do not write saved chip totals on error (esp. fts_unavailable → fake 0).
                    page.set(None);
                    last_fetch_meta.set(None);
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    });

    // Dedicated clamp Effect — never in the render closure. Write offset only when changed.
    // Ignore stale last_fetch_meta from a previous offset (Next from a gap must fetch first).
    Effect::new(move |_| {
        let Some((meta_off, total, fetched_len)) = last_fetch_meta.get() else {
            return;
        };
        let off = offset.get();
        if let Some(new) =
            clamp_offset_for_fetch_meta(off, meta_off, total, fetched_len, PAGE_LIMIT)
        {
            if new != off {
                offset.set(new);
            }
        }
    });

    Effect::new(move |_| {
        let Some(p) = page.get() else {
            return;
        };
        let n = p.rows.len();
        let i = current_idx.get();
        let next = if n == 0 { 0 } else { i.min(n - 1) };
        if next != i {
            current_idx.set(next);
            if n > 0 {
                reveal_row(next, scroll_top, viewport_h);
            }
        }
    });

    let chrome = use_context::<QueueChromeCtx>();

    Effect::new(move |_| {
        let Some(ctx) = chrome else {
            return;
        };
        ctx.queue_range.set(
            last_fetch_meta
                .get()
                .map(|(offset, total, fetched)| QueueRange {
                    offset,
                    fetched,
                    total,
                }),
        );
    });

    Effect::new(move |_| {
        let Some(ctx) = chrome else {
            return;
        };
        let Some(raw) = ctx.goto_request.get() else {
            return;
        };
        ctx.goto_request.set(None);
        let q = raw.trim().to_string();
        if q.is_empty() {
            return;
        }
        ctx.goto_miss.set(None);
        if let Ok(n) = q.parse::<i64>() {
            if let Some(p) = page.get() {
                if let Some((idx, row)) = p
                    .rows
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.review_order == Some(n))
                {
                    current_idx.set(idx);
                    reveal_row(idx, scroll_top, viewport_h);
                    let root = root_sig.get();
                    let fj = filter_json.get();
                    let fam = include_family.get();
                    let kw = keyword.get();
                    let fam_filter = with_include_family(&fj, fam).unwrap_or(fj);
                    let href = review_doc_href(&root, &row.id, Some(&fam_filter), Some(&kw));
                    navigate.with_value(|nav| {
                        nav(&href, Default::default());
                    });
                    return;
                }
            }
            ctx.goto_miss
                .set(Some(control_not_on_page(n, last_fetch_meta.get())));
            return;
        }
        active_chip.set("goto-subject".into());
        filter_json.set(subject_contains_json(&q, include_family.get()));
        keyword.set(String::new());
        keyword_draft.set(String::new());
        offset.set(0);
        selected.set(HashSet::new());
        current_idx.set(0);
        scroll_top.set(0.0);
        set_queue_dom_scroll_top(0.0);
    });

    on_cleanup(move || {
        if let Some(ctx) = chrome {
            ctx.queue_range.set(None);
            ctx.goto_miss.set(None);
        }
    });

    QUEUE_VIEWPORT_H.with(|cell| cell.set(Some(viewport_h)));
    install_queue_resize_once();
    on_cleanup(|| {
        QUEUE_VIEWPORT_H.with(|cell| cell.set(None));
    });

    Effect::new(move |_| {
        let _ = page.get();
        measure_queue_viewport(viewport_h);
        install_queue_resize_once();
        let Some(window) = web_sys::window() else {
            return;
        };
        let cb = Closure::once(Box::new(move || {
            measure_queue_viewport(viewport_h);
        }) as Box<dyn FnOnce()>);
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), 0);
        cb.forget();
    });

    view! {
        <section
            class="queue-page"
            tabindex="-1"
            attr:aria-label=move || {
                queue_title_text(
                    &active_chip.get(),
                    page.get().map(|p| p.total).unwrap_or(0),
                    &saved.get(),
                )
            }
            on:keydown=move |ev: KeyboardEvent| {
                if field_focused.get() || queue_shortcut_blocked(&ev) {
                    // Esc is exempt from the focus gate: close overlays and clear bulk selection.
                    if ev.key() == "Escape" {
                        help_open.set(false);
                        confirm_priv.set(None);
                        tag_open.set(false);
                        coding_hint.set(false);
                        selected.set(HashSet::new());
                    }
                    return;
                }
                let key = ev.key();
                match key.as_str() {
                    "?" => {
                        ev.prevent_default();
                        help_open.update(|v| *v = !*v);
                    }
                    "Escape" => {
                        help_open.set(false);
                        confirm_priv.set(None);
                        tag_open.set(false);
                        coding_hint.set(false);
                        selected.set(HashSet::new());
                    }
                    "/" => {
                        ev.prevent_default();
                        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                            if let Some(el) = doc.get_element_by_id("queue-keyword") {
                                if let Ok(input) = el.dyn_into::<HtmlInputElement>() {
                                    let _ = input.focus();
                                }
                            }
                        }
                    }
                    "ArrowDown" if ev.shift_key() => {
                        ev.prevent_default();
                        if let Some(p) = page.get() {
                            let i = current_idx.get();
                            if let Some(row) = p.rows.get(i) {
                                let root = root_sig.get();
                                let fj = filter_json.get();
                                let fam = include_family.get();
                                let kw = keyword.get();
                                let fam_filter = with_include_family(&fj, fam).unwrap_or(fj);
                                let href = review_doc_href(&root, &row.id, Some(&fam_filter), Some(&kw));
                                navigate.with_value(|nav| {
                                    nav(&href, Default::default());
                                });
                            }
                        }
                    }
                    "ArrowDown" => {
                        ev.prevent_default();
                        let n = page.get().map(|p| p.rows.len()).unwrap_or(0);
                        if n > 0 {
                            let next = (current_idx.get() + 1).min(n.saturating_sub(1));
                            current_idx.set(next);
                            reveal_row(next, scroll_top, viewport_h);
                        }
                    }
                    "ArrowUp" => {
                        ev.prevent_default();
                        let n = page.get().map(|p| p.rows.len()).unwrap_or(0);
                        if n > 0 {
                            let next = current_idx.get().saturating_sub(1);
                            current_idx.set(next);
                            reveal_row(next, scroll_top, viewport_h);
                        }
                    }
                    "Enter" => {
                        ev.prevent_default();
                        if let Some(p) = page.get() {
                            let i = current_idx.get();
                            reveal_row(i, scroll_top, viewport_h);
                            if let Some(row) = p.rows.get(i) {
                                let root = root_sig.get();
                                let fj = filter_json.get();
                                let fam = include_family.get();
                                let kw = keyword.get();
                                let fam_filter = with_include_family(&fj, fam).unwrap_or(fj);
                                let href = review_doc_href(&root, &row.id, Some(&fam_filter), Some(&kw));
                                navigate.with_value(|nav| {
                                    nav(&href, Default::default());
                                });
                            }
                        }
                    }
                    " " => {
                        ev.prevent_default();
                        if let Some(p) = page.get() {
                            let i = current_idx.get();
                            reveal_row(i, scroll_top, viewport_h);
                            if let Some(row) = p.rows.get(i) {
                                let id = row.id.clone();
                                selected.update(|set| {
                                    if set.contains(&id) {
                                        set.remove(&id);
                                    } else {
                                        set.insert(id);
                                    }
                                });
                            }
                        }
                    }
                    "1" | "2" | "3" | "p" | "r" | "[" | "]" => {
                        coding_hint.set(true);
                    }
                    _ => {}
                }
            }
        >
            <div class="queue-layout">
                <nav class="queue-rail" aria-label="Queues">
                    <div class="queue-rail-list" role="toolbar" aria-label="Queue filters">
                        <button
                            class=move || if active_chip.get() == "unreviewed" { "chip-btn active" } else { "chip-btn" }
                            on:click=move |_| {
                                active_chip.set("unreviewed".into());
                                filter_json.set(preset_uncoded_json());
                                include_family.set(false);
                                offset.set(0);
                                selected.set(HashSet::new());
                                reset_queue_navigation();
                            }
                        >
                            <span>"Unreviewed"</span>
                            <span>
                                {move || {
                                    if active_chip.get() == "unreviewed" {
                                        page.get().map(|p| p.total.to_string()).unwrap_or_default()
                                    } else {
                                        String::new()
                                    }
                                }}
                            </span>
                        </button>
                        <button
                            class=move || if active_chip.get() == "privileged" { "chip-btn active" } else { "chip-btn" }
                            on:click=move |_| {
                                active_chip.set("privileged".into());
                                filter_json.set(preset_privilege_json());
                                include_family.set(false);
                                offset.set(0);
                                selected.set(HashSet::new());
                                reset_queue_navigation();
                            }
                        >
                            <span>"Privileged"</span>
                            <span>
                                {move || {
                                    if active_chip.get() == "privileged" {
                                        page.get().map(|p| p.total.to_string()).unwrap_or_default()
                                    } else {
                                        String::new()
                                    }
                                }}
                            </span>
                        </button>
                        <button
                            class=move || if active_chip.get() == "responsive" { "chip-btn active" } else { "chip-btn" }
                            on:click=move |_| {
                                active_chip.set("responsive".into());
                                filter_json.set(preset_responsive_json());
                                include_family.set(false);
                                offset.set(0);
                                selected.set(HashSet::new());
                                reset_queue_navigation();
                            }
                        >
                            <span>"Responsive"</span>
                            <span>
                                {move || {
                                    if active_chip.get() == "responsive" {
                                        page.get().map(|p| p.total.to_string()).unwrap_or_default()
                                    } else {
                                        String::new()
                                    }
                                }}
                            </span>
                        </button>
                    </div>
                    <div class="queue-rail-heading">"Saved searches"</div>
                    <div class="queue-rail-list">
                        <For
                            each=move || saved.get()
                            key=|s| s.id.clone()
                            children=move |s| {
                                let name = s.name.clone();
                                let fj = s.filter_json.clone();
                                let kw = s.keyword.clone().unwrap_or_default();
                                let sid = s.id.clone();
                                let sid_class = sid.clone();
                                let sid_label = sid.clone();
                                view! {
                                    <button
                                        class=move || if active_chip.get() == sid_class { "chip-btn active" } else { "chip-btn" }
                                        on:click={
                                            let fj = fj.clone();
                                            let kw = kw.clone();
                                            let sid = sid.clone();
                                            move |_| {
                                                active_chip.set(sid.clone());
                                                include_family.set(read_include_family(&fj));
                                                filter_json.set(fj.clone());
                                                keyword.set(kw.clone());
                                                keyword_draft.set(kw.clone());
                                                offset.set(0);
                                                selected.set(HashSet::new());
                                                reset_queue_navigation();
                                            }
                                        }
                                    >
                                        <span>{name.clone()}</span>
                                        <span>
                                            {move || {
                                                saved_totals
                                                    .get()
                                                    .get(&sid_label)
                                                    .map(|t| t.to_string())
                                                    .unwrap_or_default()
                                            }}
                                        </span>
                                    </button>
                                }
                            }
                        />
                    </div>
                    <div class="queue-rail-heading">"Later · no filter yet"</div>
                    <div
                        class="queue-rail-inert"
                        title="no filter yet"
                        aria-disabled="true"
                    >
                        <span>"Needs decision"</span>
                        <span>"0"</span>
                    </div>
                    <div
                        class="queue-rail-inert"
                        title="no filter yet"
                        aria-disabled="true"
                    >
                        <span>"Redaction QC"</span>
                        <span>"0"</span>
                    </div>
                    <div
                        class="queue-rail-inert"
                        title="no filter yet"
                        aria-disabled="true"
                    >
                        <span>"Consistency"</span>
                        <span>"0"</span>
                    </div>
                </nav>
                <div class="queue-main">
            <div class="queue-toolbar">
                <h1>
                    {move || {
                        queue_title_text(
                            &active_chip.get(),
                            page.get().map(|p| p.total).unwrap_or(0),
                            &saved.get(),
                        )
                    }}
                </h1>
                <div class="filter-controls">
                    <input
                        id="queue-keyword"
                        type="search"
                        placeholder="Keyword (Tantivy) — / to focus"
                        prop:value=move || keyword_draft.get()
                        on:focus=move |_| field_focused.set(true)
                        on:blur=move |_| field_focused.set(false)
                        on:input=move |ev| keyword_draft.set(event_target_value(&ev))
                        on:keydown=move |ev: KeyboardEvent| {
                            if ev.key() == "Enter" {
                                keyword.set(keyword_draft.get());
                                offset.set(0);
                                reset_queue_navigation();
                            }
                        }
                    />
                    <label class="check-label">
                        <input
                            type="checkbox"
                            prop:checked=move || include_family.get()
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                include_family.set(on);
                                match with_include_family(&filter_json.get(), on) {
                                    Ok(patched) => {
                                        filter_json.set(patched);
                                        error.set(None);
                                    }
                                    Err(e) => error.set(Some(e)),
                                }
                                offset.set(0);
                                reset_queue_navigation();
                            }
                        />
                        "Include family"
                    </label>
                    <label class="check-label">
                        <input
                            type="checkbox"
                            prop:checked=move || extras.get()
                            on:change=move |ev| {
                                extras.set(event_target_checked(&ev));
                            }
                        />
                        "Lead/QC columns"
                    </label>
                    <input
                        type="text"
                        placeholder="Save search name"
                        prop:value=move || save_name.get()
                        on:focus=move |_| field_focused.set(true)
                        on:blur=move |_| field_focused.set(false)
                        on:input=move |ev| save_name.set(event_target_value(&ev))
                    />
                    <button on:click=move |_| {
                        let name = save_name.get().trim().to_string();
                        if name.is_empty() {
                            save_error.set(Some("Name is required.".into()));
                            return;
                        }
                        let root = root_sig.get();
                        let fj = match with_include_family(&filter_json.get(), include_family.get()) {
                            Ok(s) => s,
                            Err(e) => {
                                save_error.set(Some(e));
                                return;
                            }
                        };
                        let kw = keyword.get();
                        save_error.set(None);
                        leptos::task::spawn_local(async move {
                            match tauri_invoke::<SavedSearchDto, _>(
                                "saved_search_upsert",
                                &SavedSearchUpsertArgs {
                                    root: root.clone(),
                                    name,
                                    filter_json: fj,
                                    keyword: if kw.trim().is_empty() { None } else { Some(kw) },
                                    description: None,
                                    id: None,
                                },
                            ).await {
                                Ok(_) => {
                                    save_name.set(String::new());
                                    match tauri_invoke::<Vec<SavedSearchDto>, _>(
                                        "saved_searches_list",
                                        &RootArgs { root },
                                    ).await {
                                        Ok(s) => saved.set(s),
                                        Err(e) => save_error.set(Some(format!("List after save failed: {e}"))),
                                    }
                                }
                                Err(e) => save_error.set(Some(format!("Save failed: {e}"))),
                            }
                        });
                    }>"Save search"</button>
                </div>
                <Show when=move || save_error.get().is_some()>
                    <p class="error">{move || save_error.get().unwrap_or_default()}</p>
                </Show>
            </div>

            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || loading.get()>
                <p class="empty">"Loading…"</p>
            </Show>

            <div class="bulk-bar" role="region" aria-label="Bulk tag">
                <button
                    on:click=move |_| {
                        if let Some(p) = page.get() {
                            selected.set(p.rows.iter().map(|r| r.id.clone()).collect());
                        }
                    }
                >
                    {move || {
                        format!(
                            "Select page ({})",
                            page.get().map(|p| p.rows.len()).unwrap_or(0)
                        )
                    }}
                </button>
                <Show when=move || !selected.get().is_empty()>
                    <span>{move || format!("{} selected", selected.get().len())}</span>
                    <button on:click=move |_| tag_open.update(|v| *v = !*v)>"Tag…"</button>
                    <Show when=move || tag_open.get()>
                        <div class="tag-picker">
                            <For
                                each=move || catalog.get()
                                key=|c| c.id.clone()
                                children=move |c| {
                                    let id = c.id.clone();
                                    let label = c.label.clone();
                                    let is_priv = c.group_key == "privilege" || c.key == "privilege";
                                    view! {
                                        <button on:click=move |_| {
                                            let root = root_sig.get();
                                            let ids: Vec<String> = selected.get().into_iter().collect();
                                            if ids.is_empty() { return; }
                                            let code_id = id.clone();
                                            bulk_error.set(None);
                                            leptos::task::spawn_local(async move {
                                                match tauri_invoke::<ReviewCodesPreview, _>(
                                                    "review_codes_preview",
                                                    &ReviewCodesPreviewArgs {
                                                        root: root.clone(),
                                                        item_ids: ids.clone(),
                                                        add_code_ids: vec![code_id.clone()],
                                                        remove_code_ids: vec![],
                                                    },
                                                ).await {
                                                    Ok(prev) => {
                                                        if is_priv && prev.privilege_would_change > 0 {
                                                            confirm_priv.set(Some((code_id, prev.privilege_would_change)));
                                                        } else {
                                                            apply_codes_async(
                                                                root, ids, code_id, bulk_error, confirm_priv,
                                                                tag_open, selected, reload_tick,
                                                            );
                                                        }
                                                    }
                                                    Err(e) => bulk_error.set(Some(format!("Preview failed: {e}"))),
                                                }
                                            });
                                        }>{label}</button>
                                    }
                                }
                            />
                        </div>
                    </Show>
                    <Show when=move || bulk_error.get().is_some()>
                        <p class="error">{move || bulk_error.get().unwrap_or_default()}</p>
                    </Show>
                </Show>
            </div>

            <Show when=move || confirm_priv.get().is_some()>
                {move || confirm_priv.get().map(|(code_id, n)| {
                    view! {
                        <div class="confirm-bar" role="alertdialog" aria-label="Privilege confirm">
                            <p>{format!("This changes Privilege coding on {n} items.")}</p>
                            <button class="primary" on:click={
                                let code_id = code_id.clone();
                                move |_| {
                                    let root = root_sig.get();
                                    let ids: Vec<String> = selected.get().into_iter().collect();
                                    apply_codes_async(
                                        root, ids, code_id.clone(), bulk_error, confirm_priv,
                                        tag_open, selected, reload_tick,
                                    );
                                }
                            }>"Confirm"</button>
                            <button on:click=move |_| confirm_priv.set(None)>"Cancel"</button>
                        </div>
                    }
                })}
            </Show>

            <Show when=move || help_open.get()>
                <div class="help-overlay" role="dialog" aria-label="Queue shortcuts">
                    <h2>"Queue shortcuts"</h2>
                    <ul>
                        <li>"↑ ↓ — move current row"</li>
                        <li>"Enter / Shift+↓ — open review window"</li>
                        <li>"Space — toggle checkbox"</li>
                        <li>"/ — focus keyword"</li>
                        <li>"Ctrl+K — focus Go-to (Control# or subject)"</li>
                        <li>"? — this overlay"</li>
                        <li>"Esc — close overlay / clear selection"</li>
                        <li>"1 2 3 p r [ ] — coding shortcuts land in the review window (0112)"</li>
                    </ul>
                    <button on:click=move |_| help_open.set(false)>"Close"</button>
                </div>
            </Show>
            <Show when=move || coding_hint.get()>
                <p class="empty" role="status">"Coding shortcuts land in the review window (0112)."</p>
            </Show>

            <div class="queue-pager">
                <button
                    prop:disabled=move || offset.get() == 0
                    on:click=move |_| {
                        offset.update(|o| *o = o.saturating_sub(PAGE_LIMIT));
                        reset_queue_navigation();
                    }
                >"Prev page"</button>
                <button
                    prop:disabled=move || {
                        let off = offset.get();
                        if let Some((_, total, fetched_len)) = last_fetch_meta.get() {
                            return next_page_disabled(off, total, fetched_len, PAGE_LIMIT);
                        }
                        page.get()
                            .map(|p| next_page_disabled(off, p.total, p.rows.len(), PAGE_LIMIT))
                            .unwrap_or(true)
                    }
                    on:click=move |_| {
                        offset.update(|o| *o += PAGE_LIMIT);
                        reset_queue_navigation();
                    }
                >"Next page"</button>
            </div>

            {move || {
                    if page.get().is_none() {
                        // Error banner / loading handle status — do not fake an empty corpus.
                        return view! { <></> }.into_any();
                    }
                    let Some(p) = page.get() else {
                        return view! { <></> }.into_any();
                    };
                    if p.total == 0 {
                        return view! { <p class="empty">"0 in queue"</p> }.into_any();
                    }
                    let fetched_len = p.rows.len();
                    if fetched_len == 0 {
                        // total > 0 with no last-good rows: banner already set; do not lie "0 in queue".
                        return view! { <></> }.into_any();
                    }
                    let show_extras = p.extras;
                    let grid_class = if show_extras {
                        "queue-grid extras"
                    } else {
                        "queue-grid"
                    };
                    view! {
                        <div class=grid_class role="grid" aria-rowcount=fetched_len.to_string()>
                            <div class="queue-header" role="row">
                                <span role="columnheader"></span>
                                <span role="columnheader">"Control#"</span>
                                <span role="columnheader">"Date"</span>
                                <span role="columnheader">"From"</span>
                                <span role="columnheader">"Subject"</span>
                                <span role="columnheader">"Family"</span>
                                <span role="columnheader">"Resp"</span>
                                <span role="columnheader">"Privilege"</span>
                                <Show when=move || show_extras>
                                    <span role="columnheader">"Custodian"</span>
                                    <span role="columnheader">"Withhold"</span>
                                    <span role="columnheader">"Conf"</span>
                                    <span role="columnheader">"Produced"</span>
                                </Show>
                            </div>
                            <div
                                id="queue"
                                class="queue-viewport"
                                tabindex="-1"
                                on:scroll=move |ev: Event| {
                                    if let Some(t) = ev.current_target() {
                                        if let Ok(el) = t.dyn_into::<HtmlElement>() {
                                            scroll_top.set(el.scroll_top() as f64);
                                            viewport_h.set(el.client_height() as f64);
                                        }
                                    }
                                }
                            >
                                {move || {
                                    let Some(p) = page.get() else {
                                        return view! { <></> }.into_any();
                                    };
                                    let fetched_len = p.rows.len();
                                    if fetched_len == 0 {
                                        return view! { <></> }.into_any();
                                    }
                                    let spacer = fetched_len as f64 * ROW_HEIGHT;
                                    let (start, end) = visible_range(
                                        scroll_top.get(),
                                        viewport_h.get(),
                                        ROW_HEIGHT,
                                        fetched_len,
                                        OVERSCAN,
                                    );
                                    let cur = current_idx.get();
                                    let sel = selected.get();
                                    let top_pad = start as f64 * ROW_HEIGHT;
                                    let parent_fields: HashMap<
                                        String,
                                        (Option<String>, Option<String>, Option<String>),
                                    > = p
                                        .rows
                                        .iter()
                                        .map(|r| {
                                            (
                                                r.id.clone(),
                                                (r.date.clone(), r.from_addr.clone(), r.subject.clone()),
                                            )
                                        })
                                        .collect();
                                    let visible: Vec<(usize, QueueRow)> = p.rows[start..end]
                                        .iter()
                                        .cloned()
                                        .enumerate()
                                        .map(|(i, r)| (start + i, r))
                                        .collect();
                                    view! {
                                <div class="queue-spacer" style=format!("height:{spacer}px;position:relative;")>
                                    <div
                                        class="queue-window"
                                        style=format!("transform:translateY({top_pad}px);")
                                    >
                                        <For
                                            each=move || visible.clone()
                                            key=|(_, r)| r.id.clone()
                                            children=move |(idx, row)| {
                                                let id = row.id.clone();
                                                let id_cb = id.clone();
                                                let id_open = id.clone();
                                                let tip = id.clone();
                                                let checked = sel.contains(&id);
                                                let is_current = idx == cur;
                                                let indent = row.parent_item_id.is_some();
                                                let priv_coded = row.privilege_coded;
                                                let withhold = row.withhold;
                                                let conf = row.confidential.unwrap_or(false);
                                                let parent = row.parent_item_id.as_ref().and_then(|pid| {
                                                    parent_fields.get(pid).map(|(d, f, s)| {
                                                        (d.as_deref(), f.as_deref(), s.as_deref())
                                                    })
                                                });
                                                let cells = family_cell_text(
                                                    row.parent_item_id.as_deref(),
                                                    row.date.as_deref(),
                                                    row.from_addr.as_deref(),
                                                    row.subject.as_deref(),
                                                    parent,
                                                );
                                                let from = cells.from.clone();
                                                let from_title = from.clone();
                                                let subject = cells.subject.clone();
                                                let subject_title = subject.clone();
                                                let date = cells.date;
                                                let custodian =
                                                    row.custodian.clone().unwrap_or_else(|| "—".into());
                                                let custodian_title = custodian.clone();
                                                let ctrl = control_number(row.review_order);
                                                let fam = row.family_size.to_string();
                                                let resp = resp_display(&row.resp);
                                                view! {
                                                    <div
                                                        class="queue-row"
                                                        role="row"
                                                        tabindex="0"
                                                        aria-selected=is_current.to_string()
                                                        data-current=is_current.to_string()
                                                        style=format!("height:{}px;", ROW_HEIGHT)
                                                        on:focus=move |_| current_idx.set(idx)
                                                        on:click=move |_| {
                                                            current_idx.set(idx);
                                                            let root = root_sig.get();
                                                            let fj = filter_json.get();
                                                            let fam = include_family.get();
                                                            let kw = keyword.get();
                                                            let fam_filter =
                                                                with_include_family(&fj, fam).unwrap_or(fj);
                                                            let href = review_doc_href(
                                                                &root,
                                                                &id_open,
                                                                Some(&fam_filter),
                                                                Some(&kw),
                                                            );
                                                            navigate.with_value(|nav| {
                                                                nav(&href, Default::default());
                                                            });
                                                        }
                                                    >
                                                        <span role="gridcell">
                                                            <input
                                                                type="checkbox"
                                                                prop:checked=checked
                                                                on:click=move |ev: MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    let id = id_cb.clone();
                                                                    selected.update(|set| {
                                                                        if set.contains(&id) {
                                                                            set.remove(&id);
                                                                        } else {
                                                                            set.insert(id);
                                                                        }
                                                                    });
                                                                }
                                                            />
                                                        </span>
                                                        <span role="gridcell" title=tip>{ctrl}</span>
                                                        <span role="gridcell">{date}</span>
                                                        <span role="gridcell" title=from_title>
                                                            {from}
                                                        </span>
                                                        <span
                                                            role="gridcell"
                                                            class=if indent { "subject indented" } else { "subject" }
                                                            title=subject_title
                                                        >
                                                            {subject}
                                                        </span>
                                                        <span role="gridcell">{fam}</span>
                                                        <span role="gridcell">{resp}</span>
                                                        <span role="gridcell">
                                                            {if priv_coded {
                                                                view! { <span class="priv-pill">"PRIV"</span> }.into_any()
                                                            } else {
                                                                view! { <span>"—"</span> }.into_any()
                                                            }}
                                                        </span>
                                                        <Show when=move || show_extras>
                                                            <span role="gridcell" title=custodian_title.clone()>
                                                                {custodian.clone()}
                                                            </span>
                                                            <span role="gridcell">
                                                                {if withhold {
                                                                    view! { <span class="priv-pill">"WITHHOLD"</span> }.into_any()
                                                                } else {
                                                                    view! { <span>"—"</span> }.into_any()
                                                                }}
                                                            </span>
                                                            <span role="gridcell">{if conf { "C" } else { "—" }}</span>
                                                            <span role="gridcell">"—"</span>
                                                        </Show>
                                                    </div>
                                                }
                                            }
                                        />
                                    </div>
                                </div>
                                    }.into_any()
                                }}
                            </div>
                        </div>
                    }.into_any()
                }}

            <div class="queue-footer" role="status">
                {move || {
                    let sel_n = selected.get().len();
                    match page.get() {
                        Some(p) => format!("{sel_n} selected · {} in queue", p.total),
                        None => format!("{sel_n} selected"),
                    }
                }}
            </div>
                </div>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_from_keeps_smtp_and_x500() {
        assert_eq!(display_from(None), "—");
        assert_eq!(display_from(Some("")), "—");
        assert_eq!(display_from(Some("  ")), "—");
        assert_eq!(display_from(Some("ada@example.com")), "ada@example.com");
        assert_eq!(
            display_from(Some(
                "/O=EXCH/OU=FIRST ADMINISTRATIVE GROUP/CN=RECIPIENTS/CN=ADA"
            )),
            "/O=EXCH/OU=FIRST ADMINISTRATIVE GROUP/CN=RECIPIENTS/CN=ADA"
        );
    }

    #[test]
    fn family_cell_text_copies_parent_or_attachment() {
        let own = family_cell_text(None, Some("2020-01-01"), Some("a@b.c"), Some("Hello"), None);
        assert_eq!(own.subject, "Hello");
        assert_eq!(own.from, "a@b.c");

        let missing_parent = family_cell_text(Some("parent"), None, None, None, None);
        assert_eq!(missing_parent.subject, "— attachment");
        assert_eq!(missing_parent.from, "—");

        let copied = family_cell_text(
            Some("parent"),
            None,
            None,
            None,
            Some((Some("2020-01-01"), Some("ada@ex.com"), Some("Parent subj"))),
        );
        assert_eq!(copied.date, "2020-01-01");
        assert_eq!(copied.from, "ada@ex.com");
        assert_eq!(copied.subject, "Parent subj");

        let not_blank = family_cell_text(
            Some("parent"),
            None,
            None,
            Some("Child subject"),
            Some((Some("2020-01-01"), Some("ada@ex.com"), Some("Parent subj"))),
        );
        assert_eq!(not_blank.subject, "Child subject");
    }

    #[test]
    fn subject_contains_json_uses_existing_filterspec_shape() {
        let json = subject_contains_json("invoice \"Q1\"", true);
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(v["version"], 1);
        assert_eq!(v["scope"], "review_corpus");
        assert_eq!(v["include_family"], true);
        assert_eq!(v["conditions"][0]["field"], "subject");
        assert_eq!(v["conditions"][0]["op"], "contains");
        assert_eq!(v["conditions"][0]["value"], "invoice \"Q1\"");
    }

    #[test]
    fn queue_title_and_goto_miss_copy() {
        assert_eq!(
            queue_title_text("unreviewed", 12, &[]),
            "Unreviewed 12 docs"
        );
        assert_eq!(
            control_not_on_page(850, Some((0, 1200, 500))),
            "Control# 850 not found in current page (Rows 1–500)"
        );
        assert_eq!(
            control_not_on_page(12, None),
            "Control# 12 not found in current page (Rows —)"
        );
    }

    #[test]
    fn css_queue_cells_ellipsis_and_rail() {
        let css = include_str!("../../styles/app.css");
        assert!(css.contains("grid-template-columns: 244px 1fr"));
        let cell_block = css
            .split(".queue-header [role=\"columnheader\"]")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .unwrap_or("");
        assert!(
            cell_block.contains(".queue-row [role=\"gridcell\"]"),
            "ellipsis lock must target queue gridcells, not a later rule"
        );
        assert!(cell_block.contains("min-width: 0"));
        assert!(cell_block.contains("overflow: hidden"));
        assert!(cell_block.contains("text-overflow: ellipsis"));
        assert!(cell_block.contains("white-space: nowrap"));
        assert!(css.contains("minmax(0, 32px)"));
        assert!(css.contains(
            "minmax(0, 32px) minmax(0, 72px) minmax(0, 110px) minmax(0, 140px) minmax(0, 1fr) minmax(0, 56px) minmax(0, 48px) minmax(0, 72px);"
        ));
        assert!(css.contains(
            "minmax(0, 32px) minmax(0, 72px) minmax(0, 110px) minmax(0, 140px) minmax(0, 1fr) minmax(0, 56px) minmax(0, 48px) minmax(0, 72px) minmax(0, 100px) minmax(0, 80px) minmax(0, 40px) minmax(0, 72px)"
        ));
        assert!(
            !css.contains("height: 640px"),
            "flex pane must not keep a magic 640px height"
        );
        assert!(!css.contains("Archivo"));
        assert!(!css.contains("#ec3013"));
    }

    #[test]
    fn control_number_is_review_order() {
        assert_eq!(control_number(Some(42)), "42");
        assert_eq!(control_number(None), "—");
        let src = include_str!("queue.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(prod.contains("title=tip"));
        assert!(prod.contains("title=from_title"));
        assert!(prod.contains("title=subject_title"));
        assert!(prod.contains("Select page ("));
        assert!(prod.contains("id=\"queue-keyword\""));
        assert!(prod.contains("Save search"));
        assert!(!prod.contains("ACME0001"));
    }
}
