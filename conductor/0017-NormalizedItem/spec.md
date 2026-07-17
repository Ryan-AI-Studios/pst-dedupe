# 0017 — Normalized Item model + family graph

- **Track ID:** 0017-NormalizedItem
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → §2.2 (Normalized Item fields), **§2.3** (physical vs logical hash), Series A / **017**, §4.4 (`matter-core` owns items)
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** **0015-MatterStore** (Completed). **0016-PurviewIngest** Completed is **recommended** (inventory row shape + path conventions) but not strictly required to implement types/hash in isolation.

---

## 1. Objective

Define and persist the **canonical Normalized Item** model used by all downstream Desk work:

1. **Schema + APIs** for reviewable items beyond 0016’s minimal inventory rows.
2. **Family graph** (parent email ↔ attachment children) via `item_families` + item links.
3. **Physical vs logical identity** per plan §2.3:
   - `native_sha256` = CAS digest of **raw** stored bytes (already owned by 0015/0016).
   - `logical_hash` = SHA-256 of a **versioned, canonical preimage** of normalized review content (**not** raw MIME/PST property bags / `Received:` chains).
4. Pure **logical-hash + field-normalization** helpers that **0018** (PST adapter) and later extractors call.

This track is **model + persistence + pure compute**. It does **not** open PSTs, expand ZIPs, or run full-matter process jobs.

---

## 2. Context (read before starting)

### 2.1 Plan-of-record

- `C:\dev\Dedupe-plan.md` §2.2 Normalized Item field list; **§2.3** logical hash inputs (email + non-email).
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Sequencing: `../sequencing.md` — 0017 after 0015; parallel with 0016/0019; **blocks 0018** (with 0016).
- Comparison (optional): `C:\dev\Comparison.md`.

### 2.2 What 0015 delivered (foundation)

| Surface | Relevance to 0017 |
|---|---|
| `items` table | Thin columns: id, source_id, family_id, path, native_sha256, **logical_hash (nullable)**, message_id, status, size_bytes, timestamps |
| `item_families` table | Exists; **no public API** yet |
| CAS | Physical bytes only — never store logical preimage in CAS as “native” |
| Schema | **v1** (`SCHEMA_VERSION == 1`) |

### 2.3 What 0016 delivered (consume / stay compatible)

| Surface | Relevance to 0017 |
|---|---|
| Inventory rows | `path` = package-relative UTF-8 logical path (incl. `archive!/inner` nested style); `native_sha256` set for expanded leaves; `status` ∈ `discovered` \| `expanded` \| `error`; `logical_hash` / `message_id` null |
| Skip key | Resume uses `(source_id, path)` via `item_by_source_path` (app-level; no unique index yet) |
| APIs | `update_source`, `item_by_source_path`, `list_items_for_source` |
| Crate | `ingest-purview` — **do not** fold Normalized Item fields into expand; inventory stays minimal at expand time |

**Compatibility rule:** Existing 0016 rows must remain readable after migration. New columns nullable (or defaulted). Status vocabulary may **extend** (`normalized`, `extracted`, …) without invalidating `expanded`/`discovered`.

### 2.4 Existing crates (boundaries)

| Crate | Role in 0017 |
|---|---|
| **`matter-core`** | Schema migration, item/family CRUD, queries; optional module for logical-hash pure functions **or** thin re-export |
| `ingest-purview` | Unchanged behavior required; recompile only if `Item`/`ItemInput` fields grow |
| `dedup-engine` | **Do not replace** CLI Tier-1/2 scan hashing in this track. Document relationship: Desk `logical_hash` (§2.3) is the matter identity for 0021+; `dedup-engine` content hash remains CLI/legacy until a later unify track |
| `pst-reader` | **Out of scope** (0018) |
| CLI/GUI | Not required for DoD |

### 2.5 Desktop rules

- Single-exe / no daemons.
- Never mutate source PST/Purview evidence.
- Pure hash helpers are CPU-light for unit tests; bulk apply over millions of items is **0018/0019** work (blocking pool). Document that bulk rehash jobs are not this track’s responsibility.

---

## 3. In scope

### 3.1 Ownership decision

| Concern | Home |
|---|---|
| SQLite columns + migrations | **`matter-core`** (bump `SCHEMA_VERSION` → **2**) |
| Family create / list / link | **`matter-core`** |
| `update_item` / upsert normalized fields | **`matter-core`** |
| Logical-hash preimage + SHA-256 | **`matter-core`** module `logical_hash` (preferred) so 0018 depends only on matter-core types — **or** `crates/normalize-core` only if matter-core would become oversized; default **in-crate module** |
| Large body text storage | **CAS** via digest column (`text_sha256`); do not force multi-MB bodies into SQLite TEXT for P0 |

### 3.2 Schema v2 — Normalized Item fields (P0)

Extend `items` (and use `item_families`) to cover the **P0 subset** of plan §2.2. Full Nuix-scale columns are not required if deferred fields are listed in §4.

#### 3.2.1 Required columns / fields (new or existing)

| Field | Storage | Notes |
|---|---|---|
| `id` | existing | Stable item id |
| `matter_id` | existing | |
| `source_id` | existing | Nullable only for synthetic tests |
| `family_id` | existing | FK → `item_families` |
| `path` | existing | Ingest logical path; PST extractors may set synthetic paths (document convention in 0018) |
| `native_sha256` | existing | Physical CAS digest |
| `logical_hash` | existing | Hex lowercase SHA-256 of logical preimage; null until computed |
| `message_id` | existing | Normalized RFC Message-ID when known |
| `status` | existing | Processing/lifecycle string (see §3.5) |
| `size_bytes` | existing | Native/blob size when known |
| `created_at` / `modified_at` / `imported_at` | existing | Prefer RFC3339 / UTC |
| **`role`** | **new** | `standalone` \| `parent` \| `attachment` (or `child`) |
| **`parent_item_id`** | **new** | Optional denorm FK to parent item for O(1) attachment walks; must match family membership when set |
| **`mime_type`** | **new** | Best-effort IANA or `application/octet-stream` |
| **`file_category`** | **new** | Coarse: `email` \| `attachment` \| `office` \| `pdf` \| `calendar` \| `chat` \| `media` \| `container` \| `other` (stable strings) |
| **`custodian`** | **new** | Nullable string; may stay null until ingest/UI fills |
| **`subject`** / **`title`** | **new** | Email subject vs non-email title (either/both nullable) |
| **`from_addr`** | **new** | Single from/author email or display |
| **`to_addrs_json`** | **new** | JSON array of strings (see §3.2.3 storage decision) |
| **`cc_addrs_json`** | **new** | JSON array |
| **`bcc_addrs_json`** | **new** | JSON array — **required to preserve BCC for review**; also required in logical_hash preimage (§3.4.3) |
| **`author`** | **new** | Non-email author |
| **`sent_at`** / **`received_at`** | **new** | RFC3339 UTC preferred |
| **`attachment_count`** | **new** | Direct children count |
| **`text_sha256`** | **new** | CAS digest of normalized body/primary text bytes (UTF-8), nullable |
| **`html_sha256`** | **new** | Optional CAS of HTML body |
| **`logical_hash_version`** | **new** | INTEGER NOT NULL DEFAULT 0; set when hash computed (see §3.4) |
| **`extra_json`** | **new** | Optional escape hatch for extractor-specific props without schema churn |

**Indexes (v2):**

- `idx_items_logical_hash` on `logical_hash` (dedupe later)
- `idx_items_message_id` on `message_id`
- Optional unique index on `(source_id, path)` where both non-null — **recommended** to harden 0016 resume; if SQLite null uniqueness is awkward, document and keep app-level skip

#### 3.2.3 Address storage: JSON columns (architectural decision)

**P0 decision: keep `to_addrs_json` / `cc_addrs_json` / `bcc_addrs_json` on `items`.**

| Concern | Decision |
|---|---|
| Ingest / extract write path | JSON arrays are fast and match extractor output shape |
| Free-text / fielded participant search | **Tantivy (0029)** is plan-of-record primary FTS — not SQLite JSON1 |
| Case overview / comms graphs (0038, 0047) | **Out of scope for 0017.** If SQLite-native “who was on CC” becomes a product requirement, add a future `item_participants` (or similar) many-to-many table in those tracks — **do not block 0017** on it |
| SQLite JSON1 | Acceptable only for occasional admin/debug queries; **not** the scale path for participant analytics |

Document this in `matter-core` README so later tracks do not assume relational participant queries exist.

#### 3.2.4 Migration mechanics (SQLite constraints)

SQLite `ALTER TABLE` can **ADD COLUMN** and create **new indexes** easily. It does **not** fully support arbitrary “add FK / rebuild constraints” without the [table-rebuild procedure](https://www.sqlite.org/lang_altertable.html).

**Implementer rules for v1 → v2:**

1. Prefer **nullable `ADD COLUMN`** for all new item fields (no new NOT NULL without defaults).
2. `family_id` FK already exists from v1 `CREATE TABLE` — **do not** re-add it via ALTER.
3. `parent_item_id` may be a plain nullable TEXT column in v2 **without** a declared FK if ALTER cannot attach one cleanly; enforce parent existence in **application API** (and tests). Optional later migration can rebuild the table for a formal FK.
4. New indexes: `CREATE INDEX IF NOT EXISTS …` after columns exist.
5. **Test with live-shaped v1 data:** create schema-v1 matter (or open a DB stopped at v1), insert 0016-style inventory rows, run migrate → assert columns, data intact, FKs still work for existing `family_id` / `source_id`.
6. If any required change needs table rebuild, implement the full SQLite 12-step dance inside `migrate()` with a transaction; document in `review.md`. Do **not** assume naive `ALTER TABLE … ADD CONSTRAINT`.

#### 3.2.5 `item_families`

| Field | Notes |
|---|---|
| `id`, `matter_id`, `kind`, `created_at` | existing |
| `kind` | e.g. `email_attachments`, `generic` |

APIs must create families and list members by `family_id`.

### 3.3 Family graph semantics

1. **Create family** → `ItemFamily` row (`kind` required or default `email_attachments`).
2. **Assign** items: set `family_id`; parent has `role=parent`; attachments `role=attachment` + `parent_item_id`.
3. **Standalone** items: `family_id` null, `role=standalone` (0016 expanded files default until classified).
4. **Invariant:** All members of a family share the same `matter_id`. Parent and children share `family_id`.
5. **Queries:** `list_family_members(family_id)`, `list_attachments(parent_item_id)`, `get_parent(child_id)`.
6. **No cascading delete** of CAS blobs on family changes in this track (orphan blobs ok; GC later).

### 3.4 Logical hash (plan §2.3) — required algorithm

#### 3.4.1 Versioning

- Constant `LOGICAL_HASH_VERSION: u32 = 1`.
- Preimage **must** include the version so future algorithm changes do not silently collide.
- Stored `logical_hash` is **lowercase hex** SHA-256 of the preimage bytes.
- Stored `logical_hash_version` matches the algorithm used.

#### 3.4.2 Preimage framing (no boundary ambiguity) — **required**

Naive `LF`-joined `label:value` streams are **not** acceptable for variable-length body/text fields: a body containing the bytes `\nattachments:\n…` could create structural ambiguity for any consumer that re-parses the preimage, and makes the format hard to specify.

**v1 framing (required):** stream **length-prefixed UTF-8 fields** into the SHA-256 hasher (no dependency on bincode/protobuf). Recommended wire shape:

```text
For each field in fixed order:
  field_tag: u8 or short ASCII tag + 0x1e separator (document exact tags)
  length: u64 little-endian byte count of payload
  payload: exactly `length` bytes (may contain any bytes including LF)
```

Simpler equivalent that is also acceptable if fully documented and tested:

```text
v1\n
message_id\n<len>\n<bytes>\n
subject\n<len>\n<bytes>\n
from\n<len>\n<bytes>\n
to\n<len>\n<bytes>\n
cc\n<len>\n<bytes>\n
bcc\n<len>\n<bytes>\n
sent\n<len>\n<bytes>\n
received\n<len>\n<bytes>\n
body\n<len>\n<bytes>\n
attachments\n<count>\n
  for each attachment in sort order:
    filename\n<len>\n<bytes>\n
    size\n<decimal ascii>\n
    native_sha256\n<len>\n<bytes>\n
```

**Rules:**

1. **Never** embed unbounded body text as a single unlabeled line after a colon without a length.
2. Hash the **exact preimage bytes** from a pure builder function; unit tests may assert hex for golden inputs and may assert preimage length framing with embedded `\nattachments:` inside body does **not** change attachment list contribution.
3. Prefer **no new ser/de crates** for v1; hand-rolled length-prefix is enough. Bincode/protobuf allowed only if justified in review (usually unnecessary).

#### 3.4.3 Email fields in preimage (v1) — **BCC required**

Fixed field order into the framed preimage:

| Order | Field | Content |
|---|---|---|
| 1 | version | `1` |
| 2 | message_id | normalized or empty |
| 3 | subject | strict subject (collapse whitespace; **keep** RE:/FW:) |
| 4 | from | case-folded addr |
| 5 | to | sorted case-folded addrs (stable join, e.g. `\n` or `,` **inside** the length-prefixed payload) |
| 6 | cc | sorted case-folded addrs |
| 7 | **bcc** | **sorted case-folded addrs — required** |
| 8 | sent | UTC second-resolution RFC3339 or empty |
| 9 | received | UTC second-resolution RFC3339 or empty |
| 10 | body | normalized body text |
| 11 | attachments | sorted list of `(filename_lower, size, native_sha256)` |

**BCC policy (defensibility):**

- A message **with** BCC recipients and an otherwise identical copy **without** those BCC addresses **must produce different `logical_hash` values**.
- Collapsing BCC into To/Cc or omitting BCC causes **silent loss of BCC-only recipients** when dedupe suppresses “duplicates” — **unacceptable** for eDiscovery.
- Empty BCC is a zero-length (or empty list) payload, not “omit the field.”

Normalization rules (must match tests):

| Input | Rule |
|---|---|
| Message-ID | Trim; strip `<>`; lowercase (align with `dedup-engine::normalize_message_id` if practical — **share or duplicate with test parity**) |
| Subject (strict) | Unicode trim; collapse runs of whitespace to single space; **keep** `RE:`/`FW:` prefixes |
| Addresses (To/Cc/**Bcc**) | Trim; lowercase domain (and local-part lowercasing for v1 simplicity unless documented otherwise); **sort each list independently** |
| Times | Convert to UTC; round/truncate to **second** |
| Body | If HTML only: convert to text (minimal strip tags for v1 is ok if documented); CRLF→LF; remove zero-width chars; trim trailing whitespace per line or whole string (document); do **not** include `Received:` chains |
| Attachments | Direct children only: sorted `(filename_lower, size, native_sha256)` |

**Forbidden in preimage:** raw PST bags, full MIME with transport headers, CAS path strings, matter ids, source paths (identity is content, not location).

#### 3.4.4 Non-email preimage (v1)

Same **length-prefixed** framing as email. Field order:

| Order | Field |
|---|---|
| 1 | version `1` |
| 2 | category (`file_category`) |
| 3 | title |
| 4 | author |
| 5 | created (UTC second or empty) |
| 6 | text (normalized primary text) |
| 7 | children: sorted child `native_sha256` list |

#### 3.4.5 API sketch (pure)

```rust
pub struct EmailLogicalInput {
    // includes bcc: Vec<String> (or equivalent) — required field, may be empty
    /* message_id, subject, from, to, cc, bcc, sent, received, body, attachments */
}
pub struct NonEmailLogicalInput { /* ... */ }

pub fn compute_email_logical_hash(input: &EmailLogicalInput) -> String; // hex
pub fn compute_non_email_logical_hash(input: &NonEmailLogicalInput) -> String;
pub fn normalize_message_id(mid: &str) -> String;
// body/subject/addr helpers as needed
```

Callers (0018) supply already-extracted fields + child digests; **0017 does not parse EML/PST**.

#### 3.4.6 Relationship to `dedup-engine` Tier 2

| | Tier 2 content hash (`dedup-engine`) | Desk `logical_hash` v1 |
|---|---|---|
| Body | Preview-oriented normalization | Full normalized body text (via `text_sha256` content) |
| Attachments | `name:size` only | `name|size|native_sha256` |
| Subject | lowercased aggressively | Strict (keep RE/FW) |
| Use | CLI scan today | Matter dedupe / promote (0021+) |

Document in README; **do not** silently rename Tier 2 as logical_hash.

### 3.5 Status vocabulary (extend, document)

Recommended stable strings (document as constants):

| Status | Meaning |
|---|---|
| `discovered` / `expanded` | 0016 inventory only |
| `error` | Failed processing unit |
| `normalized` | Fields + logical_hash written without full extractor pipeline |
| `extracted` | Filled by extractor (0018+) |
| `partial` | Some fields present; errors recorded on `item_errors` |

0017 APIs must accept these without hardcoding only one path.

### 3.6 Matter APIs to add/extend

Minimum public surface:

| API | Purpose |
|---|---|
| `insert_family(kind) -> ItemFamily` | Create family |
| `get_family` / `list_family_members` | Read graph |
| `insert_item` / `update_item` | Accept expanded `ItemInput` (all new fields optional) |
| `set_item_family_role(...)` | Link parent/child + roles |
| `item_by_source_path` | Keep (0016) |
| `list_items_for_source` | Keep |
| Optional: `items_by_logical_hash`, `items_by_message_id` | Prep 0021 |

Audit (lightweight):

- `item.update` / `family.create` optional if high volume — at least audit when **batch normalize** helpers run; for pure unit hash APIs, audit N/A.
- Prefer: mutating APIs that change durable matter state append audit when cheap; document if per-item update is silent to avoid log explosion (0018 may batch audit).

### 3.7 Tests (required)

1. **Migration:** open v1 matter DB (or create v1 fixture) → migrate to v2; 0016-shaped rows still load.
2. **Family:** create parent + two attachments; list members; parent_item_id consistency.
3. **Logical hash stability:** same `EmailLogicalInput` → same hex; field order independence for sorted To/Cc/**Bcc**/attachments.
4. **Logical hash sensitivity:** change body or attachment digest → different hash.
5. **BCC distinctness:** identical inputs except one has a BCC recipient → **different** `logical_hash` (defensibility).
6. **Preimage framing:** body containing the ASCII substring that looks like an attachments section (e.g. `\nattachments:\nfake.pdf|1|abc`) must **not** change attachment contribution; only the structured attachment list does.
7. **Strict subject:** `RE: Hello` vs `Hello` → **different** logical_hash (RE not stripped).
8. **Message-ID normalize:** `<A@B.com>` vs `a@b.com` → same mid component.
9. **Transport independence:** two inputs with same logical fields but different “would-be MIME wrapper” notes (comments only in test) still match — proves we hash logical fields only.
10. **Non-email** hash smoke.
11. **Native vs logical:** same logical fields, different message `native_sha256` still same `logical_hash`.
12. **Migration:** v1 inventory rows survive v2 migrate (see §3.2.4).
13. `cargo test -p matter-core` green; workspace gate + **`ledgerful verify`**.

### 3.8 Docs

- Update `crates/matter-core/README.md`: schema v2 field table, family semantics, logical hash v1, status strings, relationship to 0016 inventory + dedup-engine.
- Root `ARCHITECTURE.md` / `README.md` note if item model section exists.
- `review.md` on completion.

### 3.9 Optional (not DoD)

- Unique index `(source_id, path)`.
- Promote body text helpers that put UTF-8 into CAS + set `text_sha256`.
- CLI command to dump item JSON.
- Backfill job over 0016 inventory (leave to 0018/0019).

---

## 4. Out of scope (do NOT do here)

| Deferred to | Work |
|---|---|
| **0016** (done) | ZIP expand, package detect |
| **0018** | PST parse; populate items from messages; bulk extract checkpoints |
| **0019** | Process job runner / blocking pool orchestration |
| **0021** | Matter-wide dedupe job using logical_hash / message_id |
| **0022** | thread_id / conversation_id assignment |
| **0029** | Tantivy body index |
| **0031+** | privilege_flags, code_tags product workflows |
| — | OCR, redaction blobs, preview generation |
| — | Replacing `dedup-engine` CLI path |
| — | Mutating source evidence; AI |

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0015** Completed (`matter-core` schema v1 + CAS + items shell).
- **P2 (recommended):** **0016** Completed — understand inventory `path` / status conventions and keep them valid.
- **P3:** Plan-of-record §2.2–2.3 accepted.
- **P4:** `cargo test -p matter-core` and `cargo test -p ingest-purview` green before/after (ingest must still compile against expanded `Item` types).
- *Verified from 0016 review:*
  - Inventory statuses `discovered` | `expanded` | `error`
  - Nested paths `zip!/inner`
  - No unique index on path yet
  - `logical_hash` unused

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Schema too wide / endless columns | P0 column set + `extra_json`; defer PII/privilege/OCR columns |
| Migration breaks 0016 inventory | Nullable columns; migration tests; re-run ingest-purview tests |
| Logical hash churn later | `logical_hash_version` + version in preimage |
| Body/attachments preimage ambiguity | Length-prefixed framing (§3.4.2); golden tests with adversarial body text |
| BCC-blind dedupe | BCC in preimage + storage; distinctness test |
| SQLite ALTER / FK limits | Prefer ADD COLUMN; app-level parent checks; rebuild only if required; v1 fixture migrate test |
| Participant SQL at scale | JSON P0; Tantivy 0029; relational participants deferred to graph tracks |
| Confuse Tier 2 with logical_hash | Explicit docs + separate function names |
| Family graph inconsistency | APIs set role + parent_item_id + family_id together; tests |
| Huge bodies in SQLite | `text_sha256` → CAS only |
| Scope creep into PST extract | Hard out-of-scope; pure inputs only |
| Audit log flood | Document batch audit policy for extractors |

---

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 — Schema v2:** `SCHEMA_VERSION == 2`; migration from v1 applied; 0016-shaped rows remain readable.
- [x] **DoD-2 — Item model:** Public `Item` / `ItemInput` (or equivalent) expose P0 fields in §3.2; insert + **update** work.
- [x] **DoD-3 — Family graph:** Create family; link parent + attachments; list members; roles/`parent_item_id` consistent.
- [x] **DoD-4 — Logical hash:** `compute_email_logical_hash` / non-email; versioned **length-prefixed** preimage; **BCC included**; tests in §3.7 (stability, sensitivity, BCC distinctness, framing, RE kept, native≠logical).
- [x] **DoD-5 — Compatibility:** `cargo test -p ingest-purview` still passes (or minor call-site updates only for new struct fields).
- [x] **DoD-6 — Docs:** matter-core README documents fields, family, logical hash v1 (framing + **BCC**), JSON address decision, status strings, Tier-2 distinction, migration notes.
- [x] **DoD-7 — Workspace gate:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, **`ledgerful verify`** (required).
- [x] **DoD-8 — Recorded:** `review.md`; `../conductor.md` → **Completed**; ledger TX committed (`ARCHITECTURE` or `FEATURE`).

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p matter-core
cargo test -p ingest-purview
cargo test --workspace
ledgerful verify
```

### Suggested types (implementer may refine)

```rust
pub const LOGICAL_HASH_VERSION: u32 = 1;
pub const SCHEMA_VERSION: u32 = 2; // matter-core

pub struct ItemFamily { pub id: String, pub matter_id: String, pub kind: String, pub created_at: String }

pub struct Item {
    // v1 fields …
    pub role: Option<String>,
    pub parent_item_id: Option<String>,
    pub mime_type: Option<String>,
    pub file_category: Option<String>,
    pub custodian: Option<String>,
    pub subject: Option<String>,
    pub title: Option<String>,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    pub cc_addrs_json: Option<String>,
    pub bcc_addrs_json: Option<String>,
    pub author: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub attachment_count: Option<i64>,
    pub text_sha256: Option<String>,
    pub html_sha256: Option<String>,
    pub logical_hash_version: u32,
    pub extra_json: Option<String>,
}
```

---

## 9. Acceptance narrative

An implementer (or test) can:

1. Open a matter created under schema v1 / 0016 inventory and migrate to v2 without data loss (ALTER-safe path or documented rebuild).
2. Insert a parent email item + two attachment items in one family with roles and digests; store To/Cc/**Bcc** JSON.
3. Compute `logical_hash` for the parent from length-prefixed preimage + child digests; recompute → identical.
4. Prove BCC-present vs BCC-absent copies get **different** hashes; body text cannot spoof attachment framing.
5. Leave `native_sha256` as physical custody; prove two different natives can share one logical_hash when logical fields match.
6. Hand **0018** a stable API: update item fields (including BCC) + set logical_hash after PST extract without inventing schema again.
