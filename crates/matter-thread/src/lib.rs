//! # matter-thread
//!
//! Matter-level **email threading** over Normalized Items (track **0022**):
//!
//! 1. **Headers** — Message-ID + In-Reply-To + References (union-find / JWZ-style)
//! 2. **Subject** — strip RE/FW/FWD among remaining singletons only
//! 3. **ConversationIndex** — opaque 22-byte (44-hex) prefix among remaining singletons
//! 4. **Family** — inherit parent thread fields onto attachments
//!
//! ## Identity rules
//!
//! - Never change `logical_hash` subject rules (strict keeps RE).
//! - Never delete items or CAS blobs.
//! - Never mutate source PST.
//!
//! ## Memory
//!
//! Canonical maps use **fixed-size `[u8; 32]` keys** (see [`keys`]). Parents are
//! loaded as thin [`matter_core::ThreadCandidate`] rows — not full `Item`
//! bodies with text.
//!
//! ## Transactions
//!
//! Each batch of thread field updates + `put_checkpoint` commits in **one**
//! SQLite transaction via [`matter_core::Matter::apply_thread_batch_with_checkpoint`].

#![forbid(unsafe_code)]

pub mod error;
pub mod keys;
pub mod normalize;
pub mod params;
pub mod run;
pub mod unionfind;

pub use error::{Result, ThreadError};
pub use keys::{message_id_key, sha256_hex, CompactKey};
pub use normalize::normalize_subject_thread;
pub use params::ThreadParams;
pub use run::{
    run_thread, ThreadOutcome, ThreadSummary, CONVERSATION_INDEX_PREFIX_HEX_LEN, JOB_KIND_THREAD,
    THREAD_STAGE,
};
