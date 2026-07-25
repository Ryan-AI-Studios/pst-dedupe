# Track 0063 — SecurityRedTeamFixes — Review

## Verdict: PASS WITH DEFERRED P3

**Track:** 0063-SecurityRedTeamFixes  
**Branch:** `feat/0063-security-redteam`  
**PR:** (opened after this review pack)  
**Product version:** 0.2.0-rc.1 (schema 39 unchanged)  
**Final cross-model:** Codex `gpt-5.6-luna` high — **PASS WITH DEFERRED P3** (`review.codex.final.md`)

---

## Scope

Time-boxed adversarial security review + P0/P1 fixes on Series I + RC surfaces:

- Encryption (0057), multi-user service (0058), platform SSO (0059), produce (0060), cloud CAS (0061)
- Locked folds: Path::join sandbox, key zeroize, adversarial PST parse, OIDC SSRF
- Light Series K path safety re-verify
- No Connect UX (0064); no new product features

---

## Reviewers / rounds

| Round | Reviewer | Result |
|---|---|---|
| Internal | Subagent | FAIL (process: deferred, tests, registries) → fixed |
| Internal re-review | Subagent | PASS WITH DEFERRED P3 |
| Codex 1 | gpt-5.6-luna high | FAIL (zeroize gaps, findings depth, NBT tests, close-out) |
| Fix + Codex 2 | gpt-5.6-luna high | FAIL (service passphrase lifetime, PST panic, D-0063-04 severity) |
| Fix + **Codex final** | gpt-5.6-luna high | **PASS WITH DEFERRED P3** |

---

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 Threat model | **Met** | `docs/security-threat-model-rc.md` |
| DoD-2 Findings table | **Met** | `findings.md` + full §3.2 checklist |
| DoD-3 All P0 fixed + tests | **Met** | SSRF, XBLOCK cap, btree/subnode cycle, sandbox, truncated header |
| DoD-4 P1 fixed or deferred | **Met** | PMK/passphrase zeroize fixed on production paths; residuals D-0063-* |
| DoD-5 Supply chain | **Met** | `cargo audit` allowlisted warnings; `cargo deny check` OK |
| DoD-6 Gates | **Met** | fmt, clippy `-D warnings`, `cargo test --workspace` green |
| DoD-7 Recorded | **Met** | findings.md, review.md, deferred.md, registries, ledger |
| DoD-8 Handoff | **Met** | Residuals for operators / 0064 listed |

---

## P0 fixes shipped

| ID | Fix |
|---|---|
| F-0063-01 | OIDC SSRF `validate_idp_url_for_ssrf` + discovered endpoint re-check; HTTPS-only; redirect none |
| F-0063-02 | XBLOCK/XXBLOCK 64 MiB assemble cap before `with_capacity` |
| F-0063-03/04 | NBT/BBT + subnode visited + depth 32; production `build()` cycle tests |
| F-0063-05 | Path sandbox absolute/component harden |
| (Codex) | Internal block `payload.len() < 2` → `DataTruncated` (no panic) |

## P1 fixes / residuals

| Item | Disposition |
|---|---|
| PMK ZeroizeOnDrop | **Fixed** (`Pmk`) |
| Passphrase heap (service/CLI/unlock) | **Fixed** (`ZeroizingString`; ServeConfig take-on-open) |
| IdP ClientSecret dependency | **D-0063-04 P3** |
| Env passphrase residual | **D-0063-01 P3** |
| Desk egui String widgets | **D-0063-05 P3** |
| DNS rebinding race | **D-0063-02 P3** |
| 64 MiB XBLOCK cap scale | **D-0063-03 P3** |

---

## Gate results (orchestrator observed)

```text
cargo fmt --all --check                          OK
cargo clippy --workspace --all-targets -- -D warnings  OK
cargo test --workspace                           OK (exit 0)
cargo audit                                      OK (3 allowed warnings)
cargo deny check                                 OK
```

---

## Deferred ledger

Recorded in `docs/deferred.md`: **D-0063-01** … **D-0063-05**.

Pre-existing residuals unchanged: D-0057-07/09, D-0058-02/04, D-0062-codesign, etc.

---

## Handoff

| Audience | Note |
|---|---|
| **0064** | API security hardened; Connect UX only |
| Operators | LAN cleartext residual (D-0058-02); env passphrase (D-0063-01); unsigned RC (D-0062-codesign) |
| **0062** | deny/audit policy not weakened |

---

## Completion decision

Engineering DoD met. Cross-model final gate **PASS WITH DEFERRED P3**. Track **Completed**.
