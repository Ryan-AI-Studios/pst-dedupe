# 0080 — Unique PST Outlook/Open QC Smoke

- **Track ID:** 0080-UniquePstOutlookQc
- **Status:** Ready
- **Series:** L
- **Depends on:** 0071 verification block

## 1. Objective

Automate **post-export QC** beyond open+count+sample MID: folder inventory, empty-folder rate, attach presence sampling, optional Outlook COM smoke on Windows when available.

## 2. Context

- unique-pst verification: open_ok, message_count_match, sample_mid_ok — not folder structure or attach presence.
- Operators still manually open in Outlook; COM automation is Windows-only and optional.

## 3. In scope

1. `pst-dedup unique-pst --qc` or subcommand `qc-pst unique.pst --expect-messages N`:
   - folder count, messages per top-level IPM subtree
   - sample N messages with attach count > 0 if source had attaches
   - compare to report pack if provided
2. Optional `--outlook-com-smoke` (Windows): create temporary profile / open data file if Outlook installed; skip if absent.
3. QC JSON artifact under report-dir.
4. CI: reader-only checks on fixtures; COM always skippable.

## 4. Out of scope

- Full MAPI property parity audit.
- GUI wizard wiring (thin later).

## 5. DoD

- [ ] Reader QC path always available
- [ ] COM path documented + skip-safe
- [ ] Tests on synthetic unique-pst output
