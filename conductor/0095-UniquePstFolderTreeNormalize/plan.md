# 0095 — Unique-PST Folder Tree Normalize — Plan

> Phased checklist; map to `spec.md` §7. Execute in `C:\dev\Dedupe`.
>
> **Review fold-in (2026-08-25):** Phase 0 **classifies** QC fail from existing `qc_findings.csv`
> (doubled ToPF is layout-real, QC-invisible). Layout: consecutive leading alias strip + lazy
> Unique Mail. **Close D-0070** via pre-seed. Deleted Items QC fixture/asymmetry. unique-eml N/A.
> See `spec.md` §2.8.

> **Ledger:** decide entity after Phase 0 triage. Likely
> `ledgerful ledger start crates/pst-writer --category BUGFIX --message "0095 preserve folder tree + D-0070 pre-seed"`
> (CLI pre-seed / QC matcher may share the TX or a follow-up on `crates/pst-dedup-cli`).

---

## Phase 0 — Classify + contract lock → DoD-2 draft, DoD-3 (partial)

- [x] Read operator-local `qc_findings.csv` `folder_tree_structure` **detail** (`out_folders` / `out_counts` / `expected`). Classify mode **(a–d)** (`spec.md` §2.5). No PST re-run required. → See `phase0-triage.md`: **(b)** + Deleted Items asymmetry + sanitize asymmetry; Unique Mail empty ghost.
- [x] Lock DoD-2 contract: sentinel list; case-fold; **consecutive leading** strip; multi-source **file-stem prefix** pre-seeded; Unique Mail **lazy in preserve**; flat unchanged.
- [x] QC: after writer alias strip, expected/out keys **must** use same `normalize_folder_path_key` (alias strip + sanitize). Not optional.
- [x] Deleted Items: stop treating message-bearing `/deleted items` as system.
- [x] Ledger entity: `crates/pst-writer` (TX `c61ed624-…`); CLI/QC share the work.
- [x] Hygiene: untracked root `agy-review.md` — do **not** commit.

## Phase 1 — Writer layout + D-0070 → DoD-1 (layout half)

- [x] `parse_folder_path`: strip leading consecutive aliases.
- [x] `IncrementalFolderPlan` + `WritePstOpts.known_source_paths` pre-seed. Close **D-0070**.
- [x] Preserve: lazy Unique Mail via `ensure_residual` (real NID). Flat: eager.
- [x] Update `writer_v1` empty-preserve hierarchy tests.
- [x] Writer unit + fidelity tests: non-sentinel preserved; later ToPF preserved; dual-source prefix on message 1.

## Phase 2 — QC + fixtures → DoD-1, DoD-4

- [x] Expected + out slots use `pst_writer::normalize_folder_path_key`.
- [x] Deleted Items asymmetry fixed; unit fixtures for DI / alias / sanitize / dual-source.
- [x] DoD-1 matrix covered by writer + QC unit tests (recoverable items inherits same path rules as Inbox).
- [x] Dual-source preserve: single IPM ToPF; no nested duplicate alias.
- [x] Flat layout tests still green.

## Phase 3 — Docs + deferred → DoD-2, DoD-3

- [x] `docs/unique-pst-export.md` tree contract.
- [x] Close `D-0070-multi-source-stream-prefix`. No D-0095-* spawned.

## Phase 4 — Finalize → DoD-5

- [x] `review.md`; conductor **Completed**; ledger commit (on ship).
- [x] Operator re-smoke guidance: folder_tree not defect; recipient_table is **0093**.

---

## Handoff notes

- Message count already matches — path-shape / claiming, not data loss.
- Do not disable folder-tree QC or collapse it to counts.
- Do not strip first-segment-always.
- Doubled ToPF is still worth fixing even if QC already suffix-matches it.
- 0094 nested export and 0093 heap are Completed — not this PR.
