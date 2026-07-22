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

/// Winner selection policy (applied after fidelity preference).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeepPolicy {
    /// Earliest by deterministic scan order among remaining candidates.
    #[default]
    FirstSeen,
    /// Prefer largest `message_size` (0/missing last).
    KeepLargest,
    /// Prefer sources whose path/folder matches prefer-path patterns.
    PreferPath,
}

impl KeepPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstSeen => "first_seen",
            Self::KeepLargest => "keep_largest",
            Self::PreferPath => "prefer_path",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "first_seen" => Some(Self::FirstSeen),
            "keep_largest" => Some(Self::KeepLargest),
            "prefer_path" => Some(Self::PreferPath),
            _ => None,
        }
    }
}

impl fmt::Display for KeepPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
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

/// Fidelity rank: lower is better. P0 minimum — non-degraded beats degraded.
///
/// 0 = clean (not degraded, not orphaned)
/// 1 = degraded / orphaned
pub fn fidelity_rank(item: &RecoverableScanItem) -> u8 {
    if item.integrity.degraded || item.integrity.is_orphaned {
        1
    } else {
        0
    }
}

/// Ranking key: lower is better winner.
/// `(fidelity, policy_key, path_key, nid)`
pub fn rank_key(
    item: &RecoverableScanItem,
    policy: KeepPolicy,
    prefer_path: &[String],
) -> (u8, i64, String, u64) {
    let fid = fidelity_rank(item);
    let policy_key = match policy {
        KeepPolicy::FirstSeen => item.scan_order as i64,
        KeepPolicy::KeepLargest => {
            // Larger size better → negate. Size 0/missing ranks last among sizes.
            -(item.size as i64)
        }
        KeepPolicy::PreferPath => {
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
    };
    (fid, policy_key, item.path_key(), item.locus.nid)
}

/// Provisional resolve state before materialize promotion.
#[derive(Clone, Debug)]
pub struct ResolvedKeepSet {
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path: Vec<String>,
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
                    winners.push(KeepEntry {
                        locus: item.locus.clone(),
                        message_id_norm: item.message_id_norm.clone(),
                        content_hash: item.content_hash,
                        edrm_mih_hex: item.edrm_mih_hex(),
                        integrity: item.integrity.clone(),
                        size: item.size,
                        promoted_from_failure: self.promoted_from_failure[i],
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

/// Resolve provisional winners: fidelity → policy → deterministic order.
pub fn resolve_groups(
    items: Vec<RecoverableScanItem>,
    policy: KeepPolicy,
    family_policy: FamilyPolicy,
    prefer_path: &[String],
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
        ranked.sort_by(|&a, &b| {
            rank_key(&items[a], policy, prefer_path).cmp(&rank_key(&items[b], policy, prefer_path))
        });
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
        policy,
        family_policy,
        prefer_path: prefer_path.to_vec(),
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

/// Options for [`build_keep_set_materialized`].
pub struct MaterializeBuildOpts<'a> {
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path: &'a [String],
    pub tier2_enabled: bool,
    pub created_from: Option<KeepSetProvenance>,
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
    let mut resolved = resolve_groups(
        items,
        opts.policy,
        opts.family_policy,
        opts.prefer_path,
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
    let policy = resolved.policy;
    let prefer = resolved.prefer_path.clone();
    let tier2 = resolved.tier2_enabled;

    for (g_idx, group) in resolved.groups.clone().into_iter().enumerate() {
        if group.is_empty() {
            continue;
        }

        // Rank full group once.
        let mut ranked = group.clone();
        ranked.sort_by(|&a, &b| {
            rank_key(&resolved.items[a], policy, &prefer).cmp(&rank_key(
                &resolved.items[b],
                policy,
                &prefer,
            ))
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

const DECISION_CSV_HEADER: [&str; 19] = [
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
        }
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
}
