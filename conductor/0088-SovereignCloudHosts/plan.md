# 0088 — Sovereign Cloud Host Allowlist — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-24):** ship `*.safelinks.protection.office365.us`; tests for `-my.` hosts; suffix array; record `D-0088-usgovcloud-microsoft-tld`; cheap reader substring tighten only — see `spec.md` §2.6.

> **Ledger:** `ledgerful ledger start crates/dedup-engine --category FEATURE --message "0088 sovereign cloud hosts"`

---

## Phase 0 — Design lock → DoD-4 (partial)

- [x] Re-cite Microsoft Learn GCC High / DoD endpoint pages (access date).
- [x] Lock suffix list exactly as `spec.md` §2.4 (including SafeLinks office365.us).
- [x] Confirm GCC Moderate = commercial (no extra hosts).
- [x] Confirm `extract_cloud_url` substring sites; decide cheap local suffix tighten vs residual.
- [x] Do **not** plan a shared crate helper.

## Phase 1 — Implement classifiers → DoD-1, DoD-2, DoD-3

- [x] Prefer `const` suffix array + single matcher in `body_cloud_links.rs`.
- [x] Extend SafeLinks unwrap for `*.safelinks.protection.office365.us`.
- [x] Tests: each sovereign host class; `-my.sharepoint.us` / `-my.sharepoint-mil.us` with `:w:` and `:x:`; SafeLinks unwrap → sharepoint.us document URL; `:f:` + bare root **per new suffix**; commercial green.
- [x] Optional: suffix-equivalent replace of `contains("sharepoint")` in reader if still cheap.
- [x] No production `unwrap`/`expect`.

## Phase 2 — Docs + deferred → DoD-4

- [x] Runbook: in-scope US GCC High/DoD; `admin.onedrive.us` expectation; 21Vianet exclusion; SafeLinks historical bound.
- [x] Close `D-0085-sovereign-cloud-hosts`; open `D-0088-usgovcloud-microsoft-tld`.

## Phase 3 — Finalize → DoD-5

- [x] Write `review.md`.
- [x] Set `../conductor.md` status to **Completed**.
- [x] Commit ledger TX.

---

## Handoff notes

- Thin track — prefer hours/days, not a writer program.
- Do not invent attachments from body URLs.
- Single-exe / offline-only invariant unchanged.
