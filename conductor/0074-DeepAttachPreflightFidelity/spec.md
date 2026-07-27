# 0074 — Deep Attach Preflight & Fidelity Bridge

- **Track ID:** 0074-DeepAttachPreflightFidelity  
- **Execution repo:** `C:\dev\dedupe`  
- **Governance:** this directory in `C:\dev\dedupe\conductor\`  
- **Plan-of-record:** Series L — Unique export hardening (post-0072 / INC0102784 lessons)  
- **Status:** **Ready** — architectural review folds accepted 2026-07-26 (§3.12); resource/DoS/cache folds 2026-07-26 (§3.4, §3.7, §3.9–3.10)  
- **Depends on:** Hard **0065** (scan integrity / preflight), **0066** (keep-set fidelity). Soft **0071** unique-pst. Soft **0073** attach reason strings (share vocabulary; 0074 may land first if codes frozen to match). Soft **0077** CRC noise (do not dump page CRC spam from probe).  
- **Downstream:** Earlier risk signal before multi-hour unique-pst; feeds 0073 Mode A promote; **0081** operator runbook.  
- **Priority:** **P0 Series L** (close scan-ok / export-fail attach gap).  
- **Evidence:** INC0102784 — scan preflight `ok`; unique-pst ~108k page CRC warns, **124** degraded winners, **366** attach fails. Synthetic fixtures only in CI; no client data in git.  
- **Deferred ledger:** append **D-0074-***; never auto-ScanPST or mutate sources.

---

## 1. Objective

Close the gap where **scan reports healthy preflight** but **unique-pst materialize/write** hits attachment stream CRC/page failures and degraded winners — by **budgeted** probing of attachment streams **before or at** keep-set materialize, merging results into integrity/fidelity, and escalating operator recommendations with **honest coverage limits**.

| Capability | P0 |
|---|---|
| **Budgeted deep probe** | Graded L0–L3; default L2 head-read under hard budgets (§3.3–3.4) |
| **Winner-first placement** | unique-pst / materialize winners-only probe; optional `scan` flag (§3.2) |
| **Shared reason codes** | Same strings as **0073** / additive `IntegrityReason` (§3.5) |
| **Preflight rates** | `attach_fail_rate` + threshold → `re_export_recommended` (§3.6) |
| **Keep-set fidelity** | Mark degraded so `fidelity_rank` prefers clean peers (§3.7) |
| **Honesty** | Truncation/coverage fields; not a full-export guarantee (§3.8) |
| **Safety** | No multi-GB `Vec`; sticky handles; progress + cancel (§3.9) |

**Outcome:** Operators can learn attach-stream risk **before** or **while** resolving winners — without equating preflight to a second full unique-pst, and without claiming every attach is verified.

**Industry anchors (researched 2026-07):**

- PST CRC/block corruption is common on eDiscovery exports; ScanPST diagnoses/repairs structure but can **alter metadata** — re-export from Purview/Exchange is preferred when rates are high.  
- Defensible QA uses **documented sampling limits**, not false perfection (EDRM / eDiscovery QC sampling practice).  
- Preflight is **risk estimation**; export still needs **0073** per-attach ledger for residual fails.

---

## 2. Context (ground truth)

### 2.1 What exists today

| Layer | State |
|---|---|
| Scan preflight | `skip_rate` / `crc_skip_rate` / `failed_file_rate` → `ok` \| `re_export_recommended` \| `not_export_ready` |
| Materialize | `list_attachments`; full read only if `size ≤ 64 KiB` (`SMALL_ATTACH_CAP`) |
| Large attaches | `stream_available ≈ size > 0 \|\| !filename.is_empty` — **optimistic**, no stream open |
| Soft meta fail | `ATTACH_META_FAILED` on list/open fail for small payloads only |
| Keep-set | `fidelity_rank`: non-degraded beats degraded; `merge_soft_fidelity` at materialize |
| Export | Large streams soft-fail at write (0073 ledger target) |
| INC gap | Scan ok → late attach fails + degraded winners |

### 2.2 Product rules (LOCKED)

1. **Source PSTs read-only** — never repair in-place; never auto-run ScanPST.  
2. **No multi-GB attach `Vec`** — probe streams chunked; discard bytes.  
3. **Budget before confidence** — default probe is **not** full-read of every attach.  
4. **Preflight ≠ export guarantee** — residual mid-tail corruption possible; 0073 still required.  
5. **Reason strings shared** with 0073 / 0065 style (`SCREAMING_SNAKE`).  
6. **`parents_only` / `--no-attachments`:** skip deep attach probe entirely.  
7. **Default feature gate:** deep probe **off** for plain `scan`; **on or explicit** for unique-pst winners path per §3.2.  
8. **Exit/export policy:** do not invent new exit codes (0078); preflight recommendation only unless unique-pst already fails closed.

### 2.3 Deferred roll-in

| Item | Action |
|---|---|
| 0073 attach ledger at export | Soft pair — share codes; 0074 does not replace ledger |
| 0077 CRC stderr noise | Probe uses structured counters; no per-page spam |
| 0079 parallel materialize | Sticky handles P0; parallel probe residual |
| GUI wizard checkbox | Residual **D-0074-gui** |
| Full statistical sample size tables (AQL) | Optional residual; budget + stratified size buckets enough for P0 |

---

## 3. In scope

### 3.1 Placement (LOCKED)

| Component | Location |
|---|---|
| Probe engine | Shared helper (e.g. `pst-dedup-cli` + thin `pst-reader` open/read) usable from scan + materializer |
| Integrity reasons | `dedup-engine` `IntegrityReason` additive + `reason_from_pst_error` map |
| Preflight math | `PreflightInputs` / `PreflightReport` additive attach fields |
| Winner probe | `PstMaterializer` / unique-pst finalize path |
| Optional scan probe | `scan` CLI flag |
| Keep-set merge | Existing `merge_soft_fidelity` / item integrity before or during resolve |
| Docs | unique-pst-export, scan integrity docs, 0081 cross-link |

### 3.2 Pipeline placement (LOCKED)

| Path | P0? | Behavior |
|---|---|---|
| **B. Winner-only (unique-pst / keep-set materialize)** | **Yes** | Probe attaches for **provisional or final winners** (and peers considered for promote) — best ROI vs INC export pain |
| **A. Full scan `--deep-attach-preflight`** | **Yes (optional flag)** | Probe attaches on recoverable scan items under same budgets — triage without export |
| **C. Both** | Soft | Cache by locus so A→B does not double I/O (§3.10) |

**CLI (illustrative):**

```text
# Scan triage (opt-in)
pst-dedup scan … --deep-attach-preflight [--deep-attach-level head|full] …

# Unique export (winner-only; recommended default when deep enabled)
pst-dedup unique-pst … --deep-attach-preflight …
```

Exact flag names may match existing clap style; document in help.

**Defaults (LOCKED intent):**

- Plain `scan`: deep attach **off** (fast day-1).  
- `unique-pst`: deep attach **off** by default **or** documented opt-in; if product chooses default-on for unique-pst only, must still honor budgets and cancel. Prefer **opt-in** for P0 to avoid surprise wall time; review.md records choice.

### 3.3 Graded probe depth (LOCKED)

| Level | Name | Action | Catches |
|---|---|---|---|
| **L0** | meta | `list_attachments` only (today) | list fail → `ATTACH_META_FAILED` |
| **L1** | open | Open stream handle; no read / minimal | open fail → `ATTACH_STREAM_OPEN_FAILED` |
| **L2** | head | Chunked read up to `per_attach_max_bytes`; discard | early CRC/block/truncation |
| **L3** | full | Full stream read under size/budget (still no fat Vec — streaming discard) | full chain; expensive |

**Default deep preflight level: L2 (`head`).**  
L3 only with explicit `--deep-attach-level full` (or equivalent) and still subject to total budgets.

**Never** `read_to_end` into an unbounded `Vec` for large attaches.

### 3.4 Budgets & sampling (LOCKED)

| Knob | Suggested default | Role |
|---|---|---|
| Feature gate | off (scan); opt-in unique-pst | Avoid surprise |
| `max_attaches` | e.g. **50_000** / run | Hard stop on attach count probed |
| `max_probe_bytes` | e.g. **256 MiB–1 GiB** total | Global I/O budget |
| `per_attach_max_bytes` | e.g. **1–4 MiB** (L2) | Head-read cap |
| `max_probe_time_ms` | e.g. **2000** / attach | Wall-clock abort per attach (§3.4.1) |
| `max_open_psts` | e.g. **32** | Bounded sticky handle LRU (§3.9.1) |
| `max_peer_probes_per_group` | e.g. **3** | Cap peer probes in one keep-set group (§3.7.1) |
| `timeout` / cancel | cooperative `AtomicBool` | Operator abort (run-level) |
| `sample_mode` | `all_under_budget` (deterministic order) | P0; optional `first_n` / size-stratified residual |

**On budget exhaust:**

1. Stop further probes.  
2. Set `attach_probe_truncated = true` (preflight + summary).  
3. Compute rates from **attempted** only.  
4. Do **not** claim full coverage.  
5. Recommendation may still escalate from observed fail rate.

Progress: `probed_attaches`, `probe_bytes`, current source (basename ok), level.

#### 3.4.1 Per-attach wall-clock + expansion honesty (LOCKED)

**Problem:** An unbounded per-attach read loop (pathological block chain, hangy I/O, or any future inflate path) can pin CPU/I/O and stall preflight even when `per_attach_max_bytes` is set.

**Rules:**

1. **`max_probe_time_ms` per attach** (default ~**2000 ms**): if exceeded during L1–L3, abort that attach immediately; count as probe fail with `ATTACH_STREAM_READ_FAILED` or `ATTACH_PROBE_TIMEOUT` (stable string; prefer dedicated code if distinct from I/O fail).  
2. **Byte budget remains primary** for raw PST BY_VALUE chains (pst-reader streams **stored** payload bytes — reading a `.zip` attachment does **not** auto-decompress it).  
3. **If** any code path inflates nested content during probe (embedded-msg walk, future zip peek, OLE expand): enforce a **max expansion ratio or max inflated bytes** and abort with the same fail class. Do **not** invent zip-bomb logic for pure discard of raw PST block bytes.  
4. Run-level cancel still aborts the whole probe pass.

### 3.5 Reason codes (LOCKED) — share with 0073

Public strings must match 0073 attach taxonomy where overlapping. Additive `IntegrityReason` variants (or map layer) with stable `as_str()`:

| Code | When (probe) |
|---|---|
| `ATTACH_META_FAILED` | list_attachments fail (existing) |
| `ATTACH_STREAM_OPEN_FAILED` | open fail |
| `ATTACH_STREAM_READ_FAILED` | mid-head/full read I/O |
| `ATTACH_STREAM_CRC` | CRC mismatch on attach stream (if distinguishable) |
| `ATTACH_BLOCK_NOT_FOUND` | block missing |
| `ATTACH_DATA_TRUNCATED` | truncated data |
| `ATTACH_METHOD_UNSUPPORTED` | if probe classifies method as non-portable (optional at preflight) |
| `ATTACH_PROBE_TRUNCATED` | **info** — global budget hit (not an attach fail) |
| `ATTACH_PROBE_TIMEOUT` | fail | per-attach wall-clock exceeded (§3.4.1) |
| `ATTACH_PEER_PROBE_CAP` | **info** | stopped peer walk after `max_peer_probes_per_group` (§3.7.1) |

Do **not** invent `ATTACH_STREAM_EOF` unless distinctly different from truncated/read-fail.  
Map via `reason_from_pst_error` where possible.

### 3.6 Preflight recommendation (LOCKED)

Extend pure preflight inputs/report (additive JSON under `scan_integrity_v1` or nested `attach_probe` object):

```json
"attach_probe": {
  "enabled": true,
  "level": "head",
  "attempted": 12000,
  "failed": 80,
  "truncated": false,
  "fail_rate": 0.0067,
  "max_attach_fail_rate": 0.05,
  "coverage_note": "budgeted L2 head-read; residual export ledger (0073)"
}
```

| Threshold | Default (document; configurable) |
|---|---|
| `max_attach_fail_rate` | **0.05** (5%) suggested; tune in docs |

**Escalation:** if `fail_rate > max_attach_fail_rate` and recommendation was `ok` → `re_export_recommended` + reason `attach_stream_fail_rate_exceeded`.  
Do not invent a fourth recommendation enum value unless existing three cannot express it.

**Strict mode:** LOCKED — deep-probe fail on an attach **degrades** the message; if mode is `strict`, treat like other degradations (skip message / fail closed consistent with existing strict body/CRC policy). Document exact match to 0065 strict semantics.

### 3.7 Keep-set fidelity bridge (LOCKED)

1. On probe fail: push reason into message/item `RecoverableIntegrity` (`degraded=true`).  
2. **Prefer probe before or during resolve** so `fidelity_rank` selects non-degraded peers when available.  
3. If probe runs only at materialize after provisional winner: merge via `merge_soft_fidelity` and use **0073 Mode A** promote when `--promote-on-attach-fail` (soft dependency).  
4. Correct optimistic flags:  
   - fail → do not claim stream exportable (`stream_available=false` or `stream_probe_failed`)  
   - L2 success → optional `stream_head_ok`; **not** “fully verified” unless L3  
5. All peers corrupt → escalate preflight; no infinite promote loops.

#### 3.7.1 Peer-probe cap per duplicate group (LOCKED)

**Problem:** A blast email with a corrupt shared attach across hundreds of custodians can burn the **global** `max_probe_bytes` / `max_attaches` budget probing peers in **one** keep-set group, leaving the rest of the corpus unprobed.

**Rules:**

1. **`max_peer_probes_per_group` default 3** (configurable): within one identity group, probe at most N candidates (winner order: fidelity → policy → path/nid).  
2. After N fails (or N attempts with no clean peer): **stop** peer probing for that group; keep best-effort winner as **degraded**; continue other groups.  
3. Record info tally / optional reason `ATTACH_PEER_PROBE_CAP` on the group or summary counter `peer_probe_capped_groups`.  
4. Aligns with 0073 Mode A: promote is bounded, not an exhaustive peer walk.  
5. Test: synthetic group with ≥5 dirty peers + 1 clean beyond cap — either finds clean within cap or caps and preserves global budget (inject counter assert).

### 3.8 Honesty / non-guarantee (LOCKED)

| Claim allowed | Claim forbidden |
|---|---|
| “Budgeted head-probe of N attaches; M failed; rate R” | “All attachments will export cleanly” |
| `recommendation: re_export_recommended` from attach rate | Silent `ok` when rate exceeds threshold |
| Residual mid-tail risk after L2 | Equating L2 with L3 full verification |

Docs: when rate high → **re-export from Purview/Exchange** preferred; ScanPST only on a **copy**, last resort, may change metadata.

### 3.9 Runtime safety (LOCKED)

1. Sticky handles via **bounded LRU only** (§3.9.1) — never unbounded open-all-sources.  
2. Cooperative cancel between attaches (and between chunks if cheap).  
3. Progress sink for CLI stderr / GUI log residual.  
4. Thread model: sequential P0; sink/types `Send`-ready for 0079 residual parallel.  
5. Probe must not print one log line per page CRC (counter + per-attach outcome only).  
6. Per-attach wall-clock (§3.4.1).

#### 3.9.1 Bounded sticky PST handle LRU (LOCKED)

**Problem:** Enterprise datasets often span **hundreds–thousands** of custodian PSTs. Caching one open handle per path can hit Windows/Linux FD limits (“Too many open files”) and abort preflight.

**Rules:**

1. Implement sticky opens as a **bounded LRU** (or equivalent) with hard capacity **`max_open_psts` default 32** (configurable).  
2. On capacity pressure: **drop least-recently-used** handle cleanly (`Drop`/close) before opening the next path.  
3. Applies to probe path and should be the pattern for materializer/export stream caches when they grow (align or residual to fix unbounded `HashMap` caches in the same track if touched).  
4. Reopen after eviction is allowed and expected — correctness over FD exhaustion.  
5. Test: open > capacity distinct synthetic paths (or mock) → live handles ≤ cap; no leak of closed entries.

### 3.10 Result cache (LOCKED soft / P1 if cheap)

**Problem:** Caching L1 `ok=true` and reusing it for a later L2 request **masks** head-read failures (cache poisoning across probe levels). Stale hits after an operator swaps a PST file are also wrong.

**Rules:**

1. Cache key **MUST** include **probe level** (or store `cached_level` and only hit when `cached_level >= requested_level` — L3 satisfies L2/L1; L1 never satisfies L2).  
2. Prefer also keying **source identity** with **file size + mtime** (or equivalent fingerprint) so a replaced PST at the same path does not serve stale results.  
3. Value: `{level, ok, reason, …}`.  
4. Same-process scan→unique-pst reuse only when key matches.  
5. Disk cache residual (not P0).  
6. If cache not shipped in P0: document “no cross-level reuse” so implementers do not add a naïve key later.

### 3.11 Tests (LOCKED)

| Case | Assert |
|---|---|
| Synthetic open/CRC fail fixture | reason code + degraded integrity |
| L2 head does not load multi-GB Vec | memory / mock stream |
| Budget exhaust | `truncated=true`; rates from attempted; no panic |
| Per-attach timeout | abort; fail reason; run continues |
| Winner path prefers clean peer | two peers; dirty attach loses fidelity rank |
| Peer probe cap | ≥N dirty peers; global budget not fully consumed by one group |
| LRU handle cap | >max_open_psts paths; live opens ≤ cap |
| Cache level dominance | L1 hit must not skip L2 request |
| `parents_only` | zero attach probes |
| Preflight rate | fail_rate above threshold → `re_export_recommended` |
| Cancel | stops promptly; partial tallies honest |
| Reason string | matches 0073 public names for overlapping codes |

### 3.12 Review folds accepted (LOCKED summary)

| # | Fold | Spec | Disposition |
|---|---|---|---|
| 1 | Budgeted L2 default; not unbounded full-read | §3.3–3.4 | Accepted (prior) |
| 2 | Winner-only unique-pst P0; optional scan flag | §3.2 | Accepted (prior) |
| 3 | Shared 0073/0065 reason strings | §3.5 | Accepted (prior) |
| 4 | attach_fail_rate + preflight escalation | §3.6 | Accepted (prior) |
| 5 | Merge degraded into keep-set fidelity | §3.7 | Accepted (prior) |
| 6 | Honesty / non-guarantee + ScanPST docs | §3.8 | Accepted (prior) |
| 7 | Progress/cancel; no CRC page spam | §3.9 | Accepted (prior) |
| 8 | Correct optimistic stream_available | §3.7 | Accepted (prior) |
| 9 | **Bounded LRU sticky handles** (`max_open_psts`) | §3.9.1 | **Accepted** |
| 10 | **Per-attach wall-clock**; inflate limits only if inflate exists | §3.4.1 | **Accepted** (timeout must; zip-bomb ratio only if decompression path) |
| 11 | **max_peer_probes_per_group** | §3.7.1 | **Accepted** |
| 12 | **Cache key includes level** (+ size/mtime); no L1→L2 poison | §3.10 | **Accepted** |

---

## 4. Out of scope

- Auto-running ScanPST or any source mutation.  
- Fixing corrupt source pages in-place.  
- Replacing 0073 export attach ledger.  
- Unbounded full-stream verify of every attach by default.  
- Parallel probe fleet (0079 residual).  
- GUI checkbox (residual).  
- Matter/Concordance production path.

---

## 5. Preconditions & dependencies

- **P0:** 0065 integrity + preflight machinery.  
- **P0:** 0066 fidelity_rank / materialize soft merge.  
- **Soft:** 0071 unique-pst orchestration.  
- **Soft:** 0073 reason freeze (coordinate strings even if 0074 merges first).  
- *Verified research 2026-07-26:* 64 KiB materialize cap; optimistic `stream_available`; preflight ignores attach-stream rates.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Multi-hour “scan” surprise | Default off on scan; budgets; cancel |
| False confidence from open-only | Default L2 head; honesty fields |
| Double I/O scan+export | Winner-only + level-aware cache §3.10 |
| Multi-GB RAM | Streaming discard; no fat Vec |
| FD exhaustion (1000s of PSTs) | Bounded LRU §3.9.1 |
| Hang / pathological stream | Per-attach timeout §3.4.1 |
| One blast group burns I/O budget | Peer probe cap §3.7.1 |
| L1 cache masks L2 fails | Level in cache key §3.10 |
| Divergent reason codes vs 0073 | Shared string table §3.5 |
| CRC log flood | Structured tallies only §3.9 |
| Strict mode ambiguity | Align with 0065 strict §3.6 |

---

## 7. Definition of Done

- [ ] **DoD-1 — Probe engine:** L1/L2 (default L2) chunked probe with budgets; L3 explicit only.  
- [ ] **DoD-2 — Winner path:** unique-pst/materialize can run winner-only deep probe.  
- [ ] **DoD-3 — Scan path:** optional `--deep-attach-preflight` with same budgets.  
- [ ] **DoD-4 — Reasons:** additive integrity codes; strings align with 0073 overlapping set.  
- [ ] **DoD-5 — Preflight:** attach attempt/fail/truncated/rate in report; threshold escalates recommendation.  
- [ ] **DoD-6 — Keep-set:** probe fails degrade fidelity; clean peer preferred when present.  
- [ ] **DoD-7 — Flags:** optimistic stream availability corrected on fail.  
- [ ] **DoD-8 — Safety:** no multi-GB Vec; cancel + progress; **bounded LRU** sticky handles (§3.9.1).  
- [ ] **DoD-9 — parents_only:** probe skipped.  
- [ ] **DoD-10 — Honesty:** docs + JSON coverage_note / truncated; ScanPST/re-export guidance.  
- [ ] **DoD-11 — Per-attach timeout:** wall-clock abort; run continues.  
- [ ] **DoD-12 — Peer probe cap:** one group cannot exhaust global attach budget.  
- [ ] **DoD-13 — Cache (if shipped):** level dominance; no L1→L2 poison; size/mtime when practical.  
- [ ] **DoD-14 — Tests:** §3.11 cases green; synthetic only.  
- [ ] **DoD-15 — Docs:** scan + unique-pst flags/defaults; 0073 interaction; FD/budget knobs.  
- [ ] **DoD-16 — Recorded:** `review.md`; registry **Completed**; **D-0074-***; ledger commit.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo test -p dedup-engine
cargo test -p pst-dedup-cli --test unique_pst
cargo test -p pst-dedup-cli  # scan / materializer unit tests as added
cargo clippy -p dedup-engine -p pst-dedup-cli -p pst-reader --all-targets -- -D warnings
# Full gate before commit:
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

---

## 9. Handoff

Unblocks defensible “should I export?” before multi-hour unique-pst. **0073** remains export-time inventory for residual fails. **0077** owns CRC spam. **0081** finishes operator decision tree (re-export vs ScanPST-on-copy vs proceed with ledger).
