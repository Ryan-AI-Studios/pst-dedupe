//! Three-pane review window (track 0112).

use std::cell::RefCell;

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use wasm_bindgen::JsCast;
use web_sys::{
    HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent,
};

use crate::invoke::{
    tauri_invoke, CodeCatalogEntry, FamilyMemberThin, GeomDto, ItemCodeInfo, ReviewBurnNativeArgs,
    ReviewCodesPreview, ReviewCodesPreviewArgs, ReviewDocument, ReviewDocumentArgs,
    ReviewDocumentBody, ReviewDocumentBodyArgs, ReviewGeomDeleteArgs, ReviewGeomFromHits,
    ReviewGeomFromHitsArgs, ReviewGeomList, ReviewGeomListArgs, ReviewGeomUpsertArgs,
    ReviewRasterPage, ReviewRasterPageArgs, ReviewUpsertNoteArgs, ReviewUpsertPrivilegeArgs,
    ReviewWindowApplyArgs, RootArgs,
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

/// True iff a spawned fetch still matches the live item id and generation.
fn fetch_is_current(want_id: &str, want_gen: u64, got_id: &str, got_gen: u64) -> bool {
    want_id == got_id && want_gen == got_gen
}

/// After a successful persist, keep `saving` locked through the same-item
/// `review_document` refresh so a follow-up save cannot diff against stale
/// `doc.codes`. Navigation starts a new document Effect instead.
fn persist_holds_save_for_refresh(did_navigate: bool) -> bool {
    !did_navigate
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

fn visual_size(crop_w: f64, crop_h: f64, rotate: i32) -> (f64, f64) {
    let r = ((rotate % 360) + 360) % 360;
    if r == 90 || r == 270 {
        (crop_h, crop_w)
    } else {
        (crop_w, crop_h)
    }
}

fn user_to_visual(
    ux: f64,
    uy: f64,
    crop_x: f64,
    crop_y: f64,
    crop_w: f64,
    crop_h: f64,
    rotate: i32,
) -> (f64, f64) {
    let rx = ux - crop_x;
    let ry = uy - crop_y;
    let r = ((rotate % 360) + 360) % 360;
    match r {
        90 => (ry, crop_w - rx),
        180 => (crop_w - rx, crop_h - ry),
        270 => (crop_h - ry, rx),
        _ => (rx, ry),
    }
}

/// Map a CSS-pixel point on the displayed `<img>` to raster pixels.
fn css_point_to_raster(
    css_x: f64,
    css_y: f64,
    display_w: f64,
    display_h: f64,
    raster_w: f64,
    raster_h: f64,
) -> (f64, f64) {
    if display_w <= 0.0 || display_h <= 0.0 {
        return (css_x, css_y);
    }
    (css_x * raster_w / display_w, css_y * raster_h / display_h)
}

/// Viewport client point → CSS inside `.image-frame` (`getBoundingClientRect`).
fn frame_css_point(
    client_x: f64,
    client_y: f64,
    rect_left: f64,
    rect_top: f64,
    rect_w: f64,
    rect_h: f64,
) -> (f64, f64) {
    let max_x = rect_w.max(0.0);
    let max_y = rect_h.max(0.0);
    (
        (client_x - rect_left).clamp(0.0, max_x),
        (client_y - rect_top).clamp(0.0, max_y),
    )
}

fn event_frame_rect(ev: &MouseEvent) -> Option<(f64, f64, f64, f64)> {
    let el = ev.current_target()?.dyn_into::<web_sys::Element>().ok()?;
    let rect = el.get_bounding_client_rect();
    Some((rect.left(), rect.top(), rect.width(), rect.height()))
}

fn event_to_frame_css(ev: &MouseEvent) -> Option<(f64, f64)> {
    let (left, top, width, height) = event_frame_rect(ev)?;
    Some(frame_css_point(
        f64::from(ev.client_x()),
        f64::from(ev.client_y()),
        left,
        top,
        width,
        height,
    ))
}

fn event_to_raster(ev: &MouseEvent, raster_w: f64, raster_h: f64) -> Option<(f64, f64)> {
    let (_left, _top, dw, dh) = event_frame_rect(ev)?;
    let (css_x, css_y) = event_to_frame_css(ev)?;
    Some(css_point_to_raster(css_x, css_y, dw, dh, raster_w, raster_h))
}

fn clear_in_flight_draw(
    drawing: RwSignal<bool>,
    drag_origin: RwSignal<Option<(f64, f64)>>,
    drag_now: RwSignal<Option<(f64, f64)>>,
) {
    drawing.set(false);
    drag_origin.set(None);
    drag_now.set(None);
}

/// Overlay box as **percent of the displayed image** so CSS `max-width` scaling
/// does not desync the hatch from the visible token.
fn geom_to_overlay_pct(g: &GeomDto, raster: &ReviewRasterPage) -> (f64, f64, f64, f64) {
    let rw = raster.width as f64;
    let rh = raster.height as f64;
    if rw <= 0.0 || rh <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let (x, y, w, h) = if raster.kind == "pdf" {
        let (vis_w, vis_h) = visual_size(raster.crop_box.w, raster.crop_box.h, raster.rotate);
        if vis_w <= 0.0 || vis_h <= 0.0 {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let sx = rw / vis_w;
        let sy = rh / vis_h;
        let corners = [
            (g.x, g.y),
            (g.x + g.w, g.y),
            (g.x, g.y + g.h),
            (g.x + g.w, g.y + g.h),
        ];
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (ux, uy) in corners {
            let (vx, vy) = user_to_visual(
                ux,
                uy,
                raster.crop_box.x,
                raster.crop_box.y,
                raster.crop_box.w,
                raster.crop_box.h,
                raster.rotate,
            );
            xs.push(vx * sx);
            ys.push((vis_h - vy) * sy);
        }
        let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let x1 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y1 = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (x0, y0, (x1 - x0).abs(), (y1 - y0).abs())
    } else {
        // JPEG/PNG boxes are native pixel space (y-down). Overlay is % of the displayed (maybe capped) image.
        let nw = f64::from(raster.native_width.max(1));
        let nh = f64::from(raster.native_height.max(1));
        return (
            g.x / nw * 100.0,
            g.y / nh * 100.0,
            g.w / nw * 100.0,
            g.h / nh * 100.0,
        );
    };
    (
        x / rw * 100.0,
        y / rh * 100.0,
        w / rw * 100.0,
        h / rh * 100.0,
    )
}

#[cfg(test)]
mod overlay_scale_tests {
    use super::{css_point_to_raster, frame_css_point};

    #[test]
    fn displayed_scale_maps_midpoint_to_raster() {
        let (x, y) = css_point_to_raster(200.0, 130.0, 400.0, 520.0, 1275.0, 1650.0);
        assert!((x - 637.5).abs() < 0.01);
        assert!((y - 412.5).abs() < 0.01);
    }

    #[test]
    fn frame_css_point_subtracts_rect_origin() {
        let (x, y) = frame_css_point(150.0, 80.0, 100.0, 50.0, 400.0, 300.0);
        assert!((x - 50.0).abs() < 0.01);
        assert!((y - 30.0).abs() < 0.01);
    }

    #[test]
    fn frame_css_point_clamps_to_rect() {
        let (x, y) = frame_css_point(99.0, 400.0, 100.0, 50.0, 200.0, 100.0);
        assert!((x - 0.0).abs() < 0.01);
        assert!((y - 100.0).abs() < 0.01);
        let (x2, y2) = frame_css_point(500.0, 40.0, 100.0, 50.0, 200.0, 100.0);
        assert!((x2 - 200.0).abs() < 0.01);
        assert!((y2 - 0.0).abs() < 0.01);
    }

    #[test]
    fn draw_handlers_use_frame_client_coords_not_offset() {
        let src = include_str!("review_window.rs");
        let offset_x = ["ev.offset", "_x"].concat();
        let offset_y = ["ev.offset", "_y"].concat();
        assert!(!src.contains(&offset_x), "draw must not use MouseEvent.offsetX");
        assert!(!src.contains(&offset_y), "draw must not use MouseEvent.offsetY");
        assert!(src.contains("get_bounding_client_rect"));
        assert!(src.contains("frame_css_point"));
        assert!(src.contains("on:mouseleave"));
        let mouseout = ["on:mouse", "out"].concat();
        assert!(!src.contains(&mouseout));
        assert!(
            src.contains("if pane.get() != \"image\""),
            "pane-leave clear must be its own Effect, not only the raster early-return"
        );
        assert!(src.contains("if pw < 2.0 || ph < 2.0"), "keep 2 raster-px min-drag");
    }
}

#[cfg(test)]
mod fetch_is_current_tests {
    use super::fetch_is_current;

    #[test]
    fn match_is_current() {
        assert!(fetch_is_current("itm_a", 3, "itm_a", 3));
    }

    #[test]
    fn id_mismatch_is_stale() {
        assert!(!fetch_is_current("itm_a", 3, "itm_b", 3));
    }

    #[test]
    fn gen_mismatch_is_stale() {
        assert!(!fetch_is_current("itm_a", 3, "itm_a", 4));
    }
}

#[cfg(test)]
mod persist_holds_save_for_refresh_tests {
    use super::persist_holds_save_for_refresh;

    #[test]
    fn same_item_holds_until_refresh() {
        assert!(persist_holds_save_for_refresh(false));
    }

    #[test]
    fn navigate_unlocks_immediately() {
        assert!(!persist_holds_save_for_refresh(true));
    }
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
    let doc_generation = RwSignal::new(0u64);
    let body_generation = RwSignal::new(0u64);
    let raster_generation = RwSignal::new(0u64);
    let raster_page_index = RwSignal::new(0u32);
    let raster = RwSignal::new(Option::<ReviewRasterPage>::None);
    let raster_error = RwSignal::new(Option::<String>::None);
    let geoms = RwSignal::new(Vec::<GeomDto>::new());
    let drawing = RwSignal::new(false);
    let drag_origin = RwSignal::new(Option::<(f64, f64)>::None);
    let drag_now = RwSignal::new(Option::<(f64, f64)>::None);
    let selected_geom = RwSignal::new(Option::<String>::None);

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
        let gen = doc_generation.get_untracked() + 1;
        doc_generation.set(gen);
        leptos::task::spawn_local(async move {
            match tauri_invoke::<Vec<CodeCatalogEntry>, _>(
                "review_code_catalog",
                &RootArgs { root: root.clone() },
            )
            .await
            {
                Ok(c) => {
                    if fetch_is_current(
                        &id,
                        gen,
                        &doc_id.get_untracked(),
                        doc_generation.get_untracked(),
                    ) {
                        catalog.set(c);
                    }
                }
                Err(e) => {
                    if fetch_is_current(
                        &id,
                        gen,
                        &doc_id.get_untracked(),
                        doc_generation.get_untracked(),
                    ) {
                        error.set(Some(format!("Code catalog: {e}")));
                    }
                }
            }
            match tauri_invoke::<ReviewDocument, _>(
                "review_document",
                &ReviewDocumentArgs {
                    root,
                    item_id: id.clone(),
                    filter_json,
                    keyword,
                },
            )
            .await
            {
                Ok(d) => {
                    if !fetch_is_current(
                        &id,
                        gen,
                        &doc_id.get_untracked(),
                        doc_generation.get_untracked(),
                    ) {
                        return;
                    }
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
                    if fetch_is_current(
                        &id,
                        gen,
                        &doc_id.get_untracked(),
                        doc_generation.get_untracked(),
                    ) {
                        doc.set(None);
                        error.set(Some(e));
                        loading.set(false);
                    }
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
        let gen = body_generation.get_untracked() + 1;
        body_generation.set(gen);
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ReviewDocumentBody, _>(
                "review_document_body",
                &ReviewDocumentBodyArgs {
                    root,
                    item_id: id.clone(),
                    pane: pane_now.clone(),
                },
            )
            .await
            {
                Ok(b) => {
                    if fetch_is_current(
                        &id,
                        gen,
                        &doc_id.get_untracked(),
                        body_generation.get_untracked(),
                    ) && b.pane == pane_now
                    {
                        body.set(Some(b));
                    }
                }
                Err(e) => {
                    if fetch_is_current(
                        &id,
                        gen,
                        &doc_id.get_untracked(),
                        body_generation.get_untracked(),
                    ) {
                        error.set(Some(format!("Body: {e}")));
                    }
                }
            }
        });
    });

    Effect::new(move |_| {
        let id = doc_id.get();
        raster_page_index.set(0);
        raster.set(None);
        geoms.set(Vec::new());
        selected_geom.set(None);
        raster_error.set(None);
        clear_in_flight_draw(drawing, drag_origin, drag_now);
        let _ = id;
    });

    Effect::new(move |_| {
        if pane.get() != "image" {
            clear_in_flight_draw(drawing, drag_origin, drag_now);
        }
    });

    Effect::new(move |_| {
        let root = root_sig.get();
        let id = doc_id.get();
        let pane_now = pane.get();
        let page = raster_page_index.get();
        if root.is_empty() || id.is_empty() || pane_now != "image" {
            return;
        }
        clear_in_flight_draw(drawing, drag_origin, drag_now);
        let gen = raster_generation.get_untracked() + 1;
        raster_generation.set(gen);
        raster.set(None);
        geoms.set(Vec::new());
        selected_geom.set(None);
        raster_error.set(None);
        leptos::task::spawn_local(async move {
            match tauri_invoke::<ReviewRasterPage, _>(
                "review_raster_page",
                &ReviewRasterPageArgs {
                    root: root.clone(),
                    item_id: id.clone(),
                    page_index: Some(page),
                    dpi: Some(150),
                    generation: Some(gen),
                },
            )
            .await
            {
                Ok(r) => {
                    if r.item_id == doc_id.get_untracked()
                        && r.generation == raster_generation.get_untracked()
                    {
                        raster.set(Some(r));
                    }
                }
                Err(e) => {
                    if id == doc_id.get_untracked() && gen == raster_generation.get_untracked() {
                        raster.set(None);
                        raster_error.set(Some(e));
                    }
                }
            }
            match tauri_invoke::<ReviewGeomList, _>(
                "review_geom_list",
                &ReviewGeomListArgs {
                    root,
                    item_id: id,
                    generation: Some(gen),
                },
            )
            .await
            {
                Ok(g) => {
                    if g.item_id == doc_id.get_untracked()
                        && g.generation == raster_generation.get_untracked()
                    {
                        geoms.set(g.boxes);
                    }
                }
                Err(e) => {
                    if gen == raster_generation.get_untracked() {
                        raster_error.set(Some(e));
                    }
                }
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
                family_confirm.set(false);
                family_priv_preview.set(None);
                if failed {
                    saving.set(false);
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
                let did_navigate = then_next && next.is_some();
                if !persist_holds_save_for_refresh(did_navigate) {
                    saving.set(false);
                    if let Some(nid) = next {
                        go_item(nid);
                    }
                    return;
                }
                if then_next {
                    status.set(Some("End of queue".into()));
                } else {
                    status.set(Some("Saved.".into()));
                }
                let gen = doc_generation.get_untracked() + 1;
                doc_generation.set(gen);
                let filter_json = query.with(|q| q.get("filter"));
                let keyword = query.with(|q| q.get("q"));
                match tauri_invoke::<ReviewDocument, _>(
                    "review_document",
                    &ReviewDocumentArgs {
                        root,
                        item_id: item_id.clone(),
                        filter_json,
                        keyword,
                    },
                )
                .await
                {
                    Ok(d) => {
                        if fetch_is_current(
                            &item_id,
                            gen,
                            &doc_id.get_untracked(),
                            doc_generation.get_untracked(),
                        ) {
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
                        }
                    }
                    Err(e) => {
                        if fetch_is_current(
                            &item_id,
                            gen,
                            &doc_id.get_untracked(),
                            doc_generation.get_untracked(),
                        ) {
                            status.set(Some(format!("Saved, but refresh failed: {e}")));
                        }
                    }
                }
                saving.set(false);
            });
        }
    });

    view! {
        <section
            class="review-page"
            tabindex="-1"
            on:keydown=move |ev: KeyboardEvent| {
                if ev.key() == "Escape" {
                    if drawing.get() || drag_origin.get().is_some() {
                        ev.prevent_default();
                        clear_in_flight_draw(drawing, drag_origin, drag_now);
                        return;
                    }
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
                    "," | "PageUp" if pane.get() == "image" => {
                        ev.prevent_default();
                        raster_page_index.update(|p| {
                            if *p > 0 {
                                *p -= 1;
                            }
                        });
                    }
                    "." | "PageDown" if pane.get() == "image" => {
                        ev.prevent_default();
                        if let Some(r) = raster.get() {
                            raster_page_index.update(|p| {
                                if *p + 1 < r.page_count {
                                    *p += 1;
                                }
                            });
                        }
                    }
                    "Delete" | "Backspace" if pane.get() == "image" => {
                        if let Some(gid) = selected_geom.get() {
                            ev.prevent_default();
                            let root = root_sig.get();
                            leptos::task::spawn_local(async move {
                                if let Err(e) = tauri_invoke::<(), _>(
                                    "review_geom_delete",
                                    &ReviewGeomDeleteArgs {
                                        root: root.clone(),
                                        geom_id: gid,
                                    },
                                )
                                .await
                                {
                                    error.set(Some(e));
                                    return;
                                }
                                if let Ok(g) = tauri_invoke::<ReviewGeomList, _>(
                                    "review_geom_list",
                                    &ReviewGeomListArgs {
                                        root,
                                        item_id: doc_id.get_untracked(),
                                        generation: Some(raster_generation.get_untracked()),
                                    },
                                )
                                .await
                                {
                                    if g.item_id == doc_id.get_untracked()
                                        && g.generation == raster_generation.get_untracked()
                                    {
                                        geoms.set(g.boxes);
                                    }
                                }
                            });
                            selected_geom.set(None);
                        }
                    }
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
                        <li>"r — Image tab"</li>
                        <li>", . or PageUp/PageDown — previous / next page (Image)"</li>
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
                    let bates_disp = if headers.bates_note.is_empty() {
                        headers.bates.clone()
                    } else {
                        format!("{} ({})", headers.bates, headers.bates_note)
                    };
                    let produced_copy = if headers.bates == "—" || headers.bates.is_empty() {
                        "Not produced.".to_string()
                    } else {
                        headers.bates.clone()
                    };
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
                                    <dt>"Bates"</dt><dd>{bates_disp}</dd>
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
                                    {move || {
                                        let err = raster_error.get();
                                        if let Some(e) = err {
                                            let copy = if e.contains("pdf_encrypted") || e.starts_with("pdf_encrypted") {
                                                "pdf_encrypted".to_string()
                                            } else if e.contains("unsupported_kind")
                                                || e.contains("Not a page image")
                                                || e.contains("native-only")
                                                || e.contains("no print-to-TIFF")
                                            {
                                                "Native-only (no print-to-TIFF)".to_string()
                                            } else {
                                                e
                                            };
                                            return view! { <p class="empty" id="document" tabindex="-1">{copy}</p> }.into_any();
                                        }
                                        let Some(r) = raster.get() else {
                                            return view! { <p class="empty" id="document" tabindex="-1">"Loading page…"</p> }.into_any();
                                        };
                                        let src = format!("data:image/png;base64,{}", r.png_base64);
                                        let page_label = format!("Page {} / {}", r.page_index + 1, r.page_count);
                                        let truncated = r.truncated;
                                        let boxes = geoms.get();
                                        let rw = r.width as f64;
                                        let rh = r.height as f64;
                                        let overlays: Vec<_> = boxes
                                            .iter()
                                            .filter(|g| g.page_index as u32 == r.page_index && g.status == "active")
                                            .cloned()
                                            .collect();
                                        let drag = drag_origin.get().zip(drag_now.get());
                                        let r_for_up = r.clone();
                                        view! {
                                            <div class="image-stage" id="document" tabindex="-1">
                                                <Show when=move || truncated>
                                                    <p class="empty" role="status">"Preview capped at 4096 px long side."</p>
                                                </Show>
                                                <div class="image-toolbar">
                                                    <button on:click=move |_| {
                                                        raster_page_index.update(|p| if *p > 0 { *p -= 1; });
                                                    }>"Prev"</button>
                                                    <span>{page_label}</span>
                                                    <button on:click=move |_| {
                                                        raster_page_index.update(|p| {
                                                            if let Some(rr) = raster.get() {
                                                                if *p + 1 < rr.page_count { *p += 1; }
                                                            }
                                                        });
                                                    }>"Next"</button>
                                                    <button on:click=move |_| {
                                                        if let Some(rr) = raster.get() {
                                                            let root = root_sig.get();
                                                            let id = doc_id.get();
                                                            let gen = raster_generation.get();
                                                            leptos::task::spawn_local(async move {
                                                                if let Err(e) = tauri_invoke::<serde_json::Value, _>(
                                                                    "review_geom_upsert",
                                                                    &ReviewGeomUpsertArgs {
                                                                        root: root.clone(),
                                                                        item_id: id.clone(),
                                                                        page_index: rr.page_index,
                                                                        px: 0.0,
                                                                        py: 0.0,
                                                                        pw: rr.width as f64,
                                                                        ph: rr.height as f64,
                                                                        raster_width: rr.width as f64,
                                                                        raster_height: rr.height as f64,
                                                                        reason: Some("privilege".into()),
                                                                        label: None,
                                                                        source: Some("full_page".into()),
                                                                        generation: Some(gen),
                                                                    },
                                                                ).await {
                                                                    error.set(Some(e));
                                                                    return;
                                                                }
                                                                if let Ok(g) = tauri_invoke::<ReviewGeomList, _>(
                                                                    "review_geom_list",
                                                                    &ReviewGeomListArgs {
                                                                        root,
                                                                        item_id: id,
                                                                        generation: Some(gen),
                                                                    },
                                                                ).await {
                                                                    if g.item_id == doc_id.get_untracked()
                                                                        && g.generation == raster_generation.get_untracked()
                                                                    {
                                                                        geoms.set(g.boxes);
                                                                    }
                                                                }
                                                            });
                                                        }
                                                    }>"Full page"</button>
                                                    <button on:click=move |_| {
                                                        let root = root_sig.get();
                                                        let id = doc_id.get();
                                                        leptos::task::spawn_local(async move {
                                                            match tauri_invoke::<serde_json::Value, _>(
                                                                "review_burn_native",
                                                                &ReviewBurnNativeArgs {
                                                                    root,
                                                                    item_id: id,
                                                                },
                                                            )
                                                            .await
                                                            {
                                                                Ok(_) => status.set(Some(
                                                                    "Burned native written.".into(),
                                                                )),
                                                                Err(e) => error.set(Some(e)),
                                                            }
                                                        });
                                                    }>"Burn"</button>
                                                    <button on:click=move |_| {
                                                        let root = root_sig.get();
                                                        let id = doc_id.get();
                                                        let q = find_q.get();
                                                        let gen = raster_generation.get();
                                                        leptos::task::spawn_local(async move {
                                                            match tauri_invoke::<ReviewGeomFromHits, _>(
                                                                "review_geom_from_hits",
                                                                &ReviewGeomFromHitsArgs {
                                                                    root: root.clone(),
                                                                    item_id: id.clone(),
                                                                    query: if q.trim().is_empty() { None } else { Some(q) },
                                                                    reason: Some("privilege".into()),
                                                                    generation: Some(gen),
                                                                },
                                                            ).await {
                                                                Ok(h) => {
                                                                    if h.hit_count == 0 || h.unmapped {
                                                                        status.set(Some("No hits mapped; draw boxes or withhold.".into()));
                                                                    }
                                                                    if let Ok(g) = tauri_invoke::<ReviewGeomList, _>(
                                                                        "review_geom_list",
                                                                        &ReviewGeomListArgs { root, item_id: id, generation: Some(gen) },
                                                                    ).await {
                                                                        if g.item_id == doc_id.get_untracked()
                                                                            && g.generation == raster_generation.get_untracked()
                                                                        {
                                                                            geoms.set(g.boxes);
                                                                        }
                                                                    }
                                                                }
                                                                Err(e) => error.set(Some(e)),
                                                            }
                                                        });
                                                    }>"From hits"</button>
                                                </div>
                                                <div class="image-frame"
                                                    on:mousedown=move |ev: MouseEvent| {
                                                        let Some(pt) = event_to_frame_css(&ev) else { return; };
                                                        drawing.set(true);
                                                        drag_origin.set(Some(pt));
                                                        drag_now.set(Some(pt));
                                                    }
                                                    on:mousemove=move |ev: MouseEvent| {
                                                        if drawing.get() {
                                                            if let Some(pt) = event_to_frame_css(&ev) {
                                                                drag_now.set(Some(pt));
                                                            }
                                                        }
                                                    }
                                                    on:mouseleave=move |_| {
                                                        clear_in_flight_draw(drawing, drag_origin, drag_now);
                                                    }
                                                    on:mouseup=move |ev: MouseEvent| {
                                                        if !drawing.get() { return; }
                                                        drawing.set(false);
                                                        let Some((x0, y0)) = drag_origin.get() else { return; };
                                                        let Some((x1, y1)) = event_to_frame_css(&ev) else {
                                                            drag_origin.set(None);
                                                            drag_now.set(None);
                                                            return;
                                                        };
                                                        drag_origin.set(None);
                                                        drag_now.set(None);
                                                        let (dw, dh) = event_frame_rect(&ev)
                                                            .map(|(_l, _t, w, h)| (w, h))
                                                            .unwrap_or((rw, rh));
                                                        let (ox, oy) = css_point_to_raster(x0, y0, dw, dh, rw, rh);
                                                        let (cx, cy) = event_to_raster(&ev, rw, rh)
                                                            .unwrap_or_else(|| {
                                                                css_point_to_raster(x1, y1, dw, dh, rw, rh)
                                                            });
                                                        let px = ox.min(cx);
                                                        let py = oy.min(cy);
                                                        let pw = (cx - ox).abs();
                                                        let ph = (cy - oy).abs();
                                                        if pw < 2.0 || ph < 2.0 { return; }
                                                        let rr = r_for_up.clone();
                                                        let root = root_sig.get();
                                                        let id = doc_id.get();
                                                        let gen = raster_generation.get();
                                                        leptos::task::spawn_local(async move {
                                                            if let Err(e) = tauri_invoke::<serde_json::Value, _>(
                                                                "review_geom_upsert",
                                                                &ReviewGeomUpsertArgs {
                                                                    root: root.clone(),
                                                                    item_id: id.clone(),
                                                                    page_index: rr.page_index,
                                                                    px,
                                                                    py,
                                                                    pw,
                                                                    ph,
                                                                    raster_width: rw,
                                                                    raster_height: rh,
                                                                    reason: Some("privilege".into()),
                                                                    label: None,
                                                                    source: Some("draw".into()),
                                                                    generation: Some(gen),
                                                                },
                                                            ).await {
                                                                error.set(Some(e));
                                                                return;
                                                            }
                                                            if let Ok(g) = tauri_invoke::<ReviewGeomList, _>(
                                                                "review_geom_list",
                                                                &ReviewGeomListArgs { root, item_id: id, generation: Some(gen) },
                                                            ).await {
                                                                if g.item_id == doc_id.get_untracked()
                                                                    && g.generation == raster_generation.get_untracked()
                                                                {
                                                                    geoms.set(g.boxes);
                                                                }
                                                            }
                                                        });
                                                    }
                                                >
                                                    <img src=src alt="Page raster" class="page-raster" />
                                                    {overlays.into_iter().map(|g| {
                                                        let (x, y, w, h) = geom_to_overlay_pct(&g, &r);
                                                        let gid = g.id.clone();
                                                        let selected = selected_geom.get().as_deref() == Some(gid.as_str());
                                                        let style = format!(
                                                            "left:{x}%;top:{y}%;width:{w}%;height:{h}%;"
                                                        );
                                                        view! {
                                                            <div
                                                                class=if selected { "geom-overlay selected" } else { "geom-overlay" }
                                                                style=style
                                                                on:mousedown=move |ev: MouseEvent| {
                                                                    ev.stop_propagation();
                                                                    selected_geom.set(Some(gid.clone()));
                                                                }
                                                            ></div>
                                                        }
                                                    }).collect_view()}
                                                    {drag.map(|((x0,y0),(x1,y1))| {
                                                        let x = x0.min(x1);
                                                        let y = y0.min(y1);
                                                        let w = (x1-x0).abs();
                                                        let h = (y1-y0).abs();
                                                        let style = format!("left:{x}px;top:{y}px;width:{w}px;height:{h}px;");
                                                        view! { <div class="geom-overlay draft" style=style></div> }
                                                    })}
                                                </div>
                                                <ul class="geom-list">
                                                    {boxes.into_iter().map(|g| {
                                                        let gid = g.id.clone();
                                                        let gid_del = g.id.clone();
                                                        let label = format!("{} p{} {:.0},{:.0}", g.reason, g.page_index + 1, g.x, g.y);
                                                        view! {
                                                            <li>
                                                                <button on:click=move |_| selected_geom.set(Some(gid.clone()))>{label}</button>
                                                                <button on:click=move |_| {
                                                                    let root = root_sig.get();
                                                                    let id = gid_del.clone();
                                                                    leptos::task::spawn_local(async move {
                                                                        if let Err(e) = tauri_invoke::<(), _>(
                                                                            "review_geom_delete",
                                                                            &ReviewGeomDeleteArgs { root: root.clone(), geom_id: id },
                                                                        ).await {
                                                                            error.set(Some(e));
                                                                            return;
                                                                        }
                                                                        if let Ok(list) = tauri_invoke::<ReviewGeomList, _>(
                                                                            "review_geom_list",
                                                                            &ReviewGeomListArgs {
                                                                                root,
                                                                                item_id: doc_id.get_untracked(),
                                                                                generation: Some(raster_generation.get_untracked()),
                                                                            },
                                                                        ).await {
                                                                            if list.item_id == doc_id.get_untracked()
                                                                                && list.generation
                                                                                    == raster_generation.get_untracked()
                                                                            {
                                                                                geoms.set(list.boxes);
                                                                            }
                                                                        }
                                                                    });
                                                                }>"Delete"</button>
                                                            </li>
                                                        }
                                                    }).collect_view()}
                                                </ul>
                                            </div>
                                        }.into_any()
                                    }}
                                </Show>
                                <Show when=move || pane.get() == "produced">
                                    <p class="empty" id="document" tabindex="-1">{produced_copy.clone()}</p>
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
                                    <button class="primary" autofocus
                                        prop:disabled=move || saving.get()
                                        on:click=move |_| {
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
