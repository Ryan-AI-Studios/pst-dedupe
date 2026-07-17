# 0015 — Matter store + audit log + blob CAS

- **Track ID:** 0015-MatterStore
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series A / §4, §2.3, §4.6, §5.2
- **Cross-repo contract:** n/a
- **Status:** Ready — not started

---

## 1. Objective

Create the **`matter-core`** library crate that owns:

1. On-disk **matter** layout and **SQLite** metadata DB.
2. **Content-addressable blob store (CAS)** for raw evidence bytes.
3. **Append-only audit log** with integrity linking.
4. **Jobs + checkpoints** schema for resumable ingest/process.
5. **Item-level error accumulator** schema for honest partial success.

This is the permanent foundation for all downstream Desk tracks.

## 2. Context (read before starting)

- Product plan: `C:\dev\Dedupe-plan.md` (architecture, phases, security, UX).
- Onboarding: `.agents/skills/onboarding/SKILL.md` (ledgerful + verification gate).
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Comparison context (optional): `C:\dev\Comparison.md`.
- Existing crates: `pst-reader`, `dedup-engine`, `pst-dedup-cli`, `pst-dedup-gui` — **do not** stuff matter persistence into those; new **`crates/matter-core`**.
- **Desktop rule:** single-process / single-exe path; no user-managed servers/daemons.
- **AI rule:** AI off by default (not in this track).
- Schema decisions here are **hard to reverse** — design for Series B–I without implementing them.

## 3. In scope

### 3.1 Crate / workspace

1. Create **`crates/matter-core`** (`cargo new --lib`), add to workspace `Cargo.toml`.
2. Dependencies (per plan §17): `rusqlite` with `bundled`, `thiserror` 2.x, `sha2` 0.11, `serde`, `camino` (and test deps as needed). No async runtime required in this crate.

### 3.2 Matter directory layout

Under a caller-chosen root (e.g. `Matters/<matter_id>/`):

```
matter.db          # SQLite (WAL recommended)
blobs/             # CAS: physical native bytes only
index/             # reserved for Tantivy (0029); create empty dir or .gitkeep
exports/           # reserved for production (0040)
logs/              # optional file logs
```

### 3.3 SQLite schema (minimum tables)

| Table | Purpose |
|---|---|
| `matters` | Matter id, name, created_at, schema_version, storage_root |
| `sources` | Imported paths (Purview package, PST, ZIP), status, cursors |
| `items` | Normalized items: ids, family_id, paths, `native_sha256`, `logical_hash` (nullable until 0017 fills), message_id, status, sizes, timestamps |
| `item_families` | Parent/child relationships (email ↔ attachments) |
| `item_errors` | **Error accumulator:** item_id nullable, source_id, stage, code, message, detail, created_at — parent items remain |
| `jobs` | Job id, kind, state (`pending`/`running`/`paused`/`failed`/`cancelled`/`succeeded`), started/finished, error summary |
| `job_checkpoints` | job_id, stage, cursor_json, completed_count, updated_at — **resume after crash** |
| `audit_events` | Append-only: seq, ts, actor, action, entity, params_json, tool_version, **prev_hash**, **entry_hash** |

Schema versioning: store `schema_version` and apply migrations in-crate (simple ordered SQL list is fine for P0).

### 3.4 CAS contract (explicit)

| Decision | Choice for 0015 |
|---|---|
| **Hash algorithm** | **SHA-256** (hex lowercase digest) — defensibility / interop with plan `native_sha256` |
| **What is hashed** | **Raw physical bytes only** as stored on disk (never normalized/logical body) |
| **Object path** | `blobs/sha256/<aa>/<fullhex>` (two-hex shard prefix) or `blobs/<fullhex>` — pick one in plan Phase 1 and document |
| **Collision policy** | If path exists and content differs → hard error (do not overwrite) |
| **Logical hash** | **Not stored in CAS.** Column on `items` only; computed later (0017/0018) |

### 3.5 Audit chain integrity (explicit)

- Rows are **append-only** (no UPDATE/DELETE of audit history in normal APIs).
- Each row stores:
  - `entry_hash` = SHA-256 over a **canonical** encoding of (seq, ts, actor, action, entity, params, tool_version, prev_hash).
  - `prev_hash` = previous row’s `entry_hash`, or a fixed genesis sentinel for seq=1.
- Provide `verify_audit_chain(conn) -> Result` that walks the chain and fails on break/tamper.
- **Out of scope for 0015:** Ed25519 signing of audit rows (optional later; hash chain is the P0 bar).

### 3.6 Jobs / checkpoints (explicit)

- API to create job, transition state, write checkpoint, load latest checkpoint by job+stage.
- Designed so 0016/0018/0019 can resume multi-GB ingest without reprocessing completed units.
- Checkpoint `cursor_json` is opaque to matter-core (owned by caller stages).

### 3.7 Item-level errors (explicit)

- `item_errors` supports honest partial success: failures are **recorded**, not silent.
- Parent `items` rows may exist with `status=partial|error` while siblings continue.
- Query helper: errors for source / job / item.

### 3.8 Tests

1. Create matter → open DB → layout exists.
2. CAS put/get round-trip; reject clobber of different bytes.
3. Job + checkpoint write/read; simulate “resume”.
4. Insert item_error without deleting parent item.
5. Audit append + chain verify; detect broken chain (mutated prev_hash in test-only helper or raw SQL).

## 4. Out of scope (do NOT do here)

- Purview ZIP expand / PST parsing (0016, 0018).
- Computing `logical_hash` from email bodies (0017/0018) — only reserve the column.
- Tantivy indexing (0029) — only reserve `index/` directory.
- Review UI / coding / redaction.
- Encryption at rest of matter.db (0057).
- Multi-tenant / multi-user.
- Always-on AI or network services.
- Destructive writes to source PST/Purview files.
- Unrelated dependency major upgrades (e.g. egui 0.35).

## 5. Preconditions & dependencies

- **P1 (blocking):** none (first Ready track in Series A).
- **P2:** `C:\dev\Dedupe-plan.md` accepted as plan-of-record.
- **P3:** `cargo check --workspace` green (or document blockers).
- *Verified to date:* Workspace exists; `sha2` 0.11 already used by `dedup-engine`; plan §17 pins `rusqlite` 0.40.x + `bundled`.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Schema too narrow for later tracks | Reserve columns/tables listed in §3.3; version migrations |
| CAS stores logical bytes by mistake | Spec: physical only; tests use raw vs normalized distinction |
| Audit “append-only” without integrity | Hash chain + verify API |
| Silent skips | `item_errors` required for error paths in later tracks; schema now |
| Crash mid-100GB | `jobs` + `job_checkpoints` in foundation |
| Scope creep into extractors | Hard out-of-scope list |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Crate:** `crates/matter-core` is a workspace member; `cargo test -p matter-core` runs.
- [ ] **DoD-2 — Layout + DB:** Creating a matter creates the directory layout and migrates schema to current version.
- [ ] **DoD-3 — CAS:** SHA-256 content-addressed put/get of **raw** bytes; no overwrite of conflicting content.
- [ ] **DoD-4 — Jobs/checkpoints:** Can create a job, write a checkpoint, reopen DB, and read it back (resume primitive).
- [ ] **DoD-5 — Item errors:** Can record item-level errors without removing parent item rows.
- [ ] **DoD-6 — Audit chain:** Append events; `verify_audit_chain` passes on valid log and fails if chain is broken in a test.
- [ ] **DoD-7 — Workspace gate:**  
      `cargo fmt --all --check`  
      `cargo clippy --workspace --all-targets -- -D warnings`  
      `cargo test --workspace` (or justify skipped packages)  
      **`ledgerful verify`** (or document exact failure + fallback per Agents.md)
- [ ] **DoD-8 — Docs:** ARCHITECTURE and/or README note `matter-core` + matter layout; plan-of-record path unchanged.
- [ ] **DoD-9 — Recorded:** `review.md` written; `../conductor.md` status → **Completed**; ledger transaction committed (`ARCHITECTURE` or `FEATURE`).

## 8. Verification commands (reference)

```powershell
# Session baseline (onboarding)
ledgerful doctor
ledgerful ledger status --compact
# optional: ai-brains context --show

# Track gate
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p matter-core
cargo test --workspace
ledgerful verify
```
