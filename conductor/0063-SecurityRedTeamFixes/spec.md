# 0063 — Security red team + P0/P1 fixes (Series I + RC surface)

- **Track ID:** 0063-SecurityRedTeamFixes  
- **Execution repo:** `C:\dev\dedupe`  
- **Governance:** this directory in `C:\dev\dedupe\conductor\`  
- **Plan-of-record:** Series J security consolidation after RC freeze  
- **Status:** **Completed** — Codex gpt-5.6-luna high final PASS WITH DEFERRED P3.
- **Depends on:** Hard **0057–0061** on `main`. Soft **0062 Completed** (`0.2.0-rc.1`, schema **39**, audit/deny in place).  
- **Downstream:** Operator/pilot confidence; residual **D-0063-***; **0064** Desk Connect still UX-only.  
- **Priority:** **P0 consolidation** (security).  
- **Evidence:** Synthetic adversarial tests only; **no client data**; no credentials/secrets in git.  
- **Deferred ledger:** re-validate Series I security residuals (§2.5); append **D-0063-***.

---

## 1. Objective

Run a **time-boxed adversarial security review** of shipped security-sensitive surfaces, then **fix all P0 and agreed P1** findings in-repo with regression tests — not a perpetual audit or FedRAMP program.

| Surface | Track | Example threats to re-verify |
|---|---|---|
| **Encryption at rest** | 0057 | Temp/CAS spill; FTS plaintext; seal/Drop; passphrase env; **key heap residue (zeroize)** |
| **Multi-user service** | 0058 | Actor spoof; dual-open; `--allow-lan` without TLS; lock races |
| **Platform SSO** | 0059 | Cross-tenant; PMK; **`Path::join` sandbox escape**; **OIDC discovery SSRF** |
| **Produce integrity** | 0060 / 0040 | Ghost text; withhold TOCTOU; Bates uniqueness |
| **Cloud CAS / jobs** | 0061 | Truncated put; path/key isolation; never remote SQL |
| **PST extract** | pst-reader | **B-tree cycles; allocation bombs; host OOM** |
| **RC supply chain** | 0062 | Re-run audit/deny; unsigned handoff honesty |
| **Series K export** (light) | 0067–0071 | Source overwrite; report-dir path escape |

**Outcome:**

1. Threat model one-pager (modes: offline / service / platform / cloud).  
2. Findings table (severity, evidence, disposition).  
3. All **P0** fixed + tests; agreed **P1** fixed or explicitly deferred.  
4. Residual → `docs/deferred.md` as **D-0063-***.

**Industry anchors (researched 2026-07):**

- **OWASP ASVS 5.0** (2025) as verifiable control checklist (not full L3 certification).  
- **OWASP Top 10 2025**: cryptographic failures, broken access control, misconfiguration.  
- **OWASP Multi-Tenant Security Cheat Sheet**: tenant isolation, path sandbox, key separation.  
- **Rust-specific**: `Path::join` absolute RHS override; non-zeroizing `String`/`Vec` secret residue.  
- Prefer **automated regression tests** over one-off manual “looks fine.”

---

## 2. Context

### 2.1 Baseline (ground truth)

| Item | State |
|---|---|
| Product version | **0.2.0-rc.1** (0062) |
| Schema | **SCHEMA_VERSION = 39** |
| Service | Bearer sessions; strict actor mode; body `actor` ignored (claims — re-verify) |
| Bind | Default `127.0.0.1`; non-loopback requires `--allow-lan` (**no TLS P0** — residual D-0058-02) |
| Encryption | Pure-Rust AEAD container; FTS + CAS + DB; semantic residual plaintext (D-0057-07) |
| Platform | OIDC PKCE; PMK for IdP secrets; path sandbox env |
| Supply chain | deny.toml + cargo-audit in CI (0062) |
| Desk Connect | **Not shipped** (D-0058-01 → 0064) — red team **API/service**, not missing GUI |

### 2.2 Methodology (LOCKED)

```text
Phase A — Read-only adversarial review (no feature work)
  → threat model + checklist + findings draft
Phase B — Fix P0 (+ agreed P1)
  → code + regression tests
Phase C — Re-verify (gates + targeted adversarial tests + optional Codex)
  → residual D-0063-*; review.md
```

1. **Time-box** review (e.g. 1–2 focused passes); severity triage prevents infinite findings.  
2. **P0** = exploitable data loss/leak, authz bypass, tenant hop, silent integrity fail, credential exposure.  
3. **P1** = high-likelihood misconfig abuse or TOCTOU with real impact.  
4. **P2/P3** → defer unless trivial one-liner.  
5. Each in-track fix: **test that would fail before the fix**.  
6. Optional **Codex/read-only** cross-model pass on findings + fixes (recommended for security track).

### 2.3 Deferred roll-in (reasonable)

| Deferred | 0063 action |
|---|---|
| **D-0057-09** passphrase in process env | **Review** as P1 candidate; fix if exploitable on multi-user host, else reaffirm residual |
| **D-0057-08 / D-0057-12** staging / seal Drop | **Review** temp boundaries + seal paths; fix P0 leak or incomplete seal |
| **D-0057-07** semantic plaintext | Document as known residual unless easy encrypt is in freeze scope (likely **defer**) |
| **D-0058-02** LAN without TLS | **Review** as intentional residual; ensure docs + default bind fail-closed; fix only if bind policy can be bypassed |
| **D-0058-03/04** lock TTL / dual-process | Adversarial tests; fix if exclusive lock can be skipped |
| **D-0059-*** path sandbox / PMK | **Must re-verify** isolation + **absolute Path::join**; fix escapes as P0; zeroize PMK paths |
| **D-0061-*** remote SQL / cache | Confirm physics locks still held; fix any remote SQL or cross-matter path |
| **D-0062-codesign** | Out (signing process); note unsigned operator risk |
| Desk Connect UX | **Out → 0064** |
| SAML / SCIM / FIPS CMK | Residual product — not this track |

### 2.4 Product rules (LOCKED)

1. **No new product features** (no Connect UI, no new auth protocols).  
2. **No client data** in fixtures or logs.  
3. **No secrets** in repo, CI logs, or review artifacts.  
4. Prefer **fail-closed** over silent degrade for authz/crypto.  
5. Do not claim FedRAMP/SOC2 completion.

---

## 3. In scope

### 3.1 Threat model deliverable (LOCKED)

Short doc `docs/security-threat-model-rc.md` (or under conductor review pack):

| Mode | Assets | Trust boundary | Primary threats |
|---|---|---|---|
| Offline Desk | Matter DB/CAS/FTS on disk | Single user OS account | Disk theft; temp spill; env passphrase; **key material in pagefile** |
| Service loopback | HTTP API + exclusive lock | Local processes | Actor spoof; dual open |
| Service LAN | Same + network | LAN attacker | Cleartext HTTP; token capture |
| Platform SSO | platform.db + OIDC | IdP + multi-tenant | Tenant hop; **sandbox Path::join escape**; **OIDC SSRF** |
| Cloud CAS | Object store | Cloud credentials | Truncation; key confusion |
| **Extract / ingest** | Untrusted PST bytes | Parser in service/worker | **B-tree cycles; allocation bombs; OOM host crash** |

### 3.2 Checklist surfaces (LOCKED minimum)

#### 0057 Encryption

- [ ] Encrypted matter: DB/CAS/FTS sealed when locked; no durable plaintext CAS outside workspace temp policy.  
- [ ] Temp/CAS stage under matter workspace; purged on success/fail.  
- [ ] Passphrase handling: env residual risk documented or mitigated.  
- [ ] Wrong passphrase fail-closed.  
- [ ] **Key zeroization** (§3.2.1).  

#### 0058 Multi-user service

- [ ] Session required for mutates; body `actor` ignored under strict mode (**test spoof**).  
- [ ] Default bind loopback; non-loopback requires allow_lan.  
- [ ] Exclusive matter lock vs second process.  
- [ ] OCC / item locks on contested mutates.  

#### 0059 Platform SSO

- [ ] Matter register paths confined to `PLATFORM_STORAGE_ROOT`.  
- [ ] Tenant A cannot open/register Tenant B paths.  
- [ ] **Path::join absolute-override / traversal** (§3.2.2) — adversarial tests.  
- [ ] IdP secrets not logged; PMK required when ciphertext present; **PMK/passphrase zeroized** (§3.2.1).  
- [ ] OIDC state/PKCE parameters validated.  
- [ ] **SSRF on tenant-configured OIDC discovery / JWKS URLs** (§3.2.4).  

#### 0060 / produce

- [ ] Withheld items not produced when gated.  
- [ ] Redacted path does not emit original body when redactions exist.  
- [ ] Bates uniqueness under concurrent produce residual honesty.  

#### 0061 Cloud

- [ ] No remote SQLite / NFS matter.db path.  
- [ ] Blob put integrity (size/hash) fail-closed.  
- [ ] Matter-scoped keys/prefixes.  

#### pst-reader / extract (LOCKED — adversarial parse)

- [ ] **Cycle / depth limits** on NBT/BBT/subnode walks — no infinite loop on crafted cycles (§3.2.3).  
- [ ] **Allocation bounds** — no `Vec::with_capacity(attacker_u64)` trust of unauthenticated sizes; stream or cap (§3.2.3).  
- [ ] Service/worker remains available after a single malicious PST open attempt (no whole-process OOM by design; or bounded fail).  

#### Series K export (light)

- [ ] unique-pst/eml refuse source overwrite.  
- [ ] Report paths cannot clobber inputs.  

#### 3.2.1 Cryptographic material zeroization (LOCKED)

**Problem:** Rust `String` / `Vec<u8>` do **not** wipe memory on drop. PMK, matter passphrases, and session secrets can linger in heap, **pagefile.sys**, hibernation files, or crash dumps on offline eDiscovery workstations.

**Rules:**

1. Red-team **must inventory** where PMK, passphrases, DEKs, and IdP secrets live in memory.  
2. **P0/P1 bar:** long-lived or high-value secrets (platform master key material, unlocked DEK if held, passphrase buffers after use) **must** use **zeroizing** wrappers (`zeroize` / `secrecy` or equivalent already in workspace) — not bare `String` retained after auth.  
3. If passphrase is read from env (`PST_DEDUPE_MATTER_PASSPHRASE` etc.): prefer **clear env after read** where process policy allows; document residual if OS/env constraints prevent full wipe.  
4. Existing `zeroize_string` / PMK helpers: **verify coverage is complete**, not just present for one path.  
5. Regression: unit tests that secret types implement `Zeroize` / drop wipe for critical paths (or audit doc if untestable).  
6. Incomplete coverage → fix as P1 minimum when secrets are held longer than the unlock call stack.

#### 3.2.2 Path sandbox vs `Path::join` (LOCKED)

**Problem:** `Path::join` **replaces** the base when the right-hand side is absolute (`C:\…`, `\\?\…`, `/…`). `PLATFORM_STORAGE_ROOT.join(attacker_absolute)` can escape the jail. Strings with `..`, `/`, `\`, `:` in tenant IDs or relative segments are classic traversal.

**Rules:**

1. Red-team **must** target every sandbox join of untrusted components (tenant slug, matter path, register root).  
2. Tests **must** include: absolute Windows path as component; `..\..\` relative; mixed separators. Expect **reject**, never escape.  
3. Fix policy: **reject** untrusted segments containing path separators, `..`, or drive/root markers **before** join; and/or only join after normalizing and verifying `assert_path_under_root` on the **final** path (existing helper — verify it cannot be fooled by absolute RHS).  
4. Optional hardening: `cap-std` / path-security libraries — residual if current validators are proven solid with tests.  
5. Sandbox escape = **P0**.

#### 3.2.3 Adversarial PST parsing (LOCKED)

**Problem:** `pst-reader` / extract process **unauthenticated** binary. B-tree **cycles** → infinite CPU; huge claimed sizes → **`with_capacity` OOM** → multi-tenant host crash.

**Rules:**

1. Threat model **must** list adversarial PST as a primary threat for offline + service extract.  
2. Review NBT/BBT/subnode walks for **cycle detection or hard depth limits**.  
3. Review allocations driven by PST-declared sizes (e.g. `lcb`, attach size): **must not** allocate unbounded trust of attacker fields — stream, chunk, or hard-cap with typed error.  
4. Prefer fail with `PstError` over process abort where possible.  
5. Add synthetic/malicious-structure tests where feasible (cycle fixture or depth bomb; oversized claim with tiny payload).  
6. P0: infinite loop or trivial host OOM from one PST open on the service extract path.  

#### 3.2.4 OIDC / outbound HTTP SSRF (LOCKED)

**Problem:** Tenant-configurable IdP **discovery** / token / JWKS URLs can point at **link-local metadata** (`169.254.169.254`), **loopback**, or **RFC1918** internal services. Backend fetch = **SSRF**, possibly leaking cloud IAM credentials.

**Rules:**

1. Inventory all **outbound HTTP** used for OIDC (discovery, JWKS, token, userinfo).  
2. Red-team tests: discovery URL → `http://127.0.0.1/…`, `http://169.254.169.254/…`, private ranges. Expect **block** (or document intentional allow-list only for lab).  
3. Fix: resolve hostname → IPs; **deny** loopback, link-local, multicast, and private ranges for tenant-supplied URLs unless explicit admin allow-list (default **deny**). Prefer HTTPS-only for IdP endpoints.  
4. Do not follow redirects to blocked ranges.  
5. SSRF that reaches metadata/loopback = **P0**.  

### 3.3 Supply chain (LOCKED)

- Re-run **`cargo audit`** + **`cargo deny check`** on freeze baseline.  
- New critical advisories: fix or D-0063-audit-* with justification.  
- Do not weaken 0062 license deny list.

### 3.4 Findings format (LOCKED)

```text
[ID] Title
Severity: P0|P1|P2|P3
Surface: 0057|0058|…
Evidence: file/test/repro
Impact:
Disposition: Fixed | Deferred D-0063-xx | False positive | Accepted risk
Regression test: path or N/A
```

Store in track `findings.md` + summary in `review.md`.

### 3.5 Fix policy (LOCKED)

| Severity | Action |
|---|---|
| **P0** | **Must fix** in 0063 + regression test |
| **P1** | Fix if practical in time-box; else deferred with owner |
| **P2/P3** | Defer unless trivial |

---

## 4. Out of scope

| Item | Owner |
|---|---|
| Desk Connect / SSO browser UX | **0064** |
| Full pen-test of customer networks | external |
| FedRAMP / SOC2 paperwork | never this track |
| SAML, SCIM, FIPS CMK product | residual |
| New SaaS features | freeze |
| Implementing every historical residual | no — triage only |
| Operator Authenticode cert procurement | D-0062-codesign |

---

## 5. Preconditions

- **P1:** 0057–0061 on `main`.  
- **P2:** Prefer **0062 Completed** so review targets RC freeze code.  
- **P3:** Ability to run service/platform tests in CI/local.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Infinite findings | Time-box; P0/P1 only |
| Fix scope creep | No UX/features |
| False confidence | Exploit-style tests |
| Missing LAN TLS | Document accepted residual if unfixed |
| Passphrase in env | Fix or reaffirm with residual |

---

## 7. Definition of Done

Complete only when **all** hold:

- [ ] **DoD-1 — Threat model** short doc for offline/service/platform/cloud.  
- [ ] **DoD-2 — Findings table** complete for checklist surfaces.  
- [ ] **DoD-3 — All P0 fixed** with regression tests.  
- [ ] **DoD-4 — P1** fixed or explicitly deferred as D-0063-*.  
- [ ] **DoD-5 — Supply chain:** audit + deny re-run; exceptions documented.  
- [ ] **DoD-6 — Gates:** fmt, clippy `-D warnings`, workspace tests green.  
- [ ] **DoD-7 — Recorded:** findings.md + review.md; deferred.md; registries; ledger.  
- [ ] **DoD-8 — Handoff:** known residuals listed for operators / **0064**.

---

## 8. Verification

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cargo deny check
# plus new adversarial tests under matter-service / matter-platform / matter-core / produce / storage
```

Optional:

```powershell
# Codex/read-only pass on findings + fix diff
```

---

## 9. Test / evidence matrix (minimum)

| # | Case | Expect |
|---|---|---|
| 1 | Actor spoof body field under service | Session user used; spoof ignored |
| 2 | Bind non-loopback without allow_lan | Fail closed |
| 3 | Dual process exclusive lock | Second open fails |
| 4 | Platform path outside storage root | Reject |
| 5 | **Absolute path as join component** (`C:\…`, `\\?\…`) | Reject / no escape |
| 6 | **`..` / mixed separators** under storage root | Reject or normalize under root only |
| 7 | Cross-tenant matter access attempt | Fail closed |
| 8 | Wrong passphrase encrypted matter | Fail closed |
| 9 | **OIDC discovery → 127.0.0.1 / 169.254.169.254** | Blocked (SSRF) |
| 10 | **PST oversized claim / cycle** (as constructible) | Error, not hang/OOM host |
| 11 | unique-pst out = input | Refuse |
| 12 | cargo audit/deny | Green or documented |
| 13 | Each P0 fix | Dedicated regression test red→green |

*(Exact tests depend on findings; matrix is the floor for re-verification of claimed locks.)*

### 3.6 Review folds accepted (LOCKED summary)

| # | Fold | Spec |
|---|---|---|
| 1 | **`Path::join` absolute-override / traversal** sandbox | §3.2.2 |
| 2 | **Zeroize** PMK/passphrase/key material | §3.2.1 |
| 3 | **Adversarial PST** cycles + allocation bombs | §3.2.3 |
| 4 | **OIDC discovery SSRF** (block private/link-local/loopback) | §3.2.4 |

---

## 10. Handoff

| Track / audience | Needs |
|---|---|
| **0064** | API secure; Connect UX only |
| Operators | Threat model + residual risks (LAN cleartext, env passphrase, unsigned RC) |
| **0062** | Do not regress deny/audit policy |

```text
DECISION: 0063 Ready — red team 0057–0061 + Path::join jail, key zeroize, PST parse DoS,
OIDC SSRF; fix P0/P1 with tests; residual D-0063-*; no Connect UX.
```
