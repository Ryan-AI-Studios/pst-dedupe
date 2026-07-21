//! Conversation-centric review UI (track 0056).
//!
//! Day-bucketed `conversation_id` list + full-stream message view.
//! Filters badge hits; they do **not** hide neighbors. Linear Review remains.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui::{self, Color32, RichText, Sense};
use matter_core::{
    ApplyCodesInput, ApplyCodesResult, CodeDef, ConversationMessageRow, ConversationSummary,
    Matter, CONVERSATION_LIST_DEFAULT_LIMIT, CONVERSATION_STREAM_DEFAULT_LIMIT,
    REPLY_SNIPPET_UNAVAILABLE,
};

use crate::review_body::{BodyLoader, BodyPane};
use crate::review_ui::ROW_HEIGHT;

/// Fixed stream row height for virtualization (reply chrome fits in one row).
pub const STREAM_ROW_HEIGHT: f32 = 48.0;

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------

/// Payload for the bulk-code confirm dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkCodeConfirmPayload {
    pub conversation_id: String,
    pub message_count: i64,
    pub code_labels: Vec<String>,
}

/// Build confirm payload; returns `None` if selection is incomplete.
pub fn bulk_code_confirm_payload(
    conversation_id: Option<&str>,
    message_count: Option<i64>,
    code_labels: &[String],
) -> Option<BulkCodeConfirmPayload> {
    let cid = conversation_id?.trim();
    if cid.is_empty() {
        return None;
    }
    let count = message_count?;
    if count <= 0 || code_labels.is_empty() {
        return None;
    }
    Some(BulkCodeConfirmPayload {
        conversation_id: cid.to_string(),
        message_count: count,
        code_labels: code_labels.to_vec(),
    })
}

/// Which loaded message ids should show a Hit badge.
pub fn hit_badge_ids(
    page_ids: &[String],
    active_hits: Option<&HashSet<String>>,
) -> HashSet<String> {
    match active_hits {
        None => HashSet::new(),
        Some(hits) if hits.is_empty() => HashSet::new(),
        Some(hits) => page_ids
            .iter()
            .filter(|id| hits.contains(*id))
            .cloned()
            .collect(),
    }
}

/// Format reply chrome line.
pub fn reply_chrome_line(snippet: Option<&str>) -> String {
    let body = snippet.unwrap_or(REPLY_SNIPPET_UNAVAILABLE);
    format!("In reply to: {body}")
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

enum ConvOpResult {
    ListLoaded {
        conversations: Vec<ConversationSummary>,
        total_hint: usize,
        /// Present when catalog was empty and loaded with the list.
        code_defs: Option<Vec<CodeDef>>,
    },
    StreamLoaded {
        conversation_id: String,
        messages: Vec<ConversationMessageRow>,
        hit_ids: HashSet<String>,
        select_item_id: Option<String>,
        centered: bool,
        /// Full day-bucket size (not loaded page length) for bulk-code honesty.
        bucket_message_count: i64,
        /// Rows prepended at the front (load earlier); used to preserve scroll.
        prepended: usize,
    },
    /// Resolved full bucket count so bulk confirm never shows page length.
    BulkConfirmReady {
        payload: BulkCodeConfirmPayload,
        code_ids: Vec<String>,
    },
    Coded {
        result: ApplyCodesResult,
        message: String,
    },
    Error(String),
}

/// Pending bulk-code confirm.
#[derive(Debug, Clone)]
struct BulkConfirm {
    payload: BulkCodeConfirmPayload,
    code_ids: Vec<String>,
}

/// Desk conversation review state.
pub struct ConversationState {
    pub conversations: Vec<ConversationSummary>,
    pub selected_conversation: Option<String>,
    pub messages: Vec<ConversationMessageRow>,
    pub selected_message: Option<String>,
    /// Hit badges for the loaded stream page.
    pub hit_ids: HashSet<String>,
    /// Optional external hit set (from Review filter/FTS) for list discovery + badges.
    pub active_hit_ids: Option<HashSet<String>>,
    /// Full bucket message count for the selected conversation (never page length).
    pub bucket_message_count: Option<i64>,
    pub error: Option<String>,
    pub status: Option<String>,
    pub busy: bool,
    /// Scroll stream so selected message is visible (handoff / programmatic select).
    scroll_to_selected: bool,
    /// One-shot vertical scroll offset for the stream `ScrollArea`.
    pending_scroll_offset: Option<f32>,
    /// Last known stream scroll offset (for load-earlier viewport preserve).
    last_scroll_offset_y: f32,
    /// Body loader for selected message.
    body: BodyLoader,
    code_defs: Vec<CodeDef>,
    /// Codes checked for apply (definition ids).
    selected_code_ids: HashSet<String>,
    bulk_confirm: Option<BulkConfirm>,
    coding_busy: bool,
    /// Handoff request: open conversation centered on item.
    pending_handoff: Option<(String, String)>,
    needs_list_reload: bool,
    loaded_root: Option<Utf8PathBuf>,
    op_rx: Option<Receiver<ConvOpResult>>,
}

impl Default for ConversationState {
    fn default() -> Self {
        Self {
            conversations: Vec::new(),
            selected_conversation: None,
            messages: Vec::new(),
            selected_message: None,
            hit_ids: HashSet::new(),
            active_hit_ids: None,
            bucket_message_count: None,
            error: None,
            status: None,
            busy: false,
            scroll_to_selected: false,
            pending_scroll_offset: None,
            last_scroll_offset_y: 0.0,
            body: BodyLoader::default(),
            code_defs: Vec::new(),
            selected_code_ids: HashSet::new(),
            bulk_confirm: None,
            coding_busy: false,
            pending_handoff: None,
            needs_list_reload: true,
            loaded_root: None,
            op_rx: None,
        }
    }
}

impl ConversationState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_for_matter_change(&mut self) {
        *self = Self::default();
    }

    /// Request list reload when entering the screen.
    pub fn request_reload(&mut self) {
        self.needs_list_reload = true;
    }

    /// Open conversation centered on a hit item (search handoff from Review).
    pub fn handoff_to_item(&mut self, conversation_id: String, item_id: String) {
        self.pending_handoff = Some((conversation_id, item_id));
        self.needs_list_reload = true;
    }

    /// Set active hit ids from Review filter/FTS (for list discovery + badges).
    pub fn set_active_hits(&mut self, hits: Option<HashSet<String>>) {
        self.active_hit_ids = hits;
        self.needs_list_reload = true;
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.op_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(ConvOpResult::ListLoaded {
                conversations,
                total_hint,
                code_defs,
            }) => {
                self.conversations = conversations;
                if let Some(defs) = code_defs {
                    self.code_defs = defs;
                }
                self.busy = false;
                self.op_rx = None;
                self.error = None;
                self.status = Some(format!("{total_hint} conversation(s)"));
            }
            Ok(ConvOpResult::StreamLoaded {
                conversation_id,
                messages,
                hit_ids,
                select_item_id,
                centered,
                bucket_message_count,
                prepended,
            }) => {
                self.selected_conversation = Some(conversation_id);
                self.messages = messages;
                self.hit_ids = hit_ids;
                self.bucket_message_count = Some(bucket_message_count);
                if prepended > 0 {
                    // Keep the previously visible rows in place after prepend.
                    self.pending_scroll_offset =
                        Some(self.last_scroll_offset_y + prepended as f32 * STREAM_ROW_HEIGHT);
                }
                if let Some(id) = select_item_id {
                    self.selected_message = Some(id);
                    // Only auto-scroll for centered handoff (not load more/earlier).
                    if centered {
                        self.scroll_to_selected = true;
                    }
                } else if self
                    .selected_message
                    .as_ref()
                    .is_some_and(|id| !self.messages.iter().any(|m| m.id == *id))
                {
                    self.selected_message = self.messages.first().map(|m| m.id.clone());
                }
                self.busy = false;
                self.op_rx = None;
                self.error = None;
                self.status = Some(if centered {
                    format!(
                        "Centered handoff · {} of {} message(s) in window",
                        self.messages.len(),
                        bucket_message_count
                    )
                } else {
                    format!(
                        "{} of {} message(s) loaded (full day context)",
                        self.messages.len(),
                        bucket_message_count
                    )
                });
            }
            Ok(ConvOpResult::BulkConfirmReady { payload, code_ids }) => {
                self.busy = false;
                self.op_rx = None;
                self.bucket_message_count = Some(payload.message_count);
                self.bulk_confirm = Some(BulkConfirm { payload, code_ids });
            }
            Ok(ConvOpResult::Coded { result, message }) => {
                self.coding_busy = false;
                self.busy = false;
                self.op_rx = None;
                self.status = Some(format!("{message} · {} target(s)", result.target_count));
                self.bulk_confirm = None;
            }
            Ok(ConvOpResult::Error(e)) => {
                self.error = Some(e);
                self.busy = false;
                self.coding_busy = false;
                self.op_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.coding_busy = false;
                self.op_rx = None;
                self.error = Some("Conversation load thread ended unexpectedly.".into());
            }
        }
    }

    fn spawn_list(&mut self, matter_root: &Utf8Path) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let root = matter_root.to_path_buf();
        let hits = self.active_hit_ids.clone();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        let need_codes = self.code_defs.is_empty();
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let hit_vec: Option<Vec<String>> = hits.map(|h| h.into_iter().collect());
                let conversations = matter
                    .list_conversations(hit_vec.as_deref(), CONVERSATION_LIST_DEFAULT_LIMIT, 0)
                    .map_err(|e| e.to_string())?;
                let n = conversations.len();
                let code_defs = if need_codes {
                    Some(matter.list_code_definitions().map_err(|e| e.to_string())?)
                } else {
                    None
                };
                Ok(ConvOpResult::ListLoaded {
                    conversations,
                    total_hint: n,
                    code_defs,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }

    fn spawn_stream(
        &mut self,
        matter_root: &Utf8Path,
        conversation_id: String,
        anchor_item_id: Option<String>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.bucket_message_count = None;
        let root = matter_root.to_path_buf();
        let hits = self.active_hit_ids.clone();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let (messages, centered) = if let Some(ref aid) = anchor_item_id {
                    let page = matter
                        .list_conversation_messages_around(&conversation_id, aid, None, None, true)
                        .map_err(|e| e.to_string())?;
                    (page, true)
                } else {
                    let page = matter
                        .list_conversation_messages(
                            &conversation_id,
                            None,
                            None,
                            CONVERSATION_STREAM_DEFAULT_LIMIT,
                            true,
                        )
                        .map_err(|e| e.to_string())?;
                    (page, false)
                };
                let bucket_message_count = matter
                    .list_conversation_item_ids(&conversation_id)
                    .map_err(|e| e.to_string())?
                    .len() as i64;
                let page_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
                let hit_ids = hit_badge_ids(&page_ids, hits.as_ref());
                // Verify in-conversation membership when hits were supplied.
                let hit_ids = if hits.is_some() {
                    matter
                        .conversation_hit_id_set(&conversation_id, &page_ids, hits.as_ref())
                        .map_err(|e| e.to_string())?
                } else {
                    hit_ids
                };
                Ok(ConvOpResult::StreamLoaded {
                    conversation_id,
                    messages,
                    hit_ids,
                    select_item_id: anchor_item_id,
                    centered,
                    bucket_message_count,
                    prepended: 0,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }

    fn spawn_load_more(&mut self, matter_root: &Utf8Path) {
        if self.busy {
            return;
        }
        let Some(cid) = self.selected_conversation.clone() else {
            return;
        };
        let Some(last) = self.messages.last() else {
            return;
        };
        self.busy = true;
        let root = matter_root.to_path_buf();
        let after_sent = last.sent_at.clone();
        let after_id = last.id.clone();
        let existing = self.messages.clone();
        let hits = self.active_hit_ids.clone();
        let selected = self.selected_message.clone();
        let bucket_message_count = self.bucket_message_count.unwrap_or(0);
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let more = matter
                    .list_conversation_messages(
                        &cid,
                        after_sent.as_deref(),
                        Some(after_id.as_str()),
                        CONVERSATION_STREAM_DEFAULT_LIMIT,
                        true,
                    )
                    .map_err(|e| e.to_string())?;
                let mut messages = existing;
                messages.extend(more);
                let bucket_message_count = if bucket_message_count > 0 {
                    bucket_message_count
                } else {
                    matter
                        .list_conversation_item_ids(&cid)
                        .map_err(|e| e.to_string())?
                        .len() as i64
                };
                let page_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
                let hit_ids = if hits.is_some() {
                    matter
                        .conversation_hit_id_set(&cid, &page_ids, hits.as_ref())
                        .map_err(|e| e.to_string())?
                } else {
                    hit_badge_ids(&page_ids, hits.as_ref())
                };
                Ok(ConvOpResult::StreamLoaded {
                    conversation_id: cid,
                    messages,
                    hit_ids,
                    select_item_id: selected,
                    centered: false,
                    bucket_message_count,
                    prepended: 0,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }

    fn spawn_load_earlier(&mut self, matter_root: &Utf8Path) {
        if self.busy {
            return;
        }
        let Some(cid) = self.selected_conversation.clone() else {
            return;
        };
        let Some(first) = self.messages.first() else {
            return;
        };
        self.busy = true;
        let root = matter_root.to_path_buf();
        let before_sent = first.sent_at.clone();
        let before_id = first.id.clone();
        let existing = self.messages.clone();
        let hits = self.active_hit_ids.clone();
        let selected = self.selected_message.clone();
        let bucket_message_count = self.bucket_message_count.unwrap_or(0);
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let earlier = matter
                    .list_conversation_messages_before(
                        &cid,
                        before_sent.as_deref(),
                        Some(before_id.as_str()),
                        CONVERSATION_STREAM_DEFAULT_LIMIT,
                        true,
                    )
                    .map_err(|e| e.to_string())?;
                let mut messages = earlier;
                // Dedupe if edge overlap, then append existing.
                let existing_ids: HashSet<String> = existing.iter().map(|m| m.id.clone()).collect();
                messages.retain(|m| !existing_ids.contains(&m.id));
                let prepended = messages.len();
                messages.extend(existing);
                let bucket_message_count = if bucket_message_count > 0 {
                    bucket_message_count
                } else {
                    matter
                        .list_conversation_item_ids(&cid)
                        .map_err(|e| e.to_string())?
                        .len() as i64
                };
                let page_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
                let hit_ids = if hits.is_some() {
                    matter
                        .conversation_hit_id_set(&cid, &page_ids, hits.as_ref())
                        .map_err(|e| e.to_string())?
                } else {
                    hit_badge_ids(&page_ids, hits.as_ref())
                };
                Ok(ConvOpResult::StreamLoaded {
                    conversation_id: cid,
                    messages,
                    hit_ids,
                    select_item_id: selected,
                    centered: false,
                    bucket_message_count,
                    prepended,
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }

    /// Resolve full bucket count off-thread before showing bulk confirm (never page length).
    fn spawn_bulk_confirm_resolve(
        &mut self,
        matter_root: &Utf8Path,
        code_ids: Vec<String>,
        code_labels: Vec<String>,
    ) {
        if self.busy {
            return;
        }
        let Some(cid) = self.selected_conversation.clone() else {
            return;
        };
        self.busy = true;
        self.error = None;
        let root = matter_root.to_path_buf();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open_for_read(&root).map_err(|e| e.to_string())?;
                let count = matter
                    .list_conversation_item_ids(&cid)
                    .map_err(|e| e.to_string())?
                    .len() as i64;
                let payload = bulk_code_confirm_payload(Some(&cid), Some(count), &code_labels)
                    .ok_or_else(|| {
                        "Cannot bulk-code: empty conversation or no codes.".to_string()
                    })?;
                Ok(ConvOpResult::BulkConfirmReady { payload, code_ids })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }

    fn spawn_code_selected(&mut self, matter_root: &Utf8Path, actor: &str) {
        let Some(item_id) = self.selected_message.clone() else {
            self.error = Some("Select a message to code.".into());
            return;
        };
        if self.selected_code_ids.is_empty() {
            self.error = Some("Select at least one code.".into());
            return;
        }
        if self.coding_busy {
            return;
        }
        self.coding_busy = true;
        self.busy = true;
        let root = matter_root.to_path_buf();
        let code_ids: Vec<String> = self.selected_code_ids.iter().cloned().collect();
        let actor = actor.to_string();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open(&root).map_err(|e| e.to_string())?;
                let result = matter
                    .apply_codes(ApplyCodesInput {
                        item_ids: vec![item_id],
                        add_code_ids: code_ids,
                        remove_code_ids: vec![],
                        propagate_family: false,
                        actor,
                    })
                    .map_err(|e| e.to_string())?;
                Ok(ConvOpResult::Coded {
                    result,
                    message: "Coded selected message".into(),
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }

    fn spawn_code_bucket(&mut self, matter_root: &Utf8Path, actor: &str, code_ids: Vec<String>) {
        let Some(cid) = self.selected_conversation.clone() else {
            return;
        };
        if self.coding_busy {
            return;
        }
        self.coding_busy = true;
        self.busy = true;
        let root = matter_root.to_path_buf();
        let actor = actor.to_string();
        let (tx, rx) = mpsc::channel();
        self.op_rx = Some(rx);
        thread::spawn(move || {
            let result = (|| -> Result<ConvOpResult, String> {
                let matter = Matter::open(&root).map_err(|e| e.to_string())?;
                let ids = matter
                    .list_conversation_item_ids(&cid)
                    .map_err(|e| e.to_string())?;
                if ids.is_empty() {
                    return Err("Conversation has no messages to code.".into());
                }
                let result = matter
                    .apply_codes(ApplyCodesInput {
                        item_ids: ids,
                        add_code_ids: code_ids,
                        remove_code_ids: vec![],
                        propagate_family: false,
                        actor,
                    })
                    .map_err(|e| e.to_string())?;
                Ok(ConvOpResult::Coded {
                    result,
                    message: format!("Coded entire day bucket ({cid})"),
                })
            })();
            let _ = tx.send(result.unwrap_or_else(ConvOpResult::Error));
        });
    }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

/// Draw the Conversation review screen.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut ConversationState,
    matter_root: Option<&Utf8Path>,
    actor: &str,
) {
    state.poll();
    state.body.try_take();

    let Some(root) = matter_root else {
        ui.label("Open a matter to review conversations.");
        return;
    };

    if state.loaded_root.as_deref() != Some(root) {
        state.clear_for_matter_change();
        state.loaded_root = Some(root.to_path_buf());
        state.needs_list_reload = true;
    }

    if state.needs_list_reload && !state.busy {
        state.needs_list_reload = false;
        state.spawn_list(root);
    }

    // Process handoff after list is ready (or even before).
    if let Some((cid, iid)) = state.pending_handoff.take() {
        if !state.busy {
            state.spawn_stream(root, cid, Some(iid));
        } else {
            state.pending_handoff = Some((cid, iid));
        }
    }

    ui.horizontal(|ui| {
        ui.heading("Conversations");
        ui.label(
            RichText::new("Day-bounded UTC · filters badge hits, do not hide neighbors")
                .small()
                .color(Color32::from_rgb(120, 120, 120)),
        );
        if ui
            .add_enabled(!state.busy, egui::Button::new("Refresh"))
            .clicked()
        {
            state.needs_list_reload = true;
        }
        if state.busy {
            ui.spinner();
        }
    });

    if let Some(err) = &state.error {
        ui.colored_label(Color32::from_rgb(200, 80, 80), err);
    }
    if let Some(st) = &state.status {
        ui.label(st.as_str());
    }

    ui.add_space(4.0);

    // Bulk confirm dialog
    if let Some(confirm) = state.bulk_confirm.clone() {
        egui::Window::new("Code entire day bucket")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "Apply code(s) [{}] to ALL {} message(s) in conversation:",
                    confirm.payload.code_labels.join(", "),
                    confirm.payload.message_count
                ));
                ui.monospace(&confirm.payload.conversation_id);
                ui.label(
                    RichText::new(
                        "This is explicit bulk coding for one day bucket only. Not silent.",
                    )
                    .small()
                    .color(Color32::from_rgb(160, 100, 40)),
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        state.bulk_confirm = None;
                    }
                    if ui
                        .add_enabled(!state.coding_busy, egui::Button::new("Confirm bulk code"))
                        .clicked()
                    {
                        let codes = confirm.code_ids.clone();
                        state.spawn_code_bucket(root, actor, codes);
                    }
                });
            });
    }

    egui::Panel::left("conv_list_panel")
        .resizable(true)
        .default_size(280.0)
        .min_size(180.0)
        .show_inside(ui, |ui| {
            ui.label(RichText::new("Conversations").strong());
            if state.active_hit_ids.is_some() {
                ui.label(
                    RichText::new("Showing buckets with ≥1 hit")
                        .small()
                        .color(Color32::from_rgb(80, 120, 180)),
                );
            }
            ui.separator();
            let row_h = ROW_HEIGHT + 18.0;
            let n = state.conversations.len();
            let mut open_cid: Option<String> = None;
            egui::ScrollArea::vertical()
                .id_salt("conv_list_scroll")
                .auto_shrink([false, false])
                .show_rows(ui, row_h, n, |ui, range| {
                    for i in range {
                        let c = &state.conversations[i];
                        let selected = state.selected_conversation.as_deref()
                            == Some(c.conversation_id.as_str());
                        let label = format_conversation_label(c);
                        let hover = format!(
                            "id={}\n{} msgs · {} hits\n{} → {}",
                            c.conversation_id,
                            c.message_count,
                            c.hit_count,
                            c.first_at.as_deref().unwrap_or("?"),
                            c.last_at.as_deref().unwrap_or("?")
                        );
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() {
                            open_cid = Some(c.conversation_id.clone());
                        }
                        resp.on_hover_text(hover);
                    }
                });
            if let Some(cid) = open_cid {
                state.spawn_stream(root, cid, None);
            }
        });

    egui::Panel::right("conv_tools_panel")
        .resizable(true)
        .default_size(220.0)
        .min_size(160.0)
        .show_inside(ui, |ui| {
            ui.label(RichText::new("Coding").strong());
            ui.label(
                RichText::new("Code selected message, or explicitly bulk-code the day bucket.")
                    .small(),
            );
            ui.separator();

            let active: Vec<_> = state
                .code_defs
                .iter()
                .filter(|d| d.is_active != 0)
                .collect();
            for def in &active {
                let mut on = state.selected_code_ids.contains(&def.id);
                if ui.checkbox(&mut on, &def.label).changed() {
                    if on {
                        state.selected_code_ids.insert(def.id.clone());
                    } else {
                        state.selected_code_ids.remove(&def.id);
                    }
                }
            }

            ui.add_space(6.0);
            if ui
                .add_enabled(
                    !state.coding_busy && state.selected_message.is_some(),
                    egui::Button::new("Code selected message"),
                )
                .on_hover_text("Applies checked codes to the selected message only")
                .clicked()
            {
                state.spawn_code_selected(root, actor);
            }

            ui.add_space(4.0);
            // Full bucket count only — never loaded page length (P2-2 honesty).
            let msg_count = state.bucket_message_count.or_else(|| {
                state
                    .conversations
                    .iter()
                    .find(|c| {
                        Some(c.conversation_id.as_str()) == state.selected_conversation.as_deref()
                    })
                    .map(|c| c.message_count)
            });
            let labels: Vec<String> = state
                .code_defs
                .iter()
                .filter(|d| state.selected_code_ids.contains(&d.id))
                .map(|d| d.label.clone())
                .collect();
            if ui
                .add_enabled(
                    !state.coding_busy
                        && !state.busy
                        && state.selected_conversation.is_some()
                        && !state.selected_code_ids.is_empty(),
                    egui::Button::new("Code entire day bucket…"),
                )
                .on_hover_text(
                    "Requires confirm with full day-bucket message count (not the loaded page). Codes all ids in this conversation_id.",
                )
                .clicked()
            {
                let code_ids: Vec<String> = state.selected_code_ids.iter().cloned().collect();
                if let Some(payload) = bulk_code_confirm_payload(
                    state.selected_conversation.as_deref(),
                    msg_count,
                    &labels,
                ) {
                    state.bulk_confirm = Some(BulkConfirm { payload, code_ids });
                } else {
                    // Summary/count missing (e.g. handoff without list row): resolve full count first.
                    state.spawn_bulk_confirm_resolve(root, code_ids, labels);
                }
            }

            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("Body preview").strong());
            match state.body.pane() {
                BodyPane::Idle => {
                    ui.label("Select a message.");
                }
                BodyPane::Loading { .. } => {
                    ui.spinner();
                    ui.label("Loading…");
                }
                BodyPane::Ready {
                    text, truncated, ..
                } => match text {
                    Ok(t) => {
                        egui::ScrollArea::vertical()
                            .id_salt("conv_body_scroll")
                            .max_height(280.0)
                            .show(ui, |ui| {
                                ui.label(t.as_str());
                                if *truncated {
                                    ui.colored_label(
                                        Color32::from_rgb(160, 100, 40),
                                        "(truncated)",
                                    );
                                }
                            });
                    }
                    Err(e) => {
                        ui.colored_label(Color32::from_rgb(200, 80, 80), e);
                    }
                },
            }
        });

    // Center: stream
    egui::CentralPanel::default().show_inside(ui, |ui| {
        if let Some(cid) = state.selected_conversation.clone() {
            let summary = state
                .conversations
                .iter()
                .find(|c| c.conversation_id == cid);
            ui.horizontal(|ui| {
                if let Some(s) = summary {
                    ui.label(RichText::new(format_conversation_header(s)).strong());
                } else {
                    ui.label(RichText::new(format!("Conversation {cid}")).strong());
                }
            });
            ui.label(
                RichText::new(
                    "Full day-bucket stream (not hits-only). Hit badges when a filter/FTS set is active.",
                )
                .small()
                .color(Color32::from_rgb(120, 120, 120)),
            );
            ui.separator();

            let n = state.messages.len();
            let mut clicked_id: Option<String> = None;
            let mut parent_click: Option<String> = None;

            // One-shot scroll: handoff centers selected row; load-earlier preserves viewport.
            if state.scroll_to_selected {
                if let Some(sel) = &state.selected_message {
                    if let Some(idx) = state.messages.iter().position(|m| m.id == *sel) {
                        state.pending_scroll_offset = Some(idx as f32 * STREAM_ROW_HEIGHT);
                    }
                }
                state.scroll_to_selected = false;
            }
            let mut scroll_area = egui::ScrollArea::vertical()
                .id_salt("conv_stream_scroll")
                .auto_shrink([false, false]);
            if let Some(y) = state.pending_scroll_offset.take() {
                scroll_area = scroll_area.vertical_scroll_offset(y);
            }
            let scroll = scroll_area.show_rows(ui, STREAM_ROW_HEIGHT, n, |ui, range| {
                for i in range {
                    let m = &state.messages[i];
                    let is_sel = state.selected_message.as_deref() == Some(m.id.as_str());
                    let is_hit = state.hit_ids.contains(&m.id);
                    let fill = if is_sel {
                        Color32::from_rgb(40, 70, 110)
                    } else if is_hit {
                        Color32::from_rgb(70, 55, 30)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), STREAM_ROW_HEIGHT),
                        Sense::click(),
                    );
                    if fill != Color32::TRANSPARENT {
                        ui.painter().rect_filled(rect, 2.0, fill);
                    }
                    let mut y = rect.top() + 2.0;
                    if m.parent_item_id.is_some() {
                        let chrome = reply_chrome_line(m.reply_snippet.as_deref());
                        let chrome_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.left() + 8.0, y),
                            egui::vec2(rect.width() - 16.0, 14.0),
                        );
                        ui.painter().text(
                            chrome_rect.left_top(),
                            egui::Align2::LEFT_TOP,
                            chrome,
                            egui::FontId::proportional(11.0),
                            Color32::from_rgb(160, 160, 200),
                        );
                        // Click in chrome area selects parent when present.
                        if resp.clicked()
                            && ui.input(|i| {
                                i.pointer
                                    .interact_pos()
                                    .is_some_and(|p| chrome_rect.contains(p))
                            })
                        {
                            if let Some(ref pid) = m.parent_item_id {
                                parent_click = Some(pid.clone());
                            }
                        }
                        y += 14.0;
                    }
                    let ts = m.sent_at.as_deref().unwrap_or("—");
                    let from = m.from_addr.as_deref().unwrap_or("?");
                    let subj = m.subject.as_deref().unwrap_or("");
                    let mut line = format!("{ts}  {from}  {subj}");
                    if is_hit {
                        line = format!("[Hit] {line}");
                    }
                    ui.painter().text(
                        egui::pos2(rect.left() + 8.0, y),
                        egui::Align2::LEFT_TOP,
                        line,
                        egui::FontId::proportional(13.0),
                        if is_hit {
                            Color32::from_rgb(240, 200, 100)
                        } else {
                            Color32::from_rgb(210, 210, 210)
                        },
                    );
                    if resp.clicked() && parent_click.is_none() {
                        clicked_id = Some(m.id.clone());
                    }
                }
            });
            state.last_scroll_offset_y = scroll.state.offset.y;

            if let Some(pid) = parent_click {
                if state.messages.iter().any(|m| m.id == pid) {
                    state.selected_message = Some(pid.clone());
                    state.scroll_to_selected = true;
                    spawn_body_for_selection(state, root, ui.ctx());
                } else if let Some(cid) = state.selected_conversation.clone() {
                    // Parent not in page — centered load around parent.
                    state.spawn_stream(root, cid, Some(pid));
                }
            } else if let Some(id) = clicked_id {
                state.selected_message = Some(id);
                spawn_body_for_selection(state, root, ui.ctx());
            }

            // Ensure body loads when selection set programmatically (handoff).
            if state.selected_message.is_some() {
                let needs_body = matches!(state.body.pane(), BodyPane::Idle)
                    || matches!(state.body.pane(), BodyPane::Ready { item_id, .. } if Some(item_id.as_str()) != state.selected_message.as_deref())
                    || matches!(state.body.pane(), BodyPane::Loading { item_id, .. } if Some(item_id.as_str()) != state.selected_message.as_deref());
                if needs_body {
                    spawn_body_for_selection(state, root, ui.ctx());
                }
            }

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !state.busy && !state.messages.is_empty(),
                        egui::Button::new("Load earlier"),
                    )
                    .on_hover_text("Older page of this day bucket (before-keyset, prepends)")
                    .clicked()
                {
                    state.spawn_load_earlier(root);
                }
                if ui
                    .add_enabled(
                        !state.busy && !state.messages.is_empty(),
                        egui::Button::new("Load more"),
                    )
                    .on_hover_text("Newer page of this day bucket (after-keyset, appends)")
                    .clicked()
                {
                    state.spawn_load_more(root);
                }
                let total = state
                    .bucket_message_count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".into());
                ui.label(format!("{} of {} shown", state.messages.len(), total));
            });
        } else {
            ui.label("Select a conversation from the left list.");
            ui.label(
                RichText::new(
                    "Conversations are UTC day-bounded (0055). Multi-day channels appear as multiple rows. \
                     Linear Review remains available for non-chat workflows.",
                )
                .small(),
            );
        }
    });
}

fn spawn_body_for_selection(state: &mut ConversationState, root: &Utf8Path, ctx: &egui::Context) {
    let Some(id) = state.selected_message.clone() else {
        return;
    };
    let Some(row) = state.messages.iter().find(|m| m.id == id) else {
        return;
    };
    state.body.spawn_load(
        ctx,
        root,
        id,
        row.text_sha256.clone(),
        row.html_sha256.clone(),
    );
}

fn format_conversation_label(c: &ConversationSummary) -> String {
    let team = c.team_name.as_deref().unwrap_or("—");
    let ch = c.channel_name.as_deref().unwrap_or("—");
    let ty = c.chat_type.as_deref().unwrap_or("?");
    let day = c.bucket_date.as_deref().unwrap_or("?");
    let hits = if c.hit_count > 0 {
        format!(" · {} hits", c.hit_count)
    } else {
        String::new()
    };
    format!(
        "{team} / {ch} [{ty}] {day} · {} msgs{hits}",
        c.message_count
    )
}

fn format_conversation_header(c: &ConversationSummary) -> String {
    let team = c.team_name.as_deref().unwrap_or("—");
    let ch = c.channel_name.as_deref().unwrap_or("—");
    let ty = c.chat_type.as_deref().unwrap_or("?");
    let day = c.bucket_date.as_deref().unwrap_or("?");
    format!(
        "{team} / {ch} · {ty} · bucket {day} · {} messages",
        c.message_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_confirm_payload_requires_codes_and_count() {
        assert!(bulk_code_confirm_payload(Some("c1"), Some(3), &[]).is_none());
        assert!(bulk_code_confirm_payload(Some("c1"), Some(0), &["Hot".into()]).is_none());
        assert!(bulk_code_confirm_payload(None, Some(3), &["Hot".into()]).is_none());
        // Honesty: message_count is the full day-bucket size, never the loaded page length.
        // UI resolves via summary.message_count / bucket_message_count / list_conversation_item_ids.
        let p = bulk_code_confirm_payload(Some("c1"), Some(12), &["Hot".into(), "Resp".into()])
            .expect("payload");
        assert_eq!(p.message_count, 12);
        assert_eq!(p.conversation_id, "c1");
        assert_eq!(p.code_labels.len(), 2);
        // Page length of 3 must not be treated as the bulk target when full count is 12.
        assert_ne!(p.message_count, 3);
    }

    #[test]
    fn hit_badge_only_intersection() {
        let page = vec!["a".into(), "b".into(), "c".into()];
        let hits: HashSet<String> = ["b".into(), "z".into()].into_iter().collect();
        let badges = hit_badge_ids(&page, Some(&hits));
        assert_eq!(badges, ["b".into()].into_iter().collect());
        assert!(hit_badge_ids(&page, None).is_empty());
        assert!(hit_badge_ids(&page, Some(&HashSet::new())).is_empty());
    }

    #[test]
    fn reply_chrome_unavailable() {
        assert_eq!(
            reply_chrome_line(None),
            format!("In reply to: {REPLY_SNIPPET_UNAVAILABLE}")
        );
        assert_eq!(
            reply_chrome_line(Some("hello world")),
            "In reply to: hello world"
        );
    }
}
