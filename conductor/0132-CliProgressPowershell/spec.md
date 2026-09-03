# 0132 — CLI progress vs PowerShell stderr

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> **`--json` stays on stdout.** Progress stays off the JSON stream.
> Decline `--progress-file` as CLI expansion — **docs + wrapper comment** is enough.
> Not frontend. No BCC. Do not steal **0100–0104**.

- **Track ID:** 0132-CliProgressPowershell
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** unique-pst progress-on-stderr. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-03).
- **Status:** Ready — not started
- **Depends on:** unique-pst progress-on-stderr (**0071**)
- **Spec authored:** 2026-09-03 (placeholder → Ready)
- **Series:** U (unique-export INC* HITL residuals)
>
> **Closes / absorbs:** `D-0132-cli-progress-powershell`.
> **HITL:** 2026-09-02 — PowerShell records `unique-pst: stage=…` as `NativeCommandError` (`FullyQualifiedErrorId: NativeCommandError`). Harmless; scares operators.
>
> **Harness fold-in (2026-09-03):** `opencode-review.md` + `agy-review.md`. Exact `cmd /c` + `Start-Process` snippets; no `&&`; pointer from day-1 `scan --json \| Set-Content`; timing-script explains `RedirectStandardError = $false`. See §2.9 / §7.

---

## 1. Objective

Windows PowerShell operators should be able to capture unique-pst progress without a wall of “errors.” JSON output must stay a parseable stdout document.

This is **operator capture honesty**, not a progress-protocol rewrite.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

Progress is correctly on stderr so `--json` stdout stays a document. PowerShell treats native-exe stderr as error records. Interactive `pst-dedup unique-pst …` still hits `NativeCommandError`. The timing harness already avoids it.

### 2.2 Live APIs (plan-time 2026-09-03, HEAD `f8cb240`; re-verify at execute)

| Surface | Fact |
|---|---|
| Schema | **41**. N/A. |
| Progress | stderr (`unique-pst:` / `stage=`). JSON on stdout when `--json`. |
| `scripts/unique-pst-timing.ps1` | `ProcessStartInfo` with `RedirectStandardError = $false` — child process, **not** PowerShell native-error wrapping. Interactive CLI still wraps. |
| Runbook | No PowerShell capture recipe. No bashisms; no `&&`. |

### 2.3 Pins

Do not move `unique-pst:` lines onto stdout when `--json`. Do not change JSON schema. Do not add `--progress-file` unless execute proves docs+cmd cannot close the row (Ready plan: **docs are enough**).

### 2.4 Tools (plan-time)

`ai-brains preflight` inited; ledger 0 pending / 0 drift; `scan --impact` LOW (conductor docs). Federated `output/` budget — ignore INC* packs.

### 2.8 Last-PR Cursor comments

PRs **#146, #145, #144, #143**: inline **0**, reviews **0**, Bugbot usage-limit only. **Decline**.

### 2.9 Product locks

- Runbook PowerShell section must include **exact** native snippets (no `&&`, no bash `2>&1`):
  1. `cmd /c "pst-dedup.exe unique-pst [args] 2> progress.log"`
  2. `Start-Process -FilePath … -ArgumentList … -NoNewWindow -Wait -PassThru` (or point at `scripts/unique-pst-timing.ps1`)
- State that `NativeCommandError` on stderr progress is PowerShell wrapping, not a product failure. The recipe applies to **any** `pst-dedup` native invocation that emits stderr (including the day-1 `scan --json | Set-Content` sketch) — add a pointer from that sketch to this section.
- Comment at the top of `scripts/unique-pst-timing.ps1`: why `RedirectStandardError = $false` (avoid `NativeCommandError`); pointer to the runbook section.
- If any CLI code changes: `--json` stdout remains parseable JSON in a fixture. **Docs-only close is allowed.** No `--progress-file`.

---

## 3. In scope

`docs/unique-pst-ediscovery-runbook.md` capture recipe; timing-script comment. Optional one-line README pointer if unique-pst docs already mention stderr.

## 4. Out of scope

Moving progress onto JSON stdout. Changing JSON schema. `--progress-file` CLI flag. Logging CRC page/block lines (0077). ConPTY / Windows Terminal theming. Frontend. BCC-default. 0131 hint wording.

## 5. Preconditions

Progress-on-stderr is the Unix-correct contract. Timing script already avoids NativeCommandError for harness runs.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Progress on stdout breaks `--json` | Forbidden |
| Wrapper-only fix ignored | Document in runbook **and** timing script comment |
| Adding `--progress-file` “while here” | Forbidden unless docs cannot close DoD |

## 7. Definition of Done

- [ ] **DoD-1:** Runbook includes the two native snippets in §2.9 (no `&&`). States NativeCommandError wrapping. Pointer from day-1 `scan --json | Set-Content` to the capture section.
- [ ] **DoD-2:** Timing-script header explains `RedirectStandardError = $false` and points at the runbook. `--json` stdout contract unchanged.
- [ ] **DoD-3:** If code changes: fixture `--json` stdout is parseable. Docs-only execute skips this with a review note.
- [ ] **DoD-4:** No bashisms in the snippet. fmt/clippy only if Rust changes.
- [ ] **DoD-5:** `review.md`; registry Completed; CHANGELOG if user-facing; ledger **DOCS**.

## 8. Verification

```powershell
Set-Location C:\dev\Dedupe
# Docs-only close allowed. If unique-pst CLI changes:
# cargo test -p pst-dedup-cli -- --ignored  # only if execute adds a json-stdout fixture
```

Owner may smoke: interactive unique-pst vs `cmd /c … 2> log` — optional, no INC* in git.

## 9. Deferred

| ID | Disposition |
|---|---|
| **D-0132-cli-progress-powershell** | **Absorb — this track.** |
| `--progress-file` | **Decline** (docs + existing timing script). |
| 0077 CRC log bounding | **Decline** (already shipped). |
| Moving JSON to a file by default | **Decline.** |
| Bugbot #143–#146 | **Decline.** |

## 10. Unblocks

PowerShell operators can keep a progress log without thinking unique-pst failed. Parallel with **0130** / **0131**.
