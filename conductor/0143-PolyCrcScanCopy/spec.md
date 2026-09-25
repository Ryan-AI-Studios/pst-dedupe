# 0143 — Poly-CRC scan copy (preflight `ok` vs scary rates)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **`--json` stays on stdout.** Logs stay on stderr.
> Distinct from **0077** / **0099** / **0108** (classifier + `export_risk` math), **0140** (cadence / `--crc-log-limit`), **0144** (integrity CSV).
> Do **not** change skip_rate, dual-rate, or unique-pst `export_risk`. No fourth `export_risk` value. No BCC-default. Do not steal **0100–0104**. Not frontend.

- **Track ID:** 0143-PolyCrcScanCopy
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** scan JSON/stderr operator copy. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-25).
- **Status:** Completed
- **Depends on:** **0077** / **0099** / **0108** Completed (`poly_class_crc` telemetry already ships)
- **Spec authored:** 2026-09-24 placeholder
- **Ready:** 2026-09-25 (coordinator fold of `agy-review.md` + `opencode-review.md`; placeholder expanded)
- **Fold-in:** 2026-09-25 `agy-review.md` + `opencode-review.md` (bundle `AI-review.md`)
- **Series:** X (INC0102784 CLI operator friction, readonly eval 2026-09-24)
- **Ledger category:** `FEATURE`

> **Closes / absorbs:** `D-0143-poly-crc-scan-copy`.
> **HITL (owner, not CI):** optional Desktop INC* `scan --json` confirming `poly_crc_note` present, `poly_class_crc_sources >= 1`, `preflight.recommendation == ok`. Never commit INC* PSTs.

---

## 1. Objective

Operators must see that a **poly-class** store can have a high `block_crc_read_rate` (INC* and aspose: `1.0`) next to preflight **`ok`**, and that this is the dual-rate CRC polynomial — not a failed-open integrity check and not “the PST is corrupt.”

Live JSON already names the **fact**: `files[].poly_class_crc` and `summary.poly_class_crc_sources`. The friction is missing **interpretive copy**. Deliver one additive `ScanSummary` string plus one stderr `note:` line. Do not invent a root boolean `poly_class_crc`. Do not push copy into `preflight.reasons`.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

INC* 2026-09-24: `block_crc_rate ≈ 30` (CRC hits **per recoverable message**, not a percent), `block_crc_read_rate = 1.0`, **4099** `CRC_SUSPECT` (pre-clear), `poly_class_crc=true` on sources, `crc_skip_rate=0`, preflight **`ok`**. Auditors reading `read_rate=1.0` then `preflight: ok []` assume the checker ignored bit rot.

Fold-in live re-check (2026-09-25, `target\release\pst-dedup.exe scan fixtures\aspose_outlook.pst --json`): `poly_class_crc_sources=1`, `files[0].poly_class_crc=true`, `block_crc_read_rate=1.0`, `block_crc_rate≈14.06`, `crc_suspect_messages=17`, `preflight.recommendation=ok`, `reasons=[]`. **CI fixture is aspose.** Re-verify at execute.

### 2.2 Live facts (plan-time HEAD `0fd68f3` / product squash `a4dae1e`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| Schema | Matter **41**. N/A. No `SCHEMA_VERSION` bump. `ScanSummary.schema` stays `scan_integrity_v1`. |
| Dual-rate | `is_poly_class_crc` (`scan.rs` ~1844–1853): `page_rate >= 0.50 && block_rate >= 0.50`. Then `clear_poly_false_positive_crc_suspect`. Raw page/block counters stay. |
| `FileScanStats.poly_class_crc` | Per source (`scan.rs` ~147–152). Already serialized. |
| `ScanSummary.poly_class_crc_sources` | Count of sources with `poly_class_crc` (`scan.rs` ~211–213, tallied ~1775). **No** summary-level boolean `poly_class_crc`. |
| `block_crc_rate` | `(page_crc + block_crc) / recoverable_messages` — hits per message. Do **not** print as `%`. |
| `block_crc_read_rate` | `(page_crc + block_crc) / (page_reads + block_reads)` ∈ [0,1]. This is the scary fraction. |
| `compute_preflight` | `dedup-engine` `integrity.rs` ~611–702. Keys skip/crc_skip/failed_file/attach-probe rates. **Never reads** `block_crc_read_rate`. Any non-empty `reasons` in the skip branch sets `ReExportRecommended` (~669–671). |
| `print_summary_text` | `main.rs` ~1976–2041: `crc: … read_rate=` then `preflight: ok []`. No poly copy. |
| `run_scan` | Library. **Must not** `eprintln!`. Callers: `cmd_scan` / `cmd_dups` (`main.rs`), `keep_set_cmd.rs` ~265, `unique_pst_cmd.rs` ~1621, `unique_eml_cmd.rs` ~1035. |
| 0139 notes | `eprintln!("note: {hint}")` in `keep_set_cmd.rs` ~216 (source-rank, **before** scan) and ~305 (Recoverable Items, text-only **after** keep-set). `unique_eml_cmd.rs` ~944. |
| 0140 | `--crc-log-limit` (default 10, `0` silences **per-attempt** CRC/probe writeln). Poly advisory is **not** a probe line. |
| `ScanSummary` literals | **No `Default`**. Blast: `scan.rs` constructor ~1782 + tests ~2654, ~2708; `unique_pst_cmd.rs` ~1178 (cancelled); `unique_eml_cmd.rs` ~1494, ~1606; `tests/unique_pst_also_eml.rs` **six** (~611, 764, 964, 1120, 1274, 1409). **11 explicit + 1 constructor.** |
| Oracle | `normalize_summary_for_oracle` strips `SUMMARY_ALLOWLIST_KEYS` recursively. New copy must be deterministic **and** allowlisted so parent packs without the key still compare. |
| clap | Workspace `"4"`. Lock **4.6.4**. crates.io / docs.rs latest **4.6.7**. **No new flags.** |
| GUI | `pst-dedup-gui` has its own `worker::run_scan`. **OOS.** |

### 2.3 Tools (fold-in 2026-09-25)

| Tool | Result |
|---|---|
| `ai-brains preflight --summary` | Vault live (project `93e74c21`). 5027 pins. Recall: 0077 dual-rate; 0099 effective rate / `poly_class_crc_discounted`; 0108 `effective_degraded_winner_rate`; never lower scan preflight; no fourth `export_risk`. |
| `ledgerful doctor --json` | `readyForPublish: true`, 0 block. Unrelated warn: phantom verify rows, sig-pin, sig-version, completion-model cold. |
| `ledgerful ledger status --compact` | 0 pending, 0 unaudited drift. |
| Last-PR Cursor | PRs **#158–#161**. Bugbot usage-limit only. No 0143-owned inline finding. **Decline mint.** |
| aspose scan | Fold-in: poly-class **true**; CI fixture **named**. |

### 2.4 Product locks

- Do not change `is_poly_class_crc` thresholds or `clear_poly_false_positive_crc_suspect`.
- Do not change `compute_preflight` skip math, thresholds, or `PreflightReport` fields.
- **Never** push poly copy into `preflight.reasons` (escalates to `re_export_recommended`).
- Do not add a fourth `export_risk` value. Do not edit unique-pst `effective_*` rates (**0099** / **0108**).
- Do not restrip keep-set `CRC_SUSPECT` (`D-0108-keepset-crc-retaint`).
- Do not implement **0144** integrity CSV rows, **0145** scan-once, **0146** write hotspots, **0147** deep-attach leftover.
- Do not bump matter schema 41. No BCC-default.
- Do not move logs onto stdout. Do not add flags. Do not bind the advisory to `--crc-log-limit`.
- Do not mint a summary-level boolean `poly_class_crc`.
- Do not render `block_crc_rate` as a percent.
- `run_scan` stays silent. GUI OOS.

### 2.5 Copy contract (locked)

CLI helper in `pst-dedup-cli` `scan.rs` (names implementer-local; behavior frozen):

```text
poly_crc_note(sources: u64) -> Option<String>
```

`None` iff `sources == 0`. `Some` iff `sources >= 1`. Text is **count + fixed English only** (no paths, timestamps, rates, or live recommendation string):

```text
{n} source(s) classified as poly-class CRC; systematic CRC mismatch on those sources is compatible with preflight 'ok' (not evidence of corrupt data blocks)
```

`{n}` is the decimal `poly_class_crc_sources`. Same string in JSON, stderr, and human text.

**JSON** — additive field on **`ScanSummary`** (so `scan` / `dups` / `keep-set` / `unique-pst` / `unique-eml` that serialize `summary` / `scan` all see it):

| Field | Type / rule |
|---|---|
| `poly_crc_note` | `Option<String>`. `#[serde(default, skip_serializing_if = "Option::is_none")]`. Present iff `poly_class_crc_sources >= 1`. **Omitted** (not `null`) when 0. |
| `poly_class_crc_sources` | Unchanged `u64`. |
| `files[].poly_class_crc` | Unchanged `bool`. |
| `preflight` | Unchanged. Do not add `operator_note` / reasons. |

Populate at the `ScanSummary { … }` construction in `run_scan` from `poly_class_crc_sources`. Dummy/cancelled literals: `poly_crc_note: None`.

**Stderr** — after `run_scan` returns, when `poly_crc_note` is `Some`:

```text
eprintln!("note: {note}");
```

Sites (one line per command, independent of `--crc-log-limit`, not `tracing`):

| Command | When | Order vs other `note:` |
|---|---|---|
| `scan` (`cmd_scan`) | After `run_scan`, JSON and text | Only poly line from this track |
| `dups` (`cmd_dups`) | Same | Same |
| `keep-set` | After `run_scan`, JSON and text | **After** 0139 source-rank note (that one is **before** scan). Recoverable-Items hint stays later and text-only. |
| `unique-pst` | After `run_scan` | After 0139 source-rank if that command prints it the same way; before unique-pst stdout summary |
| `unique-eml` | After `run_scan` | After 0139 source-rank note (`unique_eml_cmd.rs` ~944) |

**Human stdout** (`print_summary_text`, in scope):

- `crc:` line appends `poly_sources={}`.
- If `poly_crc_note` is `Some`, after the `preflight:` line: `  poly_crc:     {note}`.

**Oracle:** add `"poly_crc_note"` to `SUMMARY_ALLOWLIST_KEYS` so parent packs without the field still compare.

---

## 3. In scope

1. `ScanSummary.poly_crc_note` + helper; populate when `poly_class_crc_sources >= 1`.
2. Stderr `note:` on scan / dups / keep-set / unique-pst / unique-eml after `run_scan`.
3. `print_summary_text` poly_sources + poly_crc line.
4. All `ScanSummary` struct-literal updates (`None` on dummies).
5. Oracle allowlist for `poly_crc_note`.
6. Tests in §7 / plan Phase 3; README + CHANGELOG Unreleased.

---

## 4. Out of scope (do NOT do here)

- Dual-rate math, poly clear, `compute_preflight`, `PreflightReport`.
- Unique-pst `export_risk` / `effective_block_crc_read_rate` / `effective_degraded_winner_rate` / fourth enum (**0099** / **0108**).
- Keep-set `CRC_SUSPECT` restrip (`D-0108-keepset-crc-retaint`).
- `D-0077-poly-fingerprint`. `D-0099-attach-crc-job-level` stays residual.
- Integrity CSV honesty (**0144**). Scan-once (**0145**). Write hotspots (**0146**). Deep-attach leftover (**0147**).
- Binding the advisory to `--crc-log-limit`. Changing 0140 cadence.
- GUI worker. New clap flags. Matter schema. BCC-default.
- A root/summary boolean `poly_class_crc`. Putting copy in `preflight.reasons`.

---

## 5. Preconditions & dependencies

- **P1:** `poly_class_crc` / `poly_class_crc_sources` already serialize (0077).
- **P2:** 0140 `--crc-log-limit` help + silence tests must still pass.
- **P3:** `fixtures/aspose_outlook.pst` is dual-rate (fold-in verified 2026-09-25).
- *Verified to date:* `compute_preflight` reason trap; 11+1 `ScanSummary` literals; clap 4.6.4 vs 4.6.7; aspose poly + preflight `ok`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Copy in `preflight.reasons` flips recommendation | Fence in §2.4; tests assert aspose stays `ok` and reasons lack poly |
| `--crc-log-limit 0` swallows the note | Independent `eprintln!` after `run_scan`; test with `--crc-log-limit 0` |
| `run_scan` prints | Helper is opt-in at CLI surfaces only |
| Miss a `ScanSummary` literal | Phase 1 enumerates all 12 sites |
| Oracle parent↔HEAD diverge | Allowlist `poly_crc_note` |
| Note claims this run is `ok` when it is not | Frozen text says poly is **compatible with** `ok`, does not interpolate live recommendation |
| `block_crc_rate ≈ 30` read as 30% | Copy never prints that field as a percent |
| Two `note:` lines unordered | 0139 source-rank before scan; poly after scan |
| GUI / engine boundary | `PreflightReport` untouched; GUI OOS |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — JSON copy:** `scan --json` on `fixtures/aspose_outlook.pst` has `summary.files[0].poly_class_crc == true`, `summary.poly_class_crc_sources >= 1`, `summary.poly_crc_note` equal to the frozen sentence with that count, and **no** new root boolean `poly_class_crc`. `dups --json` on the same file also carries `summary.poly_crc_note`.
- [ ] **DoD-2 — Stderr one-liner:** same aspose `scan --json` (and `scan` without `--json`) prints exactly one `note: …poly-class CRC…` line on **stderr**. Repeats with `--crc-log-limit 0`. Stdout JSON remains parseable (note not on stdout). `run_scan` itself does not print.
- [ ] **DoD-3 — Preflight unchanged:** aspose `preflight.recommendation == "ok"` and `reasons` does not contain a poly token. Existing `compute_preflight` tests still pass. Dual-rate helper unchanged.
- [ ] **DoD-4 — Omit when clean:** a `ScanSummary` with `poly_class_crc_sources == 0` omits `poly_crc_note` from JSON (not `null`). No poly `note:` on stderr.
- [ ] **DoD-5 — Human + surfaces:** `print_summary_text` includes `poly_sources=` and, when poly, a `poly_crc:` line. keep-set / unique-pst / unique-eml emit the stderr note after their `run_scan` when sources ≥ 1. All `ScanSummary` literals compile.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; ledger **FEATURE**; README scan/CRC sentence; CHANGELOG Unreleased; `D-0143-poly-crc-scan-copy` closed in `docs/deferred.md` on the implement docs PR. Oracle allowlist test. Optional INC* HITL skip allowed if recorded.

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-dedup-cli --lib poly_crc
cargo test -p pst-dedup-cli --test scan_integrity --test crc_integrity_0077
cargo test -p dedup-engine --lib compute_preflight
cargo test --workspace
ledgerful verify
```

If helper tests live only in `tests/scan_integrity.rs` (no `--lib poly_crc`), run that file instead.

---

## 9. Deferred

| ID | Disposition |
|---|---|
| `D-0143-poly-crc-scan-copy` | **Absorb** — this track. |
| `D-0108-keepset-crc-retaint` | **Decline** — do not restrip keep-set. |
| `D-0077-poly-fingerprint` | **Decline** — not copy. |
| `D-0108-poly-degraded-winner-risk` | **Closed / 0108**. |
| `D-0099-attach-crc-job-level` | **Decline** — stays residual (poly/attach-CRC attribution). |
| `D-0140-scan-stderr-cadence` | **Reference** — `--crc-log-limit 0` must not silence this note. Closed / 0140. |
| `D-0139-source-rank-discoverability` | **Reference** — reuse `eprintln!("note:")`. Closed / 0139. |
| `D-0141` / `D-0142` | Closed siblings. |
| `D-0144-integrity-csv-honesty` | **Decline** — sibling CSV honesty. |
| `D-0145` / `D-0146` / `D-0147` | **Decline** — siblings. |

This fold does not edit `docs/deferred.md` (product git). Implement docs PR closes `D-0143`.

### Fold-in declines / corrections

| Id | Disposition |
|---|---|
| AGY-143-01 / OC-143-01 unexpanded placeholder | **Agree — fold** — this expansion. |
| AGY-143-02 / OC-143-02 `preflight.reasons` trap | **Agree — fold** — §2.4 / DoD-3. |
| AGY-143-03 / OC-143-03 JSON key | **Agree — fold** — `ScanSummary.poly_crc_note`; not stderr-only; not a root bool. |
| AGY-143-04 / OC-143-06 stderr + sites | **Agree — fold** — frozen sentence; `eprintln!("note:")`; sites in §2.5. |
| AGY-143-05 `--crc-log-limit` | **Agree — fold** — independent of 0140 limiter. |
| AGY-143-06 / OC-143-07 `print_summary_text` | **Agree — fold** — human in scope. |
| AGY-143-07 / OC-143-04 tests + fixture | **Agree — fold** — aspose **is** poly (fold-in scan). CI fixture named. |
| OC-143-05 literal blast | **Agree — fold** — 11+1 sites in Phase 1. |
| AGY-143-08 crate boundary | **Agree — fold** — `PreflightReport` untouched. |
| AGY-143-09 / OC-143-07 docs | **Agree — fold** — DoD-6. |
| OC-143-08 adjacent deferred | **Agree — fold** — §4 / §9. |
| OC-143-09 no summary bool | **Agree — fold** — §2.4. |
| OC-143-10 keep-set `note:` order | **Agree — fold** — 0139 then poly. |
| OC-143-11 oracle | **Agree — fold** — allowlist + deterministic copy. |
| OC-143-12 `block_crc_rate` not percent | **Agree — fold** — wording lock. |
| AGY exact stderr claiming live `'ok'` | **Agree — partial** — “compatible with preflight 'ok'”; do not interpolate this run’s recommendation. |
