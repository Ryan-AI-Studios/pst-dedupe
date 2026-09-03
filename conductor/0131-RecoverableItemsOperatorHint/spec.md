# 0131 — Recoverable Items operator hint (flag already exists)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **`--prefer-folder-class` stays opt-in.** 0075 defaults inert. Do not change `first_seen` / source-rank defaults.
> Do not treat Recoverable Items as a defect. No BCC-default. Not frontend. Do not steal **0100–0104**.

- **Track ID:** 0131-RecoverableItemsOperatorHint
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst keep-set policy. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03).
- **Status:** Ready — not started
- **Depends on:** **0075** folder-class ladder (opt-in)
- **Spec authored:** 2026-09-03 (placeholder → Ready)
- **Series:** U (unique-export INC* HITL residuals)
>
> **Closes / absorbs:** `D-0131-recoverable-items-operator-hint`.
> **HITL:** 2026-09-02 — 826 / 4055 winners (~20%) from Recoverable Items / Purges. CLI already warns via `recoverable_items_hint`. `--source-rank INC0102784.pst` then `-2` kept all **8** cross-file MID dups on the primary file. Folder-class was off — 826 RI winners is expected. Operator-local; never commit PSTs.
>
> **Harness fold-in (2026-09-03):** `opencode-review.md` + `agy-review.md`. unique-eml **already** calls `recoverable_items_hint` (`eprintln!` when `!json`); unique-pst uses `emit_log` always. Pin ledger DOCS. Do not rewrite the keepset string. See §2.9 / §9.

---

## 1. Objective

Operators running Purview mailbox searches should know dumps include Recoverable Items / Purges, that unique-pst **keeps those copies** unless they pass `--prefer-folder-class`, and that the flag is **opt-in** because it changes the keep-set.

This is **golden-flow docs** (plus a ranking-unchanged check). Not a ranking rewrite.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

Runbook §3 already has a one-line RI row. INC* showed ~20% winners from RI/Purges without the flag — expected, not a bug. Operators still ask whether unique-pst “missed live mail.”

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `f8cb240`; re-verify at execute)

| Surface | Fact |
|---|---|
| Schema | **41**. N/A. |
| `recoverable_items_hint` (`keepset.rs` ~1992) | Complete stderr string: `N winner(s) came from Recoverable Items folders; consider re-running with --prefer-folder-class to prefer live-mailbox copies`. Single `format!` (no newline). |
| unique-pst `unique_pst_cmd.rs` ~2095 | `emit_log(… "note: {hint}")` when count > 0 (including `--json`). |
| unique-eml `unique_eml_cmd.rs` ~1032 | **Does** call the same helper; `eprintln!("note: {hint}")` only when `!args.json`. `--json` unique-eml currently suppresses the hint (existing; this docs track does **not** change that). |
| Runbook ~146 | Thin: `--prefer-folder-class` when purging soft-deleted noise is matter policy. |
| Tests | `winners_from_recoverable_signal_only` in `keepset.rs` (hint `is_some`; add `--prefer-folder-class` substring assert). |
| PowerShell wrap | Truncated-looking lines in HITL logs are **0132**, not a broken hint string. |

### 2.3 Pins

Flag stays opt-in. Do not default-on. Do not change source-rank / first_seen. Do not treat RI as `defect`.

### 2.4 Tools (plan-time)

`ai-brains preflight` inited; ledger 0 pending / 0 drift; `scan --impact` LOW (conductor docs). Federated `output/` budget — ignore INC* packs.

### 2.8 Last-PR Cursor comments

PRs **#146, #145, #144, #143**: inline **0**, reviews **0**, Bugbot usage-limit only. **Decline**.

### 2.9 Product locks

- Runbook golden-flow: optional `--prefer-folder-class`; prefers Sent/live over RI/Purges; INC*-class dumps can keep ~20% RI winners **without** the flag (2026-09-02, gitignored pack). Source-rank is independent and stayed correct on the 4GB split.
- Runbook must not imply every CLI emits the hint the same way: **unique-pst** always `emit_log`; **unique-eml** `eprintln` when not `--json`. Both honor `--prefer-folder-class`. Do **not** add unique-eml `--json` hint in this track.
- Do not insert a newline in `recoverable_items_hint` (`keepset.rs`). Do not “fix” a truncated PowerShell error record (0132).
- Default ranking unchanged: existing keep-set tests still pass; clap/defaults for folder-class remain off.
- `winners_from_recoverable_signal_only`: assert hint text contains `--prefer-folder-class` (cheap string guard).
- Optional **owner HITL** (not CI): re-run INC* with the flag; report winner-from-RI delta only. No PST in git.

---

## 3. In scope

`docs/unique-pst-ediscovery-runbook.md` golden-flow expansion; confirm hint is one complete source line. Ranking tests as a no-change check. Optional owner HITL note in `review.md`.

## 4. Out of scope

Default-on folder-class. Desk wizard ordered `--folder-rank` (`D-0075-gui`). Changing source-rank / first_seen. Treating RI as a defect. PowerShell NativeCommandError (**0132**). Frontend. BCC-default.

## 5. Preconditions

0075 ladder shipped; hint function exists.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Default-on would hide dumps counsel asked for | Stay opt-in |
| HITL with flag looks like a required re-export | Docs: optional matter policy |
| “Fixing” stderr wrap | Leave to 0132 |

## 7. Definition of Done

- [ ] **DoD-1:** Runbook golden-flow: optional flag, what it prefers, INC* ~20% RI winners expected without it; unique-pst vs unique-eml hint channels as in §2.9. Flag remains opt-in.
- [ ] **DoD-2:** Default ranking unchanged (`cargo test -p dedup-engine recoverable` still green). Hint source string still names `--prefer-folder-class` (unit assert substring).
- [ ] **DoD-3:** Owner HITL with the flag is **optional**; if skipped, `review.md` says so. Never commit INC* PSTs.
- [ ] **DoD-4:** Targeted tests; docs-only close allowed (no Rust required except the optional substring assert).
- [ ] **DoD-5:** `review.md`; registry Completed; CHANGELOG if user-facing docs; ledger **DOCS**.

## 8. Verification

```powershell
Set-Location C:\dev\Dedupe
cargo test -p dedup-engine recoverable
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Docs-only execute may skip clippy if no Rust change; still run the recoverable test as a no-regression check.

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0131-recoverable-items-operator-hint** | **Absorb — this track.** |
| **D-0075-gui** ordered lists | **Decline** (Desk residual). |
| Default folder-class on | **Decline.** |
| Bugbot #143–#146 | **Decline.** |
| AGY-131-01 “unique-eml never calls hint” | **Decline as stated** (it does at ~1032). **Partial fold:** document `!json` eprintln vs unique-pst `emit_log`. |

## 10. Unblocks

Purview operators can decide folder-class without thinking unique-pst “kept trash.” Parallel with **0130** / **0132**.
