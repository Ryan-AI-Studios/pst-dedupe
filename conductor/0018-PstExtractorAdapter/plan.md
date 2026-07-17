# 0018 — PST extractor adapter — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:**  
> `ledgerful ledger start 0018-pstextractor --category FEATURE --message "PST extractor → Normalized Items + logical_hash"`  
> Commit in Finalize.

---

## Phase 0 — Preconditions → DoD-10 baseline

- [ ] Confirm **0016** Completed: `../0016-PurviewIngest/review.md` — PST inventory + CAS whole-file digests
- [ ] Confirm **0017** Completed: `../0017-NormalizedItem/review.md` — schema v2, family cohesion, logical_hash v1 + BCC + length-prefix framing
- [ ] Confirm `matter_core::LOGICAL_HASH_VERSION == 1` and `compute_email_logical_hash` available — **use, do not reimplement**
- [ ] Note CAS today: only `put_bytes(&[u8])` — streaming put required for attach path
- [ ] Read plan-of-record §2.3, §4.6 (PST checkpoint grain)
- [ ] Inventory `pst-reader` gaps (spec §2.4): body truncate, no BCC, no attach stream, path-only open
- [ ] List fixtures: `fixtures/*.pst` (Aspose/sample — no client data)
- [ ] `cargo test -p matter-core` / `pst-reader` / `ingest-purview` green

## Phase 1 — Design lock → DoD-3/4/5 prep

- [ ] Freeze path convention: `{pst_path}!/{folder}/{nid_hex}` + attach suffix
- [ ] Freeze **message native** = **`pst-native-message-v1`** field list + framing (golden tests); **not** EML
- [ ] Explicit: EML export deferred to **0040** — never used for `native_sha256`
- [ ] Freeze job kind `extract_pst`, stage `pst_extract`, default `batch_size=500` (**mid-folder**)
- [ ] Freeze recipient strategy: DisplayTo/Cc/Bcc parse for P0
- [ ] Freeze open order: FS path if exists → else CAS stream → **`workspace/temp/`**
- [ ] Sketch streaming CAS API (`put_reader` / equivalent) + collision policy parity with `put_bytes`
- [ ] Sketch `ExtractLimits` (batch_size, max_attachment_bytes, max_in_memory_put_bytes)
- [ ] Sketch resume cursor: `last_folder_path`, `last_message_nid`, `folder_message_index`
- [ ] Decide which APIs land in `pst-reader` vs `extract-pst` vs `matter-core`
- [ ] Audit action names: `extract.start` / `extract.complete` / `extract.fail`
- [ ] Confirm 0017 handoff: always pass `bcc` into `EmailLogicalInput` (empty `Vec` if unknown)

## Phase 2 — matter-core: streaming CAS + workspace temp → DoD-6, DoD-7 prep

- [ ] Add layout dirs: `workspace/`, `workspace/temp/` (constants + create on matter create/open)
- [ ] `cleanup_workspace_temp()` on create/open (delete leftover evidence files)
- [ ] Implement streaming CAS put (`Read` → hash + write → final path; no full buffer)
- [ ] Unit tests: multi-chunk reader → same digest as `put_bytes` for same content
- [ ] Unit tests: temp cleanup on open

## Phase 3 — pst-reader extensions → DoD-2/3/7 prep

- [ ] Full body API (no 4KB truncate) — keep preview helper for CLI if desired
- [ ] Add properties: delivery time, DisplayCc, DisplayBcc, HTML body (as available)
- [ ] Attachment: **streaming Read** over binary data (not production `Vec<u8>` for full payload)
- [ ] Optional: open from path only still OK; document CAS path is extract-pst’s job
- [ ] Unit/fixture tests on reader for new surfaces
- [ ] FILETIME → RFC3339 helper (reader or extract-pst)

## Phase 4 — Scaffold `extract-pst` → DoD-1

- [ ] `cargo new --lib crates/extract-pst`
- [ ] Workspace member + deps (`matter-core`, `pst-reader`, …)
- [ ] Modules: `lib.rs`, `error.rs`, `limits.rs`, `open.rs`, `recipients.rs`, `native_message.rs`, `extract.rs`, `checkpoint.rs`
- [ ] README stub: **blocking-thread WARNING**, native v1, streaming, matter temp
- [ ] `cargo check -p extract-pst`

## Phase 5 — Open + walk + item write → DoD-2, DoD-3, DoD-4

- [ ] Open FS / CAS → matter `workspace/temp/` only
- [ ] Walk folders; for each message **in batches of `batch_size` even mid-folder**:
  - [ ] Skip if path already `extracted`
  - [ ] Read props + stream attachments → CAS
  - [ ] Build **pst-native-message-v1** → CAS → parent `native_sha256`
  - [ ] Body → `text_sha256` (small put_bytes ok if under threshold)
  - [ ] Build `EmailLogicalInput` (**include bcc**) → `compute_email_logical_hash`
  - [ ] `update_item` parent fields + hash + version + status
  - [ ] Checkpoint + SQLite batch commit every `batch_size`
- [ ] Cancel checks between messages
- [ ] Integration: happy path on fixture PST
- [ ] Golden test: native v1 stable digest

## Phase 6 — Resume + CAS-only + errors + temp hygiene → DoD-5, DoD-6, DoD-7, DoD-8

- [ ] Persist/load `pst_extract` checkpoint with mid-folder index/NID
- [ ] `resume_extract` skips completed paths; continues after `last_message_nid` / index
- [ ] Integration: **mid-folder** cancel → resume no duplicates
- [ ] Integration: extract with only CAS digest; assert temp under matter root not `%TEMP%`
- [ ] Integration: orphan temp file cleaned on `Matter::open`
- [ ] Structured errors for ANSI/corrupt; per-message continue
- [ ] Audit start/complete|fail; `verify_audit_chain`

## Phase 7 — Docs + polish → DoD-9

- [ ] Finalize `crates/extract-pst/README.md` (native v1, streaming, temp, mid-folder, BCC, blocking)
- [ ] Update matter-core README (workspace temp + streaming CAS)
- [ ] Note new `pst-reader` APIs in ARCHITECTURE / reader docs
- [ ] Root README crate table
- [ ] Optional CLI smoke (not DoD)

## Phase 8 — Verification → DoD-10

- [ ] `cargo test -p pst-reader`
- [ ] `cargo test -p matter-core`
- [ ] `cargo test -p extract-pst`
- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `ledgerful verify` (**required**)
- [ ] Capture evidence for `review.md`

## Phase 9 — Finalize → DoD-11

- [ ] Write `review.md` (APIs, streaming CAS, native v1 layout, temp policy, mid-folder resume evidence, deferred EML/0040)
- [ ] Update `../conductor.md`: **0018** → **Completed**
- [ ] Update `../sequencing.md`
- [ ] Commit ledger TX
- [ ] Handoff: **0021** (with **0019**); note **0040** for EML production export

---

## Suggested file map

```
crates/extract-pst/
  Cargo.toml
  README.md
  src/
    lib.rs
    error.rs
    limits.rs
    open.rs           # FS vs CAS → workspace/temp
    recipients.rs
    native_message.rs # pst-native-message-v1 only
    checkpoint.rs
    extract.rs
  tests/
    integration.rs

crates/matter-core/src/
  cas.rs              # put_reader / streaming
  matter.rs           # workspace/temp + cleanup

crates/pst-reader/src/messaging/
  message.rs          # full body + DisplayCc/Bcc + times
  attachment.rs       # streaming Read for binary
```

---

## Default limits

| Limit | Default | Tests |
|---|---|---|
| `batch_size` | 500 | 1 (mid-folder resume) |
| `max_messages` | None | Some(small) |
| `max_in_memory_put_bytes` | 16 MiB | small |
| `max_attachment_bytes` | None or large cap | force small for fail-closed test |

---

## Handoff notes

- **0016** inventory PST rows stay; extract adds child paths under `{pst}!/…`.
- **0017** logical hash is authoritative — call `compute_email_logical_hash`.
- **Native identity** is `pst-native-message-v1` only — **not** synthetic EML (0040).
- **Attachments** stream into CAS; never full-buffer multi-GB production path.
- **Temp evidence** lives under matter `workspace/temp/`; cleaned on open; never `%TEMP%`.
- **Checkpoints** fire every `batch_size` **mid-folder**.
- **0019** must call extract APIs from a blocking pool.
- **0021** expects `logical_hash` / `message_id` populated for mail.
- Never write into the user’s PST or Purview export tree.
- Single-exe / no-daemon invariant unchanged.
