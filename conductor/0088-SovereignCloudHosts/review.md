# 0088 — Sovereign Cloud Host Allowlist — Completion review

**Track:** 0088-SovereignCloudHosts  
**Status:** Completed (engineering + governance)  
**Branch:** `feat/0088-sovereign-cloud-hosts`  
**Closes:** D-0085-sovereign-cloud-hosts  
**Opens:** D-0088-usgovcloud-microsoft-tld  
**Ledger TX:** `1eccebb5-9a64-4319-bc3d-baa44a964166`

## Reviewers / rounds

| Round | Reviewer | Verdict | Notes |
|---|---|---|---|
| Internal #1 | general-purpose | PASS WITH DEFERRED P3 | DoD-1..4 met; DoD-5 pending; dual tables + `.microsoft` residual |
| Fix | orchestrator | — | Cross-reference sync comments on dual host suffix tables |
| Codex #1 | gpt-5.6-luna high | **PASS WITH DEFERRED P3** | No P0–P2; only intentional `D-0088-usgovcloud-microsoft-tld`; DoD-5 finalize expected after review |
| Finalize | orchestrator | — | Canonical review, board Completed, ledger commit, gates |
| Codex **final** | gpt-5.6-luna high | **PASS WITH DEFERRED P3** | Prior DoD-5 closed; sole residual `D-0088-usgovcloud-microsoft-tld`; no P0–P2 |

## What shipped

- `dedup-engine` `ALLOWED_CLOUD_HOST_SUFFIXES` + `host_matches_dns_suffix` / `is_allowed_cloud_host`
- SafeLinks unwrap for `*.safelinks.protection.office365.us` (commercial outlook.com kept)
- Sovereign unit tests: `-my.sharepoint.us` / `-my.sharepoint-mil.us` action tokens; `dps.mil`; SafeLinks → sharepoint.us; bare/` :f:` exclusions; lookalike rejection
- `pst-reader` local suffix-safe cloud pointer helpers (no shared crate; no reader→engine dep)
- Docs/runbook/CHANGELOG + fidelity_contract residual IDs

### Hosts

| Class | Suffix / exact |
|---|---|
| Commercial (kept) | `*.sharepoint.com`, `*.sharepoint-df.com`, `*.onedrive.live.com`, `*.1drv.ms` |
| GCC High | `*.sharepoint.us`, `admin.onedrive.us` (exact) |
| DoD | `*.sharepoint-mil.us`, `*.dps.mil` |
| SafeLinks | `*.safelinks.protection.outlook.com`, `*.safelinks.protection.office365.us` |

**Out of P0:** 21Vianet `*.sharepoint.cn`; `.microsoft` TLD content hosts (`D-0088-usgovcloud-microsoft-tld`).

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| **1** Hosts | **Met** | GCC High/DoD including `-my.`; SafeLinks office365.us unwrap |
| **2** Proportionality | **Met** | Bare roots / `:f:` excluded per new suffix |
| **3** Regression | **Met** | Commercial 0085 tests green (`body_cloud` 33 passed) |
| **4** Honesty | **Met** | D-0085 closed; D-0088 opened; 21Vianet / GCC Moderate / SafeLinks historical / admin.onedrive.us documented |
| **5** Recorded | **Met** | this file; conductor **Completed**; ledger TX committed |

## Gates (observed)

```text
cargo test -p dedup-engine -- body_cloud     → 33 passed
cargo test -p pst-reader -- attachment       → 9 passed
cargo test -p pst-dedup-cli -- cloud_attachments → 1 passed
cargo fmt --all --check                      → ok
cargo clippy -p dedup-engine -p pst-reader -p pst-dedup-cli --all-targets -- -D warnings → ok
```

Full workspace gate + `ledgerful verify` run at finalize before publish.

## Residuals / dispositions

| Item | Disposition |
|---|---|
| `D-0088-usgovcloud-microsoft-tld` | **Deferred** (hard P3; Learn ID 23 path shapes unknown) |
| Dual host tables (engine vs reader) | **Accepted by design** (spec §2.5); sync comments added |
| DoD-5 process gap at Codex #1 | **Closed** in this finalize |

## Findings closed this track

- Codex #1 / Internal: no P0–P2
- Easy P3 dual-table drift → sync comments
- Hard P3 `.microsoft` TLD → already in `docs/deferred.md`
