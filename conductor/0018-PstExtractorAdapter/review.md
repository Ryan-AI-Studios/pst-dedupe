# 0018-PstExtractorAdapter — Review

- **Track:** 0018-PstExtractorAdapter
- **Status:** Completed — Codex **PASS WITH DEFERRED P3**
- **Date:** 2026-07-17
- **Crate:** `crates/extract-pst` (+ matter-core streaming CAS / workspace temp, pst-reader extract APIs)

## Summary

Blocking PST → Normalized Item adapter:

| Area | Result |
|---|---|
| Open | FS exact path or CAS → `workspace/temp/` only (never OS `%TEMP%`) |
| Extract | Folders/messages/attachments → items + `email_attachments` families |
| Native | **`pst-native-message-v1`** (not EML) → parent `native_sha256` |
| Logical | `compute_email_logical_hash` (0017) + BCC always supplied |
| Stream | Attach → `put_reader`; path register streams PST |
| Resume | Mid-folder `batch_size`; skip existing message paths; path+digest identity |
| Errors | Per-message continue; `attach_list_failed` → partial; audit start/complete/fail/paused |

## Public API

- `extract_pst_item`, `extract_pst_path`, `resume_extract`, `list_discovered_psts`
- `ExtractLimits`, `ExtractSummary`, `ExtractCursor`, job/stage constants
- matter-core: `put_reader`, `cleanup_workspace_temp`, `WORKSPACE_*`
- pst-reader: `read_message_extract`, `list_attachments`, `open_attachment_data`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** |
| `cargo test -p extract-pst` | **PASS** (10 unit + 15 integration) |
| `cargo test -p matter-core` / `pst-reader` | **PASS** |
| `cargo test --workspace` | **PASS** |
| `ledgerful verify` | **PASS** |

## Review loop

| Round | Verdict | Notes |
|---|---|---|
| Internal R1 | NEEDS_FIXES | skip dups, path OOM, max_messages, tests |
| Internal R2 | CLEAN | |
| Codex R1 | FAIL | attach list, path open, resume id, temp RAII |
| Codex R2 | FAIL | open_fs_path resume, CWD shadow |
| Codex R3 | **PASS WITH DEFERRED P3** | R2 fixes verified |

## Deferred (`docs/deferred.md`)

| ID | Item |
|---|---|
| D-0018-01 | Attach full-`Vec` fallback path in pst-reader before stream switch |
| D-0018-02 | EML export as native identity (never) → **0040** for human EML |
| D-0018-03 | MAPI recipient table (Display* P0) |
| D-0018-04 | Process runner / progress → **0019** |

## Unblocked

**0021** MatterDedupeJob (with **0019**). Soft: **0020** demos with real extract.
