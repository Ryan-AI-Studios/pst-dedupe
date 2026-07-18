//! Review screen: linear corpus list + body viewer + family strip + coding (0026/0027).
//!
//! # List virtualization
//!
//! Rows use a **fixed** [`ROW_HEIGHT`] so `ScrollArea::show_rows` can skip
//! non-visible items. Subject/from/date are single-line truncated — never wrap
//! list rows (variable height breaks virtualization).
//!
//! # Load policy
//!
//! Thin rows only (`list_review_thin`). If `count_in_review ≤` [`THIN_LOAD_ALL_THRESHOLD`],
//! load the full thin list; otherwise page in chunks of [`THIN_PAGE_SIZE`].
//! Never load full corpus bodies into the list.
//!
//! # Coding (0027)
//!
//! Codes for **visible rows** only (`list_item_codes`). Multi-select + batch
//! Add/Remove with optional whole-family propagate. Digits 1–9 toggle the first
//! nine active codes on the current item when the focus gate is clear.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense};
use matter_core::{
    ApplyCodesInput, ApplyCodesResult, CodeDef, ItemCodeInfo, Matter, ReviewListRow,
};

use crate::review_body::{BodyLoader, BodyPane};
use crate::review_nav;

/// Fixed list row height (sans item spacing) for `ScrollArea::show_rows`.
pub const ROW_HEIGHT: f32 = 22.0;

/// Load all thin rows when corpus is at or under this size.
pub const THIN_LOAD_ALL_THRESHOLD: u64 = 50_000;

/// Page size when corpus exceeds [`THIN_LOAD_ALL_THRESHOLD`].
pub const THIN_PAGE_SIZE: u64 = 500;

/// Prefer off-UI-thread apply when expanded target count exceeds this.
pub const CODING_OFF_THREAD_THRESHOLD: usize = 50;

/// Selection-time detail for header parties (not loaded in thin list).
#[derive(Debug, Clone, Default)]
pub struct SelectionDetail {
    pub item_id: String,
    pub to_display: Option<String>,
    pub cc_display: Option<String>,
}

/// Pending batch confirm dialog.
#[derive(Debug, Clone)]
struct BatchConfirm {
    add: bool,
    code_ids: Vec<String>,
    code_labels: Vec<String>,
    selected_ids: Vec<String>,
    selected_count: usize,
    /// Estimated targets after optional family expand (best-effort pre-expand).
    target_count: usize,
    propagate_family: bool,
}

/// Review screen state held by the desk app.
#[derive(Default)]
pub struct ReviewState {
    /// Thin rows currently in RAM (ordered by `review_order`).
    pub rows: Vec<ReviewListRow>,
    /// Total corpus count (may exceed `rows.len()` when paged).
    pub count: u64,
    /// Selected index into `rows` (0-based).
    pub selection: Option<usize>,
    /// Matter root these rows were loaded from.
    pub loaded_root: Option<Utf8PathBuf>,
    /// Last list load error.
    pub list_error: Option<String>,
    /// Body loader (generation-gated).
    pub body: BodyLoader,
    /// Whether list needs a reload (enter screen / matter change / Refresh).
    needs_reload: bool,
    /// Last selected item id (for restore after reload).
    last_item_id: Option<String>,
    /// Cached To/Cc for the current selection (fetched via `get_item`).
    selection_detail: Option<SelectionDetail>,
    /// Multi-selected item ids (checkbox column).
    pub multi_selected: HashSet<String>,
    /// Code catalog for the open matter.
    code_defs: Vec<CodeDef>,
    /// Codes for loaded/visible item ids.
    row_codes: HashMap<String, Vec<ItemCodeInfo>>,
    /// Batch panel mode: true = Add, false = Remove.
    batch_mode_add: bool,
    /// Codes checked in the batch panel (definition ids).
    batch_code_ids: HashSet<String>,
    /// Whole-family propagate checkbox (default false).
    propagate_family: bool,
    /// Open batch confirm dialog.
    batch_confirm: Option<BatchConfirm>,
    /// Async coding apply in flight.
    coding_busy: bool,
    coding_rx: Option<Receiver<Result<ApplyCodesResult, String>>>,
    coding_status: Option<String>,
    coding_error: Option<String>,
}

impl ReviewState {
    /// Mark for reload and clear rows (e.g. matter switched).
    pub fn clear_for_matter_change(&mut self) {
        self.rows.clear();
        self.count = 0;
        self.selection = None;
        self.loaded_root = None;
        self.list_error = None;
        self.body.clear();
        self.needs_reload = true;
        self.last_item_id = None;
        self.selection_detail = None;
        self.multi_selected.clear();
        self.code_defs.clear();
        self.row_codes.clear();
        self.batch_code_ids.clear();
        self.batch_confirm = None;
        self.coding_busy = false;
        self.coding_rx = None;
        self.coding_status = None;
        self.coding_error = None;
        self.batch_mode_add = true;
        self.propagate_family = false;
    }

    /// Request a thin-list reload on next show.
    pub fn request_reload(&mut self) {
        self.needs_reload = true;
    }

    /// Toggle multi-select membership for one item id.
    pub fn toggle_multi_select(&mut self, item_id: &str) {
        toggle_selection_set(&mut self.multi_selected, item_id);
    }

    /// Clear multi-select.
    pub fn clear_multi_select(&mut self) {
        self.multi_selected.clear();
    }

    fn ensure_loaded(&mut self, matter_root: &Utf8Path) {
        let root_changed = self
            .loaded_root
            .as_ref()
            .map(|r| r.as_path() != matter_root)
            .unwrap_or(true);
        if !self.needs_reload && !root_changed {
            return;
        }
        if root_changed {
            self.last_item_id = None;
            self.body.clear();
        }
        self.reload_list(matter_root);
    }

    fn reload_list(&mut self, matter_root: &Utf8Path) {
        self.needs_reload = false;
        self.list_error = None;
        // Always drop in-flight / stale body + parties: selection may change after
        // re-promote (item demoted/removed). Leaving Ready/Loading with an old
        // item_id would show permanent "Loading…" because spawn only runs on Idle.
        self.body.clear();
        self.selection_detail = None;
        match load_review_thin(matter_root) {
            Ok((count, rows)) => {
                self.count = count;
                self.rows = rows;
                self.loaded_root = Some(matter_root.to_owned());
                // Drop multi-select ids that are no longer present.
                let present: HashSet<String> = self.rows.iter().map(|r| r.id.clone()).collect();
                self.multi_selected.retain(|id| present.contains(id));
                // Restore selection by id if possible.
                let sel = if let Some(ref id) = self.last_item_id {
                    self.rows.iter().position(|r| &r.id == id)
                } else {
                    None
                };
                self.selection = sel.or(if self.rows.is_empty() { None } else { Some(0) });
                if let Some(i) = self.selection {
                    if let Some(row) = self.rows.get(i) {
                        self.last_item_id = Some(row.id.clone());
                    }
                    self.load_selection_detail(matter_root);
                }
                self.reload_coding_catalog(matter_root);
                self.refresh_row_codes(matter_root);
            }
            Err(e) => {
                self.list_error = Some(e);
                self.rows.clear();
                self.count = 0;
                self.selection = None;
                self.loaded_root = Some(matter_root.to_owned());
                self.code_defs.clear();
                self.row_codes.clear();
            }
        }
    }

    fn reload_coding_catalog(&mut self, matter_root: &Utf8Path) {
        match load_code_definitions(matter_root) {
            Ok(defs) => self.code_defs = defs,
            Err(e) => {
                self.coding_error = Some(format!("Code catalog: {e}"));
                self.code_defs.clear();
            }
        }
    }

    fn refresh_row_codes(&mut self, matter_root: &Utf8Path) {
        let ids: Vec<String> = self.rows.iter().map(|r| r.id.clone()).collect();
        match load_item_codes(matter_root, &ids) {
            Ok(map) => self.row_codes = map,
            Err(e) => {
                self.coding_error = Some(format!("Load codes: {e}"));
            }
        }
    }

    fn current_item_id(&self) -> Option<&str> {
        self.selection
            .and_then(|i| self.rows.get(i))
            .map(|r| r.id.as_str())
    }

    fn codes_for(&self, item_id: &str) -> &[ItemCodeInfo] {
        self.row_codes
            .get(item_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn active_defs(&self) -> Vec<&CodeDef> {
        self.code_defs.iter().filter(|d| d.is_active != 0).collect()
    }

    fn poll_coding(&mut self, matter_root: &Utf8Path, ctx: &egui::Context) {
        let Some(rx) = self.coding_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.coding_busy = false;
                self.coding_rx = None;
                self.coding_status = Some(format!("Coded {} item(s).", result.target_count));
                self.coding_error = None;
                self.refresh_row_codes(matter_root);
                ctx.request_repaint();
            }
            Ok(Err(e)) => {
                self.coding_busy = false;
                self.coding_rx = None;
                self.coding_error = Some(e);
                self.coding_status = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(TryRecvError::Disconnected) => {
                self.coding_busy = false;
                self.coding_rx = None;
                self.coding_error = Some("Coding worker ended unexpectedly.".into());
            }
        }
    }

    fn apply_codes_now(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        input: ApplyCodesInput,
    ) {
        if self.coding_busy {
            return;
        }
        let n = if input.propagate_family {
            // Unknown expand size — off-thread when selected is large.
            input.item_ids.len().max(CODING_OFF_THREAD_THRESHOLD)
        } else {
            input.item_ids.len()
        };
        if n > CODING_OFF_THREAD_THRESHOLD {
            self.spawn_apply_codes(matter_root, ctx, input);
        } else {
            match apply_codes_blocking(matter_root, input) {
                Ok(result) => {
                    self.coding_status = Some(format!("Coded {} item(s).", result.target_count));
                    self.coding_error = None;
                    self.refresh_row_codes(matter_root);
                }
                Err(e) => {
                    self.coding_error = Some(e);
                    self.coding_status = None;
                }
            }
        }
    }

    fn spawn_apply_codes(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        input: ApplyCodesInput,
    ) {
        if self.coding_busy {
            return;
        }
        let root = matter_root.to_owned();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.coding_rx = Some(rx);
        self.coding_busy = true;
        self.coding_status = Some("Applying codes…".into());
        self.coding_error = None;
        let _ = thread::Builder::new()
            .name("desk-coding-apply".into())
            .spawn(move || {
                let result = apply_codes_blocking(&root, input);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
    }

    /// Toggle a code on the current item (no confirm).
    fn toggle_current_code(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        code_id: &str,
        actor: &str,
    ) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        let has = self
            .codes_for(&item_id)
            .iter()
            .any(|c| c.code_id == code_id);
        let input = if has {
            ApplyCodesInput {
                item_ids: vec![item_id],
                add_code_ids: vec![],
                remove_code_ids: vec![code_id.to_string()],
                propagate_family: false,
                actor: actor.to_string(),
            }
        } else {
            ApplyCodesInput {
                item_ids: vec![item_id],
                add_code_ids: vec![code_id.to_string()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: actor.to_string(),
            }
        };
        self.apply_codes_now(matter_root, ctx, input);
    }

    fn remove_current_code(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        code_id: &str,
        actor: &str,
    ) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        self.apply_codes_now(
            matter_root,
            ctx,
            ApplyCodesInput {
                item_ids: vec![item_id],
                add_code_ids: vec![],
                remove_code_ids: vec![code_id.to_string()],
                propagate_family: false,
                actor: actor.to_string(),
            },
        );
    }

    fn select_index(&mut self, idx: usize, ctx: &egui::Context, matter_root: &Utf8Path) {
        if idx >= self.rows.len() {
            return;
        }
        if self.selection == Some(idx) {
            // Still ensure body is loading/loaded for this selection.
            if matches!(self.body.pane(), BodyPane::Idle) {
                self.spawn_body_for_selection(ctx, matter_root);
            }
            if self.selection_detail.is_none() {
                self.load_selection_detail(matter_root);
            }
            return;
        }
        self.selection = Some(idx);
        if let Some(row) = self.rows.get(idx) {
            self.last_item_id = Some(row.id.clone());
        }
        self.selection_detail = None;
        self.load_selection_detail(matter_root);
        self.spawn_body_for_selection(ctx, matter_root);
    }

    /// Fetch To/Cc for the selected item (thin list omits participant JSON).
    fn load_selection_detail(&mut self, matter_root: &Utf8Path) {
        let Some(i) = self.selection else {
            self.selection_detail = None;
            return;
        };
        let Some(row) = self.rows.get(i) else {
            self.selection_detail = None;
            return;
        };
        let item_id = row.id.clone();
        match load_party_detail(matter_root, &item_id) {
            Ok(detail) => self.selection_detail = Some(detail),
            Err(_) => {
                // Non-fatal: header still shows From from thin row.
                self.selection_detail = Some(SelectionDetail {
                    item_id,
                    to_display: None,
                    cc_display: None,
                });
            }
        }
    }

    fn spawn_body_for_selection(&mut self, ctx: &egui::Context, matter_root: &Utf8Path) {
        let Some(i) = self.selection else {
            self.body.clear();
            return;
        };
        let Some(row) = self.rows.get(i) else {
            self.body.clear();
            return;
        };
        self.body.spawn_load(
            ctx,
            matter_root,
            row.id.clone(),
            row.text_sha256.clone(),
            row.html_sha256.clone(),
        );
    }

    fn go_next(&mut self, ctx: &egui::Context, matter_root: &Utf8Path) {
        let n = self.rows.len();
        let Some(i) = self.selection else {
            if n > 0 {
                self.select_index(0, ctx, matter_root);
            }
            return;
        };
        if let Some(ni) = review_nav::next_index(i, n) {
            self.select_index(ni, ctx, matter_root);
        }
    }

    fn go_prev(&mut self, ctx: &egui::Context, matter_root: &Utf8Path) {
        let n = self.rows.len();
        let Some(i) = self.selection else {
            return;
        };
        if let Some(pi) = review_nav::prev_index(i, n) {
            self.select_index(pi, ctx, matter_root);
        }
    }
}

/// Parse `to_addrs_json` / `cc_addrs_json` (JSON string array) into a truncated display line.
pub fn format_addrs_json(raw: Option<&str>, max_chars: usize) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    let list: Vec<String> = serde_json::from_str(s).ok()?;
    if list.is_empty() {
        return None;
    }
    let joined = list.join("; ");
    if joined.chars().count() > max_chars {
        let truncated: String = joined.chars().take(max_chars.saturating_sub(1)).collect();
        Some(format!("{truncated}…"))
    } else {
        Some(joined)
    }
}

/// Load To/Cc display strings for one item via [`Matter::open_for_read`].
pub fn load_party_detail(matter_root: &Utf8Path, item_id: &str) -> Result<SelectionDetail, String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let item = matter.get_item(item_id).map_err(|e| e.to_string())?;
    Ok(SelectionDetail {
        item_id: item.id,
        to_display: format_addrs_json(item.to_addrs_json.as_deref(), 160),
        cc_display: format_addrs_json(item.cc_addrs_json.as_deref(), 120),
    })
}

/// Load count + thin rows via [`Matter::open_for_read`] (WAL-safe).
pub fn load_review_thin(matter_root: &Utf8Path) -> Result<(u64, Vec<ReviewListRow>), String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let count = matter.count_in_review(None).map_err(|e| e.to_string())?;
    let rows = if count == 0 {
        Vec::new()
    } else if count <= THIN_LOAD_ALL_THRESHOLD {
        matter
            .list_review_thin(None, count, 0)
            .map_err(|e| e.to_string())?
    } else {
        // Large corpus: load first page only (P0). Operator can re-promote / filter later.
        matter
            .list_review_thin(None, THIN_PAGE_SIZE, 0)
            .map_err(|e| e.to_string())?
    };
    Ok((count, rows))
}

/// Load code catalog (seeds if empty via writer open path when needed).
pub fn load_code_definitions(matter_root: &Utf8Path) -> Result<Vec<CodeDef>, String> {
    // Prefer read path; if empty (legacy matter never seeded), seed via open.
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let mut defs = matter.list_code_definitions().map_err(|e| e.to_string())?;
    drop(matter);
    if defs.is_empty() {
        let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
        matter.seed_default_codes().map_err(|e| e.to_string())?;
        defs = matter.list_code_definitions().map_err(|e| e.to_string())?;
    }
    Ok(defs)
}

/// Batch-load codes for the given item ids (visible rows).
pub fn load_item_codes(
    matter_root: &Utf8Path,
    item_ids: &[String],
) -> Result<HashMap<String, Vec<ItemCodeInfo>>, String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    matter.list_item_codes(item_ids).map_err(|e| e.to_string())
}

/// Apply codes on a worker / sync path.
pub fn apply_codes_blocking(
    matter_root: &Utf8Path,
    input: ApplyCodesInput,
) -> Result<ApplyCodesResult, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter.seed_default_codes().map_err(|e| e.to_string())?;
    matter.apply_codes(input).map_err(|e| e.to_string())
}

/// Toggle membership of `item_id` in a multi-select set (pure helper for tests).
pub fn toggle_selection_set(selected: &mut HashSet<String>, item_id: &str) {
    if !selected.remove(item_id) {
        selected.insert(item_id.to_string());
    }
}

/// Select a contiguous index range into multi-select by item id (pure helper).
pub fn select_range_into(
    selected: &mut HashSet<String>,
    rows: &[ReviewListRow],
    from: usize,
    to: usize,
) {
    if rows.is_empty() {
        return;
    }
    let lo = from.min(to).min(rows.len().saturating_sub(1));
    let hi = from.max(to).min(rows.len().saturating_sub(1));
    for r in &rows[lo..=hi] {
        selected.insert(r.id.clone());
    }
}

/// Paint the Review screen.
pub fn show(ui: &mut egui::Ui, state: &mut ReviewState, matter_root: &Utf8Path, actor: &str) {
    let ctx = ui.ctx().clone();

    state.ensure_loaded(matter_root);
    state.body.try_take();
    state.poll_coding(matter_root, &ctx);

    // Kick body load when we have a selection but body is idle (first paint after reload).
    if state.selection.is_some() && matches!(state.body.pane(), BodyPane::Idle) {
        state.spawn_body_for_selection(&ctx, matter_root);
    }

    ui.horizontal(|ui| {
        ui.heading("Review");
        ui.label("— Review Corpus (in_review)");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh list").clicked() {
                // Preserve last_item_id across reload.
                state.needs_reload = true;
                state.ensure_loaded(matter_root);
                if state.selection.is_some() {
                    state.spawn_body_for_selection(&ctx, matter_root);
                }
            }
        });
    });
    ui.add_space(4.0);

    if let Some(err) = state.list_error.clone() {
        ui.colored_label(Color32::from_rgb(200, 60, 60), format!("List error: {err}"));
        return;
    }

    if state.rows.is_empty() {
        ui.add_space(12.0);
        ui.label(RichText::new("No items in review. Run Promote to review on Workspace.").strong());
        ui.label("Promote builds the Review Corpus (`in_review` + `review_order`).");
        return;
    }

    // Keyboard: only when no widget has focus (egui 0.34: focused()).
    let no_focus = ctx.memory(|m| m.focused().is_none());
    if review_nav::focus_allows_shortcuts(no_focus) {
        let (want_next, want_prev, want_enter, digit) = ui.input(|i| {
            let next =
                i.key_pressed(Key::CloseBracket) || (i.modifiers.alt && i.key_pressed(Key::N));
            let prev =
                i.key_pressed(Key::OpenBracket) || (i.modifiers.alt && i.key_pressed(Key::P));
            let enter = i.key_pressed(Key::Enter);
            let digit = digit_key_index(i);
            (next, prev, enter, digit)
        });
        if want_next {
            state.go_next(&ctx, matter_root);
        } else if want_prev {
            state.go_prev(&ctx, matter_root);
        } else if want_enter {
            if let Some(i) = state.selection {
                state.select_index(i, &ctx, matter_root);
            }
        } else if let Some(di) = digit {
            let active: Vec<CodeDef> = state.active_defs().into_iter().take(9).cloned().collect();
            if let Some(def) = active.get(di) {
                let id = def.id.clone();
                state.toggle_current_code(matter_root, &ctx, &id, actor);
            }
        }
        if want_next {
            ui.input_mut(|i| {
                let _ = i.consume_key(Modifiers::NONE, Key::CloseBracket);
                let _ = i.consume_key(Modifiers::ALT, Key::N);
            });
        }
        if want_prev {
            ui.input_mut(|i| {
                let _ = i.consume_key(Modifiers::NONE, Key::OpenBracket);
                let _ = i.consume_key(Modifiers::ALT, Key::P);
            });
        }
        if want_enter {
            ui.input_mut(|i| {
                let _ = i.consume_key(Modifiers::NONE, Key::Enter);
            });
        }
    }

    // Status bar
    let n_shown = state.rows.len();
    let n_total = state.count as usize;
    let n_multi = state.multi_selected.len();
    ui.horizontal(|ui| {
        ui.label(review_nav::position_label(state.selection, n_shown));
        if n_total > n_shown {
            ui.label(format!("(showing {n_shown} of {n_total} in corpus)"));
        }
        if n_multi > 0 {
            ui.separator();
            ui.label(format!("{n_multi} selected"));
            if ui.small_button("Clear selection").clicked() {
                state.clear_multi_select();
            }
        }
        ui.separator();
        let can_prev = state
            .selection
            .and_then(|i| review_nav::prev_index(i, n_shown))
            .is_some();
        let can_next = state
            .selection
            .and_then(|i| review_nav::next_index(i, n_shown))
            .is_some();
        if ui
            .add_enabled(can_prev, egui::Button::new("Prev"))
            .on_hover_text("[ or Alt+P")
            .clicked()
        {
            state.go_prev(&ctx, matter_root);
        }
        if ui
            .add_enabled(can_next, egui::Button::new("Next"))
            .on_hover_text("] or Alt+N")
            .clicked()
        {
            state.go_next(&ctx, matter_root);
        }
        ui.label(
            RichText::new("  [ / ]  Alt+P / Alt+N  ·  1–9 toggle codes")
                .weak()
                .small(),
        );
    });
    if let Some(status) = state.coding_status.clone() {
        ui.label(
            RichText::new(status)
                .small()
                .color(Color32::from_rgb(40, 120, 60)),
        );
    }
    if let Some(err) = state.coding_error.clone() {
        ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Coding: {err}"));
    }
    if state.coding_busy {
        ui.label(RichText::new("Applying codes…").italics().small());
    }
    ui.add_space(4.0);

    // Batch confirm modal
    show_batch_confirm(ui, state, matter_root, &ctx, actor);

    // Main split: list | viewer
    let available = ui.available_size();
    let list_width = (available.x * 0.34).clamp(220.0, 480.0);

    ui.horizontal(|ui| {
        // --- Corpus list ---
        ui.allocate_ui_with_layout(
            egui::vec2(list_width, available.y - 4.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.group(|ui| {
                    ui.set_min_width(list_width - 8.0);
                    ui.set_min_height(ui.available_height());
                    ui.label(RichText::new("Corpus").strong());
                    egui::ScrollArea::vertical()
                        .id_salt("review_corpus_list")
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_HEIGHT, state.rows.len(), |ui, row_range| {
                            for row_idx in row_range {
                                let Some(row) = state.rows.get(row_idx).cloned() else {
                                    continue;
                                };
                                let selected = state.selection == Some(row_idx);
                                let checked = state.multi_selected.contains(&row.id);
                                let code_snip = format_code_snip(state.codes_for(&row.id));
                                let label = format_list_row(&row, &code_snip);
                                let indent = if row.parent_item_id.is_some() {
                                    14.0
                                } else {
                                    0.0
                                };
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                                    Sense::click(),
                                );
                                if selected {
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        ui.visuals().selection.bg_fill,
                                    );
                                } else if response.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        ui.visuals().widgets.hovered.bg_fill,
                                    );
                                }
                                // Checkbox glyph (fixed row height — not a nested widget).
                                let box_x = rect.left() + 4.0;
                                let box_c = rect.center().y;
                                let mark = if checked { "☑" } else { "☐" };
                                ui.painter().text(
                                    egui::pos2(box_x, box_c),
                                    egui::Align2::LEFT_CENTER,
                                    mark,
                                    egui::TextStyle::Body.resolve(ui.style()),
                                    ui.visuals().text_color(),
                                );
                                let text_rect = rect.shrink2(egui::vec2(22.0 + indent, 0.0));
                                ui.painter().text(
                                    text_rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    egui::TextStyle::Body.resolve(ui.style()),
                                    if selected {
                                        ui.visuals().selection.stroke.color
                                    } else {
                                        ui.visuals().text_color()
                                    },
                                );
                                // Left strip click toggles multi-select; rest selects current.
                                let check_rect = egui::Rect::from_min_size(
                                    rect.left_top(),
                                    egui::vec2(20.0, ROW_HEIGHT),
                                );
                                if response.clicked() {
                                    if let Some(pos) = response.interact_pointer_pos() {
                                        if check_rect.contains(pos) {
                                            state.toggle_multi_select(&row.id);
                                        } else if ui.input(|i| i.modifiers.shift) {
                                            // Optional shift-range multi-select from current selection.
                                            if let Some(from) = state.selection {
                                                select_range_into(
                                                    &mut state.multi_selected,
                                                    &state.rows,
                                                    from,
                                                    row_idx,
                                                );
                                            }
                                            state.select_index(row_idx, &ctx, matter_root);
                                        } else {
                                            state.select_index(row_idx, &ctx, matter_root);
                                        }
                                    } else {
                                        state.select_index(row_idx, &ctx, matter_root);
                                    }
                                }
                            }
                        });
                });
            },
        );

        // --- Viewer ---
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), available.y - 4.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                show_viewer(ui, state, matter_root, &ctx, actor);
            },
        );
    });
}

fn digit_key_index(i: &egui::InputState) -> Option<usize> {
    const KEYS: [Key; 9] = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ];
    for (idx, k) in KEYS.iter().enumerate() {
        if i.key_pressed(*k) {
            return Some(idx);
        }
    }
    None
}

fn format_code_snip(codes: &[ItemCodeInfo]) -> String {
    if codes.is_empty() {
        return String::new();
    }
    let labels: Vec<&str> = codes.iter().take(3).map(|c| c.label.as_str()).collect();
    let mut s = labels.join(", ");
    if codes.len() > 3 {
        s.push('…');
    }
    s
}

fn format_list_row(row: &ReviewListRow, code_snip: &str) -> String {
    let subj = row
        .subject
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(row.path.as_deref())
        .unwrap_or("(no subject)");
    let from = row.from_addr.as_deref().unwrap_or("");
    let date = row
        .sent_at
        .as_deref()
        .or(row.received_at.as_deref())
        .unwrap_or("");
    let prefix = if row.parent_item_id.is_some() {
        "📎 "
    } else {
        ""
    };
    // Single-line; painter/text will clip visually; keep string short.
    let mut s = format!("{prefix}{subj}");
    if !from.is_empty() {
        s.push_str("  ·  ");
        s.push_str(from);
    }
    if !date.is_empty() {
        s.push_str("  ·  ");
        // Prefer short date prefix if RFC3339-ish
        let short = if date.len() >= 10 { &date[..10] } else { date };
        s.push_str(short);
    }
    if !code_snip.is_empty() {
        s.push_str("  [");
        s.push_str(code_snip);
        s.push(']');
    }
    // Hard cap for list label length (ellipsis).
    const MAX: usize = 120;
    if s.chars().count() > MAX {
        let truncated: String = s.chars().take(MAX.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        s
    }
}

fn show_viewer(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    ctx: &egui::Context,
    actor: &str,
) {
    let row = state.selection.and_then(|i| state.rows.get(i).cloned());

    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.set_min_height(ui.available_height());

        let Some(row) = row else {
            ui.label("Select an item from the list.");
            return;
        };

        // Header
        let subject = row
            .subject
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("(no subject)");
        ui.heading(subject);
        ui.horizontal_wrapped(|ui| {
            if let Some(from) = row.from_addr.as_deref() {
                ui.label(format!("From: {from}"));
                ui.separator();
            }
            // To/Cc from selection-time detail (thin list omits participant JSON).
            if let Some(detail) = state.selection_detail.as_ref() {
                if detail.item_id == row.id {
                    if let Some(to) = detail.to_display.as_deref() {
                        ui.label(format!("To: {to}"));
                        ui.separator();
                    }
                    if let Some(cc) = detail.cc_display.as_deref() {
                        ui.label(format!("Cc: {cc}"));
                        ui.separator();
                    }
                }
            }
            if let Some(sent) = row.sent_at.as_deref() {
                ui.label(format!("Sent: {sent}"));
                ui.separator();
            }
            if let Some(recv) = row.received_at.as_deref() {
                ui.label(format!("Received: {recv}"));
                ui.separator();
            }
            if let Some(role) = row.role.as_deref() {
                ui.label(format!("Role: {role}"));
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(path) = row.path.as_deref() {
                ui.label(RichText::new(path).small().monospace());
            }
            if let Some(mime) = row.mime_type.as_deref() {
                ui.separator();
                ui.label(RichText::new(mime).small());
            }
            if let Some(sz) = row.size_bytes {
                ui.separator();
                ui.label(RichText::new(format!("{sz} bytes")).small());
            }
            if let Some(dedup) = row.dedup_role.as_deref() {
                ui.separator();
                ui.label(RichText::new(format!("dedup={dedup}")).small());
            }
            if let Some(cull) = row.cull_status.as_deref() {
                ui.separator();
                ui.label(RichText::new(format!("cull={cull}")).small());
            }
        });

        // Current-item code chips (click to remove).
        ui.add_space(2.0);
        let current_codes: Vec<ItemCodeInfo> = state.codes_for(&row.id).to_vec();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Codes:").strong().small());
            if current_codes.is_empty() {
                ui.label(RichText::new("(none)").weak().small());
            } else {
                for c in &current_codes {
                    let tip = if c.is_active == 0 {
                        format!("{} (inactive — click to remove)", c.label)
                    } else {
                        format!("{} — click to remove", c.label)
                    };
                    if ui
                        .add(egui::Button::new(format!("[{}]", c.label)).small())
                        .on_hover_text(tip)
                        .clicked()
                        && !state.coding_busy
                    {
                        state.remove_current_code(matter_root, ctx, &c.code_id, actor);
                    }
                }
            }
        });

        // Coding panel
        show_coding_panel(ui, state, matter_root, ctx, actor, &row.id);

        ui.separator();

        // Body
        let body_height = (ui.available_height() - 72.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("review_body_scroll")
            .max_height(body_height)
            .auto_shrink([false, false])
            .show(ui, |ui| match state.body.pane() {
                BodyPane::Idle => {
                    ui.label("…");
                }
                BodyPane::Loading { .. } => {
                    ui.label("Loading…");
                }
                BodyPane::Ready {
                    text,
                    truncated,
                    item_id,
                    ..
                } => {
                    if item_id != &row.id {
                        ui.label("Loading…");
                        return;
                    }
                    if *truncated {
                        ui.colored_label(
                            Color32::from_rgb(180, 120, 40),
                            "Body truncated for display (2 MiB cap). Full text remains in CAS.",
                        );
                    }
                    match text {
                        Ok(s) if s.is_empty() => {
                            ui.label(
                                RichText::new("No extracted text")
                                    .italics()
                                    .color(Color32::GRAY),
                            );
                        }
                        Ok(s) => {
                            ui.add(egui::Label::new(RichText::new(s.as_str()).monospace()).wrap());
                        }
                        Err(e) if e.contains("No extracted text") => {
                            ui.label(
                                RichText::new("No extracted text")
                                    .italics()
                                    .color(Color32::GRAY),
                            );
                        }
                        Err(e) => {
                            ui.colored_label(
                                Color32::from_rgb(200, 60, 60),
                                format!("Body error: {e}"),
                            );
                        }
                    }
                }
            });

        ui.separator();

        // Family / attachment strip
        show_family_strip(ui, state, &row, matter_root, ctx);
    });
}

fn show_coding_panel(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    ctx: &egui::Context,
    actor: &str,
    current_item_id: &str,
) {
    ui.add_space(4.0);
    ui.label(RichText::new("Coding panel").strong().small());
    ui.label(
        RichText::new("Current-item: click a code button to toggle (no confirm). 1–9 shortcuts when focus clear.")
            .weak()
            .small(),
    );

    let active: Vec<CodeDef> = state.active_defs().into_iter().cloned().collect();
    let current_codes: HashSet<String> = state
        .codes_for(current_item_id)
        .iter()
        .map(|c| c.code_id.clone())
        .collect();

    ui.horizontal_wrapped(|ui| {
        for (i, def) in active.iter().enumerate() {
            let on = current_codes.contains(&def.id);
            let shortcut = if i < 9 {
                format!(" [{}]", i + 1)
            } else {
                String::new()
            };
            let label = if on {
                format!("● {}{shortcut}", def.label)
            } else {
                format!("○ {}{shortcut}", def.label)
            };
            if ui
                .add_enabled(!state.coding_busy, egui::Button::new(label).small())
                .on_hover_text(format!("{} ({})", def.key, def.group_key))
                .clicked()
            {
                state.toggle_current_code(matter_root, ctx, &def.id, actor);
            }
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Batch:").strong().small());
        if ui
            .selectable_label(state.batch_mode_add, "Add mode")
            .clicked()
        {
            state.batch_mode_add = true;
        }
        if ui
            .selectable_label(!state.batch_mode_add, "Remove mode")
            .clicked()
        {
            state.batch_mode_add = false;
        }
        ui.checkbox(&mut state.propagate_family, "Apply to family")
            .on_hover_text(
                "Whole family unit: parent + all direct children (siblings). Default off.",
            );
    });

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Batch codes:").small());
        for def in &active {
            let mut checked = state.batch_code_ids.contains(&def.id);
            if ui.checkbox(&mut checked, def.label.as_str()).changed() {
                if checked {
                    state.batch_code_ids.insert(def.id.clone());
                } else {
                    state.batch_code_ids.remove(&def.id);
                }
            }
        }
    });

    let n_sel = state.multi_selected.len();
    let can_batch = n_sel > 0 && !state.batch_code_ids.is_empty() && !state.coding_busy;
    let mode_word = if state.batch_mode_add {
        "Add"
    } else {
        "Remove"
    };
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                can_batch,
                egui::Button::new(format!("{mode_word} on {n_sel} selected")),
            )
            .on_hover_text("Confirm dialog before batch apply")
            .clicked()
        {
            let selected_ids: Vec<String> = state.multi_selected.iter().cloned().collect();
            let code_ids: Vec<String> = state.batch_code_ids.iter().cloned().collect();
            let code_labels: Vec<String> = active
                .iter()
                .filter(|d| state.batch_code_ids.contains(&d.id))
                .map(|d| d.label.clone())
                .collect();
            // Pre-estimate targets: without expand = N; with expand use a local best-effort
            // from loaded rows (parent + siblings in RAM) — API expands for real.
            let target_count = if state.propagate_family {
                estimate_family_targets(&state.rows, &selected_ids)
            } else {
                selected_ids.len()
            };
            state.batch_confirm = Some(BatchConfirm {
                add: state.batch_mode_add,
                code_ids,
                code_labels,
                selected_ids,
                selected_count: n_sel,
                target_count,
                propagate_family: state.propagate_family,
            });
        }
        ui.label(
            RichText::new(format!(
                "(actor: {actor}; Privilege code ≠ full privilege log — see 0031)"
            ))
            .weak()
            .small(),
        );
    });
}

fn estimate_family_targets(rows: &[ReviewListRow], selected_ids: &[String]) -> usize {
    let mut set: HashSet<String> = HashSet::new();
    let by_id: HashMap<&str, &ReviewListRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
    for id in selected_ids {
        let Some(row) = by_id.get(id.as_str()) else {
            set.insert(id.clone());
            continue;
        };
        let parent = row.parent_item_id.clone().unwrap_or_else(|| row.id.clone());
        set.insert(parent.clone());
        for r in rows {
            if r.id == parent || r.parent_item_id.as_deref() == Some(parent.as_str()) {
                set.insert(r.id.clone());
            }
            if let (Some(fid), Some(pfid)) = (
                r.family_id.as_deref(),
                by_id
                    .get(parent.as_str())
                    .and_then(|p| p.family_id.as_deref())
                    .or(row.family_id.as_deref()),
            ) {
                if fid == pfid {
                    set.insert(r.id.clone());
                }
            }
        }
    }
    set.len().max(selected_ids.len())
}

fn show_batch_confirm(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    ctx: &egui::Context,
    actor: &str,
) {
    let Some(confirm) = state.batch_confirm.clone() else {
        return;
    };
    let mut open = true;
    egui::Window::new("Confirm batch coding")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            let mode = if confirm.add { "Add" } else { "Remove" };
            let codes = confirm.code_labels.join(", ");
            ui.label(format!(
                "{mode} [{codes}] on {} selected item(s)",
                confirm.selected_count
            ));
            if confirm.propagate_family {
                ui.label(format!(
                    "(family expanded → ~{} targets; final count is after whole-family unit expand)",
                    confirm.target_count
                ));
            } else {
                ui.label(format!("(no family expand → {} targets)", confirm.target_count));
            }
            ui.label(
                RichText::new("Audit records every target id. This cannot be undone except by Remove mode.")
                    .small()
                    .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    state.batch_confirm = None;
                }
                if ui
                    .add_enabled(!state.coding_busy, egui::Button::new("Apply"))
                    .clicked()
                {
                    let (add_ids, remove_ids) = if confirm.add {
                        (confirm.code_ids.clone(), Vec::new())
                    } else {
                        (Vec::new(), confirm.code_ids.clone())
                    };
                    let input = ApplyCodesInput {
                        item_ids: confirm.selected_ids.clone(),
                        add_code_ids: add_ids,
                        remove_code_ids: remove_ids,
                        propagate_family: confirm.propagate_family,
                        actor: actor.to_string(),
                    };
                    state.batch_confirm = None;
                    state.apply_codes_now(matter_root, ctx, input);
                }
            });
        });
    if !open {
        state.batch_confirm = None;
    }
}

fn show_family_strip(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    current: &ReviewListRow,
    matter_root: &Utf8Path,
    ctx: &egui::Context,
) {
    ui.label(RichText::new("Family / attachments").strong().small());
    let family_id = current.family_id.as_deref();
    let members: Vec<(usize, ReviewListRow)> = if let Some(fid) = family_id {
        state
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.family_id.as_deref() == Some(fid))
            .map(|(i, r)| (i, r.clone()))
            .collect()
    } else if current.parent_item_id.is_some() || current.attachment_count.unwrap_or(0) > 0 {
        // Fallback: parent + children linked by parent_item_id within loaded rows.
        let parent_id = current
            .parent_item_id
            .clone()
            .unwrap_or_else(|| current.id.clone());
        state
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id == parent_id || r.parent_item_id.as_deref() == Some(&parent_id))
            .map(|(i, r)| (i, r.clone()))
            .collect()
    } else {
        Vec::new()
    };

    if members.is_empty() {
        ui.label(
            RichText::new("No family members in current list.")
                .weak()
                .small(),
        );
        return;
    }

    ui.horizontal_wrapped(|ui| {
        for (idx, m) in &members {
            let is_cur = m.id == current.id;
            let label = m
                .subject
                .as_deref()
                .or(m.path.as_deref())
                .unwrap_or(m.id.as_str());
            let short: String = label.chars().take(40).collect();
            let text = if m.parent_item_id.is_some() {
                format!("📎 {short}")
            } else {
                short
            };
            if ui
                .add_enabled(!is_cur, egui::Button::new(text).small())
                .clicked()
            {
                state.select_index(*idx, ctx, matter_root);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::{
        item_role, item_status, ItemInput, PromoteFieldUpdate, DEFAULT_REVIEW_SET_NAME,
    };
    use tempfile::TempDir;

    #[test]
    fn row_height_is_fixed_positive() {
        // Document the virtualization contract: uniform height in (0, 100).
        // Coding multi-select must not change this constant (virtualization).
        const {
            assert!(ROW_HEIGHT > 0.0);
            assert!(ROW_HEIGHT < 100.0);
        };
        assert!((ROW_HEIGHT - 22.0).abs() < f32::EPSILON);
    }

    #[test]
    fn selection_set_toggle_and_range() {
        let mut set = HashSet::new();
        toggle_selection_set(&mut set, "a");
        assert!(set.contains("a"));
        toggle_selection_set(&mut set, "a");
        assert!(!set.contains("a"));
        toggle_selection_set(&mut set, "a");
        toggle_selection_set(&mut set, "b");
        assert_eq!(set.len(), 2);

        let rows = vec![
            ReviewListRow {
                id: "r0".into(),
                review_order: Some(0),
                role: None,
                parent_item_id: None,
                subject: Some("0".into()),
                from_addr: None,
                sent_at: None,
                received_at: None,
                path: None,
                file_category: None,
                mime_type: None,
                size_bytes: None,
                text_sha256: None,
                html_sha256: None,
                dedup_role: None,
                cull_status: None,
                attachment_count: None,
                family_id: None,
            },
            ReviewListRow {
                id: "r1".into(),
                review_order: Some(1),
                role: None,
                parent_item_id: None,
                subject: Some("1".into()),
                from_addr: None,
                sent_at: None,
                received_at: None,
                path: None,
                file_category: None,
                mime_type: None,
                size_bytes: None,
                text_sha256: None,
                html_sha256: None,
                dedup_role: None,
                cull_status: None,
                attachment_count: None,
                family_id: None,
            },
            ReviewListRow {
                id: "r2".into(),
                review_order: Some(2),
                role: None,
                parent_item_id: None,
                subject: Some("2".into()),
                from_addr: None,
                sent_at: None,
                received_at: None,
                path: None,
                file_category: None,
                mime_type: None,
                size_bytes: None,
                text_sha256: None,
                html_sha256: None,
                dedup_role: None,
                cull_status: None,
                attachment_count: None,
                family_id: None,
            },
        ];
        let mut range = HashSet::new();
        select_range_into(&mut range, &rows, 0, 2);
        assert_eq!(range.len(), 3);
        assert!(range.contains("r0") && range.contains("r2"));
    }

    #[test]
    fn load_review_thin_integration() {
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-review-ui");
        let matter = Matter::create(&root, "Review UI").expect("create");
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        let digest = matter
            .cas()
            .put_bytes(b"Body text for review")
            .expect("cas");
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("Review me".into()),
                from_addr: Some("a@ex.com".into()),
                text_sha256: Some(digest.clone()),
                path: Some("msg.eml".into()),
                ..Default::default()
            })
            .expect("item");
        // Non-review
        let _ = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                subject: Some("Skip".into()),
                ..Default::default()
            })
            .expect("skip");
        let job = matter.create_job("promote").expect("job");
        matter
            .apply_promote_batch_with_checkpoint(
                &job.id,
                "promote",
                &[PromoteFieldUpdate {
                    item_id: item.id.clone(),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(1),
                    promoted_at: Some("2020-01-01T00:00:00Z".into()),
                    promote_job_id: Some(job.id.clone()),
                    promote_policy: Some("unique_only".into()),
                }],
                "{}",
                1,
            )
            .expect("promote");
        drop(matter);

        let (count, rows) = load_review_thin(&root).expect("load");
        assert_eq!(count, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, item.id);
        assert_eq!(rows[0].subject.as_deref(), Some("Review me"));

        let (text, truncated) =
            crate::review_body::load_body_from_cas(&root, Some(digest.as_str()), None)
                .expect("body");
        assert_eq!(text, "Body text for review");
        assert!(!truncated);
    }

    #[test]
    fn empty_corpus_load() {
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-empty-review");
        let _ = Matter::create(&root, "Empty").expect("create");
        let (count, rows) = load_review_thin(&root).expect("load");
        assert_eq!(count, 0);
        assert!(rows.is_empty());
    }

    #[test]
    fn format_addrs_json_joins_and_truncates() {
        let raw = r#"["a@ex.com","b@ex.com"]"#;
        assert_eq!(
            format_addrs_json(Some(raw), 160).as_deref(),
            Some("a@ex.com; b@ex.com")
        );
        assert!(format_addrs_json(Some("[]"), 160).is_none());
        assert!(format_addrs_json(None, 160).is_none());
        let long = format!(r#"["{}"]"#, "x".repeat(200));
        let out = format_addrs_json(Some(&long), 40).expect("truncated");
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 40);
    }

    #[test]
    fn load_party_detail_shows_to_cc() {
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-parties");
        let matter = Matter::create(&root, "Parties").expect("create");
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                subject: Some("With parties".into()),
                from_addr: Some("from@ex.com".into()),
                to_addrs_json: Some(r#"["to1@ex.com","to2@ex.com"]"#.into()),
                cc_addrs_json: Some(r#"["cc@ex.com"]"#.into()),
                ..Default::default()
            })
            .expect("item");
        drop(matter);
        let detail = load_party_detail(&root, &item.id).expect("detail");
        assert_eq!(detail.item_id, item.id);
        assert_eq!(detail.to_display.as_deref(), Some("to1@ex.com; to2@ex.com"));
        assert_eq!(detail.cc_display.as_deref(), Some("cc@ex.com"));
    }
}
