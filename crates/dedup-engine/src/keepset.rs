//! Keep-set v1: policy-driven export winners + decision log + materialize promotion.
//!
//! Schema id: [`KEEP_SET_SCHEMA`] (`keep_set_v1`).
//!
//! Orchestration (locked):
//! 1. Sort absolute input paths (deterministic)
//! 2. Scan / collect recoverable candidates
//! 3. Resolve groups: fidelity → named policy → `(path_key, nid)`
//! 4. Optional materialize with hard-fail promotion
//! 5. Stream decision CSV + write keep-set JSON (post-promotion roles only)
//!
//! Source PSTs are never mutated. EDRM MIH is interop metadata, not a suppress tier.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::grouping::{
    mid_join_compatible, mid_present, BoundBy, DedupeScope, GroupingContext, GroupingStats,
    IdentityLevel,
};
use crate::hasher::{tier2_eligibility, Tier2IneligibleReason};
use crate::integrity::RecoverableIntegrity;

/// Stable JSON schema identifier for keep-set payloads.
pub const KEEP_SET_SCHEMA: &str = "keep_set_v1";

// ─── Policy / role enums ────────────────────────────────────────────────────

/// Winner selection policy (applied after fidelity / evidence rungs).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeepPolicy {
    /// Earliest by deterministic scan order among remaining candidates.
    ///
    /// **Note:** `first_seen` means sorted input-path order (then scan index),
    /// not chronological send time. Use [`Self::EarliestDate`] for dates.
    #[default]
    FirstSeen,
    /// Prefer largest `message_size` (0/missing last).
    KeepLargest,
    /// Prefer sources whose path/folder matches prefer-path patterns.
    PreferPath,
    /// Prefer earliest message date (submit primary, delivery fallback; missing last).
    EarliestDate,
}

impl KeepPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstSeen => "first_seen",
            Self::KeepLargest => "keep_largest",
            Self::PreferPath => "prefer_path",
            Self::EarliestDate => "earliest_date",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "first_seen" => Some(Self::FirstSeen),
            "keep_largest" => Some(Self::KeepLargest),
            "prefer_path" => Some(Self::PreferPath),
            "earliest_date" => Some(Self::EarliestDate),
            _ => None,
        }
    }
}

impl fmt::Display for KeepPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Rank context / ladder types (0075) ─────────────────────────────────────

/// Fidelity ranking mode. Default [`Self::Binary`] preserves pre-0075 winners.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FidelityMode {
    /// Clean = 0; any degraded/orphaned = 1 (pre-0075).
    #[default]
    Binary,
    /// Multi-tier: clean < soft attach meta < attach payload < body < structural.
    Graded,
}

impl FidelityMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Graded => "graded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "binary" => Some(Self::Binary),
            "graded" => Some(Self::Graded),
            _ => None,
        }
    }
}

/// Folder-class ranking mode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FolderRankMode {
    /// Ladder off — every item rank 0, class label `primary`.
    #[default]
    Off,
    /// Built-in ladder (§3.4).
    Builtin,
    /// Custom ordered patterns (worst-last); unmatched = 0 (best). Replaces builtin.
    Custom(Vec<String>),
}

/// Closed vocabulary for folder class labels (decision CSV / keep JSON).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FolderClass {
    SentItems,
    Primary,
    Archive,
    JunkEmail,
    Drafts,
    Outbox,
    DeletedItems,
    RecoverableDeletions,
    RecoverableHolds,
    RecoverablePurges,
    RecoverableVersions,
    RecoverableOps,
    RecoverableOther,
}

impl FolderClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SentItems => "sent_items",
            Self::Primary => "primary",
            Self::Archive => "archive",
            Self::JunkEmail => "junk_email",
            Self::Drafts => "drafts",
            Self::Outbox => "outbox",
            Self::DeletedItems => "deleted_items",
            Self::RecoverableDeletions => "recoverable_deletions",
            Self::RecoverableHolds => "recoverable_holds",
            Self::RecoverablePurges => "recoverable_purges",
            Self::RecoverableVersions => "recoverable_versions",
            Self::RecoverableOps => "recoverable_ops",
            Self::RecoverableOther => "recoverable_other",
        }
    }

    /// Built-in ladder rank (lower is better).
    pub fn builtin_rank(self) -> u32 {
        match self {
            Self::SentItems => 0,
            Self::Primary => 1,
            Self::Archive => 2,
            Self::JunkEmail => 3,
            Self::Drafts => 4,
            Self::Outbox => 5,
            Self::DeletedItems => 6,
            Self::RecoverableDeletions => 7,
            Self::RecoverableHolds => 8,
            Self::RecoverablePurges => 9,
            Self::RecoverableVersions => 10,
            Self::RecoverableOps => 11,
            Self::RecoverableOther => 12,
        }
    }

    /// True when this class is under Recoverable Items.
    pub fn is_recoverable_items(self) -> bool {
        matches!(
            self,
            Self::RecoverableDeletions
                | Self::RecoverableHolds
                | Self::RecoverablePurges
                | Self::RecoverableVersions
                | Self::RecoverableOps
                | Self::RecoverableOther
        )
    }
}

impl fmt::Display for FolderClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the winner date came from (or none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateSource {
    Submit,
    Delivery,
    #[default]
    None,
}

impl DateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Delivery => "delivery",
            Self::None => "none",
        }
    }
}

/// Cap on distinct duplicate source names recorded on a winner.
pub const DUPLICATE_SOURCES_CAP: usize = 8;

/// Ranking context for keep-set winner selection (0075).
///
/// All new rungs default inert (0) so pre-0075 winners are preserved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RankContext {
    pub policy: KeepPolicy,
    pub prefer_path: Vec<String>,
    /// `--prefer-bcc-copy`: BCC-bearing copy ranks better.
    pub prefer_bcc_copy: bool,
    /// `--source-rank` patterns, best-first; unmatched = len (worst).
    pub source_rank_patterns: Vec<String>,
    pub folder_rank: FolderRankMode,
    /// Swap source_rank and folder_class_rank rungs.
    pub folder_class_first: bool,
    pub fidelity_mode: FidelityMode,
}

impl RankContext {
    pub fn new(policy: KeepPolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    pub fn with_prefer_path(mut self, prefer_path: impl IntoIterator<Item = String>) -> Self {
        self.prefer_path = prefer_path.into_iter().collect();
        self
    }

    pub fn from_policy_and_prefer(policy: KeepPolicy, prefer_path: &[String]) -> Self {
        Self {
            policy,
            prefer_path: prefer_path.to_vec(),
            ..Self::default()
        }
    }
}

/// Comparable ranking key. Lower is better at every component.
///
/// Order: fidelity → bcc → (source|folder per flag) → policy → path_key → nid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankKey {
    pub fidelity: u8,
    pub bcc: u8,
    pub source: u32,
    pub folder: u32,
    /// 0 = has usable policy value; 1 = missing (earliest_date undated).
    pub policy_missing: u8,
    pub policy_value: i64,
    pub path_key: String,
    pub nid: u64,
    /// When true, folder is compared before source (does not participate in Eq of values).
    pub folder_class_first: bool,
}

impl PartialOrd for RankKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let mid = if self.folder_class_first {
            self.folder
                .cmp(&other.folder)
                .then(self.source.cmp(&other.source))
        } else {
            self.source
                .cmp(&other.source)
                .then(self.folder.cmp(&other.folder))
        };
        self.fidelity
            .cmp(&other.fidelity)
            .then(self.bcc.cmp(&other.bcc))
            .then(mid)
            .then(self.policy_missing.cmp(&other.policy_missing))
            .then(self.policy_value.cmp(&other.policy_value))
            .then(self.path_key.cmp(&other.path_key))
            .then(self.nid.cmp(&other.nid))
    }
}

/// Family (parent + attach) export policy for materialization.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FamilyPolicy {
    /// Materialize parent with attachment list/bytes (default).
    #[default]
    KeepAttachmentsWithParent,
    /// Materialize parent without attachment payloads (counts/metadata OK).
    ParentsOnly,
}

impl FamilyPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KeepAttachmentsWithParent => "keep_attachments_with_parent",
            Self::ParentsOnly => "parents_only",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "keep_attachments_with_parent" => Some(Self::KeepAttachmentsWithParent),
            "parents_only" => Some(Self::ParentsOnly),
            _ => None,
        }
    }
}

impl fmt::Display for FamilyPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Decision role for one recoverable input message.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionRole {
    /// Exportable keep-set winner.
    Unique,
    /// Suppressed as duplicate of the final winner.
    DupOf,
    /// Hard materialize failure (not exportable).
    MaterializeFailed,
}

impl DecisionRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unique => "unique",
            Self::DupOf => "dup_of",
            Self::MaterializeFailed => "materialize_failed",
        }
    }
}

impl fmt::Display for DecisionRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Core types ─────────────────────────────────────────────────────────────

/// Locus of a message within a source PST (re-open key for materialize).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageLocus {
    /// Absolute source path (original casing preserved for open).
    pub source_path: String,
    /// PST file name.
    pub source_pst: String,
    /// Folder path within the PST.
    pub folder_path: String,
    /// Message NID.
    pub nid: u64,
    /// From integrity (0065); residual orphan walk not implemented.
    pub is_orphaned: bool,
}

/// One recoverable message candidate collected during Phase 1 scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoverableScanItem {
    pub locus: MessageLocus,
    /// Normalized Message-ID used for Tier 1 (empty / None = missing).
    pub message_id_norm: Option<String>,
    /// Tier 2 v1 content hash (always computed; hex stable except char-clamp exception).
    pub content_hash: [u8; 32],
    pub size: u32,
    pub integrity: RecoverableIntegrity,
    /// Stable scan order index (after path sort).
    pub scan_order: u64,
    /// PidTagClientSubmitTime FILETIME (sent). Never invent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit_time: Option<i64>,
    /// PidTagMessageDeliveryTime FILETIME (received). Never invent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_time: Option<i64>,
    /// True iff PidTagDisplayBcc present and non-empty after trim.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_bcc: bool,
    // ─── 0076 identity fields (additive; serde default for pre-0076 JSON) ──
    /// Body property was successfully read at scan time (including genuinely empty).
    /// Absent/`None` means no body property — not the same as clean empty body.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_body_preview: bool,
    /// Non-empty normalized subject present.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subject_nonempty: bool,
    /// Non-empty sender present.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sender_nonempty: bool,
    /// Attachment count used for degenerate check / identity.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attach_count: u32,
    /// Full-body SHA-256 when strong identity requested (scan-time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_char_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_cc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_bcc: Option<String>,
    /// Strong (v2) content hash when identity level ≥ body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strong_content_hash: Option<[u8; 32]>,
    /// Component fingerprints (attribution only).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub fp_header: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub fp_body: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub fp_recipients: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub fp_attachments: u64,
    /// Normalized preview exceeded 4096 bytes (hash may differ from pre-0076).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview_bytes_over_budget: bool,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

impl RecoverableScanItem {
    /// Path compare key (Windows: lowercased absolute path; else original).
    pub fn path_key(&self) -> String {
        path_compare_key(Path::new(&self.locus.source_path))
    }

    pub fn content_hash_hex(&self) -> String {
        hex_encode(&self.content_hash)
    }

    pub fn edrm_mih_hex(&self) -> Option<String> {
        self.message_id_norm
            .as_deref()
            .filter(|m| !m.is_empty())
            .map(edrm_mih_hex)
    }

    /// Body known-unreadable from integrity reasons.
    pub fn body_unreadable(&self) -> bool {
        self.integrity.degraded_reasons.iter().any(|r| {
            matches!(
                r,
                crate::integrity::IntegrityReason::BodyTruncated
                    | crate::integrity::IntegrityReason::BodyUnavailable
            )
        })
    }

    /// Tier-2 eligibility under §3.3 (ignores escape-hatch flags).
    ///
    /// `CRC_SUSPECT` is checked first (0077): kept-despite-CRC bytes must not
    /// form Tier-2 identity unless the operator opts in.
    pub fn assess_tier2_eligibility(&self) -> Result<(), Tier2IneligibleReason> {
        if self
            .integrity
            .degraded_reasons
            .contains(&crate::integrity::IntegrityReason::CrcSuspect)
        {
            return Err(Tier2IneligibleReason::CrcSuspect);
        }
        let incomplete = self
            .integrity
            .degraded_reasons
            .contains(&crate::integrity::IntegrityReason::BodyTruncated);
        let unavailable = self
            .integrity
            .degraded_reasons
            .contains(&crate::integrity::IntegrityReason::BodyUnavailable);
        tier2_eligibility(
            incomplete,
            unavailable,
            self.has_body_preview,
            self.subject_nonempty,
            self.submit_time.is_some(),
            self.sender_nonempty,
            self.attach_count as usize,
        )
    }

    /// Bind key for Tier-2 map under the given identity level.
    pub fn tier2_bind_hash(&self, identity: IdentityLevel) -> [u8; 32] {
        if identity.is_strong() {
            self.strong_content_hash.unwrap_or(self.content_hash)
        } else {
            self.content_hash
        }
    }
}

/// Keep-set winner entry (no body payload).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeepEntry {
    pub locus: MessageLocus,
    pub message_id_norm: Option<String>,
    #[serde(with = "serde_content_hash")]
    pub content_hash: [u8; 32],
    pub edrm_mih_hex: Option<String>,
    pub integrity: RecoverableIntegrity,
    pub size: u32,
    /// True when this unique won only after prior winner(s) failed materialize.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub promoted_from_failure: bool,
    /// Folder class label (0075; `primary` when ladder off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_class: Option<String>,
    /// Rung that decided this winner (0075).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    /// Distinct other sources that held a suppressed copy (basename).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub duplicate_source_count: u64,
    /// Sorted distinct source basenames, capped at [`DUPLICATE_SOURCES_CAP`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_sources: Vec<String>,
    /// True when more than [`DUPLICATE_SOURCES_CAP`] distinct sources existed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub duplicate_sources_truncated: bool,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

/// Aggregate stats for a keep-set.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KeepSetStats {
    pub recoverable: u64,
    pub unique: u64,
    pub duplicates: u64,
    pub tier1_dups: u64,
    pub tier2_dups: u64,
    pub degraded_winners: u64,
    pub materialize_failed: u64,
    pub promoted_from_failure: u64,
    pub groups_dropped_materialize: u64,
    pub groups: u64,
    /// Groups whose members resolved dates from mixed submit/delivery sources.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub groups_date_source_mixed: u64,
    /// Groups where winner lacked BCC but a peer had BCC (always computed).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub winners_without_bcc_peer_had_bcc: u64,
    /// Winners whose folder class is under Recoverable Items (signal only).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub winners_from_recoverable_items: u64,
    /// 0076 identity-binding honesty counters (additive; default empty for pre-0076 JSON).
    #[serde(default, skip_serializing_if = "grouping_stats_is_default")]
    pub grouping: GroupingStats,
    /// Mode A soft-attach promote count (0083; accepted complete peer after incomplete skip).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub promoted_after_attach_incomplete_count: u64,
    /// Mode A all-peers-incomplete Mode C fallback count (0083).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub mode_c_fallback_all_peers_incomplete_count: u64,
}

fn grouping_stats_is_default(s: &GroupingStats) -> bool {
    s == &GroupingStats::default()
}

/// Provenance of the scan that produced candidates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeepSetProvenance {
    pub scan_integrity_schema: String,
    pub mode: String,
    pub input_files: Vec<String>,
}

/// Versioned keep-set artifact (`keep_set_v1`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeepSet {
    pub schema: String,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_from: Option<KeepSetProvenance>,
    /// 0076: identity level used for binding (`off` / `body` / …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_level: Option<String>,
    /// 0076: `global` | `per-source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_scope: Option<String>,
    pub winners: Vec<KeepEntry>,
    pub stats: KeepSetStats,
}

/// One decision row for a recoverable input (Phase 3 emit only).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub source_path: String,
    pub source_pst: String,
    pub folder_path: String,
    pub is_orphaned: bool,
    pub nid: u64,
    pub message_id_norm: Option<String>,
    pub content_hash_hex: String,
    pub edrm_mih: Option<String>,
    pub role: DecisionRole,
    /// Empty when unique / materialize_failed; `message_id` | `content_hash` | `content_hash_strong` when dup_of.
    pub tier: Option<String>,
    pub winner_source_pst: Option<String>,
    pub winner_folder: Option<String>,
    pub winner_nid: Option<u64>,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub degraded: bool,
    pub degraded_reasons: Vec<String>,
    pub size: u32,
    pub promoted_from_failure: bool,
    // ─── 0075 append-only columns ───────────────────────────────────────────
    pub folder_class: String,
    pub folder_class_rank: u32,
    pub source_rank: u32,
    pub has_bcc: bool,
    /// ISO-8601 UTC when date present; empty when missing.
    pub date_filetime_utc: String,
    pub date_source: String,
    pub decided_by: String,
    /// Unique rows only: distinct other sources with a suppressed copy.
    pub duplicate_source_count: u64,
    /// Unique rows only: `|`-delimited basenames (capped).
    pub duplicate_sources: String,
    // ─── 0076 append-only columns ───────────────────────────────────────────
    /// Bind provenance: `seed` | `message_id` | `content_hash` | `content_hash_strong`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bound_by: String,
    /// `v1` | `v2`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity_version: String,
    /// Whether this item was Tier-2 eligible under active guards.
    #[serde(default)]
    pub tier2_eligible: bool,
}

// ─── Materialization ────────────────────────────────────────────────────────

/// Why method-5 nested extract failed (0094). Distinct from missing/`None`
/// (unparsed): depth/byte-budget exhaustion must map to `ATTACH_DEPTH_LIMIT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestedExtractFail {
    /// Nested object missing or unreadable — writer emits `ATTACH_EMBEDDED_UNPARSED`.
    Unparsed,
    /// Export depth or per-nest payload budget exhausted — `ATTACH_DEPTH_LIMIT`.
    DepthLimit,
}

/// Nested message payload for unique-pst method-5 export (0094 shape A).
///
/// Dedicated type — not [`CanonicalMessage`] (nests have no locus / content_hash /
/// fidelity / MIH). Mapped to `WriteMessage` by `pst-writer::from_canonical_message*`.
/// Consumed by unique-eml nested MIME reconstruct (0106).
#[derive(Clone, Debug, Default)]
pub struct NestedCanonicalMessage {
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub display_to: Option<String>,
    pub display_cc: Option<String>,
    pub display_bcc: Option<String>,
    pub recipients: Vec<CanonicalRecipient>,
    pub message_id: Option<String>,
    pub message_class: Option<String>,
    pub message_flags: Option<u32>,
    pub submit_time: Option<i64>,
    pub body_plain: Option<String>,
    pub body_html: Option<Vec<u8>>,
    /// Child attaches; method-5 children may recurse via [`CanonicalAttachment::embedded_message`].
    pub attachments: Vec<CanonicalAttachment>,
    pub body_incomplete: bool,
    pub body_unavailable: bool,
    /// Soft-skipped child attach rows during nested extract (0094).
    pub attachments_incomplete: bool,
    /// Source nested message NID (`MessageNodeRef.nid`) for child attach stream keys.
    pub source_msg_nid: Option<u64>,
}

/// Attachment metadata (and optional small payload) on a canonical message.
///
/// Production keep-set does **not** load multi-GB attach `Vec`s. When
/// [`stream_available`](Self::stream_available) is true, downstream exporters
/// (0067+) reopen via `pst-reader::open_attachment_data` using
/// [`attach_nid`](Self::attach_nid) + parent locus — that is the streaming handle.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CanonicalAttachment {
    pub filename: String,
    pub size: u32,
    pub mime: Option<String>,
    /// Optional bytes for small test fixtures / small-payload probes only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u8>>,
    /// True when list_attachments succeeded and a stream can be opened for export.
    #[serde(default)]
    pub stream_available: bool,
    /// Attachment subnode NID for `open_attachment_data` (streaming handle key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_nid: Option<u64>,
    /// PidTagAttachMethod when known (e.g. ATTACH_EMBEDDED_MSG = 0x5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_method: Option<i32>,
    /// Explicit cloud/web-ref link-only attach (0084). Prefer over overloading
    /// [`Self::stream_available`] alone so parents_only omit stays distinct.
    #[serde(default)]
    pub is_cloud_link: bool,
    /// Provider string from `PidNameAttachmentProviderType` (open string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_provider: Option<String>,
    /// Best-effort cloud URL/path for ledger actionability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_url: Option<String>,
    /// `PidNameAttachmentPermissionType` when present (0096 MAY; never invented).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_permission_type: Option<i32>,
    /// Lazy unique-pst winner nested extract (0094). Skipped in keep-set JSON.
    #[serde(skip)]
    pub embedded_message: Option<Box<NestedCanonicalMessage>>,
    /// Extract hit depth/byte budget (0094). Writer maps to `ATTACH_DEPTH_LIMIT`
    /// even when [`Self::embedded_message`] is `None`.
    #[serde(skip)]
    pub embedded_extract_limit: bool,
}

// Re-export recipient types for keep-set callers (defined in grouping for hasher use).
pub use crate::grouping::{CanonicalRecipient, CanonicalRecipientType};

/// Fully materialized winner message (bodies held one-at-a-time by callers).
#[derive(Clone, Debug)]
pub struct CanonicalMessage {
    pub locus: MessageLocus,
    pub message_id: Option<String>,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub display_to: Option<String>,
    pub display_cc: Option<String>,
    pub display_bcc: Option<String>,
    /// Structured recipient TC rows (0082). Empty when table missing — never invented from Display*.
    pub recipients: Vec<CanonicalRecipient>,
    /// `PidTagMessageFlags` when readable (0082 zero-recip anomaly); None = skip inventing UNSENT.
    pub message_flags: Option<u32>,
    pub submit_time: Option<i64>,
    pub size: Option<u32>,
    pub message_class: Option<String>,
    pub body_plain: Option<String>,
    pub body_html: Option<Vec<u8>>,
    pub attachments: Vec<CanonicalAttachment>,
    pub fidelity: RecoverableIntegrity,
    pub message_id_norm: Option<String>,
    pub content_hash: [u8; 32],
    pub edrm_mih_hex: Option<String>,
    pub body_incomplete: bool,
    pub body_unavailable: bool,
}

/// Why a Unique row was selected after peer walk (0083 three-way vocabulary).
///
/// Distinct from provisional rank `decided_by` rungs (`sole_member`, `fidelity`, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromoteReason {
    /// Hard materialize fail on earlier peer(s); later peer accepted.
    MaterializeFail,
    /// Mode A: incomplete attach on earlier peer(s); later **complete** peer accepted.
    AttachIncomplete,
    /// Mode A flag on: every materializable peer was attach-incomplete; exported highest-ranked.
    ModeCFallbackAllPeersIncomplete,
}

impl PromoteReason {
    /// Fixed `decided_by` string for decision CSV / keep-set JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MaterializeFail => "promoted_after_materialize_fail",
            Self::AttachIncomplete => "promoted_after_attach_incomplete",
            Self::ModeCFallbackAllPeersIncomplete => "mode_c_fallback_all_peers_incomplete",
        }
    }
}

/// Options for [`finalize_with_materialize_opts`] (0083 Mode A).
#[derive(Clone, Copy, Debug, Default)]
pub struct MaterializeFinalizeOpts {
    /// When true, skip attach-incomplete peers and try the next ranked peer before accept.
    /// Default **false** (Mode C ledger-only). Mode B write-time promote is not supported.
    pub promote_on_attach_fail: bool,
}

/// Soft-skipped incomplete attach locus for attach-ledger honesty (0083).
///
/// Soft skip does **not** set [`DecisionRole::MaterializeFailed`]; peers remain
/// `DupOf` the final winner. CLI may emit ledger rows with `winner_promoted=true`.
#[derive(Clone, Debug)]
pub struct SoftSkipAttachRecord {
    pub source_path: String,
    pub source_pst: String,
    pub folder_path: String,
    pub msg_nid: u64,
    pub attach_nid: Option<u64>,
    pub attach_index: u32,
    pub filename: String,
    pub size: u32,
    pub attach_method: i32,
    pub reason_code: String,
    /// Final accepted winner source path (filled after peer accept).
    pub peer_source_path: String,
    /// Final accepted winner msg nid.
    pub peer_msg_nid: u64,
    /// Cloud provider when soft-skip was for a CloudLink attach (0084).
    pub cloud_provider: String,
    /// Cloud URL when soft-skip was for a CloudLink attach (0084).
    pub cloud_url: String,
}

/// True when a materialized message is **attach-incomplete** for Mode A (0083/0084).
///
/// Normative:
/// - any attach with `stream_available == false`; **or**
/// - any attach with explicit [`CanonicalAttachment::is_cloud_link`] (no offline
///   payload); **or**
/// - fail-severity attach outcomes already bound on message fidelity
///   ([`IntegrityReason::is_attach_probe_fail`]).
///
/// **Not** incomplete solely for: body soft flags, CRC page noise (`CrcSuspect`),
/// zero-byte by-value success, or `parents_only` policy omit (omit ≠ fail — 0073).
/// Materializers must not force `stream_available=false` solely for parents_only;
/// the writer omits by family policy independently.
///
/// **0084:** attachment-table cloud/modern link-only attaches set `is_cloud_link`
/// (and typically `stream_available=false`) so Mode A can prefer a physical peer.
/// **0085:** body-only document-shaped cloud URLs are ledged via
/// `export_body_cloud_links.csv` but **must not** set this predicate (Mode A
/// known gap — physical attach peer is not preferred over HTML-inline-only).
pub fn is_attach_incomplete(msg: &CanonicalMessage) -> bool {
    if msg
        .attachments
        .iter()
        .any(|a| !a.stream_available || a.is_cloud_link)
    {
        return true;
    }
    msg.fidelity
        .degraded_reasons
        .iter()
        .any(|r| r.is_attach_probe_fail())
}

/// Build soft-skip ledger records from an incomplete materialized message.
fn soft_skip_records_for_msg(
    msg: &CanonicalMessage,
    peer_source_path: &str,
    peer_msg_nid: u64,
) -> Vec<SoftSkipAttachRecord> {
    let mut out = Vec::new();
    let fail_reason = msg
        .fidelity
        .degraded_reasons
        .iter()
        // Prefer cloud reason over generic stream/method when both present (0084).
        .find(|r| **r == crate::integrity::IntegrityReason::AttachCloudLink)
        .or_else(|| {
            msg.fidelity
                .degraded_reasons
                .iter()
                .find(|r| r.is_attach_probe_fail())
        })
        .map(|r| r.as_str())
        .unwrap_or("ATTACH_STREAM_OPEN_FAILED");

    let incomplete_attaches: Vec<(usize, &CanonicalAttachment)> = msg
        .attachments
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.stream_available || a.is_cloud_link)
        .collect();

    if incomplete_attaches.is_empty() {
        // Fidelity-only incomplete (e.g. AttachMetaFailed with empty attach list).
        out.push(SoftSkipAttachRecord {
            source_path: msg.locus.source_path.clone(),
            source_pst: msg.locus.source_pst.clone(),
            folder_path: msg.locus.folder_path.clone(),
            msg_nid: msg.locus.nid,
            attach_nid: None,
            attach_index: 0,
            filename: String::new(),
            size: 0,
            attach_method: -1,
            reason_code: fail_reason.to_string(),
            peer_source_path: peer_source_path.to_string(),
            peer_msg_nid,
            cloud_provider: String::new(),
            cloud_url: String::new(),
        });
        return out;
    }

    for (i, a) in incomplete_attaches {
        let reason = if a.is_cloud_link {
            "ATTACH_CLOUD_LINK"
        } else {
            fail_reason
        };
        out.push(SoftSkipAttachRecord {
            source_path: msg.locus.source_path.clone(),
            source_pst: msg.locus.source_pst.clone(),
            folder_path: msg.locus.folder_path.clone(),
            msg_nid: msg.locus.nid,
            attach_nid: a.attach_nid,
            attach_index: i as u32,
            filename: a.filename.clone(),
            size: a.size,
            attach_method: a.attach_method.unwrap_or(-1),
            reason_code: reason.to_string(),
            peer_source_path: peer_source_path.to_string(),
            peer_msg_nid,
            cloud_provider: a.cloud_provider.clone().unwrap_or_default(),
            cloud_url: a.cloud_url.clone().unwrap_or_default(),
        });
    }
    out
}

/// Hard materialize failure (triggers promotion). Soft issues return Ok with flags.
#[derive(Clone, Debug)]
pub enum MaterializeError {
    Hard(String),
}

impl fmt::Display for MaterializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hard(s) => write!(f, "materialize hard fail: {s}"),
        }
    }
}

impl std::error::Error for MaterializeError {}

/// Adapter that loads a message body/props/attaches for a locus (CLI holds PstFile).
pub trait MessageMaterializer {
    fn materialize(&mut self, locus: &MessageLocus) -> Result<CanonicalMessage, MaterializeError>;
}

// ─── Errors ─────────────────────────────────────────────────────────────────

/// Keep-set module errors.
#[derive(Debug)]
pub enum KeepSetError {
    Io(std::io::Error),
    Csv(String),
    Json(String),
    Other(String),
}

impl fmt::Display for KeepSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "keep-set io: {e}"),
            Self::Csv(s) => write!(f, "keep-set csv: {s}"),
            Self::Json(s) => write!(f, "keep-set json: {s}"),
            Self::Other(s) => write!(f, "keep-set: {s}"),
        }
    }
}

impl std::error::Error for KeepSetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for KeepSetError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ─── EDRM MIH ───────────────────────────────────────────────────────────────

/// EDRM Message Identifier Hash: lowercase hex MD5 of UTF-8 bytes of normalized MID.
///
/// Interop field only — not a suppress tier. Empty/missing MID should not call this.
pub fn edrm_mih_hex(message_id_norm: &str) -> String {
    let digest = Md5::digest(message_id_norm.as_bytes());
    hex_encode(digest.as_slice())
}

// ─── Path sorting (deterministic) ───────────────────────────────────────────

/// Compare key for absolute paths: case-insensitive on Windows, case-sensitive elsewhere.
pub fn path_compare_key(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.into_owned()
    }
}

/// Sort absolute input paths for deterministic scan order.
///
/// Windows: lexicographic on lowercased absolute path (original path preserved for open).
/// Non-Windows: lexicographic on absolute path as-is.
pub fn sort_input_paths(paths: &mut [PathBuf]) {
    paths.sort_by_key(|a| path_compare_key(a));
}

// ─── Grouping (DedupIndex semantics, collect all members) ───────────────────

/// Result of [`group_candidates_with_stats`]: groups + per-item bind provenance + stats.
#[derive(Clone, Debug)]
pub struct GroupingOutcome {
    /// Groups of indices into items (scan order preserved within groups).
    pub groups: Vec<Vec<usize>>,
    /// Bind provenance per item index (same length as items).
    pub bound_by: Vec<BoundBy>,
    /// Tier-2 eligibility per item under the active context.
    pub tier2_eligible: Vec<bool>,
    pub stats: GroupingStats,
}

/// Group candidates with default 0076 context (guards on). Prefer
/// [`group_candidates_ctx`] when flags are available.
pub fn group_candidates(items: &[RecoverableScanItem], tier2_enabled: bool) -> Vec<Vec<usize>> {
    group_candidates_ctx(items, &GroupingContext::with_tier2(tier2_enabled)).groups
}

/// Group under an explicit [`GroupingContext`].
pub fn group_candidates_ctx(
    items: &[RecoverableScanItem],
    ctx: &GroupingContext,
) -> GroupingOutcome {
    group_candidates_with_stats(items, ctx)
}

/// Full grouping: same rules as [`crate::DedupIndex`], collecting all members.
pub fn group_candidates_with_stats(
    items: &[RecoverableScanItem],
    ctx: &GroupingContext,
) -> GroupingOutcome {
    let n = items.len();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_bound_mid: Vec<Option<String>> = Vec::new();
    let mut group_fp: Vec<(u64, u64, u64, u64)> = Vec::new(); // body, header, recip, attach
    let mut group_bind_hash: Vec<[u8; 32]> = Vec::new();
    let mut mid_to_group: HashMap<String, usize> = HashMap::new();
    let mut hash_to_group: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut bound_by = vec![BoundBy::Seed; n];
    let mut tier2_eligible = vec![true; n];
    let mut stats = GroupingStats::default();
    let mut cross_mid_hash_seen: HashSet<Vec<u8>> = HashSet::new();
    let mut cross_mid_cluster: HashMap<Vec<u8>, HashSet<String>> = HashMap::new();

    for (i, item) in items.iter().enumerate() {
        if item.preview_bytes_over_budget {
            stats.tier2_preview_bytes_over_budget += 1;
        }

        let eligible = match item.assess_tier2_eligibility() {
            Ok(()) => true,
            Err(Tier2IneligibleReason::UnreadableBody) => {
                if ctx.enforce_readable_body() {
                    stats.tier2_blocked_unreadable_body += 1;
                    false
                } else {
                    true
                }
            }
            Err(Tier2IneligibleReason::Degenerate) => {
                if ctx.enforce_readable_body() {
                    stats.tier2_blocked_degenerate += 1;
                    false
                } else {
                    true
                }
            }
            Err(Tier2IneligibleReason::CrcSuspect) => {
                // Operator flag only (0077). Poly dual-rate clears false-positive
                // CRC_SUSPECT before keep-set; sparse corruption still blocks.
                if ctx.allow_crc_suspect_tier2_for(&item.path_key()) {
                    true
                } else {
                    stats.tier2_blocked_crc_suspect += 1;
                    false
                }
            }
        };
        tier2_eligible[i] = eligible;

        let mid = mid_present(item.message_id_norm.as_deref()).map(|s| s.to_string());
        let scope_prefix = match ctx.scope {
            DedupeScope::Global => String::new(),
            DedupeScope::PerSource => format!("{}\0", item.path_key()),
        };

        let mut found: Option<(usize, BoundBy)> = None;

        // Tier 1
        if let Some(ref m) = mid {
            let key = format!("{scope_prefix}{m}");
            if let Some(&gid) = mid_to_group.get(&key) {
                // §3.7: always report divergence, optionally split (tier1_verify).
                let (gb, gh, gr, ga) = group_fp[gid];
                let body_diff = item.fp_body != gb;
                let meta_diff = item.fp_header != gh || item.fp_attachments != ga;
                let recip_diff = item.fp_recipients != gr;
                if body_diff {
                    stats.tier1_divergent_body += 1;
                } else if recip_diff && !meta_diff {
                    stats.tier1_divergent_recipients += 1;
                } else if meta_diff {
                    stats.tier1_divergent_metadata += 1;
                }
                let split = match ctx.tier1_verify {
                    crate::grouping::Tier1Verify::Off => false,
                    crate::grouping::Tier1Verify::Content => {
                        item.tier2_bind_hash(ctx.identity) != group_bind_hash[gid]
                    }
                    crate::grouping::Tier1Verify::Body => item.fp_body != group_fp[gid].0,
                };
                if !split {
                    found = Some((gid, BoundBy::MessageId));
                }
            }
        }

        // Tier 2
        if found.is_none() && ctx.tier2_enabled && eligible {
            let bind_hash = item.tier2_bind_hash(ctx.identity);
            let mut hkey = scope_prefix.as_bytes().to_vec();
            hkey.extend_from_slice(&bind_hash);
            if let Some(&gid) = hash_to_group.get(&hkey) {
                let (ok, new_bound) = mid_join_compatible(
                    group_bound_mid[gid].as_deref(),
                    mid.as_deref(),
                    ctx.block_cross_mid(),
                );
                if ok {
                    if let Some(nb) = new_bound {
                        if group_bound_mid[gid].is_none() {
                            group_bound_mid[gid] = Some(nb.clone());
                            let mk = format!("{scope_prefix}{nb}");
                            mid_to_group.entry(mk).or_insert(gid);
                        }
                    }
                    let bb = if ctx.identity.is_strong() {
                        BoundBy::StrongContentHash
                    } else {
                        BoundBy::ContentHash
                    };
                    found = Some((gid, bb));
                } else {
                    stats.cross_mid_blocked += 1;
                    if cross_mid_hash_seen.insert(hkey.clone()) {
                        stats.cross_mid_blocked_groups += 1;
                    }
                    if let Some(ref m) = mid {
                        let cluster = cross_mid_cluster.entry(hkey.clone()).or_default();
                        if let Some(ref existing) = group_bound_mid[gid] {
                            cluster.insert(existing.clone());
                        }
                        cluster.insert(m.clone());
                        stats.cross_mid_blocked_max_group =
                            stats.cross_mid_blocked_max_group.max(cluster.len() as u64);
                    }
                }
            }
        }

        if let Some((gid, bb)) = found {
            groups[gid].push(i);
            bound_by[i] = bb;
        } else {
            let gid = groups.len();
            groups.push(vec![i]);
            group_bound_mid.push(mid.clone());
            group_fp.push((
                item.fp_body,
                item.fp_header,
                item.fp_recipients,
                item.fp_attachments,
            ));
            let bind_hash = item.tier2_bind_hash(ctx.identity);
            group_bind_hash.push(bind_hash);
            bound_by[i] = BoundBy::Seed;

            if let Some(ref m) = mid {
                let key = format!("{scope_prefix}{m}");
                mid_to_group.insert(key, gid);
            }
            if ctx.tier2_enabled && eligible {
                let mut hkey = scope_prefix.as_bytes().to_vec();
                hkey.extend_from_slice(&bind_hash);
                // Only first group owns a given hash key (cross-MID later items stay out).
                hash_to_group.entry(hkey).or_insert(gid);
            }
        }

        // X.500-looking display recipients (honesty; independent of identity level).
        if recipient_strings_have_x500(item) {
            stats.x500_recipient_items += 1;
        }
    }

    // ── tier1_backfill (D6 residual): always count; merge only when flag on ──
    // Missed merges: same v1 content_hash across groups whose bound MIDs are
    // compatible and at least one non-empty MID is involved (not bare degenerate).
    // Streaming DedupIndex cannot retro-merge; keep-set path owns this post-pass.
    // Attribution runs *after* merge so counters describe final grouping.
    apply_tier1_backfill_pass(
        items,
        ctx,
        &mut groups,
        &mut group_bound_mid,
        &mut bound_by,
        &mut stats,
    );

    // ── Tier 2.5 split attribution (eligible v1-equal items that did not co-group) ──
    if ctx.identity.is_strong() {
        attribute_tier2_5_splits(items, &groups, &tier2_eligible, &mut stats);
    }

    GroupingOutcome {
        groups,
        bound_by,
        tier2_eligible,
        stats,
    }
}

fn recipient_strings_have_x500(item: &RecoverableScanItem) -> bool {
    use crate::grouping::recipient_has_x500;
    item.display_to.as_deref().is_some_and(recipient_has_x500)
        || item.display_cc.as_deref().is_some_and(recipient_has_x500)
        || item.display_bcc.as_deref().is_some_and(recipient_has_x500)
}

/// Attribute Tier-2.5 splits: **eligible** items that share v1 `content_hash`
/// but land in different groups under a strong identity level (final groups).
fn attribute_tier2_5_splits(
    items: &[RecoverableScanItem],
    groups: &[Vec<usize>],
    tier2_eligible: &[bool],
    stats: &mut GroupingStats,
) {
    let mut item_gid = vec![0usize; items.len()];
    for (gid, members) in groups.iter().enumerate() {
        for &i in members {
            if i < item_gid.len() {
                item_gid[i] = gid;
            }
        }
    }

    let mut v1_to_items: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        // Guard-separated / ineligible items are not Tier-2.5 identity splits.
        if !tier2_eligible.get(i).copied().unwrap_or(true) {
            continue;
        }
        v1_to_items.entry(item.content_hash).or_default().push(i);
    }

    for idxs in v1_to_items.values() {
        if idxs.len() < 2 {
            continue;
        }
        let mut gids: HashSet<usize> = HashSet::new();
        for &i in idxs {
            gids.insert(item_gid[i]);
        }
        if gids.len() <= 1 {
            continue;
        }
        // One split edge per extra group in this v1 family.
        stats.tier2_5_splits += (gids.len() as u64).saturating_sub(1);

        // Attribute using first item per group as representative.
        let mut reps: Vec<usize> = Vec::new();
        let mut seen_g = HashSet::new();
        for &i in idxs {
            let g = item_gid[i];
            if seen_g.insert(g) {
                reps.push(i);
            }
        }
        // Compare consecutive representatives for component-only attribution.
        for w in reps.windows(2) {
            let a = &items[w[0]];
            let b = &items[w[1]];
            let body_same = a.fp_body == b.fp_body;
            let header_same = a.fp_header == b.fp_header;
            let attach_same = a.fp_attachments == b.fp_attachments;
            let recip_diff = a.fp_recipients != b.fp_recipients;
            if body_same && header_same && attach_same && recip_diff {
                stats.tier2_5_splits_recipients_only += 1;
                // BCC-only: normalized to/cc match while BCC differs.
                let to_same = recip_display_eq(a.display_to.as_deref(), b.display_to.as_deref());
                let cc_same = recip_display_eq(a.display_cc.as_deref(), b.display_cc.as_deref());
                let bcc_diff =
                    !recip_display_eq(a.display_bcc.as_deref(), b.display_bcc.as_deref())
                        || a.has_bcc != b.has_bcc;
                if to_same && cc_same && bcc_diff {
                    stats.tier2_5_splits_bcc_only += 1;
                }
            }
        }
    }
}

fn recip_display_eq(a: Option<&str>, b: Option<&str>) -> bool {
    use crate::grouping::normalize_recipients;
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => normalize_recipients(x) == normalize_recipients(y),
        _ => false,
    }
}

/// Union-find post-pass for `--tier1-backfill` (D6 residual).
///
/// Always increments `tier1_backfill_candidates`. When `ctx.tier1_backfill` is
/// true, merges groups that share a v1 content hash with compatible MIDs and at
/// least one non-empty MID in the cluster (never merges bare degenerate pairs).
///
/// **Scope:** under [`DedupeScope::PerSource`], candidates are partitioned by
/// `path_compare_key(source_path)` so custodial partitions never cross-merge.
///
/// **Provenance:** after a merge, non-seed members that remain `BoundBy::Seed`
/// are reclassified (MID match → `MessageId`, else content/strong hash).
fn apply_tier1_backfill_pass(
    items: &[RecoverableScanItem],
    ctx: &GroupingContext,
    groups: &mut Vec<Vec<usize>>,
    group_bound_mid: &mut Vec<Option<String>>,
    bound_by: &mut [BoundBy],
    stats: &mut GroupingStats,
) {
    let ng = groups.len();
    if ng == 0 || items.is_empty() {
        return;
    }

    let mut item_gid = vec![0usize; items.len()];
    for (gid, members) in groups.iter().enumerate() {
        for &i in members {
            if i < item_gid.len() {
                item_gid[i] = gid;
            }
        }
    }

    // parent[i] = representative group id
    let mut parent: Vec<usize> = (0..ng).collect();
    let mut rank: Vec<u8> = vec![0; ng];
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    fn unite(parent: &mut [usize], rank: &mut [u8], a: usize, b: usize) -> bool {
        let mut ra = find(parent, a);
        let mut rb = find(parent, b);
        if ra == rb {
            return false;
        }
        if rank[ra] < rank[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        parent[rb] = ra;
        if rank[ra] == rank[rb] {
            rank[ra] = rank[ra].saturating_add(1);
        }
        true
    }

    // Index items by (optional scope key, v1 content hash) so PerSource never
    // unites across custodians (Codex F-backfill×scope).
    let mut by_key: HashMap<(String, [u8; 32]), Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        let scope_key = match ctx.scope {
            DedupeScope::Global => String::new(),
            DedupeScope::PerSource => item.path_key(),
        };
        by_key
            .entry((scope_key, item.content_hash))
            .or_default()
            .push(i);
    }

    let mut unions = 0u64;
    for idxs in by_key.values() {
        if idxs.len() < 2 {
            continue;
        }
        // Distinct groups present for this hash.
        let mut gids: Vec<usize> = idxs.iter().map(|&i| item_gid[i]).collect();
        gids.sort_unstable();
        gids.dedup();
        if gids.len() < 2 {
            continue;
        }

        // Pairwise: unite when bound MIDs compatible and cluster involves a real MID.
        for i in 0..gids.len() {
            for j in (i + 1)..gids.len() {
                let ga = gids[i];
                let gb = gids[j];
                let ma = group_bound_mid.get(ga).and_then(|m| m.as_deref());
                let mb = group_bound_mid.get(gb).and_then(|m| m.as_deref());
                let has_mid = mid_present(ma).is_some() || mid_present(mb).is_some();
                if !has_mid {
                    continue;
                }
                let (ok, _) = mid_join_compatible(ma, mb, true);
                if ok && unite(&mut parent, &mut rank, ga, gb) {
                    unions += 1;
                }
            }
        }
    }

    // Candidates = how many groups would be absorbed (edges united in the UF).
    stats.tier1_backfill_candidates += unions;

    if !ctx.tier1_backfill || unions == 0 {
        return;
    }

    // Rebuild groups from union-find (preserve first-seen seed order).
    let mut root_to_new: HashMap<usize, usize> = HashMap::new();
    let mut new_groups: Vec<Vec<usize>> = Vec::new();
    let mut new_bounds: Vec<Option<String>> = Vec::new();

    for (old_gid, members) in groups.iter().enumerate() {
        let root = find(&mut parent, old_gid);
        let new_gid = if let Some(&ngid) = root_to_new.get(&root) {
            ngid
        } else {
            let ngid = new_groups.len();
            root_to_new.insert(root, ngid);
            new_groups.push(Vec::new());
            new_bounds.push(group_bound_mid.get(root).cloned().unwrap_or(None));
            ngid
        };
        new_groups[new_gid].extend(members.iter().copied());
        // Prefer a non-empty bound MID when merging.
        if new_bounds[new_gid]
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            if let Some(b) = group_bound_mid.get(old_gid).cloned().flatten() {
                if !b.is_empty() {
                    new_bounds[new_gid] = Some(b);
                }
            }
        }
    }

    // Keep members in scan-order within each group.
    for g in &mut new_groups {
        g.sort_unstable();
    }

    // Reclassify bind provenance for absorbed seeds (former BoundBy::Seed that
    // are no longer the group's first-seen member).
    for g in &new_groups {
        if g.is_empty() {
            continue;
        }
        let seed = g[0];
        if seed < bound_by.len() {
            bound_by[seed] = BoundBy::Seed;
        }
        let seed_mid = items.get(seed).and_then(|it| it.message_id_norm.as_deref());
        for &idx in g.iter().skip(1) {
            if idx >= bound_by.len() {
                continue;
            }
            // Only reclassify former seeds / stale Seed tags after merge.
            if !matches!(bound_by[idx], BoundBy::Seed) {
                continue;
            }
            let member_mid = items.get(idx).and_then(|it| it.message_id_norm.as_deref());
            let mid_match = matches!(
                (mid_present(seed_mid), mid_present(member_mid)),
                (Some(a), Some(b)) if a == b
            );
            bound_by[idx] = if mid_match {
                BoundBy::MessageId
            } else if ctx.identity.is_strong() {
                BoundBy::StrongContentHash
            } else {
                BoundBy::ContentHash
            };
        }
    }

    *groups = new_groups;
    *group_bound_mid = new_bounds;
}

// ─── Ranking / resolve ──────────────────────────────────────────────────────

/// Graded fidelity tier for one [`IntegrityReason`] (0075 §3.6).
///
/// Unmapped reasons default to tier 3 (fail-worse). Exhaustive match so new
/// variants are a compile error until classified.
pub fn reason_fidelity_tier(reason: crate::integrity::IntegrityReason) -> u8 {
    use crate::integrity::IntegrityReason::*;
    match reason {
        // tier 1 — soft / metadata only
        AttachMetaFailed | AttachProbeTruncated | AttachPeerProbeCap | AttachProbeTimeout => 1,
        // tier 2 — attachment payload loss
        AttachStreamOpenFailed
        | AttachStreamReadFailed
        | AttachStreamCrc
        | AttachBlockNotFound
        | AttachDataTruncated
        | AttachMethodUnsupported
        | AttachCloudLink => 2,
        // tier 3 — body / data loss
        BodyTruncated | BodyUnavailable | DataTruncated | CrcMismatch | CrcSuspect
        | BlockNotFound => 3,
        // tier 4 — structural / provenance
        OrphanedNode | InvalidStructure | MessageReadFailed | PropertyError | NodeNotFound => 4,
        // File-level / open failures on a recoverable item are structural-class.
        OpenFailed | AnsiUnsupported | UnsupportedCrypt | FolderWalkFailed | PathNotFound
        | NotPst | ReadError => 4,
    }
}

/// Fidelity rank: lower is better.
///
/// Binary (default): 0 = clean, 1 = degraded/orphaned (pre-0075).
/// Graded: worst tier across reasons (0..4); binary maps `{0}→0`, `{1..4}→1`.
pub fn fidelity_rank(item: &RecoverableScanItem) -> u8 {
    fidelity_rank_with_mode(item, FidelityMode::Binary)
}

/// Fidelity rank under an explicit mode.
pub fn fidelity_rank_with_mode(item: &RecoverableScanItem, mode: FidelityMode) -> u8 {
    let graded = graded_fidelity_rank(item);
    match mode {
        FidelityMode::Binary => {
            if graded == 0 {
                0
            } else {
                1
            }
        }
        FidelityMode::Graded => graded,
    }
}

fn graded_fidelity_rank(item: &RecoverableScanItem) -> u8 {
    if !item.integrity.degraded && !item.integrity.is_orphaned {
        return 0;
    }
    let mut worst: u8 = 0;
    if item.integrity.is_orphaned {
        worst = worst.max(4);
    }
    for r in &item.integrity.degraded_reasons {
        worst = worst.max(reason_fidelity_tier(*r));
    }
    // Degraded flag with no reasons → fail-worse tier 3.
    if worst == 0 {
        3
    } else {
        worst
    }
}

/// Resolve usable date for ranking / CSV. Submit preferred; delivery fallback.
/// FILETIME `<= 0` is missing. Never invents a date.
pub fn resolve_item_date(item: &RecoverableScanItem) -> (Option<i64>, DateSource) {
    if let Some(t) = item.submit_time {
        if t > 0 {
            return (Some(t), DateSource::Submit);
        }
    }
    if let Some(t) = item.delivery_time {
        if t > 0 {
            return (Some(t), DateSource::Delivery);
        }
    }
    (None, DateSource::None)
}

/// Format FILETIME as RFC3339 UTC (second resolution), empty when missing.
///
/// Accepts any `ft > 0` (same gate as [`resolve_item_date`]), including pre-1970
/// civil dates (FILETIME epoch is 1601). Negative Unix seconds are formatted via
/// a signed Howard Hinnant civil-date algorithm — no chrono dep.
pub fn format_date_filetime_utc(ft: Option<i64>) -> String {
    let Some(ft) = ft else {
        return String::new();
    };
    if ft <= 0 {
        return String::new();
    }
    // FILETIME → Unix seconds (same formula as pst-reader / resolve_item_date).
    let unix = (ft / 10_000_000) - 11_644_473_600;
    format_unix_secs_rfc3339_i64(unix)
}

/// Civil date from signed Unix seconds (UTC) — Howard Hinnant algorithm.
fn format_unix_secs_rfc3339_i64(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400) as u64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Classify a folder path with the built-in ladder (pure string; no PST I/O).
///
/// Whole-segment, case-insensitive; Recoverable Items subfolders are parent-qualified.
/// When **any** classes match (recoverable and/or non-recoverable segments), the class with
/// the **lowest** [`FolderClass::builtin_rank`] wins (best/preferable class). That keeps
/// e.g. `Sent Items` (rank 0) ahead of a co-present recoverable class on a pathological path.
pub fn classify_folder(folder_path: &str) -> FolderClass {
    let segs: Vec<String> = folder_path
        .split('/')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if segs.is_empty() {
        return FolderClass::Primary;
    }

    let mut best: Option<FolderClass> = None;

    // Recoverable Items children (parent-qualified) — candidates only; do not short-circuit.
    if let Some(class) = classify_recoverable(&segs) {
        best = Some(min_rank_class(best, class));
    }

    // Non-recoverable class matches on any segment.
    for seg in &segs {
        if let Some(c) = match_non_recoverable_segment(seg) {
            best = Some(min_rank_class(best, c));
        }
    }
    best.unwrap_or(FolderClass::Primary)
}

/// Match a single path segment to a non-recoverable folder class (if any).
fn match_non_recoverable_segment(seg: &str) -> Option<FolderClass> {
    if seg_eq(seg, "Sent Items") || seg_eq(seg, "Sent Mail") {
        return Some(FolderClass::SentItems);
    }
    if seg_eq(seg, "Deleted Items") {
        return Some(FolderClass::DeletedItems);
    }
    if seg_eq(seg, "Outbox") {
        return Some(FolderClass::Outbox);
    }
    if seg_eq(seg, "Drafts") {
        return Some(FolderClass::Drafts);
    }
    if seg_eq(seg, "Junk Email") || seg_eq(seg, "Junk E-mail") || seg_eq(seg, "Spam") {
        return Some(FolderClass::JunkEmail);
    }
    if seg_eq(seg, "Archive")
        || seg_eq(seg, "Online Archive")
        || segment_glob_match(seg, "In-Place Archive*")
    {
        return Some(FolderClass::Archive);
    }
    None
}

fn min_rank_class(current: Option<FolderClass>, candidate: FolderClass) -> FolderClass {
    match current {
        None => candidate,
        Some(c) if candidate.builtin_rank() < c.builtin_rank() => candidate,
        Some(c) => c,
    }
}

fn classify_recoverable(segs: &[String]) -> Option<FolderClass> {
    let ri = segs.iter().position(|s| seg_eq(s, "Recoverable Items"))?;
    // Child under Recoverable Items (any descendant segment after RI).
    let after = &segs[ri + 1..];
    if after.is_empty() {
        return Some(FolderClass::RecoverableOther);
    }
    // Min rank among known recoverable subfolder matches (paths may nest).
    let mut best: Option<FolderClass> = None;
    for seg in after {
        if let Some(c) = match_recoverable_segment(seg) {
            best = Some(min_rank_class(best, c));
        }
    }
    Some(best.unwrap_or(FolderClass::RecoverableOther))
}

fn match_recoverable_segment(seg: &str) -> Option<FolderClass> {
    if seg_eq(seg, "Deletions") {
        return Some(FolderClass::RecoverableDeletions);
    }
    if seg_eq(seg, "DiscoveryHolds") || seg_eq(seg, "SubstrateHolds") {
        return Some(FolderClass::RecoverableHolds);
    }
    if seg_eq(seg, "Purges") {
        return Some(FolderClass::RecoverablePurges);
    }
    if seg_eq(seg, "Versions") {
        return Some(FolderClass::RecoverableVersions);
    }
    if seg_eq(seg, "Audits") || seg_eq(seg, "Calendar Logging") {
        return Some(FolderClass::RecoverableOps);
    }
    None
}

fn seg_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Segment glob: `*` only at start and/or end of the *pattern segment*.
/// Does not cross `/`. No regex.
pub fn segment_glob_match(segment: &str, pattern: &str) -> bool {
    let seg = segment.to_ascii_lowercase();
    let pat = pattern.to_ascii_lowercase();
    if !pat.contains('*') {
        return seg == pat;
    }
    // Only leading and/or trailing * supported within one segment.
    let leading = pat.starts_with('*');
    let trailing = pat.ends_with('*');
    let core = pat.trim_matches('*');
    if core.is_empty() {
        return true; // "*" alone
    }
    if leading && trailing {
        return seg.contains(core);
    }
    if leading {
        return seg.ends_with(core);
    }
    if trailing {
        return seg.starts_with(core);
    }
    // Internal * not supported → exact after stripping nothing useful.
    seg == pat
}

/// Multi-segment pattern match: consecutive segments; last pattern segment is
/// ancestor-or-self of the message folder (pattern may be shorter than path).
fn path_matches_folder_pattern(folder_segments: &[String], pattern: &str) -> bool {
    let pat_segs: Vec<&str> = pattern
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if pat_segs.is_empty() {
        return false;
    }
    if pat_segs.len() > folder_segments.len() {
        return false;
    }
    // Sliding window of consecutive segments.
    let n = folder_segments.len();
    let m = pat_segs.len();
    for start in 0..=(n - m) {
        let mut ok = true;
        for i in 0..m {
            if !segment_glob_match(&folder_segments[start + i], pat_segs[i]) {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

/// Folder class + rank for an item under the given mode.
pub fn folder_class_and_rank(folder_path: &str, mode: &FolderRankMode) -> (FolderClass, u32) {
    let class = classify_folder(folder_path);
    match mode {
        FolderRankMode::Off => (FolderClass::Primary, 0),
        FolderRankMode::Builtin => (class, class.builtin_rank()),
        FolderRankMode::Custom(patterns) => {
            let segs: Vec<String> = folder_path
                .split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            for (i, pat) in patterns.iter().enumerate() {
                if path_matches_folder_pattern(&segs, pat) {
                    return (class, 1 + i as u32);
                }
            }
            // Unmatched = best (0). Keep classified label for explainability.
            (class, 0)
        }
    }
}

/// Source rank: index of first matching pattern on path_compare_key; unmatched = len.
pub fn source_rank_of(item: &RecoverableScanItem, patterns: &[String]) -> u32 {
    if patterns.is_empty() {
        return 0;
    }
    let key = item.path_key();
    for (i, pat) in patterns.iter().enumerate() {
        if pat.is_empty() {
            continue;
        }
        // path_compare_key is already lowercased on Windows; patterns match case-insensitively.
        let needle = if cfg!(windows) {
            pat.to_ascii_lowercase()
        } else {
            pat.clone()
        };
        if key.contains(&needle) {
            return i as u32;
        }
    }
    patterns.len() as u32
}

fn prefer_path_policy_key(item: &RecoverableScanItem, prefer_path: &[String]) -> i64 {
    let path_hay = format!("{}|{}", item.locus.source_path, item.locus.folder_path);
    let matches = prefer_path.iter().any(|p| {
        if p.is_empty() {
            return false;
        }
        if cfg!(windows) {
            path_hay.to_lowercase().contains(&p.to_lowercase())
        } else {
            path_hay.contains(p.as_str())
        }
    });
    if matches {
        0
    } else {
        1
    }
}

/// Ranking key: lower is better winner.
///
/// Ladder: fidelity → bcc → source → folder (or folder→source) → policy → path → nid.
/// New rungs are 0 when their flags are absent (pre-0075 winners preserved).
pub fn rank_key(item: &RecoverableScanItem, ctx: &RankContext) -> RankKey {
    let fidelity = fidelity_rank_with_mode(item, ctx.fidelity_mode);
    let bcc = if ctx.prefer_bcc_copy {
        if item.has_bcc {
            0
        } else {
            1
        }
    } else {
        0
    };
    let source = source_rank_of(item, &ctx.source_rank_patterns);
    let (_class, folder) = folder_class_and_rank(&item.locus.folder_path, &ctx.folder_rank);

    let (policy_missing, policy_value) = match ctx.policy {
        KeepPolicy::FirstSeen => (0u8, item.scan_order as i64),
        KeepPolicy::KeepLargest => (0u8, -(item.size as i64)),
        KeepPolicy::PreferPath => (0u8, prefer_path_policy_key(item, &ctx.prefer_path)),
        KeepPolicy::EarliestDate => {
            let (date, _) = resolve_item_date(item);
            match date {
                Some(ft) => (0u8, ft),
                None => (1u8, 0i64),
            }
        }
    };

    RankKey {
        fidelity,
        bcc,
        source,
        folder,
        policy_missing,
        policy_value,
        path_key: item.path_key(),
        nid: item.locus.nid,
        folder_class_first: ctx.folder_class_first,
    }
}

/// Compute `decided_by` vocabulary token by comparing two rank keys.
///
/// `self_is_winner`: when true, report the rung that beat the rival; when false,
/// report the rung at which `self_key` lost to `other_key` (the winner).
pub fn decided_by_rung(
    self_key: &RankKey,
    other_key: &RankKey,
    policy: KeepPolicy,
    self_is_winner: bool,
) -> &'static str {
    let (a, b) = if self_is_winner {
        (self_key, other_key)
    } else {
        // For losers, still find first component where self is worse than winner.
        (self_key, other_key)
    };
    if a.fidelity != b.fidelity {
        return "fidelity";
    }
    if a.bcc != b.bcc {
        return "bcc_completeness";
    }
    if a.folder_class_first {
        if a.folder != b.folder {
            return "folder_class";
        }
        if a.source != b.source {
            return "source_rank";
        }
    } else {
        if a.source != b.source {
            return "source_rank";
        }
        if a.folder != b.folder {
            return "folder_class";
        }
    }
    if a.policy_missing != b.policy_missing || a.policy_value != b.policy_value {
        return match policy {
            KeepPolicy::FirstSeen => "policy_first_seen",
            KeepPolicy::KeepLargest => "policy_keep_largest",
            KeepPolicy::PreferPath => "policy_prefer_path",
            KeepPolicy::EarliestDate => "policy_earliest_date",
        };
    }
    if a.path_key != b.path_key {
        return "path_order";
    }
    if a.nid != b.nid {
        return "nid";
    }
    // Keys equal (should be rare for distinct items) — fall through to path_order.
    "path_order"
}

/// Human-summary hint when any winner came from Recoverable Items (signal only).
pub fn recoverable_items_hint(winners_from_recoverable_items: u64) -> Option<String> {
    if winners_from_recoverable_items == 0 {
        return None;
    }
    Some(format!(
        "{winners_from_recoverable_items} winner(s) came from Recoverable Items folders; \
         consider re-running with --prefer-folder-class to prefer live-mailbox copies"
    ))
}

/// Aggregate distinct other source basenames for a winner (cap [`DUPLICATE_SOURCES_CAP`]).
pub fn duplicate_source_aggregate(
    items: &[RecoverableScanItem],
    group: &[usize],
    winner_idx: usize,
) -> (u64, Vec<String>, bool) {
    let winner_pst = items[winner_idx].locus.source_pst.as_str();
    let mut names: Vec<String> = Vec::new();
    for &idx in group {
        if idx == winner_idx {
            continue;
        }
        let base = source_basename(&items[idx].locus.source_pst);
        // Exclude winner's own source name from the "other sources" set.
        if base == source_basename(winner_pst) {
            // Same source file holding another copy — still a distinct row but
            // "All Custodians" is about other sources; skip same basename.
            continue;
        }
        if !names.iter().any(|n| n == &base) {
            names.push(base);
        }
    }
    names.sort();
    let total = names.len() as u64;
    let truncated = names.len() > DUPLICATE_SOURCES_CAP;
    names.truncate(DUPLICATE_SOURCES_CAP);
    (total, names, truncated)
}

fn source_basename(source_pst: &str) -> String {
    Path::new(source_pst)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_pst.to_string())
}

/// Provisional resolve state before materialize promotion.
#[derive(Clone, Debug)]
pub struct ResolvedKeepSet {
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path: Vec<String>,
    /// Full ranking context (0075); prefer_path mirrored for back-compat fields.
    pub rank_ctx: RankContext,
    pub tier2_enabled: bool,
    /// 0076 grouping context (identity / scope / guards).
    pub grouping_ctx: GroupingContext,
    pub items: Vec<RecoverableScanItem>,
    /// Groups of indices into `items`.
    pub groups: Vec<Vec<usize>>,
    /// Provisional winner index per group (into `items`).
    pub provisional_winners: Vec<Option<usize>>,
    /// Final role per item (updated by materialize promotion).
    pub roles: Vec<DecisionRole>,
    /// Winner item index for each item (self if unique).
    pub winner_of: Vec<Option<usize>>,
    /// Tier string for dup_of rows.
    pub tier_of: Vec<Option<String>>,
    /// Bind provenance per item (0076; recorded at group time).
    pub bound_by: Vec<BoundBy>,
    /// Tier-2 eligibility per item under active guards.
    pub tier2_eligible: Vec<bool>,
    /// Grouping honesty stats.
    pub grouping_stats: GroupingStats,
    /// Per-item promoted_from_failure flag.
    pub promoted_from_failure: Vec<bool>,
    /// Per-item promote reason when Unique was selected via peer walk (0083).
    pub promote_reason: Vec<Option<PromoteReason>>,
    /// Soft-skipped incomplete attach rows for ledger honesty (0083 Mode A).
    pub soft_skip_attach_records: Vec<SoftSkipAttachRecord>,
    /// Per-group: true if all materialize attempts failed.
    pub group_dropped: Vec<bool>,
    pub created_from: Option<KeepSetProvenance>,
}

impl ResolvedKeepSet {
    /// Build keep-set JSON structure from current finalized roles.
    pub fn to_keep_set(&self) -> KeepSet {
        let mut winners = Vec::new();
        let mut stats = KeepSetStats {
            recoverable: self.items.len() as u64,
            groups: self.groups.len() as u64,
            groups_dropped_materialize: self.group_dropped.iter().filter(|d| **d).count() as u64,
            ..KeepSetStats::default()
        };

        // Honesty stats always computed (even when flags off).
        for group in &self.groups {
            if group.is_empty() {
                continue;
            }
            // Mixed date sources.
            let mut saw_submit = false;
            let mut saw_delivery = false;
            for &idx in group {
                match resolve_item_date(&self.items[idx]).1 {
                    DateSource::Submit => saw_submit = true,
                    DateSource::Delivery => saw_delivery = true,
                    DateSource::None => {}
                }
            }
            if saw_submit && saw_delivery {
                stats.groups_date_source_mixed += 1;
            }
            // BCC loss: winner without BCC, peer had BCC.
            let winner_idx = group
                .iter()
                .copied()
                .find(|&i| self.roles[i] == DecisionRole::Unique);
            if let Some(wi) = winner_idx {
                if !self.items[wi].has_bcc && group.iter().any(|&i| self.items[i].has_bcc) {
                    stats.winners_without_bcc_peer_had_bcc += 1;
                }
            }
        }

        for (i, item) in self.items.iter().enumerate() {
            match self.roles[i] {
                DecisionRole::Unique => {
                    stats.unique += 1;
                    if item.integrity.degraded {
                        stats.degraded_winners += 1;
                    }
                    if self.promoted_from_failure[i] {
                        stats.promoted_from_failure += 1;
                    }
                    match self.promote_reason.get(i).and_then(|r| *r) {
                        Some(PromoteReason::AttachIncomplete) => {
                            stats.promoted_after_attach_incomplete_count += 1;
                        }
                        Some(PromoteReason::ModeCFallbackAllPeersIncomplete) => {
                            stats.mode_c_fallback_all_peers_incomplete_count += 1;
                        }
                        Some(PromoteReason::MaterializeFail) | None => {}
                    }
                    let (class, _) =
                        folder_class_and_rank(&item.locus.folder_path, &self.rank_ctx.folder_rank);
                    // Always classify for recoverable-items signal (even when ladder off).
                    let true_class = classify_folder(&item.locus.folder_path);
                    if true_class.is_recoverable_items() {
                        stats.winners_from_recoverable_items += 1;
                    }
                    let group = self.group_containing(i);
                    let (dup_count, dup_sources, dup_trunc) =
                        duplicate_source_aggregate(&self.items, &group, i);
                    let decided = self.decided_by_for(i);
                    winners.push(KeepEntry {
                        locus: item.locus.clone(),
                        message_id_norm: item.message_id_norm.clone(),
                        content_hash: item.content_hash,
                        edrm_mih_hex: item.edrm_mih_hex(),
                        integrity: item.integrity.clone(),
                        size: item.size,
                        promoted_from_failure: self.promoted_from_failure[i],
                        folder_class: Some(class.as_str().to_string()),
                        decided_by: Some(decided.to_string()),
                        duplicate_source_count: dup_count,
                        duplicate_sources: dup_sources,
                        duplicate_sources_truncated: dup_trunc,
                    });
                }
                DecisionRole::DupOf => {
                    stats.duplicates += 1;
                    match self.tier_of[i].as_deref() {
                        Some("message_id") => stats.tier1_dups += 1,
                        Some("content_hash") | Some("content_hash_strong") => stats.tier2_dups += 1,
                        _ => {}
                    }
                }
                DecisionRole::MaterializeFailed => {
                    stats.materialize_failed += 1;
                }
            }
        }

        // Stable winner order: path_key then nid.
        winners.sort_by(|a, b| {
            let ka = path_compare_key(Path::new(&a.locus.source_path));
            let kb = path_compare_key(Path::new(&b.locus.source_path));
            ka.cmp(&kb).then_with(|| a.locus.nid.cmp(&b.locus.nid))
        });

        stats.grouping = self.grouping_stats.clone();

        KeepSet {
            schema: KEEP_SET_SCHEMA.to_string(),
            policy: self.policy,
            family_policy: self.family_policy,
            created_from: self.created_from.clone(),
            identity_level: Some(self.grouping_ctx.identity.as_str().to_string()),
            dedupe_scope: Some(self.grouping_ctx.scope.as_str().to_string()),
            winners,
            stats,
        }
    }

    fn group_containing(&self, item_idx: usize) -> Vec<usize> {
        for g in &self.groups {
            if g.contains(&item_idx) {
                return g.clone();
            }
        }
        vec![item_idx]
    }

    /// `decided_by` token for item `i`.
    fn decided_by_for(&self, i: usize) -> &'static str {
        // 0083 three-way promote vocabulary takes precedence for Unique winners.
        if self.roles[i] == DecisionRole::Unique {
            if let Some(reason) = self.promote_reason.get(i).and_then(|r| *r) {
                return reason.as_str();
            }
        }
        let group = self.group_containing(i);
        if group.len() == 1 {
            return "sole_member";
        }
        let self_key = rank_key(&self.items[i], &self.rank_ctx);
        match self.roles[i] {
            DecisionRole::Unique => {
                // Closest rival = best among non-self.
                let mut best_rival: Option<RankKey> = None;
                for &j in &group {
                    if j == i {
                        continue;
                    }
                    let k = rank_key(&self.items[j], &self.rank_ctx);
                    best_rival = Some(match best_rival {
                        None => k,
                        Some(prev) => {
                            if k < prev {
                                k
                            } else {
                                prev
                            }
                        }
                    });
                }
                if let Some(rival) = best_rival {
                    decided_by_rung(&self_key, &rival, self.policy, true)
                } else {
                    "sole_member"
                }
            }
            DecisionRole::DupOf => {
                if let Some(wi) = self.winner_of[i] {
                    let w_key = rank_key(&self.items[wi], &self.rank_ctx);
                    decided_by_rung(&self_key, &w_key, self.policy, false)
                } else {
                    "path_order"
                }
            }
            DecisionRole::MaterializeFailed => {
                // Hard-fail peers are not "promoted"; keep path_order unless we
                // historically stamped promote on the failed row (legacy false).
                "path_order"
            }
        }
    }

    /// Build one decision record for item index `i` (scan-order index into `items`).
    fn decision_at(&self, i: usize) -> DecisionRecord {
        let item = &self.items[i];
        let (winner_pst, winner_folder, winner_nid) = match self.roles[i] {
            DecisionRole::DupOf => {
                if let Some(wi) = self.winner_of[i] {
                    let w = &self.items[wi];
                    (
                        Some(w.locus.source_pst.clone()),
                        Some(w.locus.folder_path.clone()),
                        Some(w.locus.nid),
                    )
                } else {
                    (None, None, None)
                }
            }
            DecisionRole::Unique | DecisionRole::MaterializeFailed => (None, None, None),
        };

        let degraded_reasons = item
            .integrity
            .degraded_reasons
            .iter()
            .map(|r| r.as_str().to_string())
            .collect();

        let (class, folder_rank) =
            folder_class_and_rank(&item.locus.folder_path, &self.rank_ctx.folder_rank);
        let source_rank = source_rank_of(item, &self.rank_ctx.source_rank_patterns);
        let (date_ft, date_src) = resolve_item_date(item);
        let decided = self.decided_by_for(i);

        let (dup_count, dup_sources_str) = if self.roles[i] == DecisionRole::Unique {
            let group = self.group_containing(i);
            let (count, names, _) = duplicate_source_aggregate(&self.items, &group, i);
            (count, names.join("|"))
        } else {
            (0, String::new())
        };

        let bb = self.bound_by.get(i).copied().unwrap_or(BoundBy::Seed);
        let eligible = self.tier2_eligible.get(i).copied().unwrap_or(true);

        DecisionRecord {
            source_path: item.locus.source_path.clone(),
            source_pst: item.locus.source_pst.clone(),
            folder_path: item.locus.folder_path.clone(),
            is_orphaned: item.locus.is_orphaned || item.integrity.is_orphaned,
            nid: item.locus.nid,
            message_id_norm: item.message_id_norm.clone(),
            content_hash_hex: item.content_hash_hex(),
            edrm_mih: item.edrm_mih_hex(),
            role: self.roles[i],
            tier: self.tier_of[i].clone(),
            winner_source_pst: winner_pst,
            winner_folder,
            winner_nid,
            policy: self.policy,
            family_policy: self.family_policy,
            degraded: item.integrity.degraded,
            degraded_reasons,
            size: item.size,
            promoted_from_failure: self.promoted_from_failure[i],
            folder_class: class.as_str().to_string(),
            folder_class_rank: folder_rank,
            source_rank,
            has_bcc: item.has_bcc,
            date_filetime_utc: format_date_filetime_utc(date_ft),
            date_source: date_src.as_str().to_string(),
            decided_by: decided.to_string(),
            duplicate_source_count: dup_count,
            duplicate_sources: dup_sources_str,
            bound_by: bb.as_str().to_string(),
            identity_version: self.grouping_ctx.identity.identity_version().to_string(),
            tier2_eligible: eligible,
        }
    }

    /// Visit each decision in scan order, constructing one record at a time (O(1) row buffer).
    ///
    /// Prefer this (or [`Self::write_decisions_csv`]) over [`Self::to_decisions`] on the
    /// production CLI path so Phase 3 never materializes an all-rows `Vec`.
    pub fn for_each_decision<F>(&self, mut f: F) -> Result<(), KeepSetError>
    where
        F: FnMut(DecisionRecord) -> Result<(), KeepSetError>,
    {
        for i in 0..self.items.len() {
            f(self.decision_at(i))?;
        }
        Ok(())
    }

    /// Stream decision CSV rows without buffering all records (O(1) row buffer).
    pub fn write_decisions_csv(&self, wtr: &mut DecisionCsvWriter) -> Result<(), KeepSetError> {
        self.for_each_decision(|row| wtr.write_record(&row))
    }

    /// Build decision records for all recoverable items (scan order).
    ///
    /// Allocates a full `Vec` — fine for unit tests / small in-memory summaries.
    /// CLI Phase 3 should use [`Self::write_decisions_csv`] / [`Self::for_each_decision`].
    pub fn to_decisions(&self) -> Vec<DecisionRecord> {
        let mut out = Vec::with_capacity(self.items.len());
        let _ = self.for_each_decision(|row| {
            out.push(row);
            Ok(())
        });
        out
    }
}

/// Resolve provisional winners: fidelity → evidence rungs → policy → path/nid.
///
/// Prefer [`resolve_groups_with_ctx`]. This wrapper builds a default
/// [`RankContext`] from policy + prefer_path (pre-0075 behavior).
pub fn resolve_groups(
    items: Vec<RecoverableScanItem>,
    policy: KeepPolicy,
    family_policy: FamilyPolicy,
    prefer_path: &[String],
    tier2_enabled: bool,
    created_from: Option<KeepSetProvenance>,
) -> ResolvedKeepSet {
    let ctx = RankContext::from_policy_and_prefer(policy, prefer_path);
    resolve_groups_with_ctx(items, family_policy, &ctx, tier2_enabled, created_from)
}

/// Resolve provisional winners with a full [`RankContext`] (0075).
///
/// Uses default 0076 [`GroupingContext`] with `tier2_enabled`.
pub fn resolve_groups_with_ctx(
    items: Vec<RecoverableScanItem>,
    family_policy: FamilyPolicy,
    rank_ctx: &RankContext,
    tier2_enabled: bool,
    created_from: Option<KeepSetProvenance>,
) -> ResolvedKeepSet {
    resolve_groups_with_grouping(
        items,
        family_policy,
        rank_ctx,
        &GroupingContext::with_tier2(tier2_enabled),
        created_from,
    )
}

/// Resolve with full ranking + grouping contexts (0076).
pub fn resolve_groups_with_grouping(
    items: Vec<RecoverableScanItem>,
    family_policy: FamilyPolicy,
    rank_ctx: &RankContext,
    grouping_ctx: &GroupingContext,
    created_from: Option<KeepSetProvenance>,
) -> ResolvedKeepSet {
    let outcome = group_candidates_with_stats(&items, grouping_ctx);
    let groups = outcome.groups;
    let bound_by = outcome.bound_by;
    let tier2_eligible = outcome.tier2_eligible;
    let grouping_stats = outcome.stats;
    let n = items.len();
    let mut roles = vec![DecisionRole::Unique; n];
    let mut winner_of: Vec<Option<usize>> = vec![None; n];
    let mut tier_of: Vec<Option<String>> = vec![None; n];
    let promoted_from_failure = vec![false; n];
    let promote_reason = vec![None; n];
    let group_dropped = vec![false; groups.len()];
    let mut provisional_winners = Vec::with_capacity(groups.len());

    for group in &groups {
        if group.is_empty() {
            provisional_winners.push(None);
            continue;
        }
        // Rank members; lowest key wins.
        let mut ranked = group.clone();
        ranked.sort_by(|&a, &b| rank_key(&items[a], rank_ctx).cmp(&rank_key(&items[b], rank_ctx)));
        let winner = ranked[0];
        provisional_winners.push(Some(winner));

        for &idx in group {
            if idx == winner {
                roles[idx] = DecisionRole::Unique;
                winner_of[idx] = Some(winner);
                tier_of[idx] = None;
            } else {
                roles[idx] = DecisionRole::DupOf;
                winner_of[idx] = Some(winner);
                // Bound_by recorded at group time — not reconstructed.
                tier_of[idx] = bound_by
                    .get(idx)
                    .and_then(|b| b.tier_csv())
                    .map(|s| s.to_string());
            }
        }
    }

    ResolvedKeepSet {
        policy: rank_ctx.policy,
        family_policy,
        prefer_path: rank_ctx.prefer_path.clone(),
        rank_ctx: rank_ctx.clone(),
        tier2_enabled: grouping_ctx.tier2_enabled,
        grouping_ctx: grouping_ctx.clone(),
        items,
        groups,
        provisional_winners,
        roles,
        winner_of,
        tier_of,
        bound_by,
        tier2_eligible,
        grouping_stats,
        promoted_from_failure,
        promote_reason,
        soft_skip_attach_records: Vec::new(),
        group_dropped,
        created_from,
    }
}

/// Pure keep-set build without materialize (provisional winners).
pub fn build_keep_set(
    recoverable: impl IntoIterator<Item = RecoverableScanItem>,
    policy: KeepPolicy,
    family_policy: FamilyPolicy,
    prefer_path: &[String],
    tier2_enabled: bool,
) -> Result<(KeepSet, Vec<DecisionRecord>), KeepSetError> {
    let items: Vec<_> = recoverable.into_iter().collect();
    let resolved = resolve_groups(
        items,
        policy,
        family_policy,
        prefer_path,
        tier2_enabled,
        None,
    );
    Ok((resolved.to_keep_set(), resolved.to_decisions()))
}

/// Pure keep-set build with full [`RankContext`].
pub fn build_keep_set_with_ctx(
    recoverable: impl IntoIterator<Item = RecoverableScanItem>,
    family_policy: FamilyPolicy,
    rank_ctx: &RankContext,
    tier2_enabled: bool,
) -> Result<(KeepSet, Vec<DecisionRecord>), KeepSetError> {
    let items: Vec<_> = recoverable.into_iter().collect();
    let resolved = resolve_groups_with_ctx(items, family_policy, rank_ctx, tier2_enabled, None);
    Ok((resolved.to_keep_set(), resolved.to_decisions()))
}

/// Options for [`build_keep_set_materialized`].
pub struct MaterializeBuildOpts<'a> {
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path: &'a [String],
    pub tier2_enabled: bool,
    pub created_from: Option<KeepSetProvenance>,
    /// Optional full rank context; when set, overrides policy/prefer_path for ranking.
    pub rank_ctx: Option<&'a RankContext>,
    /// Optional grouping context (0076). When set, overrides `tier2_enabled`.
    pub grouping_ctx: Option<&'a GroupingContext>,
    /// Mode A pre-write promote-on-attach-fail (0083). Default false via tests that set it.
    pub promote_on_attach_fail: bool,
}

/// Build keep-set then finalize winners via materialize + promotion.
///
/// Winner bodies are delivered one-at-a-time via `on_winner` (O(1) body memory).
/// Pass `|_| Ok(())` when only decision/keep-set roles are needed.
pub fn build_keep_set_materialized<F>(
    recoverable: impl IntoIterator<Item = RecoverableScanItem>,
    opts: MaterializeBuildOpts<'_>,
    materializer: &mut dyn MessageMaterializer,
    mut on_winner: F,
) -> Result<(KeepSet, Vec<DecisionRecord>, u64), KeepSetError>
where
    F: FnMut(CanonicalMessage) -> Result<(), KeepSetError>,
{
    let items: Vec<_> = recoverable.into_iter().collect();
    let owned_ctx;
    let ctx_ref = if let Some(c) = opts.rank_ctx {
        c
    } else {
        owned_ctx = RankContext::from_policy_and_prefer(opts.policy, opts.prefer_path);
        &owned_ctx
    };
    let owned_gctx;
    let gctx_ref = if let Some(g) = opts.grouping_ctx {
        g
    } else {
        owned_gctx = GroupingContext::with_tier2(opts.tier2_enabled);
        &owned_gctx
    };
    let mut resolved = resolve_groups_with_grouping(
        items,
        opts.family_policy,
        ctx_ref,
        gctx_ref,
        opts.created_from,
    );
    let fin_opts = MaterializeFinalizeOpts {
        promote_on_attach_fail: opts.promote_on_attach_fail,
    };
    let count =
        finalize_with_materialize_opts(&mut resolved, materializer, &fin_opts, &mut on_winner)?;
    Ok((resolved.to_keep_set(), resolved.to_decisions(), count))
}

/// Merge materialize-time soft fidelity into the scan item (export honesty).
fn merge_soft_fidelity(item: &mut RecoverableScanItem, msg: &CanonicalMessage) {
    let mut reasons = item.integrity.degraded_reasons.clone();
    if msg.body_unavailable
        && !reasons.contains(&crate::integrity::IntegrityReason::BodyUnavailable)
    {
        reasons.push(crate::integrity::IntegrityReason::BodyUnavailable);
    }
    if msg.body_incomplete && !reasons.contains(&crate::integrity::IntegrityReason::BodyTruncated) {
        reasons.push(crate::integrity::IntegrityReason::BodyTruncated);
    }
    // Also absorb any reasons already on the message fidelity.
    for r in &msg.fidelity.degraded_reasons {
        if !reasons.contains(r) {
            reasons.push(*r);
        }
    }
    if !reasons.is_empty() || msg.fidelity.is_orphaned || item.integrity.is_orphaned {
        item.integrity = RecoverableIntegrity::with_degraded(
            reasons,
            item.integrity.is_orphaned || msg.fidelity.is_orphaned,
        );
    }
}

/// Materialize provisional winners; on hard fail promote next peer (§3.7.1).
///
/// Thin wrapper: Mode A flag **off** (default). Prefer
/// [`finalize_with_materialize_opts`] when threading `--promote-on-attach-fail`.
///
/// Bodies are delivered **one-at-a-time** through `on_winner` and never retained
/// as an all-winners `Vec` (O(1) body memory). Soft fidelity flags are written
/// back onto `resolved.items` so Phase 3 decision/keep rows stay honest.
///
/// Returns the count of successfully materialized winners.
pub fn finalize_with_materialize<F>(
    resolved: &mut ResolvedKeepSet,
    materializer: &mut dyn MessageMaterializer,
    on_winner: &mut F,
) -> Result<u64, KeepSetError>
where
    F: FnMut(CanonicalMessage) -> Result<(), KeepSetError>,
{
    finalize_with_materialize_opts(
        resolved,
        materializer,
        &MaterializeFinalizeOpts::default(),
        on_winner,
    )
}

/// Materialize provisional winners with optional Mode A attach-incomplete promote (0083).
///
/// Peer order follows existing `rank_key` only — **no** least-incomplete re-rank.
/// Mode B write-time mid-message promote is **not** supported.
///
/// Soft-skipped incomplete peers remain [`DecisionRole::DupOf`] (not
/// [`DecisionRole::MaterializeFailed`]). Soft-skip attach loci are recorded in
/// [`ResolvedKeepSet::soft_skip_attach_records`] for ledger honesty.
pub fn finalize_with_materialize_opts<F>(
    resolved: &mut ResolvedKeepSet,
    materializer: &mut dyn MessageMaterializer,
    opts: &MaterializeFinalizeOpts,
    on_winner: &mut F,
) -> Result<u64, KeepSetError>
where
    F: FnMut(CanonicalMessage) -> Result<(), KeepSetError>,
{
    let mut materialized_count = 0u64;
    let rank_ctx = resolved.rank_ctx.clone();
    // Clear any prior soft-skip records (re-finalize safety).
    resolved.soft_skip_attach_records.clear();

    for (g_idx, group) in resolved.groups.clone().into_iter().enumerate() {
        if group.is_empty() {
            continue;
        }

        // Rank full group once (same ladder as hard promote / 0075).
        let mut ranked = group.clone();
        ranked.sort_by(|&a, &b| {
            rank_key(&resolved.items[a], &rank_ctx).cmp(&rank_key(&resolved.items[b], &rank_ctx))
        });

        let mut final_winner: Option<usize> = None;
        let mut failed: Vec<usize> = Vec::new();
        // Incomplete peers soft-skipped while hunting for a complete peer (item idx + msg).
        let mut soft_skipped_msgs: Vec<(usize, CanonicalMessage)> = Vec::new();
        let mut had_hard_fail = false;
        let mut had_soft_skip = false;
        let mut mode_c_fallback = false;
        let mut accepted_msg: Option<CanonicalMessage> = None;
        let mut accepted_attempt: usize = 0;

        for (attempt, &idx) in ranked.iter().enumerate() {
            let locus = resolved.items[idx].locus.clone();
            match materializer.materialize(&locus) {
                Ok(mut msg) => {
                    // Soft fidelity from materialize → item integrity (export honesty).
                    merge_soft_fidelity(&mut resolved.items[idx], &msg);

                    // Message fidelity mirrors final item integrity.
                    msg.fidelity = resolved.items[idx].integrity.clone();

                    // Carry keys from scan item.
                    msg.message_id_norm = resolved.items[idx].message_id_norm.clone();
                    msg.content_hash = resolved.items[idx].content_hash;
                    msg.edrm_mih_hex = resolved.items[idx].edrm_mih_hex();

                    let incomplete = opts.promote_on_attach_fail && is_attach_incomplete(&msg);
                    let more_peers = attempt + 1 < ranked.len();

                    if incomplete && more_peers {
                        // Soft skip: valid message, incomplete attaches — try next peer.
                        // Stash so Mode C fallback can accept the highest-ranked
                        // materializable incomplete without re-materialize.
                        had_soft_skip = true;
                        soft_skipped_msgs.push((idx, msg));
                        continue;
                    }

                    if incomplete && opts.promote_on_attach_fail {
                        // No more peers: Mode C fallback — highest-ranked materializable
                        // (first soft-skipped incomplete, else this sole incomplete).
                        mode_c_fallback = true;
                        if soft_skipped_msgs.is_empty() {
                            accepted_attempt = attempt;
                            final_winner = Some(idx);
                            accepted_msg = Some(msg);
                        } else {
                            let (first_idx, first_msg) = soft_skipped_msgs.remove(0);
                            // Remaining soft-skips + current last incomplete → DupOf winner.
                            soft_skipped_msgs.push((idx, msg));
                            // Walked past first incomplete looking for complete → promoted.
                            accepted_attempt = attempt.max(1);
                            final_winner = Some(first_idx);
                            accepted_msg = Some(first_msg);
                        }
                        break;
                    }

                    // Accept complete (or flag-off any Ok).
                    accepted_attempt = attempt;
                    final_winner = Some(idx);
                    accepted_msg = Some(msg);
                    break;
                }
                Err(MaterializeError::Hard(_)) => {
                    failed.push(idx);
                    had_hard_fail = true;
                }
            }
        }

        // Mode A / Mode C: incomplete peer(s) soft-skipped, but every remaining
        // peer hard-failed (no complete Ok). Accept highest-ranked materializable
        // incomplete — same as end-of-list incomplete accept path (DoD-5 / §2.5 r7).
        if final_winner.is_none() && !soft_skipped_msgs.is_empty() {
            mode_c_fallback = true;
            let (first_idx, first_msg) = soft_skipped_msgs.remove(0);
            // Walked past first incomplete looking for complete → promoted.
            accepted_attempt = 1;
            final_winner = Some(first_idx);
            accepted_msg = Some(first_msg);
        }

        if let Some(winner) = final_winner {
            let Some(msg) = accepted_msg.take() else {
                return Err(KeepSetError::Other(
                    "internal: accepted winner without message".into(),
                ));
            };

            // Decide promote vocabulary (0083).
            let promoted = accepted_attempt > 0 || had_hard_fail || had_soft_skip;
            let promote_reason = if mode_c_fallback {
                Some(PromoteReason::ModeCFallbackAllPeersIncomplete)
            } else if promoted {
                if had_soft_skip {
                    // Soft-attach Mode A deliverable wins over mixed hard-fail history.
                    Some(PromoteReason::AttachIncomplete)
                } else if had_hard_fail {
                    Some(PromoteReason::MaterializeFail)
                } else {
                    // attempt > 0 without hard/soft should not happen; keep hard string.
                    Some(PromoteReason::MaterializeFail)
                }
            } else {
                None
            };

            // Soft-skip ledger records with peer locus of accepted winner.
            // Exclude the winner itself if it was pulled from soft_skipped_msgs.
            let peer_path = resolved.items[winner].locus.source_path.clone();
            let peer_nid = resolved.items[winner].locus.nid;
            for (skip_idx, skip_msg) in soft_skipped_msgs.drain(..) {
                if skip_idx == winner {
                    continue;
                }
                resolved
                    .soft_skip_attach_records
                    .extend(soft_skip_records_for_msg(&skip_msg, &peer_path, peer_nid));
            }

            on_winner(msg)?;
            materialized_count += 1;

            resolved.group_dropped[g_idx] = false;
            for &idx in &group {
                if failed.contains(&idx) {
                    resolved.roles[idx] = DecisionRole::MaterializeFailed;
                    resolved.winner_of[idx] = Some(winner);
                    resolved.tier_of[idx] = None;
                    resolved.promoted_from_failure[idx] = false;
                    resolved.promote_reason[idx] = None;
                } else if idx == winner {
                    resolved.roles[idx] = DecisionRole::Unique;
                    resolved.winner_of[idx] = Some(winner);
                    resolved.tier_of[idx] = None;
                    resolved.promoted_from_failure[idx] = promoted;
                    resolved.promote_reason[idx] = promote_reason;
                } else {
                    // Soft-skipped incomplete + normal dups: DupOf (not MaterializeFailed).
                    resolved.roles[idx] = DecisionRole::DupOf;
                    resolved.winner_of[idx] = Some(winner);
                    resolved.tier_of[idx] = resolved
                        .bound_by
                        .get(idx)
                        .and_then(|b| b.tier_csv())
                        .map(|s| s.to_string());
                    resolved.promoted_from_failure[idx] = false;
                    resolved.promote_reason[idx] = None;
                }
            }
        } else {
            // All hard-failed (no soft-skipped materializable) — zero exportable winners.
            debug_assert!(
                soft_skipped_msgs.is_empty(),
                "soft-skipped materializable must Mode C fallback, not drop"
            );
            let _ = soft_skipped_msgs;
            resolved.group_dropped[g_idx] = true;
            for &idx in &group {
                resolved.roles[idx] = DecisionRole::MaterializeFailed;
                resolved.winner_of[idx] = None;
                resolved.tier_of[idx] = None;
                resolved.promoted_from_failure[idx] = false;
                resolved.promote_reason[idx] = None;
            }
        }
    }

    Ok(materialized_count)
}

// ─── Decision CSV + KeepSet JSON ────────────────────────────────────────────

/// Pre-0075 decision CSV header (19 columns). New columns append only.
pub const DECISION_CSV_HEADER_V1: [&str; 19] = [
    "SourcePath",
    "SourcePst",
    "Folder",
    "IsOrphaned",
    "NID",
    "MessageIdNorm",
    "ContentHash",
    "EdrmMih",
    "Role",
    "Tier",
    "WinnerPst",
    "WinnerFolder",
    "WinnerNid",
    "Policy",
    "FamilyPolicy",
    "Degraded",
    "DegradedReasons",
    "Size",
    "PromotedFromFailure",
];

/// Full decision CSV header (pre-0075 + 0075 + 0076 append columns).
pub const DECISION_CSV_HEADER: [&str; 31] = [
    "SourcePath",
    "SourcePst",
    "Folder",
    "IsOrphaned",
    "NID",
    "MessageIdNorm",
    "ContentHash",
    "EdrmMih",
    "Role",
    "Tier",
    "WinnerPst",
    "WinnerFolder",
    "WinnerNid",
    "Policy",
    "FamilyPolicy",
    "Degraded",
    "DegradedReasons",
    "Size",
    "PromotedFromFailure",
    // 0075 append-only
    "folder_class",
    "folder_class_rank",
    "source_rank",
    "has_bcc",
    "date_filetime_utc",
    "date_source",
    "decided_by",
    "duplicate_source_count",
    "duplicate_sources",
    // 0076 append-only
    "bound_by",
    "identity_version",
    "tier2_eligible",
];

/// Streaming decision CSV writer (Phase 3 only — after resolve).
pub struct DecisionCsvWriter {
    wtr: csv::Writer<BufWriter<File>>,
    path: PathBuf,
    rows_written: u64,
}

impl DecisionCsvWriter {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, KeepSetError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(&path)?;
        let mut wtr = csv::Writer::from_writer(BufWriter::new(file));
        wtr.write_record(DECISION_CSV_HEADER)
            .map_err(|e| KeepSetError::Csv(e.to_string()))?;
        wtr.flush().map_err(|e| KeepSetError::Csv(e.to_string()))?;
        Ok(Self {
            wtr,
            path,
            rows_written: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rows_written(&self) -> u64 {
        self.rows_written
    }

    pub fn write_record(&mut self, row: &DecisionRecord) -> Result<(), KeepSetError> {
        let nid = row.nid.to_string();
        let size = row.size.to_string();
        let winner_nid = row.winner_nid.map(|n| n.to_string()).unwrap_or_default();
        let reasons = row.degraded_reasons.join(";");
        let folder_rank = row.folder_class_rank.to_string();
        let source_rank = row.source_rank.to_string();
        let dup_count = if row.role == DecisionRole::Unique {
            row.duplicate_source_count.to_string()
        } else {
            String::new()
        };
        // Free-text folder path / sources go through csv crate quoting; formula
        // neutralization is applied to user-influenced text fields (0073).
        let folder_class = neutralize_csv_formula(&row.folder_class);
        let decided_by = neutralize_csv_formula(&row.decided_by);
        let dup_sources = if row.role == DecisionRole::Unique {
            neutralize_csv_formula(&row.duplicate_sources)
        } else {
            String::new()
        };
        let date_utc = neutralize_csv_formula(&row.date_filetime_utc);
        let bound_by = neutralize_csv_formula(&row.bound_by);
        let identity_version = neutralize_csv_formula(&row.identity_version);
        let tier2_eligible = if row.tier2_eligible { "true" } else { "false" };
        self.wtr
            .write_record([
                row.source_path.as_str(),
                row.source_pst.as_str(),
                row.folder_path.as_str(),
                if row.is_orphaned { "true" } else { "false" },
                nid.as_str(),
                row.message_id_norm.as_deref().unwrap_or(""),
                row.content_hash_hex.as_str(),
                row.edrm_mih.as_deref().unwrap_or(""),
                row.role.as_str(),
                row.tier.as_deref().unwrap_or(""),
                row.winner_source_pst.as_deref().unwrap_or(""),
                row.winner_folder.as_deref().unwrap_or(""),
                winner_nid.as_str(),
                row.policy.as_str(),
                row.family_policy.as_str(),
                if row.degraded { "true" } else { "false" },
                reasons.as_str(),
                size.as_str(),
                if row.promoted_from_failure {
                    "true"
                } else {
                    "false"
                },
                folder_class.as_str(),
                folder_rank.as_str(),
                source_rank.as_str(),
                if row.has_bcc { "true" } else { "false" },
                date_utc.as_str(),
                row.date_source.as_str(),
                decided_by.as_str(),
                dup_count.as_str(),
                dup_sources.as_str(),
                // 0076 append-only
                bound_by.as_str(),
                identity_version.as_str(),
                tier2_eligible,
            ])
            .map_err(|e| KeepSetError::Csv(e.to_string()))?;
        self.rows_written += 1;
        self.wtr
            .flush()
            .map_err(|e| KeepSetError::Csv(e.to_string()))?;
        Ok(())
    }

    pub fn write_all(&mut self, rows: &[DecisionRecord]) -> Result<(), KeepSetError> {
        for row in rows {
            self.write_record(row)?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), KeepSetError> {
        self.wtr
            .flush()
            .map_err(|e| KeepSetError::Csv(e.to_string()))
    }
}

/// Write keep-set JSON (winners + stats; no bodies).
pub fn write_keep_set_json(path: impl AsRef<Path>, keep_set: &KeepSet) -> Result<(), KeepSetError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = File::create(path)?;
    let mut wtr = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut wtr, keep_set)
        .map_err(|e| KeepSetError::Json(e.to_string()))?;
    wtr.write_all(b"\n")?;
    wtr.flush()?;
    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Prefix `'` when cell (leading whitespace stripped) starts with `=+\-@` (0073).
fn neutralize_csv_formula(s: &str) -> String {
    let check = s.trim_start();
    if check.starts_with('=')
        || check.starts_with('+')
        || check.starts_with('-')
        || check.starts_with('@')
    {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// Serde helper for [u8; 32] as hex string.
mod serde_content_hash {
    use super::hex_encode;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(hash: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex_encode(hash))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let mut out = [0u8; 32];
        if s.len() != 64 {
            return Err(serde::de::Error::custom(
                "content_hash hex must be 64 chars",
            ));
        }
        for i in 0..32 {
            let byte =
                u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(serde::de::Error::custom)?;
            out[i] = byte;
        }
        Ok(out)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::{IntegrityReason, SCAN_INTEGRITY_SCHEMA};

    fn locus(path: &str, pst: &str, folder: &str, nid: u64) -> MessageLocus {
        MessageLocus {
            source_path: path.into(),
            source_pst: pst.into(),
            folder_path: folder.into(),
            nid,
            is_orphaned: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn item(
        path: &str,
        pst: &str,
        folder: &str,
        nid: u64,
        mid: Option<&str>,
        hash: [u8; 32],
        size: u32,
        scan_order: u64,
        degraded: bool,
    ) -> RecoverableScanItem {
        let integrity = if degraded {
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::BodyTruncated], false)
        } else {
            RecoverableIntegrity::clean()
        };
        RecoverableScanItem {
            locus: locus(path, pst, folder, nid),
            message_id_norm: mid.map(|s| s.to_string()),
            content_hash: hash,
            size,
            integrity,
            scan_order,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
            // Eligible Tier-2 preimage for unit tests (two weak fields + body flag).
            has_body_preview: !degraded,
            subject_nonempty: true,
            sender_nonempty: true,
            attach_count: 0,
            body_sha256: None,
            body_char_len: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            strong_content_hash: None,
            fp_header: 0,
            fp_body: 0,
            fp_recipients: 0,
            fp_attachments: 0,
            preview_bytes_over_budget: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn item_dated(
        path: &str,
        pst: &str,
        folder: &str,
        nid: u64,
        mid: Option<&str>,
        hash: [u8; 32],
        size: u32,
        scan_order: u64,
        degraded: bool,
        submit_time: Option<i64>,
        delivery_time: Option<i64>,
        has_bcc: bool,
    ) -> RecoverableScanItem {
        let mut it = item(
            path, pst, folder, nid, mid, hash, size, scan_order, degraded,
        );
        it.submit_time = submit_time;
        it.delivery_time = delivery_time;
        it.has_bcc = has_bcc;
        it
    }

    #[test]
    fn edrm_mih_fixed_vector() {
        // EDRM MIH = MD5(UTF-8 bytes of normalized MID), lowercase hex.
        // Frozen vector for "abc123@example.com" (no angle brackets).
        let mid = "abc123@example.com";
        let got = edrm_mih_hex(mid);
        assert_eq!(got.len(), 32);
        assert!(got.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        assert_eq!(got, edrm_mih_hex(mid), "deterministic");
        // Locked interop vector (must not change with formula/deps).
        assert_eq!(got, "ac623c094f3922f9fd85936e0003043a");
    }

    #[test]
    fn two_same_mid_tier1() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "Inbox",
            1,
            Some("mid@x"),
            [1; 32],
            100,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "Inbox",
            2,
            Some("mid@x"),
            [2; 32],
            100,
            1,
            false,
        );
        let (ks, dec) = build_keep_set(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.schema, KEEP_SET_SCHEMA);
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(ks.stats.duplicates, 1);
        assert_eq!(ks.stats.tier1_dups, 1);
        assert_eq!(dec.len(), 2);
        let uniq: Vec<_> = dec
            .iter()
            .filter(|d| d.role == DecisionRole::Unique)
            .collect();
        let dups: Vec<_> = dec
            .iter()
            .filter(|d| d.role == DecisionRole::DupOf)
            .collect();
        assert_eq!(uniq.len(), 1);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].tier.as_deref(), Some("message_id"));
        assert_eq!(dups[0].winner_nid, Some(uniq[0].nid));
    }

    #[test]
    fn same_content_no_mid_tier2() {
        let h = [42u8; 32];
        let a = item("C:/a.pst", "a.pst", "Inbox", 1, None, h, 50, 0, false);
        let b = item("C:/b.pst", "b.pst", "Inbox", 2, None, h, 50, 1, false);
        let (ks, dec) = build_keep_set(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(ks.stats.tier2_dups, 1);
        let dup = dec
            .iter()
            .find(|d| d.role == DecisionRole::DupOf)
            .expect("dup");
        assert_eq!(dup.tier.as_deref(), Some("content_hash"));
    }

    #[test]
    fn keep_largest_wins() {
        let mid = Some("big@x");
        let a = item("C:/a.pst", "a.pst", "Inbox", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "Inbox", 2, mid, [1; 32], 999, 1, false);
        let (ks, dec) = build_keep_set(
            vec![a, b],
            KeepPolicy::KeepLargest,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.winners.len(), 1);
        assert_eq!(ks.winners[0].size, 999);
        let uniq = dec
            .iter()
            .find(|d| d.role == DecisionRole::Unique)
            .expect("u");
        assert_eq!(uniq.nid, 2);
    }

    #[test]
    fn prefer_path_primary_wins() {
        let mid = Some("p@x");
        let a = item(
            "C:/Archive/a.pst",
            "a.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let b = item(
            "C:/Primary/b.pst",
            "b.pst",
            "Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        let prefer = vec!["Primary".to_string()];
        let (ks, _) = build_keep_set(
            vec![a, b],
            KeepPolicy::PreferPath,
            FamilyPolicy::default(),
            &prefer,
            true,
        )
        .expect("build");
        assert_eq!(ks.winners[0].locus.source_pst, "b.pst");
        assert!(ks.winners[0].locus.source_path.contains("Primary"));
    }

    #[test]
    fn clean_beats_degraded_first_seen() {
        let mid = Some("c@x");
        // Degraded first in scan order.
        let a = item("C:/a.pst", "a.pst", "Inbox", 1, mid, [1; 32], 100, 0, true);
        let b = item("C:/b.pst", "b.pst", "Inbox", 2, mid, [1; 32], 100, 1, false);
        let (ks, dec) = build_keep_set(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.winners.len(), 1);
        assert!(!ks.winners[0].integrity.degraded);
        assert_eq!(ks.winners[0].locus.nid, 2);
        let deg = dec.iter().find(|d| d.nid == 1).expect("degraded");
        assert_eq!(deg.role, DecisionRole::DupOf);
    }

    #[test]
    fn path_order_swap_same_winners() {
        let mid = Some("d@x");
        let a = item(
            "C:/z.pst", "z.pst", "Inbox", 10, mid, [1; 32], 100, 0, false,
        );
        let b = item(
            "C:/a.pst", "a.pst", "Inbox", 20, mid, [1; 32], 100, 1, false,
        );
        // First-seen with scan_order reflecting path-sorted order (a before z).
        // If we swap presentation but keep scan_order consistent with path sort:
        let a2 = item(
            "C:/a.pst", "a.pst", "Inbox", 20, mid, [1; 32], 100, 0, false,
        );
        let b2 = item(
            "C:/z.pst", "z.pst", "Inbox", 10, mid, [1; 32], 100, 1, false,
        );

        let (ks1, _) = build_keep_set(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("k1");
        // Correct deterministic: scan_order must match sorted paths.
        let (ks2, _) = build_keep_set(
            vec![a2, b2],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("k2");
        // With scan_order 0 = a.pst in both correct runs, winner is a.pst nid 20.
        // First call used wrong scan_order (z first) — document that scan_order must
        // be assigned after path sort. Correct pairs:
        let (ks_correct_a, _) = build_keep_set(
            vec![
                item(
                    "C:/a.pst", "a.pst", "Inbox", 20, mid, [1; 32], 100, 0, false,
                ),
                item(
                    "C:/z.pst", "z.pst", "Inbox", 10, mid, [1; 32], 100, 1, false,
                ),
            ],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("c1");
        let (ks_correct_b, _) = build_keep_set(
            vec![
                item(
                    "C:/z.pst", "z.pst", "Inbox", 10, mid, [1; 32], 100, 1, false,
                ),
                item(
                    "C:/a.pst", "a.pst", "Inbox", 20, mid, [1; 32], 100, 0, false,
                ),
            ],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("c2");
        assert_eq!(
            ks_correct_a.winners[0].locus.nid,
            ks_correct_b.winners[0].locus.nid
        );
        assert_eq!(ks_correct_a.winners[0].locus.source_pst, "a.pst");
        // Also sort_input_paths is stable.
        let mut p1 = vec![PathBuf::from("C:/z.pst"), PathBuf::from("C:/a.pst")];
        let mut p2 = vec![PathBuf::from("C:/a.pst"), PathBuf::from("C:/z.pst")];
        sort_input_paths(&mut p1);
        sort_input_paths(&mut p2);
        assert_eq!(p1, p2);
        // silence unused
        let _ = (ks1, ks2);
    }

    #[test]
    fn decision_n_rows_for_n_recoverable() {
        let items = vec![
            item(
                "C:/a.pst",
                "a.pst",
                "I",
                1,
                Some("m1"),
                [1; 32],
                10,
                0,
                false,
            ),
            item(
                "C:/a.pst",
                "a.pst",
                "I",
                2,
                Some("m2"),
                [2; 32],
                10,
                1,
                false,
            ),
            item(
                "C:/a.pst",
                "a.pst",
                "I",
                3,
                Some("m1"),
                [1; 32],
                10,
                2,
                false,
            ),
        ];
        let (_, dec) = build_keep_set(
            items,
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b");
        assert_eq!(dec.len(), 3);
    }

    #[test]
    fn keep_set_json_schema() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("j@x"),
            [9; 32],
            1,
            0,
            false,
        );
        let (ks, _) = build_keep_set(
            vec![a],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b");
        let v = serde_json::to_value(&ks).expect("json");
        assert_eq!(v["schema"], KEEP_SET_SCHEMA);
        assert_eq!(v["policy"], "first_seen");
        assert!(v["winners"].as_array().expect("w").len() == 1);
    }

    /// 0094: nested DTO is serde(skip) so keep-set JSON / hash preimage stay stable.
    #[test]
    fn canonical_attachment_serde_skips_embedded_message() {
        let att = CanonicalAttachment {
            filename: "e.msg".into(),
            size: 1,
            mime: None,
            data: None,
            stream_available: false,
            attach_nid: Some(1),
            attach_method: Some(5),
            is_cloud_link: false,
            cloud_provider: None,
            cloud_url: None,
            cloud_permission_type: None,
            embedded_message: Some(Box::new(NestedCanonicalMessage {
                subject: Some("secret nest".into()),
                body_plain: Some("huge".into()),
                ..Default::default()
            })),
            embedded_extract_limit: true,
        };
        let json = serde_json::to_value(&att).expect("ser");
        assert!(json.get("embedded_message").is_none());
        assert!(json.get("embedded_extract_limit").is_none());
        let back: CanonicalAttachment = serde_json::from_value(json).expect("de");
        assert!(back.embedded_message.is_none());
        assert!(!back.embedded_extract_limit);
        assert_eq!(back.filename, "e.msg");
        assert_eq!(back.attach_method, Some(5));
    }

    /// DoD-1 regression: populating nested DTO fields must not move parent hashes.
    ///
    /// Production scan builds [`AttachmentInfo`] from filename+size only; nested
    /// extract fills `embedded_message` / `embedded_extract_limit` later and those
    /// fields are not in the v1 (or strong attach-slot) preimage.
    #[test]
    fn parent_hash_unchanged_when_embedded_message_populated() {
        use crate::hasher::{
            compute_content_hash, compute_dedup_keys_ex, AttachmentInfo, StrongHashInput,
        };

        let mk =
            |embedded: Option<Box<NestedCanonicalMessage>>, limit: bool| -> CanonicalAttachment {
                CanonicalAttachment {
                    filename: "message.msg".into(),
                    size: 1234,
                    mime: Some("message/rfc822".into()),
                    data: None,
                    stream_available: false,
                    attach_nid: Some(0x25),
                    attach_method: Some(5),
                    is_cloud_link: false,
                    cloud_provider: None,
                    cloud_url: None,
                    cloud_permission_type: None,
                    embedded_message: embedded,
                    embedded_extract_limit: limit,
                }
            };

        let extract_off = mk(None, false);
        let extract_on = mk(
            Some(Box::new(NestedCanonicalMessage {
                subject: Some("Nest".into()),
                body_plain: Some("nested body must not enter parent hash".into()),
                sender: Some("nest@ex.com".into()),
                ..Default::default()
            })),
            true,
        );
        assert!(extract_on.embedded_message.is_some());
        assert!(extract_on.embedded_extract_limit);
        assert!(extract_off.embedded_message.is_none());

        // Same mapping scan uses for Tier-2 attach contribution (filename:size).
        let to_info = |a: &CanonicalAttachment| AttachmentInfo::new(a.filename.clone(), a.size);
        let atts_off = [to_info(&extract_off)];
        let atts_on = [to_info(&extract_on)];

        let subject = Some("Parent");
        let submit = Some(0x01D5B035EDA780_i64);
        let sender = Some("alice@example.com");
        let body = Some("parent body");

        let h_off = compute_content_hash(subject, submit, sender, body, &atts_off);
        let h_on = compute_content_hash(subject, submit, sender, body, &atts_on);
        assert_eq!(
            h_off, h_on,
            "v1 content_hash must ignore embedded_message / embedded_extract_limit"
        );

        let strong = StrongHashInput {
            identity: IdentityLevel::BodyRecipAttach,
            body_sha256: None,
            body_char_len: Some(11),
            display_to: Some("bob@example.com"),
            display_cc: None,
            display_bcc: None,
            recipients: None,
            ignore_inline_attachments: false,
        };
        let k_off = compute_dedup_keys_ex(None, subject, submit, sender, body, &atts_off, &strong);
        let k_on = compute_dedup_keys_ex(None, subject, submit, sender, body, &atts_on, &strong);
        assert_eq!(k_off.content_hash, k_on.content_hash);
        assert_eq!(
            k_off.strong_content_hash, k_on.strong_content_hash,
            "strong_content_hash must ignore embedded_message / embedded_extract_limit"
        );

        // Control: hash-relevant attach size still splits when nested DTO is equal.
        let size_changed = CanonicalAttachment {
            size: 9999,
            ..extract_off.clone()
        };
        let h_size = compute_content_hash(subject, submit, sender, body, &[to_info(&size_changed)]);
        assert_ne!(
            h_off, h_size,
            "control: filename:size contribution must still affect content_hash"
        );
    }

    /// 0096 DoD-2: production scan maps attaches via `AttachmentInfo::new(filename, size)`
    /// (pst-dedup-cli `scan.rs`). Permission-only canonical differences must not
    /// change parent/strong hashes when projected that way; size control must.
    #[test]
    fn parent_hash_unchanged_when_cloud_permission_differs() {
        use crate::hasher::{
            compute_content_hash, compute_dedup_keys_ex, AttachmentInfo, StrongHashInput,
        };

        // Mirrors production scan projection (filename + size only).
        let project = |a: &CanonicalAttachment| AttachmentInfo::new(a.filename.clone(), a.size);

        let none = CanonicalAttachment {
            filename: "link.xlsx".into(),
            size: 0,
            mime: None,
            data: None,
            stream_available: false,
            attach_nid: Some(1),
            attach_method: Some(7),
            is_cloud_link: true,
            cloud_provider: Some("OneDrivePro".into()),
            cloud_url: Some("https://1drv.ms/x/s!abc".into()),
            cloud_permission_type: None,
            embedded_message: None,
            embedded_extract_limit: false,
        };
        let with_perm = CanonicalAttachment {
            cloud_permission_type: Some(1),
            ..none.clone()
        };
        assert_ne!(
            none.cloud_permission_type, with_perm.cloud_permission_type,
            "fixture must differ only in permission"
        );

        let subject = Some("Cloud");
        let submit = Some(0x01D5B035EDA780_i64);
        let sender = Some("alice@example.com");
        let body = Some("body");
        let info_none = project(&none);
        let info_perm = project(&with_perm);
        let h_none = compute_content_hash(
            subject,
            submit,
            sender,
            body,
            std::slice::from_ref(&info_none),
        );
        let h_perm = compute_content_hash(
            subject,
            submit,
            sender,
            body,
            std::slice::from_ref(&info_perm),
        );
        assert_eq!(
            h_none, h_perm,
            "permission-only canonical difference must not change content_hash via scan projection"
        );

        let strong = StrongHashInput {
            identity: IdentityLevel::BodyRecipAttach,
            body_sha256: None,
            body_char_len: Some(4),
            display_to: Some("bob@example.com"),
            display_cc: None,
            display_bcc: None,
            recipients: None,
            ignore_inline_attachments: false,
        };
        let k_none = compute_dedup_keys_ex(
            None,
            subject,
            submit,
            sender,
            body,
            std::slice::from_ref(&info_none),
            &strong,
        );
        let k_perm = compute_dedup_keys_ex(
            None,
            subject,
            submit,
            sender,
            body,
            std::slice::from_ref(&info_perm),
            &strong,
        );
        assert_eq!(k_none.content_hash, k_perm.content_hash);
        assert_eq!(
            k_none.strong_content_hash, k_perm.strong_content_hash,
            "permission-only difference must not change strong_content_hash"
        );

        let size_changed = CanonicalAttachment { size: 1, ..none };
        let h_size = compute_content_hash(
            subject,
            submit,
            sender,
            body,
            std::slice::from_ref(&project(&size_changed)),
        );
        assert_ne!(
            h_none, h_size,
            "control: attach size must affect content_hash"
        );
    }

    #[test]
    fn degraded_sole_member_may_win() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("s@x"),
            [1; 32],
            10,
            0,
            true,
        );
        let (ks, dec) = build_keep_set(
            vec![a],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b");
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(ks.stats.degraded_winners, 1);
        assert!(ks.winners[0].integrity.degraded);
        assert!(dec[0].degraded);
    }

    struct MockMaterializer {
        /// nid → Ok(with_attach_count) or Err hard
        map: HashMap<u64, Result<usize, ()>>,
        family: FamilyPolicy,
    }

    impl MessageMaterializer for MockMaterializer {
        fn materialize(
            &mut self,
            locus: &MessageLocus,
        ) -> Result<CanonicalMessage, MaterializeError> {
            match self.map.get(&locus.nid) {
                Some(Ok(n_att)) => {
                    let attachments = if self.family == FamilyPolicy::ParentsOnly {
                        Vec::new()
                    } else {
                        (0..*n_att)
                            .map(|i| CanonicalAttachment {
                                filename: format!("f{i}.bin"),
                                size: 10,
                                mime: Some("application/octet-stream".into()),
                                data: Some(vec![1, 2, 3]),
                                stream_available: true,
                                attach_nid: Some(i as u64 + 100),
                                attach_method: Some(1),
                                is_cloud_link: false,
                                cloud_provider: None,
                                cloud_url: None,
                                cloud_permission_type: None,
                                embedded_message: None,
                                embedded_extract_limit: false,
                            })
                            .collect()
                    };
                    Ok(CanonicalMessage {
                        locus: locus.clone(),
                        message_id: None,
                        subject: Some("s".into()),
                        sender: None,
                        display_to: None,
                        display_cc: None,
                        display_bcc: None,
                        recipients: Vec::new(),
                        message_flags: None,
                        submit_time: None,
                        size: Some(10),
                        message_class: None,
                        body_plain: Some("body".into()),
                        body_html: None,
                        attachments,
                        fidelity: RecoverableIntegrity::clean(),
                        message_id_norm: None,
                        content_hash: [0; 32],
                        edrm_mih_hex: None,
                        body_incomplete: false,
                        body_unavailable: false,
                    })
                }
                Some(Err(())) | None => Err(MaterializeError::Hard(format!(
                    "forced fail nid={}",
                    locus.nid
                ))),
            }
        }
    }

    #[test]
    fn family_parents_only_no_attach_payloads() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("f@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let mut mat = MockMaterializer {
            map: HashMap::from([(1, Ok(2))]),
            family: FamilyPolicy::ParentsOnly,
        };
        let mut last: Option<CanonicalMessage> = None;
        let (_ks, _dec, count) = build_keep_set_materialized(
            vec![a],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::ParentsOnly,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |msg| {
                last = Some(msg);
                Ok(())
            },
        )
        .expect("m");
        assert_eq!(count, 1);
        assert!(last.expect("msg").attachments.is_empty());
    }

    #[test]
    fn family_keep_attaches_nonempty() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("f2@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let mut mat = MockMaterializer {
            map: HashMap::from([(1, Ok(2))]),
            family: FamilyPolicy::KeepAttachmentsWithParent,
        };
        let mut last: Option<CanonicalMessage> = None;
        let (_ks, _dec, count) = build_keep_set_materialized(
            vec![a],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |msg| {
                last = Some(msg);
                Ok(())
            },
        )
        .expect("m");
        assert_eq!(count, 1);
        let msg = last.expect("msg");
        assert_eq!(msg.attachments.len(), 2);
        assert!(msg.attachments[0].data.is_some());
    }

    #[test]
    fn materialize_fail_promotes_peer() {
        let mid = Some("promo@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = MockMaterializer {
            map: HashMap::from([(1, Err(())), (2, Ok(0))]),
            family: FamilyPolicy::default(),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::default(),
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert!(ks.winners[0].promoted_from_failure);
        assert_eq!(count, 1);
        let failed = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(failed.role, DecisionRole::MaterializeFailed);
        let uniq = dec.iter().find(|d| d.nid == 2).expect("b");
        assert_eq!(uniq.role, DecisionRole::Unique);
        assert!(uniq.promoted_from_failure);
        assert_eq!(uniq.decided_by, "promoted_after_materialize_fail");
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("promoted_after_materialize_fail")
        );
    }

    /// Helper: complete attach or incomplete (stream_available=false) by nid.
    struct AttachIncompleteMat {
        /// nid → Ok(complete) or Ok(incomplete) or Err hard
        map: HashMap<u64, Result<bool, ()>>,
    }

    impl MessageMaterializer for AttachIncompleteMat {
        fn materialize(
            &mut self,
            locus: &MessageLocus,
        ) -> Result<CanonicalMessage, MaterializeError> {
            match self.map.get(&locus.nid) {
                Some(Ok(complete)) => {
                    let stream_available = *complete;
                    Ok(CanonicalMessage {
                        locus: locus.clone(),
                        message_id: None,
                        subject: Some("s".into()),
                        sender: None,
                        display_to: None,
                        display_cc: None,
                        display_bcc: None,
                        recipients: Vec::new(),
                        message_flags: None,
                        submit_time: None,
                        size: Some(10),
                        message_class: None,
                        body_plain: Some("body".into()),
                        body_html: None,
                        attachments: vec![CanonicalAttachment {
                            filename: "a.bin".into(),
                            size: if stream_available { 10 } else { 0 },
                            mime: Some("application/octet-stream".into()),
                            data: if stream_available {
                                Some(vec![1, 2, 3])
                            } else {
                                None
                            },
                            stream_available,
                            attach_nid: Some(100),
                            attach_method: Some(1),
                            is_cloud_link: false,
                            cloud_provider: None,
                            cloud_url: None,
                            cloud_permission_type: None,
                            embedded_message: None,
                            embedded_extract_limit: false,
                        }],
                        fidelity: if stream_available {
                            RecoverableIntegrity::clean()
                        } else {
                            RecoverableIntegrity::with_degraded(
                                vec![IntegrityReason::AttachStreamOpenFailed],
                                false,
                            )
                        },
                        message_id_norm: None,
                        content_hash: [0; 32],
                        edrm_mih_hex: None,
                        body_incomplete: false,
                        body_unavailable: false,
                    })
                }
                Some(Err(())) | None => Err(MaterializeError::Hard(format!(
                    "forced fail nid={}",
                    locus.nid
                ))),
            }
        }
    }

    #[test]
    fn is_attach_incomplete_table() {
        let locus = MessageLocus {
            source_path: "C:/a.pst".into(),
            source_pst: "a.pst".into(),
            folder_path: "I".into(),
            nid: 1,
            is_orphaned: false,
        };
        let base = |atts: Vec<CanonicalAttachment>,
                    fidelity: RecoverableIntegrity,
                    body_u: bool,
                    body_i: bool| {
            CanonicalMessage {
                locus: locus.clone(),
                message_id: None,
                subject: Some("s".into()),
                sender: None,
                display_to: None,
                display_cc: None,
                display_bcc: None,
                recipients: Vec::new(),
                message_flags: None,
                submit_time: None,
                size: Some(10),
                message_class: None,
                body_plain: Some("b".into()),
                body_html: None,
                attachments: atts,
                fidelity,
                message_id_norm: None,
                content_hash: [0; 32],
                edrm_mih_hex: None,
                body_incomplete: body_i,
                body_unavailable: body_u,
            }
        };
        let ok_att = CanonicalAttachment {
            filename: "f.bin".into(),
            size: 0, // zero-byte success is still complete
            mime: None,
            data: Some(vec![]),
            stream_available: true,
            attach_nid: Some(1),
            attach_method: Some(1),
            is_cloud_link: false,
            cloud_provider: None,
            cloud_url: None,
            cloud_permission_type: None,
            embedded_message: None,
            embedded_extract_limit: false,
        };
        // Zero-byte by-value with empty display name (materializer must set stream_available).
        let zero_empty_name = CanonicalAttachment {
            filename: String::new(),
            size: 0,
            mime: None,
            data: Some(vec![]),
            stream_available: true,
            attach_nid: Some(1),
            attach_method: Some(1),
            is_cloud_link: false,
            cloud_provider: None,
            cloud_url: None,
            cloud_permission_type: None,
            embedded_message: None,
            embedded_extract_limit: false,
        };
        let bad_att = CanonicalAttachment {
            filename: "f.bin".into(),
            size: 10,
            mime: None,
            data: None,
            stream_available: false,
            attach_nid: Some(1),
            attach_method: Some(1),
            is_cloud_link: false,
            cloud_provider: None,
            cloud_url: None,
            cloud_permission_type: None,
            embedded_message: None,
            embedded_extract_limit: false,
        };

        // Positives
        assert!(is_attach_incomplete(&base(
            vec![bad_att.clone()],
            RecoverableIntegrity::clean(),
            false,
            false
        )));
        assert!(is_attach_incomplete(&base(
            vec![],
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::AttachMetaFailed], false),
            false,
            false
        )));
        assert!(is_attach_incomplete(&base(
            vec![ok_att.clone()],
            RecoverableIntegrity::with_degraded(
                vec![IntegrityReason::AttachStreamReadFailed],
                false
            ),
            false,
            false
        )));

        // Negatives
        assert!(!is_attach_incomplete(&base(
            vec![ok_att.clone()],
            RecoverableIntegrity::clean(),
            false,
            false
        )));
        // Spec §2.5 rule 5: zero-byte by-value empty name is NOT incomplete.
        assert!(!is_attach_incomplete(&base(
            vec![zero_empty_name],
            RecoverableIntegrity::clean(),
            false,
            false
        )));
        assert!(!is_attach_incomplete(&base(
            vec![], // parents_only omit
            RecoverableIntegrity::clean(),
            false,
            false
        )));
        assert!(!is_attach_incomplete(&base(
            vec![ok_att.clone()],
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::BodyUnavailable], false),
            true,
            false
        )));
        assert!(!is_attach_incomplete(&base(
            vec![ok_att],
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::CrcSuspect], false),
            false,
            false
        )));
        // Body incomplete alone is not attach-incomplete
        assert!(!is_attach_incomplete(&base(
            vec![],
            RecoverableIntegrity::clean(),
            false,
            true
        )));

        // 0084: explicit is_cloud_link is incomplete even if stream_available were true.
        let cloud = CanonicalAttachment {
            filename: "link.xlsx".into(),
            size: 0,
            mime: None,
            data: None,
            stream_available: true, // defensive: flag alone must still count
            attach_nid: Some(1),
            attach_method: Some(7),
            is_cloud_link: true,
            cloud_provider: Some("OneDrivePro".into()),
            cloud_url: Some("https://1drv.ms/x/s!abc".into()),
            cloud_permission_type: None,
            embedded_message: None,
            embedded_extract_limit: false,
        };
        assert!(is_attach_incomplete(&base(
            vec![cloud],
            RecoverableIntegrity::clean(),
            false,
            false
        )));
    }

    /// Materializer: nid → CloudLink incomplete (peer) vs complete by-value.
    /// Used to prove Mode A prefers physical peer over attachment-table cloud.
    struct CloudLinkMat {
        /// nids that are CloudLink (incomplete); others complete by-value
        cloud_nids: HashMap<u64, (String, String)>, // provider, url
    }

    impl MessageMaterializer for CloudLinkMat {
        fn materialize(
            &mut self,
            locus: &MessageLocus,
        ) -> Result<CanonicalMessage, MaterializeError> {
            if let Some((provider, url)) = self.cloud_nids.get(&locus.nid) {
                Ok(CanonicalMessage {
                    locus: locus.clone(),
                    message_id: None,
                    subject: Some("s".into()),
                    sender: None,
                    display_to: None,
                    display_cc: None,
                    display_bcc: None,
                    recipients: Vec::new(),
                    message_flags: None,
                    submit_time: None,
                    size: Some(10),
                    message_class: None,
                    body_plain: Some("body".into()),
                    body_html: None,
                    attachments: vec![CanonicalAttachment {
                        filename: "report.xlsx".into(),
                        size: 0,
                        mime: None,
                        data: None,
                        stream_available: false,
                        attach_nid: Some(100),
                        attach_method: Some(7), // ATTACH_BY_WEB_REFERENCE
                        is_cloud_link: true,
                        cloud_provider: Some(provider.clone()),
                        cloud_url: Some(url.clone()),
                        cloud_permission_type: None,
                        embedded_message: None,
                        embedded_extract_limit: false,
                    }],
                    fidelity: RecoverableIntegrity::with_degraded(
                        vec![IntegrityReason::AttachCloudLink],
                        false,
                    ),
                    message_id_norm: None,
                    content_hash: [0; 32],
                    edrm_mih_hex: None,
                    body_incomplete: false,
                    body_unavailable: false,
                })
            } else {
                Ok(CanonicalMessage {
                    locus: locus.clone(),
                    message_id: None,
                    subject: Some("s".into()),
                    sender: None,
                    display_to: None,
                    display_cc: None,
                    display_bcc: None,
                    recipients: Vec::new(),
                    message_flags: None,
                    submit_time: None,
                    size: Some(10),
                    message_class: None,
                    body_plain: Some("body".into()),
                    body_html: None,
                    attachments: vec![CanonicalAttachment {
                        filename: "a.bin".into(),
                        size: 10,
                        mime: Some("application/octet-stream".into()),
                        data: Some(vec![1, 2, 3]),
                        stream_available: true,
                        attach_nid: Some(100),
                        attach_method: Some(1),
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
                })
            }
        }
    }

    #[test]
    fn mode_a_promotes_physical_peer_over_cloud_link() {
        // DoD-3: cloud incomplete peer0 + physical complete peer1 → promote.
        let mid = Some("modea-cloud@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = CloudLinkMat {
            cloud_nids: HashMap::from([(
                1,
                (
                    "OneDrivePro".into(),
                    "https://contoso.sharepoint.com/sites/x/report.xlsx".into(),
                ),
            )]),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(count, 1);
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(
            ks.winners[0].locus.nid, 2,
            "Mode A must prefer physical attach peer over CloudLink"
        );
        assert!(ks.winners[0].promoted_from_failure);
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("promoted_after_attach_incomplete")
        );
        assert_eq!(ks.stats.promoted_after_attach_incomplete_count, 1);
        let skipped = dec
            .iter()
            .find(|d| d.nid == 1)
            .expect("cloud peer soft-skip");
        assert_eq!(skipped.role, DecisionRole::DupOf);
        assert!(
            ks.winners[0].duplicate_sources.iter().any(|s| s == "a.pst"),
            "dup_sources must keep full group"
        );

        // Soft-skip ledger records carry ATTACH_CLOUD_LINK + provider/url (0084).
        let mut resolved = resolve_groups_with_grouping(
            vec![
                item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false),
                item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false),
            ],
            FamilyPolicy::KeepAttachmentsWithParent,
            &RankContext::from_policy_and_prefer(KeepPolicy::FirstSeen, &[]),
            &GroupingContext::with_tier2(true),
            None,
        );
        let mut mat2 = CloudLinkMat {
            cloud_nids: HashMap::from([(
                1,
                (
                    "OneDrivePro".into(),
                    "https://contoso.sharepoint.com/sites/x/report.xlsx".into(),
                ),
            )]),
        };
        let fin = MaterializeFinalizeOpts {
            promote_on_attach_fail: true,
        };
        finalize_with_materialize_opts(&mut resolved, &mut mat2, &fin, &mut |_| Ok(()))
            .expect("finalize");
        let soft = resolved
            .soft_skip_attach_records
            .iter()
            .find(|r| r.msg_nid == 1)
            .expect("soft skip for cloud peer");
        assert_eq!(soft.reason_code, "ATTACH_CLOUD_LINK");
        assert_eq!(soft.cloud_provider, "OneDrivePro");
        assert!(
            soft.cloud_url.contains("sharepoint.com"),
            "soft skip must carry cloud_url: {}",
            soft.cloud_url
        );
    }

    #[test]
    fn mode_a_promotes_complete_peer_after_incomplete() {
        let mid = Some("modea@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = AttachIncompleteMat {
            // peer0 incomplete, peer1 complete
            map: HashMap::from([(1, Ok(false)), (2, Ok(true))]),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(count, 1);
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert!(ks.winners[0].promoted_from_failure);
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("promoted_after_attach_incomplete")
        );
        assert_eq!(ks.stats.promoted_after_attach_incomplete_count, 1);
        assert_eq!(ks.stats.mode_c_fallback_all_peers_incomplete_count, 0);
        let skipped = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(
            skipped.role,
            DecisionRole::DupOf,
            "soft skip must not be MaterializeFailed"
        );
        let uniq = dec.iter().find(|d| d.nid == 2).expect("b");
        assert_eq!(uniq.role, DecisionRole::Unique);
        assert!(uniq.promoted_from_failure);
        assert_eq!(uniq.decided_by, "promoted_after_attach_incomplete");
        // dup_sources must list the skipped incomplete peer's source
        assert!(
            ks.winners[0].duplicate_sources.iter().any(|s| s == "a.pst"),
            "dup_sources must keep full group: {:?}",
            ks.winners[0].duplicate_sources
        );
        assert_eq!(ks.winners[0].duplicate_source_count, 1);
    }

    #[test]
    fn mode_a_flag_off_accepts_incomplete_first_peer() {
        let mid = Some("modea-off@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = AttachIncompleteMat {
            map: HashMap::from([(1, Ok(false)), (2, Ok(true))]),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(count, 1);
        assert_eq!(ks.winners[0].locus.nid, 1, "flag off: first peer wins");
        assert!(!ks.winners[0].promoted_from_failure);
        assert_ne!(
            ks.winners[0].decided_by.as_deref(),
            Some("promoted_after_attach_incomplete")
        );
        assert_eq!(ks.stats.promoted_after_attach_incomplete_count, 0);
        let uniq = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(uniq.role, DecisionRole::Unique);
    }

    /// Soft-incomplete peer0 then hard-fail peer1 → Mode C fallback (not group_dropped).
    /// Regression for Codex P1: materializable soft-skip must not be discarded after
    /// later Hard failures leave `final_winner` unset.
    #[test]
    fn mode_a_soft_incomplete_then_hard_fails_mode_c_fallback() {
        let mid = Some("modea-soft-hard@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = AttachIncompleteMat {
            // peer0 attach-incomplete (soft skip), peer1 hard materialize fail
            map: HashMap::from([(1, Ok(false)), (2, Err(()))]),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(count, 1, "must export soft-incomplete fallback, not drop");
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(
            ks.stats.groups_dropped_materialize, 0,
            "must not group_drop when a materializable soft-skip exists"
        );
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert!(ks.winners[0].promoted_from_failure);
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("mode_c_fallback_all_peers_incomplete")
        );
        assert_eq!(ks.stats.mode_c_fallback_all_peers_incomplete_count, 1);
        assert_eq!(ks.stats.promoted_after_attach_incomplete_count, 0);
        let uniq = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(uniq.role, DecisionRole::Unique);
        assert_eq!(uniq.decided_by, "mode_c_fallback_all_peers_incomplete");
        assert!(uniq.promoted_from_failure);
        let hard = dec.iter().find(|d| d.nid == 2).expect("b");
        assert_eq!(
            hard.role,
            DecisionRole::MaterializeFailed,
            "hard-fail peer stays MaterializeFailed under Mode C fallback"
        );
    }

    #[test]
    fn mode_a_all_incomplete_mode_c_fallback() {
        let mid = Some("modea-all@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = AttachIncompleteMat {
            map: HashMap::from([(1, Ok(false)), (2, Ok(false))]),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(count, 1);
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(ks.stats.groups_dropped_materialize, 0);
        // Highest-ranked materializable = peer0 (first_seen / path order)
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert!(ks.winners[0].promoted_from_failure);
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("mode_c_fallback_all_peers_incomplete")
        );
        assert_eq!(ks.stats.mode_c_fallback_all_peers_incomplete_count, 1);
        let uniq = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(uniq.decided_by, "mode_c_fallback_all_peers_incomplete");
        let peer = dec.iter().find(|d| d.nid == 2).expect("b");
        assert_eq!(peer.role, DecisionRole::DupOf);
    }

    /// P3: least-incomplete must **not** re-rank. Three incomplete peers; peer1 is
    /// less incomplete than peer0 → still highest-ranked materializable (peer0)
    /// wins with `mode_c_fallback_all_peers_incomplete`.
    #[test]
    fn mode_a_least_incomplete_does_not_rerank() {
        let mid = Some("modea-least@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let c = item("C:/c.pst", "c.pst", "I", 3, mid, [1; 32], 100, 2, false);

        /// Graded incomplete: unavailable_attach_count (0 = complete).
        struct GradedIncompleteMat {
            map: HashMap<u64, Result<usize, ()>>,
        }
        impl MessageMaterializer for GradedIncompleteMat {
            fn materialize(
                &mut self,
                locus: &MessageLocus,
            ) -> Result<CanonicalMessage, MaterializeError> {
                match self.map.get(&locus.nid) {
                    Some(Ok(n_unavail)) => {
                        let n = *n_unavail;
                        let mut attachments = Vec::new();
                        if n == 0 {
                            attachments.push(CanonicalAttachment {
                                filename: "good.bin".into(),
                                size: 10,
                                mime: Some("application/octet-stream".into()),
                                data: Some(vec![1, 2, 3]),
                                stream_available: true,
                                attach_nid: Some(100),
                                attach_method: Some(1),
                                is_cloud_link: false,
                                cloud_provider: None,
                                cloud_url: None,
                                cloud_permission_type: None,
                                embedded_message: None,
                                embedded_extract_limit: false,
                            });
                        } else {
                            for i in 0..n {
                                attachments.push(CanonicalAttachment {
                                    filename: format!("bad{i}.bin"),
                                    size: 10,
                                    mime: Some("application/octet-stream".into()),
                                    data: None,
                                    stream_available: false,
                                    attach_nid: Some(100 + i as u64),
                                    attach_method: Some(1),
                                    is_cloud_link: false,
                                    cloud_provider: None,
                                    cloud_url: None,
                                    cloud_permission_type: None,
                                    embedded_message: None,
                                    embedded_extract_limit: false,
                                });
                            }
                        }
                        Ok(CanonicalMessage {
                            locus: locus.clone(),
                            message_id: None,
                            subject: Some("s".into()),
                            sender: None,
                            display_to: None,
                            display_cc: None,
                            display_bcc: None,
                            recipients: Vec::new(),
                            message_flags: None,
                            submit_time: None,
                            size: Some(10),
                            message_class: None,
                            body_plain: Some("body".into()),
                            body_html: None,
                            attachments,
                            fidelity: if n == 0 {
                                RecoverableIntegrity::clean()
                            } else {
                                RecoverableIntegrity::with_degraded(
                                    vec![IntegrityReason::AttachStreamOpenFailed],
                                    false,
                                )
                            },
                            message_id_norm: None,
                            content_hash: [0; 32],
                            edrm_mih_hex: None,
                            body_incomplete: false,
                            body_unavailable: false,
                        })
                    }
                    Some(Err(())) | None => Err(MaterializeError::Hard(format!(
                        "forced fail nid={}",
                        locus.nid
                    ))),
                }
            }
        }

        // peer0: 2 unavailable, peer1: 1 unavailable (less incomplete), peer2: 1.
        // Spec: no least-incomplete re-rank → peer0 (highest ranked) wins Mode C fallback.
        let mut mat = GradedIncompleteMat {
            map: HashMap::from([(1, Ok(2)), (2, Ok(1)), (3, Ok(1))]),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b, c],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(count, 1);
        assert_eq!(
            ks.winners[0].locus.nid, 1,
            "must keep highest-ranked materializable, not least-incomplete peer"
        );
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("mode_c_fallback_all_peers_incomplete")
        );
        assert_eq!(ks.stats.mode_c_fallback_all_peers_incomplete_count, 1);
        assert_eq!(ks.stats.promoted_after_attach_incomplete_count, 0);
        let uniq = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(uniq.decided_by, "mode_c_fallback_all_peers_incomplete");
        // Peers remain DupOf (not dropped).
        assert_eq!(
            dec.iter().find(|d| d.nid == 2).map(|d| d.role),
            Some(DecisionRole::DupOf)
        );
        assert_eq!(
            dec.iter().find(|d| d.nid == 3).map(|d| d.role),
            Some(DecisionRole::DupOf)
        );
    }

    #[test]
    fn mode_a_dup_sources_multi_source_after_promote() {
        let mid = Some("modea-dups@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let c = item("C:/c.pst", "c.pst", "I", 3, mid, [1; 32], 100, 2, false);
        let mut mat = AttachIncompleteMat {
            // incomplete, complete, incomplete — promote to b
            map: HashMap::from([(1, Ok(false)), (2, Ok(true)), (3, Ok(false))]),
        };
        let (ks, _dec, _) = build_keep_set_materialized(
            vec![a, b, c],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert_eq!(ks.winners[0].duplicate_source_count, 2);
        let names = &ks.winners[0].duplicate_sources;
        assert!(names.iter().any(|s| s == "a.pst"), "{names:?}");
        assert!(names.iter().any(|s| s == "c.pst"), "{names:?}");
    }

    #[test]
    fn mode_a_hard_promote_still_materialize_fail_string() {
        // Flag on, but only hard fails — not soft incomplete.
        let mid = Some("modea-hard@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = AttachIncompleteMat {
            map: HashMap::from([(1, Err(())), (2, Ok(true))]),
        };
        let (ks, dec, _) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::default(),
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: true,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("promoted_after_materialize_fail")
        );
        let failed = dec.iter().find(|d| d.nid == 1).expect("a");
        assert_eq!(failed.role, DecisionRole::MaterializeFailed);
    }

    #[test]
    fn all_materialize_fail_zero_winners() {
        let mid = Some("allfail@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = MockMaterializer {
            map: HashMap::from([(1, Err(())), (2, Err(()))]),
            family: FamilyPolicy::default(),
        };
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::default(),
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(ks.stats.unique, 0);
        assert_eq!(ks.winners.len(), 0);
        assert_eq!(ks.stats.groups_dropped_materialize, 1);
        assert_eq!(count, 0);
        assert!(dec
            .iter()
            .all(|d| d.role == DecisionRole::MaterializeFailed));
    }

    #[test]
    fn soft_body_unavailable_writes_back_to_decision() {
        struct SoftBodyMat;
        impl MessageMaterializer for SoftBodyMat {
            fn materialize(
                &mut self,
                locus: &MessageLocus,
            ) -> Result<CanonicalMessage, MaterializeError> {
                Ok(CanonicalMessage {
                    locus: locus.clone(),
                    message_id: None,
                    subject: Some("s".into()),
                    sender: None,
                    display_to: None,
                    display_cc: None,
                    display_bcc: None,
                    recipients: Vec::new(),
                    message_flags: None,
                    submit_time: None,
                    size: Some(10),
                    message_class: None,
                    body_plain: None,
                    body_html: None,
                    attachments: Vec::new(),
                    fidelity: RecoverableIntegrity::clean(),
                    message_id_norm: None,
                    content_hash: [0; 32],
                    edrm_mih_hex: None,
                    body_incomplete: false,
                    body_unavailable: true,
                })
            }
        }
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("soft@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let mut mat = SoftBodyMat;
        let (ks, dec, _) = build_keep_set_materialized(
            vec![a],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::default(),
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(ks.stats.unique, 1);
        assert!(ks.winners[0].integrity.degraded);
        assert!(ks.winners[0]
            .integrity
            .degraded_reasons
            .contains(&crate::integrity::IntegrityReason::BodyUnavailable));
        assert!(dec[0].degraded);
        assert!(dec[0]
            .degraded_reasons
            .iter()
            .any(|r| r == "BODY_UNAVAILABLE"));
    }

    #[test]
    fn soft_attach_meta_failed_writes_back_to_decision() {
        // Simulates list_attachments / open_attachment_data soft failure honesty
        // (production PstMaterializer sets ATTACH_META_FAILED on fidelity).
        struct SoftAttachMat;
        impl MessageMaterializer for SoftAttachMat {
            fn materialize(
                &mut self,
                locus: &MessageLocus,
            ) -> Result<CanonicalMessage, MaterializeError> {
                Ok(CanonicalMessage {
                    locus: locus.clone(),
                    message_id: None,
                    subject: Some("s".into()),
                    sender: None,
                    display_to: None,
                    display_cc: None,
                    display_bcc: None,
                    recipients: Vec::new(),
                    message_flags: None,
                    submit_time: None,
                    size: Some(10),
                    message_class: None,
                    body_plain: Some("body".into()),
                    body_html: None,
                    // Metadata may be empty when list failed; fidelity carries the reason.
                    attachments: Vec::new(),
                    fidelity: RecoverableIntegrity::with_degraded(
                        vec![IntegrityReason::AttachMetaFailed],
                        false,
                    ),
                    message_id_norm: None,
                    content_hash: [0; 32],
                    edrm_mih_hex: None,
                    body_incomplete: false,
                    body_unavailable: false,
                })
            }
        }
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("att@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let mut mat = SoftAttachMat;
        let (ks, dec, _) = build_keep_set_materialized(
            vec![a],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |_| Ok(()),
        )
        .expect("m");
        assert_eq!(ks.stats.unique, 1);
        assert!(ks.winners[0].integrity.degraded);
        assert!(ks.winners[0]
            .integrity
            .degraded_reasons
            .contains(&IntegrityReason::AttachMetaFailed));
        assert!(dec[0].degraded);
        assert!(dec[0]
            .degraded_reasons
            .iter()
            .any(|r| r == "ATTACH_META_FAILED"));
    }

    /// 0079 DoD-7: hard-fail promotion + first-materialize soft reasons only.
    ///
    /// Peer A hard-fails → promote to peer B. Soft reasons on B diverge if
    /// materialize is called a second time (call counter adds an extra reason
    /// on the 2nd success). Keep-set / on_winner must see **only** the first
    /// materialize soft set — the class of bug a prepare re-materialize would
    /// introduce when the merge point moves with D1.
    #[test]
    fn promote_first_materialize_soft_reasons_only_no_second_call_pollution() {
        struct CountingSoftMat {
            /// Successful materialize calls for nid 2 (peer B).
            b_ok_calls: u32,
        }
        impl MessageMaterializer for CountingSoftMat {
            fn materialize(
                &mut self,
                locus: &MessageLocus,
            ) -> Result<CanonicalMessage, MaterializeError> {
                match locus.nid {
                    1 => Err(MaterializeError::Hard("peer A hard-fail".into())),
                    2 => {
                        self.b_ok_calls = self.b_ok_calls.saturating_add(1);
                        // First success: attach soft fail only (realistic materialize soft).
                        // Second success would also claim CRC_SUSPECT — pre-D1 prepare
                        // re-materialize divergence class.
                        let reasons = if self.b_ok_calls == 1 {
                            vec![IntegrityReason::AttachMetaFailed]
                        } else {
                            vec![
                                IntegrityReason::AttachMetaFailed,
                                IntegrityReason::CrcSuspect,
                            ]
                        };
                        Ok(CanonicalMessage {
                            locus: locus.clone(),
                            message_id: None,
                            subject: Some("s".into()),
                            sender: None,
                            display_to: None,
                            display_cc: None,
                            display_bcc: None,
                            recipients: Vec::new(),
                            message_flags: None,
                            submit_time: None,
                            size: Some(10),
                            message_class: None,
                            body_plain: Some("body".into()),
                            body_html: None,
                            attachments: Vec::new(),
                            fidelity: RecoverableIntegrity::with_degraded(reasons, false),
                            message_id_norm: None,
                            content_hash: [0; 32],
                            edrm_mih_hex: None,
                            body_incomplete: false,
                            body_unavailable: false,
                        })
                    }
                    _ => Err(MaterializeError::Hard(format!("unknown nid={}", locus.nid))),
                }
            }
        }

        let mid = Some("promo-soft@x");
        let a = item("C:/a.pst", "a.pst", "I", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, mid, [1; 32], 100, 1, false);
        let mut mat = CountingSoftMat { b_ok_calls: 0 };
        let mut on_winner_reasons: Option<Vec<IntegrityReason>> = None;
        let (ks, dec, count) = build_keep_set_materialized(
            vec![a, b],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::default(),
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |msg| {
                on_winner_reasons = Some(msg.fidelity.degraded_reasons.clone());
                Ok(())
            },
        )
        .expect("promote materialize");

        // Single winner, single successful materialize (messages_materialized style).
        assert_eq!(count, 1, "materialized winner count must equal unique");
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(
            mat.b_ok_calls, 1,
            "finalize must call materialize once on winner B"
        );
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert!(ks.winners[0].promoted_from_failure);

        // Keep-set + on_winner see first-call soft set only.
        let expected_first = vec![IntegrityReason::AttachMetaFailed];
        assert_eq!(
            ks.winners[0].integrity.degraded_reasons, expected_first,
            "keep-set must not pick up second-materialize-only soft reasons"
        );
        assert_eq!(
            on_winner_reasons.expect("on_winner fired"),
            expected_first,
            "on_winner fidelity must match first materialize soft reasons"
        );
        let uniq = dec.iter().find(|d| d.nid == 2).expect("B unique");
        assert!(uniq.degraded);
        assert_eq!(
            uniq.degraded_reasons,
            vec!["ATTACH_META_FAILED".to_string()]
        );
        assert!(uniq.promoted_from_failure);

        // Prove the divergence class: a second materialize of B would add CRC_SUSPECT.
        let locus_b = MessageLocus {
            source_path: "C:/b.pst".into(),
            source_pst: "b.pst".into(),
            folder_path: "I".into(),
            nid: 2,
            is_orphaned: false,
        };
        let second = mat.materialize(&locus_b).expect("second call succeeds");
        assert_eq!(mat.b_ok_calls, 2);
        assert!(
            second
                .fidelity
                .degraded_reasons
                .contains(&IntegrityReason::CrcSuspect),
            "second materialize would add CRC_SUSPECT — pre-D1 prepare re-call hazard"
        );
        assert_ne!(
            second.fidelity.degraded_reasons, ks.winners[0].integrity.degraded_reasons,
            "second-call reasons must diverge from keep-set first-call set"
        );
    }

    #[test]
    fn write_decisions_csv_streams_without_to_decisions() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("stream@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("stream@x"),
            [1; 32],
            10,
            1,
            false,
        );
        let resolved = resolve_groups(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
            None,
        );
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("stream.csv");
        let mut w = DecisionCsvWriter::create(&path).expect("w");
        resolved.write_decisions_csv(&mut w).expect("stream");
        w.flush().expect("f");
        assert_eq!(w.rows_written(), 2);
        let text = std::fs::read_to_string(&path).expect("r");
        assert!(text.starts_with("SourcePath,"));
        assert_eq!(text.lines().count(), 3); // header + 2
    }

    #[test]
    fn decision_csv_roundtrip_columns() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("csv@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("csv@x"),
            [1; 32],
            10,
            1,
            false,
        );
        let (_ks, dec) = build_keep_set(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b");
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("dec.csv");
        let mut w = DecisionCsvWriter::create(&path).expect("w");
        w.write_all(&dec).expect("wa");
        w.flush().expect("f");
        let text = std::fs::read_to_string(&path).expect("r");
        assert!(text.starts_with("SourcePath,"));
        assert!(text.contains("unique") || text.contains("dup_of"));
        assert_eq!(text.lines().count(), 1 + dec.len());
    }

    #[test]
    fn write_keep_set_json_file() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("json@x"),
            [3; 32],
            10,
            0,
            false,
        );
        let (ks, _) = build_keep_set(
            vec![a],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b");
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("ks.json");
        write_keep_set_json(&path, &ks).expect("w");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("r")).expect("j");
        assert_eq!(v["schema"], KEEP_SET_SCHEMA);
    }

    #[test]
    fn provenance_field() {
        let a = item("C:/a.pst", "a.pst", "I", 1, None, [1; 32], 1, 0, false);
        let mut resolved = resolve_groups(
            vec![a],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
            Some(KeepSetProvenance {
                scan_integrity_schema: SCAN_INTEGRITY_SCHEMA.into(),
                mode: "best-effort".into(),
                input_files: vec!["C:/a.pst".into()],
            }),
        );
        let ks = resolved.to_keep_set();
        assert_eq!(
            ks.created_from
                .as_ref()
                .map(|c| c.scan_integrity_schema.as_str()),
            Some(SCAN_INTEGRITY_SCHEMA)
        );
        // silence
        let _ = &mut resolved;
    }

    #[test]
    fn keep_set_winners_sorted_path_nid_not_group_order() {
        // Two singleton groups: scan/group order is z then a; keep_set sorts a then z.
        let z = item(
            "C:/z.pst",
            "z.pst",
            "I",
            10,
            Some("z@x"),
            [9; 32],
            10,
            0,
            false,
        );
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            20,
            Some("a@x"),
            [8; 32],
            10,
            1,
            false,
        );
        let mut mat = MockMaterializer {
            map: HashMap::from([(10, Ok(0)), (20, Ok(0))]),
            family: FamilyPolicy::default(),
        };
        let mut finalize_order: Vec<(String, u64)> = Vec::new();
        let (ks, _dec, count) = build_keep_set_materialized(
            vec![z, a],
            MaterializeBuildOpts {
                policy: KeepPolicy::FirstSeen,
                family_policy: FamilyPolicy::default(),
                prefer_path: &[],
                tier2_enabled: true,
                created_from: None,
                rank_ctx: None,
                grouping_ctx: None,
                promote_on_attach_fail: false,
            },
            &mut mat,
            |msg| {
                finalize_order.push((msg.locus.source_path.clone(), msg.locus.nid));
                Ok(())
            },
        )
        .expect("build");
        assert_eq!(count, 2);
        // Group iteration follows scan/group order (z before a).
        assert_eq!(finalize_order[0].0, "C:/z.pst");
        assert_eq!(finalize_order[1].0, "C:/a.pst");
        // keep_set.winners is path+nid sorted (a before z) — export must follow this.
        assert_eq!(ks.winners.len(), 2);
        assert_eq!(ks.winners[0].locus.source_path, "C:/a.pst");
        assert_eq!(ks.winners[0].locus.nid, 20);
        assert_eq!(ks.winners[1].locus.source_path, "C:/z.pst");
        assert_eq!(ks.winners[1].locus.nid, 10);
        assert_ne!(
            finalize_order.iter().map(|(_, n)| *n).collect::<Vec<_>>(),
            ks.winners.iter().map(|w| w.locus.nid).collect::<Vec<_>>(),
            "finalize on_winner order must differ from keep_set winner order in this fixture"
        );
    }

    // ─── 0075 winner policies ─────────────────────────────────────────────

    #[test]
    fn decision_csv_header_starts_with_v1_prefix() {
        for (i, col) in DECISION_CSV_HEADER_V1.iter().enumerate() {
            assert_eq!(DECISION_CSV_HEADER[i], *col, "column {i} must remain {col}");
        }
        assert_eq!(DECISION_CSV_HEADER.len(), 31);
        assert_eq!(DECISION_CSV_HEADER_V1.len(), 19);
    }

    #[test]
    fn earliest_date_earlier_submit_wins() {
        let mid = Some("d@x");
        let later = item_dated(
            "C:/a.pst",
            "a.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
            Some(200),
            None,
            false,
        );
        let earlier = item_dated(
            "C:/b.pst",
            "b.pst",
            "Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
            Some(100),
            None,
            false,
        );
        let ctx = RankContext::new(KeepPolicy::EarliestDate);
        let (ks, _) =
            build_keep_set_with_ctx(vec![later, earlier], FamilyPolicy::default(), &ctx, true)
                .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 2);
    }

    #[test]
    fn earliest_date_equal_dates_fall_through_to_path_key() {
        let mid = Some("eq@x");
        // Same submit time; later path_key (b.pst) should lose to a.pst.
        let a = item_dated(
            "C:/a.pst",
            "a.pst",
            "Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
            Some(100),
            None,
            false,
        );
        let b = item_dated(
            "C:/b.pst",
            "b.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
            Some(100),
            None,
            false,
        );
        let ctx = RankContext::new(KeepPolicy::EarliestDate);
        let (ks, dec) = build_keep_set_with_ctx(vec![b, a], FamilyPolicy::default(), &ctx, true)
            .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert_eq!(ks.winners[0].locus.source_pst, "a.pst");
        let win = dec
            .iter()
            .find(|d| d.role == DecisionRole::Unique)
            .expect("u");
        assert!(
            win.decided_by == "path_order" || win.decided_by == "nid",
            "equal dates should fall through, got {}",
            win.decided_by
        );
    }

    #[test]
    fn earliest_date_missing_sorts_last() {
        let mid = Some("m@x");
        let dated = item_dated(
            "C:/a.pst",
            "a.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            1,
            false,
            Some(50),
            None,
            false,
        );
        let undated = item_dated(
            "C:/b.pst", "b.pst", "Inbox", 2, mid, [1; 32], 100, 0, false, None, None, false,
        );
        let ctx = RankContext::new(KeepPolicy::EarliestDate);
        let (ks, _) =
            build_keep_set_with_ctx(vec![undated, dated], FamilyPolicy::default(), &ctx, true)
                .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 1);
    }

    #[test]
    fn earliest_date_delivery_fallback_when_submit_absent() {
        let mid = Some("f@x");
        let via_delivery = item_dated(
            "C:/a.pst",
            "a.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
            None,
            Some(10),
            false,
        );
        let via_submit = item_dated(
            "C:/b.pst",
            "b.pst",
            "Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
            Some(20),
            None,
            false,
        );
        let ctx = RankContext::new(KeepPolicy::EarliestDate);
        let (ks, dec) = build_keep_set_with_ctx(
            vec![via_delivery, via_submit],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        // delivery=10 beats submit=20
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert_eq!(ks.stats.groups_date_source_mixed, 1);
        let w = dec
            .iter()
            .find(|d| d.role == DecisionRole::Unique)
            .expect("u");
        assert_eq!(w.date_source, "delivery");
    }

    #[test]
    fn folder_class_purges_loses_to_inbox() {
        let mid = Some("p@x");
        let purge = item(
            "C:/a.pst",
            "a.pst",
            "Top of Personal Folders/Recoverable Items/Purges",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let inbox = item(
            "C:/b.pst",
            "b.pst",
            "Top of Personal Folders/Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.folder_rank = FolderRankMode::Builtin;
        let (ks, dec) =
            build_keep_set_with_ctx(vec![purge, inbox], FamilyPolicy::default(), &ctx, true)
                .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 2);
        let loser = dec.iter().find(|d| d.nid == 1).expect("purge");
        assert_eq!(loser.folder_class, "recoverable_purges");
        assert_eq!(loser.decided_by, "folder_class");
    }

    #[test]
    fn folder_class_user_purges_not_demoted() {
        let mid = Some("u@x");
        // User folder literally named Purges without Recoverable Items ancestor.
        let user_purges = item(
            "C:/a.pst",
            "a.pst",
            "Top of Personal Folders/Purges",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let inbox = item(
            "C:/b.pst",
            "b.pst",
            "Top of Personal Folders/Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.folder_rank = FolderRankMode::Builtin;
        let (ks, _) = build_keep_set_with_ctx(
            vec![user_purges, inbox],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        // Both primary rank → first_seen by scan_order (user_purges scan 0 wins).
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert_eq!(
            classify_folder("Top of Personal Folders/Purges"),
            FolderClass::Primary
        );
    }

    #[test]
    fn folder_class_segment_rejects_myversions() {
        assert_eq!(classify_folder("Top/MyVersions"), FolderClass::Primary);
        assert_eq!(
            classify_folder("Top/Recoverable Items/Versions"),
            FolderClass::RecoverableVersions
        );
    }

    #[test]
    fn folder_class_min_rank_among_multi_segment_matches() {
        // Archive (2) beats Junk Email (3) even when Junk is checked first in old code.
        assert_eq!(classify_folder("Archive/Junk Email"), FolderClass::Archive);
        assert_eq!(
            classify_folder("Top/Archive/Junk E-mail"),
            FolderClass::Archive
        );
        // Drafts (4) is the only special segment; Inbox is primary by default.
        assert_eq!(classify_folder("Inbox/Drafts"), FolderClass::Drafts);
        // Sent (0) beats Archive (2).
        assert_eq!(
            classify_folder("Archive/Sent Items"),
            FolderClass::SentItems
        );
        // Recoverable: Purges (9) beats Versions (10) when both present.
        assert_eq!(
            classify_folder("Top/Recoverable Items/Purges/Versions"),
            FolderClass::RecoverablePurges
        );
        // Versions under Recoverable Items still qualifies (parent-qualified).
        assert_eq!(
            classify_folder("Top/Recoverable Items/Versions"),
            FolderClass::RecoverableVersions
        );
        // User folder Purges (no RI ancestor) stays primary.
        assert_eq!(
            classify_folder("Top of Personal Folders/Purges"),
            FolderClass::Primary
        );
        // Global min-rank: Sent Items (0) beats co-present recoverable_purges (9).
        assert_eq!(
            classify_folder("Recoverable Items/Purges/Sent Items"),
            FolderClass::SentItems
        );
        // Pure dumpster still demotes (no better non-recoverable class).
        assert_eq!(
            classify_folder("Top/Recoverable Items/Purges"),
            FolderClass::RecoverablePurges
        );
    }

    #[test]
    fn pre1970_filetime_formats_and_resolves() {
        // 1960-01-01T00:00:00Z → unix ≈ -315619200
        let unix_1960 = -315_619_200i64;
        let ft = (unix_1960 + 11_644_473_600) * 10_000_000;
        assert!(ft > 0, "pre-1970 FILETIME must still be positive");
        let iso = format_date_filetime_utc(Some(ft));
        assert_eq!(iso, "1960-01-01T00:00:00Z");
        // resolve_item_date accepts any ft > 0 (delivery fallback).
        let mut it = item("C:/a.pst", "a.pst", "Inbox", 1, None, [0; 32], 1, 0, false);
        it.submit_time = None;
        it.delivery_time = Some(ft);
        let (resolved, src) = resolve_item_date(&it);
        assert_eq!(resolved, Some(ft));
        assert_eq!(src, DateSource::Delivery);
        // Epoch boundary still works.
        let ft_epoch = 11_644_473_600i64 * 10_000_000;
        assert_eq!(
            format_date_filetime_utc(Some(ft_epoch)),
            "1970-01-01T00:00:00Z"
        );
        // Missing / non-positive stay empty.
        assert_eq!(format_date_filetime_utc(None), "");
        assert_eq!(format_date_filetime_utc(Some(0)), "");
        assert_eq!(format_date_filetime_utc(Some(-1)), "");
    }

    #[test]
    fn folder_class_sent_items_beats_inbox() {
        let mid = Some("s@x");
        let sent = item(
            "C:/a.pst",
            "a.pst",
            "Top/Sent Items",
            1,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        let inbox = item(
            "C:/b.pst",
            "b.pst",
            "Top/Inbox",
            2,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.folder_rank = FolderRankMode::Builtin;
        let (ks, _) =
            build_keep_set_with_ctx(vec![inbox, sent], FamilyPolicy::default(), &ctx, true)
                .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert_eq!(ks.winners[0].folder_class.as_deref(), Some("sent_items"));
    }

    #[test]
    fn folder_class_drafts_outbox_lose_to_junk() {
        assert!(FolderClass::JunkEmail.builtin_rank() < FolderClass::Drafts.builtin_rank());
        assert!(FolderClass::JunkEmail.builtin_rank() < FolderClass::Outbox.builtin_rank());
        assert!(FolderClass::Primary.builtin_rank() < FolderClass::JunkEmail.builtin_rank());
        assert!(FolderClass::SentItems.builtin_rank() < FolderClass::Primary.builtin_rank());
        assert!(
            FolderClass::RecoverablePurges.builtin_rank()
                < FolderClass::RecoverableVersions.builtin_rank()
        );
    }

    #[test]
    fn prefer_bcc_copy_sent_beats_inbox() {
        let mid = Some("bcc@x");
        let sent_bcc = item_dated(
            "C:/a.pst",
            "a.pst",
            "Sent Items",
            1,
            mid,
            [1; 32],
            100,
            1,
            false,
            Some(100),
            None,
            true,
        );
        let inbox_no = item_dated(
            "C:/b.pst",
            "b.pst",
            "Inbox",
            2,
            mid,
            [1; 32],
            100,
            0,
            false,
            Some(100),
            None,
            false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.prefer_bcc_copy = true;
        let (ks, _) = build_keep_set_with_ctx(
            vec![inbox_no, sent_bcc],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert_eq!(
            ks.winners[0].decided_by.as_deref(),
            Some("bcc_completeness")
        );
    }

    #[test]
    fn prefer_bcc_off_preserves_first_seen() {
        let mid = Some("bcc2@x");
        let sent_bcc = item_dated(
            "C:/z.pst",
            "z.pst",
            "Sent Items",
            1,
            mid,
            [1; 32],
            100,
            1,
            false,
            None,
            None,
            true,
        );
        let inbox_no = item_dated(
            "C:/a.pst", "a.pst", "Inbox", 2, mid, [1; 32], 100, 0, false, None, None, false,
        );
        // Flag off: scan_order 0 (inbox) wins; BCC peer loss is still counted.
        let ctx = RankContext::new(KeepPolicy::FirstSeen);
        let (ks, _) = build_keep_set_with_ctx(
            vec![inbox_no, sent_bcc],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 2);
        assert_eq!(ks.stats.winners_without_bcc_peer_had_bcc, 1);
    }

    #[test]
    fn whitespace_bcc_counts_as_absent() {
        // Mirror scan.rs: only non-empty trim sets has_bcc.
        let display_bcc = Some("   \t  ".to_string());
        let has_bcc = display_bcc
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        assert!(!has_bcc);

        let mid = Some("ws@x");
        let with_flag_false = item_dated(
            "C:/a.pst",
            "a.pst",
            "Sent Items",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
            None,
            None,
            false, // whitespace-equivalent: no BCC
        );
        let peer = item_dated(
            "C:/b.pst", "b.pst", "Inbox", 2, mid, [1; 32], 100, 1, false, None, None, false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.prefer_bcc_copy = true;
        let (ks, _) = build_keep_set_with_ctx(
            vec![with_flag_false, peer],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        // Neither has BCC → first_seen (scan_order 0) wins; BCC rung is a no-op.
        assert_eq!(ks.winners[0].locus.nid, 1);
    }

    #[test]
    fn custom_folder_rank_unmatched_is_best() {
        let mid = Some("cf@x");
        let demoted = item(
            "C:/a.pst",
            "a.pst",
            "Top/Recoverable Items/Purges",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let primary = item(
            "C:/b.pst",
            "b.pst",
            "Top/Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.folder_rank = FolderRankMode::Custom(vec!["*/Purges".into()]);
        let (ks, _) =
            build_keep_set_with_ctx(vec![demoted, primary], FamilyPolicy::default(), &ctx, true)
                .expect("build");
        assert_eq!(ks.winners[0].locus.nid, 2);
    }

    #[test]
    fn segment_glob_leading_trailing_only() {
        assert!(segment_glob_match("Purges", "*Purges"));
        assert!(segment_glob_match("SoftPurges", "*Purges"));
        assert!(segment_glob_match("ElementX", "Element*"));
        assert!(segment_glob_match("xxElementyy", "*Element*"));
        // Internal `*` (not only leading/trailing) is not a wildcard.
        assert!(!segment_glob_match("axxb", "a*b"));
        assert!(!segment_glob_match("PurgesExtra", "*Purges")); // leading * = ends_with
                                                                // Multi-segment patterns use path_matches_folder_pattern, not this helper alone.
        assert!(path_matches_folder_pattern(
            &["Top".into(), "Recoverable Items".into(), "Purges".into()],
            "*/Purges"
        ));
        assert!(!path_matches_folder_pattern(
            &["Top".into(), "Purges".into()],
            "Recoverable Items/Purges"
        ));
    }

    #[test]
    fn source_rank_ordered_primary_beats_dash2() {
        let mid = Some("inc@x");
        // path_key lexicographically prefers -2 on Windows lowercased paths...
        let dash2 = item(
            r"C:\matter\INC0102784-2.pst",
            "INC0102784-2.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let primary = item(
            r"C:\matter\INC0102784.pst",
            "INC0102784.pst",
            "Inbox",
            2,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        // Without source-rank: first_seen by scan_order (dash2 wins).
        let (ks0, _) = build_keep_set(
            vec![dash2.clone(), primary.clone()],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks0.winners[0].locus.source_pst, "INC0102784-2.pst");

        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.source_rank_patterns = vec!["INC0102784.pst".into(), "INC0102784-2.pst".into()];
        let (ks1, dec) =
            build_keep_set_with_ctx(vec![dash2, primary], FamilyPolicy::default(), &ctx, true)
                .expect("build");
        assert_eq!(ks1.winners[0].locus.source_pst, "INC0102784.pst");
        let w = dec
            .iter()
            .find(|d| d.role == DecisionRole::Unique)
            .expect("u");
        assert_eq!(w.decided_by, "source_rank");
    }

    #[test]
    fn ladder_source_outranks_folder_ceo_archive_vs_junior_inbox() {
        let mid = Some("ceo@x");
        let ceo_archive = item(
            r"C:\cust\CEO.pst",
            "CEO.pst",
            "Top/Archive",
            1,
            mid,
            [1; 32],
            100,
            1,
            false,
        );
        let junior_inbox = item(
            r"C:\cust\junior.pst",
            "junior.pst",
            "Top/Inbox",
            2,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.source_rank_patterns = vec!["CEO.pst".into()];
        ctx.folder_rank = FolderRankMode::Builtin;
        let (ks, _) = build_keep_set_with_ctx(
            vec![junior_inbox.clone(), ceo_archive.clone()],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        assert_eq!(ks.winners[0].locus.source_pst, "CEO.pst");

        // Invert: folder_class first → Inbox (primary) beats Archive.
        ctx.folder_class_first = true;
        let (ks2, _) = build_keep_set_with_ctx(
            vec![junior_inbox, ceo_archive],
            FamilyPolicy::default(),
            &ctx,
            true,
        )
        .expect("build");
        assert_eq!(ks2.winners[0].locus.source_pst, "junior.pst");
    }

    #[test]
    fn decided_by_sole_member() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "Inbox",
            1,
            Some("solo@x"),
            [9; 32],
            10,
            0,
            false,
        );
        let (ks, dec) = build_keep_set(
            vec![a],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.stats.unique, 1);
        assert_eq!(dec[0].decided_by, "sole_member");
        assert_eq!(ks.winners[0].decided_by.as_deref(), Some("sole_member"));
    }

    #[test]
    fn graded_vs_binary_fidelity() {
        let mid = Some("g@x");
        let attach_loss = {
            let mut it = item("C:/a.pst", "a.pst", "Inbox", 1, mid, [1; 32], 100, 0, false);
            it.integrity = RecoverableIntegrity::with_degraded(
                vec![IntegrityReason::AttachStreamReadFailed],
                false,
            );
            it
        };
        let body_loss = {
            let mut it = item("C:/b.pst", "b.pst", "Inbox", 2, mid, [1; 32], 100, 1, false);
            it.integrity =
                RecoverableIntegrity::with_degraded(vec![IntegrityReason::BodyTruncated], false);
            it
        };
        // Binary: both degraded → first_seen (scan 0 attach_loss wins).
        let bin = RankContext {
            fidelity_mode: FidelityMode::Binary,
            ..RankContext::new(KeepPolicy::FirstSeen)
        };
        let (ks_bin, _) = build_keep_set_with_ctx(
            vec![attach_loss.clone(), body_loss.clone()],
            FamilyPolicy::default(),
            &bin,
            true,
        )
        .expect("build");
        assert_eq!(ks_bin.winners[0].locus.nid, 1);

        // Graded: attach tier 2 beats body tier 3.
        let graded = RankContext {
            fidelity_mode: FidelityMode::Graded,
            ..RankContext::new(KeepPolicy::FirstSeen)
        };
        let (ks_g, dec) = build_keep_set_with_ctx(
            vec![body_loss, attach_loss],
            FamilyPolicy::default(),
            &graded,
            true,
        )
        .expect("build");
        assert_eq!(ks_g.winners[0].locus.nid, 1);
        assert_eq!(
            dec.iter()
                .find(|d| d.role == DecisionRole::Unique)
                .map(|d| d.decided_by.as_str()),
            Some("fidelity")
        );
    }

    #[test]
    fn reason_fidelity_tier_exhaustive_and_binary_map() {
        use IntegrityReason::*;
        let soft = [
            AttachMetaFailed,
            AttachProbeTruncated,
            AttachPeerProbeCap,
            AttachProbeTimeout,
        ];
        for r in soft {
            assert_eq!(reason_fidelity_tier(r), 1);
        }
        let attach = [
            AttachStreamOpenFailed,
            AttachStreamReadFailed,
            AttachStreamCrc,
            AttachBlockNotFound,
            AttachDataTruncated,
            AttachMethodUnsupported,
            AttachCloudLink,
        ];
        for r in attach {
            assert_eq!(reason_fidelity_tier(r), 2);
        }
        let body = [
            BodyTruncated,
            BodyUnavailable,
            DataTruncated,
            CrcMismatch,
            CrcSuspect,
            BlockNotFound,
        ];
        for r in body {
            assert_eq!(reason_fidelity_tier(r), 3);
        }
        // Binary map: graded 0 → 0; 1..4 → 1
        let clean = item("C:/a.pst", "a.pst", "Inbox", 1, None, [0; 32], 1, 0, false);
        assert_eq!(fidelity_rank_with_mode(&clean, FidelityMode::Binary), 0);
        assert_eq!(fidelity_rank_with_mode(&clean, FidelityMode::Graded), 0);
        let mut deg = clean.clone();
        deg.integrity =
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::AttachMetaFailed], false);
        assert_eq!(fidelity_rank_with_mode(&deg, FidelityMode::Graded), 1);
        assert_eq!(fidelity_rank_with_mode(&deg, FidelityMode::Binary), 1);
        deg.integrity =
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::BodyTruncated], false);
        assert_eq!(fidelity_rank_with_mode(&deg, FidelityMode::Graded), 3);
        assert_eq!(fidelity_rank_with_mode(&deg, FidelityMode::Binary), 1);
        // 0077 DoD-21: clean outranks CRC_SUSPECT under graded and binary.
        let mut suspect = clean.clone();
        suspect.integrity =
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::CrcSuspect], false);
        assert_eq!(reason_fidelity_tier(IntegrityReason::CrcSuspect), 3);
        assert_eq!(fidelity_rank_with_mode(&suspect, FidelityMode::Graded), 3);
        assert_eq!(fidelity_rank_with_mode(&suspect, FidelityMode::Binary), 1);
        assert!(
            fidelity_rank_with_mode(&clean, FidelityMode::Graded)
                < fidelity_rank_with_mode(&suspect, FidelityMode::Graded)
        );
        assert!(
            fidelity_rank_with_mode(&clean, FidelityMode::Binary)
                < fidelity_rank_with_mode(&suspect, FidelityMode::Binary)
        );
    }

    /// 0077 DoD-20: CRC_SUSPECT is Tier-2 ineligible by default.
    #[test]
    fn crc_suspect_tier2_ineligible_by_default() {
        let mut suspect = item(
            "C:/a.pst", "a.pst", "Inbox", 1, None, [7; 32], 100, 0, false,
        );
        suspect.integrity =
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::CrcSuspect], false);
        assert_eq!(
            suspect.assess_tier2_eligibility(),
            Err(Tier2IneligibleReason::CrcSuspect)
        );
        let outcome = group_candidates_with_stats(&[suspect], &GroupingContext::default());
        assert_eq!(outcome.stats.tier2_blocked_crc_suspect, 1);
        assert!(!outcome.tier2_eligible[0]);
    }

    /// 0077 DoD-20: MID Tier-1 still merges suspect + clean twins with same Message-ID.
    #[test]
    fn crc_suspect_mid_still_groups_with_clean_twin() {
        let mid = Some("same-mid@ex.com");
        let clean = item(
            "C:/clean.pst",
            "clean.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        let mut suspect = item(
            "C:/bad.pst",
            "bad.pst",
            "Inbox",
            2,
            mid,
            [2; 32],
            100,
            1,
            false,
        );
        suspect.integrity =
            RecoverableIntegrity::with_degraded(vec![IntegrityReason::CrcSuspect], false);
        let (ks, dec) = build_keep_set(
            vec![clean, suspect],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(
            ks.stats.unique, 1,
            "MID must bind suspect+clean into one group"
        );
        assert_eq!(ks.stats.duplicates, 1);
        assert_eq!(ks.stats.tier1_dups, 1);
        let dup = dec
            .iter()
            .find(|d| d.role == DecisionRole::DupOf)
            .expect("dup");
        assert_eq!(dup.tier.as_deref(), Some("message_id"));
    }

    /// 0077 DoD-20: `--allow-crc-suspect-tier2` restores Tier-2 eligibility exactly.
    #[test]
    fn allow_crc_suspect_tier2_restores_eligibility() {
        let h = [99u8; 32];
        let mut a = item("C:/a.pst", "a.pst", "Inbox", 1, None, h, 50, 0, false);
        let mut b = item("C:/b.pst", "b.pst", "Inbox", 2, None, h, 50, 1, false);
        a.integrity = RecoverableIntegrity::with_degraded(vec![IntegrityReason::CrcSuspect], false);
        b.integrity = RecoverableIntegrity::with_degraded(vec![IntegrityReason::CrcSuspect], false);

        let blocked =
            group_candidates_with_stats(&[a.clone(), b.clone()], &GroupingContext::default());
        assert_eq!(blocked.stats.tier2_blocked_crc_suspect, 2);
        // No Tier-2 merge: each seeds its own group (two uniques).
        assert_eq!(blocked.groups.len(), 2);

        let allow = GroupingContext {
            allow_crc_suspect_tier2: true,
            ..GroupingContext::default()
        };
        let allowed = group_candidates_with_stats(&[a, b], &allow);
        assert_eq!(allowed.stats.tier2_blocked_crc_suspect, 0);
        assert_eq!(
            allowed.groups.len(),
            1,
            "allow flag must restore pre-0077 Tier-2 merge on shared content hash"
        );
        assert!(allowed.tier2_eligible.iter().all(|&e| e));
    }

    #[test]
    fn duplicate_source_aggregate_cap_and_basename() {
        let mid = Some("dup@x");
        let mut items = vec![item(
            r"C:\mail\winner.pst",
            "winner.pst",
            "Inbox",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        )];
        for i in 0..10u64 {
            items.push(item(
                &format!(r"C:\mail\cust{i}.pst"),
                &format!("cust{i}.pst"),
                "Inbox",
                10 + i,
                mid,
                [1; 32],
                100,
                i + 1,
                false,
            ));
        }
        let (ks, dec) = build_keep_set(
            items,
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        let w = &ks.winners[0];
        assert_eq!(w.duplicate_source_count, 10);
        assert_eq!(w.duplicate_sources.len(), DUPLICATE_SOURCES_CAP);
        assert!(w.duplicate_sources_truncated);
        assert!(w
            .duplicate_sources
            .iter()
            .all(|s| !s.contains('\\') && !s.contains('/')));
        let row = dec
            .iter()
            .find(|d| d.role == DecisionRole::Unique)
            .expect("u");
        assert_eq!(row.duplicate_source_count, 10);
        assert_eq!(
            row.duplicate_sources.split('|').count(),
            DUPLICATE_SOURCES_CAP
        );
        // Three-surface All-Custodians parity:
        // 1) KeepEntry JSON fields
        // 2) DecisionRecord unique-row CSV fields
        // 3) export_messages fill pattern used by unique_pst_cmd
        //    (duplicate_source_count + join("|") on keep winner basenames)
        assert_eq!(w.duplicate_source_count, row.duplicate_source_count);
        assert_eq!(w.duplicate_sources.join("|"), row.duplicate_sources);
        let export_dup_count = w.duplicate_source_count;
        let export_dup_sources = w.duplicate_sources.join("|");
        assert_eq!(export_dup_count, row.duplicate_source_count);
        assert_eq!(export_dup_sources, row.duplicate_sources);
        assert_eq!(export_dup_sources.split('|').count(), DUPLICATE_SOURCES_CAP);
        assert!(w.duplicate_sources_truncated);
        // Basename-only: no absolute path separators in any surface string.
        assert!(!export_dup_sources.contains('\\') && !export_dup_sources.contains('/'));
        assert!(!row.duplicate_sources.contains('\\') && !row.duplicate_sources.contains('/'));
    }

    #[test]
    fn winners_from_recoverable_signal_only() {
        let mid = Some("ri@x");
        let purge = item(
            "C:/a.pst",
            "a.pst",
            "Top/Recoverable Items/Purges",
            1,
            mid,
            [1; 32],
            100,
            0,
            false,
        );
        // Sole member in recoverable — still wins; stat fires; hint non-empty.
        let (ks, _) = build_keep_set(
            vec![purge.clone()],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.stats.winners_from_recoverable_items, 1);
        assert!(recoverable_items_hint(ks.stats.winners_from_recoverable_items).is_some());
        assert_eq!(ks.winners[0].locus.nid, 1);

        // With ladder on, same sole member still wins (nothing to demote against).
        let mut ctx = RankContext::new(KeepPolicy::FirstSeen);
        ctx.folder_rank = FolderRankMode::Builtin;
        let (ks2, _) = build_keep_set_with_ctx(vec![purge], FamilyPolicy::default(), &ctx, true)
            .expect("build");
        assert_eq!(ks2.winners[0].locus.nid, 1);
        assert_eq!(ks2.stats.winners_from_recoverable_items, 1);
    }

    #[test]
    fn shuffled_input_identical_winners() {
        let mid = Some("sh@x");
        let a = item(
            "C:/a.pst", "a.pst", "Inbox", 10, mid, [1; 32], 100, 0, false,
        );
        let b = item(
            "C:/b.pst", "b.pst", "Inbox", 20, mid, [1; 32], 200, 1, false,
        );
        let c = item("C:/c.pst", "c.pst", "Inbox", 30, mid, [2; 32], 50, 2, false); // different hash
        let (ks1, _) = build_keep_set(
            vec![a.clone(), b.clone(), c.clone()],
            KeepPolicy::KeepLargest,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b1");
        let (ks2, _) = build_keep_set(
            vec![c, b, a],
            KeepPolicy::KeepLargest,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("b2");
        let nids1: Vec<_> = ks1.winners.iter().map(|w| w.locus.nid).collect();
        let nids2: Vec<_> = ks2.winners.iter().map(|w| w.locus.nid).collect();
        assert_eq!(nids1, nids2);
    }

    #[test]
    fn pre_0075_keep_set_json_deserializes() {
        // Minimal keep_set_v1 without 0075 winner fields.
        let json = r#"{
            "schema": "keep_set_v1",
            "policy": "first_seen",
            "family_policy": "keep_attachments_with_parent",
            "winners": [{
                "locus": {
                    "source_path": "C:/a.pst",
                    "source_pst": "a.pst",
                    "folder_path": "Inbox",
                    "nid": 1,
                    "is_orphaned": false
                },
                "message_id_norm": "m@x",
                "content_hash": "0101010101010101010101010101010101010101010101010101010101010101",
                "edrm_mih_hex": null,
                "integrity": {
                    "degraded": false,
                    "degraded_reasons": [],
                    "is_orphaned": false
                },
                "size": 10
            }],
            "stats": {
                "recoverable": 1,
                "unique": 1,
                "duplicates": 0,
                "tier1_dups": 0,
                "tier2_dups": 0,
                "degraded_winners": 0,
                "materialize_failed": 0,
                "promoted_from_failure": 0,
                "groups_dropped_materialize": 0,
                "groups": 1
            }
        }"#;
        let ks: KeepSet = serde_json::from_str(json).expect("deserialize pre-0075");
        assert_eq!(ks.schema, KEEP_SET_SCHEMA);
        assert_eq!(ks.winners.len(), 1);
        assert!(ks.winners[0].folder_class.is_none());
        assert_eq!(ks.stats.winners_from_recoverable_items, 0);
    }

    #[test]
    fn default_flags_golden_first_seen_winners() {
        // Golden: two same-MID, first_seen → lower scan_order / path wins.
        let mid = Some("golden@x");
        let a = item("C:/a.pst", "a.pst", "Inbox", 1, mid, [1; 32], 100, 0, false);
        let b = item("C:/b.pst", "b.pst", "Inbox", 2, mid, [1; 32], 100, 1, false);
        let (ks, dec) = build_keep_set(
            vec![a, b],
            KeepPolicy::FirstSeen,
            FamilyPolicy::default(),
            &[],
            true,
        )
        .expect("build");
        assert_eq!(ks.winners.len(), 1);
        assert_eq!(ks.winners[0].locus.nid, 1);
        assert_eq!(ks.winners[0].locus.source_pst, "a.pst");
        // Pre-0075 role/tier columns still meaningful.
        assert_eq!(
            dec.iter()
                .filter(|d| d.role == DecisionRole::Unique)
                .count(),
            1
        );
        assert_eq!(
            dec.iter()
                .find(|d| d.role == DecisionRole::DupOf)
                .map(|d| d.tier.as_deref()),
            Some(Some("message_id"))
        );
    }

    // ─── 0076 identity binding ─────────────────────────────────────────────

    #[test]
    fn cross_mid_same_hash_splits_by_default() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("m1"),
            [9; 32],
            10,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("m2"),
            [9; 32],
            10,
            1,
            false,
        );
        let out = group_candidates_with_stats(&[a, b], &GroupingContext::default());
        assert_eq!(out.groups.len(), 2);
        assert_eq!(out.stats.cross_mid_blocked, 1);
    }

    #[test]
    fn cross_mid_allowed_with_escape() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("m1"),
            [9; 32],
            10,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("m2"),
            [9; 32],
            10,
            1,
            false,
        );
        let ctx = GroupingContext {
            allow_cross_mid_tier2: true,
            ..Default::default()
        };
        let out = group_candidates_with_stats(&[a, b], &ctx);
        assert_eq!(out.groups.len(), 1);
    }

    #[test]
    fn unreadable_body_not_tier2_bound() {
        let mut a = item("C:/a.pst", "a.pst", "I", 1, None, [3; 32], 10, 0, true);
        a.has_body_preview = false;
        let mut b = item("C:/b.pst", "b.pst", "I", 2, None, [3; 32], 10, 1, true);
        b.has_body_preview = false;
        let out = group_candidates_with_stats(&[a, b], &GroupingContext::default());
        assert_eq!(out.groups.len(), 2);
        assert!(out.stats.tier2_blocked_unreadable_body >= 1);
    }

    #[test]
    fn degenerate_stays_unique() {
        let mut a = item("C:/a.pst", "a.pst", "I", 1, None, [4; 32], 10, 0, false);
        a.has_body_preview = false;
        a.subject_nonempty = true;
        a.sender_nonempty = false;
        a.submit_time = None;
        a.attach_count = 0;
        let mut b = item("C:/b.pst", "b.pst", "I", 2, None, [4; 32], 10, 1, false);
        b.has_body_preview = false;
        b.subject_nonempty = true;
        b.sender_nonempty = false;
        b.submit_time = None;
        b.attach_count = 0;
        let out = group_candidates_with_stats(&[a, b], &GroupingContext::default());
        assert_eq!(out.groups.len(), 2);
        assert!(out.stats.tier2_blocked_degenerate >= 1);
    }

    #[test]
    fn bound_by_recorded_not_guessed() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("mid@x"),
            [1; 32],
            10,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("mid@x"),
            [2; 32],
            10,
            1,
            false,
        );
        let out = group_candidates_with_stats(&[a, b], &GroupingContext::default());
        assert_eq!(out.bound_by[0], BoundBy::Seed);
        assert_eq!(out.bound_by[1], BoundBy::MessageId);
    }

    #[test]
    fn per_source_scope_splits_same_hash() {
        let a = item("C:/a.pst", "a.pst", "I", 1, None, [7; 32], 10, 0, false);
        let b = item("C:/b.pst", "b.pst", "I", 2, None, [7; 32], 10, 1, false);
        let ctx = GroupingContext {
            scope: DedupeScope::PerSource,
            ..Default::default()
        };
        let out = group_candidates_with_stats(&[a, b], &ctx);
        assert_eq!(out.groups.len(), 2);
        let global = group_candidates_with_stats(
            &[
                item("C:/a.pst", "a.pst", "I", 1, None, [7; 32], 10, 0, false),
                item("C:/b.pst", "b.pst", "I", 2, None, [7; 32], 10, 1, false),
            ],
            &GroupingContext::default(),
        );
        assert_eq!(global.groups.len(), 1);
    }

    #[test]
    fn decision_csv_has_0076_columns() {
        assert!(DECISION_CSV_HEADER.contains(&"bound_by"));
        assert!(DECISION_CSV_HEADER.contains(&"identity_version"));
        assert!(DECISION_CSV_HEADER.contains(&"tier2_eligible"));
        assert_eq!(DECISION_CSV_HEADER.len(), 31);
    }

    #[test]
    fn pre_0076_allow_flags_merge_cross_mid() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("m1"),
            [9; 32],
            10,
            0,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("m2"),
            [9; 32],
            10,
            1,
            false,
        );
        let pre = GroupingContext::pre_0076();
        let out = group_candidates_with_stats(&[a, b], &pre);
        assert_eq!(out.groups.len(), 1);
    }

    /// D6 residual: item A (no mid, hash H) stays alone when B joins C's MID group
    /// via Tier 1 even though B shares H with A. Backfill merges when flagged.
    #[test]
    fn tier1_backfill_off_split_on_merge_d6() {
        let a = item("C:/a.pst", "a.pst", "I", 1, None, [0xAB; 32], 10, 0, false);
        let c = item(
            "C:/c.pst",
            "c.pst",
            "I",
            2,
            Some("shared@mid"),
            [0xCD; 32],
            10,
            1,
            false,
        );
        let b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            3,
            Some("shared@mid"),
            [0xAB; 32],
            10,
            2,
            false,
        );
        let items = [a.clone(), c.clone(), b.clone()];

        let off = GroupingContext::default();
        let out_off = group_candidates_with_stats(&items, &off);
        assert_eq!(
            out_off.groups.len(),
            2,
            "default must leave D6 residual split"
        );
        assert!(
            out_off.stats.tier1_backfill_candidates >= 1,
            "must always count candidates; got {}",
            out_off.stats.tier1_backfill_candidates
        );

        let on = GroupingContext {
            tier1_backfill: true,
            ..Default::default()
        };
        let out_on = group_candidates_with_stats(&items, &on);
        assert_eq!(out_on.groups.len(), 1, "backfill must merge D6 residual");
        assert!(
            out_on.stats.tier1_backfill_candidates >= 1,
            "candidates still reported when merging"
        );
        // Absorbed former seed must not remain BoundBy::Seed.
        let seed = out_on.groups[0][0];
        for (i, bb) in out_on.bound_by.iter().enumerate() {
            if i == seed {
                assert_eq!(*bb, BoundBy::Seed);
            } else {
                assert_ne!(
                    *bb,
                    BoundBy::Seed,
                    "member {i} must reclassify after backfill merge"
                );
            }
        }

        // Per-source: same residual must NOT cross-merge custodians.
        let per = GroupingContext {
            tier1_backfill: true,
            scope: DedupeScope::PerSource,
            ..Default::default()
        };
        let out_per = group_candidates_with_stats(&items, &per);
        assert!(
            out_per.groups.len() >= 2,
            "per-source backfill must not unite across sources; got {} groups",
            out_per.groups.len()
        );
        assert_eq!(
            out_per.stats.tier1_backfill_candidates, 0,
            "cross-source pairs are not backfill candidates under per-source"
        );
    }

    #[test]
    fn allow_degenerate_tier2_restores_pre_0076_bind() {
        let mut a = item("C:/a.pst", "a.pst", "I", 1, None, [4; 32], 10, 0, false);
        a.has_body_preview = false;
        a.subject_nonempty = true;
        a.sender_nonempty = false;
        a.submit_time = None;
        a.attach_count = 0;
        let mut b = item("C:/b.pst", "b.pst", "I", 2, None, [4; 32], 10, 1, false);
        b.has_body_preview = false;
        b.subject_nonempty = true;
        b.sender_nonempty = false;
        b.submit_time = None;
        b.attach_count = 0;

        let blocked =
            group_candidates_with_stats(&[a.clone(), b.clone()], &GroupingContext::default());
        assert_eq!(blocked.groups.len(), 2);

        let allowed = GroupingContext {
            allow_degenerate_tier2: true,
            require_readable_body: false,
            ..Default::default()
        };
        let restored = group_candidates_with_stats(&[a, b], &allowed);
        assert_eq!(
            restored.groups.len(),
            1,
            "allow-degenerate restores Tier-2 bind"
        );
    }

    #[test]
    fn tier1_verify_content_splits_divergent_mid_group() {
        let a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("same@mid"),
            [1; 32],
            10,
            0,
            false,
        );
        let mut b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("same@mid"),
            [2; 32],
            10,
            1,
            false,
        );
        b.fp_body = 99;
        let off = group_candidates_with_stats(&[a.clone(), b.clone()], &GroupingContext::default());
        assert_eq!(off.groups.len(), 1, "verify off keeps MID group");
        assert!(off.stats.tier1_divergent_body >= 1 || off.stats.tier1_divergent_metadata >= 1);

        let on = GroupingContext {
            tier1_verify: crate::grouping::Tier1Verify::Content,
            ..Default::default()
        };
        let split = group_candidates_with_stats(&[a, b], &on);
        assert_eq!(split.groups.len(), 2, "tier1-verify content splits");
        // §3.7: divergence still reported when verification splits.
        assert!(
            split.stats.tier1_divergent_body >= 1 || split.stats.tier1_divergent_metadata >= 1,
            "divergence must be counted even when tier1-verify splits"
        );
    }

    #[test]
    fn component_attribution_body_vs_metadata() {
        let mut a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            1,
            Some("m@x"),
            [1; 32],
            10,
            0,
            false,
        );
        a.fp_body = 1;
        a.fp_header = 1;
        a.fp_attachments = 1;
        let mut b_meta = item(
            "C:/b.pst",
            "b.pst",
            "I",
            2,
            Some("m@x"),
            [2; 32],
            10,
            1,
            false,
        );
        b_meta.fp_body = 1;
        b_meta.fp_header = 99;
        b_meta.fp_attachments = 1;
        let out_meta =
            group_candidates_with_stats(&[a.clone(), b_meta], &GroupingContext::default());
        assert!(out_meta.stats.tier1_divergent_metadata >= 1);
        assert_eq!(out_meta.stats.tier1_divergent_body, 0);

        let mut b_body = item(
            "C:/c.pst",
            "c.pst",
            "I",
            3,
            Some("m@x"),
            [3; 32],
            10,
            1,
            false,
        );
        b_body.fp_body = 77;
        b_body.fp_header = 1;
        b_body.fp_attachments = 1;
        let out_body = group_candidates_with_stats(&[a, b_body], &GroupingContext::default());
        assert!(out_body.stats.tier1_divergent_body >= 1);
        assert_eq!(out_body.stats.tier1_divergent_metadata, 0);
    }

    #[test]
    fn refinement_default_and_split_flags_are_subsets_of_pre_0076() {
        // Multi-source synthetic covering cross-MID, degenerate, and clean dups.
        let clean_a = item("C:/a.pst", "a.pst", "I", 1, None, [10; 32], 10, 0, false);
        let clean_b = item("C:/b.pst", "b.pst", "I", 2, None, [10; 32], 10, 1, false);
        let cross_a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            3,
            Some("m1"),
            [20; 32],
            10,
            2,
            false,
        );
        let cross_b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            4,
            Some("m2"),
            [20; 32],
            10,
            3,
            false,
        );
        let mut deg_a = item("C:/a.pst", "a.pst", "I", 5, None, [30; 32], 10, 4, false);
        deg_a.has_body_preview = false;
        deg_a.subject_nonempty = true;
        deg_a.sender_nonempty = false;
        deg_a.submit_time = None;
        deg_a.attach_count = 0;
        let mut deg_b = item("C:/b.pst", "b.pst", "I", 6, None, [30; 32], 10, 5, false);
        deg_b.has_body_preview = false;
        deg_b.subject_nonempty = true;
        deg_b.sender_nonempty = false;
        deg_b.submit_time = None;
        deg_b.attach_count = 0;
        let mid_a = item(
            "C:/a.pst",
            "a.pst",
            "I",
            7,
            Some("same"),
            [40; 32],
            10,
            6,
            false,
        );
        let mid_b = item(
            "C:/b.pst",
            "b.pst",
            "I",
            8,
            Some("same"),
            [41; 32],
            10,
            7,
            false,
        );
        let items = [
            clean_a, clean_b, cross_a, cross_b, deg_a, deg_b, mid_a, mid_b,
        ];

        let baseline = group_candidates_with_stats(&items, &GroupingContext::pre_0076());
        let contexts = [
            GroupingContext::default(),
            GroupingContext {
                tier1_authority: true,
                ..GroupingContext::pre_0076()
            },
            GroupingContext {
                require_readable_body: true,
                allow_degenerate_tier2: false,
                ..GroupingContext::pre_0076()
            },
            GroupingContext {
                scope: DedupeScope::PerSource,
                ..Default::default()
            },
        ];
        for ctx in &contexts {
            let out = group_candidates_with_stats(&items, ctx);
            assert_refinement(&baseline.groups, &out.groups, items.len());
        }
    }

    /// Every group in `refined` is a subset of some group in `baseline`.
    fn assert_refinement(baseline: &[Vec<usize>], refined: &[Vec<usize>], n: usize) {
        let mut base_of = vec![usize::MAX; n];
        for (gid, members) in baseline.iter().enumerate() {
            for &i in members {
                base_of[i] = gid;
            }
        }
        for g in refined {
            if g.is_empty() {
                continue;
            }
            let parent = base_of[g[0]];
            for &i in g {
                assert_eq!(
                    base_of[i], parent,
                    "refined group {g:?} crosses baseline groups"
                );
            }
        }
    }

    #[test]
    fn index_group_candidates_equivalence_across_contexts() {
        use crate::index::{DedupIndex, IndexItem, MessageRef};

        let mk =
            |path: &str, pst: &str, nid: u64, mid: Option<&str>, hash: [u8; 32], order: u64| {
                let mut it = item(path, pst, "I", nid, mid, hash, 10, order, false);
                it.has_body_preview = true;
                it
            };
        let base_items = vec![
            mk("C:/a.pst", "a.pst", 1, Some("m1"), [1; 32], 0),
            mk("C:/b.pst", "b.pst", 2, Some("m1"), [9; 32], 1),
            mk("C:/c.pst", "c.pst", 3, None, [2; 32], 2),
            mk("C:/d.pst", "d.pst", 4, None, [2; 32], 3),
            mk("C:/e.pst", "e.pst", 5, Some("m2"), [2; 32], 4),
            mk("C:/f.pst", "f.pst", 6, Some("m3"), [3; 32], 5),
        ];

        // Note: `tier1_backfill: true` is intentionally omitted. Backfill merge is a
        // keep-set post-pass only; streaming DedupIndex cannot retro-merge, so seed
        // equivalence does not hold under that flag (CLI rejects it on scan/dups).
        let contexts = [
            GroupingContext::default(),
            GroupingContext::pre_0076(),
            GroupingContext {
                tier2_enabled: false,
                ..Default::default()
            },
            GroupingContext {
                scope: DedupeScope::PerSource,
                ..Default::default()
            },
            GroupingContext {
                allow_cross_mid_tier2: true,
                tier1_authority: false,
                ..Default::default()
            },
        ];

        for ctx in &contexts {
            // Several scan-order shuffles of equal-key stability isn't full perm;
            // permute by rotating the list.
            for rot in 0..base_items.len() {
                let mut items = base_items.clone();
                items.rotate_left(rot);
                // Reassign scan_order to match position.
                for (i, it) in items.iter_mut().enumerate() {
                    it.scan_order = i as u64;
                }

                let outcome = group_candidates_with_stats(&items, ctx);
                let mut index = DedupIndex::with_context(ctx.clone());
                let mut index_seeds = Vec::new();
                let mut index_bound = Vec::new();
                for it in &items {
                    let result = index.check_and_insert_item(IndexItem {
                        message_id: it.message_id_norm.clone(),
                        content_hash: it.content_hash,
                        strong_content_hash: it.strong_content_hash,
                        tier2_eligible: it.assess_tier2_eligibility().is_ok()
                            || !ctx.enforce_readable_body(),
                        source_key: it.path_key(),
                        fp_body: it.fp_body,
                        fp_header: it.fp_header,
                        fp_recipients: it.fp_recipients,
                        fp_attachments: it.fp_attachments,
                        msg_ref: MessageRef {
                            pst_index: 0,
                            pst_name: it.locus.source_pst.clone(),
                            folder_path: it.locus.folder_path.clone(),
                            nid: it.locus.nid,
                            subject: String::new(),
                            submit_time: it.submit_time,
                            sender: String::new(),
                            size: it.size,
                        },
                    });
                    index_bound.push(result.bound_by());
                    if result.is_unique() {
                        index_seeds.push(it.locus.nid);
                    }
                }

                let mut group_seeds: Vec<u64> = outcome
                    .groups
                    .iter()
                    .filter_map(|g| g.first().map(|&i| items[i].locus.nid))
                    .collect();
                group_seeds.sort_unstable();
                let mut idx_seeds = index_seeds.clone();
                idx_seeds.sort_unstable();
                assert_eq!(
                    group_seeds, idx_seeds,
                    "seed nids disagree for ctx={ctx:?} rot={rot}"
                );

                // BoundBy for non-seeds: index reports on insert; group_candidates on member.
                for (i, bb) in outcome.bound_by.iter().enumerate() {
                    // Seeds are BoundBy::Seed on both when unique first-seen.
                    if *bb == BoundBy::Seed {
                        assert_eq!(
                            index_bound[i],
                            BoundBy::Seed,
                            "item {i} seed mismatch rot={rot}"
                        );
                    } else {
                        assert_ne!(index_bound[i], BoundBy::Seed, "item {i} should be dup");
                        assert_eq!(
                            index_bound[i], *bb,
                            "BoundBy mismatch item {i} rot={rot} ctx={ctx:?}"
                        );
                    }
                }
            }
        }
    }
}
