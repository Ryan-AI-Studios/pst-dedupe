# Security threat model (RC / Series I surfaces)

Short adversarial model for **0.2.0-rc** offline Desk, multi-user service, platform SSO, cloud CAS, and untrusted extract. Not a FedRAMP/SOC2 claim.

## Modes

| Mode | Assets | Trust boundary | Primary threats | Mitigations (shipped / this track) |
|---|---|---|---|---|
| **Offline Desk** | Matter DB, CAS, FTS on disk; optional AEAD container | Single OS account | Disk theft; temp spill; env passphrase residue; key material in pagefile/dumps | Encryption-at-rest (0057); DEK/PMK zeroize-on-drop; temp under matter workspace; residual **D-0057-09** env passphrase |
| **Service loopback** | HTTP API + exclusive matter lock | Local processes on same host | Actor spoof; dual open; session theft on loopback | Strict actor mode (body `actor` ignored); exclusive OS lock; default bind `127.0.0.1`; bearer sessions |
| **Service LAN** | Same + network exposure | LAN attacker | Cleartext HTTP; token capture; bind misconfig | Non-loopback requires `--allow-lan`; **no TLS P0** residual **D-0058-02** |
| **Platform SSO** | `platform.db`, IdP secrets (PMK-AEAD), matter registry | IdP + multi-tenant registry | Tenant hop; `Path::join` sandbox escape; **OIDC discovery SSRF**; PMK leak | `assert_path_under_root` + component reject; HTTPS-only IdP URL SSRF guard (no private/link-local/CGNAT); PMK `ZeroizeOnDrop` |
| **Cloud CAS** | Object store blobs | Cloud credentials | Truncated put; key confusion; remote SQL | Size/hash fail-closed; matter-scoped keys; no remote SQLite path |
| **Extract / ingest** | Untrusted PST/ZIP bytes in worker | Parser process | B-tree cycles → CPU hang; `lcbTotal` allocation bombs → OOM host crash | Visited-set + depth caps on NBT/BBT/subnodes; 64 MiB XBLOCK assemble cap; typed `ResourceLimit` / `BtreeCycle` |

## Invariants

1. **Fail closed** on authz, crypto, path sandbox, and IdP URL policy — no silent degrade.
2. **No secrets in git** or CI logs.
3. Features remain **offline-capable** with local models where applicable.
4. Desk Connect UX is **out of scope** (0064); red team covers API/service, not missing GUI.

## Residual honesty (non-exhaustive)

| Id | Note |
|---|---|
| D-0057-09 | Passphrase may remain in process env after read (unsafe to clear with concurrent workers). |
| D-0058-02 | LAN bind without TLS is intentional residual; operators must not expose cleartext beyond trusted LAN. |
| D-0058-04 | Dual exclusive lock same-process soft-pass; cross-process covered by OS lock where available. |
| D-0057-07 | Semantic index plaintext residual unless encrypted in a later track. |
| D-0062-codesign | Unsigned release handoff risk (process, not code). |
| D-0063-01 | Matter passphrase may remain in process **env** after unlock (same class as D-0057-09). Heap copies on production unlock/create/change-passphrase paths use `Zeroizing`. |
| D-0063-02 | OIDC SSRF DNS rebinding / resolve-then-connect race (mitigated by re-validating discovered token/jwks/auth URLs; multi-resolve residual). |
| D-0063-03 | XBLOCK assemble hard-cap 64 MiB may reject huge legitimate single assemblies (streaming redesign needed to raise safely). |
| D-0063-04 | `openidconnect::ClientSecret` holds IdP client secret as bare `String` until `CoreClient` Drop; no zeroize API in dependency. Mitigated by exchange-only client lifetime + local secret zeroize after exchange; residual heap residue during exchange only. |
| D-0063-05 | Desk UI passphrase widgets are plain `String` (egui); cleared after submit; heap residue residual. |

See `conductor/0063-SecurityRedTeamFixes/findings.md` for disposition of 0063 findings.
