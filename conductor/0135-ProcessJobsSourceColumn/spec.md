# 0135 — Process jobs Source column (filename, not fake per-source grain)

> **0126 jobs grain stays jobs.** Dupes / NIST / Families / Except. stay `—` on each **job** row. Do not copy matter-wide totals onto rows.

- **Track ID:** 0135-ProcessJobsSourceColumn
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\`
- **Status:** Ready — not started
- **Depends on:** **0126 Completed** · **0133** preferred first (unaccounted copy)
- **Spec authored:** 2026-09-03 (placeholder) → **2026-09-03 Ready** (HEAD `cc88576`)
- **Series:** V

> **Closes / absorbs:** `D-0135-jobs-source-column`. Does **not** replace ingest/extract/profile with one mock “source job.”
> **HITL:** after two ingest jobs, Source column shows PST basenames (not only `ingest`). Pause on live extract-all still appears (**0122**).

---

## 1. Objective

Match the mockup’s **readable Source column** (custodian/file name) while keeping chrome’s honest job kinds (`ingest`, `extract_pst`, `profile_run`, …). INC* HITL showed two rows labeled only `ingest`.

---

## 2. Context (read before starting)

### 2.1 Live APIs (`cc88576`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| `ProcessJobRow` | `id, kind, state, parent_job_id, error_summary, started_at, finished_at` — **no** `source_id` / label. |
| `matter_core::Job` | Same. `JOB_SELECT_COLS` has **no** `params_json`. `jobs` table never grew a params column (schema **41**). **Do not bump schema** to add it. |
| Start params | Ingest `{ "path": "…" }` lives in the runner for the **active** job only. After persist, recover from **checkpoints**: |
| Ingest checkpoint | Stage `expand` → `ExpandCursor.source_id` + `package_root`. Map `source_id` → `Source.path` basename. |
| Extract checkpoint | Stage `pst_extract` → `ExtractCursor.pst_item_id` / `pst_path`. Map to inventory path basename. |
| `profile_run` / `qc` / `produce` | No file path → `source_label = None` → UI falls back to **kind** (0126). |
| 0122 lock | `is_orphan_running(&job_for_orphan, &progress.get())` string-lock. Dupes/NIST/Families/Except. stay `—`. |
| WASM | New fields `#[serde(default)]`. |

### 2.2 Locks

Add **optional** `source_label: Option<String>` (basename, strip `\\?\`) populated at `process_page` time. Fail-closed `None` if nothing maps.

**Ingest resolution (fold-in):** `ingest_path_on_job` inserts the source row before expand (`ingest.rs` ~104). Prefer `expand` checkpoint `source_id` → `Source.path`. If **no** checkpoint: use `list_sources()` only when the leftover is **unambiguous** (exactly one unlabeled ingest job and one source not claimed by another ingest checkpoint). Two unlabeled failures stay kind fallback — do not pair by clock. Do not fall back to kind when that unambiguous source path exists.

**Extract resolution:** prefer `ExtractCursor.pst_path` (stable leaf name / `item.path`); `open_fs_path` is the FS path and may be `None` — strip `\\?\` only on that FS field. Map `pst_item_id` through `pst_inventory` if `pst_path` is empty.

Query checkpoints **only** for `kind == "ingest" || kind == "extract_pst"`. `profile_run` / `qc` / `produce` stay `None` (kind fallback). Child jobs of `profile_run` do **not** inherit a parent source label this track (§4) — Source shows kind; note in `review.md`.

Do **not** merge two ingest jobs into one source row. Do **not** fill Dupes/NIST from `page.dupes`. Do **not** invent `Job.params_json`.

**UI lock:** Source cell may show the label **and** must still contain `{j.kind.clone()}` (0122/0126 `jobs_table_emdash_per_row_columns` string-lock). Example: label as name, kind as subline. Tests that assert `source_label.is_some()` on a running extract **before** the first checkpoint must allow `None` → kind fallback. Ingest-running can be Some from `list_sources` without a checkpoint.

### 2.3 Tools / comments

Same as 0133. Decline Bugbot usage-limit.

---

## 3. In scope

1. Host fills `source_label` per §2.2 (sources first for ingest; `pst_path` / inventory for extract).
2. UI Source column: label when Some **plus** live `j.kind.clone()` (string-lock). Else kind only.
3. Tests: ingest with source row and **no** expand checkpoint → basename (not `ingest`); extract with `pst_extract` cursor → leaf name; `profile_run` → kind; `{j.kind.clone()}` still in the row; Dupes/NIST `—`; orphan Pause string-lock.

## 4. Out of scope

- Per-row Dupes/NIST/Families/Except. numbers.
- One-row-per-source table rewrite.
- Schema bump / `jobs.params_json` column.
- Exception vault (**0136**).

## 5. Preconditions

- **P1:** 0126 jobs table in live chrome.
- *Verified:* `list_jobs` does not return start params; checkpoints do.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Implementer adds params column | Forbidden; schema 41 |
| Painting `page.dupes` on rows | Tests lock `—` in those cells |

## 7. Definition of Done

- [ ] **DoD-1** Job rows show basename when `list_sources` / extract cursor / inventory know it (ingest **without** expand checkpoint still labeled); otherwise kind. Row still contains `{j.kind.clone()}`.
- [ ] **DoD-2** 0122 orphan Pause string-lock still passes; Dupes/NIST/Families/Except. still `—`.
- [ ] **DoD-3 Recorded.**

## 8. Verification

```powershell
cargo test -p dedupe-chrome
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml process
```

## 9. Deferred roll

| Row | Disposition |
|---|---|
| D-0135-jobs-source-column | **Absorb** |
| 0126 Dupes/NIST `—` | **Keep** |
| D-0116-workflow | Remain (no fake source-grain jobs) |
| Last-PR comments | **Decline** |
| Fold-in opencode-M1 | **Fold** — ingest labels from `list_sources` first |
| Fold-in AGY-135-01 | **Fold** — keep `{j.kind.clone()}` in the Source row |
| Fold-in AGY-135-02 | **Fold** — extract-before-checkpoint tests allow kind fallback |
| Fold-in opencode-m1 | **Fold** — prefer `pst_path`; `open_fs_path` may be None |
| Fold-in opencode-m2 | **Already covered / note** — child rows stay kind; `review.md` note |
| Fold-in AGY-135-03 | **Fold** — checkpoint lookup only for ingest / extract_pst |
