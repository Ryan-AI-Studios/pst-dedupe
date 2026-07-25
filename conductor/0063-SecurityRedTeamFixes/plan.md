# 0063 — Security red team + P0/P1 fixes — Plan

> **Ledger:** `ledgerful ledger start 0063-securityredteamfixes --category SECURITY --message "Red team: Path::join, zeroize, PST DoS, OIDC SSRF + Series I surfaces"`

**Status:** **Completed** — Codex luna final PASS WITH DEFERRED P3.

Execute in `C:\dev\dedupe`. Map phases to DoD in `spec.md` §7.

---

## Locks (do not violate)

1. Time-boxed review → fix P0/P1 only.  
2. Every fix: regression test where practical.  
3. No client data; no secrets in git.  
4. No Connect UX / new auth products.  
5. Explicit adversarial coverage:  
   - **Path::join** absolute RHS + `..` (§3.2.2)  
   - **Zeroize** key/passphrase material (§3.2.1)  
   - **PST cycle/alloc DoS** (§3.2.3)  
   - **OIDC SSRF** (§3.2.4)  
6. Do not weaken 0062 deny/audit.

### Review folds (accepted)

| # | Fold | Spec |
|---|---|---|
| 1 | Path::join sandbox escape | §3.2.2 |
| 2 | Heap residue of crypto material | §3.2.1 |
| 3 | Adversarial PST OOM/loop | §3.2.3 |
| 4 | OIDC discovery SSRF | §3.2.4 |

### Deferred roll-in

| Item | Action |
|---|---|
| D-0057-09 env passphrase | Review + zeroize/clear if P1 |
| D-0059 path sandbox | Must re-verify join semantics |
| D-0058-02 LAN TLS | Residual unless bypass |
| Desk Connect | **0064** |

### Research notes

- Workspace already has `zeroize` + `zeroize_string` / PMK helpers — **audit coverage**, do not assume complete.  
- `assert_path_under_root` exists — still test absolute-RHS join.  
- pst-reader uses size-driven `with_capacity` in places — review as DoS.  
- OWASP ASVS 5.0 + multi-tenant cheat sheet + SSRF classics.

---

## Phase 0 — Precondition

- [x] Confirm 0062 on `main` (0.2.0-rc.1).  
- [x] Read sandbox.rs, pmk.rs, OIDC client, pst-reader block/btree walks.  
- [x] `ledgerful ledger start 0063-securityredteamfixes --category SECURITY --message "..."`.

---

## Phase 1 — Threat model + adversarial review → DoD-1–2

- [x] Threat model includes **adversarial PST** + **SSRF** + **key residual**.  
- [x] Checklist §3.2 including four folds.  
- [x] findings.md drafted.  
- [x] cargo audit + deny.  
- [ ] Optional Codex pass.

---

## Phase 2 — Fix P0 → DoD-3

Priority candidates (if confirmed):

- [x] Sandbox escape fixes + tests (absolute join, `..`).  
- [x] SSRF deny private/link-local/loopback on IdP HTTP.  
- [x] PST cycle/depth + allocation caps.  
- [x] Zeroize PMK/passphrase paths incomplete coverage.  
- [x] Any actor spoof / dual-open / produce integrity P0s.

---

## Phase 3 — P1 + residual → DoD-4

- [ ] Fix practical P1s.  
- [ ] D-0063-* for rest.

---

## Phase 4 — Finalize → DoD-5–8

- [ ] Full gates + audit/deny.  
- [ ] deferred.md; review.md; registries; ledger.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cargo deny check
```

---

## Handoff

- **0064** Connect UX only.  
- Operators get residual risk list (LAN cleartext, unsigned RC, etc.).

---

## Suggested order

1. Threat model update  
2. Path sandbox absolute-join tests  
3. OIDC SSRF tests + blocklist  
4. pst-reader alloc/cycle review  
5. Zeroize inventory  
6. Classic Series I checklist  
7. Fix P0/P1 + findings.md  
