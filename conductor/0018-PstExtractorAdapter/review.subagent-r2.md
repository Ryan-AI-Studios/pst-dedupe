# Internal Review R2 — 0018-PstExtractorAdapter

## Verdict: CLEAN

All four R1 **P2** findings are fixed in production code with matching tests and
docs at HEAD `34ef59dd` (`fix(0018): resume skip, stream path put, max_messages
pause, tests`) on `feat/0018-pst-extractor`. Fresh DoD engineering check finds
**no open P0/P1/P2**. Residual items are process gates (DoD-10/11) and optional
P3 nits only.

**Scope re-reviewed:**
- Prior: `conductor/0018-PstExtractorAdapter/review.subagent-r1.md`
- Claimed fix: `34ef59dd3aa7a2d3cc4b51d0f86556fe86919001` (branch HEAD)
- Branch history beyond main: `6f9c58cf` docs → `e717eeee` feat → `34ef59dd` fix
- `crates/extract-pst/**` (extract, open, limits, checkpoint, recipients, native,
  error, README, integration tests)
- Spot-check: `pst-reader` extract/attach surfaces; `matter-core` `put_reader` /
  workspace temp; root `README.md` / `ARCHITECTURE.md`; workspace `Cargo.toml`
- Spec §3 + §7 DoD; plan phases (static)

---

## Prior findings disposition

| R1 ID | Finding | Disposition | Evidence |
|---|---|---|---|
| **[P2]** Re-extract of non-`extracted` paths inserts duplicates | **Fixed** | Skip if **any** row exists for `(source_id, msg_path)` — not only `extracted`+hash (`extract.rs` ~284–295). Covers `partial` / `error` / other. No second `insert_item` for same message path. README documents skip + deferred retry-with-update. Integration: `resume_mid_folder_no_duplicates` re-runs `extract_pst_item` and asserts path-set size unchanged via `message_paths_unique`. |
| **[P2]** `extract_pst_path` buffers entire PST in RAM | **Fixed** | `extract_pst_path` opens `File`, streams via `matter.put_reader(&mut file)` — no `read_to_end` / full `Vec` (`extract.rs` ~117–150). Size from metadata. Open still prefers FS path. Integration: `extract_pst_path_streams_without_full_buf` asserts inventory digest equals a second `put_reader` of the same file. |
| **[P2]** `max_messages` early exit marks `Succeeded` | **Fixed** | `hit_max_messages` → `completed: false`, `JobState::Paused` reason `max_messages`, checkpoint written, audit `extract.paused` (not `extract.complete`) (`extract.rs` ~274–280, ~346–380). `limits.rs` / README document safety-cap contract. Happy path no longer claims complete under a cap (`max_messages: None`). Integration: `max_messages_pauses_incomplete_job` asserts Paused + checkpoint + successful `resume_extract` → Succeeded. |
| **[P2]** Partial-error integration missing; resume may skip `resume_extract` | **Fixed** | (1) `partial_attach_cap_records_errors`: `max_attachment_bytes: Some(0)` → `attach_too_large` / parent `partial` when attaches exist; always requires sibling emails + logical_hash when exercised. (2) `resume_mid_folder_no_duplicates` **requires** `first.cancelled`, asserts checkpoint `folder_message_index` / `last_message_nid`, **always** calls `resume_extract` (no full-extract fallback). |
| **[P3]** Cancel before first checkpoint non-resumable | **Improved / residual edge** | Cancel / max-cap path always `write_checkpoint` when `cancelled \|\| hit_max_messages` even if `since_batch == 0` (`extract.rs` ~342–344). Empty cursor still resume-safe (re-walk from start). Immediate cancel-before-any-message remains an edge, not a silent fail. |
| **[P3]** Residual full-`Vec` fallback in `open_attachment_data` | **Still present (deferrable)** | `pst-reader` `attachment.rs` ~231–255 still may `read_subnode_data` then size-branch. Primary path streams first. Documented residual; not a production default for fixtures. |

---

## DoD Matrix

| Requirement | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| **DoD-1 — Crate member** | **Met** | Workspace member `crates/extract-pst`; deps matter-core/pst-reader/camino/thiserror/serde/chrono/sha2; no Tokio. | Unit modules + integration compile artifacts under `target/debug/deps/extract_pst-*`. | **`cargo test -p extract-pst` not executed this session** (no shell tool). |
| **DoD-2 — Extract fixture → parents/children/families** | **Met** | Walk → parent `role=parent`, `file_category=email`, family `email_attachments`, attach children + roles; path `{pst}!/{folder}/{nid}` + attach suffix. | `happy_path_fixture_extract`: emails, hash, native, audit; family/attach asserts when fixture has attaches. | Soft fixture discovery if no message-bearing PST. |
| **DoD-3 — Identity** | **Met** | Parent native = `encode_native_message_v1` (`PNM1`+v1) → `put_bytes` → `native_sha256`; `extra_json.native_format = pst-native-message-v1`; logical via `compute_email_logical_hash` + version 1; attach via `put_reader`. README forbids EML native. | Golden `09b8a177…8160` unit + integration; happy path recompute. | — |
| **DoD-4 — Recipients / BCC** | **Met** | Display* parse; missing BCC → `[]`; always serialize `bcc_addrs_json` and pass `bcc` into logical input. | Unit empty/None/angles/`bcc_never_invented`; integration BCC changes hash. | — |
| **DoD-5 — Resume mid-folder** | **Met** | `batch_size` mid-folder + per-folder checkpoints; cursor fields; resume at `index+1`; skip any existing message path; cancel → Paused. | Forced cancel + checkpoint field assert + `resume_extract` + re-extract uniqueness. | Cancel poll threshold comment vs `>= 2` needs ≥3 messages (test hygiene P3). |
| **DoD-6 — CAS open + temp hygiene** | **Met** | FS prefer else CAS → `workspace/temp/{job}_{digest12}_{seq}_{pid}.pst`; refuse OS temp; RAII delete; Matter create/open cleanup. | CAS-only open + orphan cleanup + matter-core parity. | — |
| **DoD-7 — Streaming attach path** | **Met** (P3 residual) | Production: `open_attachment_data` → `Read` → `put_reader`. | matter-core multi-chunk + extract-pst `put_reader_parity_via_matter`. | Dead-ish full-Vec fallback in reader (P3). |
| **DoD-8 — Partial errors** | **Met** | Per-message continue; `item_errors` codes; parent `partial` on attach fail; ANSI/bad open → fail + `extract.fail`. | `partial_attach_cap_records_errors`; `ansi_or_bad_file_structured_fail`. | Soft note if fixture has zero attaches (then assert clean parents only). |
| **DoD-9 — Audit + docs** | **Met** | `extract.start` / `extract.complete` / `extract.fail` / (+ `extract.paused` for max cap). README blocking/native/stream/temp/mid-folder/BCC/max_messages. matter-core + root README + ARCHITECTURE. | Happy path `verify_audit_chain`. | README audit line omits `extract.paused` (nit). |
| **DoD-10 — Workspace gate** | **Not verifiable here** | Prior compile artifacts exist for extract-pst. | — | Orchestrator must capture fmt/clippy/`cargo test -p extract-pst` (+ matter-core/pst-reader)/workspace/`ledgerful verify`. |
| **DoD-11 — Recorded** | **Unmet** (expected pre-finalize) | No canonical `review.md`; conductor still **Ready**; no ledger TX observed. | — | Finalize after gates. |
| Spec §3.1–3.3 crate/open/entries | **Met** | Member; FS+CAS open; `extract_pst_item` / `_path` / `list_discovered_psts` / `resume_extract`; job kind/stage. | Constants + path/CAS tests. | — |
| Spec §3.5 native policy | **Met** | Not EML; not whole-PST as message native; not logical preimage as native. | Golden + extra_json. | — |
| Spec §3.7 body | **Met** | Full body via `read_message_extract`; `normalize_body` before hash; CLI path still 4KB preview. | Logical recompute from CAS body. | — |
| Spec §3.8 checkpoints | **Met** | Mid-folder batch + folder complete; max_messages honest pause; skip any path. | Resume + max_messages tests. | — |
| Spec §3.12 tests map | **Met** | 1 happy, 2 forced resume, 3 partial attach cap, 4 CAS open, 5 temp, 6 put_reader, 7 ANSI, 8–10 BCC/logical/native, plus max_messages + path stream. | See integration.rs. | Fixture soft-skips if no PST. |

---

## Findings (new)

**None at P0 / P1 / P2.**

### Residual nits (non-blocking; do not reopen R1 P2s)

1. **`resume_mid_folder_no_duplicates` cancel threshold** — `calls.fetch_add(1) >= 2` cancels before the **third** message poll; comment says “cancel before message 1”; guard is `total < 2` but effective need is **≥3** messages. Wrong threshold fails hard (good) rather than silent-pass (R1). Prefer `>= 1` after first message **or** `if total < 3`. Not a product defect.

2. **`partial_attach_cap_records_errors` soft path** — if preferred fixture has zero attachments, test notes and returns after asserting clean parents (no `attach_too_large` proof). Aspose fixtures typically have attaches; residual fixture risk only.

3. **pst-reader full-`Vec` attach fallback** — R1 P3 still in tree; primary stream path unchanged.

4. **README audit list** — does not mention `extract.paused` added for max_messages honesty.

5. **Package tests not observed this session** — static + artifact evidence only for DoD-1/10.

---

## Completeness sweep

| Check | Result |
|---|---|
| TODO / FIXME / todo! / unimplemented! in extract-pst | **None** |
| EML as `native_sha256` | **Not used**; docs forbid |
| `%TEMP%` evidence materialize | **Not used**; refuse if under OS temp |
| Silent empty success on open fail | **Fail** job + `extract.fail` + structured code |
| Invented digests on missing attach | **No**; child `error`, parent `partial` |
| Invented BCC | **No** |
| `extract_pst_path` full-file RAM buffer | **Removed** (`put_reader`) |
| `max_messages` false `Succeeded` | **Removed** (Paused + incomplete) |
| Re-extract duplicates for partial paths | **Removed** (skip any existing path) |
| Soft-skipped tests without fixture | Yes — message-bearing PST required; fixtures present under `fixtures/` |
| Orchestrator gates DoD-10/11 | Not done in R2 |

---

## Wiring and regression review

**Happy path (unchanged spine):**  
inventory PST → `extract_pst_item` → job `extract_pst` → `open_pst` (FS \| CAS temp) → folders → `read_message_extract` → body CAS → family → attach stream → native v1 CAS → `compute_email_logical_hash` → update parent → mid-folder checkpoint → audit complete.

**Fix-pass wiring:**
- Path entry: FS open → **stream CAS put** → inventory row → same extract loop.
- Re-walk / new job: existing `…!/` message path → **skip** (no second insert).
- Cap: mid-walk `run_messages >= max` → **Paused** + checkpoint + `extract.paused` → `resume_extract` continues (Paused allowed).

**Regression watch (static, no new P2):**
- Large body/HTML still fully in RAM for hash/native (documented threshold/stream for large puts; acceptable for P0).
- Skip-all-existing defers true retry-with-update (documented; correct for custody uniqueness until 0017 unique path).
- CLI `read_message_properties` still 4KB preview — intentional non-regression.

**Consumers:** no CLI/GUI DoD wiring required; 0019 will call blocking APIs.

---

## Verification evidence

| Command / check | Observed this session |
|---|---|
| Static re-read of R1 + HEAD sources/tests/docs | **Yes** |
| Git HEAD `feat/0018-pst-extractor` | **`34ef59dd`** (matches claimed fix) |
| Commit message / parent chain | `34ef59dd` fix ← `e717eeee` feat ← `6f9c58cf` docs |
| `cargo test -p extract-pst` | **Not run** (no shell available to this reviewer) |
| `cargo test -p matter-core` / `pst-reader` | **Not run** |
| fmt / clippy / workspace / `ledgerful verify` | **Not run** |
| Prior `target/debug/deps/extract_pst-*` artifacts | Present (prior builds only) |

**Recommended orchestrator gates before finalize:**

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
| P3 residual attach full-`Vec` fallback | Primary stream path covers real fixtures; optional cleanup |
| P3 resume-test cancel threshold hygiene | Test-only; fails closed if fixture too small |
| Retry-with-update for `partial`/`error` rows | Documented deferred until unique `(source_id, path)` upsert (0017 deferred) |
| MAPI recipient table vs Display* | Spec §3.14 optional |
| Secure wipe of temp | Spec optional |
| EML export | 0040 |

No P0. **No open P2.**

---

## Summary

R2 closes the R1 blocking set:

1. **Path uniqueness:** re-extract/resume never double-inserts an existing message path (any status).  
2. **`extract_pst_path`:** streams into CAS via `put_reader` (no full-file `Vec`).  
3. **`max_messages`:** honest incomplete → `Paused` + checkpoint + resumable; tests fixed.  
4. **Proof:** forced mid-folder `resume_extract` + partial attach-cap integration (+ path-stream test).

Engineering DoD-1…9 are **met** on static evidence. DoD-10/11 remain orchestrator process steps.

**Verdict: CLEAN** — no open P0/P1/P2; ready for package gates + canonical `review.md` finalize after green `cargo test -p extract-pst` (and workspace/`ledgerful verify`).
