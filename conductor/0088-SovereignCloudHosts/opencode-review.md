# opencode-review — 0088 SovereignCloudHosts (spec/plan review, review only)

- **Series context / verdict summary:** see `../../opencode-review.md` — verdict: **Ship as-is (minor notes)**.
- **Method:** code snapshot claims verified against `main` @ `c5437d0`; Microsoft Learn fetched 2026-08-24; `ledgerful scan --impact` LOW risk (governance-only diff); no code edits made.

**Verified against live sources (fetched 2026-08-24):**
- GCC High endpoints (Learn, updated 2026-07-01): SharePoint/OneDrive = `*.sharepoint.us` (ID 9) and `admin.onedrive.us` (ID 10). DoD endpoints (Learn, updated 2026-06-30): `*.dps.mil`, `*.sharepoint-mil.us` (ID 9). The spec's §2.4 starting table is **correct as written**.
- SafeLinks: no public Microsoft matrix of sovereign wrapper hosts exists. Commercial wrapper is `<region>.safelinks.protection.outlook.com` (Learn, Safe Links about, updated 2026-05-22); GCC High EOP surfaces under `*.protection.office365.us`, but a sovereign SafeLinks wrapper host is undocumented. The spec's plan to open `D-0088-safelinks-sovereign` rather than guess is right.

**Strengths:** thin scope, document-shaped gate preserved, fail-honest deferred handling, correct dependencies (0084/0085/0087 Completed).

**Findings / blind spots:**

1. **`.microsoft` TLD consolidation is missing.** Microsoft is consolidating M365 SaaS domains into the `.microsoft` TLD; the GCC High endpoint page now publishes `*.usgovcloud.microsoft`, `*.usgovcloud-static.microsoft`, and `*.usgovcloud-usercontent.microsoft` (ID 23). Document-shaped body links will eventually appear under the **usercontent** host (the commercial analog is the new cloud content domain). The spec should add this as a *recorded* future residual (e.g., fold into the SafeLinks residual note or a new `D-0088-usgovcloud-microsoft-tld`) rather than be discovered later as silent miss — the exact failure mode this track exists to fix.
2. **`admin.onedrive.us` adds near-zero detection value.** It is a Default-category sync/admin endpoint; body document links will be `tenant.sharepoint.us/:w:/…` and `tenant-my.sharepoint.us/:w:/…`, already covered by `*.sharepoint.us`. Harmless to include, but document the expectation so operators don't expect rows from it. Same nuance for `*.dps.mil` (DoD OneDrive) — keep it, but the doc note should say which hosts are *expected* to carry document-shaped links.
3. **The "dual host table" worry in §2.2 is a non-issue in code.** 0084's attach-table cloud detect is named-prop/method-driven (`classify_attach_pc` in `crates/pst-reader/src/messaging/attachment.rs:447`), with only a URL-shape heuristic (`looks_like_absolute_url`) — no hostname suffix table anywhere. The only host table is `is_commercial_cloud_host` / `is_safelinks_host` in `crates/dedup-engine/src/body_cloud_links.rs:409-423`. Phase 0's "plan single helper if practical" item can be downgraded to "confirm no hostname checks exist in attach detect" — cheaper and it is already true.
4. **GCC (Moderate) needs nothing** — it uses commercial (worldwide) endpoints. One doc line prevents someone over-reaching later.
5. **Severity context for the SafeLinks residual:** current Microsoft behavior is that Safe Links **no longer wraps URLs pointing to SharePoint or OneDrive sites** (Learn, Safe Links about). The SafeLinks unwrap path matters mainly for *historical* mail. Worth stating in the residual entry — it bounds how much detection value the sovereign-SafeLinks gap can actually cost.

**Opportunities:** restructure the host table as a `const` suffix array + single matcher so future host additions (including the `.microsoft` TLD wave) are one-line; add a negative test asserting a sovereign *folder* share (`:f:`) and bare roots stay excluded (DoD-2 already implies this — make the test enumerate each new suffix).

**Sources (accessed 2026-08-24):** Microsoft Learn — M365 US Government GCC High endpoints (updated 2026-07-01); M365 US Government DoD endpoints (updated 2026-06-30); Safe Links in Microsoft Defender for Office 365 (updated 2026-05-22).
