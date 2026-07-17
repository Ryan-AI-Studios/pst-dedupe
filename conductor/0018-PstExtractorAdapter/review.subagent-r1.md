# Internal Review R1 — 0018-PstExtractorAdapter

## Verdict: NEEDS_FIXES

Solid core delivery for a first implementer pass: workspace member `extract-pst`,
`pst-native-message-v1` (not EML), Display\* BCC-safe recipients, mid-folder
checkpoints + resume entry points, CAS → `workspace/temp/` open (never
`%TEMP%`), `Cas::put_reader` + attach `Read` stream, audit start/complete|fail,
and docs/ARCHITECTURE coverage. **Not CLEAN** because of re-extract duplicate
inserts on non-`extracted` paths, `extract_pst_path` multi-GB RAM buffering,
`max_messages` falsely completing jobs, and missing/weak proof for partial-error
+ forced mid-folder resume. DoD-10/11 gates were **not observed** in this session
(static review; no shell for `cargo`/`git diff`/`ledgerful`).

**Scope reviewed (static):**
- `conductor/0018-PstExtractorAdapter/spec.md` §3 + §7
- `crates/extract-pst/**` (lib, extract, open, native, recipients, checkpoint, limits, error, README, integration tests)
- `crates/matter-core/src/{cas,matter}.rs`, README, `tests/integration.rs` (`put_reader`, workspace temp)
- `crates/pst-reader/src/messaging/{message,attachment}.rs`, PID constants, lib docs
- Workspace `Cargo.toml`, root `README.md`, `ARCHITECTURE.md`
- Git HEAD: `feat/0018-pst-extractor` @ `e717eeee` — commits on branch beyond main:
  - `6f9c58cf` docs(0018) expand spec/plan
  - `e717eeee` feat(0018): extract-pst + streaming CAS

---

## DoD Matrix

| Requirement | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| **DoD-1 — Crate member** | **Met** | `Cargo.toml` workspace members includes `crates/extract-pst`; package `extract-pst` 0.1.0; no Tokio dep. Modules match plan. | Integration + unit modules compile to `target/debug/deps/extract_pst-*.exe` (prior builds present). | **Not observed:** `cargo test -p extract-pst` this session. |
| **DoD-2 — Extract fixture → parents/children/families** | **Partial** | `extract_one_message` inserts parent (`role=parent`, `file_category=email`), `insert_family(email_attachments)`, streams attaches → child items + `set_item_family_role`, status `extracted`/`partial`. Path convention `{pst}!/{folder}/{nid_hex}` + attach suffix. | `happy_path_fixture_extract` asserts email parents, `logical_hash`, `logical_hash_version=1`, native present, audit chain, list PSTs. | Does **not** assert attachment children / family membership when fixture has attaches. Relies on fixture discovery (`aspose_*.pst` / `sample.pst`). |
| **DoD-3 — Identity** | **Met** | Parent native = `encode_native_message_v1` (`PNM1` + u32 v1 + fixed order) → `put_bytes` → `native_sha256`. `extra_json.native_format = "pst-native-message-v1"`. Logical via `compute_email_logical_hash` + `LOGICAL_HASH_VERSION`. Attach native via `put_reader`. README forbids EML native. | Golden digest `09b8a177…8160` in unit + integration; happy path recomputes logical_hash. | Native blob always `put_bytes` (body embedded) — fine for typical bodies; not streaming. |
| **DoD-4 — Recipients / BCC** | **Met** | `parse_display_list` / `bcc_for_logical`; missing DisplayBcc → `[]`; extract always serializes `bcc_addrs_json` and passes `bcc` into `EmailLogicalInput`. Never copies To→Bcc. | Unit: empty/None, parse angles, `bcc_never_invented`, `bcc_mapping_and_logical_hash_integration` (hash differs with BCC). | Fixture may lack real DisplayBcc; covered by unit mapping. |
| **DoD-5 — Resume mid-folder** | **Partial** | `batch_size` default 500; checkpoint every N **and** per folder; cursor has `last_folder_path`, `last_message_nid`, `folder_message_index`, counts; resume at `index+1`; skip `extracted`+`logical_hash`; cancel → `Paused`. | `resume_mid_folder_no_duplicates` with `batch_size=1` + cancel after 2 poll ticks. | Test **allows** path where first run is **not** cancelled (falls back to second full extract). Does not assert checkpoint JSON fields / `resume_extract` was used. Re-processing non-`extracted` paths **inserts duplicates** (see P2). |
| **DoD-6 — CAS open + temp hygiene** | **Met** | `open_pst`: FS if present else CAS stream → `workspace/temp/{job}_{digest12}_{seq}_{pid}.pst`; refuses OS temp path; RAII delete; `Matter::create`/`open` call `cleanup_workspace_temp`. | `open_from_cas_only_temp_under_workspace`, `orphan_temp_cleaned_on_matter_open`, matter-core `workspace_temp_orphan_cleaned_on_open`. | — |
| **DoD-7 — Streaming attach path** | **Met** (minor residual) | Production attach path: `open_attachment_data` → `AttachmentDataReader: Read` → `matter.put_reader`. Leaf-block stream for multi-block; CAS `put_reader` 64 KiB loop. | matter-core unit + integration multi-chunk parity; extract-pst `put_reader_parity_via_matter`. | Heap-resident / small attaches still `Vec` (documented). Dead-ish fallback in `attachment.rs` still calls `read_subnode_data` (full `Vec`) before size check — prefer remove or stream-only. |
| **DoD-8 — Partial errors** | **Partial** | Per-message continue + `item_errors` (`message_props_failed`, `attach_data_missing`, `attach_too_large`, `cas_put_failed`); parent `partial` on attach failure; ANSI/bad open → structured fail + `extract.fail`. | `ansi_or_bad_file_structured_fail`. | **No** integration for “one bad message/attach, others ok, errors recorded” (spec §3.12.3). |
| **DoD-9 — Audit + docs** | **Met** | `extract.start` (limits/path/digest), `extract.complete` (counts), `extract.fail` (open/fatal). README: blocking, native v1, streaming, temp, mid-folder, BCC, out-of-scope. matter-core README: workspace temp + `put_reader`. Root README + ARCHITECTURE crate map. pst-reader docs for extract surfaces. | Happy path `verify_audit_chain`. | Resume does not re-emit `extract.start` (OK). |
| **DoD-10 — Workspace gate** | **Not verifiable here** | Code compiles (prior `target/debug/deps` artifacts). | — | Orchestrator must capture: `fmt`, `clippy -D warnings`, `cargo test -p extract-pst`, `cargo test -p matter-core` (incl. `put_reader`), workspace tests, `ledgerful verify`. |
| **DoD-11 — Recorded** | **Unmet** (expected pre-finalize) | No canonical `review.md`; conductor not Completed; no ledger TX in scope of R1. | — | Finalize after fixes + gates. |
| Spec §3.1 crate/deps | **Met** | matter-core, pst-reader, camino, thiserror, serde/json, chrono, sha2; hand-rolled native framing. | — | — |
| Spec §3.2 open sources | **Met** | FS prefer + CAS materialize matter-local temp. | CAS-only integration. | `extract_pst_path` loads entire PST into RAM before CAS (P2). |
| Spec §3.3 entry points | **Met** | `extract_pst_item`, `extract_pst_path`, `list_discovered_psts`, `resume_extract`; job kind `extract_pst`, stage `pst_extract`. | Constants test + list in happy path. | — |
| Spec §3.5 native policy | **Met** | Not EML; not whole-PST digest; not logical preimage as native. | Golden + extra_json. | — |
| Spec §3.7 body | **Met** | `read_message_extract` full body; `normalize_body` before hash; CAS stores normalized UTF-8. CLI preview still truncated. | Logical recompute uses CAS body. | HTML optional path present. |
| Spec §3.8 checkpoints | **Met** (impl) | Mid-folder batch + folder complete; cancel pause. | Resume test weak (above). | Cancel before first durable checkpoint → non-resumable (P3). |
| Spec §3.12 tests map | **Partial** | 1 happy, 2 resume (weak), 4 CAS open, 5 temp cleanup, 6 put_reader, 7 ANSI/bad, 8 BCC unit, 9 logical integration, 10 native golden. | — | **3 partial inject** missing; resume not forced. |
| Completeness sweep | **OK** | No `TODO`/`FIXME`/`todo!`/`unimplemented!` in `extract-pst`. | — | Soft skips: tests `eprintln!("skip: no fixture…"); return` if no message-bearing PST. |

---

## Findings (P0–P3)

### [P2] Re-extract of non-`extracted` message paths inserts duplicates

**Confidence:** High  
**Requirement:** Spec §3.8 walk/resume correctness; DoD-5 no duplicate paths; inventory authoritative on `(source_id, path)`  
**Location:** `C:\dev\Dedupe\crates\extract-pst\src\extract.rs` ~276–285, ~495–527  

**Problem:** Skip only when `status == extracted && logical_hash.is_some()`. Any prior row with `partial` / `error` / missing hash is **not** skipped, and `extract_one_message` always **`insert_item`** (never update-in-place). There is still no unique `(source_id, path)` index (deferred 0017), so a second job / re-walk creates a second parent (+ family/children) for the same logical path. `item_by_source_path` returns the **oldest** row only → further skips may still leave orphans.

**Evidence:**

```276:285:C:\dev\Dedupe\crates\extract-pst\src\extract.rs
            // Skip already-extracted.
            if let Some(existing) = matter.item_by_source_path(source_id, &msg_path)? {
                if existing.status == item_status::EXTRACTED && existing.logical_hash.is_some() {
                    // ... continue
                }
            }
```

No branch updates `existing.id`. Mid-folder **resume-by-index** avoids re-visiting completed NIDs, so DoD-5 cancel/resume is mostly safe; **full re-run** of `extract_pst_item` on a PST that already has `partial` parents is not.

**Failure scenario:** Attachment missing on first pass → parent `partial` with hash. Operator re-runs extract on same inventory item → second parent path, two families, ambiguous review inventory.

**Correction:** If `item_by_source_path` hits an existing message path: **update** that item (and children) or skip all terminal statuses (`extracted`/`partial` with hash) and only re-drive true failures with explicit “retry” policy. Prefer upsert-by-path for parents/attaches.

**Verification:** Integration: extract with forced attach failure → re-run extract → assert single parent path; optional retry clears error.

**Deferrable:** No

---

### [P2] `extract_pst_path` buffers the entire PST in RAM

**Confidence:** High  
**Requirement:** Spec §3.3 path entry; risks table multi-GB; streaming culture of §3.5.1 / DoD-6–7  
**Location:** `C:\dev\Dedupe\crates\extract-pst\src\extract.rs` ~117–146  

**Problem:** Public entry `extract_pst_path` does `read_to_end` + `put_bytes` for the whole file. Multi-GB operator PSTs OOM before extract. After CAS put, open still prefers the **filesystem** path via `candidate_fs_path`, so the full buffer was unnecessary for open.

**Evidence:**

```132:135:C:\dev\Dedupe\crates\extract-pst\src\extract.rs
    let mut file = fs::File::open(path.as_std_path())?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let digest = matter.put_bytes(&buf)?;
```

**Failure scenario:** Desk points at a 20 GB PST on disk → process OOM during “register inventory.”

**Correction:** Stream `File` into `put_reader` (or hash while reading), set `size_bytes` from metadata; open via FS path without re-materializing. Document that inventory digest is optional when FS path remains valid.

**Verification:** Unit/integration with multi-chunk file stream; assert digest matches `put_bytes` of same content without requiring a single large `Vec` in the path API.

**Deferrable:** No (public API OOM)

---

### [P2] `max_messages` early exit marks job `Succeeded` and is not resume-capable

**Confidence:** High  
**Requirement:** Spec §3.11 `ExtractSummary.completed` = finished fully; §3.8 cancel/resume honesty  
**Location:** `C:\dev\Dedupe\crates\extract-pst\src\extract.rs` ~268–271, ~343–351  

**Problem:** Hitting `limits.max_messages` breaks the walk with `cancelled == false` → `completed: true` → `JobState::Succeeded` + `extract.complete`. Remaining messages are not processed. `resume_extract` rejects `Succeeded` jobs. Safety-cap / test cap therefore **silently incomplete** with no durable “paused incomplete” state.

**Evidence:** Happy-path test itself uses `max_messages: Some(20)` and asserts `summary.completed` — encoding the wrong contract into CI.

**Failure scenario:** Operator sets `max_messages` as a safety rail on a large PST; job “succeeds” after N messages; UI shows complete; resume fails; inventory under-extracts without error.

**Correction:** Treat max-cap like soft stop: `completed: false`, job `Paused` (or distinct state), write checkpoint, do **not** emit success-complete audit (or emit `extract.paused` with reason `max_messages`). Tests that want a short run should assert incomplete **or** use a tiny fixture without claiming full completion of the PST.

**Verification:** `max_messages=1` on multi-message PST → not Succeeded; checkpoint present; `resume_extract` continues.

**Deferrable:** No

---

### [P2] Spec §3.12.3 partial-error integration test missing; resume test may not exercise `resume_extract`

**Confidence:** High  
**Requirement:** Spec §3.12.2–3, DoD-5, DoD-8  
**Location:** `C:\dev\Dedupe\crates\extract-pst\tests\integration.rs` ~198–253; no partial-inject test  

**Problem:**
1. No test forces one bad NID / attach failure while proving other messages extract and `item_errors` rows exist.
2. `resume_mid_folder_no_duplicates` only calls `resume_extract` when `first.cancelled`; otherwise runs a second full extract. Fixtures/cancel timing can pass without ever testing mid-folder resume or checkpoint fields.

**Evidence:** Attach/message error handling exists in production (`attach_data_missing`, parent `partial`) but is unproven end-to-end. Resume branch:

```231:238:C:\dev\Dedupe\crates\extract-pst\tests\integration.rs
    if first.cancelled {
        let resumed = resume_extract(...).expect("resume");
        ...
    } else {
        let _ = extract_pst_item(...).expect("second");
    }
```

**Correction:**
- Partial test: cap `max_attachment_bytes` very low **or** inject unreadable attach / corrupt NID path → assert ≥1 `item_errors`, ≥1 successful sibling message, parent `partial` when applicable.
- Resume test: assert `first.cancelled`, assert checkpoint `folder_message_index` / `last_message_nid`, call `resume_extract`, assert path uniqueness and progress beyond checkpoint.

**Verification:** Tests fail if resume/partial paths are deleted.

**Deferrable:** No (required tests)

---

### [P3] Cancel before first checkpoint yields non-resumable `Paused` job

**Confidence:** Medium  
**Requirement:** Spec §3.8 cancel → durable checkpoint  
**Location:** `extract.rs` cancel check before first message; `write_checkpoint` only after progress; `resume_extract` requires checkpoint  

**Problem:** Cancel on the first cancel-poll (before any message completes) sets `Paused` with **no** `pst_extract` checkpoint → `resume_extract` returns `InvalidJob: no checkpoint`.

**Correction:** Write an initial cursor checkpoint when the job starts (or on cancel always).

**Verification:** Cancel immediately → checkpoint exists → resume no-ops or continues cleanly.

**Deferrable:** Yes (edge; fix is cheap — prefer fix)

---

### [P3] Residual full-`Vec` fallback in `open_attachment_data`

**Confidence:** Medium  
**Requirement:** DoD-7 production path  
**Location:** `C:\dev\Dedupe\crates\pst-reader\src\messaging\attachment.rs` ~231–255  

**Problem:** After streaming resolve fails, code may `read_subnode_data` (full `Vec`) then branch on `data.len() <= 16 MiB`. For a live multi-GB path this would OOM before falling through to stream. Primary path uses `resolve_subnode_data_stream` / `open_block_stream` first (good); this fallback is largely redundant and dangerous if ever hit.

**Correction:** Remove full-buffer fallback; only stream leaf bids.

**Verification:** Code review + optional large synthetic subnode fixture.

**Deferrable:** Yes if primary stream path covers real fixtures (document residual)

---

## Completeness sweep

| Check | Result |
|---|---|
| Placeholders / TODO / todo! in extract-pst | None found |
| EML as native_sha256 | Not used; docs forbid |
| `%TEMP%` evidence materialize | Not used; explicit refuse if under OS temp |
| Silent empty success on open fail | Fail job + `extract.fail` + structured code |
| Invented digests on missing attach | No; child `error`, parent `partial` |
| Invented BCC | No |
| Soft-skipped tests without fixture | Yes — message-bearing PST required; fixtures exist under `fixtures/` |
| Orchestrator gates DoD-10/11 | Not done in R1 |

---

## Wiring and regression notes

**Happy path (intended):**  
inventory PST item → `extract_pst_item` → job `extract_pst` → `open_pst` (FS|CAS temp) → `folders` → per NID `read_message_extract` → body CAS → family → attach stream → native v1 CAS → `compute_email_logical_hash` → update parent → mid-folder checkpoint → audit complete.

**matter-core additions (0018):** `WORKSPACE_*`, `cleanup_workspace_temp` on create/open, `Cas::put_reader` / `Matter::put_reader`, documented collision/idempotent digest policy.

**pst-reader additions:** `ExtractedMessage` / `read_message_extract` (full body, DisplayCc/Bcc, delivery, HTML), `list_attachments` / `AttachmentInfo`, `open_attachment_data` / `AttachmentDataReader`, `filetime_to_rfc3339`. CLI `read_message_properties` still 4KB preview (no regression intent).

**Regression risks to watch:** large body/HTML still fully in RAM for hash/native; `extract_pst_path` OOM; duplicate items on re-extract; `max_messages` false success.

---

## Verification evidence

| Command | Observed now |
|---|---|
| `cargo test -p extract-pst` | **Not run** (no shell in this reviewer session) |
| `cargo test -p matter-core --test integration put_reader` | **Not run**; static: `put_reader_multi_chunk_matches_put_bytes` + integration `put_reader_multi_chunk_matches_put_bytes_via_matter` present |
| `git log main..HEAD --oneline` | From reflog: `6f9c58cf` docs; `e717eeee` feat(0018) extract-pst + streaming CAS (HEAD) |
| `git diff main...HEAD --stat` | **Not observed** (no shell); working tree expected to include extract-pst + matter-core + pst-reader + docs |

Prior compile artifacts under `target/debug/deps/extract_pst-*.exe` indicate the crate has been built on this machine, not that tests passed.

**Recommended orchestrator gates after fixes:**

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-reader
cargo test -p matter-core
cargo test -p extract-pst
cargo test --workspace
ledgerful verify
```

---

## Deferred candidates (reviewer proposes only)

| Item | Why possibly defer |
|---|---|
| P3 cancel-before-checkpoint | Edge; cheap fix preferred |
| P3 attachment full-Vec fallback | Residual; primary stream path OK for fixtures |
| MAPI recipient table vs Display* | Spec §3.14 optional |
| Secure wipe of temp | Spec optional |
| EML export | 0040 |

No P0. **P2s are not deferrable.**

---

## Summary

Implementation covers the architectural spine of 0018 well: native v1 custody identity, streaming attach→CAS, matter-local temp hygiene, BCC honesty, mid-folder checkpoint machinery, and documentation. **NEEDS_FIXES** before completion:

1. Upsert/skip policy so re-extract cannot duplicate paths for `partial`/retry rows.  
2. Stream (don’t `read_to_end`) in `extract_pst_path`.  
3. Honest incomplete handling for `max_messages` (and fix tests that assert false completion).  
4. Add partial-error + forced resume integration proofs.  
5. Run and record DoD-10 gates; then DoD-11 governance.

**Suggested verdict after fixes + green gates:** re-review → CLEAN (or PASS WITH DEFERRED P3 only if residual attach fallback left documented).
