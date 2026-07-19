# dedupe-desk

**Dedupe Desk** — single-exe Windows shell for matter create/open, source ingest,
PST extract, matter-level process jobs, and a **Review** surface for the promoted
corpus (tracks **0020**–**0029**).

Keyword FTS (0029): Review keyword box composes with metadata filters; Workspace
has **Build / Update search index** and **Rebuild search index** (`fts_index`).

Office extract (0033): Workspace **Extract Office text** runs `office_extract`.
PDF extract (0034): Workspace **Extract PDF text** runs `pdf_extract`; Review shows
Needs OCR banner when `pdf_needs_ocr=1` (empty/low-text).
(DOCX/XLSX/PPTX natives → `text_sha256`).
ICS extract (0035): Workspace **Extract ICS** runs `ics_extract`; Review **Calendar**
chip filters `file_category=calendar`. Multi-event ICS → archive parent + single-event children.
OCR (0036): Settings **Enable local OCR** (off by default) + tool paths; Workspace
**Run OCR** (`ocr` job) processes needs-OCR PDFs and images via system Tesseract CLI.

### Case Overview (0038)

When a matter is open, Workspace shows a **Overview** panel above the Counts strip:

| KPI / table | Source |
|---|---|
| Items · Top-level size · Review progress · Errors · Needs OCR · Withhold | `matter_core::CaseOverview` |
| File categories / Custodians / By status | Top-N rollups (empty → `(uncategorized)` / `(none)`) |
| Dedup / Cull | unique/duplicate + included/culled |
| Errors by code | Top-N error codes (actionable ops view) |
| Jobs strip | Counts by state + last few jobs |

- **Top-level size** tooltip: sum of sizes for standalone + parent only (excludes attachments; no PST double-count).
- **Review progress**: `reviewed / in_review` where reviewed = ≥1 code on the item.
- Load path: **background thread only** (`OverviewLoadState` → `load_case_overview`); concurrent SQL fan-out inside matter-core. Never runs overview SQL on the egui thread.
- Refresh: matter open, post-job refresh, **Refresh**, and **Refresh overview**.
- Evidence: synthetic tempfile matters only — never put client subjects/paths/bodies in docs or screenshots.
- **0039** must reuse `CaseOverview` for CSV/PDF reports (no duplicate rollup SQL).

## Build / run

```powershell
cargo run -p dedupe-desk
# release:
cargo build --release -p dedupe-desk
.\target\release\dedupe-desk.exe
```

Legacy scan/dedup wizard remains at `pst-dedup-gui` (`pst-dedup-gui.exe`).

## Architecture (UI thread rules)

```text
egui UI thread                         process-runner matter-worker
─────────────────                      ────────────────────────────
start / resume / cancel     ──────►    open Matter, run handlers
watch_progress.borrow()                publish JobProgressSnapshot
open_for_read (lists/stats)            WAL writes
rfd on background thread               (never stage APIs on UI)
request_repaint_after(100ms)
  while job Running
shutdown / Drop               ──────►  cancel + join worker
```

### Hard rules

| Do | Don't |
|---|---|
| `ProcessRunner::start` / `cancel` / `resume` / `watch_progress` | Call `ingest_path` / `extract_pst_*` on the UI thread |
| `Matter::open_for_read` for live list refresh | `Matter::open` from a concurrent poller (wipes `workspace/temp`) |
| Off-thread `rfd` + `dialog_open` debounce | Sync file dialogs on the UI thread; multi-dialog spam |
| `request_repaint_after(100ms)` while Running | Free-run `request_repaint()` every frame (~144 Hz CPU burner) |
| `runner.shutdown()` on exit | Detach worker without join |

### Repaint policy

While a job is **Running**, the shell requests a repaint every **100 ms** (~10 FPS). That is enough for progress bars and does not peg a CPU core or starve the matter worker.

### Dialog debounce

`dialog_open` disables all Add/Open/Create pickers until the background `rfd` thread returns a path or cancel. Prevents stacked Explorer windows from double-clicks.

### WAL

matter-core configures `PRAGMA journal_mode=WAL`. The Counts panel shows the live journal mode. UI refresh uses `open_for_read` so concurrent worker writes do not race temp cleanup.

## Manual smoke (Windows)

1. `cargo build --release -p dedupe-desk`
2. Launch `.\target\release\dedupe-desk.exe`
3. **Create matter** under e.g. `output/desk-smoke/MyCase`
4. **Add PST** from `fixtures/` (or Add folder / ZIP)
5. Watch ingest progress; optionally **Cancel** then **Resume**
6. Select a discovered PST → **Extract selected** (or Extract all)
7. **Run dedupe** — tiered Message-ID → logical_hash → family attach policy
8. **Run threading** — Message-ID graph → subject → ConversationIndex → family inherit
9. **Run near-dup** — MinHash shingles + LSH clusters (pivot/member; not exact suppress)
10. Confirm jobs table + Counts (unique/duplicate) update; journal_mode shows `wal`
11. Close the app (worker joins; window may wait briefly if a job is finishing)

### Run dedupe

Workspace **Run dedupe** starts `kind=dedupe` with default params
(`use_message_id` / `use_logical_hash` / `suppress_children_with_parent` /
`reset` / `batch_size=500`). Uses the same progress / cancel / resume path as
ingest and extract. Identity is desk MID + `logical_hash` — not CLI content-hash.

### Run threading

Workspace **Run threading** starts `kind=thread` with default params
(`use_headers` / `use_subject_fallback` / `use_conversation_index` / `reset` /
`batch_size=500` / `family_inherit`). Reuses progress / cancel / resume.
Matters extracted before 0022 need **re-extract** to populate reply headers
(re-extract refreshes the four header columns on existing message paths).

### Run near-dup

Workspace **Run near-dup** starts `kind=neardup` with default params
(`minhash_shingle_v1`: k=5, cjk_char_n=2, H=128, 16×8 bands, threshold 0.80,
`skip_exact_duplicates`, `min_chars=80`, `reset`, `batch_size=200`). Reuses
progress / cancel / resume. Near-dup groups are **flag-only** — Desk does not
auto-hide them as exact duplicates. See `crates/matter-neardup/README.md`.

### Run cull

Workspace **cull preset** dropdown + **Run cull** starts `kind=cull`:

| Selection | Params shape |
|---|---|
| Built-in (`unique_only`, `unique_plus_family`, `noise_light`) | `{ "preset_name", "reset": true, "batch_size": 500 }` |
| User preset (from matter DB) | `{ "preset_id", "reset": true, "batch_size": 500 }` |

Built-ins that work out of the box are always listed. **User presets** appear
when present in the open matter’s `cull_presets` table (loaded on refresh).
Desk 0024 does not ship a full preset editor — create/update presets via
matter-core API (`upsert_cull_preset`) or future UI. `date_window` remains an
engine built-in but is **not** listed until bounds are filled — operators
supply `start`/`end` (offset-aware RFC3339) via JSON params or a user preset.
Flag-only: sets `cull_status` / reasons; never deletes items or CAS. Reuses
progress / cancel / resume. See `crates/matter-cull/README.md`.

### Promote to review

Workspace **promote policy** dropdown + **Promote to review** starts
`kind=promote` with defaults (`policy=auto`, `expand_families=true`,
`reset=true`, `batch_size=500`, review set **Review Corpus**).

| Policy | Meaning |
|---|---|
| `auto` | `cull_included` if any `cull_status` set, else `unique_only` |
| named | `cull_included`, `unique_only`, `unique_plus_family`, `all_extracted`, `cull_included_plus_family` |

Flag-only membership (`in_review` / `review_order`); never deletes items or CAS.
Bidirectional family expand is on by default. Reuses progress / cancel / resume.
See `crates/matter-promote/README.md`.

### Review screen (0026) + coding (0027) + filters (0028) + notes (0030) + privilege (0031) + redaction (0032)

Nav **Review** (or Workspace **Open Review**) shows the default Review Corpus:

| Region | Behavior |
|---|---|
| Keyword bar | Keyword box + **Search** / **Clear**; composes FTS hits ∩ metadata filters; status “N keyword hits · M after filters” |
| Index | **Update index** / **Rebuild index** (`fts_index` job; rebuild uses `reset:true`) |
| Filter bar | Custodian, codes, date from/to (RFC3339+offset), include family, **Note text contains…**; **Apply** / **Clear**; quick chips Uncoded / Privilege / Responsive / **Has notes** / **Has highlights** / **Has redactions** / **Redacted text stale** / **Withheld** / **Privilege log incomplete** |
| Privilege protocol | Collapsible 502(d)/502(e) notes + description_required — **informational only** (Desk does not issue FRE 502 orders) |
| Saved searches | Dropdown Load / Save (name) / Delete — stores `FilterSpec` JSON + optional `keyword` in `saved_searches` |
| Corpus list | Thin rows (`list_review_thin`, filtered, or keyword-composed); multi-select ☑; fixed `ROW_HEIGHT` 22.0 |
| Status | “Showing N of M” + **Load more** when filtered/large count exceeds loaded rows |
| Header | Subject, From, To/Cc (selection-time fetch), dates, path, mime, size, role chips; **📝 N notes · H highlights · ⬛ R redactions** |
| Code chips | Current-item codes; click chip to **remove** (no confirm) |
| Coding panel | Active code buttons toggle current item; batch **Add** / **Remove** mode; family checkbox; Apply |
| Privilege panel | When Privilege code or claim row present (or **Assert privilege**): basis, status, withhold, include_on_log, description, Save; optional **Draft from note…** (confirm — never auto on export); family split banner |
| Export | **Export privilege log…** (worker + file dialog) with Review-only checkbox; result shows row / blank-desc / withheld counts |
| Body | CAS text (`text_sha256` preferred, else `html_sha256` with block-aware strip); yellow highlights + **black redactions on top**; select text → **Highlight** / **Note on selection**, or **Redact** when Redact mode is on |
| Redact mode | Toggle + reason ComboBox (`privilege` / `pii` / `confidential` / `other`) + optional stamp label; **Regenerate redacted text** (off-thread) writes true CAS artifact |
| Notes panel | List newest-first; add document note; edit/delete; stale banner from **in-memory re-resolve** (not raw DB status alone) |
| Redactions panel | List reason/quote; delete per region; stale banner; “Redacted text outdated — Regenerate” when artifact missing |
| Family strip | Same-`family_id` members in the loaded list; click to open |

**Work product:** Notes and highlights live in the matter DB only. They are **not**
rewritten into CAS and are **not** included in production/load-file export by
default (opt-in later in **0040**). Privilege log descriptions are separate from
notes — never auto-copied into the log.

**Privilege / withhold (0031):** Applying the Privilege code ensures a claim row
(`withhold=1` by default). Soft-clear on code remove retains description for
internal audit only; cleared rows never appear on the privilege log. **0040**
must call `item_is_withheld` before production and must not dump cleared
`item_privilege.description` into load-file metadata.

**Prerequisite:** run **Promote to review** on Workspace first. Empty state points operators there.

**Keyword FTS (0029):** separate keyword box (Tantivy under `index/`) composes with the metadata filter bar. Metadata conditions stay SQL-only; body text is never FilterSpec SQL. Empty keyword restores metadata-only list. Digits 1–9 still require unfocused widgets (keyword box steals focus like filter fields).

**Keyboard (only when no widget has focus — filter / keyword text fields steal focus):**

| Action | Binding |
|---|---|
| Next | `]` or `Alt+N` or **Next** button |
| Previous | `[` or `Alt+P` or **Prev** button |
| Open selected | click list row or **Enter** |
| Toggle code 1–9 | Digits `1`–`9` map first 9 **active** codes on **current** item |

No wrap at ends. Focus gate: `ctx.memory(\|m\| m.focused().is_none())` (egui 0.34)
and note-editor / privilege-description / redact-reason focus (previous frame) — note / privilege / redact / filter / keyword `TextEdit` blocks digit coding.

#### Notes / highlights (0030)

| Step | Behavior |
|---|---|
| Document note | Notes panel multiline + **Save note** (`highlight_id` null) |
| Highlight | Select in body select box → **Highlight** (default yellow `#FFF59D`) |
| Passage note | **Note on selection** creates/binds highlight, opens draft with quote as **hint only**; operator types body and **Save** (empty body rejected) |
| Paint | Active ranges yellow on layout job; header/banner/list status from **re-resolve** against current body |
| Stale | Digest mismatch + quote not found → stale UI immediately; optional `persist_stale` aligns SQLite once per item+digest |
| Delete highlight | Unlinks notes (keeps note body as document-level) |
| Anchoring | Char indices + quote + digest; whitespace-normalized re-resolve on body drift |
| Filter | Quick chips **Has notes** / **Has highlights**; **Note text contains…** bound to `FilterDraft.note_text` |

#### Redaction (0032) — distinct from highlights

| Step | Behavior |
|---|---|
| Redact mode | Toggle on → selection creates **redaction** (black), not yellow highlight |
| Reason | ComboBox: privilege / pii / confidential / other; optional stamp label (metadata only) |
| Redact | Select in body select box → **Redact**; create **NULLs** redacted artifact pointer |
| Paint | Black bars **on top of** yellow highlights (`LayoutJob`) |
| Regenerate | Off-thread: merge active intervals → write CAS with fixed `[REDACTED]` token → set `redacted_text_sha256` |
| Stale | Missing/outdated artifact while count>0 → “Redacted text outdated — Regenerate”; region re-resolve reuses 0030 whitespace path |
| Privilege | `reason=privilege` sets claim `partial_redaction` + withhold |
| Filter | Quick chips **Has redactions** / **Redacted text stale** |
| Original CAS | Never rewritten — redacted text is a separate produce artifact for **0040** |

**Residual — dual body widgets (egui 0.34):** Review body uses a painted `Label` (`LayoutJob` highlight/redaction backgrounds) **plus** a second multiline `TextEdit` for char-range selection. Unifying paint + selection on one widget is deferred to avoid breaking highlight/redaction creation under the current egui version; operators select text in the lower box.

#### Filters / saved searches (0028)

| Step | Behavior |
|---|---|
| Apply | Compile draft → `FilterSpec` → `count_items_filtered` + `list_items_filtered_thin`; reset multi-select to present ids |
| Clear | Empty filter → full Review Corpus via `list_review_thin` |
| Include family | Expand hits to parent + attachments (conditions only on hits; outer still `in_review`) |
| Quick chips | Preset `FilterSpec` constants (`code_missing`, privilege, responsive) |
| Save / Load | Named row in `saved_searches`; Load replaces draft + Apply |
| Batch coding | Still **current multi-selection within the filtered list** — not auto-select-all-filtered |
| Paging | Load more appends next `LIMIT/OFFSET` page (compound partial index on matter DB) |
| Keyword + filter | FTS hit ids ∩ FilterSpec (family expand **after** intersect when include_family) |

#### Keyword FTS (0029)

| Step | Behavior |
|---|---|
| Search | Enter / **Search** → Tantivy QueryParser (Boolean/phrase, default AND) → unique item ids → compose with active FilterSpec |
| Clear keyword | Drops keyword; list reloads metadata-only (filter still applied if active) |
| Update index | Job `fts_index` incremental via `fts_text_sha256` bookkeeping |
| Rebuild index | `reset:true` after dropping any readers; recreates `index/` + clears fts_* columns |
| Save / Load | Optional `saved_searches.keyword` restored with FilterSpec |

#### Coding / batch (0027)

| Step | Behavior |
|---|---|
| Multi-select | Checkbox strip on each fixed-height list row |
| Current item | Panel buttons toggle; chips remove; digits 1–9; **no** confirm |
| Batch | Choose **Add** or **Remove** mode → check codes → **Apply to N selected** → confirm dialog (`N` selected, family-expanded `~M`) |
| Family | ☑ **Apply to family** (default **unchecked**) — whole unit: parent + all direct children (siblings) |
| Actor | `DeskSettings.reviewer_name` (Home “Reviewer (actor)” field; empty → `"desk"`) |
| Add code… | Coding panel creates custom def (label → slug key; group `custom`/`issues`; multi) |
| Large batch | Multi-item batch, family propagate, or N &gt; 50 → off UI thread + `request_repaint` |
| Codes in list | Up to 2–3 labels for **visible** viewport rows only (`list_item_codes`; selection always loaded for chips) |

**Privilege code + privilege log (0031):** tagging Privilege applies membership **and** ensures a claim row (withhold default on). Export privilege log writes the standard CSV; protocol notes are informational only (Desk does not issue FRE 502 orders).

#### egui traps (required)

| Trap | Desk mitigation |
|---|---|
| Variable-height list rows kill FPS on large corpora | Fixed `ROW_HEIGHT` (22.0) + `ScrollArea::show_rows`; single-line truncate; multi-select must not change height |
| Async body stays on “Loading…” until mouse moves | Worker clones `egui::Context`, sends channel payload, then **`ctx.request_repaint()`** |
| Shortcuts steal from filter / search boxes | Handle next/prev/digits only when `focused().is_none()` |
| Full corpus bodies in RAM | List never loads bodies; body load is selection-scoped + 2 MiB display cap |
| Huge batch freezes UI | Off-thread `apply_codes` when N &gt; ~50 |
| Flat WHERE drops family attachments | Family expand via CTE (matter-core); UI checkbox only |

**Load policy:** if count ≤ 50_000, load all thin rows; else first page of 500 + **Load more**.

## Tests

```powershell
cargo test -p dedupe-desk
```

Pure helpers cover nav, params JSON, settings, WAL refresh snapshot, dialog debounce,
HTML strip, review nav clamp, and tempfile list+body load. Full GUI interaction is
manual (see above).
