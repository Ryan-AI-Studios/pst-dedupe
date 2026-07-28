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

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::index::DedupTier;
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
    /// Tier 2 content hash (always computed).
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
    /// Empty when unique / materialize_failed; `message_id` | `content_hash` when dup_of.
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
}

// ─── Materialization ────────────────────────────────────────────────────────

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
}

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

/// Group candidates using the same Tier1/Tier2 binding rules as [`crate::DedupIndex`],
/// but collecting **all** members per group instead of first-seen only.
///
/// Returns groups of indices into `items` (scan order preserved within groups).
pub fn group_candidates(items: &[RecoverableScanItem], tier2_enabled: bool) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut mid_to_group: HashMap<String, usize> = HashMap::new();
    let mut hash_to_group: HashMap<[u8; 32], usize> = HashMap::new();

    for (i, item) in items.iter().enumerate() {
        let mut found: Option<usize> = None;

        if let Some(mid) = item.message_id_norm.as_deref() {
            if !mid.is_empty() {
                if let Some(&gid) = mid_to_group.get(mid) {
                    found = Some(gid);
                }
            }
        }

        if found.is_none() && tier2_enabled {
            if let Some(&gid) = hash_to_group.get(&item.content_hash) {
                found = Some(gid);
            }
        }

        if let Some(gid) = found {
            groups[gid].push(i);
        } else {
            let gid = groups.len();
            groups.push(vec![i]);
            if let Some(mid) = item.message_id_norm.as_deref() {
                if !mid.is_empty() {
                    mid_to_group.insert(mid.to_string(), gid);
                }
            }
            if tier2_enabled {
                hash_to_group.insert(item.content_hash, gid);
            }
        }
    }

    groups
}

/// Determine the tier that bound a member to its group's seed (for decision CSV).
fn member_tier(
    items: &[RecoverableScanItem],
    seed_idx: usize,
    member_idx: usize,
    tier2_enabled: bool,
) -> Option<DedupTier> {
    if member_idx == seed_idx {
        return None;
    }
    let seed = &items[seed_idx];
    let member = &items[member_idx];

    // Prefer Message-ID when both share the same non-empty MID.
    if let (Some(a), Some(b)) = (
        seed.message_id_norm.as_deref(),
        member.message_id_norm.as_deref(),
    ) {
        if !a.is_empty() && a == b {
            return Some(DedupTier::MessageId);
        }
    }
    // Also: member matched via MID to a group that was seeded with that MID even if
    // seed mid equals member mid already handled. If member has MID matching seed's MID.
    if let Some(mid) = member.message_id_norm.as_deref() {
        if !mid.is_empty() {
            if let Some(seed_mid) = seed.message_id_norm.as_deref() {
                if seed_mid == mid {
                    return Some(DedupTier::MessageId);
                }
            }
        }
    }

    if tier2_enabled && member.content_hash == seed.content_hash {
        return Some(DedupTier::ContentHash);
    }

    // Member may have joined via content hash to a seed that also has MID:
    // content hashes equal → content_hash tier (cross-tier acceptable).
    if tier2_enabled {
        // Walk: if member has no MID (or empty) and hashes match any — content hash.
        let member_mid_empty = member
            .message_id_norm
            .as_deref()
            .map(|m| m.is_empty())
            .unwrap_or(true);
        if member_mid_empty && member.content_hash == seed.content_hash {
            return Some(DedupTier::ContentHash);
        }
        // Hashes equal under tier2 path.
        if member.content_hash == seed.content_hash {
            return Some(DedupTier::ContentHash);
        }
    }

    // Fallback: treat as content_hash when in same group (should be rare).
    Some(DedupTier::ContentHash)
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
        | AttachMethodUnsupported => 2,
        // tier 3 — body / data loss
        BodyTruncated | BodyUnavailable | DataTruncated | CrcMismatch | BlockNotFound => 3,
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
    /// Per-item promoted_from_failure flag.
    pub promoted_from_failure: Vec<bool>,
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
                        Some("content_hash") => stats.tier2_dups += 1,
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

        KeepSet {
            schema: KEEP_SET_SCHEMA.to_string(),
            policy: self.policy,
            family_policy: self.family_policy,
            created_from: self.created_from.clone(),
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
        if self.promoted_from_failure[i] && self.roles[i] == DecisionRole::Unique {
            return "promoted_after_materialize_fail";
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
                if self.promoted_from_failure.get(i).copied().unwrap_or(false) {
                    "promoted_after_materialize_fail"
                } else {
                    "path_order"
                }
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
pub fn resolve_groups_with_ctx(
    items: Vec<RecoverableScanItem>,
    family_policy: FamilyPolicy,
    rank_ctx: &RankContext,
    tier2_enabled: bool,
    created_from: Option<KeepSetProvenance>,
) -> ResolvedKeepSet {
    let groups = group_candidates(&items, tier2_enabled);
    let n = items.len();
    let mut roles = vec![DecisionRole::Unique; n];
    let mut winner_of: Vec<Option<usize>> = vec![None; n];
    let mut tier_of: Vec<Option<String>> = vec![None; n];
    let promoted_from_failure = vec![false; n];
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

        // Seed for tier labeling = first by scan order in the group (group binding seed).
        let seed = *group
            .iter()
            .min_by_key(|&&idx| items[idx].scan_order)
            .unwrap_or(&winner);

        for &idx in group {
            if idx == winner {
                roles[idx] = DecisionRole::Unique;
                winner_of[idx] = Some(winner);
                tier_of[idx] = None;
            } else {
                roles[idx] = DecisionRole::DupOf;
                winner_of[idx] = Some(winner);
                let tier = member_tier(&items, seed, idx, tier2_enabled);
                tier_of[idx] = match tier {
                    Some(DedupTier::MessageId) => Some("message_id".into()),
                    Some(DedupTier::ContentHash) => Some("content_hash".into()),
                    None => None,
                };
            }
        }
    }

    ResolvedKeepSet {
        policy: rank_ctx.policy,
        family_policy,
        prefer_path: rank_ctx.prefer_path.clone(),
        rank_ctx: rank_ctx.clone(),
        tier2_enabled,
        items,
        groups,
        provisional_winners,
        roles,
        winner_of,
        tier_of,
        promoted_from_failure,
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
    let mut resolved = resolve_groups_with_ctx(
        items,
        opts.family_policy,
        ctx_ref,
        opts.tier2_enabled,
        opts.created_from,
    );
    let count = finalize_with_materialize(&mut resolved, materializer, &mut on_winner)?;
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
    let mut materialized_count = 0u64;
    let rank_ctx = resolved.rank_ctx.clone();
    let tier2 = resolved.tier2_enabled;

    for (g_idx, group) in resolved.groups.clone().into_iter().enumerate() {
        if group.is_empty() {
            continue;
        }

        // Rank full group once.
        let mut ranked = group.clone();
        ranked.sort_by(|&a, &b| {
            rank_key(&resolved.items[a], &rank_ctx).cmp(&rank_key(&resolved.items[b], &rank_ctx))
        });

        let mut final_winner: Option<usize> = None;
        let mut failed: Vec<usize> = Vec::new();
        let mut promoted = false;

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

                    if attempt > 0 {
                        promoted = true;
                    }
                    final_winner = Some(idx);
                    on_winner(msg)?;
                    materialized_count += 1;
                    break;
                }
                Err(MaterializeError::Hard(_)) => {
                    failed.push(idx);
                }
            }
        }

        // Seed for tier labels.
        let seed = *group
            .iter()
            .min_by_key(|&&idx| resolved.items[idx].scan_order)
            .unwrap_or(&ranked[0]);

        if let Some(winner) = final_winner {
            resolved.group_dropped[g_idx] = false;
            for &idx in &group {
                if failed.contains(&idx) {
                    resolved.roles[idx] = DecisionRole::MaterializeFailed;
                    resolved.winner_of[idx] = Some(winner);
                    resolved.tier_of[idx] = None;
                    resolved.promoted_from_failure[idx] = false;
                } else if idx == winner {
                    resolved.roles[idx] = DecisionRole::Unique;
                    resolved.winner_of[idx] = Some(winner);
                    resolved.tier_of[idx] = None;
                    resolved.promoted_from_failure[idx] = promoted;
                } else {
                    resolved.roles[idx] = DecisionRole::DupOf;
                    resolved.winner_of[idx] = Some(winner);
                    let tier = member_tier(&resolved.items, seed, idx, tier2);
                    resolved.tier_of[idx] = match tier {
                        Some(DedupTier::MessageId) => Some("message_id".into()),
                        Some(DedupTier::ContentHash) => Some("content_hash".into()),
                        None => None,
                    };
                    resolved.promoted_from_failure[idx] = false;
                }
            }
        } else {
            // All failed — zero exportable winners.
            resolved.group_dropped[g_idx] = true;
            for &idx in &group {
                resolved.roles[idx] = DecisionRole::MaterializeFailed;
                resolved.winner_of[idx] = None;
                resolved.tier_of[idx] = None;
                resolved.promoted_from_failure[idx] = false;
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

/// Full decision CSV header (pre-0075 + 0075 append columns).
pub const DECISION_CSV_HEADER: [&str; 28] = [
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
        assert_eq!(DECISION_CSV_HEADER.len(), 28);
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
        ];
        for r in attach {
            assert_eq!(reason_fidelity_tier(r), 2);
        }
        let body = [
            BodyTruncated,
            BodyUnavailable,
            DataTruncated,
            CrcMismatch,
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
}
