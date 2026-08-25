# 0088 — Implementation notes

**Ledger TX:** `1eccebb5-9a64-4319-bc3d-baa44a964166` (not committed — orchestrator)

## Files changed

- `crates/dedup-engine/src/body_cloud_links.rs` — `ALLOWED_CLOUD_HOST_SUFFIXES` + `host_matches_dns_suffix` / `is_allowed_cloud_host`; SafeLinks `*.safelinks.protection.office365.us`; sovereign unit tests
- `crates/pst-reader/src/messaging/attachment.rs` — suffix-safe local cloud-pointer helpers (no `dedup-engine` dep); lookalike rejection test
- `crates/pst-dedup-cli/src/fidelity_contract.rs` — `cloud_modern_attachments` reason + tests (D-0085 closed; D-0088 residual)
- `docs/unique-pst-ediscovery-runbook.md`, `docs/unique-pst-export.md` — GCC High/DoD in-scope; admin.onedrive.us; 21Vianet; GCC Moderate; SafeLinks historical bound
- `docs/deferred.md` — D-0085 **Closed in 0088**; D-0088-usgovcloud-microsoft-tld open
- `CHANGELOG.md` — Unreleased 0088 section
- `conductor/0088-SovereignCloudHosts/plan.md` — Phases 0–2 checked
- `conductor/0088-SovereignCloudHosts/spec.md` — status **In Progress**
- `conductor/conductor.md` — 0088 row **In Progress**

## Host list shipped

**Cloud document hosts (suffix / exact):**

| Host | Notes |
|---|---|
| `*.sharepoint.com`, `sharepoint.com` | commercial (0085) |
| `*.sharepoint-df.com` | commercial |
| `*.onedrive.live.com` | commercial |
| `*.1drv.ms` | commercial; document-shaped by shortener |
| `*.sharepoint.us` | GCC High (covers `-my.`) |
| `admin.onedrive.us` | exact; harmless include — no body-cloud rows without document shape |
| `*.sharepoint-mil.us` | DoD (covers `-my.`) |
| `*.dps.mil` | DoD OneDrive |

**SafeLinks wrappers:** `*.safelinks.protection.outlook.com` (kept) + `*.safelinks.protection.office365.us` (0088); unwrap then re-test nested target.

**Out of P0:** 21Vianet `*.sharepoint.cn`; `.microsoft` TLD content hosts.

## Reader substring tightened?

**Yes.** `extract_cloud_url` / url_fallback now use local suffix-equivalent helpers (`looks_like_cloud_pointer` / `text_mentions_cloud_host`). `notsharepoint.attacker.com` does **not** match. No shared crate; no `pst-reader` → `dedup-engine` dependency.

## Verification

```powershell
cargo test -p dedup-engine -- body_cloud   # 33 passed
cargo test -p pst-reader -- attachment     # 9 passed (filter)
cargo fmt --all --check                    # ok
cargo clippy -p dedup-engine -p pst-reader --all-targets -- -D warnings  # ok
```

Also: `cargo test -p pst-dedup-cli -- cloud_attachments_not_silently_preserved` — ok.

## Residuals

- **Closed:** `D-0085-sovereign-cloud-hosts`
- **Open:** `D-0088-usgovcloud-microsoft-tld` (future `.microsoft` TLD content hosts)

## Blocked / left for orchestrator

- Phase 3: `review.md`, conductor **Completed**, ledger TX commit
- Tracks 0089–0092 not touched
