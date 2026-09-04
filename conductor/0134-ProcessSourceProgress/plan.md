# 0134 — Process source progress — Plan

> Depends on **0133** for Unaccounted-for copy. Status: **Ready — not started**.
> Fold-in 2026-09-03: `opencode-review.md` + `agy-review.md`.

> **Ledger:** `ledgerful ledger start 0134-source-progress --category FEATURE --message "Process source rows + real Tauri drop"`

---

## Phase 0 — Pin drop API → DoD-2

- [ ] Re-read Tauri 2 `onDragDropEvent` + `tauri.conf.json` (do **not** set `dragDropEnabled: false`).
- [ ] Confirm `core:default` still nests `core:event:allow-listen` (docs). Only add `"core:event:default"` if execute hits `event.listen not allowed`.
- [ ] Two-branch probe: (a) `__TAURI__.webview.getCurrentWebview().onDragDropEvent`; (b) if missing, host `on_drag_drop_event` + emit + WASM `event.listen`. Both fail → D-0116-drop remains.
- [ ] Confirm `Source` / `ProcessSourceRow` still have no size field.

## Phase 1 — Rows → DoD-1

- [ ] Host `size_bytes: Option<u64>`: `symlink_metadata`; `None` unless `is_file()`; then `len()`. Never `walkdir`. `None` on any error. No `list_items_for_source`.
- [ ] WASM `#[serde(default)]`.
- [ ] UI: basename + `kind · status` + size when Some. Progress bar only if `source_shows_extract_progress`.

## Phase 2 — Drop → DoD-2 / DoD-3

- [ ] Subscribe via the branch that Phase 0 selected. On `drop`, `process_start` ingest `{ "path": … }`.
- [ ] Multi-file: sequential ingest drain **or** fail-closed listing unqueued basenames on the same `error` signal as pickers (`is_busy_invoke_err`). Never silent. Must not write `extract_queue`. Must not auto-extract.
- [ ] Keep Add folder / Add ZIP / PST.
- [ ] Test: `tauri.conf.json` production snippet has no `dragDropEnabled: false`.

## Phase 3 — Finalize → DoD-4

- [ ] `review.md`; close D-0116-drop **only** if drop works; ledger commit.
- [ ] HITL: drop fixture PST/ZIP on release chrome EXE.

## Handoff

- Do not advertise OST/MBOX.
- Do not `list_items_for_source` on every `process_page` poll.
- Do not treat a missing `__TAURI__.webview` as API-blocked until the host-Rust branch is tried.
