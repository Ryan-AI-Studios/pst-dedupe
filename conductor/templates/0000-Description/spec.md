# 0000 — <Track Title>

> Template. Copy `templates/0000-Description/` to `####-PascalDescription/`, fill every `<…>`, then
> register the track in `../conductor.md`. Every track MUST keep a clear Definition of Done (§7).

- **Track ID:** 0000-Description
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → `<section / series>`
- **Cross-repo contract:** n/a (unless noted)
- **Status:** Ready — not started

---

## 1. Objective
<One or two sentences: what this track delivers and why it matters now.>

## 2. Context (read before starting)
- Product plan: `C:\dev\Dedupe-plan.md`
- Guardrails: `../TRACK-GUARDRAILS.md`
- Existing crates: `pst-reader`, `dedup-engine`, `pst-dedup-cli`, `pst-dedup-gui`
- **Desktop rule:** no user-managed servers/daemons; single-exe launch path.

## 3. In scope
1. <…>

## 4. Out of scope (do NOT do here)
- <Adjacent work that belongs to another track; name the track.>

## 5. Preconditions & dependencies
- **P1 (blocking):** <…>
- *Verified to date:* <facts already confirmed.>

## 6. Risks
| Risk | Mitigation |
|---|---|
| <…> | <…> |

## 7. Definition of Done
Complete only when ALL hold:
- [ ] **DoD-1 —** <objective, checkable criterion>
- [ ] **DoD-2 —** <…>
- [ ] **DoD-n — Recorded:** outcome in `review.md`; registry status set to **Completed**; ledger
      transaction committed in the execution repo (category `ARCHITECTURE|FEATURE|INFRA|SECURITY|REFACTOR|BUGFIX|DOCS|CHORE`).

## 8. Verification commands (reference)
```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# track-specific commands here
```
