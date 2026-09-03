# 0127 — Embedded-depth operator hint (default 3 footgun)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Do **not** raise `--max-embedded-depth` default. Identity `MAX_EMBEDDED_IDENTITY_DEPTH` stays **3**.
> Do **not** close `D-0067-embedded-depth` (matter children). No BCC-default. Not frontend.
> Do not steal **0100–0104** IDs. Do not restrip keep-set CRC.

- **Track ID:** 0127-EmbeddedDepthOperatorHint
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst nested extract. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03).
- **Status:** Ready — not started
- **Depends on:** **0101 Completed** (`--max-embedded-depth` 1–8, default **3**) · **0106** unique-eml depth
- **Spec authored:** 2026-09-03 (placeholder → Ready)
- **Series:** U (unique-export INC* HITL residuals)
>
> **Closes / absorbs:** `D-0127-embedded-depth-operator-hint`. Does **not** close `D-0067-embedded-depth`.
> **HITL:** operator-local INC* at default 3 historically `ATTACH_DEPTH_LIMIT` + exit 64; at `--max-embedded-depth 8` (2026-08-29 / 2026-09-02) depth-limit **0**. Never commit those PSTs. CI uses `unique_pst_depth` / `unique_eml_depth` fixtures.
>
> **Harness fold-in (2026-09-03):** `opencode-review.md` + `agy-review.md`. Hint keys on materializer/writer depth-limit events, not `attachments_failed_by_reason` (None / empty histogram when `--attach-ledger off`). unique-eml stderr even on `--json` / allow-partial success. Dual clap help (`unique_pst_cmd.rs` + `main.rs` UniqueEml). Configured-cap assertion at depth 7. See §2.9 / §9.

---

## 1. Objective

When unique-pst/unique-eml ledgers `ATTACH_DEPTH_LIMIT`, the operator must see **`--max-embedded-depth`**, the **configured cap**, and that deeper nests were skipped — not a silent “it wrote.” Default 3 stays correct for identity hash; INC*-class Purview nests often need **8**.

This is **correctness of operator disclosure**, not a parser change. Unique-export still fail-closes on leftover depth-limit rows (exit 64) unless `--allow-partial-fidelity`.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0101** shipped the flag. Runbook already says re-run with 8 if the ledger shows `ATTACH_DEPTH_LIMIT`. Operators who omit the flag still discover it via exit 64. HITL 2026-09-02 at **8** is clean (`ATTACH_DEPTH_LIMIT=0`).

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `f8cb240`; re-verify at execute)

| Surface | Fact |
|---|---|
| Schema | **41**. N/A this CLI track (no bump). |
| clap `unique-pst` | `--max-embedded-depth` default **3**, `parse_max_embedded_depth_arg` rejects outside 1–8. Help: “Deeper nests ledger ATTACH_DEPTH_LIMIT.” |
| `tests/unique_pst_depth.rs` | Default 3 → `ATTACH_DEPTH_LIMIT` ≥ 1; depth 4 recovers 4th nest. **Does not** assert stderr names the flag/cap. |
| `tests/unique_eml_depth.rs` | Same pattern for unique-eml. |
| Runbook | Nested `.msg` deeper than 3 → re-run with 8. INC* 2026-08-29 cited. |
| `D-0067-embedded-depth` | Residual **matter/Relativity child-document extract**. Do not close. |

### 2.3 Pins

CLI on stable Rust. No schema bump. No daemon.

### 2.4 Tools / last-PR

- `ai-brains preflight` (inited; **4857** pinned). Recall 0101 default-3 / D-0067 not closed.
- Ledger 0 pending / 0 drift. Impact **LOW** (conductor docs). Federated `output/` 5000-file budget — ignore; do not `git add` INC* packs.

### 2.8 Last-PR Cursor comments

PRs **#146, #145, #144, #143** (last four merged product PRs at plan-time): inline comments **0**, reviews **0**, Bugbot usage-limit only on #145. **Decline** — nothing to fold. Do not mint a BCC-default track or steal 0100–0104.

### 2.9 Product locks

When depth-limit events fired (**> 0**), not when the optional histogram says so:

- **Count source:** materializer `embedded_extract_limit` / writer `ATTACH_DEPTH_LIMIT` events (available in every `--attach-ledger` mode). Do **not** key solely on `export.attachments_failed_by_reason` — that field is `None` in Off (`apply_to_export_section`; unique-eml `ledger_summary_fields` Off tuple), and Off returns before incrementing `failed_by_reason`.
- stderr one line: `--max-embedded-depth=<configured>` (the value used this run, e.g. `7` not hardcoded `3`) + that deeper nests were skipped.
- unique-pst: hint via `emit_log` (stderr + `on_log`). No new output channel. Fires under `--json`.
- unique-eml: write the same class of line to **stderr** (`writeln!(std::io::stderr(), …)` or `eprintln!`) **unconditionally** when the count is > 0 — including `--json` and `--allow-partial-fidelity` success. Do not gate on `classified_exit != Success` (that path is the only existing unique-eml `writeln` besides the RI hint).
- JSON / summary already has `export.max_embedded_depth` (unique-pst) / `max_embedded_depth` (unique-eml) — keep it.
- clap `--help`: one extra sentence that INC*-class / deep method-5 Purview nests often need **8**; default **3** is identity-safe, not “always enough.” Update **both** clap sites: `unique_pst_cmd.rs` (`UniquePstClapArgs`) **and** `main.rs` (`UniqueEml`). Keep `default_value_t = 3` (do not “fix” to `default_value`).
- Runbook: add 2026-09-02 pack (`output/inc0102784-post-0126/`, gitignored) next to 2026-08-29; state default-3 is the footgun.

**Do not** change default 3. **Do not** change identity depth 3. **Do not** invent a new exit code.

---

## 3. In scope

`pst-dedup-cli` unique-pst + unique-eml stderr/help when depth-limit fires; `docs/unique-pst-ediscovery-runbook.md` (+ export.md one-liner if needed); extend `unique_pst_depth` / `unique_eml_depth` assertions.

## 4. Out of scope

Raising default to 8. Matter child extract (`D-0067`). BCC-default. Frontend. Keep-set CRC restrip. 0128–0132.

## 5. Preconditions

0101/0106 shipped. Fixture depth tests exist.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Operators think 8 is always required | Copy: INC*-class / deep method-5 only |
| Raising default “while we’re here” | Forbidden |
| Hint only on unique-pst | Same stderr contract on unique-eml (including `--json`) |
| Hint keyed on `attachments_failed_by_reason` | Off ledger → `None` / empty histogram; key on extract/write events |
| Hardcoded `--max-embedded-depth=3` in the hint | Print the **configured** cap; assert `=7` in the depth-7 fixture |

## 7. Definition of Done

- [ ] **DoD-1:** Fixture at default 3: stderr names `--max-embedded-depth=` **and the configured cap** (`3`) when depth-limit events > 0. Depth-7 fixture (`ceiling_8_fails_at_7_succeeds_at_8` / unique-eml equivalent) asserts `--max-embedded-depth=7`. Hint still fires if execute adds an `--attach-ledger off` smoke (optional; Full is the default fixture).
- [ ] **DoD-2:** `--help` on **both** unique-pst and unique-eml + runbook: default 3 vs INC* need-8. Default remains 3 (`default_value_t = 3` still).
- [ ] **DoD-3:** unique-eml stderr contains `--max-embedded-depth=` in `tests/unique_eml_depth.rs` (not JSON-only). Hint emits on `--json` / allow-partial success. `D-0067-embedded-depth` still open.
- [ ] **DoD-4:** `cargo test -p pst-dedup-cli --test unique_pst_depth --test unique_eml_depth`; fmt/clippy as gate.
- [ ] **DoD-5:** `review.md`; registry Completed; CHANGELOG; ledger FEATURE.

## 8. Verification

```powershell
Set-Location C:\dev\Dedupe
cargo test -p pst-dedup-cli --test unique_pst_depth --test unique_eml_depth
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0127-embedded-depth-operator-hint** | **Absorb — this track.** |
| **D-0067-embedded-depth** | **Remain** (matter children). |
| BCC-default / `D-0108-keepset-crc-retaint` | **Decline.** |
| Bugbot #143–#146 | **Decline.** |
| opencode-O1 pinned-count 4857 vs 4858 | **Decline** (trivia). |

## 10. Unblocks

Operators can set `--max-embedded-depth 8` before a counsel unique looks broken. **0128** is independent copy.
