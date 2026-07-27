# 0073 — Export Attachment Failure Ledger

- **Track ID:** 0073-ExportAttachmentFailureLedger  
- **Execution repo:** `C:\dev\dedupe`  
- **Governance:** this directory in `C:\dev\dedupe\conductor\`  
- **Plan-of-record:** Series L — Unique export hardening (post-0072 / INC0102784 lessons)  
- **Status:** **Ready** — architectural review folds accepted 2026-07-26 (§3.11); scale/CSV-security folds 2026-07-26 (§3.4.5–3.4.7, §3.3.1)  
- **Depends on:** Hard **0071** (`unique_export_report_v1` + unique-pst). Soft **0065** integrity reasons, **0066** keep-set promote, **0069/0070** writer fidelity/streaming. Soft **0074** deep attach preflight (shared reason codes; not blocking).  
- **Downstream:** Operator remediation; **0074** join codes; **0077** noise control; **0078** exit codes; **0081** runbook.  
- **Priority:** **P0 Series L** (operator cannot act on “366 failed” without inventory).  
- **Evidence:** INC0102784 unique-pst 2026-07-26 — 18,609 attaches written, **366 failed**, exit 1; report pack had **no per-attach ledger**. Synthetic fixtures only in CI; no client data in git.  
- **Deferred ledger:** append **D-0073-***; do not invent ScanPST repair.

---

## 1. Objective

Make every **soft-failed** (and policy-omitted) attachment on the unique-export path **identifiable, classifiable, joinable, and remediable** so operators can re-export, promote peers, or document exceptions — instead of a single aggregate count.

| Capability | P0 |
|---|---|
| **Stable reason codes** | Expand writer fidelity beyond Depth/EmbeddedUnparsed; map all `attachments_failed++` paths |
| **Streamed attach ledger** | `export_attachments.csv` under report-dir with **locus keys** (not subject-only) |
| **Counter ≡ rows invariant** | `attachments_failed == count(fail-severity ledger rows)` |
| **summary.json histogram** | `attachments_failed_by_reason` (+ ledger path pointer) |
| **Partial message honesty** | Message-level incomplete flag or mandatory join docs + preferred additive column |
| **Promote (narrow)** | Pre-write promote optional; **no** silent mid-volume half-message (§3.6) |
| **unique-eml** | Soft residual or minimal parity (§3.8) |

**Outcome:** After unique-pst, counsel opens `export_attachments.csv` + summary histogram and knows *which* attaches failed *why*, joinable to `export_messages.csv` / decisions.

**Industry anchors (researched 2026-07):**

- Microsoft Purview eDiscovery process reports: per-item **Status + ErrorWarning** in Items.csv — inventory, not aggregate-only.  
- eDiscovery exception handling: log path, name, size/hash metadata, exception description; deliver exceptions as a first-class report.  
- MS-OXCMSG **PidTagAttachMethod**: BY_VALUE (1), EMBEDDED_MSG (5), BY_REFERENCE / OLE / web — remediation differs (corruption vs non-portable method).  
- ScanPST repairs structure; tools must **not** claim repair — inventory + operator re-export.  
- No attachment **bytes** or message bodies in default reports (PII / privilege).

---

## 2. Context (ground truth)

### 2.1 What exists today

| Layer | State |
|---|---|
| `AttachmentFidelityKind` | Only `DepthLimitExceeded`, `EmbeddedUnparsed` |
| `AttachmentFidelityEvent` | `message_subject` + `attach_filename` only — **not joinable** |
| Silent `attachments_failed++` | Unsupported method; resolve payload None; mid-stream read Err — **no event** |
| `attachments_omitted_by_policy` | Separate counter for `parents_only` (must stay non-fail) |
| Report pack (0071) | `summary.json`, `decisions.csv`, `export_messages.csv`, volumes, keepset — **no attach ledger** |
| Exit honesty | `attachments_failed > 0` → `ok=false` + non-zero (unique_pst tests) |
| Integrity (0065) | `ATTACH_META_FAILED` + CRC/block reasons at scan/materialize |
| Keep-set (0066) | `promoted_from_failure` on **materialize hard fail** — not writer soft-fail |
| eml_pack (0067) | Soft-skip attaches; count only; tracing |
| INC pain | 366 fails + ~246 MB CRC stderr noise; one-line attach summary |

### 2.2 Gaps this track closes

1. No durable per-attach inventory after unique-pst.  
2. Fidelity events lack locus and most fail paths.  
3. Operators cannot distinguish corrupt stream vs unsupported method vs policy omit.  
4. Promote-on-attach-fail is under-specified vs streaming writer atomicity.

### 2.3 Product rules (LOCKED)

1. **Source PSTs read-only** — never repair/mutate sources.  
2. **One ledger row per attach outcome of interest** — never one row per CRC page (0077).  
3. **No attach bytes / body text** in ledger or summary.  
4. **Omit ≠ fail** — `parents_only` / policy omit is severity `info`, does not increment `attachments_failed` or force `ok=false`.  
5. **Valid zero-byte by-value** (`data: Some([])` or empty stream) is **success**, not fail (0069 lock).  
6. **UI thread N/A** — CLI/headless first; GUI residual.  
7. **Exit policy unchanged** unless **0078** says otherwise — attach fails still `ok=false`.  
8. **CSV spreadsheet-safe** — neutralize formula-leading free-text (§3.4.5).  
9. **CSV bounded** — default row cap; histogram never truncated (§3.4.7).  
10. **Writer critical path** — no fsync-per-fail-row (§3.4.6).

### 2.4 Deferred roll-in

| Item | 0073 action |
|---|---|
| Deep stream preflight before export | **0074** — share reason code strings |
| CRC stderr noise | **0077** — ledger is not a dump of page CRCs |
| Exit code taxonomy | **0078** — keep 0071 attach→non-zero |
| Materialize perf | **0079** |
| GUI wizard attach summary | Residual **D-0073-gui** |
| Matter produce Concordance attach exceptions | Out (0040 path separate) |

---

## 3. In scope

### 3.1 Placement (LOCKED)

| Component | Location |
|---|---|
| Reason enum + event DTO + sink | `pst-writer` (expand `AttachmentFidelityKind` / rename to export attach reason; keep public API honest) |
| Locus on write path | Materialize → `WriteMessage` / `WriteAttachment` (or side map keyed by write order) |
| Stream ledger CSV | `pst-dedup-cli` `unique_export_report` + unique-pst flush |
| summary.json fields | Additive under `unique_export_report_v1` (§3.5) |
| Optional pre-write promote | keep-set / unique-pst orchestration (§3.6) |
| Tests | `pst-writer` fidelity + `pst-dedup-cli` unique_pst |
| Docs | `docs/unique-pst-export.md`, fidelity doc, CHANGELOG note |

### 3.2 Stable reason codes (LOCKED)

String codes are the public API (CSV + summary). Align with 0065 style (`SCREAMING_SNAKE`). Map reader/`PstError` via `reason_from_pst_error` where applicable.

| Code | Severity | When |
|---|---|---|
| `ATTACH_METHOD_UNSUPPORTED` | fail | method ∉ {BY_VALUE=1, EMBEDDED_MSG=5}; store raw method int |
| `ATTACH_STREAM_OPEN_FAILED` | fail | cannot resolve/open payload (`Ok(None)` / open err) |
| `ATTACH_STREAM_READ_FAILED` | fail | mid-stream I/O while writing chain |
| `ATTACH_STREAM_CRC` | fail | CRC mismatch surfaced on attach stream (if distinguishable) |
| `ATTACH_BLOCK_NOT_FOUND` | fail | block missing on attach path |
| `ATTACH_DATA_TRUNCATED` | fail | truncated attach data |
| `ATTACH_SIZE_CAP` | fail | only if a documented size cap rejects the attach |
| `ATTACH_DEPTH_LIMIT` | fail | was `DepthLimitExceeded` |
| `ATTACH_EMBEDDED_UNPARSED` | fail | was `EmbeddedUnparsed` |
| `ATTACH_META_FAILED` | fail | materialize/list attach meta (align 0065) when surfaced at export |
| `ATTACH_OMITTED_BY_POLICY` | **info** | `parents_only` / family omit — **not** in `attachments_failed` |
| `ATTACH_UNKNOWN` | fail | last-resort; must be rare; test coverage pressures this down |
| `ATTACH_LEDGER_TRUNCATED` | **info** | CSV row cap hit (§3.4.7); not an attach failure itself |

**Remediation buckets (docs / summary optional group):**

| Bucket | Codes (examples) | Operator action |
|---|---|---|
| Corrupt / unreadable | CRC, block, truncated, stream open/read | Re-export source; ScanPST (external); do not claim in-tool repair |
| Non-portable method | METHOD_UNSUPPORTED | EML path, re-export with cloud content, or accept omit |
| Fidelity limit | DEPTH_LIMIT, EMBEDDED_UNPARSED | Residual fidelity; peer promote if available |
| Policy | OMITTED_BY_POLICY | Expected under `parents_only` |

### 3.3 Locus identity (LOCKED)

**Problem:** `message_subject` + `filename` collide and do not join to `export_messages.csv`.

**Required columns** on every ledger row (fail or info):

| Column | Required | Notes |
|---|---|---|
| `source_id` | Yes | Stable 0-based index into `summary.inputs` / same input order as unique-pst CLI (join-safe; preferred for handoff) |
| `source_path` | Yes* | **Same encoding as `export_messages.csv`** for join (§3.3.1) |
| `folder_path` | Best-effort | Empty string if unknown; **CSV-injection sanitized** (§3.4.5) |
| `msg_nid` | Yes | Source message NID (u64 decimal) |
| `attach_nid` | Yes if known | Else attach index `attach_index` (0-based) **required** as fallback |
| `attach_index` | Yes | Always present for stable ordering |
| `filename` | Best-effort | Display; may be empty; **CSV-injection sanitized** (§3.4.5) |
| `size` | Best-effort | Declared or actual; empty if unknown |
| `attach_method` | Yes | Raw i32 (MS-OXCMSG); `-1` if unknown |
| `reason_code` | Yes | §3.2 |
| `severity` | Yes | `fail` \| `info` |
| `volume_path` | If written | Output volume when message committed; sanitized if string |
| `volume_index` | If written | 1-based volume index |
| `winner_promoted` | Yes | bool — true if this row is after a promote for the group |
| `peer_source_id` / `peer_msg_nid` | If promoted | Peer locus when known |
| `message_subject` | Optional | Display only — **not** primary key; sanitized |

**Threading:** Materializer / keep-set winners already know locus; export DTOs must carry source locus through to writer events. Do not invent synthetic NIDs that cannot join.

#### 3.3.1 Source path / PII honesty (LOCKED)

**Problem:** Absolute paths like `C:\exports\John_Doe_Terminated_Fraud_Investigation.pst` leak PII/privilege context when the report pack is handed to IT or a vendor. The same risk already exists on `export_messages.csv` (`source_path` column).

**Rules:**

1. **Join consistency first:** Attach ledger `source_path` MUST use the **same string** as `export_messages.csv` / keep-set locus for that input (no divergent basenames that break joins).  
2. **`source_id` required:** 0-based index into `summary.inputs` (and peer via `peer_source_id`). Primary join key for tooling that wants to avoid path strings.  
3. **Default path form:** Keep current unique-export provenance (normalized absolute paths as today) so re-run/orchestration still works.  
4. **Handoff risk (docs LOCKED):** Entire `report-dir` is **operator-sensitive** (paths, folder names, subjects, filenames). Document: do not post report packs to untrusted third parties without redaction; prefer sharing histogram + reason codes first.  
5. **Optional residual (not P0 blocker):** `--ledger-path-mode=full|basename` (or report-pack redaction pass in **0081**) that rewrites path columns for handoff **while keeping `source_id` stable**. If basename mode is shipped, it must apply to **both** `export_messages` and `export_attachments` or document that only id-join is valid.  
6. Never put full path into **reason_code** or free-text error blobs.

### 3.4 Streamed ledger + invariant (LOCKED)

#### 3.4.1 File

```text
{report-dir}/export_attachments.csv
```

- Streamed / batched write — **not** “build multi-million-row Vec then write”.  
- Optional in-memory Vec for unit tests / small runs only.  
- Writer → sink contract: see §3.4.6 (must not fsync-per-row on the critical path).

#### 3.4.2 CLI flag

```text
--attach-ledger full|summary-only|off
```

| Value | Behavior |
|---|---|
| `full` (default) | Stream/batched CSV + histogram in summary |
| `summary-only` | Histogram only; no CSV file |
| `off` | Neither CSV nor histogram fields (still count `attachments_failed` for exit honesty) |

Optional (if cheap): `--attach-ledger-max-rows <N>` override of §3.4.7 default.

#### 3.4.3 Invariant (LOCKED)

```text
For severity=fail accounting (always true):
  summary.export.attachments_failed == total fail events observed by writer
  histogram tallies include ALL fails (even after CSV truncation)

For CSV when mode=full and not truncated:
  count(CSV fail rows) == attachments_failed

When CSV truncated (§3.4.7):
  count(CSV fail rows) <= max_rows
  summary.export.attachment_ledger_truncated == true
  histogram + attachments_failed remain complete
```

CI must fail if any `attachments_failed++` path skips the **accounting sink** (histogram/counter). CSV may stop early under the row cap.

Inventory Phase A maps every `++` site in `production.rs` (and eml if in scope).

#### 3.4.4 Noise (LOCKED)

- **One row per attach**, never per page/block CRC.  
- Do not dump 246 MB of CRC lines into CSV (0077 owns stderr noise).

#### 3.4.5 CSV injection / spreadsheet safety (LOCKED)

**Problem:** eDiscovery corpora include phishing/malware. Attachment `filename` / `folder_path` / subject may start with `=`, `+`, `-`, or `@`. Opening `export_attachments.csv` in Excel can treat them as formulas (CSV injection / DDE-class risk).

**Rules (non-negotiable for ledger CSV and any new string columns this track adds):**

1. After normal RFC-style quoting (`csv_escape`), apply **formula neutralization**: if the field (leading whitespace stripped for the check) starts with `=`, `+`, `-`, or `@`, prefix with a single quote `'` (or a leading tab/space — pick one and test; **prefer `'`**).  
2. Apply to **all** free-text fields: `filename`, `folder_path`, `message_subject`, `source_path`, `volume_path`, peer path strings.  
3. Shared helper in `unique_export_report` (e.g. `csv_escape_cell`) preferred so `export_messages` can adopt the same helper in this track when touching that writer (soft: at least new attach ledger + any new columns; hard residual to retrofit all 0071 CSVs if scope blows).  
4. Unit tests: `=cmd|...`, `+1+1`, `@SUM(`, `-2+3` → neutralized; normal names unchanged.  
5. Document in unique-pst-export: open CSVs as text or trust neutralization; still treat report-dir as sensitive.

#### 3.4.6 Sink architecture — non-blocking / 0079-ready (LOCKED)

**Problem:** Synchronous `File::write` inside a per-attach `FnMut` stalls the PST write loop on every soft-fail and will not scale if **0079** parallelizes materialize/write (global lock on `FnMut`).

**Rules:**

1. **Critical path must not fsync-per-row.** The writer callback may only: (a) tally histogram/counters, and (b) **enqueue** an owned `AttachLedgerEvent` (cheap).  
2. **Preferred P0:** `std::sync::mpsc` (or equivalent) to a **dedicated ledger writer thread** that batches lines (e.g. BufWriter, flush every N rows or few ms) into `export_attachments.csv`.  
3. Acceptable P0 alternative: **thread-local / single-threaded** buffered queue drained at message boundaries **if** unique-pst remains single-writer — but the public sink type should be **`Send` of events** so 0079 can switch to channel without API break.  
4. **Do not** require `crossbeam` unless already a workspace dep; `std::sync::mpsc` is enough.  
5. On run end: flush + join writer thread before summary is finalized.  
6. Backpressure: if channel fills under cascade, **drop CSV enqueues** only after applying §3.4.7 cap (prefer cap first); never drop histogram/counter updates.  
7. Writer crate stays free of CLI paths — sink trait/`FnMut` injected by CLI.

#### 3.4.7 Disk exhaustion / CSV row cap (LOCKED)

**Problem:** Cascading corruption (or a future bug) could emit millions of fail events and fill the operator disk via streamed CSV. Cycle detection (0063) reduces infinite B-tree walks, but defense-in-depth still applies.

**Rules:**

1. Default **`MAX_ATTACH_LEDGER_ROWS = 500_000`** (fail + info rows that would be written to CSV). Configurable via flag if cheap.  
2. When cap is hit under `full` mode:  
   - Write **one final row** with `reason_code=ATTACH_LEDGER_TRUNCATED` (severity `info`), filename empty, other locus best-effort / zeros, and optional size field = rows dropped estimate if known.  
   - **Stop further CSV writes**.  
   - **Continue** incrementing `attachments_failed` + histogram for all subsequent fails.  
   - Set `summary.export.attachment_ledger_truncated = true` and `attachment_ledger_rows_written`.  
3. Cap does **not** silence exit honesty (`ok=false` still when fails > 0).  
4. Test: inject >cap synthetic events → truncated flag, final marker row, histogram complete, disk writer stopped.

### 3.5 Report schema (LOCKED) — additive v1

Prefer **additive** fields under existing `unique_export_report_v1` (do not break 0071/0072 consumers):

```json
"export": {
  "attachments_written": 18609,
  "attachments_failed": 366,
  "attachments_omitted_by_policy": 0,
  "attachments_failed_by_reason": {
    "ATTACH_STREAM_READ_FAILED": 200,
    "ATTACH_METHOD_UNSUPPORTED": 100,
    "ATTACH_EMBEDDED_UNPARSED": 66
  },
  "attachment_ledger": "export_attachments.csv",
  "attachment_ledger_mode": "full",
  "attachment_ledger_truncated": false,
  "attachment_ledger_rows_written": 366
}
```

Document in `docs/unique-pst-export.md` + CHANGELOG. If a hard break is ever needed, use `unique_export_report_v1_1` only with explicit migration note — default is additive v1.

### 3.6 Promote-on-attach-fail (LOCKED — narrow)

**Problem:** Writer soft-fails occur **after** keep-set winner lock and often **during** streaming write. Mid-message promote without rollback leaves half-written objects.

| Mode | P0? | Behavior |
|---|---|---|
| **A. Pre-write promote (preferred)** | Yes if flag on | Before streaming write commits a winner family: if materialize/preflight marks attach incomplete **and** a peer exists, promote next peer by **same keep-set policy order**; set `promoted_from_failure` / decisions note `promote_reason=attach_incomplete` |
| **B. Write-time promote** | **Out / residual** | Soft-fail mid-write → abort message + promote + rewrite — requires message atomicity not guaranteed for P0 |
| **C. Ledger-only** | Always | Flag off or no peer: write best-effort message, ledger fails, `ok=false` |

**CLI:**

```text
--promote-on-attach-fail
```

- Default: **off** (preserve today’s determinism).  
- When on: **Mode A only** for P0.  
- Peer order: existing keep-set peer list (first_seen / policy) — **deterministic**.  
- decisions.csv must record promotion (extend notes/columns as needed).  
- Interaction with **0074**: deep preflight should feed the same incomplete signal; 0073 must work with materialize-level incomplete flags even before 0074 lands.  
- Interaction with **0075**: do not invent a second policy engine.

### 3.7 Partial message honesty (LOCKED)

A message may be written with **N−k** attaches missing while still appearing in `export_messages.csv`.

**P0 (minimum):** Document left-join: attach completeness = `export_messages` ⟕ `export_attachments` (fail rows for that msg_nid).

**P0 (preferred, low cost):** Additive column on `export_messages.csv`:

```text
attachments_failed_count
```

(or `attach_incomplete` bool). Bump column list in docs; keep prior columns prefix-stable if parsers are column-name based; if fixed-order parsers exist, append only at end and document.

### 3.8 unique-eml parity (LOCKED soft)

| Path | 0073 requirement |
|---|---|
| **unique-pst** | Full ledger + histogram (P0) |
| **unique-eml** | **Either** (a) emit attach skip rows into pack manifest / side CSV with same reason codes, **or** (b) residual **D-0073-eml** with explicit honesty in docs |
| **matter-produce** | Out — separate product surface |

### 3.9 Writer API changes (LOCKED summary)

1. Expand reason kind enum; deprecate 2-variant-only mental model.  
2. Expand event with §3.3 fields (subject optional).  
3. Emit event (sink) on **every** fail and info omit path in `write_one_attachment` / parents_only.  
4. Prefer streaming sink over sole `Vec` accumulation for production unique-pst.  
5. Keep `WritePstReport` counters; Vec events optional for tests.

### 3.10 Evidence & hygiene (LOCKED)

- CI: synthetic soft-fail fixtures only.  
- No client PST paths, bodies, or attach bytes in git/logs/review.  
- Operator INC re-smoke optional; document synthetic equivalent in review.md.  
- Filenames/paths in CSV are operator-local sensitive — report-dir is not committed (§3.3.1).  
- CSV injection neutralization on free-text cells (§3.4.5).

### 3.11 Review folds accepted (LOCKED summary)

| # | Fold | Spec | Disposition |
|---|---|---|---|
| 1 | Locus keys + joinable columns (not subject-only) | §3.3 | Accepted (prior) |
| 2 | Full reason taxonomy; omit ≠ fail; zero-byte success | §3.2, §2.3 | Accepted (prior) |
| 3 | Every `++` → accounting sink; histogram complete | §3.4.3 | Accepted (prior; refined for truncation) |
| 4 | Stream CSV + histogram; default full; no per-page CRC | §3.4 | Accepted (prior) |
| 5 | Additive `unique_export_report_v1` fields | §3.5 | Accepted (prior) |
| 6 | Promote = pre-write only; write-time residual | §3.6 | Accepted (prior) |
| 7 | Partial message honesty | §3.7 | Accepted (prior) |
| 8 | unique-eml soft residual or minimal parity | §3.8 | Accepted (prior) |
| 9 | Method vs corruption remediation buckets | §3.2 | Accepted (prior) |
| 10 | Exit honesty unchanged (0078 owns taxonomy) | §2.3 | Accepted (prior) |
| 11 | **CSV injection neutralization** (`=+\-@`) | §3.4.5 | **Accepted** (must) |
| 12 | **CSV row cap + TRUNCATED marker**; histogram continues | §3.4.7 | **Accepted** (must; default 500k) |
| 13 | **Non-blocking sink** — enqueue + batch writer thread; 0079-ready `Send` | §3.4.6 | **Accepted** (prefer mpsc; no hard crossbeam pin) |
| 14 | **source_id + path PII honesty**; join matches export_messages | §3.3.1 | **Accepted** (id required; full path default + handoff docs; basename mode residual) |

---

## 4. Out of scope

- In-place source CRC repair / ScanPST automation.  
- Write-time mid-message promote with layout rollback (Mode B).  
- Parallel materialize (**0079**).  
- Full Connected Desk attach UI.  
- Matter Concordance production exception packs (**0040**).  
- Changing attach→exit policy without **0078**.  
- Dumping per-block CRC into the ledger (**0077**).

---

## 5. Preconditions & dependencies

- **P0 (blocking):** 0071 unique-pst report pack + `export_messages.csv` writer.  
- **P0 (blocking):** `pst-writer` soft-fail paths inventory.  
- **Soft:** 0065/0066 reason + promote primitives.  
- **Soft:** 0074 for earlier incomplete signal (share codes).  
- *Verified research 2026-07-26:* silent `++` without events at method skip / resolve None / stream Err; events subject+filename only; omit counter separate.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Unjoinable ledger | Locus columns + source_id LOCKED §3.3 |
| Silent `++` regressions | Accounting invariant §3.4.3 |
| OOM from Vec events | Enqueue + batch §3.4.6 |
| CSV injection (Excel) | Neutralize `=+\-@` §3.4.5 |
| Disk fill from cascade | Row cap + TRUNCATED §3.4.7 |
| Writer stalled on disk | Background ledger thread §3.4.6 |
| Path PII on handoff | source_id + sensitive report-dir docs §3.3.1 |
| Promote breaks determinism | Default off; Mode A only; decisions.csv |
| Mid-write half message | Mode B residual |
| Schema break 0071/0072 | Additive v1 only §3.5 |
| CRC noise in CSV | One row/attach §3.4.4 |
| Conflating omit with fail | severity=info; separate counters |
| unique-eml forgotten | §3.8 residual or parity |

---

## 7. Definition of Done

Complete only when **all** hold:

- [ ] **DoD-1 — Taxonomy:** Stable reason codes §3.2 implemented; every former silent `attachments_failed++` path classified.  
- [ ] **DoD-2 — Locus events:** Events carry source_path, msg_nid, attach_index/(attach_nid), method, reason, severity.  
- [ ] **DoD-3 — Ledger file:** unique-pst with `--attach-ledger=full` writes batched/streamed `export_attachments.csv` via §3.4.6 sink.  
- [ ] **DoD-4 — Invariant:** Soft-fail fixtures: fail accounting == histogram; CSV fail rows == failed when not truncated.  
- [ ] **DoD-5 — Histogram:** `summary.json` includes `attachments_failed_by_reason` + ledger pointer + truncation fields (additive v1).  
- [ ] **DoD-6 — Omit ≠ fail:** `parents_only` does not increment `attachments_failed` or force fail-severity rows.  
- [ ] **DoD-7 — Zero-byte success:** Empty by-value attach still succeeds (regression).  
- [ ] **DoD-8 — Promote:** Mode A **or** residual **D-0073-promote**.  
- [ ] **DoD-9 — Partial honesty:** export_messages incomplete column **or** documented join + test.  
- [ ] **DoD-10 — unique-eml:** Parity **or** **D-0073-eml** residual.  
- [ ] **DoD-11 — Exit:** attach fails still `ok=false` / non-zero (0071).  
- [ ] **DoD-12 — CSV injection:** free-text cells neutralize leading `=+\-@` (§3.4.5); unit tests.  
- [ ] **DoD-13 — Row cap:** over-cap synthetic run → marker row, `attachment_ledger_truncated`, histogram complete.  
- [ ] **DoD-14 — source_id:** ledger rows include `source_id` joinable to `summary.inputs`.  
- [ ] **DoD-15 — Docs:** unique-pst-export + fidelity + reason→action + report-dir sensitivity + CSV open safety.  
- [ ] **DoD-16 — Tests:** §8 green; synthetic only.  
- [ ] **DoD-17 — Recorded:** `review.md`; registry **Completed**; deferred D-0073-*; ledger commit.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo test -p pst-writer --test writer_fidelity
cargo test -p pst-writer --test writer_streaming
cargo test -p pst-dedup-cli --test unique_pst
cargo clippy -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings
# Full gate before commit:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 8.1 Required test cases

| Case | Assert |
|---|---|
| Unsupported attach method | fail row + reason METHOD_UNSUPPORTED + method int |
| Stream open/read fail | fail row + STREAM_* code |
| Embedded depth / unparsed | fail row + existing mapped codes |
| Zero-byte by-value | success; failed unchanged |
| parents_only | omitted_by_policy++; no fail row; ok not forced by omit alone |
| Soft-fail fixture unique-pst | CSV exists; histogram; ok=false; invariant |
| summary-only mode | no CSV; histogram present |
| CSV injection cells | `=cmd|…`, `+1`, `@x`, `-2` neutralized |
| Row cap truncation | final ATTACH_LEDGER_TRUNCATED; truncated=true; histogram continues |
| source_id present | matches summary.inputs index |
| Promote Mode A (if shipped) | peer written; decisions note; winner_promoted |
| Stale invariant | force missing accounting sink → test fails (or exhaustive path inventory) |

---

## 9. Report pack layout (after 0073)

```text
{report-dir}/
  summary.json              # unique_export_report_v1 + attach histogram fields
  decisions.csv
  keepset.json
  volumes.csv
  export_messages.csv       # + preferred attachments_failed_count
  export_attachments.csv    # NEW (mode=full)
  integrity.csv             # optional
```

---

## 10. Handoff

Unblocks operator remediation on attach soft-fails. **0074** should reuse reason strings. **0077** must not reintroduce per-page noise into this CSV. **0081** maps reason → action for the runbook.
