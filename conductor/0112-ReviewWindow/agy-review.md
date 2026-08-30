# Track review: 0112-ReviewWindow

**Harness:** Antigravity (`agy`)  
**Track:** `conductor/0112-ReviewWindow`  
**Date:** 2026-08-30  

---

## Summary

Line-by-line verification of every Origin claim in `spec.md` against live code on `main` @ `cb4aa31`:

1. **Origin Claim 1 (Matter-core APIs for single item & CAS retrieval):**
   - *Verification:* Verified live in [`crates/matter-core/src/matter.rs`](file:///C:/dev/Dedupe/crates/matter-core/src/matter.rs).
   - `Matter::get_item` returns `Item` metadata with `text_sha256` and `html_sha256` CAS digests. `read_cas_prefix` allows bounded streaming prefix extraction without loading entire oversize CAS blobs into RAM.

2. **Origin Claim 2 (Review neighbor navigation helper):**
   - *Verification:* Verified that `Matter::review_neighbors` does not exist in `crates/matter-core`.
   - Adding `review_neighbors` in `matter-core` (rather than pulling raw SQL into `dedupe-chrome`) adheres strictly to architectural boundaries.

3. **Origin Claim 3 (HTML strip helper isolation):**
   - *Verification:* Verified that `html_to_review_text` currently resides exclusively in `crates/dedupe-desk/src/html_strip.rs`.
   - `dedupe-chrome` must duplicate/copy this helper to avoid creating an illegal workspace dependency on `dedupe-desk`.

4. **Origin Claim 4 (PR #113 Bugbot on exclusive write lock in code catalog):**
   - *Verification:* Verified live in [`crates/dedupe-chrome/src/codes.rs:L59`](file:///C:/dev/Dedupe/crates/dedupe-chrome/src/codes.rs#L59).
   - `review_code_catalog_blocking` unconditionally calls `open_matter_write`, blocking concurrent WAL readers. Refactoring to `open_matter_read` first (and writing only when empty) fixes this concurrency bottleneck.

---

## Blind-Spot Headlines

1. **False-Pass Hazard on HTML Block Separation Tests:** If `html_strip` naive regex merges adjacent paragraph tags (`<p>Hello</p><p>World</p> -> HelloWorld`), testing `assert!(res.contains("Hello") && res.contains("World"))` passes accidentally. Tests must assert exact word separation (`"Hello World"` or `"Hello\nWorld"`).
2. **Anchor Drop-Out in Neighbors Navigation:** When an item is coded `Responsive` under the `Unreviewed` filter, it drops out of the filtered set. If `review_neighbors` only searches within the filtered set by ID match, `next_id` returns `None`. Navigation must search relative to the anchor's sort key (`review_order`, `imported_at`, `path`, `id`).
3. **Privilege Basis Validation Barrier:** If `review_window_apply` does not validate that an asserted privilege basis exists before writing `item_codes`, a test passing a valid basis will not catch the missing validation. Tests must assert that applying `privilege` without a basis returns an error and does not mutate `item_codes`.
4. **2 MiB CAS Truncation Flag Verification:** Reading a 2 MiB + 1 byte blob must verify that `truncated == true` and the returned payload length is capped at 2 MiB.

---

## Findings (B/M/m/O)

| ID | Sev | Finding with concrete failure scenario | Fix |
|---|---|---|---|
| **F-0112-1** | **Major** | **Concurrent lock contention on `review_code_catalog`:** [`codes.rs:L59`](file:///C:/dev/Dedupe/crates/dedupe-chrome/src/codes.rs#L59) opens `open_matter_write` on every invocation, preventing concurrent background tasks from reading the catalog. | Open with `open_matter_read` first; only open `open_matter_write` if `active_empty == true`. |
| **F-0112-2** | **Major** | **Anchor drop-out failure on Save & Next:** In the Unreviewed queue, coding the current document removes it from the filter. If `review_neighbors` attempts an exact ID lookup inside the filtered result, navigation stalls. | Implement `review_neighbors` by comparing against the anchor's sort key (`review_order`, `imported_at`, `path`, `id`). |
| **F-0112-3** | **Minor** | **HTML text concatenation without whitespace separator:** Naive tag stripping converts `<p>Hello</p><p>World</p>` to `HelloWorld`, corrupting search/display tokens. | Ensure block-level elements (`<p>`, `<div>`, `<br>`, `<tr>`) emit newline/whitespace in `html_strip.rs`. |
| **F-0112-4** | **Minor** | **Privilege basis requirement bypass:** Persisting `privilege` code without an accompanying `privilege_basis` creates corrupt privilege log records. | Validate `privilege_basis` presence in `review_window_apply` before applying codes. |
| **F-0112-5** | **Observational** | **Strict XSS protection for email HTML:** Raw HTML from email CAS blobs must never be assigned to `innerHTML` in Leptos UI. | Render all document bodies strictly via `<pre class="doc-body">` text nodes. |

---

## What Looks Solid

- **Orthogonal Coding Model:** Responsiveness radios (1/2/3) are strictly separated from the Privilege checkbox (`p`) and type dropdown, preserving clean produce filter semantics (`responsive AND NOT withheld`).
- **Bounded CAS Streaming:** 2 MiB prefix cap prevents UI freeze and excessive memory consumption on multi-gigabyte attachments.
- **Family Card Safety:** `propagate_family` defaults to `false`, with explicit confirmation prompts when applying tags across a family group.
- **IPC Permissions Hygiene:** All new host commands are explicitly mapped in Tauri capabilities without blanket filesystem grants.

---

## Deferred Fold-In Table

| Deferred ID | Action | Rationale |
|---|---|---|
| `D-0112-review-window` | **Absorb and close** | Fully implemented by Track 0112. |
| `D-0026-03` | **Partial absorb (keep open)** | Text/HTML native viewer delivered; Image rastering owned by 0114. |
| `D-0117-queue-virtualization` | **Decline (keep open)** | Minted placeholder for PR #113 queue Bugbot items. |
| `D-0113-produce-checklist` | **Decline (keep open)** | Produce checklist; owned by Track 0113. |
| `D-0114-zpdf-raster` | **Decline (keep open)** | Native PDF/zpdf rasterizer; owned by Track 0114. |

---

## PR / Review Comments the Plan Missed

- None. The plan explicitly folds the catalog write-lock Bugbot finding from PR #113 into §3.3 and mints Track 0117 for the remaining queue UI items.

---

## Research / Tools Notes

- **ai-brains:** Used from `C:\dev\Dedupe`. Preflight verified (3892 pinned memories). Decision record `10a4067c` confirmed for Track 0112.
- **ledgerful:** Used from `C:\dev\Dedupe`. Verified status `0 pending / 0 unaudited drift`.
- **gh cli:** Verified last merged PRs (#114, #113, #112, #111).

---

## Verdict: Ready after fixes

The plan is well-architected, adheres strictly to repository boundaries, and is ready for implementation. Ensure `review_neighbors` uses sort-key traversal, HTML stripping preserves word boundaries, and privilege basis validation is enforced.

To fold in these review findings, run:
```powershell
/foldin 0112
```
