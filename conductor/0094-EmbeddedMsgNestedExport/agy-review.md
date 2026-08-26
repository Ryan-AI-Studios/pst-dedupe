# Antigravity Review — Track 0094: Embedded Message Nested Export

- **Track ID:** `0094-EmbeddedMsgNestedExport`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-25
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, MS-PST §2.4.6.2 subnode message analysis, and recursive export architecture.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0094-EmbeddedMsgNestedExport/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0094-EmbeddedMsgNestedExport/plan.md)

---

## 1. Executive Summary

Track 0094 addresses the single largest source of attachment export failures in real-world operator mailboxes. In operator runs on `INC0102784.pst` (4,055 messages), **100% of all attachment failures (374 / 374)** were classified as `ATTACH_EMBEDDED_UNPARSED`, affecting 220 parent messages and representing ~241 MB of email evidence.

The root cause in the codebase is remarkably concentrated:
- `pst-writer` **already contains** a complete recursive embedded message subnode builder (`build_embedded_message_object`), but `from_canonical_message` and `from_canonical_message_owned` hardcode `embedded_message: None`.
- `pst-reader` (from Track 0090) **already contains** embedded message subnode resolution helpers (`resolve_embedded_root`, `resolve_embedded_root_nbt`).
- `CanonicalAttachment` lacks the nested message representation to carry extracted child messages from `pst_materializer` to the writer.

This track bridges the gap, converting 374 soft-failures into successfully written nested subnode messages and closing `D-0067-embedded-depth`.

---

## 2. Technical Context & MS-PST Protocol Anchors

### 2.1 MS-PST §2.4.6.2 (Attachment to Message — Method 5)
- When `PidTagAttachMethod` is `ATTACH_EMBEDDED_MSG` (`0x00000005`), the attachment represents a nested Message object.
- The attachment PC does not contain binary stream data under `PidTagAttachDataBinary`. Instead, the attachment node has a **subnode tree** containing the child Message PC, Recipient Table, and child Attachment Table.
- `pst-writer::production::build_embedded_message_object` correctly implements this layout:
  1. Allocates a child `NID_TYPE_NORMAL_MESSAGE` (for use as a subnode key).
  2. Recursively calls `build_message_payload` with `depth + 1`.
  3. Writes the child PC heap data chain, links its subnode BID, and attaches it via `layout.add_subnode_leaf`.
  4. Writes the attachment PC with `PID_TAG_ATTACH_METHOD` = 5 and size reflecting the nested message.

---

## 3. Blind Spots & Technical Findings

### Finding 0094-1: Nested Child Attachment Streaming (`open_attachment_data` NBT Limitation)
- **Critical Reader Constraint:**
  - When an embedded message itself contains child binary attachments (e.g. an attached email that contains a PDF or spreadsheet), the child attachment must be streamed into the output PST.
  - In `pst-reader/src/messaging/attachment.rs`, `open_attachment_data_inner` begins with:
    ```rust
    let msg_entry = self.nbt.get(message_nid).ok_or(PstError::NodeNotFound(message_nid.0))?;
    ```
  - **The Hazard:** `self.nbt` contains **only top-level PST messages**. A nested subnode message's NID does not exist in the NBT! If `PstAttachStreamSource` attempts to open a child attachment using the nested message's NID via `open_attachment_data`, it will fail with `PstError::NodeNotFound`.
- **Recommendation:**
  - Add a subnode-aware streaming helper to `pst-reader`:
    `open_attachment_data_subnode(parent_node: &MessageNodeRef, attach_nid: NodeId) -> Result<AttachmentDataReader>`
  - Ensure `PstAttachStreamSource` can locate attachments under nested message subnodes.

### Finding 0094-2: DTO Representation and Serde Boundaries
- **Code Constraint in `dedup-engine/src/keepset.rs`:**
  - `CanonicalAttachment` derives `#[derive(Clone, Debug, Default, Serialize, Deserialize)]`.
  - `CanonicalMessage` derives `#[derive(Clone, Debug)]` (it does **not** implement `Serialize` / `Deserialize`).
  - If `pub embedded_message: Option<Box<CanonicalMessage>>` is added to `CanonicalAttachment` without annotations, `serde` compilation will fail.
- **Recommendation:**
  - Tag the new field with `#[serde(skip)]` on `CanonicalAttachment`:
    ```rust
    /// Materialized nested message for method-5 (ATTACH_EMBEDDED_MSG) attachments.
    #[serde(skip)]
    pub embedded_message: Option<Box<CanonicalMessage>>,
    ```
  - Materialized winner attachments are in-memory DTOs used transiently during export; skipping serde preserves backward compatibility and avoids bloating JSON reports.

### Finding 0094-3: Strict Depth & Byte Budgeting (Loop & Bomb Defense)
- **Edge Case:** Malicious or malformed PSTs with cyclic subnodes or deeply nested forwards (e.g. 50 levels of embedded emails).
- **Guardrails:**
  - Enforce `MAX_EMBEDDED_DEPTH = 3` (aligned with 0090 and existing writer constants).
  - When `depth >= MAX_EMBEDDED_DEPTH`, do not recurse. Leave `embedded_message: None` and record `AttachmentFidelityKind::DepthLimit` / `ATTACH_DEPTH_LIMIT`.
  - Enforce max nested body budget (e.g. 32 MiB) to prevent unbounded memory growth during materialization.

### Finding 0094-4: Unique-EML Export Path Parity
- **Parity Opportunity:**
  - In `crates/dedup-engine/src/eml_pack.rs`, `write_canonical_eml` writes attachments.
  - When `att.attach_method == Some(ATTACH_EMBEDDED_MSG)` and `att.embedded_message` is `Some(msg)`, `eml_pack.rs` can serialize the nested message as a standard MIME `message/rfc822` part using standard EML headers and body.
  - This allows `unique-eml` to achieve full nested export parity alongside `unique-pst`.

### Finding 0094-5: Operator Smoke Impact (INC0102784)
- **Expected Outcome:**
  - Prior: 374 / 374 attachment failures (`ATTACH_EMBEDDED_UNPARSED`), run exited with `ATTACH_SOFT_FAIL`.
  - Post-0094: 374 embedded messages successfully written (`embedded_messages_written: 374`), `attachments_failed: 0`, run exits with `SUCCESS`.

---

## 4. Recommended Spec & Plan Amendments

1. **Update `plan.md` §Phase 1 (Reader & Materialize):**
   - Add `open_attachment_data_subnode` in `pst-reader` to support streaming binary attachments located inside embedded messages.
   - Add `#[serde(skip)] pub embedded_message: Option<Box<CanonicalMessage>>` to `CanonicalAttachment`.
   - Update `pst_materializer.rs` to extract nested messages up to `depth < MAX_EMBEDDED_DEPTH`.
2. **Update `plan.md` §Phase 2 (Writer Adapter):**
   - Update `from_canonical_message` and `from_canonical_message_owned` to map `embedded_message` into `WriteAttachment.embedded_message`.
3. **Update §7 Definition of Done (DoD-1 & DoD-3):**
   - Assert round-trip verification: a PST with an embedded message (and an embedded message containing a binary attach) writes cleanly and re-opens in `pst-reader`.
   - Assert `ATTACH_DEPTH_LIMIT` is recorded when depth exceeds 3.

---

## 5. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with nested child streaming & serde amendments)**
- **Complexity / Risk:** Medium (requires `pst-reader` subnode attach stream helper and recursive materialize mapping).
- **Execution Estimate:** 1.5 – 2 days.
- **ROI Rating:** **Highest in Series N** (resolves 100% of operator attachment failures on INC0102784).
