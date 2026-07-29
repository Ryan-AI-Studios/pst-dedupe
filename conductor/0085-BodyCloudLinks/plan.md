# 0085 — Body-Inline Cloud Link Detect — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Review fold-in (2026-07-29):** document-shaped allowlist (`:x:` Excel mandatory),
> never strip query params, Mode A known-gap honesty, sovereign residual, ReDoS-by-construction —
> see `spec.md` §2.10.
>
> **Ledger:** open a transaction before starting —
> `ledgerful ledger start 0085-BodyCloudLinks --category FEATURE --message "<intent>"`
> — and commit it in the final phase.

---

## Phase 0 — Precondition / pattern gate → DoD-9

- [x] Confirm board: 0082–0084 **Completed**; D-0084-body-cloud-links **open** in `docs/deferred.md`.
- [x] Re-read Purview [Cloud attachments in eDiscovery](https://learn.microsoft.com/en-us/purview/edisc-cloud-attachments) (access date in `review.md`); lock caps: 100k / 2048 / 50 as defaults.
- [x] Grep materialize for `body_html` / `body_plain` availability on unique-pst path.
- [x] Lock **document-shaped allowlist** table:
  - Action tokens: `:w:`, **`:x:` (Excel — mandatory)**, `:p:`, `:b:`, `:u:`; **exclude `:f:`** by default
  - Extension markers: `.docx` / `.xlsx` / `.pptx` / `.pdf` / … (Phase 0 final list)
  - Hosts: commercial SharePoint / OneDrive / `1drv.ms` / SafeLinks only
  - **Reject** bare site roots / intranet pages without document markers
- [x] Confirm **never strip query params** on normalize (HTML-unescape + trim only).
- [x] Note residual **D-0085-sovereign-cloud-hosts** (GCC High / DoD SafeLinks + SharePoint suffixes unconfirmed).
- [x] Decide quoted-content policy: **full-body scan** (recommended) vs blockquote skip — record in review notes.
- [x] Decide contract shape: extend `cloud_modern_attachments` reason vs add `body_cloud_links` property — prefer one clear story; must include Mode A body-only **known gap**.
- [x] Confirm **no new major deps**; workspace **`regex` only** (no `fancy-regex`); ReDoS closed by construction — **do not** budget ReDoS property tests.
- [x] Re-query crates.io if >7 days after 2026-07-29; expect KEEP.
- [x] `ledgerful ledger status --compact`; start FEATURE ledger tx.
- [x] Re-read `spec.md` §2.5–§2.10 — **no invent attach**, **no Mode A body incomplete**, **no hydration**, **document-shaped only**, **query preserve**.

## Phase 1 — Pure scanner → DoD-1, DoD-4 (unit)

- [x] Implement `scan_body_cloud_links` in `dedup-engine` (or dedicated module re-exported).
- [x] Caps: body window, URL length, max 50, exact-string dedupe **preserving query**.
- [x] SafeLinks unwrap when nested target is **document-shaped**.
- [x] **Allowlist accuracy tests** (primary effort):
  - hit: `:w:`, **`:x:`**, `:p:`, `:b:`, `:u:`, `.xlsx` path, `1drv.ms`, SafeLinks→document target with `?d=` retained
  - miss: bare `…sharepoint.com/sites/HR`, non-cloud https, `:f:` folder (default)
  - two queries differ → two rows; cap truncation flag; empty body
- [x] `cargo test -p dedup-engine` green.

## Phase 2 — Report schema + unique-pst wire → DoD-2, DoD-3

- [x] Add `EXPORT_BODY_CLOUD_LINKS_CSV_HEADER` + writer (reuse batching patterns from attach ledger where practical).
- [x] Append `body_cloud_link_count` to `EXPORT_MESSAGES_CSV_HEADER` (append-only).
- [x] After materialize (or during report finalize), run scanner; enqueue rows; fill counts.
- [x] Summary histogram fields (messages_with_body_cloud_links, body_cloud_links_total, truncated_messages).
- [x] Integration test: synthetic HTML body with document-shaped URL (incl. query) → CSV present; messages column count matches.
- [x] Assert **no** new attach row invented; `is_attach_incomplete` false for body-only fixture.
- [x] `cargo test -p pst-dedup-cli` targeted green.

## Phase 3 — Mode A / attach non-regression → DoD-6

- [x] Re-run Mode A × attach CloudLink tests (0084).
- [x] Explicit test: body-only cloud URL message remains attach-complete; Mode A does not soft-skip solely for body hits.
- [x] Confirm exit 64 not forced by body-only hits alone (fixture with clean attaches).
- [x] Docs note for implementer: known gap “physical peer vs HTML-inline-only” lands in Phase 4 prose (not Mode A code change).

## Phase 4 — Contract + docs → DoD-5, DoD-7, DoD-8

- [x] Update `fidelity_contract_v1` — body residual closed; payload still not Preserved; **Mode A body-only known gap** explicit; document-shaped commercial allowlist; sovereign residual named.
- [x] Docs: unique-pst-export, eDiscovery runbook:
  - hit-list workflow; **full URL + query** for as-sent native collection
  - document-shaped filter (not intranet noise)
  - caps; offline honesty
  - Mode A non-interaction **and** physical-vs-inline known gap
  - sovereign residual
- [x] `docs/deferred.md`: close **D-0084-body-cloud-links**; open **D-0085-sovereign-cloud-hosts**; leave D-0084-cloud-named-prop-write / D-0076 / D-0079 / D-0073-eml as declined or residual.
- [x] CHANGELOG `[Unreleased]`.
- [x] Optional: unique-eml reuses scanner + CSV if body path already hot — only if < small additive; do not open full D-0073-eml. (skipped — not cheap additive)

## Phase 5 — Full verification + finalize → DoD-10, DoD-11

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace` (+ post-fix body_cloud 26 + unique_pst body_cloud 3)
- [x] `cargo deny check`
- [x] `ledgerful verify` (or justified fallback)
- [x] Write `review.md`: fold-in table (§2.10), allowlist tokens incl. `:x:`, query-preserve evidence, ReDoS-by-construction note, Mode A known gap, deferred close/open, residual hosts. Codex FAIL→fix → final PASS WITH DEFERRED P3.
- [x] Update `../conductor.md` + `ROADMAP.md`: 0085 → **Completed**.
- [x] Commit ledger transaction.

---

## Handoff notes

- **Body URL in PST body ≠ file collected.** Hit-list is for operators / native collection — keep **query strings**.
- **Document-shaped only** — bare SharePoint intranet is a miss, not a hit.
- **Do not** feed body hits into `is_attach_incomplete`.
- **Do not** invent attach objects to “look like” modern attachments.
- Stock **`regex` only** — no `fancy-regex`; spend tests on allowlist accuracy.
- Production forbids `.unwrap()` / `.expect()`.
- Rollback: if allowlist false-positive rate is bad, narrow markers further rather than host-only loosen without product decision.
- **Implementation note:** body scan runs at prepare (`prepared_winner_from_canonical`) because write-path `TakeWriteMsgs` moves `WriteMessage` bodies out of prepared before export row construction.
