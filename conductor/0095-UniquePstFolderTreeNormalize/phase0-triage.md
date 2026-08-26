# 0095 Phase 0 triage — INC0102784 `folder_tree_structure`

Source: `output/inc0102784-0092-full/report/qc_findings.csv` (operator-local).
Message counts: out 4055 == expected 4055 (not data loss).

## Classified modes

| Mode | Evidence | Disposition |
|---|---|---|
| **(b) D-0070 prefix race** | 411 unprefixed doubled-ToPF paths (640 msgs) **and** 2031 `…/INC0102784/…` paths (3415 msgs) in the same volume | **Fix here** — pre-seed `known_source_paths` |
| **Deleted Items asymmetry** | 83 expected `…/deleted items` keys (345 msgs); output DI slots filtered by `is_system_folder_path` → matched 0 | **Fix here** — stop treating message-bearing `/deleted items` as system |
| **(a) Unique Mail residual** | `Unique Mail` present with **0** messages | Layout noise only; **lazy allocate** in preserve (not QC fail alone) |
| Doubled ToPF (layout) | `Root/ToPF/ToPF/<mailbox>/…` ubiquitous | **Fix here** — consecutive leading alias strip (counsel-visible) |
| Sanitize asymmetry | After simulating DI fix: 14 starved leaves (`"tony"`→`_tony_`, trailing dots, `**`→`__`) vs unclaimed sanitized outs | **Fix here** — expected keys use writer sanitize + alias strip |

## Contract lock (DoD-2)

- Sentinels (case-fold): `root`, `top of personal folders`, `top of information store`, `top of outlook data file`, `ipm_subtree`
- Strip consecutive **leading** only; stop at first non-alias
- File-stem multi-source prefix remains on (≥2 known sources, pre-seeded)
- Unique Mail lazy in preserve; flat still eager
- QC expected-key normalization **must** use the same alias strip + sanitize (required after writer strip or suffix match breaks)

## Not deferred

All classified QC interactions above are in-scope for 0095. Close `D-0070-multi-source-stream-prefix`.
