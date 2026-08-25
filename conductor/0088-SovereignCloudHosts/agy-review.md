# Antigravity Review — Track 0088: Sovereign Cloud Host Allowlist

- **Track ID:** `0088-SovereignCloudHosts`
- **Reviewer:** Antigravity (Advanced Agentic Pair Programmer)
- **Date:** 2026-08-24
- **Review Scope:** Review only (no implementation) — plan audit, blind spot discovery, industry/protocol research, and improvement recommendations.
- **Spec / Plan Reference:** [`spec.md`](file:///C:/dev/Dedupe/conductor/0088-SovereignCloudHosts/spec.md), [`plan.md`](file:///C:/dev/Dedupe/conductor/0088-SovereignCloudHosts/plan.md)

---

## 1. Executive Summary

Track 0088 completes the missing sovereign-cloud allowlist entries (GCC High, US DoD, and related environments) for offline body-cloud link detection and attachment-table cloud classification. This closes `D-0085-sovereign-cloud-hosts`.

The track is tightly scoped and sound in its core premise: **detect + ledger only, document-shaped gate preserved, no network hydration, no synthetic attachment creation**. 

However, this review identifies several crucial **blind spots** in the endpoint matrix (especially SafeLinks sovereign domains), architectural alignment between reader and engine, and test fixture definitions.

---

## 2. Blind Spots & Technical Findings

### Finding 0088-1: SafeLinks Sovereign Domain is Fully Documented (`safelinks.protection.office365.us`)
- **Blind Spot in Spec:** §2.4 and §3 mark SafeLinks sovereign coverage as tentative, suggesting opening `D-0088-safelinks-sovereign` if public tables are thin.
- **Evidence / Online Research:** Microsoft Defender for Office 365 officially publishes and operates `safelinks.protection.office365.us` (and regional subdomains such as `*.safelinks.protection.office365.us`) as the dedicated SafeLinks rewrite endpoint for **both GCC High and US DoD** tenants (as opposed to commercial `safelinks.protection.outlook.com`).
- **Recommendation:** Do not defer SafeLinks sovereign to another residual. Incorporate `is_safelinks_host` recognition for `safelinks.protection.office365.us` and `*.safelinks.protection.office365.us` directly into Track 0088 Phase 1.

### Finding 0088-2: OneDrive for Business Sovereign Subdomain Structure
- **Blind Spot in Spec:** §2.4 lists `admin.onedrive.us` as a primary OneDrive endpoint. 
- **Evidence / Online Research:** `admin.onedrive.us` is only an administrative portal. User OneDrive for Business URLs in GCC High use `<tenant>-my.sharepoint.us/personal/<user>/...` and in DoD use `<tenant>-my.sharepoint-mil.us/personal/<user>/...`. 
- **Recommendation:** Ensure test vectors explicitly cover `<tenant>-my.sharepoint.us` and `<tenant>-my.sharepoint-mil.us` path shares with document action tokens (e.g. `:w:`, `:x:`, `:p:`, `:b:`, `:u:`).

### Finding 0088-3: Reader vs Engine Cloud Host Classifier Drift
- **Blind Spot in Live Code:**
  - In `dedup-engine/src/body_cloud_links.rs`: `is_commercial_cloud_host` checks exact domain endings (`.sharepoint.com`, `.sharepoint-df.com`, `.onedrive.live.com`, `.1drv.ms`).
  - In `pst-reader/src/messaging/attachment.rs`: `extract_cloud_url` and `classify_attach_pc` perform loose substring checks: `t.contains("sharepoint") || t.contains("1drv.")`.
- **Impact:** `pst-reader`'s substring check accidentally matches `.sharepoint.us` already, but `body_cloud_links.rs` fails to match it. Furthermore, loose substring checks in reader risk matching false domains like `notsharepoint.attacker.com`.
- **Recommendation:** Consolidate cloud host classification into a single shared helper (e.g., `dedup_engine::is_cloud_host` or a public classifier in `pst-reader`) that enforces strict domain suffix checking for both commercial and sovereign hosts across both crates.

### Finding 0088-4: Sovereign Cloud Host Scope Boundary (21Vianet & Retired Endpoints)
- **Scope Consideration:** 
  - 21Vianet (China sovereign) operates under `*.sharepoint.cn`.
  - Historical German sovereign cloud (`*.sharepoint.de`) was retired in favor of EU Data Boundary commercial endpoints (`*.sharepoint.com`).
- **Recommendation:** Explicitly document the sovereign matrix in `docs/unique-pst-ediscovery-runbook.md`:
  - **In-Scope / Active:** US GCC High (`*.sharepoint.us`, `*.office365.us`), US DoD (`*.sharepoint-mil.us`, `*.dps.mil`).
  - **Documented Exclusions:** 21Vianet (`*.sharepoint.cn`) if not covered, with clear reasoning.

---

## 3. Recommended Spec & Plan Amendments

1. **Update §2.4 Endpoint Table:**
   - GCC High: `*.sharepoint.us`, `*.office365.us` (SafeLinks: `safelinks.protection.office365.us`).
   - DoD: `*.sharepoint-mil.us`, `*.dps.mil`, `safelinks.protection.office365.us`.
   - OneDrive: `<tenant>-my.sharepoint.us`, `<tenant>-my.sharepoint-mil.us`.
2. **Update §7 Definition of Done (DoD-1):**
   - Require verification that unwrapped `safelinks.protection.office365.us` links pointing to document-shaped `*.sharepoint.us` URLs are correctly parsed and preserved in `export_body_cloud_links.csv`.
3. **Refactor Shared Classifier in Phase 1:**
   - Unify `is_cloud_host` to prevent `dedup-engine` and `pst-reader` drift.

---

## 4. Verdict & Risk Rating

- **Track Rating:** **PASS (Ready with minor spec enhancements)**
- **Complexity / Risk:** Low (pure string / regex parsing; offline-only invariant unchanged).
- **Execution Estimate:** 0.5 – 1 day.
