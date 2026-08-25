//! Full-stream per-attachment SHA-256 for Tier-2.5 `body-recip-attach` (0086/0090).
//!
//! Streams via `open_attachment_data` + fixed 64 KiB chunks (same family as 0074
//! `DISCARD_CHUNK`). Never materializes multi-GB `Vec`s. Length mismatch,
//! cloud-link, open/IO/CRC failure, cancel, and budget exhaustion map to
//! Choice B unread sentinels (domain-separated name+size), not omit / not empty digest.
//!
//! **0090:** method-5 / `message/rfc822` embeds use `embedded-msg-hash/v1` instead of
//! unread-sentinel-only or raw-blob-only. Nested child attaches are hashed under the
//! nested message's subnode tree (not the parent NBT).

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dedup_engine::{
    attach_depth_limit_sentinel, attach_unread_sentinel, compute_embedded_msg_hash_v1,
    embedded_attachments_hash, embedded_body_missing_hash, embedded_header_hash,
    embedded_recipients_hash, hash_full_body, CanonicalRecipient, ATTACH_EMBEDDED_MSG,
    EMPTY_CONTENT_SHA256, MAX_EMBEDDED_MSG_DEPTH,
};
use pst_reader::{EmbeddedChildAttach, MessageNodeRef, NodeId, PstFile};
use sha2::{Digest, Sha256};

/// Fixed digest buffer size (64 KiB) — never grow with attach size.
const DIGEST_CHUNK: usize = 64 * 1024;

/// Default max attaches full-stream digested per run.
pub const DEFAULT_MAX_ATTACHES: u64 = 50_000;
/// Default max digest bytes per run (1 GiB).
pub const DEFAULT_MAX_BYTES: u64 = 1_073_741_824;
/// Default per-attach max bytes (512 MiB; not the 0074 L2 1 MiB head).
pub const DEFAULT_PER_ATTACH_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Budgets for attach-content identity digests (distinct from 0074 head probe).
#[derive(Clone, Copy, Debug)]
pub struct AttachContentHashBudgets {
    pub max_attaches: u64,
    pub max_bytes: u64,
    pub per_attach_max_bytes: u64,
}

impl Default for AttachContentHashBudgets {
    fn default() -> Self {
        Self {
            max_attaches: DEFAULT_MAX_ATTACHES,
            max_bytes: DEFAULT_MAX_BYTES,
            per_attach_max_bytes: DEFAULT_PER_ATTACH_MAX_BYTES,
        }
    }
}

/// Result of hashing one attachment stream / embedded identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachDigestResult {
    /// Binary by-value stream SHA-256 (`bytes` = stream bytes read).
    Real { digest: [u8; 32], bytes: u64 },
    /// Nested `embedded-msg-hash/v1` (method-5 or rfc822); not a full binary stream digest.
    Embedded { digest: [u8; 32], bytes: u64 },
    /// Choice B unread sentinel (cloud / fail / mismatch / budget / cancel).
    Unread { sentinel: [u8; 32] },
    /// Depth-cap sentinel (0090); distinct from unread.
    DepthLimit { sentinel: [u8; 32] },
}

impl AttachDigestResult {
    pub fn digest(self) -> [u8; 32] {
        match self {
            Self::Real { digest, .. } | Self::Embedded { digest, .. } => digest,
            Self::Unread { sentinel } | Self::DepthLimit { sentinel } => sentinel,
        }
    }

    pub fn is_unread(self) -> bool {
        matches!(self, Self::Unread { .. })
    }

    pub fn is_depth_limit(self) -> bool {
        matches!(self, Self::DepthLimit { .. })
    }

    pub fn is_embedded(self) -> bool {
        matches!(self, Self::Embedded { .. })
    }
}

/// Mutable run-level budget counters for attach-content digests.
#[derive(Clone, Debug, Default)]
pub struct AttachContentHashState {
    pub attaches_digested: u64,
    pub bytes_digested: u64,
    pub unread: u64,
    pub truncated: bool,
    pub embedded_parsed: u64,
    pub embedded_depth_limit: u64,
    pub embedded_unparsed: u64,
}

impl AttachContentHashState {
    pub fn budget_exhausted(&self, budgets: &AttachContentHashBudgets) -> bool {
        self.truncated
            || self.attaches_digested >= budgets.max_attaches
            || self.bytes_digested >= budgets.max_bytes
    }
}

fn cancel_requested(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .map(|c| c.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// True when attach should use embedded-msg identity (method 5 or rfc822 MIME).
pub fn is_embedded_identity_attach(method: Option<i32>, mime: Option<&str>) -> bool {
    if method == Some(ATTACH_EMBEDDED_MSG) {
        return true;
    }
    mime.is_some_and(|m| m.to_ascii_lowercase().contains("message/rfc822"))
}

/// Hash one attachment for strong identity (0086 stream or 0090 embedded).
#[allow(clippy::too_many_arguments)]
pub fn hash_attachment_for_identity(
    pst: &mut PstFile,
    parent: &MessageNodeRef,
    attach_nid: NodeId,
    filename: &str,
    declared_size: u32,
    attach_method: Option<i32>,
    mime_tag: Option<&str>,
    is_cloud_link: bool,
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> AttachDigestResult {
    let mark_unread = |state: &mut AttachContentHashState| {
        state.unread = state.unread.saturating_add(1);
        AttachDigestResult::Unread {
            sentinel: attach_unread_sentinel(filename, declared_size),
        }
    };

    if cancel_requested(cancel) {
        state.truncated = true;
        return mark_unread(state);
    }
    if is_cloud_link {
        return mark_unread(state);
    }
    if state.budget_exhausted(budgets) {
        state.truncated = true;
        return mark_unread(state);
    }

    if is_embedded_identity_attach(attach_method, mime_tag) {
        return hash_embedded_attachment(
            pst,
            parent,
            attach_nid,
            filename,
            declared_size,
            attach_method,
            mime_tag,
            depth,
            ignore_inline,
            budgets,
            state,
            cancel,
        );
    }

    hash_binary_under_parent(
        pst,
        parent,
        attach_nid,
        filename,
        declared_size,
        budgets,
        state,
        cancel,
    )
}

/// NBT-parent wrapper: resolve message node then [`hash_attachment_for_identity`].
///
/// Pass `attach_method` / `mime_tag` so method-5 / rfc822 embeds take the 0090 path.
/// Wrapper keeps `ignore_inline = false` and `depth = 0`.
#[allow(clippy::too_many_arguments)]
pub fn hash_attachment_stream(
    pst: &mut PstFile,
    msg_nid: NodeId,
    attach_nid: NodeId,
    filename: &str,
    declared_size: u32,
    attach_method: Option<i32>,
    mime_tag: Option<&str>,
    is_cloud_link: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> AttachDigestResult {
    let parent = match pst.message_node_from_nbt(msg_nid) {
        Ok(p) => p,
        Err(_) => {
            state.unread = state.unread.saturating_add(1);
            return AttachDigestResult::Unread {
                sentinel: attach_unread_sentinel(filename, declared_size),
            };
        }
    };
    hash_attachment_for_identity(
        pst,
        &parent,
        attach_nid,
        filename,
        declared_size,
        attach_method,
        mime_tag,
        is_cloud_link,
        0,
        false,
        budgets,
        state,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
fn hash_binary_under_parent(
    pst: &mut PstFile,
    parent: &MessageNodeRef,
    attach_nid: NodeId,
    filename: &str,
    declared_size: u32,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> AttachDigestResult {
    let mark_unread = |state: &mut AttachContentHashState| {
        state.unread = state.unread.saturating_add(1);
        AttachDigestResult::Unread {
            sentinel: attach_unread_sentinel(filename, declared_size),
        }
    };

    let mut reader = match pst.open_attach_data_from_message_node(parent, attach_nid) {
        Ok(r) => r,
        Err(_) => return mark_unread(state),
    };

    let mut hasher = Sha256::new();
    let mut buf = [0u8; DIGEST_CHUNK];
    let mut bytes_read: u64 = 0;
    let per_cap = budgets.per_attach_max_bytes;

    loop {
        if cancel_requested(cancel) {
            state.truncated = true;
            return mark_unread(state);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return mark_unread(state),
        };
        let n_u = n as u64;
        if bytes_read.saturating_add(n_u) > per_cap {
            state.truncated = true;
            return mark_unread(state);
        }
        if state
            .bytes_digested
            .saturating_add(bytes_read)
            .saturating_add(n_u)
            > budgets.max_bytes
        {
            state.truncated = true;
            return mark_unread(state);
        }
        hasher.update(&buf[..n]);
        bytes_read = bytes_read.saturating_add(n_u);
    }

    if reader.crc_suspect() {
        return mark_unread(state);
    }
    if declared_size > 0 && bytes_read != u64::from(declared_size) {
        return mark_unread(state);
    }

    let digest: [u8; 32] = if declared_size == 0 && bytes_read == 0 {
        EMPTY_CONTENT_SHA256
    } else {
        hasher.finalize().into()
    };

    state.attaches_digested = state.attaches_digested.saturating_add(1);
    state.bytes_digested = state.bytes_digested.saturating_add(bytes_read);
    AttachDigestResult::Real {
        digest,
        bytes: bytes_read,
    }
}

#[allow(clippy::too_many_arguments)]
fn hash_embedded_attachment(
    pst: &mut PstFile,
    parent: &MessageNodeRef,
    attach_nid: NodeId,
    filename: &str,
    declared_size: u32,
    attach_method: Option<i32>,
    mime_tag: Option<&str>,
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> AttachDigestResult {
    let mark_unread = |state: &mut AttachContentHashState| {
        state.unread = state.unread.saturating_add(1);
        state.embedded_unparsed = state.embedded_unparsed.saturating_add(1);
        AttachDigestResult::Unread {
            sentinel: attach_unread_sentinel(filename, declared_size),
        }
    };

    // Admit against max_attaches before depth / nested resolve (tied to same
    // max_attaches: each embedded parse occupies one attach slot). Depth
    // sentinels must not consume count past the cap and starve siblings.
    if state.attaches_digested >= budgets.max_attaches || state.budget_exhausted(budgets) {
        state.truncated = true;
        return mark_unread(state);
    }

    if depth >= MAX_EMBEDDED_MSG_DEPTH {
        state.embedded_depth_limit = state.embedded_depth_limit.saturating_add(1);
        // Do not increment attaches_digested — depth sentinel is not a parse slot.
        return AttachDigestResult::DepthLimit {
            sentinel: attach_depth_limit_sentinel(filename, declared_size),
        };
    }

    // Method-5: reject oversize declared_size before resolve/PC load when known.
    if attach_method == Some(ATTACH_EMBEDDED_MSG)
        && declared_size > 0
        && u64::from(declared_size) > budgets.per_attach_max_bytes
    {
        state.truncated = true;
        return mark_unread(state);
    }

    // Reserve slot before nested work so child recursion sees parent admission.
    state.attaches_digested = state.attaches_digested.saturating_add(1);

    // Method 5: subnode identity path.
    if attach_method == Some(ATTACH_EMBEDDED_MSG) {
        let nested = match pst.resolve_embedded_root(parent, attach_nid) {
            Ok(n) => n,
            Err(_) => return mark_unread(state),
        };
        let run_remaining = budgets.max_bytes.saturating_sub(state.bytes_digested);
        let body_budget = run_remaining.min(budgets.per_attach_max_bytes);
        let fields = match pst.read_identity_from_message_node(&nested, body_budget) {
            Ok(f) => f,
            Err(e) => {
                if matches!(e, pst_reader::PstError::ResourceLimit(_)) {
                    state.truncated = true;
                }
                return mark_unread(state);
            }
        };
        if fields.crc_suspect {
            return mark_unread(state);
        }
        return finalize_embedded_identity(
            pst,
            &nested,
            &fields,
            depth,
            ignore_inline,
            budgets,
            state,
            cancel,
            filename,
            declared_size,
        );
    }

    // Method 1 / by-value rfc822: parse bytes.
    if mime_tag.is_some_and(|m| m.to_ascii_lowercase().contains("message/rfc822")) {
        return hash_rfc822_embedded(
            pst,
            parent,
            attach_nid,
            filename,
            declared_size,
            depth,
            ignore_inline,
            budgets,
            state,
            cancel,
        );
    }

    mark_unread(state)
}

#[allow(clippy::too_many_arguments)]
fn finalize_embedded_identity(
    pst: &mut PstFile,
    nested: &MessageNodeRef,
    fields: &pst_reader::EmbeddedIdentityFields,
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
    filename: &str,
    declared_size: u32,
) -> AttachDigestResult {
    let mark_unread = |state: &mut AttachContentHashState| {
        state.unread = state.unread.saturating_add(1);
        state.embedded_unparsed = state.embedded_unparsed.saturating_add(1);
        AttachDigestResult::Unread {
            sentinel: attach_unread_sentinel(filename, declared_size),
        }
    };

    if cancel_requested(cancel) {
        state.truncated = true;
        return mark_unread(state);
    }

    // Nested body bytes count toward the same per-attach / run caps as streams.
    // Residual: when declared_size was 0/missing, PC already loaded before this check.
    let body_bytes = fields
        .body_plain
        .as_ref()
        .map(|s| s.len() as u64)
        .unwrap_or(0);
    if body_bytes > budgets.per_attach_max_bytes
        || state.bytes_digested.saturating_add(body_bytes) > budgets.max_bytes
    {
        state.truncated = true;
        return mark_unread(state);
    }

    // Charge parent body BEFORE child recursion so children see the updated budget.
    // If a child later unread-fails, leave these bytes charged (honest accounting).
    state.bytes_digested = state.bytes_digested.saturating_add(body_bytes);

    let header = embedded_header_hash(
        fields.subject.as_deref(),
        fields.submit_time,
        fields.sender_email.as_deref(),
    );
    let body_hash = match fields.body_sha256 {
        Some(d) => d,
        None => match fields.body_plain.as_ref() {
            Some(s) => hash_full_body(s).0,
            None => embedded_body_missing_hash(),
        },
    };
    let canon: Vec<CanonicalRecipient> = fields
        .recipients
        .iter()
        .map(CanonicalRecipient::from_reader)
        .collect();
    let recip = embedded_recipients_hash(
        fields.display_to.as_deref(),
        fields.display_cc.as_deref(),
        fields.display_bcc.as_deref(),
        if canon.is_empty() {
            None
        } else {
            Some(canon.as_slice())
        },
    );

    let mut child_digests: Vec<[u8; 32]> = Vec::new();
    for child in &fields.child_attachments {
        if child.is_inline && ignore_inline {
            continue;
        }
        let dig = hash_child_under_nested(
            pst,
            nested,
            child,
            depth.saturating_add(1),
            ignore_inline,
            budgets,
            state,
            cancel,
        );
        child_digests.push(dig.digest());
    }
    let atts = embedded_attachments_hash(&child_digests);
    let digest = compute_embedded_msg_hash_v1(depth, header, body_hash, recip, atts);

    // Slot already reserved in hash_embedded_attachment; body already charged above.
    state.embedded_parsed = state.embedded_parsed.saturating_add(1);
    AttachDigestResult::Embedded {
        digest,
        bytes: body_bytes,
    }
}

#[allow(clippy::too_many_arguments)]
fn hash_child_under_nested(
    pst: &mut PstFile,
    nested: &MessageNodeRef,
    child: &EmbeddedChildAttach,
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> AttachDigestResult {
    hash_attachment_for_identity(
        pst,
        nested,
        child.nid,
        &child.filename,
        child.size,
        child.attach_method,
        child.mime_tag.as_deref(),
        child.is_cloud_link,
        depth,
        ignore_inline,
        budgets,
        state,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
fn hash_rfc822_embedded(
    pst: &mut PstFile,
    parent: &MessageNodeRef,
    attach_nid: NodeId,
    filename: &str,
    declared_size: u32,
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> AttachDigestResult {
    let mark_unread = |state: &mut AttachContentHashState| {
        state.unread = state.unread.saturating_add(1);
        state.embedded_unparsed = state.embedded_unparsed.saturating_add(1);
        AttachDigestResult::Unread {
            sentinel: attach_unread_sentinel(filename, declared_size),
        }
    };

    // Run remaining + per-attach caps before reading into a Vec.
    let remaining = budgets.max_bytes.saturating_sub(state.bytes_digested);
    let per_cap = budgets
        .per_attach_max_bytes
        .min(16 * 1024 * 1024)
        .min(remaining);
    if per_cap == 0 {
        state.truncated = true;
        return mark_unread(state);
    }
    if declared_size > 0 {
        let dec = u64::from(declared_size);
        if dec > budgets.per_attach_max_bytes || dec > remaining {
            state.truncated = true;
            return mark_unread(state);
        }
    }

    let mut reader = match pst.open_attach_data_from_message_node(parent, attach_nid) {
        Ok(r) => r,
        Err(_) => return mark_unread(state),
    };
    let mut buf = [0u8; DIGEST_CHUNK];
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        if cancel_requested(cancel) {
            state.truncated = true;
            return mark_unread(state);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return mark_unread(state),
        };
        if (bytes.len() as u64).saturating_add(n as u64) > per_cap {
            // Stop early — do not grow past remaining budget → unread.
            state.truncated = true;
            return mark_unread(state);
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    if reader.crc_suspect() {
        return mark_unread(state);
    }
    if declared_size > 0 && bytes.len() as u64 != u64::from(declared_size) {
        return mark_unread(state);
    }

    let parsed = match parse_simple_rfc822(&bytes, depth, ignore_inline, budgets, state, cancel) {
        Some(p) => p,
        None => return mark_unread(state),
    };

    let header = embedded_header_hash(
        parsed.subject.as_deref(),
        parsed.submit_time,
        parsed.sender.as_deref(),
    );
    let (body_hash, _) = hash_full_body(&parsed.body);
    let recip = embedded_recipients_hash(
        parsed.display_to.as_deref(),
        parsed.display_cc.as_deref(),
        parsed.display_bcc.as_deref(),
        None,
    );
    let child_digests = parsed.child_digests;
    let atts = embedded_attachments_hash(&child_digests);
    let digest = compute_embedded_msg_hash_v1(depth, header, body_hash, recip, atts);

    // Slot already reserved in hash_embedded_attachment.
    state.bytes_digested = state.bytes_digested.saturating_add(bytes.len() as u64);
    state.embedded_parsed = state.embedded_parsed.saturating_add(1);
    AttachDigestResult::Embedded {
        digest,
        bytes: bytes.len() as u64,
    }
}

/// Minimal rfc822 parse result (no mailparse dependency).
#[derive(Debug, Default)]
struct Rfc822Parsed {
    subject: Option<String>,
    sender: Option<String>,
    submit_time: Option<i64>,
    display_to: Option<String>,
    display_cc: Option<String>,
    display_bcc: Option<String>,
    body: String,
    child_digests: Vec<[u8; 32]>,
}

/// Bounded single-part / simple multipart parser. Fail → None (caller unread).
fn parse_simple_rfc822(
    raw: &[u8],
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> Option<Rfc822Parsed> {
    let text = std::str::from_utf8(raw).ok()?;
    let (header_block, body) = split_headers_body(text)?;
    let headers = unfold_headers(header_block);

    let content_type = header_value(&headers, "content-type").unwrap_or_default();
    let ct_lower = content_type.to_ascii_lowercase();

    // Multipart: text → body; message/rfc822 → recursive embedded hash; else opaque.
    if ct_lower.starts_with("multipart/") {
        let boundary = mime_param(&content_type, "boundary")?;
        return parse_multipart_rfc822(
            &headers,
            body,
            &boundary,
            depth,
            ignore_inline,
            budgets,
            state,
            cancel,
        );
    }

    // Single-part message/rfc822 or plain: headers + body text.
    Some(Rfc822Parsed {
        subject: header_value(&headers, "subject"),
        sender: extract_email(header_value(&headers, "from").as_deref()),
        submit_time: parse_date_best_effort(header_value(&headers, "date").as_deref()),
        display_to: header_value(&headers, "to"),
        display_cc: header_value(&headers, "cc"),
        display_bcc: header_value(&headers, "bcc"),
        body: body.to_string(),
        child_digests: Vec::new(),
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_multipart_rfc822(
    headers: &[(String, String)],
    body: &str,
    boundary: &str,
    depth: u8,
    ignore_inline: bool,
    budgets: &AttachContentHashBudgets,
    state: &mut AttachContentHashState,
    cancel: &Option<Arc<AtomicBool>>,
) -> Option<Rfc822Parsed> {
    let delim = format!("--{boundary}");
    let parts: Vec<&str> = body.split(&delim).collect();
    if parts.len() < 2 {
        return None;
    }
    let mut text_body = String::new();
    let mut child_digests = Vec::new();
    let mut saw_text = false;

    for part in parts {
        let part = part.trim_start_matches("\r\n").trim_start_matches('\n');
        if part.is_empty() || part.starts_with("--") {
            continue;
        }
        let (ph, pb) = match split_headers_body(part) {
            Some(v) => v,
            None => continue,
        };
        let phs = unfold_headers(ph);
        if ignore_inline && mime_part_is_inline(&phs) {
            continue;
        }
        let pct = header_value(&phs, "content-type")
            .unwrap_or_else(|| "text/plain".into())
            .to_ascii_lowercase();
        if pct.starts_with("text/plain") || pct.starts_with("text/html") {
            if !saw_text || pct.starts_with("text/plain") {
                text_body = pb.to_string();
                saw_text = true;
            }
        } else if pct.contains("message/rfc822") || pct.starts_with("message/") {
            // Recursive embedded-msg-hash/v1 for nested RFC822 MIME children.
            // Thread budgets/state: count slots, fail closed on parse/cap (never raw hash).
            let child_depth = depth.saturating_add(1);
            let part_name = "rfc822-part";
            let part_size = pb.len() as u32;
            let push_unread = |state: &mut AttachContentHashState, digests: &mut Vec<[u8; 32]>| {
                state.unread = state.unread.saturating_add(1);
                state.embedded_unparsed = state.embedded_unparsed.saturating_add(1);
                digests.push(attach_unread_sentinel(part_name, part_size));
            };

            if cancel_requested(cancel) {
                state.truncated = true;
                push_unread(state, &mut child_digests);
                continue;
            }
            // Count exhaustion before depth: unread sentinel, do not increment past cap.
            if state.budget_exhausted(budgets) || state.attaches_digested >= budgets.max_attaches {
                state.truncated = true;
                push_unread(state, &mut child_digests);
                continue;
            }
            if child_depth >= MAX_EMBEDDED_MSG_DEPTH {
                state.embedded_depth_limit = state.embedded_depth_limit.saturating_add(1);
                // Do not consume attaches_digested — depth sentinel must not starve siblings.
                child_digests.push(attach_depth_limit_sentinel(part_name, part_size));
                continue;
            }

            // Reserve attach slot before nested parse (same as top-level method-1).
            state.attaches_digested = state.attaches_digested.saturating_add(1);

            match parse_simple_rfc822(
                pb.as_bytes(),
                child_depth,
                ignore_inline,
                budgets,
                state,
                cancel,
            ) {
                Some(nested) => {
                    let header = embedded_header_hash(
                        nested.subject.as_deref(),
                        nested.submit_time,
                        nested.sender.as_deref(),
                    );
                    let (body_hash, _) = hash_full_body(&nested.body);
                    let recip = embedded_recipients_hash(
                        nested.display_to.as_deref(),
                        nested.display_cc.as_deref(),
                        nested.display_bcc.as_deref(),
                        None,
                    );
                    let atts = embedded_attachments_hash(&nested.child_digests);
                    child_digests.push(compute_embedded_msg_hash_v1(
                        child_depth,
                        header,
                        body_hash,
                        recip,
                        atts,
                    ));
                    state.embedded_parsed = state.embedded_parsed.saturating_add(1);
                }
                None => {
                    // Fail closed: unread sentinel — never raw SHA-256 of malformed bytes.
                    push_unread(state, &mut child_digests);
                }
            }
        } else {
            // Non-rfc822 binary / file parts: opaque SHA-256 of part body.
            let mut h = Sha256::new();
            h.update(pb.as_bytes());
            child_digests.push(h.finalize().into());
        }
    }

    Some(Rfc822Parsed {
        subject: header_value(headers, "subject"),
        sender: extract_email(header_value(headers, "from").as_deref()),
        submit_time: parse_date_best_effort(header_value(headers, "date").as_deref()),
        display_to: header_value(headers, "to"),
        display_cc: header_value(headers, "cc"),
        display_bcc: header_value(headers, "bcc"),
        body: text_body,
        child_digests,
    })
}

/// Honor `identity_ignore_inline_attachments`: Content-Disposition: inline or Content-ID.
fn mime_part_is_inline(headers: &[(String, String)]) -> bool {
    if header_value(headers, "content-id")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    header_value(headers, "content-disposition")
        .map(|v| v.to_ascii_lowercase().starts_with("inline"))
        .unwrap_or(false)
}

fn split_headers_body(text: &str) -> Option<(&str, &str)> {
    if let Some(i) = text.find("\r\n\r\n") {
        return Some((&text[..i], &text[i + 4..]));
    }
    if let Some(i) = text.find("\n\n") {
        return Some((&text[..i], &text[i + 2..]));
    }
    None
}

fn unfold_headers(block: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in block.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, ref mut v)) = current {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some(prev) = current.take() {
            out.push(prev);
        }
        if let Some((name, rest)) = line.split_once(':') {
            current = Some((name.trim().to_ascii_lowercase(), rest.trim().to_string()));
        }
    }
    if let Some(prev) = current {
        out.push(prev);
    }
    out
}

fn header_value(headers: &[(String, String)], name: &str) -> Option<String> {
    let want = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(k, _)| k == &want)
        .map(|(_, v)| v.clone())
}

fn mime_param(content_type: &str, name: &str) -> Option<String> {
    let want = name.to_ascii_lowercase();
    for part in content_type.split(';').skip(1) {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k.trim().eq_ignore_ascii_case(&want) {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn extract_email(from: Option<&str>) -> Option<String> {
    let s = from?.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(start) = s.find('<') {
        if let Some(end) = s[start..].find('>') {
            let email = s[start + 1..start + end].trim();
            if !email.is_empty() {
                return Some(email.to_string());
            }
        }
    }
    Some(s.to_string())
}

/// Best-effort RFC 2822 `Date` → Windows FILETIME i64.
///
/// Uses `chrono::DateTime::parse_from_rfc2822`. Unparsable / empty → `None`
/// (header slot omits submit_time; never invents a fake timestamp).
fn parse_date_best_effort(date: Option<&str>) -> Option<i64> {
    let s = date?.trim();
    if s.is_empty() {
        return None;
    }
    let dt = chrono::DateTime::parse_from_rfc2822(s).ok()?;
    unix_secs_to_filetime(dt.timestamp())
}

fn unix_secs_to_filetime(unix_secs: i64) -> Option<i64> {
    // FILETIME = 100-ns ticks since 1601-01-01; Unix epoch offset = 11644473600s.
    unix_secs
        .checked_add(11_644_473_600)?
        .checked_mul(10_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dedup_engine::attach_unread_sentinel;

    fn parse_rfc822_default(raw: &[u8], depth: u8, ignore_inline: bool) -> Option<Rfc822Parsed> {
        let budgets = AttachContentHashBudgets::default();
        let mut state = AttachContentHashState::default();
        let cancel = None;
        parse_simple_rfc822(raw, depth, ignore_inline, &budgets, &mut state, &cancel)
    }

    #[test]
    fn unread_helper_matches_engine_sentinel() {
        let s = attach_unread_sentinel("a.pdf", 12);
        let r = AttachDigestResult::Unread { sentinel: s };
        assert!(r.is_unread());
        assert_eq!(r.digest(), s);
    }

    #[test]
    fn empty_digest_constant() {
        let mut h = Sha256::new();
        h.update([]);
        let d: [u8; 32] = h.finalize().into();
        assert_eq!(d, EMPTY_CONTENT_SHA256);
    }

    #[test]
    fn budget_exhausted_logic() {
        let budgets = AttachContentHashBudgets {
            max_attaches: 2,
            max_bytes: 100,
            per_attach_max_bytes: 50,
        };
        let mut state = AttachContentHashState::default();
        assert!(!state.budget_exhausted(&budgets));
        state.attaches_digested = 2;
        assert!(state.budget_exhausted(&budgets));
        state.attaches_digested = 0;
        state.bytes_digested = 100;
        assert!(state.budget_exhausted(&budgets));
        state.bytes_digested = 0;
        state.truncated = true;
        assert!(state.budget_exhausted(&budgets));
    }

    #[test]
    fn simple_rfc822_parse_subject_body() {
        let raw = b"From: Alice <alice@example.com>\r\n\
Subject: Nested Subject\r\n\
To: bob@example.com\r\n\
Content-Type: text/plain\r\n\
\r\n\
Hello nested body\r\n";
        let p = parse_rfc822_default(raw, 0, false).expect("parse");
        assert_eq!(p.subject.as_deref(), Some("Nested Subject"));
        assert_eq!(p.sender.as_deref(), Some("alice@example.com"));
        assert_eq!(p.display_to.as_deref(), Some("bob@example.com"));
        assert!(p.body.contains("Hello nested body"));
        assert!(p.child_digests.is_empty());
    }

    #[test]
    fn rfc822_date_parses_to_filetime() {
        let raw = b"From: a@x.com\r\n\
Date: Mon, 02 Jan 2006 15:04:05 +0000\r\n\
Subject: Dated\r\n\
\r\n\
body\r\n";
        let p = parse_rfc822_default(raw, 0, false).expect("parse");
        assert!(p.submit_time.is_some(), "RFC2822 Date must map to FILETIME");
        let raw_bad = b"From: a@x.com\r\nDate: not-a-date\r\nSubject: X\r\n\r\nbody\r\n";
        let pb = parse_rfc822_default(raw_bad, 0, false).expect("parse");
        assert!(pb.submit_time.is_none(), "unparsable Date → None");
    }

    #[test]
    fn rfc822_ignore_inline_skips_cid_parts() {
        let raw = b"Content-Type: multipart/mixed; boundary=b1\r\n\
Subject: M\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain\r\n\
\r\n\
hello\r\n\
--b1\r\n\
Content-Type: application/octet-stream\r\n\
Content-Disposition: inline\r\n\
\r\n\
secret\r\n\
--b1\r\n\
Content-Type: application/pdf\r\n\
Content-Disposition: attachment\r\n\
\r\n\
pdfbytes\r\n\
--b1--\r\n";
        let with = parse_rfc822_default(raw, 0, false).expect("parse");
        let without = parse_rfc822_default(raw, 0, true).expect("parse");
        assert_eq!(with.child_digests.len(), 2);
        assert_eq!(without.child_digests.len(), 1);
        assert_ne!(
            embedded_attachments_hash(&with.child_digests),
            embedded_attachments_hash(&without.child_digests)
        );
    }

    #[test]
    fn rfc822_nested_message_part_uses_embedded_hash() {
        let nested = "From: n@x.com\r\nSubject: Inner\r\n\r\ninner-body\r\n";
        let raw = format!(
            "Content-Type: multipart/mixed; boundary=bnd\r\n\
Subject: Outer\r\n\
\r\n\
--bnd\r\n\
Content-Type: text/plain\r\n\
\r\n\
outer text\r\n\
--bnd\r\n\
Content-Type: message/rfc822\r\n\
\r\n\
{nested}\
--bnd--\r\n"
        );
        let budgets = AttachContentHashBudgets::default();
        let mut state = AttachContentHashState::default();
        let cancel = None;
        let p = parse_simple_rfc822(raw.as_bytes(), 0, false, &budgets, &mut state, &cancel)
            .expect("parse");
        assert_eq!(p.child_digests.len(), 1);
        assert_eq!(state.embedded_parsed, 1);
        assert_eq!(state.attaches_digested, 1);
        // Opaque SHA of nested bytes must differ from embedded-msg-hash/v1.
        let mut opaque = Sha256::new();
        opaque.update(nested.as_bytes());
        let opaque_d: [u8; 32] = opaque.finalize().into();
        assert_ne!(p.child_digests[0], opaque_d);
        assert_ne!(
            p.child_digests[0],
            attach_unread_sentinel("rfc822-part", nested.len() as u32)
        );
    }

    #[test]
    fn rfc822_malformed_nested_uses_unread_sentinel_not_raw() {
        // Nested part lacks header/body split → parse_simple_rfc822 returns None.
        let nested = "From: n@x.com\r\nSubject: Inner with no blank line";
        let raw = format!(
            "Content-Type: multipart/mixed; boundary=bnd\r\n\
Subject: Outer\r\n\
\r\n\
--bnd\r\n\
Content-Type: text/plain\r\n\
\r\n\
outer text\r\n\
--bnd\r\n\
Content-Type: message/rfc822\r\n\
\r\n\
{nested}\
--bnd--\r\n"
        );
        let budgets = AttachContentHashBudgets::default();
        let mut state = AttachContentHashState::default();
        let cancel = None;
        let p = parse_simple_rfc822(raw.as_bytes(), 0, false, &budgets, &mut state, &cancel)
            .expect("parent multipart must still parse");
        assert_eq!(p.subject.as_deref(), Some("Outer"));
        assert_eq!(p.child_digests.len(), 1);
        let expected = attach_unread_sentinel("rfc822-part", nested.len() as u32);
        assert_eq!(p.child_digests[0], expected);
        let mut opaque = Sha256::new();
        opaque.update(nested.as_bytes());
        let opaque_d: [u8; 32] = opaque.finalize().into();
        assert_ne!(p.child_digests[0], opaque_d);
        assert_eq!(state.embedded_unparsed, 1);
        assert_eq!(state.unread, 1);
        assert_eq!(state.embedded_parsed, 0);
    }

    #[test]
    fn rfc822_depth_cap_does_not_push_attaches_digested_above_max() {
        // Several message/rfc822 children at child_depth >= MAX must not consume
        // parse-count slots (depth sentinel only), so attaches_digested stays ≤ N.
        let nested = "From: n@x.com\r\nSubject: Inner\r\n\r\ninner-body\r\n";
        let mut raw = String::from(
            "Content-Type: multipart/mixed; boundary=bnd\r\n\
Subject: Outer\r\n\
\r\n\
--bnd\r\n\
Content-Type: text/plain\r\n\
\r\n\
outer text\r\n",
        );
        for _ in 0..3 {
            raw.push_str(&format!(
                "--bnd\r\n\
Content-Type: message/rfc822\r\n\
\r\n\
{nested}"
            ));
        }
        raw.push_str("--bnd--\r\n");

        let max_n = 2u64;
        let budgets = AttachContentHashBudgets {
            max_attaches: max_n,
            max_bytes: 1_000_000,
            per_attach_max_bytes: 1_000_000,
        };
        let mut state = AttachContentHashState::default();
        let cancel = None;
        // depth = MAX-1 → child_depth == MAX → depth-cap path for each nested part.
        let p = parse_simple_rfc822(
            raw.as_bytes(),
            MAX_EMBEDDED_MSG_DEPTH.saturating_sub(1),
            false,
            &budgets,
            &mut state,
            &cancel,
        )
        .expect("parent multipart must still parse");
        assert_eq!(p.child_digests.len(), 3);
        let expected = attach_depth_limit_sentinel("rfc822-part", nested.len() as u32);
        assert!(p.child_digests.iter().all(|d| d == &expected));
        assert_eq!(state.embedded_depth_limit, 3);
        assert!(
            state.attaches_digested <= max_n,
            "depth-cap children must not push attaches_digested above max_attaches; got {}",
            state.attaches_digested
        );
        assert_eq!(
            state.attaches_digested, 0,
            "depth sentinel must not consume a parse-count slot"
        );
        assert_eq!(state.embedded_parsed, 0);
        assert!(!state.truncated);
    }

    #[test]
    fn rfc822_nested_respects_max_attaches() {
        let nested = "From: n@x.com\r\nSubject: Inner\r\n\r\ninner-body\r\n";
        let raw = format!(
            "Content-Type: multipart/mixed; boundary=bnd\r\n\
Subject: Outer\r\n\
\r\n\
--bnd\r\n\
Content-Type: text/plain\r\n\
\r\n\
outer text\r\n\
--bnd\r\n\
Content-Type: message/rfc822\r\n\
\r\n\
{nested}\
--bnd--\r\n"
        );
        // Top-level already reserved the only attach slot (max_attaches=1).
        let budgets = AttachContentHashBudgets {
            max_attaches: 1,
            max_bytes: 1_000_000,
            per_attach_max_bytes: 1_000_000,
        };
        let mut state = AttachContentHashState {
            attaches_digested: 1,
            ..Default::default()
        };
        let cancel = None;
        let p = parse_simple_rfc822(raw.as_bytes(), 0, false, &budgets, &mut state, &cancel)
            .expect("parent multipart must still parse");
        assert_eq!(p.child_digests.len(), 1);
        assert_eq!(
            p.child_digests[0],
            attach_unread_sentinel("rfc822-part", nested.len() as u32)
        );
        assert!(state.truncated);
        assert_eq!(state.embedded_parsed, 0);
        assert_eq!(
            state.attaches_digested, 1,
            "nested must not consume a success slot"
        );
        assert_eq!(state.embedded_unparsed, 1);
    }

    #[test]
    fn rfc822_subject_change_changes_hash_components() {
        let a = b"From: a@x.com\r\nSubject: A\r\n\r\nbody\r\n";
        let b = b"From: a@x.com\r\nSubject: B\r\n\r\nbody\r\n";
        let pa = parse_rfc822_default(a, 0, false).unwrap();
        let pb = parse_rfc822_default(b, 0, false).unwrap();
        let ha = embedded_header_hash(pa.subject.as_deref(), None, pa.sender.as_deref());
        let hb = embedded_header_hash(pb.subject.as_deref(), None, pb.sender.as_deref());
        assert_ne!(ha, hb);
    }

    #[test]
    fn embedded_admit_before_parse_respects_max_attaches() {
        let budgets = AttachContentHashBudgets {
            max_attaches: 1,
            max_bytes: 1_000_000,
            per_attach_max_bytes: 1_000_000,
        };
        let state = AttachContentHashState {
            attaches_digested: 1,
            ..Default::default()
        };
        assert!(state.budget_exhausted(&budgets));
    }

    #[test]
    fn is_embedded_detects_method5_and_rfc822() {
        assert!(is_embedded_identity_attach(Some(5), None));
        assert!(is_embedded_identity_attach(Some(1), Some("message/rfc822")));
        assert!(!is_embedded_identity_attach(
            Some(1),
            Some("application/pdf")
        ));
    }

    #[test]
    fn missing_nested_body_component_ne_empty() {
        let missing = embedded_body_missing_hash();
        let (empty, _) = hash_full_body("");
        assert_ne!(missing, empty);
    }

    #[test]
    fn nested_body_budget_helpers_match_stream_policy() {
        // Mirrors finalize_embedded_identity pre-digest gate.
        let budgets = AttachContentHashBudgets {
            max_attaches: 50_000,
            max_bytes: 100,
            per_attach_max_bytes: 50,
        };
        let body_bytes = 60u64;
        assert!(body_bytes > budgets.per_attach_max_bytes);
        let body_ok = 40u64;
        let mut state = AttachContentHashState {
            bytes_digested: 70,
            ..Default::default()
        };
        assert!(state.bytes_digested.saturating_add(body_ok) > budgets.max_bytes);
        state.bytes_digested = 10;
        assert!(state.bytes_digested.saturating_add(body_ok) <= budgets.max_bytes);
    }

    #[test]
    fn method5_parent_body_charged_before_child_aggregate_budget() {
        // Parent body 60 + child body 60 with max_bytes=100: after charging parent,
        // child aggregate check must fail closed (unread/truncated path).
        let budgets = AttachContentHashBudgets {
            max_attaches: 50_000,
            max_bytes: 100,
            per_attach_max_bytes: 80,
        };
        let parent_body = 60u64;
        let child_body = 60u64;
        assert!(parent_body <= budgets.per_attach_max_bytes);
        assert!(child_body <= budgets.per_attach_max_bytes);

        let mut state = AttachContentHashState::default();
        assert!(state.bytes_digested.saturating_add(parent_body) <= budgets.max_bytes);
        // Charge-before-children (finalize_embedded_identity).
        state.bytes_digested = state.bytes_digested.saturating_add(parent_body);
        assert_eq!(state.bytes_digested, 60);
        // Child sees already-updated bytes_digested and fails the run budget.
        assert!(
            state.bytes_digested.saturating_add(child_body) > budgets.max_bytes,
            "child must observe parent charge and fail closed"
        );
        state.truncated = true;
        assert!(state.budget_exhausted(&budgets));
        // Honest: leave parent body charged after child unread.
        assert_eq!(state.bytes_digested, 60);
    }
}
