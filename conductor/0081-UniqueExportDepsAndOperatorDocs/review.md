# Track Completion Audit — 0081-UniqueExportDepsAndOperatorDocs

## Verdict: PASS WITH DEFERRED P3

**Date:** 2026-07-29  
**Branch:** `track/0081-unique-export-deps-docs`  
**Series:** L closer (unique export hardening)

### Reviewers / rounds

| Round | Reviewer | Verdict |
|---|---|---|
| Internal r1 | subagent DoD + code | FAIL (messages CSV missing `source_id`; DoD-17 open) |
| Fix | source_id column + qc-pst summary.inputs resolve | — |
| Internal r2 | subagent | PASS WITH DEFERRED P3 |
| Codex 1 | gpt-5.6-luna high | FAIL (`-Jobs` no-op; `-no.bak` hygiene) |
| Fix | remove `-Jobs`; remove bak; remove fixture pollution | — |
| Codex 2 | gpt-5.6-luna high | FAIL (reqwest dual provenance; fixture) |
| Fix | correct reqwest invert notes; remove `fixtures/keep_set_summary.json` | — |
| Codex 3 | gpt-5.6-luna high | FAIL (stale Outlook import-only docs; threshold configurability) |
| Fix | align Outlook claims; label fixed vs CLI thresholds | — |
| **Codex final** | **gpt-5.6-luna high** | **PASS WITH DEFERRED P3** (no remaining P0–P2) |

---

## Scope Reviewed

- Dependency pin audit + safe PATCH/MINOR bumps (`Cargo.lock`)
- `deny.toml` dead-ignore prune
- `--ledger-path-mode full|basename` + trailing `source_id` on `export_messages.csv`
- Standalone `qc-pst` basename resolve via `summary.inputs[source_id]`
- `scripts/unique-pst-timing.ps1`
- `docs/unique-pst-ediscovery-runbook.md` (§0–11)
- Links: README, `operator-golden-path.md` Path B, `unique-pst-export.md`
- Deferred hygiene: D-0073-basename closed; D-0077-repair-diff closed; D-0078 runbook constraint
- Outlook client-retirement honesty re-verified 2026-07-29

---

## Dependency audit table

**Research date:** 2026-07-29

| crate | workspace pin | lock (after) | crates.io max | decision | notes |
|---|---|---|---|---|---|
| clap | `"4"` | **4.6.4** | ~4.6.4 | **PATCH** | was 4.6.2 |
| serde_json | 1.x | **1.0.151** | ~1.0.151 | **PATCH** | was 1.0.149 |
| thiserror | 2.x | **2.0.19** (+ dual 1.0.69) | ~2.0.19 | **PATCH** 2.x | dual 1.x via oauth2/openidconnect **ACCEPT_DUAL** |
| camino | 1.x | **1.2.5** | ~1.2.5 | **PATCH** | was 1.2.4 |
| uuid | 1.x | **1.24.0** | ~1.24.0 | **MINOR** | was 1.23.1 |
| sha2 | product **0.11** | 0.11.0 (+ **0.10.9** dual) | 0.11.0 | **KEEP** / **ACCEPT_DUAL** | 0.10.9 ← SSO crypto + lopdf/pdf-extract |
| md-5 | product **0.10** | 0.10.6 | 0.11.0 | **KEEP** | EDRM MIH product pin (Q5) |
| rusqlite | — | 0.40.1 | 0.40.1 | **KEEP** | |
| eframe | 0.34 | 0.34.2 | 0.35.x | **DECLINE_MAJOR** | mid-RC |
| reqwest | product 0.12 | **0.12.28** active; **0.13.4** lock residual | 0.13.4 | **DECLINE_MAJOR** | Active default: desk + matter-ai + oauth2→openidconnect → **0.12.28**. Residual 0.13.4 via `object_store` / matter-storage **cloud-s3** only (not oauth dual). |
| aes-gcm | 0.10 | 0.10.3 | 0.11 | **DECLINE_MAJOR** | |
| argon2 | 0.5 | 0.5.3 | 0.5.3 | **KEEP** | |
| rand | multi | **0.8.7 / 0.9.5 / 0.10.2** | 0.10.2 | **KEEP** all | **RUSTSEC-2026-0097** floors already met; no force-unify |

### Dual invert notes

- **sha2@0.10.9:** openidconnect/oauth2/ed25519-dalek/p256/p384 (SSO) + lopdf → pdf-extract
- **thiserror@1.0.69:** oauth2 / openidconnect → matter-service
- **rand:** past RUSTSEC-2026-0097 floors on all lines
- **reqwest:** see table (corrected after Codex review)

### deny.toml

Pruned dead `advisory-not-detected` ignores: RUSTSEC-2026-0186, 0190, 0194, 0195.  
Retained live: RUSTSEC-2023-0071 (rsa), RUSTSEC-2026-0192 (ttf-parser).  
`cargo deny check`: advisories/bans/licenses/sources **ok**.

---

## Requirement / DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| 1 Dep audit | **Met** | This `review.md` table + research date |
| 2 Safe bumps | **Met** | Approved PATCH/MINOR only |
| 3 Runbook §0–11 | **Met** | `docs/unique-pst-ediscovery-runbook.md` |
| 4 Links | **Met** | README, Path B, unique-pst-export |
| 5 Deferred | **Met** | D-0073/D-0077 closed; D-0078 constraint |
| 6 Exit honesty | **Met** | No blanket retry exit 5 + PS switch |
| 7 ScanPST | **Met** | Copy-only + two-command count-diff |
| 8 deny | **Met** | Pruned + green |
| 9 Basename | **Met** | CLI default full; tests; source_id |
| 10 Timing script | **Met** | `scripts/unique-pst-timing.ps1` |
| 11 Outlook | **Met** | Dated MS citations; open/add wording aligned |
| 12 Custody | **Met** | Matter Archive mandate; ≠ full de-id |
| 13 Thresholds | **Met** | CLI vs fixed constants labeled |
| 14 Disposition | **Met** | Firm policy; no product wipe |
| 15 Accident dirs | **Met** | Absent |
| 16 Tests | **Met** | fmt / clippy -D / workspace test / deny green |
| 17 Recorded | **Met** | this file + conductor Completed + ledger |

---

## Findings disposition

| ID | Severity | Disposition |
|---|---|---|
| messages CSV no source_id | P2 | **Fixed** — trailing `source_id` column |
| qc-pst basenamed paths | P2 | **Fixed** — resolve via `summary.inputs` |
| timing `-Jobs` no-op | P2 | **Fixed** — parameter removed |
| reqwest dual misstated | P2 | **Fixed** — audit notes corrected |
| stale Outlook import-only | P2 | **Fixed** — unique-pst-export + D-0080-newoutlook |
| threshold configurability | P2 | **Fixed** — CLI vs fixed constants labeled |
| `-no.bak` / keep_set fixture | P3 | **Fixed** — removed |
| D-0078-retryable code | P3 | **Deferred residual** (runbook constraint satisfied) |
| GUI path-mode control | P3 | Out of scope; default Full inert |

---

## Verification evidence (orchestrator)

```
cargo fmt --all --check                          PASS
cargo clippy --workspace --all-targets -- -D warnings  PASS
cargo test --workspace                           PASS
cargo deny check                                 PASS (advisories/bans/licenses/sources ok)
```

Internal r2 + Codex final gate: no remaining P0–P2.

---

## Deferred residual

- **D-0078-retryable** — code residual (`retryable: bool` JSON field not shipped); **0081 runbook forbids blanket retry of exit 5**.
- Pre-existing Series L residuals unchanged except D-0073-basename / D-0077-repair-diff **closed**.

---

## Completion decision

**PASS WITH DEFERRED P3.** Series L unique-export hardening docs closed. Engineering DoD met; fresh Codex gpt-5.6-luna high final gate clean of findings greater than P3.
