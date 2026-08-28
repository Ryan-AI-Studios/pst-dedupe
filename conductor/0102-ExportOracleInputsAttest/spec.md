# 0102 — Export Oracle Inputs Attest

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open the allowlist semantics or CRC
> policy during implementation.

- **Track ID:** 0102-ExportOracleInputsAttest
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `docs/unique-pst-export.md` + this track. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-28 fold-in); do **not** chase it at execute.
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0099 (Completed). 0100 / 0101 Completed are not code dependencies; Series P order is 0099 → 0100 → 0101 → **0102** → 0103.
- **Spec authored:** 2026-08-28
- **Series:** P (Unique-PST defensibility)
>
> **Closes:** `D-0099-oracle-inputs-attest`.
> **HITL:** none. Unit tests on synthetic `summary.json` Value trees are sufficient. No INC* smoke.
>
> **Last-PR fold-in (2026-08-28):** PRs **#93, #92, #91, #90**. Origin Bugbot is **#89** (already this track). Disposition in §2.8.
>
> **Review fold-in (2026-08-28):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Parent-oracle docs + inverse mismatch test added; pointer-string asserts required.

---

## 1. Objective

Make unique-pst `export_oracle` actually compare the 0099 `export_risk.inputs` attest fields.

Today `SUMMARY_ALLOWLIST_KEYS` contains `"inputs"`, and `strip_keys_recursive` deletes **every** object key of that name — including the product object `export_risk.inputs`. `compare_integrity_counters` then JSON-pointers `/export_risk/inputs/…` on the **normalized** tree, so both sides are always missing and the attest is a no-op.

This advances unique-export **defensibility**: two packs that disagree on effective CRC rate / poly discount must not compare equivalent. 0099 already added the pointers and forbade allowlisting them; this track makes that DoD true.

---

## 2. Context (read before starting)

### 2.1 Diagnosis (PR #89 Bugbot, still live)

**Origin:** PR **#89** (0099) Cursor Bugbot, commit `77aee92`. Minted as this placeholder while planning **0100**; re-confirmed while planning **0101**. Not stolen into 0100 or 0101.

Bugbot text (verbatim gist): the new `/export_risk/inputs/…` pointers run after `normalize_summary_for_oracle`, which recursively strips every key named `inputs`. That deletes `export_risk.inputs` before comparison, so the four attest fields always look missing and cannot catch a divergence.

### 2.2 Live code snapshot (verified 2026-08-28, `main` @ `11e455f`)

Re-verify line numbers at execute.

| Surface | State |
|---|---|
| Allowlist | `export_oracle.rs` `SUMMARY_ALLOWLIST_KEYS` still includes `"inputs"` (job-level path array intent). |
| Recursive strip | `strip_keys_recursive` `map.remove`s every allowlist key at **every** object, then recurses. Nested `export_risk.inputs` is deleted. |
| Root blanking | After strip, `normalize_summary_for_oracle` does `obj.insert("inputs", empty array)` on the **root** object only. That is the real path-local handling for `UniqueExportSummary.inputs: Vec<String>`. |
| Pointers | `compare_integrity_counters` still lists the four 0099 paths **and** runs on the **already-normalized** `sa`/`sb`. Both pointers are `None`/`None` → no mismatch. |
| Call order | `compare_export_packs`: `normalize_summary_for_oracle` both sides → whole-object `sa != sb` (also blind to stripped `export_risk.inputs`) → `compare_integrity_counters(&sa, &sb, …)`. |
| Product object | `unique_export_report.rs` `ExportRisk.inputs: ExportRiskInputs` with `effective_block_crc_read_rate: Option<f64>`, `poly_class_crc_discounted`, `discount_attach_stream_crc`, `poly_class_crc_sources` (`#[serde(default)]`). Vocabulary / thresholds **not** this track. |
| Root `inputs` | `UniqueExportSummary.inputs: Vec<String>` — absolute source paths; also the join key for `source_id`. Volatile for oracle. |
| Tests | `normalize_strips_timing_and_paths` and `allowlist_equalizes_parent_without_0079_counters` do **not** assert `export_risk.inputs` survival. No test that two summaries differing only in attest fields mismatch. |
| Docs | `docs/unique-pst-export.md` oracle-allowlist paragraph names 0079 measurement fields + paths/hashes/timings. Does not warn that recursive `"inputs"` also kills `export_risk.inputs`. |

Two `"inputs"` keys exist on a live summary and **must not be treated as one**:

| JSON pointer | Type | Oracle role |
|---|---|---|
| `/inputs` | `string[]` (source paths) | **Volatile.** Blank to `[]` at root. |
| `/export_risk/inputs` | `ExportRiskInputs` object | **Product attest.** Keep. Compare. |

No other nested `"inputs"` object is on `UniqueExportSummary` at plan-time (re-verify if a new field is added).

### 2.3 MS-PST / crate APIs (plan-time)

**N/A this track** for MS-PST structures. This is JSON oracle compare inside `pst-dedup-cli`. No writer/reader change. No new crates. No schema-id bump (`unique_export_report_v1` already serializes `export_risk.inputs` from 0099).

Crate-registry API churn: none expected.

### 2.4 Why 0099’s pointers were inert

0099 DoD-3 required:

- `/export_risk/inputs/effective_block_crc_read_rate`
- `/export_risk/inputs/poly_class_crc_discounted`
- `/export_risk/inputs/discount_attach_stream_crc`
- `/export_risk/inputs/poly_class_crc_sources`

and **forbade** putting those keys on `SUMMARY_ALLOWLIST_KEYS`. Implement added the pointer list but left the pre-existing recursive `"inputs"` strip. Pointers on a stripped tree are always missing-equal.

Whole-object equality after strip is equally blind: both sides lose `export_risk.inputs`.

### 2.5 Tools (plan-time)

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 3835 pinned).
- `ai-brains sync query` / `recall "export_oracle inputs attest allowlist"` — 0099 fold-in locked oracle pointers **not** on `SUMMARY_ALLOWLIST`; declined fourth `export_risk` value / fingerprint / per-event attach CRC.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` 0 pending / 0 unaudited drift. Impact **LOW** (working tree is skills + this placeholder + unrelated untracked files).
- Ledger tx for this planning pass: `bbcbfc56-7403-4ae6-b279-5487efa9cbf5`.

### 2.6 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0099: attest pointers are DoD; do **not** allowlist them | Keep the four pointers; do not add `export_risk.inputs` or its field names to the allowlist |
| 0099: declined per-event attach CRC, fingerprint, fourth risk value | Stay declined (§4 / §9) |
| 0079: allowlist equalizes parent **measurement** fields | Keep 0079 keys; do **not** equalize pre-0099 packs that lack `export_risk.inputs` |

### 2.7 How this advances the north star

Counsel-facing unique-PST must be honest. `export_risk` is the handoff gate. If two runs disagree on whether poly-class CRC was discounted, the 0079 structural oracle must **say so**. Today it cannot. Fixing the strip is the attest 0099 already claimed.

### 2.8 Last-PR Cursor comments (merged #93, #92, #91, #90)

Skill: last 2–4 merged product PRs. Also re-read origin **#89** because it **is** this track.

| PR | Comment | Verdict |
|---|---|---|
| **#93** (0101 docs) | No review / issue / inline comments | n/a |
| **#92** (0101 depth) | No review / issue / inline comments | n/a |
| **#91** (0100 docs) | No review / issue / inline comments | n/a |
| **#90** Bugbot | Recipient-table `table_subs.insert(0, matrix_nid)` leaves SLBLOCK unsorted once cell NIDs exist. Outlook searches SLBLOCK by NID. | **Valid. Not this track.** Already **0103** + `D-0100-slblock-nid-order`. Do not steal. Sketch stays on 0103. |
| **#89** Bugbot (origin; not in the last-four window) | Recursive `"inputs"` strip deletes `export_risk.inputs` before pointer compare | **This track.** Diagnosis re-verified live @ `11e455f`. |

No new placeholder minted this pass. No BCC-default track. No frontend steal of 0102.

### 2.9 Harness fold-in (2026-08-28)

Sources: `opencode-review.md` (vs `main` @ `11e455f`), `agy-review.md` (PASS; restates the Ready plan). Diagnosis and locked fix **confirmed** by both. Load-bearing corrections are opencode.

| Id | Claim | Source | Disposition | Landing |
|---|---|---|---|---|
| opencode-M1 | After the fix, pre-0099 parent packs mismatch HEAD on `export_risk.inputs`; current “pre-0079 parent still compares equal” docs go stale-red. Env-gated `unique_pst_parent_baseline_oracle_when_env_set` still `assert_equivalent()`. Inverse test missing. | opencode | **Agree — fold** (wording **partial**). Pre-0099 JSON usually still **has** `export_risk.inputs` (0077) but **omits** the four 0099 keys; pre-0077 may lack `export_risk` entirely. Either shape must mismatch the four pointers. Qualify docs + module header + parent-gate test comment; add inverse unit test. Baseline bin must be **post-0099**. Do not change `assert_equivalent()` to expect-red. | §3.1; §3 items 4–5; DoD-2/3; plan Phase 1–3 |
| opencode-O1 | Pointer-string assert in mismatch tests must be required, not “also acceptable” | opencode | **Agree — fold** | §3 item 4; plan Phase 2 |
| opencode-O2 | `strip_keys_recursive` is still name-based; future product fields named `path`/`bytes`/`out` would be stripped | opencode | **Agree — fold** (comment only; no path-aware rewrite) | §3 item 6; plan Phase 1 |
| opencode-O3 | `C:\dev\Dedupe-plan.md` still absent | opencode | **Agree — fold** | header |
| opencode-L1–L6 | Existing 0079 fixture stays green; no other `"inputs"` keys; no allowlist collision inside `ExportRiskInputs`; same-binary pack tests stay green; line refs exact; serde_json exact-f64 safe | opencode | **Already covered** | — |
| agy-0102-1..4 | Strip vs root blanking; call order; exact-f64; 0079 non-interference | agy | **Already covered** | — |

**Declined / not locked**

- Changing the env-gated parent test to `assert` a mismatch (operator may point `PST_DEDUPE_BASELINE_BIN` at a post-0099 binary; then it must still pass).
- Path-aware `strip_keys_recursive` (O2 is a comment; rewrite stays OOS unless Option A fails).

---

## 3. In scope

1. **Stop recursive-stripping `export_risk.inputs`.** Locked mechanism (§3.1): **remove `"inputs"` from `SUMMARY_ALLOWLIST_KEYS`.** Keep the existing root `obj.insert("inputs", empty array)` so job-level source paths still equalize. Comment at the insert: this is `/inputs` (paths), **not** `/export_risk/inputs`.
2. **Keep** the four `compare_integrity_counters` pointers. Do **not** add `export_risk.inputs` or those four field names to the allowlist. Do **not** drop the pointers on the theory that whole-object equality is enough — named diagnostics stay.
3. **Keep call order:** normalize both summaries, then whole-object compare, then `compare_integrity_counters` on the **normalized** trees. Do not switch pointer compare to raw JSON (would leave whole-object equality still blind if strip regressed).
4. **Tests** in `export_oracle.rs` `mod tests` (synthetic `serde_json::json!`; no PST, no `compare_export_packs` temp dirs required):
   - After `normalize_summary_for_oracle`, all four `/export_risk/inputs/…` pointers are **present** (values preserved).
   - Two summaries that differ **only** in `effective_block_crc_read_rate` (e.g. `0.0` vs `0.20`) → `compare_integrity_counters` reports a mismatch whose string **contains that pointer**. The pointer string is **required** (not optional next to whole-object `sa != sb`).
   - Two summaries that differ **only** in a second attest field (`poly_class_crc_discounted` true vs false) → mismatch string **contains that pointer**.
   - Two summaries that differ **only** in root `/inputs` path arrays → after normalize, **no** integrity mismatch and root `/inputs` is `[]` on both.
   - Identical attest objects → no pointer mismatch.
   - **Inverse (parent-shaped):** HEAD-shaped tree with the four attest keys vs pre-0099-shaped tree whose `export_risk.inputs` **omits** those four keys (0077 object may still be present) → after normalize, `compare_integrity_counters` mismatches on `/export_risk/inputs/…`. This is the attest working.
   - Existing `allowlist_equalizes_parent_without_0079_counters` stays green. If that fixture is extended with `export_risk`, give **both** sides the **same** attest object so the test still proves 0079 measurement equalization, not attest deletion.
5. **Docs:** `docs/unique-pst-export.md` oracle-allowlist paragraph: recursive strip must not treat the name `inputs` as volatile everywhere; job-level `summary.inputs` is blanked at **root only**; `export_risk.inputs` is product. **Qualify** the existing “pre-0079 parent still compares equal” sentence: 0079 **measurement** equalization still holds; a **pre-0099** parent that omits the four attest keys **must mismatch** HEAD — intended. Operator `PST_DEDUPE_BASELINE_BIN` must be a **post-0099** `pst-dedup.exe` (or the env-gated gate is expected-red on those pointers). Same qualification on the `export_oracle.rs` module doc (`# Parent vs HEAD / pre-0079 packs`) and the doc-comment on `unique_pst_parent_baseline_oracle_when_env_set` (`unique_pst.rs` ~1507). One CHANGELOG sentence.
6. **Comment** on `SUMMARY_ALLOWLIST_KEYS`: product fields (keep_set, export fidelity, exit_code, degraded_reasons, **`export_risk` / `export_risk.inputs`**) are not allowlisted. Strip is **name-based**; new product fields must not reuse allowlist names (`path`, `bytes`, `out`, `inputs`, …) or the oracle must go path-aware.

### 3.1 Locked fix (do not reopen)

**Do this:** remove `"inputs"` from `SUMMARY_ALLOWLIST_KEYS`; keep root blanking.

**Do not do this unless Option A is proven insufficient at execute (it should not be):**

- Path-aware `strip_keys_recursive` special-case for `"inputs"`.
- Renaming `ExportRisk.inputs` (would churn `summary.json` / counsel tooling).
- Comparing integrity counters on the raw tree *instead of* fixing the strip.

**Parent-oracle (0079):** allowlist still equalizes additive **measurement** (`phase_timings`, `messages_materialized`, `source_pst_opens`, `bytes_written_total`, `prepared_bytes_peak`, `hash_ms`, timings, paths, hashes). That sentence stays true **only** for those measurement keys.

`export_risk.inputs` is product. Typical **pre-0099** JSON still has the 0077 `inputs` object but **omits** the four 0099 keys; **pre-0077** may lack `export_risk` entirely. After this strip fix, either shape vs HEAD **must mismatch** on `/export_risk/inputs/…`. That is the attest working. Do not restore pre-0099 parent equality by allowlisting the attest object or its field names.

Operator gate: `PST_DEDUPE_BASELINE_BIN` must point at a **post-0099** binary if the env-gated test is expected green. Do not rewrite `unique_pst_parent_baseline_oracle_when_env_set` to assert a mismatch.

**Float compare:** exact `serde_json::Value` equality (same as `/scan/block_crc_rate` today). No epsilon. `effective_block_crc_read_rate` is derived from integer CRC counts; same inputs → same JSON number.

---

## 4. Out of scope (do NOT do here)

- Changing `export_risk` vocabulary, thresholds (0.15 / 0.50), monotone `max`, or poly-class classifier (**0099** closed).
- Per-event / per-source writer attach-CRC split (**D-0099-attach-crc-job-level**).
- True CRC polynomial fingerprint (**D-0077-poly-fingerprint**).
- A fourth `export_risk` value, in-tool ScanPST / CRC repair, zeroing raw CRC telemetry.
- Recipient TC / SLBLOCK NID order (**0103** / `D-0100-slblock-nid-order`).
- HNBITMAPHDR / attach-table TC.
- `--max-embedded-depth` / unique-eml nested MIME (**0101** / **D-0067** residual).
- BCC default / `--include-bcc-recipients` (**0082**).
- Expanding the pointer list to every `ExportRiskInputs` field (whole-object compare catches the rest; the four 0099 names stay as diagnostics).
- Making `compare_integrity_counters` public or adding a CLI `pst-dedup oracle` subcommand.
- Schema-id bump of `unique_export_report_v1`.
- Frontend / Hermes (**0105+**).
- Operator INC* re-smoke (**D-0094-inc-resmoke**).

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0099 shipped `ExportRisk.inputs` on `summary.json` and the four pointers. This track does not re-derive rates.
- **P2:** `export_oracle` stays in `pst-dedup-cli` (0079). Do not push policy into `dedup-engine` or `pst-reader`.
- *Verified to date:* allowlist still has `"inputs"`; pointers still listed; strip still recursive; root blanking still present. HEAD `11e455f`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Removing `"inputs"` from the allowlist lets root paths leak into whole-object compare | Root `insert("inputs", [])` already runs **after** strip; assert in tests that differing path arrays equalize |
| A future nested `"inputs"` object is now kept | Plan-time: only root array + `export_risk.inputs`. Comment the allowlist. New fields named `inputs` need an explicit oracle decision |
| Parent-oracle vs pre-0099 binary now mismatches | **Intended.** Qualify `docs/unique-pst-export.md` ~187, `export_oracle.rs` module doc, and `unique_pst.rs` ~1507. Inverse unit test. Baseline = post-0099. Do not allowlist attest |
| Whole-object `export_risk` already differs, so pointers look redundant | Keep pointers for named mismatch strings; tests call `compare_integrity_counters` directly |
| `unique_pst` pack tests flake on f64 | Exact JSON; same binary/inputs. No epsilon. Do not add a full-pack test unless unit tests are insufficient |
| Touching `strip_keys_recursive` breaks 0079 parent equalization | Keep existing allowlist keys except `"inputs"`; keep `allowlist_equalizes_parent_without_0079_counters` green |
| Hotspot `export_exit_0078.rs` | Do **not** edit that file; tests live in `export_oracle.rs` |

---

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 — Strip honesty:** `"inputs"` is **not** on `SUMMARY_ALLOWLIST_KEYS`. After `normalize_summary_for_oracle`, `/export_risk/inputs` and the four attest pointers remain with original values. Root `/inputs` is still `[]`. The four pointers remain in `compare_integrity_counters`. Attest field names are **not** on the allowlist.
- [x] **DoD-2 — Tests:** Synthetic unit tests in `export_oracle.rs`: (a) pointers survive normalize; (b) differ-only-in-`effective_block_crc_read_rate` **mismatches** and the mismatch string **contains that pointer**; (c) differ-only-in-`poly_class_crc_discounted` **mismatches** with that pointer string; (d) differ-only-in-root-`inputs` paths **matches** after normalize; (e) identical attest **matches**; (f) `allowlist_equalizes_parent_without_0079_counters` still green; (g) pre-0099-shaped `export_risk.inputs` (four attest keys omitted) vs HEAD-shaped **mismatches** on `/export_risk/inputs/…`. No client PSTs in git.
- [x] **DoD-3 — Docs:** `docs/unique-pst-export.md` oracle-allowlist paragraph names the two `inputs` keys **and** qualifies pre-0079 parent equality (measurement only; pre-0099 attest mismatch is intended; `PST_DEDUPE_BASELINE_BIN` = post-0099). Same qualification on `export_oracle.rs` module doc and `unique_pst_parent_baseline_oracle_when_env_set` doc-comment. CHANGELOG one-liner. `D-0099-oracle-inputs-attest` closed in `docs/deferred.md`.
- [x] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`BUGFIX` on `crates/pst-dedup-cli` at implement). No HITL required.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-dedup-cli export_oracle
cargo test -p pst-dedup-cli --lib export_oracle
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
# before implement-track publish:
cargo test --workspace
```

`--lib export_oracle` is the intended filter (module is `pst_dedup_cli::export_oracle`; re-verify at execute). Keep at least the tests in §3 item 4.

No operator INC* command. No unique-pst binary run required for DoD.

---

## 9. Deferred roll (mandatory)

Entire `docs/deferred.md` scanned 2026-08-28. Related open rows:

| Row | Disposition |
|---|---|
| **D-0099-oracle-inputs-attest** | **Absorb and close** on implement. This track. |
| **D-0099-attach-crc-job-level** | **Decline.** Job-level attach CRC vs scan-time poly class. 0099 residual. |
| **D-0077-poly-fingerprint** | **Decline.** Dual-rate stays the classifier. |
| **D-0077-systematic-poly** | **Already closed in 0099.** Honesty of the gate; this track attests it in the oracle. |
| **D-0100-slblock-nid-order** | **Decline** (0103). #90 Bugbot stays there. |
| **D-0100-hn-bitmap-hdr** | **Decline.** |
| **D-0093-attachment-tc-page** | **Decline.** |
| **D-0094-inc-resmoke** | **Decline.** Optional operator HITL; not oracle JSON. |
| **D-0097-window-edge-normalize** | **Decline.** Parked polish. |
| **D-0079-*** (measurement / mmap / `--jobs`) | **Decline.** Keep 0079 allowlist keys as-is except `"inputs"`. |
| Other `docs/deferred.md` rows | **Decline** — not export_oracle attest. |

Med/high never parked here. No BCC-default track. No frontend steal of 0100–0104.

---

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production (`export_oracle.rs` already uses `unwrap_or` on path basename — do not add panics).
4. Crate boundary: oracle stays `pst-dedup-cli`. Do not teach `dedup-engine` oracle policy.
5. Unique-export: no silent recipient/attach/count drops. This track does not add `known_gap`.
6. No in-tool ScanPST / CRC repair of evidence.
7. `--include-bcc-recipients` default **off**.
8. `export_risk` vocabulary stays three-valued (`ok` / `re_export_recommended` / `not_export_ready`).
9. Do not rename `ExportRisk.inputs`.
10. Do not allowlist `export_risk.inputs` or its attest field names.
