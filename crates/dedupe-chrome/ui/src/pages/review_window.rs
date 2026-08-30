//! Three-pane review window (track 0112).

use std::cell::RefCell;

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement, KeyboardEvent};

use crate::invoke::{
    tauri_invoke, CodeCatalogEntry, FamilyMemberThin, ItemCodeInfo, ReviewCodesPreview,
    ReviewCodesPreviewArgs, ReviewDocument, ReviewDocumentArgs, ReviewDocumentBody,
    ReviewDocumentBodyArgs, ReviewUpsertNoteArgs, ReviewUpsertPrivilegeArgs, ReviewWindowApplyArgs,
    RootArgs,
};
use crate::path_id::{encode_matter_id, matter_home_href_from_param, review_doc_href};

const BASIS_OPTIONS: &[(&str, &str)] = &[
    ("attorney_client", "Attorney-Client"),
    ("work_product", "Work Product"),
    ("attorney_client_work_product", "AC+WP"),
    ("common_interest", "Common Interest"),
    ("other", "Other"),
];

#[derive(Clone, Debug)]
struct DittoSnap {
    resp: Option<String>,
    privilege: bool,
    basis: String,
    withhold: bool,
    confidential: bool,
}

thread_local! {
    static DITTO: RefCell<Option<DittoSnap>> = const { RefCell::new(None) };
}

fn ditto_get() -> Option<DittoSnap> {
    DITTO.with(|c| c.borrow().clone())
}

fn ditto_set(snap: DittoSnap) {
    DITTO.with(|c| *c.borrow_mut() = Some(snap));
}

fn codes_state(codes: &[ItemCodeInfo]) -> (Option<String>, bool, bool) {
    let mut resp = None;
    let mut privilege = false;
    let mut confidential = false;
    for c in codes {
        if c.group_key == "responsiveness" {
            resp = Some(c.key.clone());
        }
        if c.group_key == "privilege" || c.key == "privilege" {
            privilege = true;
        }
        if c.key == "confidential" {
            confidential = true;
        }
    }
    (resp, privilege, confidential)
}

fn catalog_id(catalog: &[CodeCatalogEntry], key: &str) -> Option<String> {
    catalog.iter().find(|c| c.key == key).map(|c| c.id.clone())
}

fn focus_id(id: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = doc.get_element_by_id(id) {
            if let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() {
                let _ = html.focus();
            }
        }
    }
}

fn shortcut_gated(ev: &KeyboardEvent) -> bool {
    let Some(target) = ev.target() else {
        return false;
    };
    let Ok(el) = target.dyn_into::<web_sys::Element>() else {
        return false;
    };
    let id = el.id();
    let tag = el.tag_name().to_ascii_lowercase();
    if id == "log-note" || id == "privilege-type" || id == "doc-find" {
        return true;
    }
    matches!(tag.as_str(), "textarea" | "select")
        || (tag == "input" && el.get_attribute("type").unwrap_or_default() == "search")
}

fn window_find(query: &str) {
    let Some(win) = web_sys::window() else {
        return;
    };
    if let Ok(func) = js_sys::Reflect::get(&win, &wasm_bindgen::JsValue::from_str("find")) {
        if let Ok(func) = func.dyn_into::<js_sys::Function>() {
            let _ = func.call1(&win, &wasm_bindgen::JsValue::from_str(query));
        }
    }
}

#[component]
pub fn ReviewWindow() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let navigate = StoredValue::new(use_navigate());

    let root_sig = RwSignal::new(String::new());
    let doc_id = RwSignal::new(String::new());
    let doc = RwSignal::new(Option::<ReviewDocument>::None);
    let body = RwSignal::new(Option::<ReviewDocumentBody>::None);
    let catalog = RwSignal::new(Vec::<CodeCatalogEntry>::new());
    let error = RwSignal::new(Option::<String>::None);
    let status = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(false);
    let pane = RwSignal::new(String::from("native"));
    let help_open = RwSignal::new(false);
    let v_pending = RwSignal::new(false);
    let family_propagate = RwSignal::new(false);
    let family_confirm = RwSignal::new(false);
    let family_priv_preview = RwSignal::new(Option::<u64>::None);
    let note_draft = RwSignal::new(String::new());
    let pending_resp = RwSignal::new(Option::<String>::None);
    let pending_priv = RwSignal::new(false);
    let pending_basis = RwSignal::new(String::from("attorney_client"));
    let pending_withhold = RwSignal::new(false);
    let pending_conf = RwSignal::new(false);
    let find_q = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    let home =
        move || params.with(|p| matter_home_href_from_param(&p.get("id").unwrap_or_default()));
    let queue_href = move || {
        params.with(|p| {
            format!(
                "/matters/{}/review",
                encode_matter_id(&p.get("id").unwrap_or_default())
            )
        })
    };

    Effect::new(move |_| {
        let root = params.with(|p| p.get("id").unwrap_or_default());
        let id = params.with(|p| p.get("docId").unwrap_or_default());
        if root.is_empty() || id.is_empty() {
            error.set(Some("Missing matter or document id.".into()));
            return;
        }
        root_sig.set(root.clone());
        doc_id.set(id.clone());
        loading.set(true);
        error.set(None);
        family_confirm.set(false);
        family_priv_preview.set(None);
        note_draft.set(String::new());
        let filter_json = query.with(|q| q.get("filter"));
        let keyword = query.with(|q| q.get("q"));
        leptos::task::spawn_local(async move {
            match tauri_invoke::<Vec<CodeCatalogEntry>, _>(
                "review_code_catalog",
                &RootArgs { root: root.clone() },
            )
            .await
            {
                Ok(c) => catalog.set(c),
                Err(e) => error.set(Some(format!("Code catalog: {e}"))),
            }
            match tauri_invoke::<ReviewDocument, _>(
                "review_document",
                &ReviewDocumentArgs {
                    root,
                    item_id: id,
                    filter_json,
                    keyword,
                },
            )
            .await
            {
                Ok(d) => {
                    let (resp, priv_on, conf) = codes_state(&d.codes);
                    pending_resp.set(resp);
                    pending_priv.set(priv_on);
                    pending_conf.set(conf);
                    pending_withhold.set(
                        d.privilege
                            .as_ref()
                            .map(|p| p.withhold != 0)
                            .unwrap_or(false),
                    );
                    family_propagate.set(false);
                    pending_basis.set(String::from("attorney_client"));
                    if let Some(p) = &d.privilege {
                        if !p.basis.is_empty() {
                            pending_basis.set(p.basis.clone());
                        }
                    }
                    doc.set(Some(d));
                    loading.set(false);
                }
                Err(e) => {
                    doc.set(None);
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    });

    Effect::new(move |_| {
        let root = root_sig.get();
        let id = doc_id.get();
        let pane_now = pane.get();
        if root.is_empty() || id.is_empty() {
            return;
        }
        if pane_now != "native" && pane_now != "text" {
            body.set(None);
            return;
        }
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ReviewDocumentBody, _>(
                "review_document_body",
                &ReviewDocumentBodyArgs {
                    root,
                    item_id: id,
                    pane: pane_now,
                },
            )
            .await
            {
                Ok(b) => body.set(Some(b)),
                Err(e) => error.set(Some(format!("Body: {e}"))),
            }
        });
    });

    let go_item = move |item: String| {
        let root = root_sig.get();
        let filter = query.with(|q| q.get("filter"));
        let keyword = query.with(|q| q.get("q"));
        let href = review_doc_href(&root, &item, filter.as_deref(), keyword.as_deref());
        navigate.with_value(|nav| {
            nav(&href, Default::default());
        });
    };

    let persist_and_maybe_next = StoredValue::new({
        let go_item = go_item;
        move |then_next: bool| {
            if saving.get_untracked() {
                return;
            }
            let Some(d) = doc.get_untracked() else {
                return;
            };
            let cat = catalog.get_untracked();
            let (cur_resp, cur_priv, cur_conf) = codes_state(&d.codes);
            let want_resp = pending_resp.get_untracked();
            let want_priv = pending_priv.get_untracked();
            let want_conf = pending_conf.get_untracked();
            let basis = pending_basis.get_untracked();
            let withhold = pending_withhold.get_untracked();
            let propagate = family_propagate.get_untracked();
            let note = note_draft.get_untracked();
            if want_priv && basis.trim().is_empty() {
                status.set(Some("Privilege type is required before save.".into()));
                focus_id("privilege-type");
                return;
            }
            if want_priv && note.trim().is_empty() {
                status.set(Some(
                    "Consider a log note for this privilege claim (not required).".into(),
                ));
            }
            let mut add = Vec::new();
            let mut remove = Vec::new();
            if want_resp != cur_resp {
                if let Some(k) = &want_resp {
                    if let Some(id) = catalog_id(&cat, k) {
                        add.push(id);
                    }
                }
            }
            if want_priv && !cur_priv {
                if let Some(id) = catalog_id(&cat, "privilege") {
                    add.push(id);
                }
            }
            if !want_priv && cur_priv {
                if let Some(id) = catalog_id(&cat, "privilege") {
                    remove.push(id);
                }
            }
            if want_conf && !cur_conf {
                if let Some(id) = catalog_id(&cat, "confidential") {
                    add.push(id);
                }
            }
            if !want_conf && cur_conf {
                if let Some(id) = catalog_id(&cat, "confidential") {
                    remove.push(id);
                }
            }
            let claim_dirty = want_priv
                && (d
                    .privilege
                    .as_ref()
                    .map(|p| p.basis.clone())
                    .unwrap_or_default()
                    != basis
                    || d.privilege
                        .as_ref()
                        .map(|p| p.withhold != 0)
                        .unwrap_or(false)
                        != withhold);
            if propagate
                && d.family_size > 1
                && !family_confirm.get_untracked()
                && (!add.is_empty() || !remove.is_empty())
            {
                family_confirm.set(true);
                family_priv_preview.set(None);
                if d.family_size <= 100 {
                    let root = root_sig.get_untracked();
                    let ids: Vec<String> = d.family_members.iter().map(|m| m.id.clone()).collect();
                    let add_ids = add.clone();
                    let remove_ids = remove.clone();
                    leptos::task::spawn_local(async move {
                        match tauri_invoke::<ReviewCodesPreview, _>(
                            "review_codes_preview",
                            &ReviewCodesPreviewArgs {
                                root,
                                item_ids: ids,
                                add_code_ids: add_ids,
                                remove_code_ids: remove_ids,
                            },
                        )
                        .await
                        {
                            Ok(p) => family_priv_preview.set(Some(p.privilege_would_change)),
                            Err(e) => status.set(Some(format!("Family preview: {e}"))),
                        }
                    });
                }
                return;
            }
            let root = root_sig.get_untracked();
            let item_id = d.item_id.clone();
            let next = d.next_id.clone();
            let note_to_save = note.trim().to_string();
            saving.set(true);
            leptos::task::spawn_local(async move {
                let mut failed = false;
                let adding_priv = add.iter().any(|id| {
                    cat.iter().any(|c| {
                        c.id == *id && (c.key == "privilege" || c.group_key == "privilege")
                    })
                });
                if !add.is_empty() || !remove.is_empty() {
                    match tauri_invoke::<serde_json::Value, _>(
                        "review_window_apply",
                        &ReviewWindowApplyArgs {
                            root: root.clone(),
                            item_ids: vec![item_id.clone()],
                            add_code_ids: add,
                            remove_code_ids: remove,
                            propagate_family: Some(propagate),
                            privilege_basis: if adding_priv {
                                Some(basis.clone())
                            } else {
                                None
                            },
                            withhold: Some(withhold),
                            include_on_log: Some(true),
                            privilege_description: None,
                        },
                    )
                    .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            status.set(Some(format!("Save failed: {e}")));
                            failed = true;
                        }
                    }
                }
                if !failed && want_priv && claim_dirty && !adding_priv {
                    match tauri_invoke::<serde_json::Value, _>(
                        "review_upsert_privilege",
                        &ReviewUpsertPrivilegeArgs {
                            root: root.clone(),
                            item_id: item_id.clone(),
                            basis: basis.clone(),
                            withhold: Some(withhold),
                            description: None,
                        },
                    )
                    .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            status.set(Some(format!("Privilege save failed: {e}")));
                            failed = true;
                        }
                    }
                }
                if !failed && !note_to_save.is_empty() {
                    if let Err(e) = tauri_invoke::<serde_json::Value, _>(
                        "review_upsert_note",
                        &ReviewUpsertNoteArgs {
                            root: root.clone(),
                            item_id: item_id.clone(),
                            body: note_to_save,
                            id: None,
                        },
                    )
                    .await
                    {
                        status.set(Some(format!("Note save failed: {e}")));
                        failed = true;
                    }
                }
                saving.set(false);
                family_confirm.set(false);
                family_priv_preview.set(None);
                if failed {
                    return;
                }
                note_draft.set(String::new());
                ditto_set(DittoSnap {
                    resp: want_resp,
                    privilege: want_priv,
                    basis,
                    withhold,
                    confidential: want_conf,
                });
                if then_next {
                    if let Some(nid) = next {
                        go_item(nid);
                    } else {
                        status.set(Some("End of queue".into()));
                    }
                } else {
                    status.set(Some("Saved.".into()));
                }
            });
        }
    });

    view! {
        <section
            class="review-page"
            tabindex="-1"
            on:keydown=move |ev: KeyboardEvent| {
                if ev.key() == "Escape" {
                    if help_open.get() {
                        ev.prevent_default();
                        help_open.set(false);
                        return;
                    }
                    ev.prevent_default();
                    let href = queue_href();
                    navigate.with_value(|nav| nav(&href, Default::default()));
                    return;
                }
                if ev.ctrl_key() && ev.key().eq_ignore_ascii_case("f") {
                    ev.prevent_default();
                    focus_id("doc-find");
                    return;
                }
                if shortcut_gated(&ev) {
                    return;
                }
                let key = ev.key();
                if v_pending.get() {
                    v_pending.set(false);
                    match key.as_str() {
                        "n" | "N" => { ev.prevent_default(); pane.set("native".into()); return; }
                        "t" | "T" => { ev.prevent_default(); pane.set("text".into()); return; }
                        "i" | "I" => { ev.prevent_default(); pane.set("image".into()); return; }
                        _ => {}
                    }
                }
                match key.as_str() {
                    "1" => { ev.prevent_default(); pending_resp.set(Some("responsive".into())); }
                    "2" => { ev.prevent_default(); pending_resp.set(Some("not_responsive".into())); }
                    "3" => { ev.prevent_default(); pending_resp.set(Some("needs_second_look".into())); }
                    "p" | "P" => {
                        ev.prevent_default();
                        let on = !pending_priv.get();
                        pending_priv.set(on);
                        if on && pending_basis.get().trim().is_empty() {
                            status.set(Some("Select a privilege type.".into()));
                            focus_id("privilege-type");
                        }
                    }
                    "d" => {
                        ev.prevent_default();
                        match ditto_get() {
                            None => status.set(Some("Nothing to ditto yet.".into())),
                            Some(s) => {
                                pending_resp.set(s.resp);
                                pending_priv.set(s.privilege);
                                pending_basis.set(s.basis);
                                pending_withhold.set(s.withhold);
                                pending_conf.set(s.confidential);
                            }
                        }
                    }
                    "D" => {
                        ev.prevent_default();
                        match ditto_get() {
                            None => status.set(Some("Nothing to ditto yet.".into())),
                            Some(s) => {
                                pending_resp.set(s.resp);
                                pending_priv.set(s.privilege);
                                pending_basis.set(s.basis);
                                pending_withhold.set(s.withhold);
                                pending_conf.set(s.confidential);
                                persist_and_maybe_next.with_value(|f| f(true));
                            }
                        }
                    }
                    "Enter" => {
                        ev.prevent_default();
                        persist_and_maybe_next.with_value(|f| f(true));
                    }
                    "[" => {
                        ev.prevent_default();
                        if let Some(prev) = doc.get().and_then(|d| d.prev_id) {
                            go_item(prev);
                        }
                    }
                    "]" => {
                        ev.prevent_default();
                        if let Some(next) = doc.get().and_then(|d| d.next_id) {
                            go_item(next);
                        } else {
                            status.set(Some("End of queue".into()));
                        }
                    }
                    "r" | "R" => { ev.prevent_default(); pane.set("image".into()); }
                    "v" | "V" => { ev.prevent_default(); v_pending.set(true); }
                    "f" | "F" => { ev.prevent_default(); focus_id("family-card"); }
                    "?" => { ev.prevent_default(); help_open.update(|v| *v = !*v); }
                    "/" => { ev.prevent_default(); focus_id("doc-find"); }
                    _ => {}
                }
            }
        >
            <div class="toolbar">
                <A href=home>"← Matter home"</A>
                <A href=queue_href>"← Queue"</A>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="error">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || status.get().is_some()>
                <p class="empty" role="status">{move || status.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || help_open.get()>
                <div class="help-overlay" role="dialog" aria-label="Review window shortcuts">
                    <h2>"Review window shortcuts"</h2>
                    <ul>
                        <li>"1 — Responsive"</li>
                        <li>"2 — Not responsive"</li>
                        <li>"3 — Needs review"</li>
                        <li>"p — Privilege"</li>
                        <li>"d — Ditto · Shift+d ditto + next"</li>
                        <li>"Enter — Save & Next"</li>
                        <li>"[ ] — previous / next document"</li>
                        <li>"r — Image tab (0114 stub)"</li>
                        <li>"v then n / t / i — Native / Text / Image"</li>
                        <li>"f — family card"</li>
                        <li>"/ — find in document"</li>
                        <li>"Esc — close overlay or return to queue"</li>
                    </ul>
                    <p>"Privilege is `p`, not `3`."</p>
                    <button on:click=move |_| help_open.set(false)>"Close"</button>
                </div>
            </Show>

            <Show when=move || doc.get().is_some()>
                {move || doc.get().map(|d| {
                    let members = d.family_members.clone();
                    let cur_id = d.item_id.clone();
                    let family_size = d.family_size;
                    let truncated = d.family_truncated;
                    let apply_enabled = d.apply_to_family_enabled;
                    let headers = d.clone();
                    view! {
                        <div class="review-window">
                            <aside class="review-pane family-card" id="family-card" tabindex="-1">
                                <h2>"Related"</h2>
                                <p class="empty">
                                    {if truncated {
                                        format!("showing 100 of {family_size}")
                                    } else {
                                        format!("{family_size} family member(s)")
                                    }}
                                </p>
                                <For
                                    each=move || members.clone()
                                    key=|m: &FamilyMemberThin| m.id.clone()
                                    children=move |m| {
                                        let id = m.id.clone();
                                        let indent = m.parent_item_id.is_some();
                                        let current = id == cur_id;
                                        let label = m.subject.clone().unwrap_or_else(|| id.clone());
                                        view! {
                                            <button
                                                class=if indent {
                                                    if current { "member child current" } else { "member child" }
                                                } else if current {
                                                    "member current"
                                                } else {
                                                    "member"
                                                }
                                                on:click=move |_| go_item(id.clone())
                                            >{label}</button>
                                        }
                                    }
                                />
                                <label>
                                    <input
                                        type="checkbox"
                                        prop:checked=move || family_propagate.get()
                                        prop:disabled=move || !apply_enabled
                                        on:change=move |ev| {
                                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                family_propagate.set(el.checked());
                                            }
                                        }
                                    />
                                    " Apply to family (off by default)"
                                </label>
                            </aside>

                            <section class="review-pane">
                                <div class="viewer-tabs" role="tablist">
                                    <button role="tab" aria-selected=move || pane.get() == "native"
                                        on:click=move |_| pane.set("native".into())>"Native"</button>
                                    <button role="tab" aria-selected=move || pane.get() == "text"
                                        on:click=move |_| pane.set("text".into())>"Text"</button>
                                    <button role="tab" aria-selected=move || pane.get() == "image"
                                        on:click=move |_| pane.set("image".into())>"Image"</button>
                                    <button role="tab" aria-selected=move || pane.get() == "produced"
                                        on:click=move |_| pane.set("produced".into())>"Produced"</button>
                                </div>
                                <dl class="headers">
                                    <dt>"From"</dt><dd>{headers.from_addr.clone().unwrap_or_else(|| "—".into())}</dd>
                                    <dt>"To"</dt><dd>{headers.to_addrs_json.clone().unwrap_or_else(|| "—".into())}</dd>
                                    <dt>"Cc"</dt><dd>{headers.cc_addrs_json.clone().unwrap_or_else(|| "—".into())}</dd>
                                    <dt>"Subject"</dt><dd>{headers.subject.clone().unwrap_or_else(|| "—".into())}</dd>
                                    <dt>"Sent"</dt><dd>{headers.sent_at.clone().unwrap_or_else(|| "—".into())}</dd>
                                    <dt>"Received"</dt><dd>{headers.received_at.clone().unwrap_or_else(|| "—".into())}</dd>
                                    <dt>"Control#"</dt><dd>{headers.control_number.clone()}</dd>
                                    <dt>"Bates"</dt><dd>{format!("{} ({})", headers.bates, headers.bates_note)}</dd>
                                </dl>
                                <div class="find-row">
                                    <input
                                        id="doc-find"
                                        type="search"
                                        placeholder="Find in document"
                                        prop:value=move || find_q.get()
                                        on:input=move |ev| {
                                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                find_q.set(el.value());
                                            }
                                        }
                                        on:keydown=move |ev: KeyboardEvent| {
                                            if ev.key() == "Enter" {
                                                ev.prevent_default();
                                                window_find(&find_q.get());
                                            }
                                        }
                                    />
                                </div>
                                <Show when=move || pane.get() == "image">
                                    <p class="empty" id="document" tabindex="-1">"No raster yet (0114)."</p>
                                </Show>
                                <Show when=move || pane.get() == "produced">
                                    <p class="empty" id="document" tabindex="-1">"— · 0113"</p>
                                </Show>
                                <Show when=move || pane.get() == "native" || pane.get() == "text">
                                    {move || body.get().map(|b| {
                                        let banner = b.truncated;
                                        let text = b.text.clone();
                                        view! {
                                            <Show when=move || banner>
                                                <p class="empty" role="status">"Showing first 2 MiB"</p>
                                            </Show>
                                            <pre class="doc-body" id="document" tabindex="-1">{text}</pre>
                                        }
                                    })}
                                </Show>
                            </section>

                            <aside class="review-pane coding-pane">
                                <h2>"Code"</h2>
                                <fieldset class="code-radios">
                                    <legend>"Responsiveness"</legend>
                                    <label>
                                        <input type="radio" name="resp"
                                            prop:checked=move || pending_resp.get().as_deref() == Some("responsive")
                                            on:change=move |_| pending_resp.set(Some("responsive".into())) />
                                        " Responsive"
                                    </label>
                                    <label>
                                        <input type="radio" name="resp"
                                            prop:checked=move || pending_resp.get().as_deref() == Some("not_responsive")
                                            on:change=move |_| pending_resp.set(Some("not_responsive".into())) />
                                        " Non-responsive"
                                    </label>
                                    <label>
                                        <input type="radio" name="resp"
                                            prop:checked=move || pending_resp.get().as_deref() == Some("needs_second_look")
                                            on:change=move |_| pending_resp.set(Some("needs_second_look".into())) />
                                        " Needs review"
                                    </label>
                                </fieldset>
                                <div class="code-checks">
                                    <label>
                                        <input type="checkbox"
                                            prop:checked=move || pending_priv.get()
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                    pending_priv.set(el.checked());
                                                }
                                            }
                                        />
                                        " Privilege"
                                    </label>
                                    <label>
                                        "Type "
                                        <select id="privilege-type"
                                            prop:value=move || pending_basis.get()
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlSelectElement>().ok()) {
                                                    pending_basis.set(el.value());
                                                }
                                            }
                                        >
                                            <For
                                                each=|| BASIS_OPTIONS.iter().copied()
                                                key=|o| o.0
                                                children=move |(val, label)| {
                                                    view! { <option value=val>{label}</option> }
                                                }
                                            />
                                        </select>
                                    </label>
                                    <label>
                                        <input type="checkbox"
                                            prop:checked=move || pending_withhold.get()
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                    pending_withhold.set(el.checked());
                                                }
                                            }
                                        />
                                        " Withhold from produce"
                                    </label>
                                    <label>
                                        <input type="checkbox"
                                            prop:checked=move || pending_conf.get()
                                            on:change=move |ev| {
                                                if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                                    pending_conf.set(el.checked());
                                                }
                                            }
                                        />
                                        " Confidential"
                                    </label>
                                </div>
                                <label>
                                    "Log note"
                                    <textarea id="log-note" rows="4"
                                        prop:value=move || note_draft.get()
                                        on:input=move |ev| {
                                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()) {
                                                note_draft.set(el.value());
                                            }
                                        }
                                    ></textarea>
                                </label>
                                <p class="ai-off">"AI off"</p>
                                <div class="history">
                                    <h3>"History"</h3>
                                    <For
                                        each=move || d.codes.clone()
                                        key=|c| format!("{}-{}", c.code_id, c.set_at)
                                        children=move |c| {
                                            view! {
                                                <p>{format!("{} · {} · {}", c.label, c.set_by, c.set_at)}</p>
                                            }
                                        }
                                    />
                                    <For
                                        each=move || d.notes.clone()
                                        key=|n| n.id.clone()
                                        children=move |n| {
                                            view! {
                                                <p class="empty">{format!("{} · {} · {}", n.body, n.updated_by, n.updated_at)}</p>
                                            }
                                        }
                                    />
                                </div>
                                <Show when=move || family_confirm.get()>
                                    <div class="confirm-bar" role="alertdialog">
                                        <p>{format!("Apply to {family_size} family members")}</p>
                                        <Show when=move || family_priv_preview.get().is_some()>
                                            <p>
                                                {move || {
                                                    family_priv_preview
                                                        .get()
                                                        .map(|n| format!("Privilege would change on {n} family members"))
                                                        .unwrap_or_default()
                                                }}
                                            </p>
                                        </Show>
                                        <button class="primary" on:click=move |_| {
                                            family_confirm.set(true);
                                            persist_and_maybe_next.with_value(|f| f(true));
                                        }>"Confirm"</button>
                                        <button on:click=move |_| {
                                            family_confirm.set(false);
                                            family_priv_preview.set(None);
                                        }>"Cancel"</button>
                                    </div>
                                </Show>
                                <div class="review-actions">
                                    <button class="secondary" on:click=move |_| {
                                        match ditto_get() {
                                            None => status.set(Some("Nothing to ditto yet.".into())),
                                            Some(s) => {
                                                pending_resp.set(s.resp);
                                                pending_priv.set(s.privilege);
                                                pending_basis.set(s.basis);
                                                pending_withhold.set(s.withhold);
                                                pending_conf.set(s.confidential);
                                            }
                                        }
                                    }>"Ditto"</button>
                                    <button class="primary" autofocus on:click=move |_| {
                                        persist_and_maybe_next.with_value(|f| f(true));
                                    }>"Save & Next"</button>
                                </div>
                            </aside>
                        </div>
                    }
                })}
            </Show>
            <Show when=move || loading.get() && doc.get().is_none() && error.get().is_none()>
                <p class="empty">"Loading document…"</p>
            </Show>
        </section>
    }
}
