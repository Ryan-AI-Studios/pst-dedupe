//! First-pass virtualized review queue (track 0111).

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlElement, HtmlInputElement, KeyboardEvent, MouseEvent};

use crate::invoke::{
    tauri_invoke, CodeCatalogEntry, QueueRow, ReviewApplyCodesArgs, ReviewCodesPreview,
    ReviewCodesPreviewArgs, ReviewQueuePage, ReviewQueuePageArgs, RootArgs, SavedSearchDto,
    SavedSearchUpsertArgs,
};
use crate::path_id::{encode_matter_id, matter_home_href_from_param, review_doc_href};
use crate::queue_window::{visible_range, OVERSCAN, ROW_HEIGHT};

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

fn resp_display(resp: &Option<String>) -> String {
    resp.clone().unwrap_or_else(|| "—".into())
}

fn format_date(d: &Option<String>) -> String {
    d.as_deref().unwrap_or("—").to_string()
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
    let viewport_h = RwSignal::new(640.0f64);

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
        current_idx.set(0);
        offset.set(0);
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
                    page.set(Some(p));
                    loading.set(false);
                }
                Err(e) => {
                    // Do not write saved chip totals on error (esp. fts_unavailable → fake 0).
                    page.set(None);
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    });

    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));
    let id_encoded = move || encode_matter_id(&root_sig.get());

    view! {
        <section
            class="queue-page"
            tabindex="-1"
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
                            current_idx.update(|i| *i = (*i + 1).min(n.saturating_sub(1)));
                        }
                    }
                    "ArrowUp" => {
                        ev.prevent_default();
                        current_idx.update(|i| *i = i.saturating_sub(1));
                    }
                    "Enter" => {
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
                    " " => {
                        ev.prevent_default();
                        if let Some(p) = page.get() {
                            let i = current_idx.get();
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
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
                <nav class="tabs" aria-label="Matter workspace">
                    <A href=move || format!("/matters/{}", id_encoded())>"Home"</A>
                    <A href=move || format!("/matters/{}/process", id_encoded())>"Process"</A>
                    <A href=move || format!("/matters/{}/review", id_encoded())>"Review"</A>
                    <A href=move || format!("/matters/{}/produce", id_encoded())>"Produce"</A>
                    <A href=move || format!("/matters/{}/admin", id_encoded())>"Admin"</A>
                </nav>
            </div>
            <h1>"Review"</h1>

            <div class="filter-bar">
                <div class="chip-row" role="toolbar" aria-label="Queue filters">
                    <button
                        class=move || if active_chip.get() == "unreviewed" { "chip-btn active" } else { "chip-btn" }
                        on:click=move |_| {
                            active_chip.set("unreviewed".into());
                            filter_json.set(preset_uncoded_json());
                            include_family.set(false);
                            offset.set(0);
                            selected.set(HashSet::new());
                            current_idx.set(0);
                        }
                    >"Unreviewed"</button>
                    <button
                        class=move || if active_chip.get() == "privileged" { "chip-btn active" } else { "chip-btn" }
                        on:click=move |_| {
                            active_chip.set("privileged".into());
                            filter_json.set(preset_privilege_json());
                            include_family.set(false);
                            offset.set(0);
                            selected.set(HashSet::new());
                            current_idx.set(0);
                        }
                    >"Privileged"</button>
                    <button
                        class=move || if active_chip.get() == "responsive" { "chip-btn active" } else { "chip-btn" }
                        on:click=move |_| {
                            active_chip.set("responsive".into());
                            filter_json.set(preset_responsive_json());
                            include_family.set(false);
                            offset.set(0);
                            selected.set(HashSet::new());
                            current_idx.set(0);
                        }
                    >"Responsive"</button>
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
                                            current_idx.set(0);
                                        }
                                    }
                                >{
                                    move || {
                                        match saved_totals.get().get(&sid_label) {
                                            Some(t) => format!("{name} ({t})"),
                                            None => name.clone(),
                                        }
                                    }
                                }</button>
                            }
                        }
                    />
                </div>
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
                    }>"Save"</button>
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

            <Show when=move || !selected.get().is_empty()>
                <div class="bulk-bar" role="region" aria-label="Bulk tag">
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
                </div>
            </Show>

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
                        current_idx.set(0);
                        scroll_top.set(0.0);
                    }
                >"Prev page"</button>
                <button
                    prop:disabled=move || {
                        page.get().map(|p| p.offset + p.rows.len() as u64 >= p.total).unwrap_or(true)
                    }
                    on:click=move |_| {
                        offset.update(|o| *o += PAGE_LIMIT);
                        current_idx.set(0);
                        scroll_top.set(0.0);
                    }
                >"Next page"</button>
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
                    if page.get().is_none() {
                        // Error banner / loading handle status — do not fake an empty corpus.
                        return view! { <></> }.into_any();
                    }
                    let Some(p) = page.get() else {
                        return view! { <></> }.into_any();
                    };
                    let fetched_len = p.rows.len();
                    if p.total == 0 || fetched_len == 0 {
                        return view! { <p class="empty">"0 in queue"</p> }.into_any();
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
                    let show_extras = p.extras;
                    let top_pad = start as f64 * ROW_HEIGHT;
                    let visible: Vec<(usize, QueueRow)> = p.rows[start..end]
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(i, r)| (start + i, r))
                        .collect();
                    view! {
                        <div class="queue-spacer" style=format!("height:{spacer}px;position:relative;")>
                            <div
                                class=if show_extras { "queue-window extras" } else { "queue-window" }
                                style=format!("transform:translateY({top_pad}px);")
                                role="grid"
                                aria-rowcount=fetched_len.to_string()
                            >
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
                                        let from = row.from_addr.clone().unwrap_or_else(|| "—".into());
                                        let subject = row.subject.clone().unwrap_or_else(|| "—".into());
                                        let custodian = row.custodian.clone().unwrap_or_else(|| "—".into());
                                        let ctrl = control_number(row.review_order);
                                        let date = format_date(&row.date);
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
                                                <span role="gridcell">{from}</span>
                                                <span
                                                    role="gridcell"
                                                    class=if indent { "subject indented" } else { "subject" }
                                                >{subject}</span>
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
                                                    <span role="gridcell">{custodian.clone()}</span>
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

            <div class="queue-footer" role="status">
                {move || {
                    let sel_n = selected.get().len();
                    match page.get() {
                        Some(p) => format!("{sel_n} selected · {} in queue", p.total),
                        None => format!("{sel_n} selected"),
                    }
                }}
            </div>
        </section>
    }
}
