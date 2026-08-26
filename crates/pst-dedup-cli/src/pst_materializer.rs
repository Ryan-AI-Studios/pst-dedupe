//! Shared PST materializer + attach stream source for keep-set / unique-eml.
//!
//! Source PSTs are opened read-only. Large attach payloads are never loaded into
//! multi-GB `Vec`s — exporters stream via [`PstAttachStreamSource`].
//!
//! Track **0079**: materializer and attach stream source share one bounded LRU
//! [`PstHandleCache`] via `Rc<RefCell<…>>` (closes D-0074-mat-lru).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dedup_engine::attach_reason_from_pst_error;
use dedup_engine::reason_from_pst_error;
use dedup_engine::{
    AttachStreamSource, CanonicalAttachment, CanonicalMessage, CanonicalRecipient, EmlWriteError,
    FamilyPolicy, IntegrityReason, MaterializeError, MessageLocus, MessageMaterializer,
    NestedCanonicalMessage, NestedExtractFail,
};
use pst_reader::{
    EmbeddedExportAttach, EmbeddedExportFields, MessageNodeRef, NodeId, PstError, PstFile,
    ATTACH_EMBEDDED_MSG, MAX_NESTED_EXPORT_PAYLOAD_BYTES,
};

use crate::attach_probe::{path_mtime_and_size, probe_attach_stream, ProbeLevel, ProbeResultCache};

/// Default max open source PST handles (matches probe path; 0079 §3.6).
pub const DEFAULT_MAX_OPEN_PSTS: usize = 32;

/// Optional soft-warning sink (GUI Log panel / CLI on_log bridge).
pub type MaterializeWarnCb = Arc<Mutex<dyn FnMut(String) + Send>>;

/// Bounded LRU of open read-only PST handles shared by materialize + attach stream.
///
/// Evicts least-recently-used when over capacity. Counts every successful
/// `PstFile::open` in [`Self::opens`].
pub struct PstHandleCache {
    capacity: usize,
    order: VecDeque<String>,
    map: HashMap<String, PstFile>,
    /// Cumulative successful opens (includes re-opens after eviction).
    opens: u64,
}

impl PstHandleCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            map: HashMap::new(),
            opens: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn opens(&self) -> u64 {
        self.opens
    }

    fn touch(&mut self, path: &str) {
        if let Some(pos) = self.order.iter().position(|p| p == path) {
            self.order.remove(pos);
        }
        self.order.push_back(path.to_string());
    }

    fn evict_lru(&mut self) {
        while self.map.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
    }

    /// Open or reuse a sticky handle; drops LRU when over capacity.
    pub fn get_mut(&mut self, path: &str) -> Result<&mut PstFile, String> {
        if self.map.contains_key(path) {
            self.touch(path);
            return self
                .map
                .get_mut(path)
                .ok_or_else(|| format!("pst missing after touch: {path}"));
        }
        self.evict_lru();
        let pst = PstFile::open(Path::new(path)).map_err(|e| format!("open {path}: {e}"))?;
        self.opens = self.opens.saturating_add(1);
        self.map.insert(path.to_string(), pst);
        self.touch(path);
        self.map
            .get_mut(path)
            .ok_or_else(|| format!("pst missing after open: {path}"))
    }
}

/// Shared handle cache handle (`Rc<RefCell<…>>` — single-threaded unique-pst path).
pub type SharedPstHandleCache = Rc<RefCell<PstHandleCache>>;

/// Materializer holding open PST handles (source PSTs remain read-only).
pub struct PstMaterializer {
    /// Bounded LRU of open PST files (shared with attach stream when provided).
    handles: SharedPstHandleCache,
    /// When false / parents_only, skip loading attach bytes (metadata list may still be empty).
    load_attach_payloads: bool,
    /// parents_only: still list attach metadata for omit ledger rows; never load payloads.
    parents_only: bool,
    /// Soft attach/open warnings (in addition to tracing).
    on_warn: Option<MaterializeWarnCb>,
    /// When set (0074 deep attach), verify stream open/head at materialize and set
    /// `stream_available` from the probe (not optimistic size/filename).
    deep_probe_level: Option<ProbeLevel>,
    /// Per-attach head-read budget when deep_probe_level is Head/Full (default 1 MiB).
    deep_probe_per_attach_max_bytes: u64,
    deep_probe_time_ms: u64,
    /// Cooperative cancel for deep attach probe (0074) — cancel ≠ attach fail.
    cancel: Option<Arc<AtomicBool>>,
    /// Phase-1b probe cache: set `stream_available` without re-opening streams (0074 P1-A).
    /// `Arc` so materialize can consult the cache while holding a PST handle borrow.
    probe_result_cache: Option<(Arc<ProbeResultCache>, ProbeLevel)>,
    /// Count of successful `materialize` returns (0079 D1 assertion).
    messages_materialized: u64,
}

impl PstMaterializer {
    pub fn new(family: FamilyPolicy) -> Self {
        Self::with_handle_cache(
            family,
            Rc::new(RefCell::new(PstHandleCache::new(DEFAULT_MAX_OPEN_PSTS))),
        )
    }

    /// Build with an explicit shared handle cache (unique-pst shares with attach stream).
    pub fn with_handle_cache(family: FamilyPolicy, handles: SharedPstHandleCache) -> Self {
        Self {
            handles,
            load_attach_payloads: family == FamilyPolicy::KeepAttachmentsWithParent,
            parents_only: family == FamilyPolicy::ParentsOnly,
            on_warn: None,
            deep_probe_level: None,
            deep_probe_per_attach_max_bytes: 1_048_576,
            deep_probe_time_ms: 2000,
            cancel: None,
            probe_result_cache: None,
            messages_materialized: 0,
        }
    }

    /// Shared cache handle (clone of the `Rc`).
    pub fn handle_cache(&self) -> SharedPstHandleCache {
        Rc::clone(&self.handles)
    }

    pub fn messages_materialized(&self) -> u64 {
        self.messages_materialized
    }

    pub fn source_pst_opens(&self) -> u64 {
        self.handles.borrow().opens()
    }

    /// Bridge soft attach/open warnings to a structured log sink (unique-pst GUI).
    pub fn with_warn_sink(mut self, on_warn: MaterializeWarnCb) -> Self {
        self.on_warn = Some(on_warn);
        self
    }

    /// Enable deep attach stream probe during materialize (0074). Corrects optimistic
    /// `stream_available`. L2 head success ⇒ stream_available=true (not "fully verified").
    ///
    /// Prefer [`with_probe_result_cache`] after a phase-1b budgeted pass so materialize
    /// does not re-open streams under a second budget.
    pub fn with_deep_attach_probe(
        mut self,
        level: ProbeLevel,
        per_attach_max_bytes: u64,
        max_probe_time_ms: u64,
    ) -> Self {
        self.deep_probe_level = Some(level);
        self.deep_probe_per_attach_max_bytes = per_attach_max_bytes;
        self.deep_probe_time_ms = max_probe_time_ms;
        self
    }

    /// Apply phase-1b probe outcomes at materialize without re-I/O (0074 P1-A).
    ///
    /// Cache hit at `level`: fail → `stream_available=false` (+ soft reason);
    /// ok → `stream_available=true` (non-parents_only). Miss → legacy optimistic
    /// (honest when attach_probe.truncated / unprobed peers).
    pub fn with_probe_result_cache(mut self, cache: ProbeResultCache, level: ProbeLevel) -> Self {
        self.probe_result_cache = Some((Arc::new(cache), level));
        self
    }

    /// Thread run-level cancel into deep attach probe (do not re-degrade cancel as fail).
    pub fn with_cancel(mut self, cancel: Option<Arc<AtomicBool>>) -> Self {
        self.cancel = cancel;
        self
    }
}

/// Optimistic `stream_available` for an attachment successfully returned by
/// `list_attachments` when no deep-probe or cache outcome has classified it.
///
/// Listing success implies an exportable stream handle exists **or** a valid
/// zero-byte by-value payload (including empty display name). Must not require
/// `size > 0` or a non-empty filename (§2.5 rule 5 / 0083).
#[inline]
fn optimistic_listed_stream_available(_size: u32, _filename: &str) -> bool {
    true
}

/// True hard failures that must promote peers. Everything else may soft-recover
/// via `read_message_properties` (scan already classified many of these as recoverable).
fn is_hard_structural_reason(reason: dedup_engine::IntegrityReason) -> bool {
    use dedup_engine::IntegrityReason::*;
    matches!(
        reason,
        OpenFailed
            | AnsiUnsupported
            | UnsupportedCrypt
            | FolderWalkFailed
            | NodeNotFound
            | BlockNotFound
            | PathNotFound
            | NotPst
            | ReadError
    )
}

impl MessageMaterializer for PstMaterializer {
    fn materialize(
        &mut self,
        locus: &MessageLocus,
    ) -> std::result::Result<CanonicalMessage, MaterializeError> {
        // Validates extract + attachment *metadata* for promotion honesty.
        // Large attach payloads are never loaded into Vecs; `stream_available` marks
        // that open_attachment_data can be used by downstream exporters.
        let parents_only = self.parents_only;
        let load_payloads = self.load_attach_payloads;
        let deep_level = self.deep_probe_level;
        let deep_per = self.deep_probe_per_attach_max_bytes;
        let deep_time_ms = self.deep_probe_time_ms;
        let cancel = self.cancel.clone();
        // Phase-1b cache takes precedence over live deep re-probe (no second budget).
        // Clone Arc before opening PST so we can look up while holding a handle borrow.
        let probe_cache = self.probe_result_cache.clone();
        let cache_identity = probe_cache
            .as_ref()
            .map(|_| path_mtime_and_size(&locus.source_path));
        // Clone warn sink before borrowing the handle cache.
        let warn_cb = self.on_warn.clone();
        let emit_soft = |msg: String| {
            tracing::warn!("{msg}");
            if let Some(cb) = &warn_cb {
                if let Ok(mut g) = cb.lock() {
                    g(msg);
                }
            }
        };
        // Hold the shared cache for the full materialize (single-threaded path).
        let mut handles = self.handles.borrow_mut();
        let pst = handles
            .get_mut(&locus.source_path)
            .map_err(MaterializeError::Hard)?;
        let nid = NodeId(locus.nid);

        let mut soft_reasons: Vec<dedup_engine::IntegrityReason> = Vec::new();

        // Prefer full extract; on soft body/property errors fall back to properties
        // so sole degraded winners are not ghost-dropped (§3.7 rule 3 / D-0065-soft-body).
        let (
            message_id,
            subject,
            sender,
            display_to,
            display_cc,
            display_bcc,
            recipients,
            message_flags,
            submit_time,
            size,
            message_class,
            body_plain,
            body_html,
            body_incomplete,
            body_unavailable,
        ) = match pst.read_message_extract(nid) {
            Ok(extracted) => {
                let body_unavailable =
                    extracted.body_text.is_none() && extracted.body_html.is_none();
                if body_unavailable {
                    soft_reasons.push(dedup_engine::IntegrityReason::BodyUnavailable);
                }
                // 0077: body/props block CRC → message CRC_SUSPECT (fidelity + Tier-2).
                if extracted.crc_suspect
                    && !soft_reasons.contains(&dedup_engine::IntegrityReason::CrcSuspect)
                {
                    soft_reasons.push(dedup_engine::IntegrityReason::CrcSuspect);
                }
                let recipients: Vec<dedup_engine::CanonicalRecipient> = extracted
                    .recipients
                    .iter()
                    .map(dedup_engine::CanonicalRecipient::from_reader)
                    .collect();
                (
                    extracted.message_id,
                    extracted.subject,
                    extracted.sender_email,
                    extracted.display_to,
                    extracted.display_cc,
                    extracted.display_bcc,
                    recipients,
                    extracted.message_flags,
                    extracted.submit_time,
                    extracted.message_size.map(|s| s as u32),
                    extracted.message_class,
                    extracted.body_text,
                    extracted.body_html,
                    false,
                    body_unavailable,
                )
            }
            Err(e) => {
                let reason = reason_from_pst_error(&e);
                if is_hard_structural_reason(reason) {
                    return Err(MaterializeError::Hard(format!(
                        "extract nid={:#x} {}: {e}",
                        locus.nid,
                        reason.as_str()
                    )));
                }
                match pst.read_message_properties(nid) {
                    Ok(props) => {
                        let body_incomplete = props.body_incomplete;
                        soft_reasons.push(dedup_engine::IntegrityReason::BodyUnavailable);
                        if body_incomplete
                            && !soft_reasons.contains(&dedup_engine::IntegrityReason::BodyTruncated)
                        {
                            soft_reasons.push(dedup_engine::IntegrityReason::BodyTruncated);
                        }
                        if !soft_reasons.contains(&reason) {
                            soft_reasons.push(reason);
                        }
                        // 0077: props-path block CRC → message CRC_SUSPECT.
                        if props.crc_suspect
                            && !soft_reasons.contains(&dedup_engine::IntegrityReason::CrcSuspect)
                        {
                            soft_reasons.push(dedup_engine::IntegrityReason::CrcSuspect);
                        }
                        let recipients: Vec<dedup_engine::CanonicalRecipient> = props
                            .recipients
                            .iter()
                            .map(dedup_engine::CanonicalRecipient::from_reader)
                            .collect();
                        (
                            props.message_id,
                            props.subject,
                            props.sender_email,
                            props.display_to,
                            props.display_cc,
                            props.display_bcc,
                            recipients,
                            props.message_flags,
                            props.submit_time,
                            props.message_size.map(|s| s as u32),
                            None,
                            None,
                            None,
                            body_incomplete,
                            true,
                        )
                    }
                    Err(e2) => {
                        let r2 = reason_from_pst_error(&e2);
                        if is_hard_structural_reason(r2) {
                            return Err(MaterializeError::Hard(format!(
                                "extract+props nid={:#x} {}: {e2}",
                                locus.nid,
                                r2.as_str()
                            )));
                        }
                        soft_reasons.push(dedup_engine::IntegrityReason::BodyUnavailable);
                        if !soft_reasons.contains(&r2) {
                            soft_reasons.push(r2);
                        }
                        (
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            Vec::new(),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            false,
                            true,
                        )
                    }
                }
            }
        };

        let mut attachments = Vec::new();
        // Always list attachment *metadata* (including parents_only) so the writer can emit
        // ATTACH_OMITTED_BY_POLICY info rows / ATTACH_META_FAILED when list fails.
        // Payloads are never loaded under parents_only; under keep-with-parent only small
        // probes are optionally buffered (large streams stay on AttachStreamSource).
        match pst.list_attachments(nid) {
            Ok(list) => {
                // Cap optional small-payload probe so we never materialize multi-GB Vecs.
                const SMALL_ATTACH_CAP: u32 = 64 * 1024;
                for att in list {
                    let mut data = None;
                    // Metadata list always (incl. parents_only) for omit ledger rows.
                    // Do **not** force stream_available=false under parents_only: policy omit
                    // ≠ attach fail (0073), and Mode A incomplete (0083) must not treat
                    // parents_only omit as attach-incomplete.
                    // Default (no deep probe): optimistically available for any attach
                    // successfully returned by list_attachments — including zero-byte
                    // by-value with empty filename (§2.5 rule 5 / 0083). Do **not** gate
                    // on size>0 or non-empty name (that falsely marked empty zero-byte
                    // attaches incomplete and spuriously promoted Mode A).
                    // Deep probe / cache (0074): may set false on fail — never claim
                    // exportable after probe fail.
                    let mut stream_available =
                        optimistic_listed_stream_available(att.size, &att.filename);

                    // 0084: attachment-table CloudLink → incomplete; prefer ATTACH_CLOUD_LINK.
                    // Explicit is_cloud_link keeps parents_only omit distinct from cloud fail.
                    if att.is_cloud_link {
                        stream_available = false;
                        if !soft_reasons.contains(&IntegrityReason::AttachCloudLink) {
                            soft_reasons.push(IntegrityReason::AttachCloudLink);
                        }
                    }

                    // Prefer phase-1b cache (no re-I/O). Cache miss keeps optimistic.
                    // Skip deep probe for CloudLink (no offline payload to verify).
                    // Skip method-5: binary open_attachment_data cannot open embeds;
                    // nested extract later fills embedded_message (0094 P2).
                    let is_method5 = att.attach_method == Some(ATTACH_EMBEDDED_MSG);
                    let mut applied_cache = false;
                    if !parents_only && !att.is_cloud_link && !is_method5 {
                        if let (Some((cache, level)), Some((mtime, source_size))) =
                            (probe_cache.as_ref(), cache_identity)
                        {
                            if let Some(outcome) = cache.get(
                                &locus.source_path,
                                locus.nid,
                                att.nid.0,
                                att.size,
                                mtime,
                                source_size,
                                *level,
                            ) {
                                applied_cache = true;
                                stream_available = outcome.ok;
                                if !outcome.ok {
                                    let reason = outcome
                                        .reason
                                        .unwrap_or(IntegrityReason::AttachStreamOpenFailed);
                                    emit_soft(format!(
                                        "deep attach probe cache fail (soft {}) nid={:#x} attach_nid={:#x}",
                                        reason.as_str(),
                                        locus.nid,
                                        att.nid.0
                                    ));
                                    if !soft_reasons.contains(&reason) {
                                        soft_reasons.push(reason);
                                    }
                                } else if outcome.reason == Some(IntegrityReason::CrcSuspect)
                                    && !soft_reasons.contains(&IntegrityReason::CrcSuspect)
                                {
                                    soft_reasons.push(IntegrityReason::CrcSuspect);
                                }
                            }
                        }
                    }

                    if !parents_only
                        && !att.is_cloud_link
                        && !is_method5
                        && !applied_cache
                        && deep_level.is_some()
                    {
                        // Cancel mid-materialize: do not re-degrade as attach fail.
                        let cancelled_now = cancel
                            .as_ref()
                            .map(|c| c.load(Ordering::SeqCst))
                            .unwrap_or(false);
                        if cancelled_now {
                            // Leave optimistic stream_available unset for export honesty.
                            stream_available = false;
                        } else {
                            let level = deep_level.unwrap_or(ProbeLevel::Head);
                            let deadline = std::time::Instant::now()
                                + std::time::Duration::from_millis(deep_time_ms);
                            let outcome = probe_attach_stream(
                                pst, nid, att.nid, level, deep_per, deep_per, deadline, &cancel,
                            );
                            let cancelled_after = cancel
                                .as_ref()
                                .map(|c| c.load(Ordering::SeqCst))
                                .unwrap_or(false);
                            if cancelled_after {
                                // Cancel outcome is non-fail (ok=true, reason=None); no soft degrade.
                                stream_available = false;
                            } else {
                                stream_available = outcome.ok;
                                if !outcome.ok {
                                    let reason = outcome
                                        .reason
                                        .unwrap_or(IntegrityReason::AttachStreamOpenFailed);
                                    emit_soft(format!(
                                        "deep attach probe failed (soft {}) nid={:#x} attach_nid={:#x}",
                                        reason.as_str(),
                                        locus.nid,
                                        att.nid.0
                                    ));
                                    if !soft_reasons.contains(&reason) {
                                        soft_reasons.push(reason);
                                    }
                                } else if outcome.reason == Some(IntegrityReason::CrcSuspect) {
                                    // 0077: attach stream block CRC is warning-only but
                                    // message-level CRC_SUSPECT so Tier-2 / fidelity guards fire.
                                    if !soft_reasons.contains(&IntegrityReason::CrcSuspect) {
                                        soft_reasons.push(IntegrityReason::CrcSuspect);
                                    }
                                }
                            }
                        }
                    } else if !parents_only
                        && !att.is_cloud_link
                        && !is_method5
                        && !applied_cache
                        && load_payloads
                        && att.size > 0
                        && att.size <= SMALL_ATTACH_CAP
                    {
                        match pst.open_attachment_data(nid, att.nid) {
                            Ok(mut reader) => {
                                let mut buf = Vec::new();
                                match reader.read_to_end(&mut buf) {
                                    Ok(_) => {
                                        data = Some(buf);
                                        // 0077: consume AttachmentDataReader::crc_suspect
                                        // into message integrity (DoD-19 attach stream).
                                        if reader.crc_suspect()
                                            && !soft_reasons.contains(&IntegrityReason::CrcSuspect)
                                        {
                                            soft_reasons.push(IntegrityReason::CrcSuspect);
                                        }
                                    }
                                    Err(e) => {
                                        emit_soft(format!(
                                            "open/read attachment payload failed (soft ATTACH_STREAM_READ_FAILED) nid={:#x} attach_nid={:#x}: {e}",
                                            locus.nid, att.nid.0
                                        ));
                                        let reason = IntegrityReason::AttachStreamReadFailed;
                                        if !soft_reasons.contains(&reason) {
                                            soft_reasons.push(reason);
                                        }
                                        // CRC during partial stream read still taints.
                                        if reader.crc_suspect()
                                            && !soft_reasons.contains(&IntegrityReason::CrcSuspect)
                                        {
                                            soft_reasons.push(IntegrityReason::CrcSuspect);
                                        }
                                        stream_available = false;
                                    }
                                }
                            }
                            Err(e) => {
                                let reason = attach_reason_from_pst_error(&e);
                                emit_soft(format!(
                                    "open_attachment_data failed (soft {}) nid={:#x} attach_nid={:#x}: {e}",
                                    reason.as_str(),
                                    locus.nid,
                                    att.nid.0
                                ));
                                if !soft_reasons.contains(&reason) {
                                    soft_reasons.push(reason);
                                }
                                stream_available = false;
                            }
                        }
                    }
                    attachments.push(CanonicalAttachment {
                        filename: att.filename,
                        size: att.size,
                        mime: att.mime_tag,
                        data,
                        stream_available,
                        attach_nid: Some(att.nid.0),
                        attach_method: att.attach_method,
                        is_cloud_link: att.is_cloud_link,
                        cloud_provider: att.cloud_provider,
                        cloud_url: att.cloud_url,
                        cloud_permission_type: att.cloud_permission_type,
                        embedded_message: None,
                        embedded_extract_limit: false,
                    });
                }
            }
            Err(e) => {
                emit_soft(format!(
                    "list_attachments failed during materialize (soft ATTACH_META_FAILED) nid={:#x}: {e}",
                    locus.nid
                ));
                soft_reasons.push(dedup_engine::IntegrityReason::AttachMetaFailed);
            }
        }

        let fidelity = if soft_reasons.is_empty() {
            dedup_engine::integrity::RecoverableIntegrity::clean()
        } else {
            dedup_engine::integrity::RecoverableIntegrity::with_degraded(
                soft_reasons,
                locus.is_orphaned,
            )
        };

        // Drop handle borrow before mutating materializer counters.
        drop(handles);
        self.messages_materialized = self.messages_materialized.saturating_add(1);

        Ok(CanonicalMessage {
            locus: locus.clone(),
            message_id,
            subject,
            sender,
            display_to,
            display_cc,
            display_bcc,
            recipients,
            message_flags,
            submit_time,
            size,
            message_class,
            body_plain,
            body_html,
            attachments,
            fidelity,
            message_id_norm: None,
            content_hash: [0; 32],
            edrm_mih_hex: None,
            body_incomplete,
            body_unavailable,
        })
    }
}

/// Attach stream source sharing the materializer's [`PstHandleCache`] (0079).
///
/// Previously a separate unbounded `HashMap` (D-0074-mat-lru / D4 double-open).
/// When built via [`PstAttachStreamSource::with_handle_cache`], opens reuse the
/// same sticky handles as materialize.
///
/// **0094:** [`Self::register_message_node`] records nested [`MessageNodeRef`]s so
/// child by-value attaches under nests use `open_attach_data_from_message_node`
/// (nested NIDs are not in the NBT). Keys are `(source_path, nid)` — NIDs are
/// only unique within a single store.
pub struct PstAttachStreamSource {
    pub(crate) handles: SharedPstHandleCache,
    /// Nested (and optionally top-level) message roots keyed by `(source_path, nid)`.
    pub(crate) message_nodes: HashMap<(String, u64), MessageNodeRef>,
}

impl PstAttachStreamSource {
    pub fn new() -> Self {
        Self {
            handles: Rc::new(RefCell::new(PstHandleCache::new(DEFAULT_MAX_OPEN_PSTS))),
            message_nodes: HashMap::new(),
        }
    }

    /// Share a handle cache with [`PstMaterializer`] (preferred unique-pst path).
    pub fn with_handle_cache(handles: SharedPstHandleCache) -> Self {
        Self {
            handles,
            message_nodes: HashMap::new(),
        }
    }

    pub fn source_pst_opens(&self) -> u64 {
        self.handles.borrow().opens()
    }

    pub fn handle_cache(&self) -> SharedPstHandleCache {
        Rc::clone(&self.handles)
    }

    /// Register a message root for nested child-attach streaming (0094).
    pub fn register_message_node(&mut self, source_path: &str, node: MessageNodeRef) {
        self.message_nodes
            .insert((source_path.to_string(), node.nid.0), node);
    }

    /// Lookup a registered message root (test / diagnostics).
    pub fn lookup_message_node(&self, source_path: &str, nid: u64) -> Option<MessageNodeRef> {
        self.message_nodes
            .get(&(source_path.to_string(), nid))
            .copied()
    }
}

impl Default for PstAttachStreamSource {
    fn default() -> Self {
        Self::new()
    }
}

impl PstAttachStreamSource {
    /// Open a concrete [`pst_reader::AttachmentDataReader`] (preserves `crc_suspect`).
    ///
    /// Prefer this over [`AttachStreamSource::open_attach`] when the consumer must
    /// observe late warning-only CRC after stream complete (unique-pst writer path).
    ///
    /// When `(parent.source_path, parent.nid)` was registered via
    /// [`Self::register_message_node`], opens via `open_attach_data_from_message_node`
    /// (nested parents are not in the NBT).
    pub fn open_attachment_data_reader(
        &mut self,
        parent: &MessageLocus,
        attach_nid: u64,
    ) -> Result<pst_reader::AttachmentDataReader, EmlWriteError> {
        let nested = self
            .message_nodes
            .get(&(parent.source_path.clone(), parent.nid))
            .copied();
        let mut handles = self.handles.borrow_mut();
        let pst = handles
            .get_mut(&parent.source_path)
            .map_err(|e| EmlWriteError::Other(format!("open attach stream pst: {e}")))?;
        if let Some(root) = nested {
            return pst
                .open_attach_data_from_message_node(&root, NodeId(attach_nid))
                .map_err(|e| {
                    EmlWriteError::Other(format!(
                        "open_attach_data_from_message_node parent={:#x} attach={attach_nid:#x}: {e}",
                        parent.nid
                    ))
                });
        }
        pst.open_attachment_data(NodeId(parent.nid), NodeId(attach_nid))
            .map_err(|e| {
                EmlWriteError::Other(format!(
                    "open_attachment_data parent={:#x} attach={attach_nid:#x}: {e}",
                    parent.nid
                ))
            })
    }
}

/// Winner-only method-5 nested extract into [`CanonicalAttachment::embedded_message`] (0094).
///
/// `max_embedded_depth` must match writer `WritePstOpts::max_embedded_depth` (clamped 1–8).
/// Depth/byte-budget exhaustion sets [`CanonicalAttachment::embedded_extract_limit`] so the
/// writer emits `ATTACH_DEPTH_LIMIT` rather than unparsed.
pub fn materialize_nested_for_winner(
    attach_src: &mut PstAttachStreamSource,
    msg: &mut CanonicalMessage,
    max_embedded_depth: u32,
) -> Result<(), String> {
    let max_depth = max_embedded_depth.clamp(1, 8);
    let source_path = msg.locus.source_path.clone();
    let parent_nid = msg.locus.nid;
    // Ensure parent NBT node is available for resolve; nested nodes registered below.
    {
        let mut handles = attach_src.handles.borrow_mut();
        let pst = handles
            .get_mut(&source_path)
            .map_err(|e| format!("nested extract open pst: {e}"))?;
        if let Ok(root) = pst.message_node_from_nbt(NodeId(parent_nid)) {
            drop(handles);
            attach_src.register_message_node(&source_path, root);
        }
    }
    for att in &mut msg.attachments {
        fill_nested_on_attach(attach_src, &source_path, parent_nid, att, max_depth)?;
    }
    Ok(())
}

fn fill_nested_on_attach(
    attach_src: &mut PstAttachStreamSource,
    source_path: &str,
    parent_msg_nid: u64,
    att: &mut CanonicalAttachment,
    remaining_depth: u32,
) -> Result<(), String> {
    if att.attach_method != Some(ATTACH_EMBEDDED_MSG) {
        return Ok(());
    }
    let Some(attach_nid) = att.attach_nid else {
        return Ok(());
    };
    if remaining_depth == 0 {
        att.embedded_extract_limit = true;
        att.embedded_message = None;
        return Ok(());
    }
    let parent = attach_src
        .message_nodes
        .get(&(source_path.to_string(), parent_msg_nid))
        .copied()
        .ok_or_else(|| {
            format!("missing MessageNodeRef for parent {source_path} nid={parent_msg_nid:#x}")
        })?;

    let extract_result = {
        let mut handles = attach_src.handles.borrow_mut();
        let pst = handles
            .get_mut(source_path)
            .map_err(|e| format!("nested extract pst: {e}"))?;
        match pst.resolve_embedded_root(&parent, NodeId(attach_nid)) {
            Ok(nested_root) => {
                match pst.read_export_from_message_node(
                    &nested_root,
                    remaining_depth.saturating_sub(1),
                    MAX_NESTED_EXPORT_PAYLOAD_BYTES,
                ) {
                    Ok(fields) => Ok((nested_root, fields)),
                    Err(PstError::ResourceLimit(_)) => Err(NestedExtractFail::DepthLimit),
                    Err(_) => Err(NestedExtractFail::Unparsed),
                }
            }
            Err(_) => Err(NestedExtractFail::Unparsed),
        }
    };

    match extract_result {
        Ok((_nested_root, fields)) => {
            register_export_tree(attach_src, source_path, &fields);
            att.embedded_extract_limit = false;
            att.embedded_message = Some(Box::new(map_export_to_nested(&fields)));
        }
        Err(NestedExtractFail::DepthLimit) => {
            att.embedded_extract_limit = true;
            att.embedded_message = None;
        }
        Err(NestedExtractFail::Unparsed) => {
            att.embedded_extract_limit = false;
            att.embedded_message = None;
        }
    }
    Ok(())
}

fn register_export_tree(
    attach_src: &mut PstAttachStreamSource,
    source_path: &str,
    fields: &EmbeddedExportFields,
) {
    attach_src.register_message_node(
        source_path,
        MessageNodeRef {
            nid: fields.source_msg_nid,
            bid_data: fields.bid_data,
            bid_sub: fields.bid_sub,
        },
    );
    for child in &fields.attachments {
        if let Some(ref emb) = child.embedded {
            register_export_tree(attach_src, source_path, emb);
        }
    }
}

fn map_export_to_nested(fields: &EmbeddedExportFields) -> NestedCanonicalMessage {
    NestedCanonicalMessage {
        subject: fields.subject.clone(),
        sender: fields.sender.clone(),
        display_to: fields.display_to.clone(),
        display_cc: fields.display_cc.clone(),
        display_bcc: fields.display_bcc.clone(),
        recipients: fields
            .recipients
            .iter()
            .map(CanonicalRecipient::from_reader)
            .collect(),
        message_id: fields.message_id.clone(),
        message_class: fields.message_class.clone(),
        message_flags: fields.message_flags,
        submit_time: fields.submit_time,
        body_plain: fields.body_plain.clone(),
        body_html: fields.body_html.clone(),
        attachments: fields.attachments.iter().map(map_export_attach).collect(),
        body_incomplete: fields.body_incomplete,
        body_unavailable: fields.body_unavailable,
        attachments_incomplete: fields.attachments_incomplete,
        source_msg_nid: Some(fields.source_msg_nid.0),
    }
}

fn map_export_attach(a: &EmbeddedExportAttach) -> CanonicalAttachment {
    CanonicalAttachment {
        filename: a.filename.clone(),
        size: a.size,
        mime: a.mime_tag.clone(),
        data: None,
        stream_available: a.stream_available,
        attach_nid: Some(a.nid.0),
        attach_method: a.attach_method,
        is_cloud_link: a.is_cloud_link,
        cloud_provider: None,
        cloud_url: None,
        cloud_permission_type: None,
        embedded_message: a
            .embedded
            .as_ref()
            .map(|e| Box::new(map_export_to_nested(e))),
        embedded_extract_limit: a.embedded_depth_limited,
    }
}

impl AttachStreamSource for PstAttachStreamSource {
    /// Open attachment binary stream (including embedded ATTACH_EMBEDDED_MSG when
    /// `open_attachment_data` can yield bytes).
    ///
    /// Soft failure: returns `Err` so the pack writer **skips** the MIME part (no fake
    /// body). Full nested MAPI re-parse of embedded messages remains residual
    /// `D-0067-embedded-depth`.
    fn open_attach(
        &mut self,
        parent: &MessageLocus,
        attach_nid: u64,
    ) -> Result<Box<dyn Read>, EmlWriteError> {
        let reader = self.open_attachment_data_reader(parent, attach_nid)?;
        Ok(Box::new(reader))
    }
}

#[cfg(test)]
mod handle_cache_tests {
    use super::*;
    use dedup_engine::integrity::RecoverableIntegrity;
    use dedup_engine::{is_attach_incomplete, CanonicalAttachment, CanonicalMessage, MessageLocus};

    /// Codex P2: zero-byte by-value with empty filename must stay optimistically
    /// available (not Mode-A incomplete) when listing succeeded and no probe failed.
    #[test]
    fn zero_byte_empty_name_listed_attach_not_incomplete() {
        // Materializer default before probe/cache overrides.
        assert!(
            optimistic_listed_stream_available(0, ""),
            "zero-byte empty-name must be stream_available"
        );
        assert!(optimistic_listed_stream_available(0, "empty.bin"));
        assert!(optimistic_listed_stream_available(10, ""));
        assert!(optimistic_listed_stream_available(10, "a.bin"));

        let locus = MessageLocus {
            source_path: "C:/a.pst".into(),
            source_pst: "a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            is_orphaned: false,
        };
        let msg = CanonicalMessage {
            locus,
            message_id: None,
            subject: Some("s".into()),
            sender: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            recipients: Vec::new(),
            message_flags: None,
            submit_time: None,
            size: Some(0),
            message_class: None,
            body_plain: Some("b".into()),
            body_html: None,
            attachments: vec![CanonicalAttachment {
                filename: String::new(),
                size: 0,
                mime: None,
                data: Some(vec![]),
                stream_available: optimistic_listed_stream_available(0, ""),
                attach_nid: Some(100),
                attach_method: Some(1), // by-value
                is_cloud_link: false,
                cloud_provider: None,
                cloud_url: None,
                cloud_permission_type: None,
                embedded_message: None,
                embedded_extract_limit: false,
            }],
            fidelity: RecoverableIntegrity::clean(),
            message_id_norm: None,
            content_hash: [0; 32],
            edrm_mih_hex: None,
            body_incomplete: false,
            body_unavailable: false,
        };
        assert!(
            !is_attach_incomplete(&msg),
            "zero-byte empty-name by-value with listed success must not be attach-incomplete"
        );
    }

    #[test]
    fn message_nodes_keyed_by_source_path_and_nid() {
        use pst_reader::BlockId;
        let mut src = PstAttachStreamSource::new();
        let a = MessageNodeRef {
            nid: NodeId(0x2004),
            bid_data: BlockId(0x11),
            bid_sub: BlockId(0x12),
        };
        let b = MessageNodeRef {
            nid: NodeId(0x2004),
            bid_data: BlockId(0x21),
            bid_sub: BlockId(0x22),
        };
        src.register_message_node(r"C:\a.pst", a);
        src.register_message_node(r"C:\b.pst", b);
        let got_a = src
            .lookup_message_node(r"C:\a.pst", 0x2004)
            .expect("a registered");
        let got_b = src
            .lookup_message_node(r"C:\b.pst", 0x2004)
            .expect("b registered");
        assert_eq!(got_a.bid_data, BlockId(0x11));
        assert_eq!(got_b.bid_data, BlockId(0x21));
        assert!(src.lookup_message_node(r"C:\missing.pst", 0x2004).is_none());
    }

    #[test]
    fn method5_skips_binary_probe_gate() {
        // Method-5 must not be treated as by-value binary for incompleteness.
        // stream_available stays optimistic after list; nested extract fills later.
        assert!(optimistic_listed_stream_available(100, "message.msg"));
        let locus = MessageLocus {
            source_path: "C:/a.pst".into(),
            source_pst: "a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            is_orphaned: false,
        };
        let msg = CanonicalMessage {
            locus,
            message_id: None,
            subject: Some("s".into()),
            sender: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            recipients: Vec::new(),
            message_flags: None,
            submit_time: None,
            size: Some(0),
            message_class: None,
            body_plain: Some("b".into()),
            body_html: None,
            attachments: vec![CanonicalAttachment {
                filename: "message.msg".into(),
                size: 64,
                mime: None,
                data: None,
                stream_available: true, // optimistic; binary probe skipped for method-5
                attach_nid: Some(100),
                attach_method: Some(ATTACH_EMBEDDED_MSG),
                is_cloud_link: false,
                cloud_provider: None,
                cloud_url: None,
                cloud_permission_type: None,
                embedded_message: None,
                embedded_extract_limit: false,
            }],
            fidelity: RecoverableIntegrity::clean(),
            message_id_norm: None,
            content_hash: [0; 32],
            edrm_mih_hex: None,
            body_incomplete: false,
            body_unavailable: false,
        };
        assert!(
            !is_attach_incomplete(&msg),
            "method-5 with optimistic stream_available must not be attach-incomplete"
        );
    }

    #[test]
    fn handle_cache_evicts_when_over_capacity() {
        // Capacity 2: open three distinct missing paths fail without growth;
        // use real fixture for open success if present.
        let sample =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
        if !sample.is_file() {
            return;
        }
        // Copy to three distinct paths so cache keys differ.
        let dir = std::env::temp_dir().join(format!("pst_handle_cache_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.pst");
        let b = dir.join("b.pst");
        let c = dir.join("c.pst");
        std::fs::copy(&sample, &a).expect("copy a");
        std::fs::copy(&sample, &b).expect("copy b");
        std::fs::copy(&sample, &c).expect("copy c");

        let mut cache = PstHandleCache::new(2);
        cache.get_mut(a.to_str().unwrap()).expect("open a");
        cache.get_mut(b.to_str().unwrap()).expect("open b");
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.opens(), 2);
        // Touch a so b is LRU.
        cache.get_mut(a.to_str().unwrap()).expect("touch a");
        cache.get_mut(c.to_str().unwrap()).expect("open c");
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.opens(), 3); // c is a new open
                                      // b should have been evicted; re-open increments opens.
        cache.get_mut(b.to_str().unwrap()).expect("reopen b");
        assert_eq!(cache.opens(), 4);
        assert_eq!(cache.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn materializer_and_attach_share_opens() {
        let sample =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
        if !sample.is_file() {
            return;
        }
        let shared = Rc::new(RefCell::new(PstHandleCache::new(8)));
        let mat = PstMaterializer::with_handle_cache(FamilyPolicy::ParentsOnly, Rc::clone(&shared));
        let attach = PstAttachStreamSource::with_handle_cache(Rc::clone(&shared));
        let path = sample.to_str().unwrap();
        shared.borrow_mut().get_mut(path).expect("open via shared");
        assert_eq!(shared.borrow().opens(), 1);
        // Attach stream reuses without re-open.
        let _ = attach.handle_cache().borrow_mut().get_mut(path);
        assert_eq!(shared.borrow().opens(), 1);
        assert_eq!(mat.source_pst_opens(), 1);
        assert_eq!(attach.source_pst_opens(), 1);
    }
}
