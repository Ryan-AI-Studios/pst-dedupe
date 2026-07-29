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
    AttachStreamSource, CanonicalAttachment, CanonicalMessage, EmlWriteError, FamilyPolicy,
    IntegrityReason, MaterializeError, MessageLocus, MessageMaterializer,
};
use pst_reader::{NodeId, PstFile};

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

                    // Prefer phase-1b cache (no re-I/O). Cache miss keeps optimistic.
                    let mut applied_cache = false;
                    if !parents_only {
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

                    if !parents_only && !applied_cache && deep_level.is_some() {
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
pub struct PstAttachStreamSource {
    handles: SharedPstHandleCache,
}

impl PstAttachStreamSource {
    pub fn new() -> Self {
        Self {
            handles: Rc::new(RefCell::new(PstHandleCache::new(DEFAULT_MAX_OPEN_PSTS))),
        }
    }

    /// Share a handle cache with [`PstMaterializer`] (preferred unique-pst path).
    pub fn with_handle_cache(handles: SharedPstHandleCache) -> Self {
        Self { handles }
    }

    pub fn source_pst_opens(&self) -> u64 {
        self.handles.borrow().opens()
    }

    pub fn handle_cache(&self) -> SharedPstHandleCache {
        Rc::clone(&self.handles)
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
    pub fn open_attachment_data_reader(
        &mut self,
        parent: &MessageLocus,
        attach_nid: u64,
    ) -> Result<pst_reader::AttachmentDataReader, EmlWriteError> {
        let mut handles = self.handles.borrow_mut();
        let pst = handles
            .get_mut(&parent.source_path)
            .map_err(|e| EmlWriteError::Other(format!("open attach stream pst: {e}")))?;
        pst.open_attachment_data(NodeId(parent.nid), NodeId(attach_nid))
            .map_err(|e| {
                EmlWriteError::Other(format!(
                    "open_attachment_data parent={:#x} attach={attach_nid:#x}: {e}",
                    parent.nid
                ))
            })
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
