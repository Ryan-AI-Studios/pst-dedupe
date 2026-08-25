# Antigravity Review — Track 0090: Embedded Message Content Hash

- **Track ID:** `0090-EmbeddedMsgContentHash`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-24
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, Relativity parity analysis, and PST subnode recursion design.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0090-EmbeddedMsgContentHash/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0090-EmbeddedMsgContentHash/plan.md)

---

## 1. Executive Summary

Track 0090 addresses a significant identity fidelity edge case under `--strong-content-hash body-recip-attach`: when an attachment is an embedded email (`ATTACH_EMBEDDED_MSG` / `message/rfc822`), the raw attach blob hash is replaced with a bounded, recursive Relativity-style identity hash. This closes `D-0086-embedded-email-hash`.

The objective aligns with industry eDiscovery standards (Relativity Processing four-component AttachmentHash). However, this review identifies several critical **structural PST reader hurdles**, an **incomplete preimage sketch in §2.4**, and **budget/recursion guardrails** that require rigorous definition.

---

## 2. Blind Spots & Technical Findings

### Finding 0090-1: Missing Header Component in Spec Preimage Sketch (§2.4)
- **Blind Spot in Spec:** §2.4 drafts the preimage sketch as:
  ```text
  embedded_component = SHA-256(
      b"pst-dedup/embedded-msg-hash/v1\0"
      || depth_u8
      || body_hash_32
      || recipients_hash_32
      || attachments_hash_32
  )
  ```
- **Hazard:** Notice that the sketch omitted the **header component** (subject, submit time, sender email)! Two different attached messages that share the same body and recipients but have different subjects or submit dates would falsely collide.
- **Relativity Protocol Anchor:** Relativity's AttachmentHash for embedded emails recursively computes the full four-component hash (`HeaderHash`, `MessageBodyHash`, `RecipientHash`, `AttachmentHash`).
- **Correction:** The preimage MUST include the child's header hash (or normalized subject, submit time, and sender):
  ```text
  embedded_component = SHA-256(
      b"pst-dedup/embedded-msg-hash/v1\0"
      || depth_u8
      || header_hash_32       // normalized subject | submit_time | sender
      || body_hash_32         // normalized full body SHA-256
      || recipients_hash_32   // normalized recipients (structured TC or display)
      || attachments_hash_32  // sorted child attachment slots
  )
  ```

### Finding 0090-2: PST Subnode Message PC Parsing Requirement in `pst-reader`
- **Live Code Fact:** In MS-PST files, `ATTACH_EMBEDDED_MSG` attachments do **not** store binary stream data under `PidTagAttachDataBinary`. The embedded message is stored as an independent subnode tree under the attachment node (containing the child Message PC, Recipient Table, and Attachment Table).
- **Current Limitation:** Currently, `pst.open_attachment_data(msg_nid, attach_nid)` fails or returns empty on embedded messages because it looks for `PidTagAttachDataBinary`.
- **Requirement:** `pst-reader` must provide a subnode inspection helper:
  `pst.read_embedded_message_properties(parent_nid, attach_nid, depth) -> Result<MessageProperties>` (or extract the child PC).
- **Fail-Closed Degrade:** If the subnode is missing or corrupt, it must return a Choice B unread sentinel (`attach_unread_sentinel`) rather than panicking or producing an all-zero hash.

### Finding 0090-3: Recursion Depth & Byte Budgeting
- **Design Rule:** Hard limit on nesting depth (e.g. `MAX_EMBEDDED_DEPTH = 3`).
- **Policy at Depth Limit:** When `depth >= max_depth`:
  - Should the child attachment slot become an unread sentinel with reason `DEPTH_LIMIT` or fall back to raw blob?
  - **Recommendation:** Use a domain-separated depth-limit sentinel:
    `SHA-256(b"pst-dedup/attach-depth-limit/v1\0" || name || size)`
    This guarantees deterministic group splitting without unbounded memory recursion or stack overflow.

### Finding 0090-4: EML / RFC822 Source Formats
- **Format Consideration:** In pure EML files (or PST attachments with `message/rfc822`), the embedded message is a raw MIME stream.
- **Handling:** For EML, `mailparse` or standard MIME header/body extraction can parse the embedded `message/rfc822` part into header/body/recipients/attachments.
- **Consistency:** Ensure both PST subnode embedded messages and EML `message/rfc822` attachments yield equivalent recursive hash semantics.

---

## 3. Recommended Spec & Plan Amendments

1. **Fix §2.4 Preimage Formula:** Explicitly include `header_hash_32` in the `embedded-msg-hash/v1` specification.
2. **Update §3 In Scope:** Add requirement for `pst-reader` embedded message subnode property loader.
3. **Update §7 Definition of Done (DoD-1 & DoD-3):**
   - Assert that changing the subject of an attached email changes the parent's content hash under `--strong-content-hash body-recip-attach`.
   - Assert that exceeding `MAX_EMBEDDED_DEPTH` produces a deterministic sentinel without stack overflow or panic.

---

## 4. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with essential preimage & reader subnode amendments)**
- **Complexity / Risk:** Medium (requires NDB subnode reading for embedded message PCs).
- **Execution Estimate:** 1.5 – 2 days.
