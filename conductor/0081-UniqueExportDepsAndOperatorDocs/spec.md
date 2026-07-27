# 0081 — Unique Export Dep Pins & Operator Docs

- **Track ID:** 0081-UniqueExportDepsAndOperatorDocs
- **Status:** Ready
- **Series:** L

## 1. Objective

Audit and pin **current** crate versions for the unique-export path; publish an **operator runbook** for multi-mailbox eDiscovery PSTs (INC-style) covering integrity, CRC noise, attach partials, policy choice, and ScanPST/re-export decisions.

## 2. Context — dependency snapshot (workspace 2026-07-26)

| Dep | Workspace pin | Notes / research |
|-----|---------------|------------------|
| clap | 4 | Stay on 4.x; 4.5+ derive/env fine |
| serde / serde_json | 1 | Stable |
| sha2 | 0.11 | Current line; keep for content hashes |
| md-5 | 0.10 | EDRM MIH only |
| rusqlite | 0.40 | Confirm latest 0.40.x / 0.41 when auditing |
| chrono | 0.4 | Stay |
| thiserror | 2 | Stay |
| eframe | 0.34 | GUI path only |
| tantivy | 0.26 | FTS; not unique-pst critical path |
| reqwest | 0.12 | Service/AI; not unique-pst |
| object_store | 0.14 | Feature-gated cloud |

**Action:** run `cargo outdated` / crates.io check for unique-export crates (`pst-reader`, `pst-writer`, `dedup-engine`, `pst-dedup-cli`); bump only within deny.toml policy; no drive-by majors mid-RC without 0062-style freeze note.

## 3. In scope

1. Dependency audit write-up in track `review.md` (current latest compatible pins).
2. Operator doc `docs/unique-pst-ediscovery-runbook.md`:
   - inspect → scan → keep-set → unique-pst flow with timings expectations
   - interpret attach partial / CRC risk / first_seen path order
   - when to ScanPST / re-export from Microsoft 365 eDiscovery
   - prefer_path / folder policies (0075)
   - artifact inventory
3. Link from README + operator-golden-path.
4. Optional: sample PowerShell timing wrapper used on INC case.

## 4. Out of scope

- Full workspace major upgrades.
- Codesign (0062 residual).

## 5. DoD

- [ ] Dep audit recorded
- [ ] Runbook merged and linked
- [ ] No broken cargo deny/audit
