# 0016 — Purview package ingest + ZIP safety

- **Track ID:** 0016-PurviewIngest
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → §2.1–2.2, §4.6, §5.1–5.2, §5.5, Series A / **016**, §17 (`zip` 8.6)
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** **0015-MatterStore** (Completed — `matter-core` schema v1)

---

## 1. Objective

Add a library surface that **detects** Microsoft Purview-style export layouts (and simpler PST/ZIP/folder dumps), **registers** them as matter `sources`, and **safely expands** ZIP containers into the matter with:

- path-traversal / symlink rejection,
- zip-bomb resource limits,
- **resumable expand checkpoints at sub-entry (leaf) grain** — not only top-level ZIP completion (mega-zip safe),
- legacy ZIP name encoding fallbacks so non-UTF-8 evidence names are preserved,
- audit events for import start / progress boundaries / end / failure,
- honest partial success via `item_errors` (and source-level status).

This track stops at **container expand + source registration + discovered-file inventory**. It does **not** parse PST messages into Normalized Items (0018) and does **not** define the full item schema (0017).

---

## 2. Context (read before starting)

### 2.1 Plan-of-record

- Product plan: `C:\dev\Dedupe-plan.md` §§2.1–2.2 (Purview artifacts + ingest pipeline), §4.6 (resumable ingest; ZIP expand grain **tightened for this track** — see §3.5), §5.1 (path traversal / zip bombs), §5.5 (fuzz growth with ZIP), Series A track **016**, §17.2 (`zip` **8.6.x** stable — **not** 9.0-pre).
- Comparison (optional): `C:\dev\Comparison.md`.
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Sequencing: `../sequencing.md` (0016 after 0015; parallel with 0017 / 0019; unblocks **0018** with 0017).

### 2.2 What 0015 delivered (consume, do not re-implement)

| Capability | Where | Use in 0016 |
|---|---|---|
| Matter layout + `matter.db` | `Matter::create` / `open` | Target matter for import |
| Schema v1 tables | `sources`, `jobs`, `job_checkpoints`, `item_errors`, `items`, `audit_events` | Register package; job + expand stage; errors; inventory rows |
| CAS physical SHA-256 | `put_bytes` / `get_bytes` / `blob_exists` | Store expanded entry bytes (raw physical only) |
| Jobs + opaque checkpoints | `create_job`, `set_job_state`, `put_checkpoint`, `get_checkpoint` | Job kind `ingest`; stage `expand` |
| Sources | `insert_source(path, kind, status, cursor_json)` | Package / PST / ZIP / expanded child paths |
| Item errors | `record_item_error` | Per-entry expand failures without aborting whole package |
| Audit chain | `append_audit` | `ingest.*` actions |
| Schema version | `SCHEMA_VERSION == 1` | Prefer **no** migration if possible; if `update_source` needs only API, stay on v1 |

**Gaps in 0015 API that 0016 may need (small matter-core extensions, still this track):**

- `update_source` (status, cursor_json) — not present today; required for resume metadata on the source row.
- `list_sources` (optional convenience).
- `list_items_for_source` and/or lookup by `(source_id, path)` — **required for resume** so already-inventoried logical paths skip re-CAS.

Do **not** stuff ZIP parsing into `matter-core`. Prefer new crate **`crates/ingest-purview`** (name matches plan §4.4).

### 2.3 Product / desktop rules

- Single-exe / no user-managed daemons.
- **Never** mutate source Purview/PST/ZIP paths (read-only evidence).
- AI off by default (not relevant here).
- **Sync API only** in this crate (no Tokio dependency). Expand + hash of multi-GB packages is **CPU- and IO-bound**.
  - Expose cancel checks via `should_cancel: &dyn Fn() -> bool` (or equivalent) so callers can abort without an async runtime here.
  - **Caller contract (document in crate README, required for DoD-9):** `ingest_path` / `resume_ingest` **must** be invoked from a **dedicated blocking worker** (e.g. `std::thread`, `rayon`, or `tokio::task::spawn_blocking` in 0019+). Calling them on the GUI thread or a Tokio worker thread will freeze the Desk. This crate does not enforce that; 0019 owns the pool.

### 2.4 Existing crates (reuse boundaries)

| Crate | Role in 0016 |
|---|---|
| `matter-core` | Persistence, CAS, jobs, audit, errors |
| `pst-reader` | **Out of scope** for message extract — optional: detect `.pst` only by extension/header magic if cheap |
| `dedup-engine` | Not used |
| CLI/GUI | Optional thin smoke later; **not** required for DoD (library + tests first) |

---

## 3. In scope

### 3.1 Crate / workspace

1. Create **`crates/ingest-purview`** library; add to workspace `Cargo.toml`.
2. Dependencies (plan §17):
   - `matter-core` (path)
   - `zip` **8.6** with **trimmed features** (`default-features = false`; enable only what fixtures need — at least `deflate`; add `bzip2` / `aes-crypto` only if required by real Purview samples)
   - Encoding helpers as needed for ZIP name fallbacks (e.g. small CP437 / codepage decode; prefer well-known crates over ad-hoc tables if available)
   - `camino`, `thiserror`, `serde` / `serde_json`, `sha2` (via matter-core CAS preferred)
   - `tempfile` for tests
3. No Tokio requirement in this crate (sync API; cancel hook optional).

### 3.2 Package detector

Classify a user-selected path into a **package kind** (string constants, stable for audit/UI):

| Kind | Heuristic (minimum) |
|---|---|
| `single_pst` | File ends with `.pst` (case-insensitive) and/or MS-PST magic when cheap |
| `single_zip` | File ends with `.zip` / is a ZIP local-file header |
| `purview_package` | Directory (or expanded root) matching **heuristic** Purview-ish layout: presence of `.pst` and/or nested zips plus common export noise (e.g. `*.csv`/`*.xml` reports, `Exchange`/`SharePoint`-like subtrees). **Document heuristics**; do not require Microsoft-private schema. |
| `raw_dump` | Directory of mixed files without strong Purview signals |
| `unsupported` | Empty path, unreadable, or clearly not an import target |

Detector is **best-effort** and must never invent structure. Prefer over-classifying as `raw_dump` over false `purview_package` if ambiguous.

### 3.3 Safe ZIP expand

Implement a hardened expand path used for top-level and nested ZIPs:

| Guard | Requirement |
|---|---|
| Path traversal | Reject `..` segments, absolute paths (`/`, `C:\…`), drive-relative oddities **after** name decoding (see §3.3.1) |
| Symlinks / special | Do not create or follow symlinks; reject ZIP symlink/external-link flags where the crate exposes them |
| Zip bomb — uncompressed size | Configurable max **total** uncompressed bytes per job (default: e.g. 50 GiB or env/test override) |
| Zip bomb — ratio | Configurable max compression ratio (uncompressed/compressed) per entry and/or package |
| Zip bomb — entry count | Configurable max entries per archive / job |
| Streaming | Prefer streaming extract to CAS or bounded buffer; **do not** load entire multi-GB entry into a single `Vec` unless size is known and under a small threshold |
| Nested ZIP | Recurse with same guards; depth cap (default e.g. 8) |
| Nested PST | **Discover and register** as child source or inventory row; **do not** open with `pst-reader` here |
| 7z | **Out of scope for expand in 0016** — detect `.7z` and record `item_errors` / source note `unsupported_container`; plan allows 7z later |

**Collision / destination:** Expanded physical bytes go into **matter CAS** (`native_sha256` = CAS digest). Logical path relative to package root is stored on inventory (see §3.6). Never write expanded trees into the **source** package directory.

#### 3.3.1 ZIP entry name encoding (evidence preservation)

ZIP entry names are **not** reliably UTF-8. Historical archives and many Windows tools use CP437, Windows-1252, Shift-JIS, etc. Dropping or hard-failing a whole package solely because a name is non-UTF-8 is **not** acceptable for eDiscovery.

**Required decode policy for `path_safety` / expand:**

1. If the ZIP general-purpose bit 11 (UTF-8) is set **or** the name bytes are valid UTF-8 → decode as UTF-8.
2. Else try **CP437** (ZIP historical default for non-UTF-8 names).
3. Else try **Windows-1252** / Latin-1 style single-byte mapping so every byte becomes a Unicode scalar (lossy but bijective for 0x00–0xFF where applicable).
4. Result is always stored as a **UTF-8 logical path** in SQLite / inventory (`path` column).
5. **After** decoding, run the same traversal/absolute/symlink rejection on the Unicode path.
6. Record which decode path was used only if useful for debug (optional field in error detail); do **not** require perfect original-codepage round-trip for DoD.
7. Reject only unsafe paths and bomb limits — **not** “unknown encoding.”

Unit tests must cover at least one non-UTF-8 name that still expands successfully after fallback.

### 3.4 Source registration + job lifecycle

For each import invocation:

1. Open existing matter (`Matter::open`).
2. `insert_source` for the user path with detected `kind` and `status=importing` (or `pending` then `importing`).
3. `create_job(kind: "ingest")` → `set_job_state(Running)`.
4. Run detect → expand pipeline.
5. On success: source `status=ready` (or `imported`); job `Succeeded`.
6. On fatal failure: source `status=failed`; job `Failed` + `error_summary`; partial inventory retained.
7. On cancel (if cancel hook fires): source `status=paused` or `failed` with clear code; job `Cancelled` or `Paused` per matter-core states; **checkpoint must allow resume**.

### 3.5 Expand checkpoints (resume) — mega-zip safe

Plan-of-record §4.6 lists a minimum of “each top-level entry fully extracted.” That grain is **insufficient** for Purview: a single `export.zip` is often **50–100 GB with millions of nested files**. Crashing at 99% of one top-level archive must **not** force a full re-extract.

**This track’s required grain (stricter than the plan minimum):**

| Stage name | Grain |
|---|---|
| `expand` | After each **successfully extracted leaf entry** (file put to CAS + inventory row committed), **or** on a configurable cadence: every **N entries** and/or every **X uncompressed bytes**, whichever comes first — still recording the last successful logical path |

Top-level package members (e.g. `mail.pst`, `export.zip`) may still be marked complete when finished, but **sub-entry progress inside a mega-ZIP is mandatory**.

`cursor_json` (owned by ingest-purview, opaque to matter-core) must be enough to resume, e.g.:

```json
{
  "source_id": "src_…",
  "package_root": "C:\\exports\\case1",
  "archive_stack": ["export.zip", "inner/files.zip"],
  "last_successfully_extracted_logical_path": "export.zip!/custodian_a/mail/Inbox/msg001.eml",
  "completed_count": 128490,
  "bytes_extracted": 48102912000,
  "completed_top_level": ["manifest.csv"],
  "nested_depth_max_seen": 2,
  "limits": { "max_uncompressed_bytes": …, "checkpoint_every_n_entries": 50, "checkpoint_every_bytes": 67108864 }
}
```

**Resume rules (required):**

1. Load job checkpoint stage `expand` (source of truth). Optionally mirror to source `cursor_json` via `update_source`.
2. For each candidate leaf logical path: if inventory already has `(source_id, path)` with `native_sha256` present and `status` in (`expanded`, `discovered` complete) → **skip** re-read/re-CAS (idempotent).
3. Prefer also trusting `last_successfully_extracted_logical_path` as a fast-forward hint **within the current archive walk**, but **inventory + digest is authoritative** (survives partial checkpoint flush races).
4. Re-open the same source path read-only; never rewrite the user’s package.
5. Checkpoint cadence defaults must balance durability vs SQLite write overhead (document in README; tests use tiny N).

**DoD test:** interrupt mid-archive after at least one **inner** entry is committed (not merely after finishing a whole top-level ZIP); resume must not re-`put_bytes` for already inventoried paths.

### 3.6 Discovered-file inventory (minimal items)

0017 owns the full Normalized Item model. For 0016, record a **minimal inventory** so 0018 can find work **and** resume can skip:

- Prefer `Matter::insert_item` with:
  - `source_id` set
  - `path` = package-relative logical path (**UTF-8 after encoding fallback**, §3.3.1)
  - `native_sha256` = CAS digest when bytes were stored
  - `status` = `discovered` | `expanded` | `error`
  - `size_bytes` when known
  - `logical_hash` / `message_id` left **null**
- Logical path uniqueness for resume: treat `(source_id, path)` as the skip key (document; if schema lacks unique index, enforce in application logic for 0016).
- For expand failures: `record_item_error` with `stage="expand"`, structured `code` (e.g. `zip_path_traversal`, `zip_bomb_ratio`, `io_error`, `unsupported_7z`).

Do **not** invent family graphs, MIME taxonomy, or email fields.

### 3.7 Audit events

Append (via matter-core) at least:

| Action | When |
|---|---|
| `ingest.start` | Job begins; params: path, detected kind, limits |
| `ingest.source` | Source row created |
| `ingest.expand.entry` | Optional (may be high volume) — prefer sampling or only failures; **required** summary counts at end |
| `ingest.checkpoint` | Optional if frequent; final checkpoint params ok |
| `ingest.complete` | Success; counts: entries, bytes, errors, nested_zips, pst_found |
| `ingest.fail` | Fatal failure; code + message |

`tool_version` = ingest-purview (or workspace) package version.

### 3.8 Safety tests + fuzz/property tests

1. **Unit / integration (required):**
   - Happy path: synthetic folder + ZIP with nested ZIP + dummy `.pst` file → sources/items/CAS/audit/job succeed.
   - Path traversal entry rejected; no file outside extract root/CAS.
   - Absolute path entry rejected.
   - Non-UTF-8 entry name (e.g. CP437 or Windows-1252 bytes) expands and inventory path is valid UTF-8.
   - Zip-bomb limit trips (crafted small archive with absurd uncompressed size / ratio / entry count).
   - **Resume mid-mega-zip:** interrupt after an **inner** leaf is committed → second run skips that path’s CAS put.
   - Corrupt ZIP → structured error + job/source failed or partial with `item_errors`.
2. **Property or fuzz (required for DoD, plan §5.5):**
   - At least one of:
     - `cargo-fuzz` target for path sanitizer / entry-name normalizer, **or**
     - `proptest`/`quickcheck`-style property tests over random path strings and zip name edge cases.
   - Goal: no panics; traversal always rejected after decode.

### 3.9 Docs

- Short `crates/ingest-purview/README.md` covering:
  - detector kinds, limits, resume grain, encoding fallbacks, out of scope
  - **blocking-thread caller warning** (must not run on UI/async executor — §2.3)
- Root `ARCHITECTURE.md` / `README.md`: one-line module map entry for `ingest-purview`.
- This track’s `review.md` on completion.

### 3.10 Optional (nice-to-have, not DoD)

- CLI subcommand on `pst-dedup-cli` like `ingest <matter> <path>` for manual smoke (still run via blocking path).
- Purview manifest CSV/XML **parse** — detect presence only; full manifest mapping can wait.

---

## 4. Out of scope (do NOT do here)

| Deferred to | Work |
|---|---|
| **0017** | Full Normalized Item fields, family graph, logical_hash policy |
| **0018** | PST open via `pst-reader`, message/attachment extraction |
| **0019** | Generic process runner, progress channels, UI job orchestration, **spawn_blocking / rayon pool** |
| **0020** | Desk shell “Add source” UX |
| **0033+** | Office/PDF extractors |
| **0055** | Teams chat adapters |
| — | 7z expand, AV virus-scan hooks, encryption of matter at rest |
| — | Mutating source exports; uploading to cloud |
| — | Always-on AI |
| — | Unrelated dep majors (egui 0.35, zip 9-pre) |

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0015-MatterStore** Completed — `crates/matter-core` present; `cargo test -p matter-core` green; review in `../0015-MatterStore/review.md`.
- **P2:** Plan-of-record `C:\dev\Dedupe-plan.md` accepted.
- **P3:** Workspace builds after 0015 land (or this track includes any uncommitted 0015 if not yet on `main`).
- *Verified from 0015 review:*
  - CAS path `blobs/sha256/<aa>/<hex>`
  - Job states: pending/running/paused/failed/cancelled/succeeded
  - Checkpoint opaque `cursor_json`
  - `insert_source` exists; **`update_source` does not** — add if needed
  - Schema version **1**

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Zip crate feature surface / MSRV | Pin 8.6; trim features; document MSRV if raised |
| Disk double-write (source + CAS) | Stream to CAS; no full second tree under matter except reserved dirs |
| False Purview detection | Document heuristics; prefer `raw_dump` when unsure |
| Silent skip of bad entries | `item_errors` + counts in `ingest.complete` |
| **Mega-zip re-extract after crash** | **Sub-entry checkpoints + inventory skip by `(source_id, path)` + digest** |
| Checkpoint write amplification | Configurable N-entries / X-bytes cadence; test with small N |
| Non-UTF-8 ZIP names drop evidence | CP437 → Win-1252/Latin-1 fallback; never fail solely on encoding |
| GUI/async freeze | Document blocking-thread caller contract; 0019 owns pool |
| Scope creep into PST parse | Hard boundary: register `.pst` only |
| Hostile ZIP crashes process | Limits + fail closed + property/fuzz tests |
| matter-core API gaps | Minimal extensions (`update_source`, path lookup); no ZIP logic in matter-core |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Crate:** `crates/ingest-purview` is a workspace member; `cargo test -p ingest-purview` runs.
- [ ] **DoD-2 — Detect:** Public API classifies at least: single PST, single ZIP, directory package (`purview_package` or `raw_dump`), unsupported.
- [ ] **DoD-3 — Safe expand:** ZIP expand rejects traversal/absolute paths; enforces configurable size/ratio/entry limits; nested ZIP recursion with depth cap; `.7z` not expanded (explicit unsupported path); **non-UTF-8 names preserved via encoding fallback**.
- [ ] **DoD-4 — Matter integration:** Import registers `sources`, creates `ingest` job, stores expanded bytes in CAS, writes minimal inventory `items`, uses `item_errors` for per-entry failures.
- [ ] **DoD-5 — Resume (mega-zip grain):** Expand stage checkpoints at **leaf/sub-entry** cadence (not only top-level ZIP complete); integration test interrupts **mid-archive** and resume **skips** already inventoried paths (no redundant CAS put).
- [ ] **DoD-6 — Audit:** `ingest.start` + `ingest.complete` or `ingest.fail` present; chain still verifies via `Matter::verify_audit_chain`.
- [ ] **DoD-7 — Hostile-input tests:** Automated tests for happy path + traversal + bomb limit + corrupt zip + encoding fallback; plus property or fuzz tests for path safety.
- [ ] **DoD-8 — Workspace gate:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, relevant tests pass, and **`ledgerful verify`** (hard requirement; not optional).
- [ ] **DoD-9 — Docs + recorded:** crate README includes **blocking-thread warning** + limits/resume/encoding notes; ARCHITECTURE/README note; `review.md` written; `../conductor.md` → **Completed**; ledger transaction committed (`FEATURE` or `SECURITY`).

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p matter-core
cargo test -p ingest-purview
cargo test --workspace
# If cargo-fuzz target added (optional CI later):
# cargo fuzz run zip_path_safety -- -max_total_time=30
ledgerful verify
```

### Suggested public API sketch (implementer may refine names)

```rust
// crates/ingest-purview
pub enum PackageKind { SinglePst, SingleZip, PurviewPackage, RawDump, Unsupported }

pub struct ExpandLimits {
    pub max_uncompressed_bytes: u64,
    pub max_compression_ratio: f64,
    pub max_entries: u64,
    pub max_zip_depth: u32,
    /// Durability vs write amplification (mega-zip).
    pub checkpoint_every_n_entries: u64,
    pub checkpoint_every_bytes: u64,
}

pub struct DetectResult { pub kind: PackageKind, /* notes */ }

pub struct IngestSummary {
    pub source_id: String,
    pub job_id: String,
    pub kind: PackageKind,
    pub entries_ok: u64,
    pub entries_err: u64,
    pub bytes_cas: u64,
    pub psts_found: u64,
}

pub fn detect(path: &Utf8Path) -> Result<DetectResult>;

/// Blocking / CPU+IO heavy. Callers MUST run on a worker thread (see crate README).
pub fn ingest_path(
    matter: &Matter,
    path: &Utf8Path,
    limits: &ExpandLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<IngestSummary>;

/// Same threading contract as `ingest_path`.
pub fn resume_ingest(
    matter: &Matter,
    source_id: &str,
    job_id: &str,
    limits: &ExpandLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<IngestSummary>;
```

---

## 9. Acceptance narrative (product)

An operator (or test) can:

1. Create a matter with `matter-core`.
2. Point `ingest-purview` at a **synthetic** Purview-like folder containing nested ZIPs and a dummy PST (from a **blocking** worker).
3. Observe CAS blobs + `sources` + inventory items + audit + succeeded job.
4. Kill **mid-expand of a large/nested ZIP** after an inner leaf is committed; resume without redoing that leaf’s CAS put.
5. Expand an archive whose entry names are legacy-encoded (non-UTF-8); inventory still records UTF-8 logical paths.
6. Feed a malicious ZIP (traversal / bomb); expand fails closed with recorded errors — **source evidence untouched**.
