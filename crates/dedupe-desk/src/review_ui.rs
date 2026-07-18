//! Review screen: linear corpus list + body viewer + family strip + coding + filters
//! + notes/highlights (0026/0027/0028/0029/0030).
//!
//! # List virtualization
//!
//! Rows use a **fixed** [`ROW_HEIGHT`] so `ScrollArea::show_rows` can skip
//! non-visible items. Subject/from/date are single-line truncated — never wrap
//! list rows (variable height breaks virtualization).
//!
//! # Load policy
//!
//! Thin rows only (`list_review_thin` / `list_items_filtered_thin`). If count ≤
//! [`THIN_LOAD_ALL_THRESHOLD`], load the full thin list; otherwise page in
//! chunks of [`THIN_PAGE_SIZE`] with **Load more**. Never load full corpus
//! bodies into the list.
//!
//! # Coding (0027)
//!
//! Codes for **visible rows** only (`list_item_codes`). Multi-select + batch
//! Add/Remove with optional whole-family propagate. Digits 1–9 toggle the first
//! nine active codes on the current item when the focus gate is clear.
//!
//! # Filters (0028) + keyword FTS (0029)
//!
//! Metadata [`FilterSpec`] composes with optional Tantivy keyword via
//! `compose_keyword_filter`. Keyword / filter text fields steal focus; digit
//! shortcuts respect `focus().is_none()`.
//!
//! # Notes / highlights (0030)
//!
//! Stand-off work-product annotations in the matter DB (never CAS). Selectable
//! body + yellow paint for active ranges; notes panel for document/passage notes.
//!
//! # Privilege (0031)
//!
//! Claim fields + withhold hold + privilege log export. Panel when Privilege code
//! or claim row is present (or Assert). Notes never auto-copy into log description.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense};
use matter_core::{
    parse_bound_instant, ApplyCodesInput, ApplyCodesResult, CodeDef, CodeDefInput, FilterCondition,
    FilterSpec, ItemCodeInfo, ItemHighlight, ItemNote, ItemPrivilege, Matter, ResolvedHighlight,
    ReviewListRow, SavedSearch, SavedSearchInput, UpsertNoteInput, UpsertPrivilegeProtocolInput,
};

use crate::review_body::{BodyLoader, BodyPane};
use crate::review_nav;
use crate::review_notes::{
    body_digest_for_item, body_job_for_ui, find_highlight_for_selection, find_resolved,
    highlight_input_from_selection, highlight_ui_status, note_upsert_from_draft,
    passage_note_hint_from_quote, resolve_for_paint, selection_from_char_range, stale_count_for_ui,
    BodySelection,
};
use crate::review_privilege::{
    assert_privilege_blocking, basis_options, default_privilege_log_path,
    draft_description_from_note, export_privilege_log_blocking, family_split_banner,
    focus_allows_coding_with_privilege, load_privilege_panel, load_protocol_blocking,
    should_show_privilege_panel, status_options, upsert_privilege_blocking,
    upsert_protocol_blocking, PrivilegePanelDraft,
};

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

/// Draft fields for the Review filter bar (0028 + 0030 notes chips).
#[derive(Debug, Clone, Default)]
pub struct FilterDraft {
    pub custodian: String,
    pub date_from: String,
    pub date_to: String,
    pub include_family: bool,
    /// Selected code **keys** for any_of (empty = no code condition).
    pub code_keys: HashSet<String>,
    /// Uncoded chip / `code_missing eq true`. Mutually exclusive with [`Self::code_keys`]
    /// when serializing via [`Self::to_filter_spec`].
    pub code_missing: bool,
    /// `has_notes eq true` chip (track 0030).
    pub has_notes: bool,
    /// `has_highlights eq true` chip (track 0030).
    pub has_highlights: bool,
    /// Withhold hold chip (track 0031).
    pub privilege_withheld: bool,
    /// Privilege log incomplete chip (track 0031).
    pub privilege_log_incomplete: bool,
    /// Optional note body contains (LIKE).
    pub note_text: String,
    /// Name for Save as.
    pub save_name: String,
    /// Currently selected saved search id in the dropdown (if any).
    pub selected_saved_id: Option<String>,
}

impl FilterDraft {
    /// Build a [`FilterSpec`] from draft fields. Empty draft → default corpus filter.
    ///
    /// Dates are pre-validated with [`parse_bound_instant`] so naive timestamps fail
    /// before list load. `code_missing` and `code_keys` are mutually exclusive: when
    /// `code_missing` is set, only the uncoded condition is emitted.
    pub fn to_filter_spec(&self) -> Result<FilterSpec, String> {
        let mut conditions = Vec::new();
        let cust = self.custodian.trim();
        if !cust.is_empty() {
            conditions.push(FilterCondition {
                field: "custodian".into(),
                op: "contains".into(),
                value: Some(serde_json::json!(cust)),
                values: None,
                start: None,
                end: None,
            });
        }
        if self.code_missing {
            conditions.push(FilterCondition {
                field: "code_missing".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            });
        } else if !self.code_keys.is_empty() {
            let mut keys: Vec<String> = self.code_keys.iter().cloned().collect();
            keys.sort();
            conditions.push(FilterCondition {
                field: "code".into(),
                op: "any_of".into(),
                value: None,
                values: Some(keys),
                start: None,
                end: None,
            });
        }
        if self.has_notes {
            conditions.push(FilterCondition {
                field: "has_notes".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            });
        }
        if self.has_highlights {
            conditions.push(FilterCondition {
                field: "has_highlights".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            });
        }
        if self.privilege_withheld {
            conditions.push(FilterCondition {
                field: "privilege_withhold".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            });
        }
        if self.privilege_log_incomplete {
            conditions.push(FilterCondition {
                field: "privilege_status".into(),
                op: "any_of".into(),
                value: None,
                values: Some(vec!["asserted".into()]),
                start: None,
                end: None,
            });
            conditions.push(FilterCondition {
                field: "privilege_log_ready".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(false)),
                values: None,
                start: None,
                end: None,
            });
        }
        let note_q = self.note_text.trim();
        if !note_q.is_empty() {
            conditions.push(FilterCondition {
                field: "note_text".into(),
                op: "contains".into(),
                value: Some(serde_json::json!(note_q)),
                values: None,
                start: None,
                end: None,
            });
        }
        let from = self.date_from.trim();
        let to = self.date_to.trim();
        if !from.is_empty() || !to.is_empty() {
            if from.is_empty() || to.is_empty() {
                return Err("Date filter needs both From and To (RFC3339 with offset or Z)".into());
            }
            parse_bound_instant(from).map_err(|e| e.to_string())?;
            parse_bound_instant(to).map_err(|e| e.to_string())?;
            conditions.push(FilterCondition {
                field: "best_effort_date".into(),
                op: "between".into(),
                value: None,
                values: None,
                start: Some(from.to_string()),
                end: Some(to.to_string()),
            });
        }
        Ok(FilterSpec {
            include_family: self.include_family,
            conditions,
            ..FilterSpec::default()
        })
    }

    /// Populate draft from an applied / loaded FilterSpec (best-effort).
    pub fn from_filter_spec(spec: &FilterSpec) -> Self {
        let mut d = Self {
            include_family: spec.include_family,
            ..Self::default()
        };
        for c in &spec.conditions {
            match (c.field.as_str(), c.op.as_str()) {
                ("custodian", "eq" | "contains") => {
                    if let Some(v) = c.value.as_ref().and_then(|v| v.as_str()) {
                        d.custodian = v.to_string();
                    }
                }
                ("code", "any_of") => {
                    if let Some(vals) = &c.values {
                        for k in vals {
                            d.code_keys.insert(k.clone());
                        }
                    }
                }
                ("code_missing", _) => {
                    // Default true when value omitted (engine treats missing as true).
                    let want = c.value.as_ref().and_then(|v| v.as_bool()).unwrap_or(true);
                    d.code_missing = want;
                    if want {
                        d.code_keys.clear();
                    }
                }
                ("has_notes", "eq") => {
                    d.has_notes = c.value.as_ref().and_then(|v| v.as_bool()).unwrap_or(true);
                }
                ("has_highlights", "eq") => {
                    d.has_highlights = c.value.as_ref().and_then(|v| v.as_bool()).unwrap_or(true);
                }
                ("privilege_withhold", "eq") => {
                    d.privilege_withheld =
                        c.value.as_ref().and_then(|v| v.as_bool()).unwrap_or(true);
                }
                ("privilege_log_ready", "eq") => {
                    // Incomplete chip uses eq false; mark draft when loading incomplete preset.
                    let ready = c.value.as_ref().and_then(|v| v.as_bool()).unwrap_or(true);
                    if !ready {
                        d.privilege_log_incomplete = true;
                    }
                }
                ("note_text", "contains") => {
                    if let Some(v) = c.value.as_ref().and_then(|v| v.as_str()) {
                        d.note_text = v.to_string();
                    }
                }
                ("best_effort_date" | "sent_at" | "received_at", "between") => {
                    if let Some(s) = &c.start {
                        d.date_from = s.clone();
                    }
                    if let Some(e) = &c.end {
                        d.date_to = e.clone();
                    }
                }
                _ => {}
            }
        }
        d
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Review screen state held by the desk app.
#[derive(Default)]
pub struct ReviewState {
    /// Thin rows currently in RAM (ordered by `review_order` / filter order).
    pub rows: Vec<ReviewListRow>,
    /// Total corpus / filtered count (may exceed `rows.len()` when paged).
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
    /// Codes for **visible** (and selected) item ids only — not the full thin list.
    row_codes: HashMap<String, Vec<ItemCodeInfo>>,
    /// Last `ScrollArea::show_rows` viewport range into `rows` (for code load scope).
    visible_row_range: std::ops::Range<usize>,
    /// Batch panel mode: true = Add, false = Remove.
    batch_mode_add: bool,
    /// Codes checked in the batch panel (definition ids).
    batch_code_ids: HashSet<String>,
    /// Whole-family propagate checkbox (default false).
    propagate_family: bool,
    /// Open batch confirm dialog.
    batch_confirm: Option<BatchConfirm>,
    /// Expand “Add code…” form in the coding panel.
    show_add_code: bool,
    /// Draft label for new custom code.
    add_code_label: String,
    /// Group for new custom code (`custom` or `issues`).
    add_code_group: String,
    /// Async coding apply in flight.
    coding_busy: bool,
    coding_rx: Option<Receiver<Result<ApplyCodesResult, String>>>,
    coding_status: Option<String>,
    coding_error: Option<String>,
    /// Filter bar draft (edit fields).
    pub filter_draft: FilterDraft,
    /// Applied filter (None = full corpus via empty default FilterSpec path).
    pub applied_filter: Option<FilterSpec>,
    /// True when an Apply was used with non-empty conditions or include_family.
    pub filter_active: bool,
    /// Draft keyword box text.
    pub keyword_draft: String,
    /// Applied keyword (None / empty = metadata-only list).
    pub applied_keyword: Option<String>,
    /// Approx FTS hit count before filter intersection (when keyword active).
    pub keyword_hit_count: Option<u64>,
    /// Index status banner / search errors.
    pub keyword_error: Option<String>,
    /// True when index is missing/stale (banner).
    pub index_outdated: bool,
    /// Saved searches for the open matter.
    saved_searches: Vec<SavedSearch>,
    /// Filter validation / save errors.
    filter_error: Option<String>,
    /// Filter status line (e.g. saved / loaded).
    filter_status: Option<String>,
    /// Notes for the current selection (newest first).
    item_notes: Vec<ItemNote>,
    /// Highlights for the current selection (stored SQLite rows).
    item_highlights: Vec<ItemHighlight>,
    /// `(item_id, display_digest)` for which we already ran
    /// `resolve_highlights(..., persist_stale: true)` this session.
    stale_persist_key: Option<(String, String)>,
    /// Draft for new document or passage note (user-entered only).
    note_draft: String,
    /// When set, next Save binds the draft as a passage note to this highlight.
    pending_highlight_id: Option<String>,
    /// Hint-only quote context for the passage-note editor (never auto-saved).
    passage_note_hint: String,
    /// Note id being edited (if any) + draft body.
    note_edit_id: Option<String>,
    note_edit_body: String,
    /// Last body char selection (for Highlight / Note on selection).
    body_selection: Option<BodySelection>,
    /// Buffer for selectable body TextEdit (reverted if mutated).
    body_edit_buf: String,
    /// Item id that `body_edit_buf` was synced from.
    body_edit_item_id: Option<String>,
    /// True when a notes TextEdit had focus last frame (coding focus gate).
    note_editor_focused: bool,
    /// Notes/highlights status / errors.
    notes_status: Option<String>,
    notes_error: Option<String>,
    /// Async note/highlight mutate in flight.
    notes_busy: bool,
    notes_rx: Option<Receiver<Result<NotesMutateResult, String>>>,
    /// Privilege claim for current selection (if any).
    item_privilege: Option<ItemPrivilege>,
    /// Privilege panel draft fields.
    privilege_draft: PrivilegePanelDraft,
    /// Force-open panel after Assert even if row not yet reloaded.
    privilege_force_open: bool,
    /// Family split banner text (when inconsistent).
    privilege_family_banner: Option<String>,
    /// Privilege description TextEdit focused last frame (coding focus gate).
    privilege_editor_focused: bool,
    /// Privilege panel status / errors.
    privilege_status_msg: Option<String>,
    privilege_error: Option<String>,
    /// Confirm dialog: draft description from latest note.
    privilege_confirm_note_draft: bool,
    /// Confirm dialog: set withhold=0 while still asserted.
    privilege_confirm_clear_withhold: bool,
    /// Pending withhold=false save after confirm.
    privilege_pending_save_no_withhold: bool,
    /// Export progress / result.
    privilege_export_status: Option<String>,
    privilege_export_error: Option<String>,
    privilege_export_busy: bool,
    privilege_export_rx: Option<Receiver<Result<String, String>>>,
    /// Export scope: true = review_corpus.
    privilege_export_review_only: bool,
    /// Matter protocol draft (thin settings).
    protocol_draft_502d: String,
    protocol_draft_502e: String,
    protocol_description_required: bool,
    protocol_loaded_for: Option<Utf8PathBuf>,
    protocol_status: Option<String>,
}

/// Result of an off-thread notes/highlights mutation.
#[derive(Debug)]
struct NotesMutateResult {
    item_id: String,
    notes: Vec<ItemNote>,
    highlights: Vec<ItemHighlight>,
    message: String,
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
        self.visible_row_range = 0..0;
        self.batch_code_ids.clear();
        self.batch_confirm = None;
        self.show_add_code = false;
        self.add_code_label.clear();
        self.add_code_group = "custom".into();
        self.coding_busy = false;
        self.coding_rx = None;
        self.coding_status = None;
        self.coding_error = None;
        self.batch_mode_add = true;
        self.propagate_family = false;
        self.filter_draft.clear();
        self.applied_filter = None;
        self.filter_active = false;
        self.keyword_draft.clear();
        self.applied_keyword = None;
        self.keyword_hit_count = None;
        self.keyword_error = None;
        self.index_outdated = false;
        self.saved_searches.clear();
        self.filter_error = None;
        self.filter_status = None;
        self.item_notes.clear();
        self.item_highlights.clear();
        self.stale_persist_key = None;
        self.note_draft.clear();
        self.pending_highlight_id = None;
        self.passage_note_hint.clear();
        self.note_edit_id = None;
        self.note_edit_body.clear();
        self.body_selection = None;
        self.body_edit_buf.clear();
        self.body_edit_item_id = None;
        self.note_editor_focused = false;
        self.notes_status = None;
        self.notes_error = None;
        self.notes_busy = false;
        self.notes_rx = None;
        self.item_privilege = None;
        self.privilege_draft = PrivilegePanelDraft::default();
        self.privilege_force_open = false;
        self.privilege_family_banner = None;
        self.privilege_editor_focused = false;
        self.privilege_status_msg = None;
        self.privilege_error = None;
        self.privilege_confirm_note_draft = false;
        self.privilege_confirm_clear_withhold = false;
        self.privilege_pending_save_no_withhold = false;
        self.privilege_export_status = None;
        self.privilege_export_error = None;
        self.privilege_export_busy = false;
        self.privilege_export_rx = None;
        self.privilege_export_review_only = true;
        self.protocol_draft_502d.clear();
        self.protocol_draft_502e.clear();
        self.protocol_description_required = true;
        self.protocol_loaded_for = None;
        self.protocol_status = None;
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
        self.keyword_error = None;
        // Always drop in-flight / stale body + parties: selection may change after
        // re-promote (item demoted/removed). Leaving Ready/Loading with an old
        // item_id would show permanent "Loading…" because spawn only runs on Idle.
        self.body.clear();
        self.selection_detail = None;
        let kw = self
            .applied_keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let keyword_active = kw.is_some();
        let load = if keyword_active || self.filter_active {
            let spec = self.applied_filter.clone().unwrap_or_default();
            // When only keyword is active, still use FilterSpec default (review_corpus).
            load_review_composed(matter_root, kw, &spec, 0, None)
        } else {
            load_review_thin(matter_root).map(|(c, r)| (c, r, false, None))
        };
        match load {
            Ok((count, rows, _more, fts_hits)) => {
                self.count = count;
                self.rows = rows;
                self.keyword_hit_count = fts_hits;
                self.index_outdated = false;
                self.loaded_root = Some(matter_root.to_owned());
                // Viewport unknown until next paint; fallback window used for first load.
                self.visible_row_range = 0..0;
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
                self.reload_saved_searches(matter_root);
            }
            Err(e) => {
                let el = e.to_ascii_lowercase();
                if el.contains("index") || el.contains("build") || el.contains("outdated") {
                    self.index_outdated = true;
                    self.keyword_error = Some(e.clone());
                }
                // Filter/keyword load failures: surface so Clear stays usable.
                // Keep previous rows when possible so the list is not blanked on a bad Apply.
                if self.filter_active || keyword_active {
                    self.filter_error = Some(format!("List load: {e}"));
                    if self.rows.is_empty() {
                        self.list_error = Some(e);
                        self.count = 0;
                        self.selection = None;
                    }
                } else {
                    self.list_error = Some(e);
                    self.rows.clear();
                    self.count = 0;
                    self.selection = None;
                    self.code_defs.clear();
                    self.row_codes.clear();
                }
                self.loaded_root = Some(matter_root.to_owned());
                self.visible_row_range = 0..0;
            }
        }
    }

    /// Apply keyword draft and reload (Enter / Search button).
    pub fn apply_keyword(&mut self, matter_root: &Utf8Path) {
        let kw = self.keyword_draft.trim().to_string();
        if kw.is_empty() {
            self.clear_keyword(matter_root);
            return;
        }
        self.applied_keyword = Some(kw);
        self.keyword_error = None;
        self.needs_reload = true;
        self.ensure_loaded(matter_root);
    }

    /// Clear keyword; restore metadata-only (or unfiltered corpus).
    pub fn clear_keyword(&mut self, matter_root: &Utf8Path) {
        self.keyword_draft.clear();
        self.applied_keyword = None;
        self.keyword_hit_count = None;
        self.keyword_error = None;
        self.index_outdated = false;
        self.needs_reload = true;
        self.ensure_loaded(matter_root);
    }

    fn reload_saved_searches(&mut self, matter_root: &Utf8Path) {
        match load_saved_searches(matter_root) {
            Ok(list) => self.saved_searches = list,
            Err(e) => {
                self.filter_error = Some(format!("Saved searches: {e}"));
            }
        }
    }

    /// Apply draft filter and reload list.
    pub fn apply_filter(&mut self, matter_root: &Utf8Path) {
        match self.filter_draft.to_filter_spec() {
            Ok(spec) => {
                let active = !spec.conditions.is_empty() || spec.include_family;
                self.applied_filter = Some(spec);
                self.filter_active = active;
                self.filter_error = None;
                self.filter_status = if active {
                    Some("Filter applied.".into())
                } else {
                    Some("No conditions — showing full corpus.".into())
                };
                self.needs_reload = true;
                self.ensure_loaded(matter_root);
            }
            Err(e) => {
                self.filter_error = Some(e);
                self.filter_status = None;
            }
        }
    }

    /// Clear filter draft + applied filter; restore full corpus.
    pub fn clear_filter(&mut self, matter_root: &Utf8Path) {
        self.filter_draft.clear();
        self.applied_filter = None;
        self.filter_active = false;
        self.filter_error = None;
        self.filter_status = Some("Filter cleared.".into());
        self.needs_reload = true;
        self.ensure_loaded(matter_root);
    }

    /// Apply a preset FilterSpec (quick chip).
    pub fn apply_preset(&mut self, matter_root: &Utf8Path, spec: FilterSpec) {
        self.filter_draft = FilterDraft::from_filter_spec(&spec);
        self.applied_filter = Some(spec);
        self.filter_active = true;
        self.filter_error = None;
        self.filter_status = Some("Preset applied.".into());
        self.needs_reload = true;
        self.ensure_loaded(matter_root);
    }

    /// Load more rows when count exceeds loaded (filtered or large corpus).
    pub fn load_more(&mut self, matter_root: &Utf8Path) {
        if (self.rows.len() as u64) >= self.count {
            return;
        }
        let offset = self.rows.len() as u64;
        let kw = self
            .applied_keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let result = if kw.is_some() || self.filter_active {
            let spec = self.applied_filter.clone().unwrap_or_default();
            load_review_composed(matter_root, kw, &spec, offset, Some(THIN_PAGE_SIZE))
                .map(|(c, r, more, _)| (c, r, more))
        } else {
            load_review_page(matter_root, offset, THIN_PAGE_SIZE).map(|(c, r)| {
                let n = r.len() as u64;
                (c, r, offset + n < c)
            })
        };
        match result {
            Ok((count, more_rows, _)) => {
                self.count = count;
                let existing: HashSet<String> = self.rows.iter().map(|r| r.id.clone()).collect();
                for r in more_rows {
                    if !existing.contains(&r.id) {
                        self.rows.push(r);
                    }
                }
                self.filter_error = None;
            }
            Err(e) => {
                self.filter_error = Some(format!("Load more: {e}"));
            }
        }
    }

    fn save_current_filter(&mut self, matter_root: &Utf8Path, actor: &str) {
        let name = self.filter_draft.save_name.trim().to_string();
        if name.is_empty() {
            self.filter_error = Some("Enter a name to save the search.".into());
            return;
        }
        // Prefer the *applied* FilterSpec when a filter is active so Uncoded /
        // loaded non-UI conditions are preserved (draft is a partial view).
        let spec = if self.filter_active {
            if let Some(applied) = self.applied_filter.clone() {
                applied
            } else {
                match self.filter_draft.to_filter_spec() {
                    Ok(s) => s,
                    Err(e) => {
                        self.filter_error = Some(e);
                        return;
                    }
                }
            }
        } else {
            match self.filter_draft.to_filter_spec() {
                Ok(s) => s,
                Err(e) => {
                    self.filter_error = Some(e);
                    return;
                }
            }
        };
        let filter_json = match serde_json::to_string(&spec) {
            Ok(j) => j,
            Err(e) => {
                self.filter_error = Some(format!("Serialize filter: {e}"));
                return;
            }
        };
        // Upsert by name if an existing saved search matches.
        let existing_id = self
            .saved_searches
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.id.clone());
        let keyword = self.applied_keyword.clone().or_else(|| {
            let t = self.keyword_draft.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        });
        match upsert_saved_search(
            matter_root,
            SavedSearchInput {
                id: existing_id,
                name,
                description: None,
                filter_json,
                keyword,
                created_by: Some(actor.to_string()),
            },
        ) {
            Ok(saved) => {
                // Keep applied_filter in sync with what was persisted.
                let active = !spec.conditions.is_empty() || spec.include_family;
                self.applied_filter = Some(spec);
                self.filter_active = active;
                if let Some(ref kw) = saved.keyword {
                    self.keyword_draft = kw.clone();
                    self.applied_keyword = Some(kw.clone());
                }
                self.filter_draft.selected_saved_id = Some(saved.id.clone());
                self.filter_draft.save_name = saved.name.clone();
                self.filter_status = Some(format!("Saved “{}”.", saved.name));
                self.filter_error = None;
                self.reload_saved_searches(matter_root);
            }
            Err(e) => {
                self.filter_error = Some(e);
            }
        }
    }

    fn load_selected_saved_search(&mut self, matter_root: &Utf8Path) {
        let Some(id) = self.filter_draft.selected_saved_id.clone() else {
            self.filter_error = Some("Select a saved search first.".into());
            return;
        };
        let Some(ss) = self.saved_searches.iter().find(|s| s.id == id).cloned() else {
            self.filter_error = Some("Saved search not found.".into());
            return;
        };
        match serde_json::from_str::<FilterSpec>(&ss.filter_json) {
            Ok(spec) => {
                self.filter_draft = FilterDraft::from_filter_spec(&spec);
                self.filter_draft.selected_saved_id = Some(ss.id);
                self.filter_draft.save_name = ss.name.clone();
                self.applied_filter = Some(spec.clone());
                self.filter_active = !spec.conditions.is_empty() || spec.include_family;
                // Restore keyword from saved search (schema v10).
                if let Some(ref kw) = ss.keyword {
                    self.keyword_draft = kw.clone();
                    self.applied_keyword = Some(kw.clone());
                } else {
                    self.keyword_draft.clear();
                    self.applied_keyword = None;
                    self.keyword_hit_count = None;
                }
                self.filter_error = None;
                self.filter_status = Some(format!("Loaded “{}”.", ss.name));
                self.needs_reload = true;
                self.ensure_loaded(matter_root);
            }
            Err(e) => {
                self.filter_error = Some(format!("Invalid saved filter: {e}"));
            }
        }
    }

    fn delete_selected_saved_search(&mut self, matter_root: &Utf8Path) {
        let Some(id) = self.filter_draft.selected_saved_id.clone() else {
            self.filter_error = Some("Select a saved search to delete.".into());
            return;
        };
        match delete_saved_search(matter_root, &id) {
            Ok(()) => {
                self.filter_draft.selected_saved_id = None;
                self.filter_status = Some("Saved search deleted.".into());
                self.filter_error = None;
                self.reload_saved_searches(matter_root);
            }
            Err(e) => {
                self.filter_error = Some(e);
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
        let ids =
            item_ids_for_code_load(&self.rows, &self.visible_row_range, self.current_item_id());
        if ids.is_empty() {
            self.row_codes.clear();
            return;
        }
        match load_item_codes(matter_root, &ids) {
            Ok(map) => self.row_codes = map,
            Err(e) => {
                self.coding_error = Some(format!("Load codes: {e}"));
            }
        }
    }

    /// Remember viewport from `show_rows` and reload codes when it moves.
    fn note_visible_row_range(&mut self, matter_root: &Utf8Path, range: std::ops::Range<usize>) {
        if self.visible_row_range == range {
            return;
        }
        self.visible_row_range = range;
        self.refresh_row_codes(matter_root);
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
                self.reload_privilege_for_selection(matter_root);
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
        // Multi-item batch, family expand (unknown target size), or large N → off UI thread.
        if should_apply_codes_off_thread(
            input.item_ids.len(),
            input.propagate_family,
            CODING_OFF_THREAD_THRESHOLD,
        ) {
            self.spawn_apply_codes(matter_root, ctx, input);
        } else {
            match apply_codes_blocking(matter_root, input) {
                Ok(result) => {
                    self.coding_status = Some(format!("Coded {} item(s).", result.target_count));
                    self.coding_error = None;
                    self.refresh_row_codes(matter_root);
                    self.reload_privilege_for_selection(matter_root);
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

    fn clear_notes_for_selection(&mut self) {
        self.item_notes.clear();
        self.item_highlights.clear();
        self.stale_persist_key = None;
        self.note_draft.clear();
        self.pending_highlight_id = None;
        self.passage_note_hint.clear();
        self.note_edit_id = None;
        self.note_edit_body.clear();
        self.body_selection = None;
        self.body_edit_buf.clear();
        self.body_edit_item_id = None;
        self.notes_status = None;
        self.notes_error = None;
    }

    fn clear_privilege_for_selection(&mut self) {
        self.item_privilege = None;
        self.privilege_draft = PrivilegePanelDraft::default();
        self.privilege_force_open = false;
        self.privilege_family_banner = None;
        self.privilege_status_msg = None;
        self.privilege_error = None;
        self.privilege_confirm_note_draft = false;
        self.privilege_confirm_clear_withhold = false;
        self.privilege_pending_save_no_withhold = false;
    }

    fn reload_notes_for_selection(&mut self, matter_root: &Utf8Path) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            self.clear_notes_for_selection();
            return;
        };
        match load_notes_highlights(matter_root, &item_id) {
            Ok((notes, highlights)) => {
                self.item_notes = notes;
                self.item_highlights = highlights;
                // Force re-resolve + optional persist against the current body.
                self.stale_persist_key = None;
                self.notes_error = None;
            }
            Err(e) => {
                self.item_notes.clear();
                self.item_highlights.clear();
                self.stale_persist_key = None;
                self.notes_error = Some(e);
            }
        }
    }

    fn reload_privilege_for_selection(&mut self, matter_root: &Utf8Path) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            self.clear_privilege_for_selection();
            return;
        };
        // Preserve dirty draft when same item (e.g. code apply refresh).
        let keep_dirty = self.privilege_draft.item_id.as_deref() == Some(item_id.as_str())
            && self.privilege_draft.dirty;
        let saved_draft = if keep_dirty {
            Some(self.privilege_draft.clone())
        } else {
            None
        };
        match load_privilege_panel(matter_root, &item_id) {
            Ok((row, cons)) => {
                self.item_privilege = row.clone();
                if let Some(d) = saved_draft {
                    self.privilege_draft = d;
                } else {
                    self.privilege_draft =
                        PrivilegePanelDraft::from_privilege(&item_id, row.as_ref());
                }
                self.privilege_family_banner = family_split_banner(&cons);
                self.privilege_error = None;
            }
            Err(e) => {
                self.item_privilege = None;
                self.privilege_family_banner = None;
                self.privilege_error = Some(e);
            }
        }
    }

    fn save_privilege_now(&mut self, matter_root: &Utf8Path, actor: &str) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        // Dangerous override: withhold=0 while not cleared → confirm first.
        if !self.privilege_draft.withhold
            && self.privilege_draft.status != "cleared"
            && !self.privilege_pending_save_no_withhold
        {
            self.privilege_confirm_clear_withhold = true;
            return;
        }
        self.privilege_pending_save_no_withhold = false;
        let input = self.privilege_draft.to_upsert_input(&item_id, actor);
        match upsert_privilege_blocking(matter_root, input) {
            Ok(row) => {
                self.item_privilege = Some(row.clone());
                self.privilege_draft = PrivilegePanelDraft::from_privilege(&item_id, Some(&row));
                self.privilege_status_msg = Some("Privilege claim saved.".into());
                self.privilege_error = None;
                self.privilege_force_open = true;
                // Refresh family banner.
                if let Ok((_, cons)) = load_privilege_panel(matter_root, &item_id) {
                    self.privilege_family_banner = family_split_banner(&cons);
                }
            }
            Err(e) => {
                self.privilege_error = Some(e);
                self.privilege_status_msg = None;
            }
        }
    }

    fn assert_privilege_now(&mut self, matter_root: &Utf8Path, actor: &str) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        let code_id = self
            .code_defs
            .iter()
            .find(|d| d.key == "privilege")
            .map(|d| d.id.clone());
        match assert_privilege_blocking(matter_root, &item_id, actor, code_id.as_deref()) {
            Ok(row) => {
                self.item_privilege = Some(row.clone());
                self.privilege_draft = PrivilegePanelDraft::from_privilege(&item_id, Some(&row));
                self.privilege_force_open = true;
                self.privilege_status_msg = Some("Privilege asserted.".into());
                self.privilege_error = None;
                self.refresh_row_codes(matter_root);
                if let Ok((_, cons)) = load_privilege_panel(matter_root, &item_id) {
                    self.privilege_family_banner = family_split_banner(&cons);
                }
            }
            Err(e) => {
                self.privilege_error = Some(e);
                self.privilege_status_msg = None;
            }
        }
    }

    fn poll_privilege_export(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.privilege_export_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(msg)) => {
                self.privilege_export_busy = false;
                self.privilege_export_rx = None;
                self.privilege_export_status = Some(msg);
                self.privilege_export_error = None;
                ctx.request_repaint();
            }
            Ok(Err(e)) => {
                self.privilege_export_busy = false;
                self.privilege_export_rx = None;
                self.privilege_export_error = Some(e);
                self.privilege_export_status = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.privilege_export_busy = false;
                self.privilege_export_rx = None;
                self.privilege_export_error =
                    Some("Privilege export worker ended unexpectedly.".into());
            }
        }
    }

    fn spawn_privilege_export(&mut self, matter_root: &Utf8Path, ctx: &egui::Context) {
        if self.privilege_export_busy {
            return;
        }
        let root = matter_root.to_owned();
        let scope_review = self.privilege_export_review_only;
        let default_path = default_privilege_log_path(matter_root);
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.privilege_export_rx = Some(rx);
        self.privilege_export_busy = true;
        self.privilege_export_status = Some("Exporting privilege log…".into());
        self.privilege_export_error = None;
        let _ = thread::Builder::new()
            .name("desk-privilege-export".into())
            .spawn(move || {
                // Prefer operator save dialog; fall back to default path under exports/.
                let path = rfd::FileDialog::new()
                    .set_file_name(default_path.file_name().unwrap_or("privilege_log.csv"))
                    .add_filter("CSV", &["csv"])
                    .save_file();
                let result = match path {
                    Some(p) => match Utf8PathBuf::from_path_buf(p) {
                        Ok(utf8) => {
                            export_privilege_log_blocking(&root, scope_review, &utf8).map(|r| {
                                format!(
                                    "Exported {} row(s) · blank desc {} · withheld {} → {}",
                                    r.row_count,
                                    r.blank_description_count,
                                    r.withheld_count,
                                    r.path
                                )
                            })
                        }
                        Err(_) => Err("Export path is not valid UTF-8.".into()),
                    },
                    None => Ok("Export cancelled.".into()),
                };
                let _ = tx.send(result);
                ctx.request_repaint();
            });
    }

    fn ensure_protocol_loaded(&mut self, matter_root: &Utf8Path) {
        if self.protocol_loaded_for.as_deref() == Some(matter_root) {
            return;
        }
        match load_protocol_blocking(matter_root) {
            Ok(p) => {
                self.protocol_draft_502d = p.fre_502d_note.unwrap_or_default();
                self.protocol_draft_502e = p.fre_502e_note.unwrap_or_default();
                self.protocol_description_required = p.description_required != 0;
                self.protocol_loaded_for = Some(matter_root.to_path_buf());
                self.protocol_status = None;
            }
            Err(e) => {
                self.protocol_status = Some(format!("Protocol load: {e}"));
            }
        }
    }

    fn save_protocol_now(&mut self, matter_root: &Utf8Path, actor: &str) {
        match upsert_protocol_blocking(
            matter_root,
            UpsertPrivilegeProtocolInput {
                log_format: "standard".into(),
                fre_502d_note: Some(self.protocol_draft_502d.clone()),
                fre_502e_note: Some(self.protocol_draft_502e.clone()),
                description_required: self.protocol_description_required,
                actor: actor.to_string(),
            },
        ) {
            Ok(_) => {
                self.protocol_status = Some("Privilege protocol saved.".into());
                self.protocol_loaded_for = Some(matter_root.to_path_buf());
            }
            Err(e) => {
                self.protocol_status = Some(format!("Protocol save failed: {e}"));
            }
        }
    }

    /// Ready display body for the current selection, if loaded.
    fn ready_body_for_item(&self, item_id: &str) -> Option<&str> {
        match self.body.pane() {
            BodyPane::Ready {
                item_id: bid,
                text: Ok(t),
                ..
            } if bid == item_id => Some(t.as_str()),
            _ => None,
        }
    }

    /// In-memory re-resolve against the ready body (None when body not ready).
    fn resolved_highlights_for_item(
        &self,
        item_id: &str,
        text_sha256: Option<&str>,
    ) -> Option<Vec<ResolvedHighlight>> {
        let body = self.ready_body_for_item(item_id)?;
        let digest = body_digest_for_item(text_sha256, body);
        Some(resolve_for_paint(&self.item_highlights, body, &digest))
    }

    /// Persist `status=stale` in SQLite once per (item, digest) when body is ready.
    /// UI still prefers in-memory resolve; this only aligns stored rows.
    fn maybe_persist_stale_resolves(
        &mut self,
        matter_root: &Utf8Path,
        item_id: &str,
        text_sha256: Option<&str>,
    ) {
        if self.item_highlights.is_empty() {
            return;
        }
        let Some(body) = self.ready_body_for_item(item_id).map(|s| s.to_string()) else {
            return;
        };
        let digest = body_digest_for_item(text_sha256, &body);
        let key = (item_id.to_string(), digest.clone());
        if self.stale_persist_key.as_ref() == Some(&key) {
            return;
        }
        match Matter::open(matter_root) {
            Ok(matter) => {
                match matter.resolve_highlights(item_id, &body, &digest, true) {
                    Ok(_resolved) => {
                        // Refresh stored rows so status columns match persist.
                        if let Ok(hls) = matter.list_highlights(item_id) {
                            self.item_highlights = hls;
                        }
                        self.stale_persist_key = Some(key);
                    }
                    Err(_) => {
                        // Non-fatal: banners still use in-memory resolve; avoid
                        // clobbering an operator-visible notes_error.
                        self.stale_persist_key = Some(key);
                    }
                }
            }
            Err(_) => {
                // Matter briefly unreadable — skip; try next frame / reload.
            }
        }
    }

    fn poll_notes(&mut self, matter_root: &Utf8Path, ctx: &egui::Context) {
        let Some(rx) = self.notes_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.notes_busy = false;
                self.notes_rx = None;
                if self.current_item_id() == Some(result.item_id.as_str()) {
                    self.item_notes = result.notes;
                    self.item_highlights = result.highlights;
                } else {
                    self.reload_notes_for_selection(matter_root);
                }
                self.notes_status = Some(result.message);
                self.notes_error = None;
                ctx.request_repaint();
            }
            Ok(Err(e)) => {
                self.notes_busy = false;
                self.notes_rx = None;
                self.notes_error = Some(e);
                self.notes_status = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.notes_busy = false;
                self.notes_rx = None;
                self.notes_error = Some("Notes worker ended unexpectedly.".into());
            }
        }
    }

    fn save_document_note(&mut self, matter_root: &Utf8Path, ctx: &egui::Context, actor: &str) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        let pending = self.pending_highlight_id.clone();
        let input =
            match note_upsert_from_draft(&item_id, &self.note_draft, pending.as_deref(), actor) {
                Ok(i) => i,
                Err(e) => {
                    self.notes_error = Some(e);
                    return;
                }
            };
        let is_passage = input.highlight_id.is_some();
        let ok = self.run_notes_mutate(matter_root, ctx, item_id, move |matter| {
            matter.upsert_note(input)?;
            Ok(if is_passage {
                "Passage note saved.".into()
            } else {
                "Document note saved.".into()
            })
        });
        // Keep draft + pending highlight binding on failure so the operator
        // does not lose work product text.
        if ok {
            self.note_draft.clear();
            self.pending_highlight_id = None;
            self.passage_note_hint.clear();
        }
    }

    fn save_note_edit(&mut self, matter_root: &Utf8Path, ctx: &egui::Context, actor: &str) {
        let Some(note_id) = self.note_edit_id.clone() else {
            return;
        };
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        if self.note_edit_body.trim().is_empty() {
            self.notes_error = Some("note body cannot be empty or whitespace-only".into());
            return;
        }
        let body = self.note_edit_body.clone();
        let actor = actor.to_string();
        let ok = self.run_notes_mutate(matter_root, ctx, item_id.clone(), move |matter| {
            matter.upsert_note(UpsertNoteInput {
                id: Some(note_id),
                item_id,
                body,
                // highlight_id ignored on update (matter-core); body-only edit.
                highlight_id: None,
                actor,
            })?;
            Ok("Note updated.".into())
        });
        if ok {
            self.note_edit_id = None;
            self.note_edit_body.clear();
        }
    }

    fn delete_note_ui(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        note_id: &str,
        actor: &str,
    ) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        let note_id = note_id.to_string();
        let actor = actor.to_string();
        self.run_notes_mutate(matter_root, ctx, item_id, move |matter| {
            matter.delete_note(&note_id, &actor)?;
            Ok("Note deleted.".into())
        });
    }

    fn create_highlight_from_selection(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        actor: &str,
        with_note: bool,
    ) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        let Some(sel) = self.body_selection else {
            self.notes_error = Some("Select text in the body first.".into());
            return;
        };

        let body = match self.body.pane() {
            BodyPane::Ready {
                item_id: bid,
                text: Ok(t),
                ..
            } if bid == &item_id => t.clone(),
            _ => {
                self.notes_error = Some("Body not ready.".into());
                return;
            }
        };
        let text_sha = self
            .selection
            .and_then(|i| self.rows.get(i))
            .and_then(|r| r.text_sha256.clone());
        let digest = body_digest_for_item(text_sha.as_deref(), &body);
        // Reuse only active-after-resolve highlights (not stale offset collisions).
        let resolved = resolve_for_paint(&self.item_highlights, &body, &digest);

        // Passage-note path: bind draft to an existing matching active highlight
        // when present (no second create), else create highlight first — never
        // auto-persist synthetic "Note on: …" body text.
        if with_note {
            if let Some(existing) =
                find_highlight_for_selection(&self.item_highlights, &resolved, sel)
            {
                let hid = existing.id.clone();
                let hint = passage_note_hint_from_quote(&existing.exact_quote);
                self.pending_highlight_id = Some(hid);
                self.passage_note_hint = hint;
                self.notes_status =
                    Some("Type a passage note and Save (linked to selection highlight).".into());
                self.notes_error = None;
                return;
            }
        }

        let input = match highlight_input_from_selection(&item_id, &body, &digest, sel, actor, None)
        {
            Ok(i) => i,
            Err(e) => {
                self.notes_error = Some(e);
                return;
            }
        };
        let quote_for_hint = input.exact_quote.clone();
        let ok = self.run_notes_mutate(matter_root, ctx, item_id, move |matter| {
            let _hl = matter.create_highlight(input)?;
            Ok("Highlight created.".into())
        });
        if ok && with_note {
            // Bind draft to the just-created (or matching) highlight — user must type body.
            // Re-resolve after reload so we only attach to active resolved ranges.
            let resolved_after = resolve_for_paint(&self.item_highlights, &body, &digest);
            if let Some(hl) =
                find_highlight_for_selection(&self.item_highlights, &resolved_after, sel)
            {
                self.pending_highlight_id = Some(hl.id.clone());
                self.passage_note_hint = passage_note_hint_from_quote(&quote_for_hint);
                self.notes_status = Some(
                    "Highlight created — type a passage note and Save (not auto-saved).".into(),
                );
            } else {
                self.passage_note_hint = passage_note_hint_from_quote(&quote_for_hint);
                self.notes_status = Some(
                    "Highlight created — type a passage note after reload if binding missing."
                        .into(),
                );
            }
        }
    }

    fn delete_highlight_ui(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        highlight_id: &str,
        actor: &str,
    ) {
        let Some(item_id) = self.current_item_id().map(|s| s.to_string()) else {
            return;
        };
        let highlight_id = highlight_id.to_string();
        let actor = actor.to_string();
        self.run_notes_mutate(matter_root, ctx, item_id, move |matter| {
            matter.delete_highlight(&highlight_id, &actor)?;
            Ok("Highlight deleted (linked notes unlinked).".into())
        });
    }

    /// Run a notes/highlights mutation. Returns `true` only when the write
    /// succeeded (reload of lists may still surface a secondary error).
    fn run_notes_mutate<F>(
        &mut self,
        matter_root: &Utf8Path,
        ctx: &egui::Context,
        item_id: String,
        f: F,
    ) -> bool
    where
        F: FnOnce(&Matter) -> Result<String, matter_core::Error> + Send + 'static,
    {
        if self.notes_busy {
            return false;
        }
        // Sync path for small single writes (notes/highlights are cheap).
        let root = matter_root.to_owned();
        let write_ok = match Matter::open(&root) {
            Ok(matter) => match f(&matter) {
                Ok(message) => {
                    match load_notes_highlights(&root, &item_id) {
                        Ok((notes, highlights)) => {
                            self.item_notes = notes;
                            self.item_highlights = highlights;
                            self.notes_status = Some(message);
                            self.notes_error = None;
                        }
                        Err(e) => {
                            // Write already committed; keep success so drafts clear.
                            self.notes_status = Some(message);
                            self.notes_error = Some(e);
                        }
                    }
                    true
                }
                Err(e) => {
                    self.notes_error = Some(e.to_string());
                    self.notes_status = None;
                    false
                }
            },
            Err(e) => {
                self.notes_error = Some(e.to_string());
                false
            }
        };
        let _ = ctx; // reserved for future off-thread path + request_repaint
        write_ok
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
            if self.item_notes.is_empty() && self.item_highlights.is_empty() {
                self.reload_notes_for_selection(matter_root);
            }
            if self.privilege_draft.item_id.as_deref() != self.current_item_id() {
                self.reload_privilege_for_selection(matter_root);
            }
            return;
        }
        self.selection = Some(idx);
        if let Some(row) = self.rows.get(idx) {
            self.last_item_id = Some(row.id.clone());
        }
        self.selection_detail = None;
        self.load_selection_detail(matter_root);
        // Keep header chips correct even when selection is off the visible page.
        self.refresh_row_codes(matter_root);
        self.clear_notes_for_selection();
        self.reload_notes_for_selection(matter_root);
        self.clear_privilege_for_selection();
        self.reload_privilege_for_selection(matter_root);
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
        // Large corpus: load first page only; UI offers Load more.
        matter
            .list_review_thin(None, THIN_PAGE_SIZE, 0)
            .map_err(|e| e.to_string())?
    };
    Ok((count, rows))
}

/// Load a page of the unfiltered review corpus.
pub fn load_review_page(
    matter_root: &Utf8Path,
    offset: u64,
    limit: u64,
) -> Result<(u64, Vec<ReviewListRow>), String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let count = matter.count_in_review(None).map_err(|e| e.to_string())?;
    let rows = matter
        .list_review_thin(None, limit, offset)
        .map_err(|e| e.to_string())?;
    Ok((count, rows))
}

/// Load filtered count + thin rows. Returns `(count, rows, has_more)`.
///
/// When `limit_override` is `None`, uses full load if count ≤ threshold else first page.
pub fn load_review_filtered(
    matter_root: &Utf8Path,
    spec: &FilterSpec,
    offset: u64,
    limit_override: Option<u64>,
) -> Result<(u64, Vec<ReviewListRow>, bool), String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let count = matter
        .count_items_filtered(spec)
        .map_err(|e| e.to_string())?;
    if count == 0 {
        return Ok((0, Vec::new(), false));
    }
    let limit = if let Some(l) = limit_override {
        l
    } else if offset == 0 && count <= THIN_LOAD_ALL_THRESHOLD {
        count
    } else {
        THIN_PAGE_SIZE
    };
    let rows = matter
        .list_items_filtered_thin(spec, limit, offset)
        .map_err(|e| e.to_string())?;
    let loaded = rows.len() as u64;
    let has_more = offset + loaded < count;
    Ok((count, rows, has_more))
}

/// Load list composing optional keyword FTS with metadata filter.
///
/// Returns `(count, rows, has_more, fts_hit_count_approx)`.
///
/// Performs **one** Tantivy open/search, then intersects hits with FilterSpec
/// (avoids triple index open on Apply/reload).
pub fn load_review_composed(
    matter_root: &Utf8Path,
    keyword: Option<&str>,
    spec: &FilterSpec,
    offset: u64,
    limit_override: Option<u64>,
) -> Result<(u64, Vec<ReviewListRow>, bool, Option<u64>), String> {
    let Some(kw) = keyword.map(str::trim).filter(|s| !s.is_empty()) else {
        let (count, rows, has_more) =
            load_review_filtered(matter_root, spec, offset, limit_override)?;
        return Ok((count, rows, has_more, None));
    };

    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;

    // Single FTS open: hit count for status + id set for filter intersection.
    let hits = matter_search::search_keyword(
        matter_root,
        &matter_search::KeywordQuery {
            query: kw.to_string(),
            limit: matter_search::DEFAULT_FTS_FETCH_LIMIT,
            offset: 0,
        },
    )
    .map_err(|e| e.to_string())?;
    let fts_hits = hits.item_ids.len() as u64;
    if hits.item_ids.is_empty() {
        return Ok((0, Vec::new(), false, Some(0)));
    }

    // Count first (cheap TEMP join) so we can choose load-all vs page size.
    let count = matter
        .count_items_filtered_in_ids(spec, &hits.item_ids)
        .map_err(|e| e.to_string())?;
    if count == 0 {
        return Ok((0, Vec::new(), false, Some(fts_hits)));
    }

    let limit = if let Some(l) = limit_override {
        l
    } else if offset == 0 && count <= THIN_LOAD_ALL_THRESHOLD {
        count
    } else {
        THIN_PAGE_SIZE
    };

    let rows = matter
        .list_items_filtered_thin_in_ids(spec, &hits.item_ids, limit, offset)
        .map_err(|e| e.to_string())?;

    let loaded = rows.len() as u64;
    let has_more = offset + loaded < count;
    Ok((count, rows, has_more, Some(fts_hits)))
}

/// List saved searches for the matter.
pub fn load_saved_searches(matter_root: &Utf8Path) -> Result<Vec<SavedSearch>, String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    matter.list_saved_searches().map_err(|e| e.to_string())
}

/// Upsert a saved search (writer open).
pub fn upsert_saved_search(
    matter_root: &Utf8Path,
    input: SavedSearchInput,
) -> Result<SavedSearch, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter.upsert_saved_search(input).map_err(|e| e.to_string())
}

/// Delete a saved search (writer open).
pub fn delete_saved_search(matter_root: &Utf8Path, search_id: &str) -> Result<(), String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter
        .delete_saved_search(search_id)
        .map_err(|e| e.to_string())
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
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    matter.list_item_codes(item_ids).map_err(|e| e.to_string())
}

/// Item ids whose codes should be queried: viewport slice + current selection.
///
/// When `visible` is empty (not yet painted), loads a small leading window so the
/// first frame has list chips without scanning the full thin corpus (up to 50k).
pub fn item_ids_for_code_load(
    rows: &[ReviewListRow],
    visible: &std::ops::Range<usize>,
    selection_id: Option<&str>,
) -> Vec<String> {
    const FALLBACK_WINDOW: usize = 64;
    let n = rows.len();
    if n == 0 {
        return Vec::new();
    }
    let (start, end) = if visible.start >= visible.end {
        (0, n.min(FALLBACK_WINDOW))
    } else {
        let start = visible.start.min(n);
        let end = visible.end.min(n).max(start);
        (start, end)
    };
    let mut ids: Vec<String> = rows[start..end].iter().map(|r| r.id.clone()).collect();
    if let Some(sel) = selection_id {
        if !ids.iter().any(|id| id == sel) {
            ids.push(sel.to_string());
        }
    }
    ids
}

/// Whether `apply_codes` should run off the UI thread.
///
/// Off-thread for multi-item batch, any family-propagate apply, or N above threshold.
pub fn should_apply_codes_off_thread(
    selected_count: usize,
    propagate_family: bool,
    threshold: usize,
) -> bool {
    selected_count > 1 || propagate_family || selected_count > threshold
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

/// Insert a custom code definition (label → slug key). Returns new definition id.
fn load_notes_highlights(
    matter_root: &Utf8Path,
    item_id: &str,
) -> Result<(Vec<ItemNote>, Vec<ItemHighlight>), String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    let notes = matter.list_notes(item_id).map_err(|e| e.to_string())?;
    let highlights = matter.list_highlights(item_id).map_err(|e| e.to_string())?;
    Ok((notes, highlights))
}

pub fn upsert_code_definition_blocking(
    matter_root: &Utf8Path,
    label: &str,
    group_key: &str,
) -> Result<String, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter.seed_default_codes().map_err(|e| e.to_string())?;
    // Place new codes after seed defaults (sort_order 0..5).
    let sort_order = 100i64;
    matter
        .upsert_code_definition(CodeDefInput {
            id: None,
            key: None,
            label: label.to_string(),
            group_key: group_key.to_string(),
            cardinality: "multi".into(),
            color: None,
            sort_order,
            is_active: true,
        })
        .map_err(|e| e.to_string())
}

/// Toggle membership of `item_id` in a multi-select set (pure helper for tests).
pub fn toggle_selection_set(selected: &mut HashSet<String>, item_id: &str) {
    if !selected.remove(item_id) {
        selected.insert(item_id.to_string());
    }
}

/// Select a code into the batch-selected set for **Add** mode.
///
/// When `cardinality` is `single`, removes any other selected batch codes that
/// share the same `group_key` (last click wins — deterministic UX).
///
/// `defs` is the active catalog used to map selected ids → group membership.
pub fn select_batch_code_for_add(
    selected: &mut HashSet<String>,
    code_id: &str,
    group_key: &str,
    cardinality: &str,
    defs: &[CodeDef],
) {
    if cardinality == "single" {
        for def in defs {
            if def.group_key == group_key && def.id != code_id {
                selected.remove(&def.id);
            }
        }
    }
    selected.insert(code_id.to_string());
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

/// Operator request from the Review keyword bar (FTS index jobs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtsUiRequest {
    /// Incremental build/update (`reset: false`).
    UpdateIndex,
    /// Full rebuild (`reset: true`).
    RebuildIndex,
}

/// Paint the Review screen.
///
/// When the operator clicks index buttons, `fts_request` is set for the app to
/// start `fts_index` on the process-runner worker.
///
/// `index_job_busy`: when true (runner writing / FTS rebuild), skip Tantivy
/// open on the UI thread and disable Search / Update / Rebuild to avoid
/// Windows mmap races with `remove_dir_all(index/)`.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    actor: &str,
    fts_request: &mut Option<FtsUiRequest>,
    index_job_busy: bool,
) {
    let ctx = ui.ctx().clone();

    // Avoid mmap'ing index/ while the worker may be deleting it.
    if !index_job_busy {
        state.ensure_loaded(matter_root);
    }
    state.body.try_take();
    state.poll_coding(matter_root, &ctx);
    state.poll_notes(matter_root, &ctx);

    // Kick body load when we have a selection but body is idle (first paint after reload).
    if state.selection.is_some() && matches!(state.body.pane(), BodyPane::Idle) {
        state.spawn_body_for_selection(&ctx, matter_root);
    }

    ui.horizontal(|ui| {
        ui.heading("Review");
        let kw_on = state
            .applied_keyword
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if kw_on && state.filter_active {
            ui.label("— Keyword + metadata filter");
        } else if kw_on {
            ui.label("— Keyword search");
        } else if state.filter_active {
            ui.label("— Filtered subset (metadata)");
        } else {
            ui.label("— Review Corpus (in_review)");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !state.privilege_export_busy,
                    egui::Button::new("Export privilege log…"),
                )
                .on_hover_text("CSV privilege log (FRCP 26(b)(5) style columns)")
                .clicked()
            {
                state.spawn_privilege_export(matter_root, &ctx);
            }
            ui.checkbox(&mut state.privilege_export_review_only, "Review only")
                .on_hover_text("When checked, export review corpus only; else entire matter");
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
    if let Some(st) = state.privilege_export_status.clone() {
        ui.label(
            RichText::new(st)
                .small()
                .color(Color32::from_rgb(40, 120, 60)),
        );
    }
    if let Some(err) = state.privilege_export_error.clone() {
        ui.colored_label(Color32::from_rgb(200, 60, 60), err);
    }
    ui.add_space(4.0);

    // Keyword + filter bar always available — including when list_error is set.
    show_keyword_bar(ui, state, matter_root, fts_request, index_job_busy);
    ui.add_space(2.0);
    show_filter_bar(ui, state, matter_root, actor);
    ui.add_space(4.0);
    show_privilege_protocol_strip(ui, state, matter_root, actor);
    ui.add_space(2.0);

    if let Some(err) = state.list_error.clone() {
        ui.colored_label(Color32::from_rgb(200, 60, 60), format!("List error: {err}"));
        ui.label(
            RichText::new("Use Clear or adjust the filter above, then Apply / Refresh.")
                .weak()
                .small(),
        );
        // Fall through: still show empty/list body when rows remain.
    }

    if state.rows.is_empty() {
        ui.add_space(8.0);
        if state.list_error.is_some() {
            // List failed and no rows to show; filter bar already rendered.
            return;
        }
        if state.filter_active {
            ui.label(RichText::new("No items match the current filter.").strong());
            ui.label("Adjust conditions or Clear to restore the full Review Corpus.");
        } else {
            ui.label(
                RichText::new("No items in review. Run Promote to review on Workspace.").strong(),
            );
            ui.label("Promote builds the Review Corpus (`in_review` + `review_order`).");
        }
        return;
    }

    // Keyboard: only when no widget has focus (egui 0.34: focused()).
    // Filter / keyword / note / privilege TextEdit steal focus — digit shortcuts must not fire.
    // Focus flags are from the previous frame's TextEdits.
    let no_focus = ctx.memory(|m| m.focused().is_none());
    let note_focus = state.note_editor_focused;
    let priv_focus = state.privilege_editor_focused;
    state.note_editor_focused = false;
    state.privilege_editor_focused = false;
    state.poll_privilege_export(&ctx);
    if review_nav::focus_allows_shortcuts(no_focus)
        && focus_allows_coding_with_privilege(no_focus, note_focus, priv_focus)
    {
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
            let label = if state.filter_active {
                format!("(showing {n_shown} of {n_total} filtered)")
            } else {
                format!("(showing {n_shown} of {n_total} in corpus)")
            };
            ui.label(label);
            if ui.small_button("Load more").clicked() {
                state.load_more(matter_root);
            }
        } else if state.filter_active {
            ui.label(format!("({n_total} match filter)"));
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
                    let mut painted_range: Option<std::ops::Range<usize>> = None;
                    egui::ScrollArea::vertical()
                        .id_salt("review_corpus_list")
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_HEIGHT, state.rows.len(), |ui, row_range| {
                            painted_range = Some(row_range.clone());
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
                    if let Some(range) = painted_range {
                        state.note_visible_row_range(matter_root, range);
                    }
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

/// Keyword search bar (0029) + index actions.
fn show_keyword_bar(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    fts_request: &mut Option<FtsUiRequest>,
    index_job_busy: bool,
) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Keyword").strong());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.keyword_draft)
                    .desired_width(280.0)
                    .hint_text("Boolean / phrase…"),
            );
            if !index_job_busy && resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                state.apply_keyword(matter_root);
            }
            if ui
                .add_enabled(!index_job_busy, egui::Button::new("Search"))
                .on_hover_text(if index_job_busy {
                    "Wait for the index job to finish"
                } else {
                    "Run keyword search (intersects metadata filter)"
                })
                .clicked()
            {
                state.apply_keyword(matter_root);
            }
            if ui
                .add_enabled(!index_job_busy, egui::Button::new("Clear keyword"))
                .clicked()
            {
                state.clear_keyword(matter_root);
            }
            ui.separator();
            if ui
                .add_enabled(!index_job_busy, egui::Button::new("Update index").small())
                .on_hover_text("Incremental FTS index (reset:false)")
                .clicked()
            {
                *fts_request = Some(FtsUiRequest::UpdateIndex);
            }
            if ui
                .add_enabled(!index_job_busy, egui::Button::new("Rebuild index").small())
                .on_hover_text("Full rebuild after dropping index handles (reset:true)")
                .clicked()
            {
                *fts_request = Some(FtsUiRequest::RebuildIndex);
            }
        });
        if index_job_busy {
            ui.label(
                RichText::new("Index job running — keyword search paused until complete.")
                    .small()
                    .color(Color32::from_rgb(180, 100, 20)),
            );
        }

        if let Some(hits) = state.keyword_hit_count {
            ui.label(
                RichText::new(format!(
                    "{hits} keyword hits · {} after filters",
                    state.count
                ))
                .small()
                .color(Color32::from_rgb(40, 80, 140)),
            );
        } else if state
            .applied_keyword
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            ui.label(
                RichText::new(format!("{} after filters", state.count))
                    .small()
                    .color(Color32::from_rgb(40, 80, 140)),
            );
        }

        if state.index_outdated {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(180, 100, 20),
                    "Search index outdated — Update index",
                );
            });
        }
        if let Some(err) = state.keyword_error.clone() {
            ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Keyword: {err}"));
        }
    });
}

/// Filter bar: draft fields, quick chips, Apply/Clear, saved search CRUD.
fn show_filter_bar(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    actor: &str,
) {
    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Filter").strong());
            ui.label(
                RichText::new("(metadata; intersects keyword when active)")
                    .weak()
                    .small(),
            );
        });

        // Quick chips
        ui.horizontal_wrapped(|ui| {
            ui.label("Quick:");
            if ui.small_button("Uncoded").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_uncoded());
            }
            if ui.small_button("Privilege").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_privilege());
            }
            if ui.small_button("Responsive").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_responsive());
            }
            if ui.small_button("Has notes").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_has_notes());
            }
            if ui.small_button("Has highlights").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_has_highlights());
            }
            if ui.small_button("Withheld").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_withheld());
            }
            if ui.small_button("Privilege log incomplete").clicked() {
                state.apply_preset(matter_root, FilterSpec::preset_privilege_log_incomplete());
            }
            if state.filter_draft.code_missing {
                ui.label(
                    RichText::new("active: Uncoded")
                        .small()
                        .color(Color32::from_rgb(40, 120, 60)),
                );
            }
            if state.filter_draft.has_notes {
                ui.label(
                    RichText::new("active: Has notes")
                        .small()
                        .color(Color32::from_rgb(40, 120, 60)),
                );
            }
        });

        ui.horizontal(|ui| {
            ui.label("Custodian:");
            ui.add(
                egui::TextEdit::singleline(&mut state.filter_draft.custodian)
                    .desired_width(160.0)
                    .hint_text("contains…"),
            );
            ui.label("Date from:");
            ui.add(
                egui::TextEdit::singleline(&mut state.filter_draft.date_from)
                    .desired_width(170.0)
                    .hint_text("RFC3339+offset"),
            );
            ui.label("to:");
            ui.add(
                egui::TextEdit::singleline(&mut state.filter_draft.date_to)
                    .desired_width(170.0)
                    .hint_text("exclusive end"),
            );
            ui.checkbox(&mut state.filter_draft.include_family, "Include family");
        });

        ui.horizontal(|ui| {
            ui.label("Note text:");
            ui.add(
                egui::TextEdit::singleline(&mut state.filter_draft.note_text)
                    .desired_width(220.0)
                    .hint_text("Note text contains…"),
            );
            if !state.filter_draft.note_text.trim().is_empty() {
                ui.label(
                    RichText::new("active: note text")
                        .small()
                        .color(Color32::from_rgb(40, 120, 60)),
                );
            }
        });

        // Codes multi-select (active defs)
        ui.horizontal_wrapped(|ui| {
            ui.label("Codes:");
            let active: Vec<CodeDef> = state
                .code_defs
                .iter()
                .filter(|d| d.is_active != 0)
                .cloned()
                .collect();
            if active.is_empty() {
                ui.label(RichText::new("(catalog empty)").weak().small());
            } else {
                for def in &active {
                    let mut on = state.filter_draft.code_keys.contains(&def.key);
                    if ui.checkbox(&mut on, &def.label).changed() {
                        if on {
                            state.filter_draft.code_keys.insert(def.key.clone());
                            // Code any_of and uncoded are mutually exclusive in draft.
                            state.filter_draft.code_missing = false;
                        } else {
                            state.filter_draft.code_keys.remove(&def.key);
                        }
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            if ui.button("Apply filter").clicked() {
                state.apply_filter(matter_root);
            }
            if ui.button("Clear").clicked() {
                state.clear_filter(matter_root);
            }
            ui.separator();
            ui.label("Saved:");
            let selected_label = state
                .filter_draft
                .selected_saved_id
                .as_ref()
                .and_then(|id| {
                    state
                        .saved_searches
                        .iter()
                        .find(|s| &s.id == id)
                        .map(|s| s.name.clone())
                })
                .unwrap_or_else(|| "(none)".into());
            egui::ComboBox::from_id_salt("review_saved_search")
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(state.filter_draft.selected_saved_id.is_none(), "(none)")
                        .clicked()
                    {
                        state.filter_draft.selected_saved_id = None;
                    }
                    for ss in state.saved_searches.clone() {
                        let selected =
                            state.filter_draft.selected_saved_id.as_deref() == Some(&ss.id);
                        if ui.selectable_label(selected, &ss.name).clicked() {
                            state.filter_draft.selected_saved_id = Some(ss.id);
                        }
                    }
                });
            if ui.button("Load").clicked() {
                state.load_selected_saved_search(matter_root);
            }
            if ui.button("Delete").clicked() {
                state.delete_selected_saved_search(matter_root);
            }
            ui.separator();
            ui.label("Save as:");
            ui.add(
                egui::TextEdit::singleline(&mut state.filter_draft.save_name)
                    .desired_width(120.0)
                    .hint_text("name"),
            );
            if ui.button("Save").clicked() {
                state.save_current_filter(matter_root, actor);
            }
        });

        if let Some(st) = state.filter_status.clone() {
            ui.label(
                RichText::new(st)
                    .small()
                    .color(Color32::from_rgb(40, 120, 60)),
            );
        }
        if let Some(err) = state.filter_error.clone() {
            ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Filter: {err}"));
        }
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

        // Privilege claim panel (0031)
        show_privilege_panel(ui, state, matter_root, actor, &row.id);

        // Align DB stale flags once body is available (cheap; once per item+digest).
        state.maybe_persist_stale_resolves(matter_root, &row.id, row.text_sha256.as_deref());

        // In-memory re-resolve drives banners / labels / paint (not raw DB alone).
        let resolved_for_ui =
            state.resolved_highlights_for_item(&row.id, row.text_sha256.as_deref());
        let resolved_slice = resolved_for_ui.as_deref();

        // Notes / highlights header counts
        let n_notes = state.item_notes.len();
        let n_hl = state.item_highlights.len();
        let n_stale = stale_count_for_ui(&state.item_highlights, resolved_slice);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("📝 {n_notes} notes · {n_hl} highlights"))
                    .strong()
                    .small(),
            );
            if n_stale > 0 {
                ui.colored_label(
                    Color32::from_rgb(180, 120, 40),
                    format!("{n_stale} stale highlight(s) — re-resolve failed"),
                );
            }
        });
        if let Some(st) = state.notes_status.clone() {
            ui.label(
                RichText::new(st)
                    .small()
                    .color(Color32::from_rgb(40, 120, 60)),
            );
        }
        if let Some(err) = state.notes_error.clone() {
            ui.colored_label(Color32::from_rgb(200, 60, 60), err);
        }

        ui.separator();

        // Body (selectable + highlight paint)
        let body_height = (ui.available_height() - 160.0).max(100.0);
        // Clone pane data so we can mutably borrow `state` for selection/paint.
        let body_view = match state.body.pane() {
            BodyPane::Idle => BodyView::Idle,
            BodyPane::Loading { .. } => BodyView::Loading,
            BodyPane::Ready {
                text,
                truncated,
                item_id,
                ..
            } => {
                if item_id != &row.id {
                    BodyView::Loading
                } else {
                    match text {
                        Ok(s) => BodyView::Ready {
                            text: s.clone(),
                            truncated: *truncated,
                        },
                        Err(e) => BodyView::Error(e.clone()),
                    }
                }
            }
        };
        egui::ScrollArea::vertical()
            .id_salt("review_body_scroll")
            .max_height(body_height * 0.62)
            .auto_shrink([false, false])
            .show(ui, |ui| match body_view {
                BodyView::Idle => {
                    ui.label("…");
                }
                BodyView::Loading => {
                    ui.label("Loading…");
                }
                BodyView::Ready { text, truncated } => {
                    if truncated {
                        ui.colored_label(
                            Color32::from_rgb(180, 120, 40),
                            "Body truncated for display (2 MiB cap). Full text remains in CAS.",
                        );
                    }
                    if text.is_empty() {
                        ui.label(
                            RichText::new("No extracted text")
                                .italics()
                                .color(Color32::GRAY),
                        );
                    } else {
                        show_selectable_body(
                            ui,
                            state,
                            &row,
                            &text,
                            resolved_for_ui.as_deref().unwrap_or(&[]),
                        );
                    }
                }
                BodyView::Error(e) if e.contains("No extracted text") => {
                    ui.label(
                        RichText::new("No extracted text")
                            .italics()
                            .color(Color32::GRAY),
                    );
                }
                BodyView::Error(e) => {
                    ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Body error: {e}"));
                }
            });

        // Selection actions
        ui.horizontal(|ui| {
            let has_sel = state.body_selection.map(|s| !s.is_empty()).unwrap_or(false);
            if ui
                .add_enabled(
                    has_sel && !state.notes_busy,
                    egui::Button::new("Highlight").small(),
                )
                .on_hover_text("Create yellow stand-off highlight on selection")
                .clicked()
            {
                state.create_highlight_from_selection(matter_root, ctx, actor, false);
            }
            if ui
                .add_enabled(
                    has_sel && !state.notes_busy,
                    egui::Button::new("Note on selection").small(),
                )
                .on_hover_text(
                    "Create highlight (if needed) and open a passage-note draft — type text, then Save",
                )
                .clicked()
            {
                state.create_highlight_from_selection(matter_root, ctx, actor, true);
            }
            if let Some(sel) = state.body_selection {
                ui.label(
                    RichText::new(format!("sel chars {}..{}", sel.start, sel.end))
                        .small()
                        .weak(),
                );
            }
        });

        ui.separator();

        // Notes panel (banner + labels use same resolved list as paint).
        show_notes_panel(
            ui,
            state,
            matter_root,
            ctx,
            actor,
            resolved_for_ui.as_deref(),
        );

        ui.separator();

        // Family / attachment strip
        show_family_strip(ui, state, &row, matter_root, ctx);
    });
}

enum BodyView {
    Idle,
    Loading,
    Ready { text: String, truncated: bool },
    Error(String),
}

fn show_selectable_body(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    row: &ReviewListRow,
    body: &str,
    resolved: &[ResolvedHighlight],
) {
    // Sync edit buffer when selection/body changes.
    if state.body_edit_item_id.as_deref() != Some(row.id.as_str()) || state.body_edit_buf != body {
        // Only force-resync when item changed or buffer drifted from edits.
        if state.body_edit_item_id.as_deref() != Some(row.id.as_str()) {
            state.body_edit_buf = body.to_string();
            state.body_edit_item_id = Some(row.id.clone());
        } else if state.body_edit_buf != body {
            // Prefer loaded body over accidental edits.
            state.body_edit_buf = body.to_string();
        }
    }

    let wrap_width = ui.available_width();
    let job = body_job_for_ui(body, resolved, wrap_width);

    // Paint highlighted body (read-only layout job).
    // Dual widget residual (egui 0.34): Label paints ranges; TextEdit below captures
    // selection. Unifying paint+cursor on one widget is deferred — see desk README.
    ui.add(egui::Label::new(job).wrap().selectable(true));

    // Selection capture via a second pass TextEdit (same text) — frame-less, used for cursor range.
    // Keep buffer in sync; reject mutations so CAS display is never rewritten here.
    let mut buf = state.body_edit_buf.clone();
    let output = egui::TextEdit::multiline(&mut buf)
        .id_salt("review_body_select")
        .font(egui::TextStyle::Monospace)
        .desired_width(ui.available_width())
        .desired_rows(6)
        .hint_text("Select text here to create a highlight…")
        .show(ui);
    if buf != body {
        // Discard edits — body is CAS-backed work product display only.
        state.body_edit_buf = body.to_string();
    } else {
        state.body_edit_buf = buf;
    }
    if let Some(range) = output.cursor_range {
        let r = range.as_sorted_char_range();
        state.body_selection = selection_from_char_range(r);
    }
    if output.response.gained_focus() || output.response.has_focus() {
        // Body select TextEdit also steals digit shortcuts (covered by focused()).
    }
}

fn show_notes_panel(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    ctx: &egui::Context,
    actor: &str,
    resolved: Option<&[ResolvedHighlight]>,
) {
    ui.label(RichText::new("Notes").strong().small());
    ui.label(
        RichText::new(
            "Work product — stored in matter DB only; not produced with load files by default.",
        )
        .weak()
        .small(),
    );

    // Stale banner — from in-memory re-resolve when body is ready.
    let n_stale = stale_count_for_ui(&state.item_highlights, resolved);
    if n_stale > 0 {
        ui.colored_label(
            Color32::from_rgb(180, 120, 40),
            format!("⚠ {n_stale} highlight(s) are stale (body changed; quote not re-found)."),
        );
    }

    // Add document note, or passage note when pending_highlight_id is set.
    let is_passage_draft = state.pending_highlight_id.is_some();
    ui.horizontal(|ui| {
        if is_passage_draft {
            ui.label("New passage note:");
            if let Some(hid) = state.pending_highlight_id.as_deref() {
                ui.label(
                    RichText::new(format!("🔗 {hid}"))
                        .small()
                        .color(Color32::from_rgb(80, 100, 160)),
                );
            }
        } else {
            ui.label("New document note:");
        }
    });
    let hint: String = if is_passage_draft && !state.passage_note_hint.is_empty() {
        state.passage_note_hint.clone()
    } else if is_passage_draft {
        "Type a passage note linked to the highlight…".into()
    } else {
        "Type a document note…".into()
    };
    let draft_resp = ui.add(
        egui::TextEdit::multiline(&mut state.note_draft)
            .id_salt("note_draft")
            .desired_width(ui.available_width())
            .desired_rows(2)
            .hint_text(hint),
    );
    if draft_resp.has_focus() {
        state.note_editor_focused = true;
    }
    ui.horizontal(|ui| {
        let can_save = !state.note_draft.trim().is_empty() && !state.notes_busy;
        let save_label = if is_passage_draft {
            "Save passage note"
        } else {
            "Save note"
        };
        if ui
            .add_enabled(can_save, egui::Button::new(save_label).small())
            .clicked()
        {
            state.save_document_note(matter_root, ctx, actor);
        }
        if is_passage_draft
            && ui
                .small_button("Cancel passage")
                .on_hover_text("Keep the highlight; discard passage-note draft binding")
                .clicked()
        {
            state.pending_highlight_id = None;
            state.passage_note_hint.clear();
            // Keep note_draft text so the operator can still save as a document note.
            state.notes_status = Some("Passage binding cleared (highlight kept).".into());
        }
    });

    // List notes newest first (API order).
    egui::ScrollArea::vertical()
        .id_salt("notes_list_scroll")
        .max_height(140.0)
        .show(ui, |ui| {
            if state.item_notes.is_empty() {
                ui.label(RichText::new("(no notes)").weak().small());
                return;
            }
            let notes: Vec<ItemNote> = state.item_notes.clone();
            for note in notes {
                ui.group(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&note.updated_at).small().weak());
                        ui.label(
                            RichText::new(format!("by {}", note.updated_by))
                                .small()
                                .weak(),
                        );
                        if let Some(hid) = note.highlight_id.as_deref() {
                            ui.label(
                                RichText::new(format!("🔗 passage {hid}"))
                                    .small()
                                    .color(Color32::from_rgb(80, 100, 160)),
                            );
                        } else {
                            ui.label(RichText::new("document").small().weak());
                        }
                    });
                    if state.note_edit_id.as_deref() == Some(note.id.as_str()) {
                        let edit_resp = ui.add(
                            egui::TextEdit::multiline(&mut state.note_edit_body)
                                .id_salt(format!("note_edit_{}", note.id))
                                .desired_width(ui.available_width())
                                .desired_rows(2),
                        );
                        if edit_resp.has_focus() {
                            state.note_editor_focused = true;
                        }
                        ui.horizontal(|ui| {
                            let can_edit_save =
                                !state.note_edit_body.trim().is_empty() && !state.notes_busy;
                            if ui
                                .add_enabled(can_edit_save, egui::Button::new("Save").small())
                                .clicked()
                            {
                                state.save_note_edit(matter_root, ctx, actor);
                            }
                            if ui.small_button("Cancel").clicked() {
                                state.note_edit_id = None;
                                state.note_edit_body.clear();
                            }
                        });
                    } else {
                        ui.label(RichText::new(&note.body).small());
                        ui.horizontal(|ui| {
                            if ui.small_button("Edit").clicked() {
                                state.note_edit_id = Some(note.id.clone());
                                state.note_edit_body = note.body.clone();
                            }
                            if ui
                                .add_enabled(!state.notes_busy, egui::Button::new("Delete").small())
                                .clicked()
                            {
                                state.delete_note_ui(matter_root, ctx, &note.id, actor);
                            }
                        });
                    }
                });
            }
        });

    // Highlights list (compact) — status labels from re-resolve when available.
    if !state.item_highlights.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Highlights").strong().small());
        let hls: Vec<ItemHighlight> = state.item_highlights.clone();
        for hl in hls {
            ui.horizontal_wrapped(|ui| {
                let res = resolved.and_then(|r| find_resolved(r, &hl.id));
                let status_raw = highlight_ui_status(&hl, res);
                let status = if status_raw == "stale" {
                    "⚠ stale"
                } else {
                    "active"
                };
                // Prefer remapped offsets from resolve for display when present.
                let (start, end) = res
                    .map(|r| (r.start_utf8, r.end_utf8))
                    .unwrap_or((hl.start_utf8, hl.end_utf8));
                ui.label(
                    RichText::new(format!(
                        "[{status}] chars {start}..{end} “{}”",
                        hl.exact_quote.chars().take(40).collect::<String>()
                    ))
                    .small()
                    .monospace(),
                );
                if ui
                    .add_enabled(!state.notes_busy, egui::Button::new("Delete").small())
                    .clicked()
                {
                    state.delete_highlight_ui(matter_root, ctx, &hl.id, actor);
                }
            });
        }
    }
}

fn show_privilege_protocol_strip(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    actor: &str,
) {
    state.ensure_protocol_loaded(matter_root);
    ui.collapsing("Privilege protocol (informational 502 notes)", |ui| {
        ui.label(
            RichText::new("FRE 502(d)/502(e) references only — Desk does not issue court orders.")
                .weak()
                .small(),
        );
        ui.horizontal(|ui| {
            ui.label("502(d):");
            ui.add(
                egui::TextEdit::singleline(&mut state.protocol_draft_502d)
                    .desired_width(280.0)
                    .hint_text("Order date / docket cite…"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("502(e):");
            ui.add(
                egui::TextEdit::singleline(&mut state.protocol_draft_502e)
                    .desired_width(280.0)
                    .hint_text("Clawback agreement ref…"),
            );
        });
        ui.checkbox(
            &mut state.protocol_description_required,
            "Description required (warn on blank log rows)",
        );
        if ui.small_button("Save protocol").clicked() {
            state.save_protocol_now(matter_root, actor);
        }
        if let Some(st) = state.protocol_status.clone() {
            ui.label(RichText::new(st).small());
        }
    });
}

fn show_privilege_panel(
    ui: &mut egui::Ui,
    state: &mut ReviewState,
    matter_root: &Utf8Path,
    actor: &str,
    current_item_id: &str,
) {
    let has_code = state
        .codes_for(current_item_id)
        .iter()
        .any(|c| c.key == "privilege");
    let has_row = state.item_privilege.is_some();
    let show = should_show_privilege_panel(has_code, has_row, state.privilege_force_open);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Privilege").strong().small());
        if !show
            && ui
                .small_button("Assert privilege")
                .on_hover_text("Apply Privilege code + open claim panel")
                .clicked()
        {
            state.assert_privilege_now(matter_root, actor);
        }
    });

    if let Some(banner) = state.privilege_family_banner.clone() {
        ui.colored_label(Color32::from_rgb(180, 100, 20), format!("⚠ {banner}"));
    }
    if let Some(st) = state.privilege_status_msg.clone() {
        ui.label(
            RichText::new(st)
                .small()
                .color(Color32::from_rgb(40, 120, 60)),
        );
    }
    if let Some(err) = state.privilege_error.clone() {
        ui.colored_label(Color32::from_rgb(200, 60, 60), err);
    }

    if !show {
        return;
    }

    // Confirm dialogs
    if state.privilege_confirm_clear_withhold {
        egui::Window::new("Clear production withhold?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(
                    "You are setting withhold=0 while privilege is still asserted. \
                     Production (0040) will not hold this item. Continue?",
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        state.privilege_confirm_clear_withhold = false;
                        state.privilege_draft.withhold = true;
                    }
                    if ui.button("Confirm clear withhold").clicked() {
                        state.privilege_confirm_clear_withhold = false;
                        state.privilege_pending_save_no_withhold = true;
                        state.save_privilege_now(matter_root, actor);
                    }
                });
            });
    }
    if state.privilege_confirm_note_draft {
        egui::Window::new("Draft description from note?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(
                    "Copy the latest document note into the privilege log description draft. \
                     This is never automatic on export — review before Save.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        state.privilege_confirm_note_draft = false;
                    }
                    if ui.button("Insert draft").clicked() {
                        state.privilege_confirm_note_draft = false;
                        let note_body = state
                            .item_notes
                            .first()
                            .map(|n| n.body.as_str())
                            .unwrap_or("");
                        state.privilege_draft.description = draft_description_from_note(
                            &state.privilege_draft.description,
                            note_body,
                        );
                        state.privilege_draft.dirty = true;
                    }
                });
            });
    }

    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label("Basis:");
            egui::ComboBox::from_id_salt("priv_basis")
                .selected_text({
                    basis_options()
                        .iter()
                        .find(|(k, _)| *k == state.privilege_draft.basis)
                        .map(|(_, l)| *l)
                        .unwrap_or(state.privilege_draft.basis.as_str())
                })
                .show_ui(ui, |ui| {
                    for (key, label) in basis_options() {
                        if ui
                            .selectable_label(state.privilege_draft.basis == *key, *label)
                            .clicked()
                        {
                            state.privilege_draft.basis = (*key).to_string();
                            state.privilege_draft.dirty = true;
                        }
                    }
                });
            ui.label("Status:");
            egui::ComboBox::from_id_salt("priv_status")
                .selected_text({
                    status_options()
                        .iter()
                        .find(|(k, _)| *k == state.privilege_draft.status)
                        .map(|(_, l)| *l)
                        .unwrap_or(state.privilege_draft.status.as_str())
                })
                .show_ui(ui, |ui| {
                    for (key, label) in status_options() {
                        if ui
                            .selectable_label(state.privilege_draft.status == *key, *label)
                            .clicked()
                        {
                            state.privilege_draft.status = (*key).to_string();
                            state.privilege_draft.dirty = true;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            if ui
                .checkbox(
                    &mut state.privilege_draft.withhold,
                    "Withhold from production",
                )
                .changed()
            {
                state.privilege_draft.dirty = true;
            }
            if ui
                .checkbox(
                    &mut state.privilege_draft.include_on_log,
                    "Include on privilege log",
                )
                .changed()
            {
                state.privilege_draft.dirty = true;
            }
        });
        ui.label("Description (subject-matter log text — not privileged body):");
        let desc_resp = ui.add(
            egui::TextEdit::multiline(&mut state.privilege_draft.description)
                .id_salt("privilege_description")
                .desired_width(ui.available_width())
                .desired_rows(3)
                .hint_text("e.g. Legal advice re contract negotiation"),
        );
        if desc_resp.has_focus() {
            state.privilege_editor_focused = true;
        }
        if desc_resp.changed() {
            state.privilege_draft.dirty = true;
        }
        ui.horizontal(|ui| {
            if ui.small_button("Save").clicked() {
                state.save_privilege_now(matter_root, actor);
            }
            if ui
                .small_button("Draft from note…")
                .on_hover_text("Optional: copy latest note into description draft (confirm)")
                .clicked()
            {
                state.privilege_confirm_note_draft = true;
            }
            if !has_code
                && ui
                    .small_button("Assert (apply code)")
                    .on_hover_text("Apply Privilege code if missing")
                    .clicked()
            {
                state.assert_privilege_now(matter_root, actor);
            }
        });
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

    // P0 “Add code…” — label → slug key; group custom/issues, multi cardinality.
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!state.coding_busy, egui::Button::new("Add code…").small())
            .on_hover_text("Create a custom catalog entry (label → machine key)")
            .clicked()
        {
            state.show_add_code = !state.show_add_code;
            if state.show_add_code && state.add_code_group.trim().is_empty() {
                state.add_code_group = "custom".into();
            }
        }
    });
    if state.show_add_code {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Label:").small());
            ui.add(
                egui::TextEdit::singleline(&mut state.add_code_label)
                    .desired_width(140.0)
                    .hint_text("e.g. Trade secret"),
            );
            ui.label(RichText::new("Group:").small());
            if ui
                .selectable_label(state.add_code_group == "custom", "custom")
                .clicked()
            {
                state.add_code_group = "custom".into();
            }
            if ui
                .selectable_label(state.add_code_group == "issues", "issues")
                .clicked()
            {
                state.add_code_group = "issues".into();
            }
            let can_create = !state.add_code_label.trim().is_empty() && !state.coding_busy;
            if ui
                .add_enabled(can_create, egui::Button::new("Create").small())
                .clicked()
            {
                let label = state.add_code_label.trim().to_string();
                let group = if state.add_code_group.trim().is_empty() {
                    "custom".to_string()
                } else {
                    state.add_code_group.trim().to_string()
                };
                match upsert_code_definition_blocking(matter_root, &label, &group) {
                    Ok(_) => {
                        state.add_code_label.clear();
                        state.show_add_code = false;
                        state.coding_error = None;
                        state.coding_status = Some(format!("Added code “{label}”."));
                        state.reload_coding_catalog(matter_root);
                    }
                    Err(e) => {
                        state.coding_error = Some(format!("Add code: {e}"));
                    }
                }
            }
        });
        ui.label(
            RichText::new("Key is slugified from the label; cardinality = multi.")
                .weak()
                .small(),
        );
    }

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
            let hover = if def.cardinality == "single" {
                format!(
                    "{} — single-group '{}': only one batch selection (last click wins)",
                    def.key, def.group_key
                )
            } else {
                format!("{} ({})", def.key, def.group_key)
            };
            if ui
                .checkbox(&mut checked, def.label.as_str())
                .on_hover_text(hover)
                .changed()
            {
                if checked {
                    if state.batch_mode_add {
                        select_batch_code_for_add(
                            &mut state.batch_code_ids,
                            &def.id,
                            &def.group_key,
                            &def.cardinality,
                            &active,
                        );
                    } else {
                        state.batch_code_ids.insert(def.id.clone());
                    }
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
    fn filter_draft_to_spec_and_clear() {
        let mut draft = FilterDraft {
            custodian: "alice".into(),
            include_family: true,
            ..FilterDraft::default()
        };
        draft.code_keys.insert("responsive".into());
        let spec = draft.to_filter_spec().expect("spec");
        assert!(spec.include_family);
        assert_eq!(spec.conditions.len(), 2);
        assert!(spec
            .conditions
            .iter()
            .any(|c| c.field == "custodian" && c.op == "contains"));
        assert!(spec
            .conditions
            .iter()
            .any(|c| c.field == "code" && c.op == "any_of"));

        // Incomplete date range rejected.
        draft.date_from = "2023-01-01T00:00:00Z".into();
        assert!(draft.to_filter_spec().is_err());
        draft.date_to = "2024-01-01T00:00:00Z".into();
        let with_date = draft.to_filter_spec().expect("with date");
        assert_eq!(with_date.conditions.len(), 3);

        // Naive dates rejected before Apply (parse_bound_instant).
        draft.date_from = "2023-01-01T00:00:00".into();
        draft.date_to = "2024-01-01T00:00:00".into();
        assert!(draft.to_filter_spec().is_err());
        draft.date_from.clear();
        draft.date_to.clear();

        // code_missing takes precedence over code_keys when serializing.
        draft.code_missing = true;
        let uncoded = draft.to_filter_spec().expect("uncoded");
        assert!(uncoded.conditions.iter().any(|c| c.field == "code_missing"));
        assert!(!uncoded
            .conditions
            .iter()
            .any(|c| c.field == "code" && c.op == "any_of"));

        draft.clear();
        assert!(draft.custodian.is_empty());
        assert!(!draft.include_family);
        assert!(draft.code_keys.is_empty());
        assert!(!draft.code_missing);
        let empty = draft.to_filter_spec().expect("empty");
        assert!(empty.conditions.is_empty());
        assert!(!empty.include_family);
    }

    #[test]
    fn filter_spec_serde_presets() {
        let u = FilterSpec::preset_uncoded();
        let j = serde_json::to_string(&u).expect("ser");
        let back: FilterSpec = serde_json::from_str(&j).expect("de");
        assert_eq!(back.conditions.len(), 1);
        assert_eq!(back.conditions[0].field, "code_missing");

        // Draft must encode Uncoded so re-Apply does not wipe to empty corpus.
        let draft = FilterDraft::from_filter_spec(&u);
        assert!(draft.code_missing);
        assert!(draft.code_keys.is_empty());
        let round = draft.to_filter_spec().expect("round");
        assert_eq!(round.conditions.len(), 1);
        assert_eq!(round.conditions[0].field, "code_missing");

        let p = FilterSpec::preset_privilege();
        assert_eq!(p.conditions[0].values.as_ref().unwrap()[0], "privilege");
        let r = FilterSpec::preset_responsive();
        assert_eq!(r.conditions[0].values.as_ref().unwrap()[0], "responsive");
    }

    #[test]
    fn focus_gate_blocks_shortcuts_when_text_focused() {
        // Mirrors review_nav contract used by filter TextEdit fields.
        assert!(review_nav::focus_allows_shortcuts(true));
        assert!(!review_nav::focus_allows_shortcuts(false));
    }

    #[test]
    fn apply_clear_filter_state_machine() {
        let mut state = ReviewState::default();
        assert!(!state.filter_active);
        state.filter_draft.custodian = "alice".into();
        let spec = state.filter_draft.to_filter_spec().expect("spec");
        state.applied_filter = Some(spec);
        state.filter_active = true;
        assert!(state.filter_active);
        state.filter_draft.clear();
        state.applied_filter = None;
        state.filter_active = false;
        assert!(!state.filter_active);
        assert!(state
            .filter_draft
            .to_filter_spec()
            .expect("empty")
            .conditions
            .is_empty());
    }

    #[test]
    fn save_prefers_applied_filter_over_empty_draft() {
        // Uncoded chip path historically left draft empty; Save must still persist applied.
        let mut state = ReviewState::default();
        let uncoded = FilterSpec::preset_uncoded();
        state.applied_filter = Some(uncoded.clone());
        state.filter_active = true;
        // Intentionally leave draft empty (pre-fix regression shape).
        state.filter_draft = FilterDraft::default();
        state.filter_draft.save_name = "Uncoded save".into();

        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-save-applied");
        let _ = Matter::create(&root, "Save Applied").expect("create");

        state.save_current_filter(&root, "tester");
        assert!(
            state.filter_error.is_none(),
            "save err: {:?}",
            state.filter_error
        );

        let list = load_saved_searches(&root).expect("list");
        assert_eq!(list.len(), 1);
        let loaded: FilterSpec =
            serde_json::from_str(&list[0].filter_json).expect("parse saved json");
        assert_eq!(loaded.conditions.len(), 1);
        assert_eq!(loaded.conditions[0].field, "code_missing");
    }

    /// Matter-backed Apply / Clear / Save Uncoded round-trip (§3.8.12 + P2).
    #[test]
    fn filter_apply_clear_save_uncoded_integration() {
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-filter-apply");
        let matter = Matter::create(&root, "Filter Apply").expect("create");
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        let coded = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("coded".into()),
                ..Default::default()
            })
            .expect("coded");
        let bare = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("bare".into()),
                ..Default::default()
            })
            .expect("bare");
        let job = matter.create_job("promote").expect("job");
        matter
            .apply_promote_batch_with_checkpoint(
                &job.id,
                "promote",
                &[
                    PromoteFieldUpdate {
                        item_id: coded.id.clone(),
                        in_review: Some(1),
                        review_set_id: Some(set.id.clone()),
                        review_order: Some(1),
                        promoted_at: Some("2020-01-01T00:00:00Z".into()),
                        promote_job_id: Some(job.id.clone()),
                        promote_policy: Some("unique_only".into()),
                    },
                    PromoteFieldUpdate {
                        item_id: bare.id.clone(),
                        in_review: Some(1),
                        review_set_id: Some(set.id.clone()),
                        review_order: Some(2),
                        promoted_at: Some("2020-01-01T00:00:00Z".into()),
                        promote_job_id: Some(job.id.clone()),
                        promote_policy: Some("unique_only".into()),
                    },
                ],
                "{}",
                2,
            )
            .expect("promote");
        drop(matter);

        let defs = load_code_definitions(&root).expect("defs");
        let priv_code = defs.iter().find(|d| d.key == "privilege").expect("priv");
        apply_codes_blocking(
            &root,
            ApplyCodesInput {
                item_ids: vec![coded.id.clone()],
                add_code_ids: vec![priv_code.id.clone()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: "desk-test".into(),
            },
        )
        .expect("code");

        // Unfiltered load path via filtered helper with empty default.
        let (full_count, full_rows, _) =
            load_review_filtered(&root, &FilterSpec::default(), 0, None).expect("full");
        assert_eq!(full_count, 2);
        assert_eq!(full_rows.len(), 2);

        let mut state = ReviewState::default();
        state.apply_preset(&root, FilterSpec::preset_uncoded());
        assert!(state.filter_active);
        assert!(state.filter_draft.code_missing);
        assert!(state.list_error.is_none());
        assert!(state.filter_error.is_none());
        assert_eq!(state.count, 1);
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].id, bare.id);

        // Re-Apply from draft must keep uncoded (not wipe to empty corpus).
        state.apply_filter(&root);
        assert!(state.filter_active);
        assert!(state.filter_draft.code_missing);
        assert_eq!(state.count, 1);
        assert_eq!(state.rows[0].id, bare.id);

        // Save applied Uncoded → JSON contains code_missing.
        state.filter_draft.save_name = "Uncoded only".into();
        state.save_current_filter(&root, "desk-test");
        assert!(
            state.filter_error.is_none(),
            "save: {:?}",
            state.filter_error
        );
        let saved = load_saved_searches(&root).expect("saved");
        assert_eq!(saved.len(), 1);
        let json_spec: FilterSpec =
            serde_json::from_str(&saved[0].filter_json).expect("filter_json");
        assert!(json_spec
            .conditions
            .iter()
            .any(|c| c.field == "code_missing"));

        // Clear restores full corpus.
        state.clear_filter(&root);
        assert!(!state.filter_active);
        assert!(!state.filter_draft.code_missing);
        assert_eq!(state.count, 2);
        assert_eq!(state.rows.len(), 2);

        // Load saved search re-applies uncoded.
        state.filter_draft.selected_saved_id = Some(saved[0].id.clone());
        state.load_selected_saved_search(&root);
        assert!(state.filter_active);
        assert!(state.filter_draft.code_missing);
        assert_eq!(state.count, 1);
        assert_eq!(state.rows[0].id, bare.id);

        // Naive dates rejected on Apply (filter_error, not silent full corpus).
        state.filter_draft.code_missing = false;
        state.filter_draft.date_from = "2023-01-01T00:00:00".into();
        state.filter_draft.date_to = "2024-01-01T00:00:00".into();
        state.apply_filter(&root);
        assert!(state.filter_error.is_some());
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

    fn stub_row(id: &str) -> ReviewListRow {
        ReviewListRow {
            id: id.into(),
            review_order: Some(0),
            role: None,
            parent_item_id: None,
            subject: Some(id.into()),
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
        }
    }

    #[test]
    fn item_ids_for_code_load_visible_page_plus_selection() {
        let rows: Vec<ReviewListRow> = (0..100).map(|i| stub_row(&format!("r{i}"))).collect();
        // Empty range → leading fallback window only (not full 100).
        let ids = item_ids_for_code_load(&rows, &(0..0), None);
        assert_eq!(ids.len(), 64);
        assert_eq!(ids[0], "r0");
        assert_eq!(ids.last().map(String::as_str), Some("r63"));

        // Viewport middle slice.
        let ids = item_ids_for_code_load(&rows, &(10..15), None);
        assert_eq!(ids, vec!["r10", "r11", "r12", "r13", "r14"]);

        // Selection off-screen is always included for header chips.
        let ids = item_ids_for_code_load(&rows, &(10..12), Some("r90"));
        assert_eq!(ids, vec!["r10", "r11", "r90"]);

        // Selection already in viewport is not duplicated.
        let ids = item_ids_for_code_load(&rows, &(10..12), Some("r10"));
        assert_eq!(ids, vec!["r10", "r11"]);

        assert!(item_ids_for_code_load(&[], &(0..10), Some("x")).is_empty());
    }

    #[test]
    fn should_apply_codes_off_thread_policy() {
        assert!(!should_apply_codes_off_thread(1, false, 50));
        assert!(should_apply_codes_off_thread(2, false, 50));
        assert!(should_apply_codes_off_thread(1, true, 50));
        assert!(should_apply_codes_off_thread(51, false, 50));
        assert!(should_apply_codes_off_thread(50, true, 50));
    }

    #[test]
    fn select_batch_code_for_add_enforces_single_group() {
        let defs = vec![
            CodeDef {
                id: "c_resp".into(),
                matter_id: "m".into(),
                key: "responsive".into(),
                label: "Responsive".into(),
                group_key: "responsiveness".into(),
                cardinality: "single".into(),
                color: None,
                sort_order: 0,
                is_active: 1,
                created_at: String::new(),
            },
            CodeDef {
                id: "c_not".into(),
                matter_id: "m".into(),
                key: "not_responsive".into(),
                label: "Not Responsive".into(),
                group_key: "responsiveness".into(),
                cardinality: "single".into(),
                color: None,
                sort_order: 1,
                is_active: 1,
                created_at: String::new(),
            },
            CodeDef {
                id: "c_hot".into(),
                matter_id: "m".into(),
                key: "hot".into(),
                label: "Hot".into(),
                group_key: "issues".into(),
                cardinality: "multi".into(),
                color: None,
                sort_order: 2,
                is_active: 1,
                created_at: String::new(),
            },
            CodeDef {
                id: "c_conf".into(),
                matter_id: "m".into(),
                key: "confidential".into(),
                label: "Confidential".into(),
                group_key: "issues".into(),
                cardinality: "multi".into(),
                color: None,
                sort_order: 3,
                is_active: 1,
                created_at: String::new(),
            },
        ];

        let mut selected = HashSet::new();
        select_batch_code_for_add(&mut selected, "c_resp", "responsiveness", "single", &defs);
        assert!(selected.contains("c_resp"));

        // Last click wins within single group.
        select_batch_code_for_add(&mut selected, "c_not", "responsiveness", "single", &defs);
        assert!(!selected.contains("c_resp"));
        assert!(selected.contains("c_not"));

        // Multi group does not collapse siblings.
        select_batch_code_for_add(&mut selected, "c_hot", "issues", "multi", &defs);
        select_batch_code_for_add(&mut selected, "c_conf", "issues", "multi", &defs);
        assert!(selected.contains("c_not"));
        assert!(selected.contains("c_hot"));
        assert!(selected.contains("c_conf"));
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn apply_and_upsert_code_via_desk_helpers() {
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-desk-coding");
        let matter = Matter::create(&root, "Desk Coding").expect("create");
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("Code me".into()),
                ..Default::default()
            })
            .expect("item");
        drop(matter);

        let custom_id =
            upsert_code_definition_blocking(&root, "Trade secret", "custom").expect("upsert");
        assert!(!custom_id.is_empty());
        let defs = load_code_definitions(&root).expect("defs");
        assert!(defs
            .iter()
            .any(|d| d.label == "Trade secret" && d.key == "trade_secret"));

        let hot = defs.iter().find(|d| d.key == "hot").expect("hot seed");
        apply_codes_blocking(
            &root,
            ApplyCodesInput {
                item_ids: vec![item.id.clone()],
                add_code_ids: vec![hot.id.clone(), custom_id.clone()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: "desk-test".into(),
            },
        )
        .expect("apply");

        let map = load_item_codes(&root, std::slice::from_ref(&item.id)).expect("codes");
        let codes = map.get(&item.id).expect("item codes");
        let keys: HashSet<&str> = codes.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains("hot"));
        assert!(keys.contains("trade_secret"));

        // Scoped load: only requested ids.
        let empty = load_item_codes(&root, &[]).expect("empty");
        assert!(empty.is_empty());
    }

    #[test]
    fn note_on_selection_draft_binds_highlight_without_fake_body() {
        // Desk helper contract: user text + highlight_id; never "Note on: …".
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-passage-draft");
        let matter = Matter::create(&root, "Passage Draft").expect("create");
        let body = "The confidential clause is material.";
        let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("Note me".into()),
                text_sha256: Some(digest.clone()),
                ..Default::default()
            })
            .expect("item");
        // Char range of "confidential"
        let start = body.find("confidential").expect("quote") as i64;
        let end = start + "confidential".chars().count() as i64;
        let hl = matter
            .create_highlight(matter_core::CreateHighlightInput {
                item_id: item.id.clone(),
                start_utf8: start,
                end_utf8: end,
                exact_quote: "confidential".into(),
                display_body: body.into(),
                body_digest: digest,
                color: None,
                actor: "desk-test".into(),
            })
            .expect("hl");
        drop(matter);

        // Empty draft rejected (UI would not call Save).
        assert!(note_upsert_from_draft(&item.id, "", Some(&hl.id), "desk-test").is_err());

        let input = note_upsert_from_draft(
            &item.id,
            "Attorney: this may be privileged.",
            Some(&hl.id),
            "desk-test",
        )
        .expect("input");
        assert_eq!(input.highlight_id.as_deref(), Some(hl.id.as_str()));
        assert_eq!(input.body, "Attorney: this may be privileged.");
        assert!(!input.body.starts_with("Note on:"));

        let matter = Matter::open(&root).expect("open");
        let note = matter.upsert_note(input).expect("save");
        assert_eq!(note.highlight_id.as_deref(), Some(hl.id.as_str()));
        assert_eq!(note.body, "Attorney: this may be privileged.");
        // Only user text was stored — no synthetic placeholder notes.
        let notes = matter.list_notes(&item.id).expect("list");
        assert_eq!(notes.len(), 1);
        assert!(!notes[0].body.starts_with("Note on:"));
    }

    #[test]
    fn failed_note_save_keeps_draft_and_edit_state() {
        let mut state = ReviewState {
            rows: vec![stub_row("itm_missing")],
            selection: Some(0),
            note_draft: "precious draft text".into(),
            pending_highlight_id: Some("hlt_pending".into()),
            passage_note_hint: "Passage note on “x”…".into(),
            ..Default::default()
        };

        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        // No matter created → open fails; draft must remain.
        let missing = base.join("no-such-matter");
        let ctx = egui::Context::default();
        state.save_document_note(&missing, &ctx, "desk-test");
        assert_eq!(state.note_draft, "precious draft text");
        assert_eq!(state.pending_highlight_id.as_deref(), Some("hlt_pending"));
        assert!(!state.passage_note_hint.is_empty());
        assert!(state.notes_error.is_some());

        // Edit path: keep edit buffer on failure.
        state.note_edit_id = Some("note_x".into());
        state.note_edit_body = "edit body keep me".into();
        state.save_note_edit(&missing, &ctx, "desk-test");
        assert_eq!(state.note_edit_id.as_deref(), Some("note_x"));
        assert_eq!(state.note_edit_body, "edit body keep me");
        assert!(state.notes_error.is_some());
    }

    #[test]
    fn successful_document_note_clears_draft() {
        let tmp = TempDir::new().unwrap();
        let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let root = base.join("matter-note-clear");
        let matter = Matter::create(&root, "Note Clear").expect("create");
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("Doc note".into()),
                ..Default::default()
            })
            .expect("item");
        drop(matter);

        let mut state = ReviewState {
            rows: vec![stub_row(&item.id)],
            selection: Some(0),
            note_draft: "  durable work product  ".into(),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        state.save_document_note(&root, &ctx, "desk-test");
        assert!(state.notes_error.is_none(), "err: {:?}", state.notes_error);
        assert!(state.note_draft.is_empty(), "draft cleared after success");
        assert!(state.pending_highlight_id.is_none());
        assert_eq!(state.item_notes.len(), 1);
        assert_eq!(state.item_notes[0].body, "durable work product");
        assert!(state.item_notes[0].highlight_id.is_none());
    }
}
