# 0134 — ProcessSourceProgress — Review

## Scope

Source rows show basename + `kind · status` + optional file-only `size_bytes` (`symlink_metadata`, never walkdir). Real Tauri drop: WASM `onDragDropEvent` then host `process-file-drop`. Multi-file: ingest first; list every unqueued name. Both Add buttons kept. `dragDropEnabled` left at Tauri default.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Basename/status/size/honest progress | PASS | File-only size; mapped extract progress only. |
| DoD-2 Real Tauri drop; honest multi-file | PASS | Two-branch listener; `attach_drop_listener` surfaces registration failure; `drop_error_after_start` lists names on Busy and non-Busy errors. |
| DoD-3 Keep Add buttons; no OST/MBOX | PASS | Both Add buttons; drop copy names PST/ZIP/Purview/folder. |
| DoD-4 Recorded | PASS | This file; registry Completed; product squash PR **#150**. |

## Gates

Same Series V gate as 0133 (PR **#150** / `a8287b4`). Final Codex `review.codex-r2.md` **PASS**.

## HITL (owner)

Owner chrome EXE drop. Spec allows after merge.

## Residual lows

Host `emit("process-file-drop")` ignores delivery `Err` (`lib.rs`); registration failure is surfaced. Not a DoD gap.

## Publish

- PR **#150** / `a8287b4`
- Closes **D-0134-source-progress** and **D-0116-drop**
