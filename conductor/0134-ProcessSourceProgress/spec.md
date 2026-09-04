# 0134 — Process source progress (rows + real drop)

> Do **not** steal **0122** Busy, **0126** jobs Dupes/NIST `—`, **0133** unaccounted formula.
> Do not vendor the mockup. No OST/MBOX. No BCC-default. Keep **Add folder** and **Add ZIP / PST**.

- **Track ID:** 0134-ProcessSourceProgress
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` is **absent**; do not chase it.
- **Cross-repo contract:** mockup is workflow research only. Steal basename + honest progress + drop that **ingests**. Do not port ACME GB / fake 68%.
- **Status:** Ready — not started
- **Depends on:** **0133** (label gap first; do not hide Unaccounted-for) · **0126 Completed**
- **Spec authored:** 2026-09-03 (placeholder) → **2026-09-03 Ready** (HEAD `cc88576`)
- **Series:** V

> **Closes / absorbs:** `D-0134-source-progress`, **`D-0116-drop`** (actual Tauri file drop). Does **not** add OST/MBOX engines (`detect.rs` PackageKind).
> **HITL:** release chrome EXE: drop a fixture PST onto Process (not HTML5 fake); row shows basename + status; optional size if the file still exists. Keep both Add buttons. Source PSTs read-only.

---

## 1. Objective

Bring the **Sources** pane to mockup-grade workflow: basename, honest status, extract progress when the live snapshot is that source — **and** make the drop-zone a real drop target — without dropping chrome’s extra ingest kinds (ZIP, Purview, folder) or the two picker buttons.

---

## 2. Context (read before starting)

### 2.1 Why now

Mockup: custodian filename, GB, item progress, done/running/queued. Chrome HITL: full Desktop paths, `single_pst · importing` lag, drop-zone is **copy only** (D-0116-drop). 0126 forbade fake 68% and OST/MBOX ads.

### 2.2 Live APIs (plan-time `cc88576`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| `ProcessSourceRow` / `Source` | `id, path, kind, status` only. **No** `size_bytes`. 0126: do **not** `list_items_for_source` on every `process_page`. |
| Progress | `source_shows_extract_progress` already maps `extract_current_name` + `extract_pst` snapshot to a source via inventory paths. |
| Drop | **No** `onDragDropEvent` / `on_drag_drop` in chrome today. `tauri.conf.json` windows omit `dragDropEnabled` → Tauri 2 default **true**. |
| Tauri 2 drop (docs 2026-09-03) | `getCurrentWebview().onDragDropEvent`; `payload.type === 'drop'` → `payload.paths`. HTML5 drop and Tauri drop are **mutually exclusive** on Windows. **Do not** set `dragDropEnabled: false`. |
| Chrome JS | `withGlobalTauri: true`. Pickers use `wasm_bindgen` `["window","__TAURI__","dialog"]`. **No** `@tauri-apps/api` npm. **Two-branch drop gate (fold-in):** (a) bind `["window","__TAURI__","webview"]` `getCurrentWebview().onDragDropEvent`; (b) if that namespace is missing, host-Rust `on_drag_drop_event` in chrome `setup` + `emit` / WASM `["window","__TAURI__","event","listen"]`. **Only after both fail** does DoD-2 record API-blocked / leave D-0116-drop open. **No fake HTML5 drop.** |
| Capabilities | Live `capabilities/default.json` already has `"core:default"`. Tauri 2 docs: `core:default` **includes** `core:event:default` (`allow-listen` / `allow-unlisten`). Do **not** add a duplicate `core:event:default` unless execute hits a live `event.listen not allowed`. Re-verify the nested grant at execute. |
| Ingest | `process_start` kind `ingest`, params `{ "path": "…" }`. Busy = `RunnerError::Busy`. Allowlist kinds already refuse unsupported. |
| Ingest kinds | `single_pst` / `single_zip` / `purview_package` / `raw_dump` / `unsupported`. |
| Schema | **41**. No bump. |
| MS-PST | **N/A this track.** |

### 2.3 Locks

Keep pickers. Keep Purview/folder drop copy (0126: PST · ZIP · Purview package · folder — not OST/MBOX). Cheap size: **file-only**. Use `symlink_metadata` (same reparse-reject pattern as `ingest-purview` `expand.rs`); `is_file()` then `len()`, else `None`. Never `walkdir`. Fail-closed `None` on any IO/permission/UNC error. Progress bar only when `source_shows_extract_progress` is true. Do not invent GB from ACME.

Drop → existing `process_start` ingest. Do **not** auto-extract (0133 CTA). Do **not** wipe 0122 `extract_queue`. Busy uses the **picker-identical** path (`is_busy_invoke_err` / same error signal). Multi-file: ingest the first path; remaining paths either drain sequentially after each ingest success (separate from `extract_queue`) **or** fail-closed with `error` listing **unqueued** basenames — never silent drop. Document the chosen policy in `review.md`.

`dragDropEnabled` stays omitted (default true). `include_str` tripwire: `tauri.conf.json` must not contain `dragDropEnabled: false`.

### 2.4 Tools / comments

Same as 0133 Ready pass. Bugbot usage-limit **Decline**. Impact LOW. Recall: D-0116-drop still copy-only after 0126.

---

## 3. In scope

1. Source row: basename (strip `\\?\`), `kind · status`, optional `size_bytes: Option<u64>` (`#[serde(default)]` WASM) — **file-only**, see §2.3.
2. Progress when live extract maps to that source (existing helper; show a bar only then).
3. Wire drop-zone via the two-branch gate (§2.2). Multi-file: sequential drain **or** listed unqueued Busy fail — picker-identical error copy.
4. Tests: basename helper; size None for dirs/missing; drop handler ingest params; unsupported refused; `tauri.conf.json` has no `dragDropEnabled: false`.

## 4. Out of scope

- OST/MBOX/7z parsers.
- Jobs table Source column (**0135**).
- Password vault (**0136**).
- Auto-extract on drop.
- Changing 0133 unaccounted math.

## 5. Preconditions

- **P1:** 0133 CTA exists or this track must not hide Unaccounted-for.
- **P2:** Two-branch drop gate (§2.2). Only after **both** WASM webview and host-Rust emit fail does DoD-2 record API-blocked and leave D-0116-drop open.

## 6. Risks

| Risk | Mitigation |
|---|---|
| HTML5 drop used instead of Tauri paths | Forbidden on Windows with default `dragDropEnabled` |
| Drop starts extract | Ingest only |
| `list_items_for_source` on poll | Forbidden — size from `fs::metadata` only |

## 7. Definition of Done

- [ ] **DoD-1** Source rows show basename + live status + optional size; progress only when honestly mapped.
- [ ] **DoD-2** Drop ingest works for PST/ZIP/folder/Purview package **or** execute records API-blocked with D-0116-drop still open (no silent skip, no fake HTML5 drop).
- [ ] **DoD-3** Both Add buttons remain. No OST/MBOX copy. `dragDropEnabled` stays default true.
- [ ] **DoD-4 Recorded.**

## 8. Verification

```powershell
cargo test -p dedupe-chrome
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml
```

Owner HITL on release EXE (CDP/WebView2 optional). Playwright cannot drive egui Desk.

## 9. Deferred roll

| Row | Disposition |
|---|---|
| D-0116-drop | **Absorb** if drop ships; else remain with execute note |
| D-0134-source-progress | **Absorb** |
| D-0016-05 7z | Decline |
| D-0024-01 NSRL | Decline |
| D-0116-workflow | Remain |
| D-0133 | Do not steal formula |
| Last-PR comments | **Decline** |
| Fold-in opencode-M1 | **Fold** — two-branch drop gate (WASM then host-Rust emit) |
| Fold-in AGY-134-01 | **Decline / partial** — `core:default` already includes `core:event:default`; re-verify at execute; add explicit grant only on live deny |
| Fold-in opencode-m1 / AGY-134-03 | **Fold** — file-only `symlink_metadata` size; never walkdir |
| Fold-in opencode-m2 / AGY-134-02 | **Fold** — picker-identical Busy; multi-file never silent |
| Fold-in opencode-O1 | **Fold** — `tauri.conf.json` tripwire: no `dragDropEnabled: false` |
