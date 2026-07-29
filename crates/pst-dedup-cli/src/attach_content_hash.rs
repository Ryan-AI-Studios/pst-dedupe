//! Full-stream per-attachment SHA-256 for Tier-2.5 `body-recip-attach` (0086).
//!
//! Streams via `open_attachment_data` + fixed 64 KiB chunks (same family as 0074
//! `DISCARD_CHUNK`). Never materializes multi-GB `Vec`s. Length mismatch,
//! cloud-link, open/IO/CRC failure, cancel, and budget exhaustion map to
//! Choice B unread sentinels (domain-separated name+size), not omit / not empty digest.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dedup_engine::{attach_unread_sentinel, EMPTY_CONTENT_SHA256};
use pst_reader::{NodeId, PstFile};
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

/// Result of hashing one attachment stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachDigestResult {
    /// Full stream successfully hashed; `bytes` is bytes_read.
    Real { digest: [u8; 32], bytes: u64 },
    /// Choice B unread sentinel (cloud / fail / mismatch / budget / cancel).
    Unread { sentinel: [u8; 32] },
}

impl AttachDigestResult {
    pub fn digest(self) -> [u8; 32] {
        match self {
            Self::Real { digest, .. } => digest,
            Self::Unread { sentinel } => sentinel,
        }
    }

    pub fn is_unread(self) -> bool {
        matches!(self, Self::Unread { .. })
    }
}

/// Mutable run-level budget counters for attach-content digests.
#[derive(Clone, Debug, Default)]
pub struct AttachContentHashState {
    pub attaches_digested: u64,
    pub bytes_digested: u64,
    pub unread: u64,
    pub truncated: bool,
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

/// Hash one attachment's binary stream under budgets and cancel.
///
/// - Cloud-link / no binary: unread without open attempt when `is_cloud_link`.
/// - Declared size 0 + immediate EOF → real `SHA-256("")`.
/// - Declared size > 0 and `bytes_read != size` → unread (length mismatch).
/// - CRC suspect after read → unread (do not invent success).
#[allow(clippy::too_many_arguments)]
pub fn hash_attachment_stream(
    pst: &mut PstFile,
    msg_nid: NodeId,
    attach_nid: NodeId,
    filename: &str,
    declared_size: u32,
    is_cloud_link: bool,
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

    let mut reader = match pst.open_attachment_data(msg_nid, attach_nid) {
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
        // Cap exceeded mid-stream → unread (partial digest is not identity).
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

    // Length match when size is authoritative (>0).
    if declared_size > 0 && bytes_read != u64::from(declared_size) {
        return mark_unread(state);
    }

    // Legitimate empty: size 0 + EOF → SHA-256("").
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

#[cfg(test)]
mod tests {
    use super::*;
    use dedup_engine::attach_unread_sentinel;

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
}
