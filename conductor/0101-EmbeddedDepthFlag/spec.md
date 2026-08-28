# 0101 — Embedded Message Depth Flag

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open the default or clamp during implementation.

- **Track ID:** 0101-EmbeddedDepthFlag
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → unique-PST nested export (file not present on this machine at plan-time; re-verify at execute)
- **Cross-repo contract:** n/a
- **Status:** In Progress
- **Depends on:** 0094 · 0098 (Completed). 0100 Completed is not a code dependency; Series P order is 0099 → 0100 → **0101** → 0102.
- **Spec authored:** 2026-08-27
- **Series:** P (Unique-PST defensibility)
>
> **Narrows:** `D-0067-embedded-depth` (unique-pst extract/write depth CLI only — does **not** close the row).
> **HITL:** optional operator INC* re-smoke at `--max-embedded-depth 8` (`D-0094-inc-resmoke`).
>
> **Last-PR fold-in (2026-08-27):** PRs **#91, #90, #89, #88**. Disposition in §2.8. Valid leftover from #90 (SLBLOCK NID order) is **0103**, not this track.
>
> **Review fold-in (2026-08-27):** `opencode-review.md` + `agy-review.md`. Disposition in §2.9 and `foldin-note.md`. Phase 0 product locks stay closed. DoD-2 no longer requires a CLI chain-of-9 (writer cannot emit a 9th nest).

---

## 1. Objective

Expose the writer’s existing `WritePstOpts::max_embedded_depth` (clamped **[1, 8]**, default **3**) on `pst-dedup unique-pst` so operators can recover nested `.msg` past depth 3 without a code change. Keep `ATTACH_DEPTH_LIMIT` ledger honesty when a nest is still deeper (or still over the 32 MiB per-nest budget). Default stays **3**.

This advances unique-export **defensibility**: INC* post-0098 still exits 64 solely on four `ATTACH_DEPTH_LIMIT` rows. Counsel can raise the knob, re-export, and disclose whatever remains — no silent drop, no in-tool repair.

---

## 2. Context (read before starting)

### 2.1 Operator evidence (INC0102784, post-0098)

`output/inc0102784-post-0098/` (operator-local; not in git). Same inputs/order as 0097.

| Signal | Value |
|---|---|
| Written / verify found | **4055 / 4055** (0098) |
| Nested-export fail | **4** `ATTACH_DEPTH_LIMIT` = **two** method-5 attaches (indexes 6 and 7) on one memorandum × **two** source PSTs |
| Other attach fails | 374 `ATTACH_EMBEDDED_UNPARSED` were the 0094 class; post-0098 residual nested-export fail is **only** these 4 depth-limit rows |
| Exit (post-0099 / 0100) | **64** `ATTACH_SOFT_FAIL` |

Raising unique-pst depth to **8** is the operator recovery path. It is **not** proven in CI (no INC* in git). If HITL at 8 still shows those 4 rows, classify depth vs 32 MiB `ResourceLimit` in `review.md` — both map to `ATTACH_DEPTH_LIMIT` today. Do **not** split event codes in this track.

### 2.2 Live code snapshot (verified 2026-08-27, `main` @ `4726803`)

| Surface | State |
|---|---|
| CLI unique-pst | `unique_pst_cmd.rs` **hardcodes** `let nested_extract_depth = 3u32;` then passes it to `materialize_nested_for_winner` **and** `WritePstOpts.max_embedded_depth`. No clap flag. |
| Clap | `UniquePstClapArgs` has `--include-bcc-recipients` / `--promote-on-attach-fail` but **no** depth flag. |
| Runtime args | `UniquePstCliArgs` has no depth field. GUI `unique_wizard.rs` builds `UniquePstCliArgs` field-by-field (must compile after the new field). |
| Writer | `WritePstOpts::max_embedded_depth` default 3; `embedded_depth_limit()` = `.clamp(1, 8)`. Halt when `depth >= max_depth \|\| attach.embedded_depth_limited`. Comment: depth 0 = top-level; each embed increments. |
| Materializer | `materialize_nested_for_winner(..., max_embedded_depth)` clamps 1–8. First-level method-5: `remaining_depth == 0` → `embedded_extract_limit`; else `read_export_from_message_node(..., remaining.saturating_sub(1), 32 MiB)`. |
| Reader export | `read_export_from_message_node`: `remaining_child_depth == 0` → `embedded_depth_limited`; `PstError::ResourceLimit` (byte budget) also → `embedded_depth_limited` (same `ATTACH_DEPTH_LIMIT` at writer). |
| Identity hash | `MAX_EMBEDDED_IDENTITY_DEPTH = 3` in `pst-reader` (`embedded.rs`). **Different surface** from export depth. `embedded_msg_hash_0090` writes with `max_embedded_depth: 8` then asserts hash depth-limit ≥ 1. **Do not change.** |
| unique-eml | `EmlWriteOpts.max_embedded_depth` default 3 — **out of scope**. |
| Summary | `ExportSection` is **Serialize-only** (no `Deserialize`). No `max_embedded_depth` field today. Schema stays `unique_export_report_v1` (always-present new key; do **not** bump the schema id). |
| Tests | Writer `embedded_depth_cap_enforced` (chain depth 5 @ max 3). Extract flag `embedded_depth_limited_flag_maps_to_depth_limit`. No CLI unique-pst test that varies extract depth. |
| GUI | `run_unique_pst_with_options`; wizard has no depth slider (`D-0074-gui` style). |

**Re-verify at execute:** line numbers and the hardcoded `3u32` site.

### 2.3 MS-PST / crate APIs (plan-time)

**N/A this track** for new MS-PST structures. Nested write + `PidTagAttachDataObject` PtypObject already shipped in **0094**. This track is a CLI/library wire of an existing product budget.

Writer depth semantics (0094, still live): `--max-embedded-depth N` allows **N** nested `ATTACH_EMBEDDED_MSG` levels under the top-level winner. N=3 matches today’s hardcode. N=8 is the product ceiling (hostile-nest RAM/time). Unbounded recursion stays forbidden.

Crate-registry API churn: none expected. No new deps.

### 2.4 Why default stays 3

INC* recovery wants 8, but raising the default changes RAM/time on every unique-pst run and surprises operators who already documented depth 3. **Phase 0 lock:** default **3**; operators pass `--max-embedded-depth 8` for INC*. Identity hash depth stays 3 so keep-set grouping does not silently change.

### 2.5 Tools (plan-time)

Ran from `C:\dev\Dedupe`: `ai-brains preflight --summary` (inited); `ai-brains sync query` / `recall "embedded depth"` — 0094/0090/0069 decisions confirm depth owner = writer `max_embedded_depth`, extract exhaust → `ATTACH_DEPTH_LIMIT`, unique-eml ignores nests, identity **`MAX_EMBEDDED_IDENTITY_DEPTH=3`**. `ledgerful doctor --json` readyForPublish at plan-time; fold-in pass (2026-08-27) re-ran status (0 pending).

### 2.6 ai-brains decisions absorbed

| Memory | Use here |
|---|---|
| 0094: depth owner writer `max_embedded_depth`; 32 MiB per-nest; winner-only extract | Pass **one** value to materialize + writer |
| 0090: identity **`MAX_EMBEDDED_IDENTITY_DEPTH=3`**; leave D-0067 open | Do **not** retune hash depth |
| 0069: product ceiling 8 | Keep clamp [1, 8] |

### 2.7 How this advances the north star

Counsel-facing unique-PST must be honest. Today the only remaining INC* nested-export fail is depth-capped method-5 mail that operators cannot raise without a rebuild. Wiring the existing clamp makes the deliverable complete **or** still ledgered — never silently dropped.

### 2.8 Last-PR Cursor comments (merged #91, #90, #89, #88)

| PR | Comment | Verdict |
|---|---|---|
| **#91** (0100 docs) | No review/issue/inline comments | n/a |
| **#90** Bugbot | Recipient-table `table_subs.insert(0, matrix_nid)` leaves SLBLOCK unsorted once cell NIDs exist. Outlook searches SLBLOCK by NID. **Still live** (`production.rs` ~4710). | **Valid. Not this track.** Minted **0103-RecipientTcSlblockNidOrder** + `D-0100-slblock-nid-order`. |
| **#89** Bugbot | Oracle `"inputs"` allowlist strips `export_risk.inputs` | **Already 0102** (`D-0099-oracle-inputs-attest`). Re-verified still Proposed. Do not steal. |
| **#88** Bugbot | Window-edge bare URL skips `normalize_candidate` | **Already** `D-0097-window-edge-normalize` (parked, not Series P). |

0100 leftover nits (long-string cell-NID assert, HNBITMAPHDR 7-vs-8 unit test, stale 0093 comment on `RecipientTcTruncatedEvent`): **decline** — optional polish, not 0101, not new tracks.

### 2.9 Harness fold-in (2026-08-27)

Sources: `opencode-review.md`, `agy-review.md` (agy restates the Ready plan; load-bearing corrections are opencode). Verified against `main` @ `4726803`.

| Finding | Disposition |
|---|---|
| Writer cannot emit a 9th nest; CLI chain-of-9 is unbuildable | **Agree — fold** into DoD-2 / Phase 2 |
| `ExportSection` is Serialize-only; `serde(default)` is a no-op | **Agree — fold** |
| Cancel path `CancelledSummaryCtx` has no depth field | **Agree — fold** (thread effective depth; do not hardcode 3) |
| DoD-3 summary field never asserted in tests | **Agree — fold** |
| §8 `--test unique_pst` helper injects `--no-attachments` | **Agree — fold** (new test module) |
| Library clamp 0/9 untested | **Agree — fold** |
| Named-prop `scan_messages_with_depth` is a third consumer of the same value | **Already covered** (single assignment); **note** in plan so nobody adds a second knob |
| `--also-eml` × depth | **Decline** (flag is accepted-but-ignored `D-0071-also-eml`) |
| Bump `unique_export_report_v1` | **Decline** — one docs/CHANGELOG sentence that v1 gained an always-present key |
| Name drift `MAX_EMBEDDED_DEPTH` | **Agree — fold** (glossary; use `MAX_EMBEDDED_IDENTITY_DEPTH` for hash) |

**Depth names (do not conflate):**

| Name | Owner | Role |
|---|---|---|
| `WritePstOpts::max_embedded_depth` / unique-pst `--max-embedded-depth` | writer + CLI | This track’s extract/write knob, clamp [1, 8], default 3 |
| `MAX_EMBEDDED_IDENTITY_DEPTH` | `pst-reader` | 0090 hash recursion, **locked 3** |
| `DEFAULT_MAX_EMBEDDED_DEPTH` | `eml_pack` / `named_prop_map` | Default 3 on those surfaces; unique-eml OOS |

---

## 3. In scope

1. Clap `--max-embedded-depth` on `unique-pst` (long flag name locked). Default **3**. **Reject** values outside **1–8** as usage error (do not silently clamp operator typos). Help text states the range and default.
2. `UniquePstCliArgs.max_embedded_depth: u32`. `into_cli_args` copies the parsed value. `run_unique_pst_with_options` uses **that same value** for `materialize_nested_for_winner` and `WritePstOpts.max_embedded_depth` (replace the hardcoded `3u32`). Runtime `.clamp(1, 8)` remains as belt-and-suspenders for library callers.
3. GUI compile: `unique_wizard.rs` sets `max_embedded_depth: 3`. **No** wizard slider (out of scope; see §4 / §9).
4. Report honesty: `ExportSection.max_embedded_depth: u32` (plain field, **always serialize**, no `skip_serializing_if`, no `serde(default)` — the type is Serialize-only). Set from the **effective clamped** value at **both** summary build sites: the completed-export path and the cancel path (`CancelledSummaryCtx` + three construction sites ~1440 / ~1621 / ~1699). Do **not** name the field `inputs` (0102 strip hazard). Schema id stays `unique_export_report_v1`.
5. Tests (synthetic only; **new** `pst-dedup-cli` integration module — do **not** use `unique_pst.rs` `run_unique_pst`, which injects `--no-attachments`):
   - Default / omitted flag behaves as 3 (depth-4 nest → `ATTACH_DEPTH_LIMIT`; 4th nest absent).
   - `--max-embedded-depth 4` recovers that depth-4 nest (clean ledger for the chain).
   - **Ceiling pair (buildable):** 8-deep source @ `--max-embedded-depth 7` → `ATTACH_DEPTH_LIMIT`; same source @ **8** → clean. Writer-built sources hold **at most 8** nested levels (`depth >= max_depth` halt). A CLI chain-of-9 is **unbuildable**.
   - Writer unit: in-memory chain of 9 @ `max_embedded_depth: 8` → `embedded_depth_limit_hits > 0` and `embedded_messages_written <= 8` (extends `embedded_depth_cap_enforced`).
   - `--max-embedded-depth 0` and `9` (and non-integer) are **clap** usage errors.
   - Library path: `UniquePstCliArgs.max_embedded_depth` 0 → effective **1**, 9 → effective **8**, asserted on `summary.json` (bypass clap).
   - `summary.json` `export.max_embedded_depth` equals the effective value (default 3 and flag 4).
6. Docs: flag row in `docs/unique-pst-export.md`; one-liner in `docs/unique-pst-ediscovery-runbook.md` — if ledger shows `ATTACH_DEPTH_LIMIT`, re-export with `--max-embedded-depth 8` (ceiling); remaining hits stay disclosed. Nested paragraph in `docs/unique-pst-export.md` (0094) updated to say the CLI owns the knob. CHANGELOG / flag-doc sentence: v1 gained an always-present `export.max_embedded_depth` (unknown keys ignored; **no** schema-id bump).
7. Optional owner HITL: INC* unique-pst with `--max-embedded-depth 8` → `output/inc0102784-post-0101/` (operator-local). Not CI. Not required to mark the track complete if recorded as skipped in `review.md`.

---

## 4. Out of scope (do NOT do here)

- Unbounded depth / Relativity child-document extract / unique-eml `message/rfc822` (rest of **D-0067-embedded-depth**).
- Changing `MAX_EMBEDDED_IDENTITY_DEPTH` / `embedded-msg-hash/v1` budgets (**0090**).
- Splitting `ATTACH_DEPTH_LIMIT` into depth vs 32 MiB byte-budget codes.
- Raising the **default** from 3 to 8.
- Changing exit-64 / `ATTACH_SOFT_FAIL` policy for remaining over-depth nests.
- BCC default / `--include-bcc-recipients` (**0082**).
- Recipient TC / HNBITMAPHDR / attach-table TC (**0100** leftovers, **D-0093-attachment-tc-page**, **D-0100-hn-bitmap-hdr**).
- SLBLOCK NID order (**0103** / `D-0100-slblock-nid-order`).
- Oracle `export_risk.inputs` attest (**0102**).
- Desk wizard depth slider (compile default 3 only).
- CRC repair / ScanPST of source evidence.
- Frontend / Hermes (**0105+**).
- Schema-id bump of `unique_export_report_v1`.
- A CLI fixture with 9 nested levels (writer cannot emit it).

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0094 nested extract/write path stays the implementation (`materialize_nested_for_winner` + writer depth halt). Do not invent a second recursion.
- **P2:** 0098 verify-count honest (INC* 4055/4055) so HITL is comparable.
- *Verified to date:* hardcoded `nested_extract_depth = 3`; writer clamp 1–8; INC* 4 depth-limit rows; identity depth 3 separate.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Depth 8 RAM/time on hostile nests | Keep 0094 32 MiB per-nest + count budgets; fail closed; product ceiling 8 |
| Default 3 change surprises operators | **Default stays 3** |
| INC* 4 rows are byte-budget, not depth | HITL at 8 distinguishes; if unchanged, document residual — not a 0101 code fail; do not split event codes |
| Clap silent clamp hides typos (`99` → 8) | **Reject** outside 1–8 at clap; runtime clamp only for library |
| Materialize/writer depth mismatch | Single local `nested_extract_depth` (or equivalent) assigned once; named-prop scan **inherits** `write_opts_base.max_embedded_depth` — do not add a second knob |
| Identity hash silently follows export depth | **Lock:** hash depth unchanged |
| GUI struct literal miss | Wizard + UniquePstCliArgs test literals listed in `plan.md` |
| New summary key named `inputs` | Field is `max_embedded_depth` on `export` |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Wire:** `--max-embedded-depth` on unique-pst; default 3; clap **rejects** &lt;1 and &gt;8; the **same** effective value reaches `materialize_nested_for_winner` and `WritePstOpts.max_embedded_depth`. GUI/library compiles with default 3. Identity hash depth unchanged.
- [ ] **DoD-2 — Synthetic:** A depth-4 method-5 nest **fails** (ledger `ATTACH_DEPTH_LIMIT`) at 3 and **succeeds** at 4. An 8-deep nest **fails** at 7 and **succeeds** at 8. Writer unit: chain of 9 @ max 8 still halt-fires (`hits > 0`, written ≤ 8). Clap usage errors for 0 and 9. Library 0→1 and 9→8. No client PSTs in git. (CLI cannot prove “deeper than 8”: the writer never emits a 9th nest. Beyond-8 on a non-writer source stays fail-closed at the existing halt; optional INC* HITL is recovery at 8, not a >8 proof.)
- [ ] **DoD-3 — Honesty:** `summary.json` `export.max_embedded_depth` records the effective value (**asserted** on default 3 and `--max-embedded-depth 4`; cancel path echoes the requested/effective depth). Docs + runbook one-liner. Remaining over-depth nests still fail closed with `ATTACH_DEPTH_LIMIT` (exit 64 policy unchanged).
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger commit (`FEATURE` on `crates/pst-dedup-cli` at implement). Optional INC* HITL noted run or skipped. `D-0067-embedded-depth` **narrowed** (CLI done); unique-eml MIME / matter children remain. `D-0094-inc-resmoke` updated from HITL.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-dedup-cli --test unique_pst_depth
cargo test -p pst-writer embedded_depth
cargo test -p pst-dedup-cli --test embedded_msg_hash_0090
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
# before implement-track publish:
cargo test --workspace
```

`--test unique_pst_depth` is the intended new module name (re-verify at execute). Do **not** put attach-dependent depth tests in `--test unique_pst` (that binary’s helper injects `--no-attachments`). Keep at least: clap reject, depth-4 round-trip at 3 vs 4, 8@7 vs 8@8, writer chain-9@8 halt, identity-hash still green, `export.max_embedded_depth` on summary.json.

Optional HITL (not CI):

```powershell
# operator-local INC* only — never commit output/
.\target\release\pst-dedup.exe unique-pst `
  <INC0102784.pst> <INC0102784-2.pst> `
  --out <local>\inc0102784-post-0101\unique.pst `
  --report-dir <local>\inc0102784-post-0101 `
  --max-embedded-depth 8 --overwrite --json
```

---

## 9. Deferred roll (mandatory)

| Row | Disposition |
|---|---|
| **D-0067-embedded-depth** | **Absorb unique-pst CLI half.** After DoD, notes: unique-pst depth is operator-tunable 1–8 (default 3). **Residual stays:** unique-eml nested MIME `message/rfc822`; matter/Relativity child-document extract; 32 MiB per-nest; hard cap 8. **Do not close** the P1 row. |
| **D-0094-inc-resmoke** | **Partial.** Optional HITL at depth 8 is DoD-4. If skipped, row stays residual. If HITL clears the 4 rows, close or narrow in `review.md`. |
| **D-0093-attachment-tc-page** | **Decline.** Attach-table TC. |
| **D-0100-hn-bitmap-hdr** | **Decline.** HNBITMAPHDR pages 8/136/264. |
| **D-0100-slblock-nid-order** | **Minted this pass → 0103.** Not absorbed here. |
| **D-0099-oracle-inputs-attest** | **Decline** (0102). |
| **D-0097-window-edge-normalize** | **Decline** (parked polish). |
| **D-0074-gui** / wizard extras | **Decline** a depth slider (same class as other CLI-only flags). Compile default only. |
| **D-0071-also-eml** | **Decline** interaction note — unique-pst warns and ignores `--also-eml`; no depth coupling. |
| Other `docs/deferred.md` rows | **Decline** — not unique-pst extract depth. |

Schema-id bump for `export.max_embedded_depth`: **declined** (v1 always-present key + docs sentence). Med/high never parked here. No BCC-default track.

---

## 10. Product locks (do not reopen)

1. Never mutate source PST / Purview files.
2. Never commit client PSTs, `output/`, `evidence/`, or matter folders with client mail.
3. No `unwrap` / `expect` in production.
4. Crate boundaries: clap/orchestration in `pst-dedup-cli`; write clamp stays `pst-writer`; extract stays materializer + `pst-reader`. Do not teach `dedup-engine` a second depth policy.
5. No silent recipient/attach/count drops. `known_gap` only if a spec says so — this track does **not** add a known_gap for over-depth (still fail + ledger).
6. No in-tool ScanPST / CRC repair of evidence.
7. Default depth **3**; clamp **[1, 8]**; clap **rejects** outside that range.
8. Identity `MAX_EMBEDDED_IDENTITY_DEPTH = 3` unchanged.
9. 32 MiB per-nest budget unchanged.
10. `--include-bcc-recipients` default **off**.
