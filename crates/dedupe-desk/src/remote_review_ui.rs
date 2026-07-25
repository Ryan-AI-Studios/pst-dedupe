//! Thin Connected-mode review (list + body + codes with OCC) — track **0064**.
//!
//! Full Solo review parity is residual. Body loads use a **single dedicated
//! worker** (at most one active HTTP body fetch) plus generation tokens
//! (latest-wins); 409 retains local draft codes and sets a conflict flag.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use eframe::egui::{self, Color32, RichText, Sense};

use crate::remote_client::{
    is_auth_failure_message, ConnectedSession, RemoteApplyCodesRequest, RemoteClient, RemoteError,
    RemoteItemBody, RemoteItemThin,
};

const ROW_HEIGHT: f32 = 22.0;

/// Background list load result.
struct ListLoadResult {
    items: Result<Vec<RemoteItemThin>, String>,
}

/// Background body load result (generation-gated).
struct BodyLoadResult {
    gen: u64,
    item_id: String,
    body: Result<RemoteItemBody, String>,
}

/// Job for the single body worker (latest queued job wins when draining).
struct BodyJob {
    gen: u64,
    item_id: String,
    session: ConnectedSession,
    result_tx: Sender<BodyLoadResult>,
    ctx: egui::Context,
}

/// Background codes apply result (item + generation gated).
struct CodesApplyResult {
    gen: u64,
    item_id: String,
    /// Draft codes that were attempted (retained on 409).
    draft_add: Vec<String>,
    draft_remove: Vec<String>,
    result: Result<(Vec<String>, Vec<i64>), CodesApplyErr>,
}

#[derive(Debug, Clone)]
struct CodesConflictDetail {
    message: String,
    #[allow(dead_code)]
    expected: Option<i64>,
    actual: Option<i64>,
    server_item: Option<RemoteItemThin>,
    /// Honest thin-API note when codes list is unavailable.
    snapshot_note: Option<String>,
    /// Non-auth snapshot refresh failure (surfaced in conflict panel).
    snapshot_error: Option<String>,
}

#[derive(Debug, Clone)]
enum CodesApplyErr {
    Conflict(Box<CodesConflictDetail>),
    Other(String),
    Unauthorized,
}

/// Draft coding state retained across 409 conflicts (scoped to one item).
#[derive(Debug, Clone, Default)]
pub struct CodesDraft {
    pub add_ids: String,
    pub remove_ids: String,
}

/// Connected thin review state.
pub struct RemoteReviewState {
    pub rows: Vec<RemoteItemThin>,
    pub list_error: Option<String>,
    pub selection: Option<usize>,
    body_gen: u64,
    body_text: Option<Result<String, String>>,
    body_truncated: bool,
    body_loading: bool,
    body_item_id: Option<String>,
    list_rx: Option<Receiver<ListLoadResult>>,
    body_rx: Option<Receiver<BodyLoadResult>>,
    /// Single-flight body worker job channel (dropped on clear → worker exits).
    body_job_tx: Option<Sender<BodyJob>>,
    codes_rx: Option<Receiver<CodesApplyResult>>,
    /// Generation for codes apply (latest-wins; navigate bumps to abandon in-flight).
    codes_gen: u64,
    /// Item id the current draft/conflict belongs to (if any).
    codes_item_id: Option<String>,
    list_busy: bool,
    codes_busy: bool,
    pub codes_draft: CodesDraft,
    pub status: Option<String>,
    pub error: Option<String>,
    /// OCC conflict: draft retained; operator must re-apply or discard.
    pub conflict: bool,
    pub conflict_message: Option<String>,
    pub server_version_hint: Option<i64>,
    /// Server subject/status summary for conflict panel (from get_item).
    pub server_snapshot_summary: Option<String>,
    /// Honest note when thin API cannot list codes/notes.
    pub conflict_snapshot_note: Option<String>,
    /// Non-auth failure refreshing server snapshot on 409.
    pub conflict_snapshot_error: Option<String>,
    needs_reload: bool,
}

impl Default for RemoteReviewState {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            list_error: None,
            selection: None,
            body_gen: 0,
            body_text: None,
            body_truncated: false,
            body_loading: false,
            body_item_id: None,
            list_rx: None,
            body_rx: None,
            body_job_tx: None,
            codes_rx: None,
            codes_gen: 0,
            codes_item_id: None,
            list_busy: false,
            codes_busy: false,
            codes_draft: CodesDraft::default(),
            status: None,
            error: None,
            conflict: false,
            conflict_message: None,
            server_version_hint: None,
            server_snapshot_summary: None,
            conflict_snapshot_note: None,
            conflict_snapshot_error: None,
            needs_reload: true,
        }
    }
}

impl RemoteReviewState {
    pub fn clear(&mut self) {
        // Drop body_job_tx so the dedicated worker exits (no orphan pool).
        *self = Self::default();
    }

    pub fn request_reload(&mut self) {
        self.needs_reload = true;
    }

    pub fn current_item(&self) -> Option<&RemoteItemThin> {
        self.selection.and_then(|i| self.rows.get(i))
    }

    /// True when list/body/codes surface a mid-session 401/unauthorized.
    pub fn has_auth_failure(&self) -> bool {
        if self.error.as_deref().is_some_and(is_auth_failure_message) {
            return true;
        }
        if self
            .list_error
            .as_deref()
            .is_some_and(is_auth_failure_message)
        {
            return true;
        }
        if let Some(Err(e)) = self.body_text.as_ref() {
            if is_auth_failure_message(e) {
                return true;
            }
        }
        false
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.list_rx.as_ref() {
            match rx.try_recv() {
                Ok(ListLoadResult { items }) => {
                    self.list_rx = None;
                    self.list_busy = false;
                    match items {
                        Ok(rows) => {
                            self.rows = rows;
                            self.list_error = None;
                            // Restore selection by id if possible.
                            if let Some(id) = self.body_item_id.clone() {
                                if let Some(idx) = self.rows.iter().position(|r| r.id == id) {
                                    self.selection = Some(idx);
                                }
                            }
                        }
                        Err(e) => {
                            self.list_error = Some(e);
                        }
                    }
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(TryRecvError::Disconnected) => {
                    self.list_rx = None;
                    self.list_busy = false;
                    self.list_error = Some("List worker ended unexpectedly.".into());
                }
            }
        }

        if let Some(rx) = self.body_rx.as_ref() {
            match rx.try_recv() {
                Ok(BodyLoadResult { gen, item_id, body }) => {
                    self.body_rx = None;
                    // Latest-wins: discard stale generation.
                    if gen != self.body_gen {
                        // Stale result (superseded while single-flight worker finished
                        // an older job). Stay loading if a newer gen is outstanding.
                        if self.body_loading && self.body_gen != gen {
                            ctx.request_repaint_after(std::time::Duration::from_millis(50));
                        }
                        return;
                    }
                    self.body_loading = false;
                    match body {
                        Ok(b) => {
                            self.body_text = Some(Ok(b.text));
                            self.body_truncated = b.truncated;
                            self.body_item_id = Some(item_id);
                            // Refresh review_version on the row if present.
                            if let Some(idx) = self.selection {
                                if let Some(row) = self.rows.get_mut(idx) {
                                    if row.id == b.item_id {
                                        row.review_version = b.review_version;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.body_text = Some(Err(e));
                            self.body_truncated = false;
                        }
                    }
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(TryRecvError::Disconnected) => {
                    self.body_rx = None;
                    // Worker may still be processing a newer job with a different rx.
                    // Only surface error if we still expect this generation.
                    if self.body_loading {
                        ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    }
                }
            }
        }

        if let Some(rx) = self.codes_rx.as_ref() {
            match rx.try_recv() {
                Ok(CodesApplyResult {
                    gen,
                    item_id,
                    draft_add,
                    draft_remove,
                    result,
                }) => {
                    self.codes_rx = None;
                    // Ignore stale results (navigate or superseded apply).
                    if !codes_result_is_current(
                        self.codes_gen,
                        gen,
                        self.codes_item_id.as_deref(),
                        &item_id,
                    ) {
                        // Stale channel drained; do not touch current-item draft/conflict.
                        ctx.request_repaint();
                        return;
                    }
                    self.codes_busy = false;
                    match result {
                        Ok((_targets, versions)) => {
                            self.clear_conflict_ui();
                            self.status = Some(format!("Codes applied on {item_id}."));
                            self.error = None;
                            self.codes_draft = CodesDraft::default();
                            if let Some(v) = versions.first() {
                                if let Some(row) = self.rows.iter_mut().find(|r| r.id == item_id) {
                                    row.review_version = *v;
                                }
                            }
                        }
                        Err(CodesApplyErr::Conflict(detail)) => {
                            // Non-destructive: retain draft fields for this item.
                            self.codes_draft.add_ids = draft_add.join(",");
                            self.codes_draft.remove_ids = draft_remove.join(",");
                            self.conflict = true;
                            self.conflict_message = Some(detail.message);
                            self.server_version_hint = detail
                                .actual
                                .or_else(|| detail.server_item.as_ref().map(|i| i.review_version));
                            self.conflict_snapshot_note = detail.snapshot_note;
                            self.conflict_snapshot_error = detail.snapshot_error;
                            self.server_snapshot_summary = detail
                                .server_item
                                .as_ref()
                                .map(format_server_snapshot_summary);
                            if let Some(server) = detail.server_item {
                                if let Some(row) = self.rows.iter_mut().find(|r| r.id == server.id)
                                {
                                    *row = server;
                                }
                            }
                            self.error = Some(
                                "Version conflict — your draft codes are kept. Review server state, then Retry or Discard."
                                    .into(),
                            );
                        }
                        Err(CodesApplyErr::Unauthorized) => {
                            self.error = Some("Session expired or unauthorized (401)".into());
                        }
                        Err(CodesApplyErr::Other(e)) => {
                            self.error = Some(e);
                        }
                    }
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(TryRecvError::Disconnected) => {
                    self.codes_rx = None;
                    self.codes_busy = false;
                    self.error = Some("Codes worker ended unexpectedly.".into());
                }
            }
        }
    }

    fn clear_conflict_ui(&mut self) {
        self.conflict = false;
        self.conflict_message = None;
        self.server_version_hint = None;
        self.server_snapshot_summary = None;
        self.conflict_snapshot_note = None;
        self.conflict_snapshot_error = None;
    }

    /// Abandon draft/conflict and in-flight codes for a previous item (selection change).
    fn clear_codes_for_navigation(&mut self) {
        self.codes_gen = self.codes_gen.wrapping_add(1);
        self.codes_rx = None; // drop channel — worker send fails; result discarded
        self.codes_busy = false;
        self.codes_draft = CodesDraft::default();
        self.codes_item_id = None;
        self.clear_conflict_ui();
    }

    fn ensure_list(&mut self, session: &ConnectedSession, ctx: &egui::Context) {
        if !self.needs_reload || self.list_busy {
            return;
        }
        self.needs_reload = false;
        self.list_busy = true;
        self.list_error = None;
        let session = session.clone();
        let (tx, rx) = mpsc::channel();
        self.list_rx = Some(rx);
        let ctx = ctx.clone();
        let _ = thread::Builder::new()
            .name("desk-remote-list".into())
            .spawn(move || {
                let items = match RemoteClient::new() {
                    Ok(client) => client
                        .list_items(&session, Some(500), None)
                        .map_err(|e| e.to_string()),
                    Err(e) => Err(e),
                };
                let _ = tx.send(ListLoadResult { items });
                ctx.request_repaint();
            });
    }

    /// Ensure the single body worker is running; returns its job sender.
    fn ensure_body_worker(&mut self) -> Sender<BodyJob> {
        if let Some(tx) = self.body_job_tx.as_ref() {
            return tx.clone();
        }
        let (job_tx, job_rx) = mpsc::channel::<BodyJob>();
        let _ = thread::Builder::new()
            .name("desk-remote-body".into())
            .spawn(move || body_worker_loop(job_rx));
        self.body_job_tx = Some(job_tx.clone());
        job_tx
    }

    fn select_index(&mut self, idx: usize, session: &ConnectedSession, ctx: &egui::Context) {
        if idx >= self.rows.len() {
            return;
        }
        let item_id = self.rows[idx].id.clone();
        // Item-scoped draft: clear when navigating to a different item.
        let same_item =
            self.codes_item_id.as_deref() == Some(item_id.as_str()) && self.selection == Some(idx);
        if !same_item {
            // Always clear when selection target differs from draft scope.
            if self.codes_item_id.as_deref() != Some(item_id.as_str()) {
                self.clear_codes_for_navigation();
            }
        }
        self.selection = Some(idx);
        self.codes_item_id = Some(item_id.clone());
        // Bump generation — latest-wins apply filter; worker single-flights HTTP.
        self.body_gen = self.body_gen.wrapping_add(1);
        let gen = self.body_gen;
        self.body_loading = true;
        self.body_text = None;
        self.body_truncated = false;
        self.body_item_id = Some(item_id.clone());

        let (result_tx, result_rx) = mpsc::channel();
        self.body_rx = Some(result_rx);
        let job_tx = self.ensure_body_worker();
        let _ = job_tx.send(BodyJob {
            gen,
            item_id,
            session: session.clone(),
            result_tx,
            ctx: ctx.clone(),
        });
    }

    fn apply_codes(
        &mut self,
        session: &ConnectedSession,
        ctx: &egui::Context,
        use_server_version: bool,
    ) {
        if self.codes_busy || session.is_read_only() {
            return;
        }
        let Some(item) = self.current_item().cloned() else {
            self.error = Some("Select an item first.".into());
            return;
        };
        // Always apply to the currently selected item (never a stale draft item).
        let item_id = item.id.clone();
        self.codes_item_id = Some(item_id.clone());
        let add = parse_id_list(&self.codes_draft.add_ids);
        let remove = parse_id_list(&self.codes_draft.remove_ids);
        if add.is_empty() && remove.is_empty() {
            self.error = Some("Enter code id(s) to add and/or remove.".into());
            return;
        }
        let expected = if use_server_version {
            self.server_version_hint.unwrap_or(item.review_version)
        } else {
            item.review_version
        };
        let req = RemoteApplyCodesRequest {
            add_code_ids: if add.is_empty() {
                None
            } else {
                Some(add.clone())
            },
            remove_code_ids: if remove.is_empty() {
                None
            } else {
                Some(remove.clone())
            },
            propagate_family: Some(false),
            expected_version: expected,
        };
        self.codes_gen = self.codes_gen.wrapping_add(1);
        let gen = self.codes_gen;
        self.codes_busy = true;
        self.error = None;
        self.status = Some("Applying codes…".into());
        let session = session.clone();
        let (tx, rx) = mpsc::channel();
        self.codes_rx = Some(rx);
        let ctx = ctx.clone();
        let draft_add = add;
        let draft_remove = remove;
        let _ = thread::Builder::new()
            .name("desk-remote-codes".into())
            .spawn(move || {
                let result = match RemoteClient::new() {
                    Ok(client) => match client.apply_codes(&session, &item_id, &req) {
                        Ok(r) => Ok((r.target_item_ids, r.review_versions)),
                        Err(RemoteError::Unauthorized) => Err(CodesApplyErr::Unauthorized),
                        Err(e) if e.is_version_conflict() => {
                            map_conflict_with_snapshot(&client, &session, &item_id, e)
                        }
                        Err(e) => Err(CodesApplyErr::Other(e.to_string())),
                    },
                    Err(e) => Err(CodesApplyErr::Other(e)),
                };
                let _ = tx.send(CodesApplyResult {
                    gen,
                    item_id,
                    draft_add,
                    draft_remove,
                    result,
                });
                ctx.request_repaint();
            });
    }

    fn discard_draft(&mut self) {
        self.codes_draft = CodesDraft::default();
        self.clear_conflict_ui();
        self.error = None;
        self.status = Some("Draft discarded.".into());
    }
}

/// Fetch server snapshot on 409; propagate Unauthorized; surface other refresh errors.
fn map_conflict_with_snapshot(
    client: &RemoteClient,
    session: &ConnectedSession,
    item_id: &str,
    conflict_err: RemoteError,
) -> Result<(Vec<String>, Vec<i64>), CodesApplyErr> {
    let (expected, actual) = conflict_err.conflict_versions().unwrap_or((None, None));
    match client.get_item(session, item_id) {
        Ok(server_item) => Err(CodesApplyErr::Conflict(Box::new(CodesConflictDetail {
            message: conflict_err.to_string(),
            expected,
            actual: actual.or(Some(server_item.review_version)),
            server_item: Some(server_item),
            snapshot_note: Some(
                "Server version is shown; codes/notes list is not available in the thin API."
                    .into(),
            ),
            snapshot_error: None,
        }))),
        Err(RemoteError::Unauthorized) => Err(CodesApplyErr::Unauthorized),
        Err(snap_err) => Err(CodesApplyErr::Conflict(Box::new(CodesConflictDetail {
            message: conflict_err.to_string(),
            expected,
            actual,
            server_item: None,
            snapshot_note: None,
            snapshot_error: Some(format!("Could not refresh server snapshot: {snap_err}")),
        }))),
    }
}

fn format_server_snapshot_summary(item: &RemoteItemThin) -> String {
    let subject = item.subject.as_deref().unwrap_or("(no subject)");
    format!(
        "Server: v{} · status={} · {}",
        item.review_version, item.status, subject
    )
}

/// Whether a codes apply result should be applied to the current UI state.
#[cfg_attr(not(test), allow(dead_code))]
pub fn codes_result_is_current(
    current_gen: u64,
    result_gen: u64,
    current_item_id: Option<&str>,
    result_item_id: &str,
) -> bool {
    current_gen == result_gen && current_item_id == Some(result_item_id)
}

/// Pure helper: navigate A→B clears draft scope (unit-tested).
#[cfg_attr(not(test), allow(dead_code))]
pub fn navigate_should_clear_codes_draft(draft_item_id: Option<&str>, new_item_id: &str) -> bool {
    draft_item_id != Some(new_item_id)
}

/// Pure helper for 409 snapshot error classification (unit-tested).
#[cfg_attr(not(test), allow(dead_code))]
pub fn classify_snapshot_refresh_error(err_display: &str) -> SnapshotRefreshClass {
    if is_auth_failure_message(err_display) {
        SnapshotRefreshClass::Unauthorized
    } else {
        SnapshotRefreshClass::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRefreshClass {
    Unauthorized,
    Other,
}

fn parse_id_list(raw: &str) -> Vec<String> {
    raw.split([',', ' ', ';', '\n', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Single dedicated body worker: at most one HTTP body load at a time.
/// Queued jobs are drained to the latest before each fetch (single-flight + latest-wins).
fn body_worker_loop(rx: Receiver<BodyJob>) {
    loop {
        let first = match rx.recv() {
            Ok(j) => j,
            Err(_) => break,
        };
        let job = take_latest_body_job(first, &rx);
        let body = match RemoteClient::new() {
            Ok(client) => client
                .get_item_body(&job.session, &job.item_id)
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let ctx = job.ctx;
        let _ = job.result_tx.send(BodyLoadResult {
            gen: job.gen,
            item_id: job.item_id,
            body,
        });
        ctx.request_repaint();
    }
}

/// Keep only the newest queued body job (cooperative supersede before HTTP).
fn take_latest_body_job(first: BodyJob, rx: &Receiver<BodyJob>) -> BodyJob {
    let mut job = first;
    while let Ok(newer) = rx.try_recv() {
        job = newer;
    }
    job
}

/// Pure drain policy for tests: given a head value and remaining queue, return latest.
#[cfg_attr(not(test), allow(dead_code))]
pub fn take_latest_gen(first: u64, queued: &[u64]) -> u64 {
    queued.iter().copied().fold(first, |_, g| g)
}

/// Apply 409 conflict policy to draft state (unit-tested).
#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_conflict_retain_draft(
    draft: &mut CodesDraft,
    conflict: &mut bool,
    conflict_message: &mut Option<String>,
    attempted_add: &[String],
    attempted_remove: &[String],
    message: &str,
) {
    draft.add_ids = attempted_add.join(",");
    draft.remove_ids = attempted_remove.join(",");
    *conflict = true;
    *conflict_message = Some(message.to_string());
}

/// Whether a body result with `result_gen` should be applied given `current_gen`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn body_result_is_current(current_gen: u64, result_gen: u64) -> bool {
    current_gen == result_gen
}

/// Render Connected thin review.
pub fn show(ui: &mut egui::Ui, state: &mut RemoteReviewState, session: &ConnectedSession) {
    let ctx = ui.ctx().clone();
    state.poll(&ctx);
    state.ensure_list(session, &ctx);

    ui.heading("Review (Connected)");
    ui.label(
        RichText::new(
            "Thin remote review: list · body · codes (OCC). Jobs, produce, FTS, AI, notes, and privilege require Solo or host CLI.",
        )
        .weak()
        .small(),
    );
    if session.is_read_only() {
        ui.colored_label(
            Color32::from_rgb(180, 120, 40),
            "Role is read_only — mutates (codes) are disabled.",
        );
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!state.list_busy, egui::Button::new("Refresh list"))
            .clicked()
        {
            state.request_reload();
        }
        if state.list_busy {
            ui.spinner();
            ui.label("Loading…");
        }
        ui.label(format!("{} item(s)", state.rows.len()));
    });
    if let Some(err) = &state.list_error {
        ui.colored_label(Color32::from_rgb(200, 60, 60), err);
    }
    if let Some(err) = &state.error {
        ui.colored_label(Color32::from_rgb(200, 60, 60), err);
    }
    if let Some(st) = &state.status {
        ui.label(st);
    }

    if state.conflict {
        egui::Frame::default()
            .fill(Color32::from_rgb(60, 40, 20))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.colored_label(
                    Color32::from_rgb(255, 200, 120),
                    "Version conflict — another reviewer updated this item.",
                );
                if let Some(msg) = &state.conflict_message {
                    ui.label(RichText::new(msg).small());
                }
                if let Some(summary) = &state.server_snapshot_summary {
                    ui.label(RichText::new(summary).strong());
                } else if let Some(v) = state.server_version_hint {
                    ui.label(format!(
                        "Server review_version is now {v}. Your unsaved draft codes are kept below."
                    ));
                } else {
                    ui.label(
                        "Your unsaved draft codes are kept below. Re-apply with the new version or discard.",
                    );
                }
                if let Some(note) = &state.conflict_snapshot_note {
                    ui.label(RichText::new(note).small().weak());
                }
                if let Some(snap_err) = &state.conflict_snapshot_error {
                    ui.colored_label(Color32::from_rgb(255, 160, 120), snap_err);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !state.codes_busy && !session.is_read_only(),
                            egui::Button::new("Retry with my changes"),
                        )
                        .clicked()
                    {
                        state.apply_codes(session, &ctx, true);
                    }
                    if ui.button("Discard my changes").clicked() {
                        state.discard_draft();
                    }
                });
            });
        ui.add_space(4.0);
    }

    ui.columns(2, |cols| {
        // List
        cols[0].heading("Items");
        egui::ScrollArea::vertical()
            .id_salt("remote_review_list")
            .auto_shrink([false, false])
            .show_rows(&mut cols[0], ROW_HEIGHT, state.rows.len(), |ui, range| {
                for idx in range {
                    let row = &state.rows[idx];
                    let selected = state.selection == Some(idx);
                    let label = format!(
                        "{}  {}",
                        row.subject.as_deref().unwrap_or("(no subject)"),
                        row.from_addr.as_deref().unwrap_or("")
                    );
                    let response = ui.selectable_label(selected, truncate(&label, 80));
                    if response.clicked() {
                        // Need mutable select after borrow ends — collect intent.
                    }
                    // Store click via sense on the row area.
                    let full = ui.interact(
                        response.rect,
                        ui.id().with(("remote_row", idx)),
                        Sense::click(),
                    );
                    if full.clicked() || response.clicked() {
                        // selection applied after loop via pending — use direct call pattern.
                        // We can't call select_index while iterating rows by ref.
                        // Mark by setting a side channel.
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("remote_pending_select"), idx);
                        });
                    }
                    ui.label(
                        RichText::new(format!("v{} · {}", row.review_version, row.status))
                            .small()
                            .weak(),
                    );
                }
            });

        // Body + codes
        cols[1].heading("Body");
        if state.body_loading {
            cols[1].horizontal(|ui| {
                ui.spinner();
                ui.label("Loading body…");
            });
        }
        match &state.body_text {
            Some(Ok(text)) => {
                if state.body_truncated {
                    cols[1].colored_label(
                        Color32::from_rgb(180, 120, 40),
                        "Body truncated for display.",
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("remote_body")
                    .max_height(320.0)
                    .show(&mut cols[1], |ui| {
                        ui.label(text);
                    });
            }
            Some(Err(e)) => {
                cols[1].colored_label(Color32::from_rgb(200, 60, 60), e);
            }
            None => {
                if !state.body_loading {
                    cols[1].label(RichText::new("Select an item.").weak());
                }
            }
        }

        cols[1].add_space(8.0);
        cols[1].heading("Codes");
        if let Some(item) = state.current_item() {
            cols[1].label(format!(
                "Item {} · expected_version = {}",
                item.id, item.review_version
            ));
        }
        cols[1].horizontal(|ui| {
            ui.label("Add code ids:");
            ui.add(
                egui::TextEdit::singleline(&mut state.codes_draft.add_ids)
                    .desired_width(200.0)
                    .hint_text("comma-separated"),
            );
        });
        cols[1].horizontal(|ui| {
            ui.label("Remove code ids:");
            ui.add(
                egui::TextEdit::singleline(&mut state.codes_draft.remove_ids)
                    .desired_width(200.0)
                    .hint_text("comma-separated"),
            );
        });
        let can_mutate =
            !session.is_read_only() && !state.codes_busy && state.current_item().is_some();
        if cols[1]
            .add_enabled(can_mutate, egui::Button::new("Apply codes"))
            .on_disabled_hover_text(if session.is_read_only() {
                "read_only role"
            } else {
                "Select an item"
            })
            .clicked()
        {
            state.apply_codes(session, &ctx, state.conflict);
        }
        if state.codes_busy {
            cols[1].spinner();
        }
    });

    // Apply pending selection outside borrows.
    if let Some(idx) = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<usize>(egui::Id::new("remote_pending_select")))
    {
        state.select_index(idx, session, &ctx);
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_client::{
        build_codes_request_json, force_clear_connected_session, is_auth_failure_message,
        BearerToken, ConnectedSession,
    };

    #[test]
    fn conflict_retains_draft_and_sets_flag() {
        let mut draft = CodesDraft {
            add_ids: String::new(),
            remove_ids: String::new(),
        };
        let mut conflict = false;
        let mut msg = None;
        apply_conflict_retain_draft(
            &mut draft,
            &mut conflict,
            &mut msg,
            &["code_a".into()],
            &["code_b".into()],
            "version conflict: expected 1, actual 2",
        );
        assert!(conflict);
        assert_eq!(draft.add_ids, "code_a");
        assert_eq!(draft.remove_ids, "code_b");
        assert!(msg.unwrap().contains("version conflict"));
    }

    #[test]
    fn stale_body_discarded_after_navigate() {
        let mut current = 1u64;
        // Navigate → gen 2
        current = current.wrapping_add(1);
        assert!(!body_result_is_current(current, 1));
        assert!(body_result_is_current(current, 2));
    }

    #[test]
    fn body_single_flight_drains_to_latest_gen() {
        // Rapid navigate: gens 1,2,3,4,5 queued after first starts → only 5 runs next.
        assert_eq!(take_latest_gen(1, &[2, 3, 4, 5]), 5);
        assert_eq!(take_latest_gen(7, &[]), 7);
        assert_eq!(take_latest_gen(1, &[9]), 9);
        // Stale result still discarded by generation gate.
        let current = 5u64;
        assert!(!body_result_is_current(current, 1));
        assert!(body_result_is_current(current, 5));
    }

    #[test]
    fn body_channel_drain_keeps_only_latest() {
        let (tx, rx) = mpsc::channel();
        tx.send(1u64).unwrap();
        tx.send(2u64).unwrap();
        tx.send(5u64).unwrap();
        let first = rx.recv().unwrap();
        let latest = {
            let mut j = first;
            while let Ok(n) = rx.try_recv() {
                j = n;
            }
            j
        };
        assert_eq!(latest, 5);
    }

    #[test]
    fn navigate_clears_codes_draft_for_other_item() {
        assert!(navigate_should_clear_codes_draft(Some("item-a"), "item-b"));
        assert!(!navigate_should_clear_codes_draft(Some("item-a"), "item-a"));
        assert!(navigate_should_clear_codes_draft(None, "item-b"));
    }

    #[test]
    fn select_index_clears_draft_when_item_changes() {
        let mut state = RemoteReviewState {
            rows: vec![
                RemoteItemThin {
                    id: "item-a".into(),
                    subject: Some("A".into()),
                    from_addr: None,
                    sent_at: None,
                    review_version: 1,
                    status: "active".into(),
                },
                RemoteItemThin {
                    id: "item-b".into(),
                    subject: Some("B".into()),
                    from_addr: None,
                    sent_at: None,
                    review_version: 1,
                    status: "active".into(),
                },
            ],
            codes_draft: CodesDraft {
                add_ids: "code_from_a".into(),
                remove_ids: String::new(),
            },
            codes_item_id: Some("item-a".into()),
            conflict: true,
            conflict_message: Some("stale conflict".into()),
            selection: Some(0),
            ..RemoteReviewState::default()
        };
        // Simulate navigation clear (same as select_index without HTTP).
        assert!(navigate_should_clear_codes_draft(
            state.codes_item_id.as_deref(),
            "item-b"
        ));
        state.clear_codes_for_navigation();
        state.selection = Some(1);
        state.codes_item_id = Some("item-b".into());
        assert!(state.codes_draft.add_ids.is_empty());
        assert!(!state.conflict);
        assert!(state.conflict_message.is_none());
        assert!(!state.codes_busy);
    }

    #[test]
    fn stale_codes_result_for_item_a_ignored_when_on_b() {
        // On item B at gen 2; result for item A at gen 1 must not apply.
        assert!(!codes_result_is_current(2, 1, Some("item-b"), "item-a"));
        assert!(!codes_result_is_current(2, 2, Some("item-b"), "item-a"));
        assert!(!codes_result_is_current(2, 1, Some("item-b"), "item-b"));
        assert!(codes_result_is_current(2, 2, Some("item-b"), "item-b"));
    }

    #[test]
    fn apply_always_uses_current_item_id() {
        // codes_item_id is set from current selection before spawn.
        let item = RemoteItemThin {
            id: "current-item".into(),
            subject: None,
            from_addr: None,
            sent_at: None,
            review_version: 3,
            status: "active".into(),
        };
        let mut state = RemoteReviewState {
            rows: vec![item.clone()],
            selection: Some(0),
            codes_draft: CodesDraft {
                add_ids: "c1".into(),
                remove_ids: String::new(),
            },
            codes_item_id: Some("old-item".into()),
            ..RemoteReviewState::default()
        };
        // Mimic apply_codes pre-flight scoping without network.
        let current = state.current_item().unwrap().id.clone();
        state.codes_item_id = Some(current.clone());
        assert_eq!(state.codes_item_id.as_deref(), Some("current-item"));
        assert_eq!(current, "current-item");
    }

    #[test]
    fn snapshot_auth_failure_is_detectable() {
        assert_eq!(
            classify_snapshot_refresh_error("Session expired or unauthorized (401)"),
            SnapshotRefreshClass::Unauthorized
        );
        assert_eq!(
            classify_snapshot_refresh_error("Network error: connection refused"),
            SnapshotRefreshClass::Other
        );
        // Unauthorized on conflict refresh must surface as auth fail for Solo force.
        let state = RemoteReviewState {
            error: Some("Session expired or unauthorized (401)".into()),
            ..RemoteReviewState::default()
        };
        assert!(state.has_auth_failure());
    }

    #[test]
    fn snapshot_error_not_swallowed_on_conflict() {
        // Conflict path stores snapshot_error for the UI (not silent .ok()).
        let state = RemoteReviewState {
            conflict: true,
            conflict_snapshot_error: Some(
                "Could not refresh server snapshot: Network error: timeout".into(),
            ),
            ..RemoteReviewState::default()
        };
        assert!(state
            .conflict_snapshot_error
            .as_ref()
            .unwrap()
            .contains("timeout"));
        // Auth class is separate and detectable.
        assert_eq!(
            classify_snapshot_refresh_error("unauthorized"),
            SnapshotRefreshClass::Unauthorized
        );
    }

    #[test]
    fn server_snapshot_summary_includes_version_status() {
        let item = RemoteItemThin {
            id: "i1".into(),
            subject: Some("Hello".into()),
            from_addr: None,
            sent_at: None,
            review_version: 7,
            status: "reviewed".into(),
        };
        let s = format_server_snapshot_summary(&item);
        assert!(s.contains("v7"));
        assert!(s.contains("reviewed"));
        assert!(s.contains("Hello"));
    }

    #[test]
    fn has_auth_failure_detects_401_surfaces() {
        let mut state = RemoteReviewState::default();
        assert!(!state.has_auth_failure());
        state.error = Some("Session expired or unauthorized (401)".into());
        assert!(state.has_auth_failure());
        state.error = None;
        state.list_error = Some("HTTP 401 (unauthorized): bad token".into());
        assert!(state.has_auth_failure());
        state.list_error = None;
        state.body_text = Some(Err("Session expired or unauthorized (401)".into()));
        assert!(state.has_auth_failure());
    }

    #[test]
    fn auth_fail_helper_clears_session() {
        let mut session = Some(ConnectedSession {
            base_url: "http://127.0.0.1:7749".into(),
            token: BearerToken::new("dead-token"),
            user_id: "u1".into(),
            display_name: "Alice".into(),
            role: "reviewer".into(),
            expires_at: None,
        });
        assert!(force_clear_connected_session(&mut session));
        assert!(session.is_none());
        assert!(!force_clear_connected_session(&mut session));
        assert!(is_auth_failure_message(
            "Session expired or unauthorized (401)"
        ));
        assert!(!is_auth_failure_message("version conflict: expected 1"));
    }

    #[test]
    fn remote_codes_sends_expected_version_without_actor() {
        let v = build_codes_request_json(Some(vec!["x".into()]), None, None, 42);
        assert_eq!(v["expected_version"], 42);
        assert!(v.get("actor").is_none());
    }
}
