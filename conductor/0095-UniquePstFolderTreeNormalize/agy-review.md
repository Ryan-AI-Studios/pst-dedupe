# Antigravity Review — Track 0095: Unique-PST Folder Tree Normalize

- **Track ID:** `0095-UniquePstFolderTreeNormalize`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-25
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, folder hierarchy root-cause analysis, and QC tree comparison normalization.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0095-UniquePstFolderTreeNormalize/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0095-UniquePstFolderTreeNormalize/plan.md)

---

## 1. Executive Summary

Track 0095 addresses the fatal QC verification failure (`property=folder_tree_structure`, `hard_fail=true` -> `VERIFY_FAILED`) observed during operator preserve-layout exports on multi-mailbox archives (`INC0102784.pst`).

While all 4,055 unique messages were successfully written without data loss, the output folder hierarchy suffered from two prominent path-shape defects:
1. **Redundant Nested Root:** `Root/Top of Personal Folders/<mailbox>/Top of Personal Folders/Inbox` (duplicating `Top of Personal Folders` under the mailbox prefix).
2. **Empty Residual Ghost Folder:** `Root/Top of Personal Folders/Unique Mail` was always pre-allocated even when zero messages used residual routing.

This track rectifies both the writer's path-segment parsing and the differential QC tree comparison, narrowing `D-0070-multi-source-stream-prefix` and enabling clean end-to-end verification.

---

## 2. Root Cause Analysis & Protocol Anchors

### 2.1 MS-PST §2.4.4 Folder Hierarchy & IPM_SUBTREE
- In MS-PST, every store has a root folder (`NID_ROOT_FOLDER`, `0x122`) whose primary child is `IPM_SUBTREE` (the root of the user-visible mailbox, typically named `"Top of Personal Folders"` or `"Top of Outlook Data File"` in Outlook).
- When `pst-reader` reads source messages, `locus.folder_path` contains the full path including the source's IPM container: `"Top of Personal Folders/Inbox"`.
- When `pst-writer` creates the destination PST with `--folder-layout preserve`:
  1. It creates `IPM_SUBTREE` with display name `"Top of Personal Folders"`.
  2. For multi-source exports, it prepends a prefix segment (e.g. `"INC0102784"`).
  3. It then takes `locus.folder_path` and verbatim splits it into `["Top of Personal Folders", "Inbox"]`.
  4. The resulting hierarchy under `IPM_SUBTREE` becomes `<prefix>/Top of Personal Folders/Inbox`, causing the visible duplicate.

---

## 3. Blind Spots & Technical Findings

### Finding 0095-1: Leading Root/IPM Segment Stripping in `parse_folder_path`
- **Root Cause in `pst-writer`:**
  - `parse_folder_path` in `crates/pst-writer/src/production.rs` splits paths on slashes without stripping the leading `Root` or `Top of Personal Folders` container names.
- **Recommendation:**
  - Update `parse_folder_path` to strip leading well-known root aliases (`"root"`, `"top of personal folders"`, `"top of information store"`, `"top of outlook data file"`, `"ipm_subtree"`) before returning segments.
  - With leading alias stripped:
    - Single-source preserve: `"Top of Personal Folders/Inbox"` -> `["Inbox"]` -> `Top of Personal Folders/Inbox`.
    - Multi-source preserve: `"Top of Personal Folders/Inbox"` -> `["<mailbox>", "Inbox"]` -> `Top of Personal Folders/<mailbox>/Inbox`.
  - This matches standard eDiscovery production standards and eliminates nested duplicate root folders.

### Finding 0095-2: Lazy Materialization of Residual "Unique Mail" Folder
- **Defect in `IncrementalFolderPlan::start`:**
  - `IncrementalFolderPlan::start` unconditionally allocates a `PlannedFolder` for `"Unique Mail"` in `roots`.
  - When all messages successfully resolve to preserved folder paths, this results in an empty, ghost `"Unique Mail"` folder at the root of the output PST.
- **Recommendation:**
  - Do not pre-allocate `"Unique Mail"` in `roots` during `start`.
  - Allocate the residual folder lazily in `assign_message` only if and when a message actually falls back to `PathParseOutcome::Residual`.

### Finding 0095-3: QC Suffix Matching & Normalization Symmetries
- **Defect in `unique_pst_qc.rs` (`folder_tree_matches`):**
  - `unique_pst_qc.rs` compares `digest.folder_paths` against `expected_folder_counts`.
  - `export_messages.csv` contains `folder_path` from source message loci.
  - If `expected_folder_counts` has `"Top of Personal Folders/Inbox"` and output has `"Root/Top of Personal Folders/<mailbox>/Inbox"`, `normalize_folder_key` must strip leading root/IPM prefixes symmetrically on both sides so that `folder_leaf_matches` accurately correlates counts per custodian/folder.

### Finding 0095-4: Preserving Legitimate User Subfolders Named "Top of Personal Folders"
- **Edge Case:** What if a user created a nested subfolder named "Top of Personal Folders" inside their Inbox (`"Top of Personal Folders/Inbox/Top of Personal Folders"`)?
- **Rule:** Only strip the **first/leading** segment if it matches a root alias; never strip subsequent matching segments in the path.

### Finding 0095-5: Flat Layout Isolation
- **Invariant:** `--folder-layout flat` routes all messages directly to `Top of Personal Folders/Unique Mail` (or `--folder-display-name`). This path must remain completely unaffected.

---

## 4. Recommended Spec & Plan Amendments

1. **Update `plan.md` §Phase 1 (Writer Implementation):**
   - In `pst-writer::production::parse_folder_path`, strip leading root/IPM container alias segments (`"root"`, `"top of personal folders"`, `"top of information store"`, etc.).
   - In `IncrementalFolderPlan`, make residual folder creation lazy (allocate NID only on first residual message).
2. **Update `plan.md` §Phase 1 (QC Normalization):**
   - Update `normalize_folder_key` in `unique_pst_qc.rs` to strip leading root/IPM aliases before computing expected leaf keys.
3. **Update §7 Definition of Done (DoD-1 & DoD-2):**
   - Assert that multi-source preserve exports produce `Top of Personal Folders/<source_prefix>/<folder>` without nested duplicate `Top of Personal Folders` and without empty `Unique Mail` folders.
   - Assert `unique_pst_qc` passes with `hard_fail=false` on multi-source preserve fixtures.

---

## 5. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with leading root stripping and lazy residual allocation)**
- **Complexity / Risk:** Low (pure path-string normalization and lazy plan allocation; no NDB/binary encoding changes).
- **Execution Estimate:** 0.5 – 1 day.
