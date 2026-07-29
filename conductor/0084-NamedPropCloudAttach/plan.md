# 0084 — Named Property Resolution & Cloud Attach Detect — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.
>
> **Review fold-in (2026-07-29):** pointer preserve (DoD-4b), ledger `cloud_url`/`cloud_provider`,
> body-link honesty residual, Phase-0 GUID+name lock — see `spec.md` §2.10.

> **Ledger:** open a transaction before starting —
> `ledgerful ledger start 0084-NamedPropCloudAttach --category FEATURE --message "<intent>"`
> — and commit it in the final phase.

---

## Phase 0 — Precondition / allowlist gate → DoD-9

- [x] Confirm board: 0080–0083 **Completed**; workspace builds.
- [x] Re-read MS-PST **Named Property Lookup Map** (NID `0x61`) live Learn page; cite access date in `review.md`.
- [x] **Lock allowlist (verified 2026-07-29; re-confirm if >7 days):**
  - GUID **PSETID_Attachment** `{96357F7F-59E1-47D0-99A7-46515C183B54}`
  - Name **`AttachmentProviderType`** via **`NamedPropId::Name`** (not LID)
  - Type PtypString; open provider string (`OneDrivePro` / `OneDriveConsumer` documented — do not closed-enum)
  - Phase 0 sample: which classic tag or co-named prop holds **URL** on synthetic / optional operator PSTs → lock `cloud_url` extraction order
- [x] Inventory writer stub for `NID_NAME_TO_ID_MAP` and attach PC write path for **fixture synthesis**.
- [x] Confirm live ghost path: `write_one_attachment` returns `Ok(None)` for non-BY_VALUE/EMBEDDED → plan CloudLink **pointer-row** branch (DoD-4b).
- [x] Grep for existing NPMAP parse (must not duplicate).
- [x] Re-query crates.io if >7 days after 2026-07-29; expect **no bumps**.
- [x] `ledgerful ledger status --compact`; start FEATURE ledger tx.
- [x] Re-read `spec.md` §2.5–§2.10 — **no network hydration**; **no Preserved payload**; **attach-table only**; **OR signals intentional**.

## Phase 1 — NPMAP parse + resolve API → DoD-1, DoD-7

- [x] Implement Name-to-ID-Map parse on `PstFile` (cache per open).
- [x] Resolve (GUID + LID) and (GUID + string name) → NPID.
- [x] Unit tests: missing node; empty streams; one entry hit for `AttachmentProviderType`; unknown miss; corrupt entry → degrade (no process panic; PST still usable).
- [x] `cargo test -p pst-reader` green.

## Phase 2 — Attach classification + materialize flags → DoD-2

- [x] On attach PC load, resolve allowlisted named props when map present.
- [x] Classify CloudLink `{ provider, url }` vs classic; set explicit flag and/or force incomplete when no exportable payload.
- [x] Implement **independent OR** signals (named-prop hit **or** locked web-ref/method + no payload) — document in code comment + review.md so future readers do not “simplify” to named-prop-only.
- [x] Synthetic fixture (writer helper or test PST bytes) with NPMAP entry + cloud-like attach (+ URL string when possible).
- [x] `cargo test -p pst-reader` / materialize path tests green.

## Phase 3 — Incomplete, ledger actionability, Mode A → DoD-3, DoD-4

- [x] Extend `is_attach_incomplete` for cloud-without-payload.
- [x] Emit **`ATTACH_CLOUD_LINK`** on unique-pst attach ledger + histogram (prefer over bare METHOD_UNSUPPORTED when CloudLink).
- [x] **Append CSV columns** `cloud_provider`, `cloud_url` to `EXPORT_ATTACHMENTS_CSV_HEADER` + `AttachLedgerRow` + writers/docs; empty when N/A; neutralize injection on URL.
- [x] Tests: header present; CloudLink row populates provider/URL when fixture supplies them.
- [x] Mode A test: cloud incomplete peer + physical complete peer → promote when `--promote-on-attach-fail`.
- [x] Flag off: Mode C ledger only (cloud fail still counted).
- [x] `cargo test -p dedup-engine` / `pst-dedup-cli` green.

## Phase 4 — Pointer preserve (anti-ghost) → DoD-4b

- [x] Branch production writer / materialize path: CloudLink → **write metadata/pointer attach row** (no invented binary).
- [x] Best-effort classic string tag for URL/path when known (Phase 0 tag choice).
- [x] Test: source had attachment-table cloud row → unique-PST still has attach row (not silent omit) + ledger `ATTACH_CLOUD_LINK`.
- [x] If classic tags cannot carry URL for counsel: record **D-0084-cloud-named-prop-write** residual with evidence; ledger URL still mandatory.
- [x] Confirm parents_only / MethodUnsupported paths still correct for non-cloud OLE/ref methods.

## Phase 5 — Contract + QC + docs → DoD-5, DoD-6, DoD-8

- [x] Update `fidelity_contract_v1` reasons/status for cloud + provider prop (not Preserved for payload; **attach-table scope** + body residual named in reason text).
- [x] Fix/adjust QC contract tests that expected pure blind-spot wording.
- [x] Docs: unique-pst-export, eDiscovery runbook:
  - offline; no hydration
  - pointer preserve for supplemental discovery
  - Mode A benefit
  - **honesty: body-only inline links not classified** (Purview-shaped residual D-0084-body-cloud-links)
  - do **not** cite Cloudficient draft as settled industry vocabulary
- [x] `docs/deferred.md`: close D-0080-cloud-attachments (attach-table detect); narrow D-0068-04; open D-0084-body-cloud-links (+ D-0084-cloud-named-prop-write if needed).
- [x] CHANGELOG `[Unreleased]`.
- [x] Confirm no reqwest/network path for attach download was introduced.

## Phase 6 — Full verification + finalize → DoD-10, DoD-11

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo deny check`
- [x] `ledgerful verify` (or justified fallback)
- [x] Write `review.md`: NPMAP design, allowlist, OR-signal intent, ledger columns, pointer-preserve evidence, contract before/after, deferred closes/opens, residual (full named write, hydration, body links, attach-content), dual-AI fold-in table.
- [x] Update `../conductor.md`: 0084 → **Completed**; Series M next candidates (body cloud links residual, D-0076, D-0079, D-0073-eml).
- [x] Commit ledger transaction.

---

## Handoff notes

- **Detection ≠ collection.** Never download cloud files in this track.
- **Attach-table ≠ body links.** Closing D-0080 does not close modern-attach completeness.
- Prefer **explicit** cloud flags over overloading `stream_available` alone if parents_only semantics get muddy — document choice in review.md.
- Writer production full named-prop stub may remain; fixture-only NPMAP write is fine; **CloudLink pointer row is in-scope**.
- Do not expand into full PidLid calendar set mid-track — split if needed.
- Production forbids `.unwrap()` / `.expect()`.
- Rollback: leave cloud as known residual if NPMAP proves too large — do not ship silent Preserved.
- Future body-scan residual should treat Purview limits (HTML only, URL/body caps, first-N links) as **design inputs**, not as product claims of parity.

