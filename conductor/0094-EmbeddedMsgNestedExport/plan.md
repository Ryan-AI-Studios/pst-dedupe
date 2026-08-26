# 0094 — Embedded Message Nested Export — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-25):** PtypObject `0x3701` **in scope** (closes D-0069-embed-object);
> full nested extract (not 0090 identity); wire existing `open_attach_data_from_message_node`;
> depth fail → `ATTACH_DEPTH_LIMIT`; `serde(skip)` + winner-only extract; unique-eml nested MIME
> **out** — see `spec.md` §2.10.

> **Ledger:** `ledgerful ledger start crates/pst-writer --category FEATURE --message "0094 embedded msg nested export"`

---

## Phase 0 — Design lock → DoD-4 (partial), hygiene

- [ ] Lock export depth owner = writer `max_embedded_depth` (default 3, clamp `[1, 8]`). Extract receives the **same** budget. Exhaustion → `ATTACH_DEPTH_LIMIT`, not UNPARSED.
- [ ] Lock per-nest payload ceiling **32 MiB** (existing per-attach/run caps still win).
- [ ] DTO shape: **A** dedicated nested type → `WriteMessage`, **or** **B** synthesized `CanonicalMessage` (`embedded-msg-hash/v1` for `content_hash`; locus parent-relative, no fake folder). Nested field on `CanonicalAttachment` is **`#[serde(skip)]`** either way.
- [ ] Confirm method-5 only — method-1 rfc822 stays by-value binary.
- [ ] Confirm unique-eml **ignores** nested DTO this track.
- [ ] Confirm PtypObject layout: `PcValue::Object { nid, size }` → Ptyp `0x000D`, 8-byte heap `{Nid, ulSize}` on attach PC `0x3701`. Never non-empty PtypBinary `0x3701` on method-5.
- [ ] **Hygiene:** untracked `fixtures/keep_set_summary.json` — delete, gitignore, or move to `output/`.

## Phase 1 — Reader / materialize → DoD-1, DoD-2, DoD-3

- [ ] Bids-based **full** nested extract from `MessageNodeRef` (HTML, `message_class`, MID, flags, recipients, child attach metadata) under depth/byte budgets. Identity helpers stay for 0090.
- [ ] Fail closed: unreadable → `embedded = None` + unparsed reason. Budget exhaust → **distinct depth-limit reason**.
- [ ] Child by-value attaches: stream via **`open_attach_data_from_message_node`** (do not call NBT `open_attachment_data` with a nested NID). Prefer `AttachStreamSource` over buffering the nest.
- [ ] Lazy: extract nests **only for unique-pst winners** (plus tests).
- [ ] Reader: resolve nested root **via PtypObject `0x3701` first**, subnode-scan fallback for 0069-era files.
- [ ] Unit tests: `pst-reader` nested extract + materialize fail-closed / depth-limit reasons.
- [ ] Parent-hash regression: same fixture hashed with extract on vs off → identical `content_hash` / `strong_content_hash`.

## Phase 2 — Adapter + writer → DoD-1, DoD-2

- [ ] Map nested through `from_canonical_message` / `_owned` (stop hardcoding `None`).
- [ ] `PcValue::Object`; method-5 attach PC writes `PidTagAttachDataObject`. Keep RAM relief `att.embedded_message = None` after write.
- [ ] Fidelity tests:
  - Nested write + **reopen via `0x3701`**; nested subject/sender/recipients/body match; `embedded_messages_written >= 1` and `embedded_unparsed == 0` on the parseable fixture. Update test 12 (still forbid PtypBinary payload; **require** PtypObject).
  - Nest containing a **by-value child attach** streams (no `NodeNotFound`).
  - Unparsed path still emits `ATTACH_EMBEDDED_UNPARSED` (no ghost attach).
  - Depth > 3 → `ATTACH_DEPTH_LIMIT`.

## Phase 3 — QC / docs / deferred → DoD-3, DoD-4, DoD-5

- [ ] Fidelity doc + unique-pst export notes: PtypObject discovery; unique-eml still honesty-only for nests.
- [ ] Close `D-0069-embed-object`. Close or narrow `D-0067-embedded-depth` (unique-eml nested MIME / matter child docs residual).
- [ ] `has_embedded` QC stratum already exists — keep. Optional: max-nest-depth candidate if extract now exposes depth.
- [ ] Operator re-smoke guidance in `review.md` (large drop, not guaranteed zero; optional Outlook-open of a nest).

## Phase 4 — Finalize → DoD-6

- [ ] `review.md`; conductor **Completed**; ledger commit.

---

## Handoff notes

- Highest ROI for INC0102784 attach soft-fail (374/374).
- Do not conflate with 0090 hash — export is separate; parent hashes must not move.
- Do not invent by-value `.msg` bytes. PtypObject is the discovery property, not a file payload.
- unique-eml nested RFC822 is **not** this PR.
- 0093 is Completed — heap diversion is already on `main`.
