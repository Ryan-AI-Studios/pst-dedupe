# 0088 — Internal completion review (r1)

**Track:** `0088-SovereignCloudHosts`  
**Branch:** `feat/0088-sovereign-cloud-hosts`  
**Reviewer role:** internal completion (read-only product judgment)  
**Date:** 2026-08-24  

## Verdict

**PASS WITH DEFERRED P3**

Product DoD-1..DoD-4 are met in code and docs. DoD-5 (finalize: `review.md`, conductor **Completed**, ledger TX commit) is intentionally not done yet. Open residual **D-0088-usgovcloud-microsoft-tld** is correctly recorded. No P0–P2 blockers found.

---

## DoD matrix

| DoD | Requirement | Status | Evidence |
|---|---|---|---|
| **DoD-1 — Hosts** | GCC High + DoD document-shaped URLs including `-my.sharepoint.us` / `-my.sharepoint-mil.us`; unwrap `*.safelinks.protection.office365.us` → document-shaped `*.sharepoint.us` | **Met** | `ALLOWED_CLOUD_HOST_SUFFIXES` + `is_safelinks_host`; tests `gcc_high_my_sharepoint_us_action_tokens`, `dod_my_sharepoint_mil_us_action_tokens`, `dps_mil_document_shaped_hit`, `gcc_high_safelinks_unwrap_to_sharepoint_us` |
| **DoD-2 — Proportionality** | Bare sovereign roots / `:f:` excluded **per new suffix** | **Met** | `sovereign_bare_roots_and_folder_f_excluded` covers `sharepoint.us`, `-my.`, `sharepoint-mil.us`, `dps.mil`; folder `:f:` gate unchanged |
| **DoD-3 — Regression** | Commercial 0085 unit tests remain green | **Met** | Existing commercial hit/miss/SafeLinks/cap tests still present; `cargo test -p dedup-engine -- body_cloud` → **33 passed** |
| **DoD-4 — Honesty** | Close D-0085; open D-0088-usgovcloud-microsoft-tld; document 21Vianet, GCC Moderate, SafeLinks historical bound, `admin.onedrive.us` | **Met** | `docs/deferred.md`; runbook + `unique-pst-export.md` + CHANGELOG Unreleased; fidelity reason + test asserts D-0085 **closed** and D-0088 residual |
| **DoD-5 — Recorded** | `review.md`; conductor **Completed**; ledger TX committed | **Not done** | Plan Phase 3 unchecked; conductor row **In Progress**; ledger TX `1eccebb5-…` noted uncommitted in implementation-notes |

---

## Checklist audit (non-DoD)

| Item | Result |
|---|---|
| Hosts: GCC High/DoD + `-my.`; SafeLinks office365.us; bare/`:f:` excluded; commercial green | **OK** |
| No shared crate / no `pst-reader` → `dedup-engine` dep | **OK** — local suffix helpers in `attachment.rs`; `pst-reader/Cargo.toml` has no `dedup-engine` |
| Reader substring FP (`notsharepoint.attacker.com`) | **OK** — `text_mentions_cloud_host` / `cloud_pointer_suffix_safe_rejects_lookalike` |
| D-0085 closed; D-0088-usgovcloud-microsoft-tld opened; docs honest | **OK** |
| Placeholders / stubs / missing tests / wrong residual IDs / fidelity still asserting open D-0085 | **None found** — fidelity asserts `closed` + D-0088 residual |
| Production `.unwrap()` / `.expect()` in changed product paths | **None** — only test `.expect(...)` in `attachment.rs` / `fidelity_contract.rs` |

---

## Findings

### [P3] DoD-5 finalize still open

- **Confidence:** High  
- **Location:** `conductor/0088-SovereignCloudHosts/plan.md` Phase 3; `conductor/conductor.md` (0088 **In Progress**); ledger TX `1eccebb5-9a64-4319-bc3d-baa44a964166`  
- **Problem:** Implementation + docs are complete, but track finalize artifacts are not written/committed. Series blurb still says “0088–0092 Ready” while the 0088 row is **In Progress**.  
- **Correction:** Orchestrator Phase 3 — write `review.md`, set conductor **Completed** (and align Series M blurb), commit ledger TX.  
- **Easy vs hard:** **Easy** (process/docs only).

### [P3] Dual host-suffix tables can drift (accepted by design)

- **Confidence:** High  
- **Location:** `crates/dedup-engine/src/body_cloud_links.rs` (`ALLOWED_CLOUD_HOST_SUFFIXES`) vs `crates/pst-reader/src/messaging/attachment.rs` (`CLOUD_POINTER_HOST_SUFFIXES`)  
- **Problem:** Spec §2.5 / A3 declined a shared crate; both lists currently match, but future host adds require two edits.  
- **Correction:** Leave as-is for 0088; when adding hosts later, update both arrays (or reopen a tiny shared-constants track only if drift bites). Residual already covers `.microsoft` TLD separately.  
- **Easy vs hard:** **Easy** to keep in sync manually; **hard** only if forced into a shared crate now (out of scope).

### [P3] Intentional product residual — `.microsoft` TLD hosts

- **Confidence:** High  
- **Location:** `docs/deferred.md` → `D-0088-usgovcloud-microsoft-tld`  
- **Problem:** Learn GCC High ID 23 content hosts (`*.usgovcloud.microsoft`, etc.) are out of P0 by lock.  
- **Correction:** None in 0088 — residual correctly opened; do not guess path shapes.  
- **Easy vs hard:** **Hard** (needs Learn path-shape research + fixtures) — correctly deferred.

---

## Non-findings (explicit)

- SafeLinks `*.safelinks.protection.office365.us` shipped (not deferred).  
- `admin.onedrive.us` is exact-host include; bare admin URL does not produce body-cloud hits.  
- 21Vianet / GCC Moderate honesty present in runbook, export doc, deferred notes, CHANGELOG.  
- `looks_like_cloud_pointer` still treats any absolute URL as URL-shaped (attach Signal 3). Spec FP target was substring `contains("sharepoint")` on lookalike hosts; bare `notsharepoint.attacker.com` is rejected. Not treated as a DoD miss.

---

## Verification evidence

Commands run by this review (2026-08-24), observed results:

| Command | Result |
|---|---|
| `cargo test -p dedup-engine -- body_cloud` | **33 passed**, 0 failed |
| `cargo test -p pst-reader -- attachment` | **9 passed**, 0 failed (filter; includes classify + related) |
| `cargo test -p pst-dedup-cli -- cloud_attachments` | **1 passed** (`fidelity_contract::tests::cloud_attachments_not_silently_preserved`) |
| `cargo fmt --all --check` | **FMT_OK** (exit 0) |
| `cargo clippy -p dedup-engine -p pst-reader -p pst-dedup-cli --all-targets -- -D warnings` | **CLIPPY_EXIT=0** (`Finished …`) |

`ledgerful verify` was not required by the review command list and was not run here.

---

## Summary for orchestrator

Ship product work as **ready to finalize**. Close DoD-5 only: write track `review.md`, flip conductor to **Completed**, commit the open FEATURE ledger TX. No product code changes required from this internal review.
