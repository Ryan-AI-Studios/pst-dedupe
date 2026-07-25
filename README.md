# PST-Dedupe

A pure Rust Windows tool for deduplicating emails across Outlook PST files.

## What It Does

- Opens one or more **Unicode PST files** (Outlook 2003+ format), including **Permute**-encrypted stores.
- Walks folders and extracts message properties.
- Detects duplicate emails with a tiered strategy:
  - **Tier 1:** `Message-ID` exact match (definitive).
  - **Tier 2:** SHA-256 content hash from subject, date, sender, body preview, and attachment metadata (fallback when Message-ID is missing).
- Produces a **CSV report** showing unique vs. duplicate messages.
- Optionally **exports unique messages as `.eml` files** (GUI path).
- Surfaces:
  - **`dedupe-desk`** — primary product shell: create/open matter, add sources, ingest/extract with live progress (track 0020)
  - **`pst-dedup` CLI** — agent- and script-friendly PST tools plus **headless matter automation** (`matter`, `job`, `profile`, `workflow`, `ingest`, `report`, `qc`, `produce`, `gap`), **opt-in multi-user matter service** (`service serve|bootstrap-admin|user`, track 0058), and **platform control plane** (`platform tenant|idp|matter`, track 0059)
  - **`pst-dedup-gui`** — egui scan/dedup wizard + **Unique PST Export** wizard (same keep-set path as CLI `unique-pst`; track 0072)

**Product modes:** **Desk solo** (default single-exe, local matter open) stays unchanged. **Matter service** is opt-in: one host process holds an exclusive OS lock on the matter, unlocks encrypted matters once, and exposes loopback HTTP (`127.0.0.1` by default; LAN needs `--allow-lan`) for concurrent reviewers with locks, OCC versions, batches, and sampling QC. **Platform SSO** is a further opt-in: `service serve --platform platform.db` plus `platform` CLI for tenants/IdP/matter registration (OIDC Auth Code + PKCE, PMK for IdP secrets, `PLATFORM_STORAGE_ROOT` sandbox). See `ARCHITECTURE.md`, `crates/matter-service/README.md`, and `crates/matter-platform/README.md`.

## Build

Requires [Rust](https://rustup.rs/) 1.80+ on Windows.

```powershell
# CLI (recommended for scripts and agents)
cargo build --release -p pst-dedup-cli

# Dedupe Desk (primary GUI)
cargo build --release -p dedupe-desk

# Legacy scan GUI
cargo build --release -p pst-dedup-gui
```

### Release Executables

| Binary | Path |
|---|---|
| Desk | `target\release\dedupe-desk.exe` |
| CLI | `target\release\pst-dedup.exe` |
| Legacy GUI | `target\release\pst-dedup-gui.exe` |

```powershell
.\target\release\dedupe-desk.exe
.\target\release\pst-dedup.exe --help
.\target\release\pst-dedup-gui.exe
```

## CLI Usage

### PST tools

```powershell
# Structure + folder counts
.\target\release\pst-dedup.exe inspect archive.pst --top 20

# Full dedup summary (machine-readable; includes scan_integrity_v1)
.\target\release\pst-dedup.exe scan archive.pst --json

# Duplicates only
.\target\release\pst-dedup.exe dups archive.pst --limit 25 --json

# CSV report (+ summary footer) and auto sidecar integrity ledger
.\target\release\pst-dedup.exe scan archive.pst --csv output\report.csv
# → also writes output\report.integrity.csv (skips + degraded rows)

# Multiple PSTs, best-effort (default) or strict
.\target\release\pst-dedup.exe scan a.pst b.pst --json --dups --limit 50
.\target\release\pst-dedup.exe scan a.pst b.pst --mode strict --json
.\target\release\pst-dedup.exe scan good.pst bad.pst --allow-failed-files --json

# Export keep-set (policy resolve + decision CSV + winners JSON; source PSTs read-only)
# Paths may be positional and/or repeated --input (merged; sorted for determinism).
.\target\release\pst-dedup.exe keep-set a.pst b.pst `
  --policy first_seen `
  --decision-csv output\decisions.csv `
  --keep-set-json output\keepset.json `
  --json
.\target\release\pst-dedup.exe keep-set --input a.pst --input b.pst `
  --policy first_seen --decision-csv output\decisions.csv --json
.\target\release\pst-dedup.exe keep-set a.pst b.pst --policy keep_largest --materialize --json
.\target\release\pst-dedup.exe keep-set archive.pst primary.pst `
  --policy prefer_path --prefer-path-contains Primary `
  --family-policy parents_only --decision-csv output\dec.csv
```

Useful flags: `--no-tier2`, `--no-attachments`, `--mode best-effort|strict`,
`--allow-failed-files`, `--integrity-csv`, `--max-skip-rate`, `--max-crc-skip-rate`,
`--max-failed-file-rate`, `--skip-limit`, `-v` / `-vv` (logs on stderr).
For quiet agent runs: `$env:RUST_LOG = 'error'`.

**Scan integrity (track 0065):** classifies recoverable vs skipped messages with stable
reason codes (`CRC_MISMATCH`, `BODY_TRUNCATED`, `ATTACH_META_FAILED`, …). Default
`--mode best-effort` keeps degraded attach/body/orphan messages with reasons; `--mode strict`
skips them and exits non-zero. Preflight recommendation (`ok` / `re_export_recommended` /
`not_export_ready`) is **guidance only** — this tool never repairs source PSTs.
**Non-zero exit still flushes** CSV/integrity/JSON artifacts first (safe for automation
and 0066 force-consume of partial recoverable sets). Empty `folder_path` alone is not
orphan; use `is_orphaned`. Intentional Tier-2 4KB body preview is **not** `BODY_TRUNCATED`.

**Keep-set export (track 0066, schema `keep_set_v1`):** single artifact for unique EML/PST/report.

| Concern | Behavior |
|---|---|
| **Policies** | `first_seen` (default), `keep_largest`, `prefer_path` — applied **after** fidelity preference |
| **Fidelity** | Non-degraded always beats degraded within a group; degraded may win only if no clean peer |
| **Determinism** | Absolute input paths are sorted before scan; ties break on `(path_key, nid)` |
| **Orchestration** | Phase 1 scan/groups → Phase 2 resolve → Phase 2b materialize+promote → Phase 3 decision stream |
| **Promotion** | Hard materialize fail promotes next peer; never ghost-drops a group when a peer exists |
| **Family** | `keep_attachments_with_parent` (default) vs `parents_only` (no attach payloads) |
| **EDRM MIH** | Optional MD5 of normalized Message-ID (interop id only — **not** a suppress tier) |
| **Outputs** | Decision CSV only **after** resolve; keep-set JSON = winners + stats (no bodies) |

**Unique EML pack (track 0067, schema `eml_pack_v1`):** keep-set winners only (no re-dedupe) →
volume-batched `.eml` directory for Outlook/Thunderbird import.

```powershell
.\target\release\pst-dedup.exe unique-eml a.pst b.pst `
  --out output\unique_eml_pack `
  --policy first_seen `
  --decision-csv output\decisions.csv `
  --keep-set-json output\keepset.json `
  --json
# Refuse non-empty --out unless --overwrite
.\target\release\pst-dedup.exe unique-eml archive.pst --out output\pack --overwrite --json
```

| Concern | Behavior |
|---|---|
| **No re-dedupe** | Same pipeline as `keep-set` (fidelity → policy → promote); always materializes |
| **Volumes** | Always `VOL001`… under `--out` (default **10 000** files/dir; `--files-per-volume`) |
| **Date** | RFC 5322 **UTC +0000 only** (host local TZ ignored) |
| **MIME** | plain / `multipart/alternative` / `multipart/mixed` + base64 attaches; embedded → `message/rfc822` |
| **MAX_PATH** | Abs path budget ≤250; subject truncated first; counter + hash kept |
| **Manifest** | `{out}/manifest.json` (`eml_pack_v1`); `eml_written == unique` on success |
| **Family** | `parents_only` omits attach/embedded MIME parts |
| **Import** | See `docs/unique-eml-import.md` — manual Outlook/Thunderbird import per volume folder |

**Unique PST export (track 0071, schema `unique_export_report_v1`):** keep-set winners → streaming unique PST (+ optional multi-volume) + report pack.

```powershell
.\target\release\pst-dedup.exe unique-pst a.pst b.pst `
  --out output\unique.pst `
  --report-dir output\unique_report `
  --policy first_seen `
  --json
# Soft multi-volume (~10 GiB physical); oversize family may exceed:
.\target\release\pst-dedup.exe unique-pst archive.pst `
  --out output\unique.pst --max-volume-bytes 10737418240 --overwrite --json
```

| Concern | Behavior |
|---|---|
| **No re-dedupe** | Same pipeline as `keep-set` / `unique-eml` (resolve + promote only) |
| **Writer** | `write_unicode_pst_streaming` only; attach streams; progress on **stderr** |
| **Volumes** | Volume 1 = `--out`; then `{stem}_vol002.pst`, …; split **between messages** |
| **Oversized family** | Soft max may be exceeded; family never severed |
| **Report pack** | `summary.json` + decisions + keepset + volumes + **mandatory `export_messages.csv`** |
| **Verify** | Open + count + sample MID; full rehash only with `--verify-hash` |
| **Partial fail** | Keep completed volumes; delete incomplete current; flush pack `ok=false` |
| **How-to** | See [`docs/unique-pst-export.md`](docs/unique-pst-export.md) |

### Headless matter automation (track 0045)

Same engine as Dedupe Desk: create a matter, import profiles/workflows, run jobs, export reports — no GUI.

```powershell
$m = "C:\Matters\cli-smoke"
.\target\release\pst-dedup.exe matter create --path $m --name "cli-smoke" --json
.\target\release\pst-dedup.exe matter info --path $m --json

# Generic job (always waits for terminal)
.\target\release\pst-dedup.exe job run --path $m --kind classify --json
# Entity / PII packs (opt-in; offline regex + Luhn; mask+hash only — not forensic-grade)
# See crates/matter-entity/README.md for honesty, FA-regex/ReDoS note, digest idempotency.
.\target\release\pst-dedup.exe job run --path $m --kind entity_scan --json
# People–comms graph (opt-in; headers primary; two-pass; BCC separate; schema v26)
# See crates/matter-people/README.md for honesty (over-merge, self-mail, not Relativity CA).
.\target\release\pst-dedup.exe job run --path $m --kind people_graph --json
# Concept / theme clustering (opt-in; schema v27; method tfidf_kmeans_v1)
# Honesty: offline TF–IDF + mandatory L2 + k-means + c-TF-IDF/ICF labels.
# Requested k is a target (actual cluster_count may be lower). Not near-dup (0023),
# not embeddings (0050), not Relativity LSI Conceptual Analytics, not privilege detection.
# See crates/matter-cluster/README.md for prep strip / caps / FilterSpec.
.\target\release\pst-dedup.exe job run --path $m --kind concept_cluster --json
# Sentiment / tone (opt-in; schema v28; method vader_lexicon_v1)
# Honesty: offline lexicon heuristic; unit-extreme + footer strip; Unscored ≠ Neutral;
# not privilege/coding; sarcasm and dilution imperfect.
# See crates/matter-sentiment/README.md for license tree + limits.
.\target\release\pst-dedup.exe job run --path $m --kind sentiment --json
# Semantic index (opt-in; schema v29; local embeddings; default mock:hash_v1)
# Additive to keyword FTS — not a replacement. Offline only; no weights in git.
# See crates/matter-semantic/README.md for pre-filter / group-before-limit / single-exe.
.\target\release\pst-dedup.exe job run --path $m --kind semantic_index --json

# AI first-pass code suggestions (off by default; enable AI on the matter first).
# Mock or local OpenAI-compatible (Ollama/LM Studio). Cloud requires allow_remote.
# Keys: PST_DEDUPE_AI_API_KEY or OS keyring — never SQLite. Suggestions ≠ final codes.
# See crates/matter-ai/README.md.
.\target\release\pst-dedup.exe job run --path $m --kind ai_suggest_codes --json
.\target\release\pst-dedup.exe job list --path $m --json
.\target\release\pst-dedup.exe job status --path $m --job-id <id> --json
.\target\release\pst-dedup.exe job cancel --path $m --job-id <id> --json
.\target\release\pst-dedup.exe job resume --path $m --job-id <id> --json

# Profiles / workflows (import custom JSON without Desk)
.\target\release\pst-dedup.exe profile list --path $m --json
.\target\release\pst-dedup.exe profile import --path $m --file .\my-profile.json --json
.\target\release\pst-dedup.exe profile run --path $m --profile builtin:standard --json

.\target\release\pst-dedup.exe workflow list --path $m --json
.\target\release\pst-dedup.exe workflow import --path $m --file .\my-workflow.json --json
.\target\release\pst-dedup.exe workflow run --path $m --workflow builtin:reduce_only_chain --json

# Convenience wrappers
.\target\release\pst-dedup.exe ingest --path $m --source C:\Data\package.zip --json
.\target\release\pst-dedup.exe report export --path $m --out C:\Matters\cli-smoke\exports\report1 --json
.\target\release\pst-dedup.exe qc run --path $m --json
.\target\release\pst-dedup.exe produce run --path $m --params-json '@C:\params\produce.json' --json
.\target\release\pst-dedup.exe gap run --path $m --params-json '{}' --json
```

**Path rules**

| Kind | Rule |
|---|---|
| CLI args (`--path`, `--file`, `--source`, `--out`, `@file`) | Relative to process **CWD**, then normalized absolute |
| Paths **inside** `--params-json` (`path`, `source_path`, `output_dir`, …) | Must be **absolute** (exit **2** if relative) |

**Stdout isolation (`--json`)**

- **stdout:** only the final JSON envelope (parseable with no preprocess)
- **stderr:** tracing, progress lines, cancel notices
- Progress never uses stdout

**Exit codes**

| Code | Meaning |
|---|---|
| **0** | Success |
| **1** | Generic / unexpected error |
| **2** | Usage / validation (bad args, bad JSON, unknown kind, relative path in params) |
| **3** | Matter busy (another job active) |
| **4** | Job finished **failed** or **cancelled** |
| **5** | Matter open/create/IO error |

**SIGINT / Ctrl+C**

1. First Ctrl+C → request cooperative cancel (no `process::exit` in the handler); wait for terminal + runner join + clean SQLite drop. Interrupted jobs often end **paused** or **cancelled** → exit **4** (`ok: false`).
2. Second Ctrl+C → force-abort request (documented last resort).

**`job cancel` vs SIGINT:** `job cancel` marks a non-terminal job **cancelled** in the matter DB (cleanup / leftover rows). In-flight work in the *current* process is stopped with **Ctrl+C** (cooperative cancel on the ProcessRunner).

**Import JSON shape** (profile/workflow): top-level `name` + either nested `body` or bare `version` + `stages`/`nodes`.

## Test

```powershell
# Full workspace gate (format, clippy, tests)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Or use Ledgerful (same steps as .ledgerful/config.toml verify.steps):
ledgerful verify
```

## Git hooks + Ledgerful (Windows)

After clone, install hooks (requires [`ledgerful`](https://github.com/Ryan-AI-Studios/Ledgerful) on `PATH`):

```powershell
# PowerShell 7+ or Windows PowerShell 5.1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install-hooks.ps1
# or: pwsh -File scripts\install-hooks.ps1
```

| Hook | What it runs |
|---|---|
| **pre-commit** | `ledgerful ledger status --compact --exit-code --verify-signatures` then `scripts\pre-commit.ps1` (fmt / clippy / test) |
| **pre-push** | Ledger status gate + `ledgerful verify --scope fast` |
| **commit-msg** / **post-commit** | Ledgerful intent sidecar + post-commit promotion |

Manual hygiene (same as pre-commit cargo steps):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\pre-commit.ps1
```

CI (GitHub Actions) runs **Windows-only** `fmt` / `clippy` / `test` on push and PRs (`.github/workflows/ci.yml`).

### PST Fixtures

Integration tests in `pst-reader` require Unicode PST files in `fixtures/`:

```powershell
cargo test -p pst-reader --test integration
```

Small Aspose/sample fixtures live under `fixtures/` (see `fixtures/README.md`). Real multi-mailbox PSTs are useful for **manual** CLI/Desk smoke on a **local path only** — never commit case evidence. Implementation track docs live under local `conductor/` (gitignored; not published with this repo).

## Architecture

| Crate | Responsibility |
|---|---|
| `pst-reader` | Pure Rust PST parser: header, NDB, LTP, messaging extraction |
| `dedup-engine` | Dedup hashing, index, CSV report, EML serialization |
| `pst-dedup-cli` | CLI surface: inspect / scan / dups (JSON + CSV) |
| `pst-dedup-gui` | egui app and background scan worker |
| `pst-writer` | Experimental/fixture PST writing and EML import helpers |
| `matter-core` | Matter layout + SQLite (schema v34: Normalized Item + dedupe/thread/neardup/cull/promote + `review_sets` + coding/`guidance` + `saved_searches` + review-list index + metadata filters + entity hits / `entity_flags` + people–comms graph + concept cluster tables + `sentiment_*` tone columns + semantic index meta / chunks + AI config / `item_ai_suggestions` + grounded `item_ai_suggestion_citations` + `transcript_*` STT bookkeeping + language packs / `fts_lang_fingerprint` / optional `language_tag` + Teams/chat columns `conversation_id` / `chat_type` / `team_name` / `channel_name` / `chat_export_format` / `conversation_bucket_date` / `teams_extract_*`) + CAS (`put_bytes` / streaming `put_reader`) + audit + jobs + logical_hash v1 + `workspace/temp/` |
| `extract-teams` | Offline Teams/chat export adapters (HTML+PST required, JSON best-effort) + resumable `teams_extract` job; plain-text bodies via ammonia; day-bucketed `conversation_id` (schema v34) |
| `ingest-purview` | Purview/package/ZIP detect + safe expand + resumable inventory (blocking worker API; `*_on_job` for runner) |
| `extract-pst` | PST → Normalized Items + families + logical_hash; `pst-native-message-v1` native (not EML); mid-folder resume (blocking; `*_on_job` for runner) |
| `process-runner` | In-process job runner: single matter worker, cancel, watch progress, Option C job-id authority |
| `matter-cull` | Flag-only data reduction: built-in + user presets, family fixpoint, `cull_*` result columns (never deletes items/CAS) |
| `matter-promote` | Flag-only promote-to-review: policies + bidirectional family expand + single-pass `review_order` (never deletes items/CAS) |
| `matter-entity` | Offline entity/PII packs (`entity_scan`): regex + Luhn, mask+hash hits only (schema v25) |
| `matter-people` | Offline people–comms graph (`people_graph`): participants + directed edges + timeline (schema v26); BCC separate; two-pass |
| `matter-cluster` | Offline concept clustering (`concept_cluster`): `tfidf_kmeans_v1` + c-TF-IDF/ICF labels (schema v27); not near-dup / not embeddings |
| `matter-sentiment` | Offline sentiment / tone (`sentiment`): `vader_lexicon_v1` + unit-extreme aggregation (schema v28); Unscored ≠ Neutral |
| `matter-semantic` | Offline semantic search (`semantic_index`): MockEmbedder default, chunk+overlap, model-namespaced store, pre-filter cosine (schema v29); additive to keyword FTS |
| `matter-ai` | Opt-in AI provider (Mock + OpenAI-compatible) + first-pass `ai_suggest_codes` with grounded citations + human promote (suggestions only; schema v31). Off by default; keys via keyring / `PST_DEDUPE_AI_API_KEY` |
| `stt-plugin` | Opt-in local speech-to-text (`transcribe`): whisper.cpp CLI sidecar + optional ffmpeg PCM coerce; mock engine for CI; schema v32 `transcript_*` bookkeeping. **Off by default** — no silent model download, no cloud STT. Un-diarized; human must listen before attribution. See [`crates/stt-plugin/README.md`](crates/stt-plugin/README.md) |

**Matter layout** (Desk foundation): `matter.db`, `blobs/sha256/<aa>/<hex>`, reserved `index/` / `exports/` / `logs/` / `semantic/`, `workspace/temp/`.
See [`crates/matter-core/README.md`](crates/matter-core/README.md), [`crates/ingest-purview/README.md`](crates/ingest-purview/README.md), [`crates/extract-pst/README.md`](crates/extract-pst/README.md), [`crates/process-runner/README.md`](crates/process-runner/README.md), [`crates/matter-entity/README.md`](crates/matter-entity/README.md), [`crates/matter-people/README.md`](crates/matter-people/README.md), [`crates/matter-semantic/README.md`](crates/matter-semantic/README.md), [`crates/matter-ai/README.md`](crates/matter-ai/README.md), [`crates/stt-plugin/README.md`](crates/stt-plugin/README.md), and [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Current Status

| Feature | Status |
|---|---|
| Unicode PST header parse | Works (including correct `bCryptMethod` alignment) |
| NDB B-tree traversal | Works |
| LTP HN / BTH / TC | Works (HNPAGEMAP `cFree`, TC RowIndex NIDs) |
| Folder/message traversal | Works (fixtures + real multi-mailbox PST) |
| NDB_CRYPT_PERMUTE | Works (verified on encrypted real PST) |
| NDB_CRYPT_CYCLIC | Implemented with unit tests |
| Tier 1 / Tier 2 dedup | Works, configurable |
| CSV report export | Works (CLI + engine) |
| CLI inspect / scan / dups | Works (`--json`, `--csv`) |
| EML export | Legacy path still available; prefer Unique PST wizard |
| Unique PST GUI | Wizard over `run_unique_pst` (cancel, log, repaint) — see `docs/unique-pst-export.md` |
| GUI scan progress | Works |
| Per-file error visibility | Works |
| ANSI PST support | Detected and rejected |
| CRC validation | Warning-only (algorithm under review) |
| Named property map | Stubbed (not needed for core dedup) |
| Large-file stress testing | Pending |

## Verification Gate (Before Commit)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

## License

**Proprietary commercial software.** All rights reserved.

This repository is **not** open source. You may not use, run, or redistribute Dedupe / Dedupe Desk for production or commercial work without a **paid commercial license** from the copyright holder (Ryan / Ryan-AI-Studios).

See [`LICENSE`](LICENSE) for the full terms (including a narrow private evaluation exception). Third-party dependencies keep their own licenses (typically permissive MIT/Apache).
