//! Shared PST materializer + attach stream source for keep-set / unique-eml.
//!
//! Source PSTs are opened read-only. Large attach payloads are never loaded into
//! multi-GB `Vec`s — exporters stream via [`PstAttachStreamSource`].

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

use dedup_engine::reason_from_pst_error;
use dedup_engine::{
    AttachStreamSource, CanonicalAttachment, CanonicalMessage, EmlWriteError, FamilyPolicy,
    MaterializeError, MessageLocus, MessageMaterializer,
};
use pst_reader::{NodeId, PstFile};

/// Optional soft-warning sink (GUI Log panel / CLI on_log bridge).
pub type MaterializeWarnCb = Arc<Mutex<dyn FnMut(String) + Send>>;

/// Materializer holding open PST handles (source PSTs remain read-only).
pub struct PstMaterializer {
    /// Absolute path string → open file.
    psts: HashMap<String, PstFile>,
    /// When false / parents_only, skip loading attach bytes (metadata list may still be empty).
    load_attach_payloads: bool,
    /// parents_only: do not list attaches at all for payload purposes.
    parents_only: bool,
    /// Soft attach/open warnings (in addition to tracing).
    on_warn: Option<MaterializeWarnCb>,
}

impl PstMaterializer {
    pub fn new(family: FamilyPolicy) -> Self {
        Self {
            psts: HashMap::new(),
            load_attach_payloads: family == FamilyPolicy::KeepAttachmentsWithParent,
            parents_only: family == FamilyPolicy::ParentsOnly,
            on_warn: None,
        }
    }

    /// Bridge soft attach/open warnings to a structured log sink (unique-pst GUI).
    pub fn with_warn_sink(mut self, on_warn: MaterializeWarnCb) -> Self {
        self.on_warn = Some(on_warn);
        self
    }

    fn open_pst(&mut self, path: &str) -> std::result::Result<&mut PstFile, MaterializeError> {
        if !self.psts.contains_key(path) {
            let pst = PstFile::open(Path::new(path))
                .map_err(|e| MaterializeError::Hard(format!("open {}: {e}", path)))?;
            self.psts.insert(path.to_string(), pst);
        }
        self.psts
            .get_mut(path)
            .ok_or_else(|| MaterializeError::Hard(format!("pst missing after open: {path}")))
    }
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
        // Clone warn sink before opening PST (pst holds &mut self.psts).
        let warn_cb = self.on_warn.clone();
        let emit_soft = |msg: String| {
            tracing::warn!("{msg}");
            if let Some(cb) = &warn_cb {
                if let Ok(mut g) = cb.lock() {
                    g(msg);
                }
            }
        };
        let pst = self.open_pst(&locus.source_path)?;
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
                (
                    extracted.message_id,
                    extracted.subject,
                    extracted.sender_email,
                    extracted.display_to,
                    extracted.display_cc,
                    extracted.display_bcc,
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
                        (
                            props.message_id,
                            props.subject,
                            props.sender_email,
                            props.display_to,
                            None,
                            None,
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
                            None, None, None, None, None, None, None, None, None, None, None,
                            false, true,
                        )
                    }
                }
            }
        };

        let mut attachments = Vec::new();
        // parents_only: empty attachments list (family policy).
        // KeepAttachmentsWithParent: always list metadata; payloads optional / size-capped.
        if !parents_only {
            match pst.list_attachments(nid) {
                Ok(list) => {
                    // Cap optional small-payload probe so we never materialize multi-GB Vecs.
                    const SMALL_ATTACH_CAP: u32 = 64 * 1024;
                    for att in list {
                        let mut data = None;
                        let stream_available = att.size > 0 || !att.filename.is_empty();
                        if load_payloads && att.size > 0 && att.size <= SMALL_ATTACH_CAP {
                            match pst.open_attachment_data(nid, att.nid) {
                                Ok(mut reader) => {
                                    let mut buf = Vec::new();
                                    match reader.read_to_end(&mut buf) {
                                        Ok(_) => data = Some(buf),
                                        Err(e) => {
                                            emit_soft(format!(
                                                "open/read attachment payload failed (soft ATTACH_META_FAILED) nid={:#x} attach_nid={:#x}: {e}",
                                                locus.nid, att.nid.0
                                            ));
                                            if !soft_reasons.contains(
                                                &dedup_engine::IntegrityReason::AttachMetaFailed,
                                            ) {
                                                soft_reasons.push(
                                                    dedup_engine::IntegrityReason::AttachMetaFailed,
                                                );
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    emit_soft(format!(
                                        "open_attachment_data failed (soft ATTACH_META_FAILED) nid={:#x} attach_nid={:#x}: {e}",
                                        locus.nid, att.nid.0
                                    ));
                                    if !soft_reasons
                                        .contains(&dedup_engine::IntegrityReason::AttachMetaFailed)
                                    {
                                        soft_reasons
                                            .push(dedup_engine::IntegrityReason::AttachMetaFailed);
                                    }
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
        }

        let fidelity = if soft_reasons.is_empty() {
            dedup_engine::integrity::RecoverableIntegrity::clean()
        } else {
            dedup_engine::integrity::RecoverableIntegrity::with_degraded(
                soft_reasons,
                locus.is_orphaned,
            )
        };

        Ok(CanonicalMessage {
            locus: locus.clone(),
            message_id,
            subject,
            sender,
            display_to,
            display_cc,
            display_bcc,
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

/// Independent PST handle cache for streaming attach bytes during EML write.
///
/// Separate from [`PstMaterializer`] because `finalize_with_materialize` holds an exclusive
/// borrow on the materializer while `on_winner` runs. Read-only multi-open is fine on Windows.
pub struct PstAttachStreamSource {
    psts: HashMap<String, PstFile>,
}

impl PstAttachStreamSource {
    pub fn new() -> Self {
        Self {
            psts: HashMap::new(),
        }
    }

    fn open_pst(&mut self, path: &str) -> Result<&mut PstFile, EmlWriteError> {
        if !self.psts.contains_key(path) {
            let pst = PstFile::open(Path::new(path))
                .map_err(|e| EmlWriteError::Other(format!("open attach stream pst {path}: {e}")))?;
            self.psts.insert(path.to_string(), pst);
        }
        self.psts
            .get_mut(path)
            .ok_or_else(|| EmlWriteError::Other(format!("pst missing after open: {path}")))
    }
}

impl Default for PstAttachStreamSource {
    fn default() -> Self {
        Self::new()
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
        let pst = self.open_pst(&parent.source_path)?;
        let reader = pst
            .open_attachment_data(NodeId(parent.nid), NodeId(attach_nid))
            .map_err(|e| {
                EmlWriteError::Other(format!(
                    "open_attachment_data parent={:#x} attach={attach_nid:#x}: {e}",
                    parent.nid
                ))
            })?;
        Ok(Box::new(reader))
    }
}
