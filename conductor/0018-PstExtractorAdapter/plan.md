# 0018 — PST extractor adapter — Plan

Phased checklist. Map phases to DoD items in `spec.md` §7. Execute in `C:\dev\dedupe`.

> **Ledger:**  
> `ledgerful ledger start 0018-pstextractor --category FEATURE --message "PST extractor → Normalized Items + logical_hash"`  
> Commit in Finalize.

---

## Phase 0 — Preconditions → DoD-10 baseline

- [x] Confirm **0016** Completed: `../0016-PurviewIngest/review.md` — PST inventory + CAS whole-file digests
- [x] Confirm **0017** Completed: `../0017-NormalizedItem/review.md` — schema v2, family cohesion, logical_hash v1 + BCC + length-prefix framing
- [x] Confirm `matter_core::LOGICAL_HASH_VERSION == 1` and `compute_email_logical_hash` available — **use, do not reimplement**
- [x] Note CAS today: only `put_bytes(&[u8])` — streaming put required for attach path
- [x] Read plan-of-record §2.3, §4.6 (PST checkpoint grain)
- [x] Inventory `pst-reader` gaps (spec §2.4): body truncate, no BCC, no attach stream, path-only open
- [x] List fixtures: `fixtures/*.pst` (Aspose/sample — no client data)
- [x] `cargo test -p matter-core` / `pst-reader` / `ingest-purview` green

## Phase 1 — Design lock → DoD-3/4/5 prep

- [x] Freeze path convention: `{pst_path}!/{folder}/{nid_hex}` + attach suffix
- [x] Freeze **message native** = **`pst-native-message-v1`** field list + framing (golden tests); **not** EML
- [x] Explicit: EML export deferred to **0040** — never used for `native_sha256`
- [x] Freeze job kind `extract_pst`, stage `pst_extract`, default `batch_size=500` (**mid-folder**)
- [x] Freeze recipient strategy: DisplayTo/Cc/Bcc parse for P0
- [x] Freeze open order: FS path if exists → else CAS stream → **`workspace/temp/`**
- [x] Sketch streaming CAS API (`put_reader` / equivalent) + collision policy parity with `put_bytes`
- [x] Sketch `ExtractLimits` (batch_size, max_attachment_bytes, max_in_memory_put_bytes)
- [x] Sketch resume cursor: `last_folder_path`, `last_message_nid`, `folder_message_index`
- [x] Decide which APIs land in `pst-reader` vs `extract-pst` vs `matter-core`
- [x] Audit action names: `extract.start` / `extract.complete` / `extract.fail`
- [x] Confirm 0017 handoff: always pass `bcc` into `EmailLogicalInput` (empty `Vec` if unknown)

## Phase 2 — matter-core: streaming CAS + workspace temp → DoD-6, DoD-7 prep

- [x] Add layout dirs: `workspace/`, `workspace/temp/` (constants + create on matter create/open)
- [x] `cleanup_workspace_temp()` on create/open (delete leftover evidence files)
- [x] Implement streaming CAS put (`Read` → hash + write → final path; no full buffer)
- [x] Unit tests: multi-chunk reader → same digest as `put_bytes` for same content
- [x] Unit tests: temp cleanup on open

## Phase 3 — pst-reader extensions → DoD-2/3/7 prep

- [x] Full body API (no 4KB truncate) — keep preview helper for CLI if desired
- [x] Add properties: delivery time, DisplayCc, DisplayBcc, HTML body (as available)
- [x] Attachment: **streaming Read** over binary data (not production `Vec<u8>` for full payload)
- [x] Optional: open from path only still OK; document CAS path is extract-pst’s job
- [x] Unit/fixture tests on reader for new surfaces
- [x] FILETIME → RFC3339 helper (reader or extract-pst)

## Phase 4 — Scaffold `extract-pst` → DoD-1

- [x] `cargo new --lib crates/extract-pst`
- [x] Workspace member + deps (`matter-core`, `pst-reader`, …)
- [x] Modules: `lib.rs`, `error.rs`, `limits.rs`, `open.rs`, `recipients.rs`, `native_message.rs`, `extract.rs`, `checkpoint.rs`
- [x] README stub: **blocking-thread WARNING**, native v1, streaming, matter temp
- [x] `cargo check -p extract-pst`

## Phase 5 — Open + walk + item write → DoD-2, DoD-3, DoD-4

- [x] Open FS / CAS → matter `workspace/temp/` only
- [x] Walk folders; for each message **in batches of `batch_size` even mid-folder**:
  - [x] Skip if path already `extracted`
  - [x] Read props + stream attachments → CAS
  - [x] Build **pst-native-message-v1** → CAS → parent `native_sha256`
  - [x] Body → `text_sha256` (small put_bytes ok if under threshold)
  - [x] Build `EmailLogicalInput` (**include bcc**) → `compute_email_logical_hash`
  - [x] `update_item` parent fields + hash + version + status
  - [x] Checkpoint + SQLite batch commit every `batch_size`
- [x] Cancel checks between messages
- [x] Integration: happy path on fixture PST
- [x] Golden test: native v1 stable digest

## Phase 6 — Resume + CAS-only + errors + temp hygiene → DoD-5, DoD-6, DoD-7, DoD-8

- [x] Persist/load `pst_extract` checkpoint with mid-folder index/NID
- [x] `resume_extract` skips completed paths; continues after `last_message_nid` / index
- [x] Integration: **mid-folder** cancel → resume no duplicates
- [x] Integration: extract with only CAS digest; assert temp under matter root not `%TEMP%`
- [x] Integration: orphan temp file cleaned on `Matter::open`
- [x] Structured errors for ANSI/corrupt; per-message continue
- [x] Audit start/complete|fail; `verify_audit_chain`

## Phase 7 — Docs + polish → DoD-9

- [x] Finalize `crates/extract-pst/README.md` (native v1, streaming, temp, mid-folder, BCC, blocking)
- [x] Update matter-core README (workspace temp + streaming CAS)
- [x] Note new `pst-reader` APIs in ARCHITECTURE / reader docs
- [x] Root README crate table
- [x] Optional CLI smoke (not DoD)

## Phase 8 — Verification → DoD-10

- [x] `cargo test -p pst-reader`
- [x] `cargo test -p matter-core`
- [x] `cargo test -p extract-pst`
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `ledgerful verify` (**required**)
- [x] Capture evidence for `review.md`

## Phase 9 — Finalize → DoD-11

- [x] Write `review.md` (APIs, streaming CAS, native v1 layout, temp policy, mid-folder resume evidence, deferred EML/0040)
- [x] Update `../conductor.md`: **0018** → **Completed**
- [x] Update `../sequencing.md`
- [x] Commit ledger TX
- [x] Handoff: **0021** (with **0019**); note **0040** for EML production export

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
