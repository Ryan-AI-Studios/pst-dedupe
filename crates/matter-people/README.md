# matter-people

Offline **people–communications graph** for Dedupe Desk (track **0047**).

## What it does

- Parses **From / To / Cc / Bcc** headers into relational `item_participants`
- Builds person nodes with `identity_kind` + `normalized_key` (**smtp | display | x500 | other**)
- Aggregates directed **From → recipient** edges with **separate To / Cc / Bcc counters**
- Timeline day/week buckets of message volume
- Job kind: `people_graph` (two-pass; opt-in)

## Architectural locks

1. **Do not drop non-SMTP** — display names and X.500/Exchange DNs become people nodes
2. **SMTP** reuses `matter_entity::normalize_email` (0046 punctuation + case-fold)
3. **BCC separated** — never mixed into default Top Pairs (`visible_count = to + cc` only)
4. **No A→A edges** — self-mail increments `people.self_mail_count`
5. **Two-pass**: resumable Pass 1 → `item_participants`; atomic Pass 2 SQL aggregates
6. **Headers primary**; `include_entity_emails` default **false**. Setting `true` **fails closed** (entity-body email join deferred — not a silent no-op)
7. **SQLite only** — no Neo4j
8. **person_id** = full **64-char** hex of `sha256(identity_kind || "\0" || normalized_key)`
9. **SMTP** treats `local@domain` as email even when domain has **no** `.` (`alice@corp` → smtp / domain `corp`); dotted domains reuse `matter_entity::normalize_email`
10. **Display / X.500 / other** keys use Unicode case-fold (`to_lowercase`)

## Honesty / limits

- **Not** legal name resolution or identity proof.
- Display-name collisions (**"John Doe"**) **over-merge** into one node — preferred over silent drop of internal mail.
- BCC is stored for investigation/filter but **excluded** from default pair rankings and `visible_count`.
- Self-notes/drafts do not dominate Top Pairs.
- Incomplete when extract lacks full recipient tables (MAPI residual).
- **Not** Relativity Communication Analysis parity.
- Timeline uses available item dates only (`sent_at` → `received_at` → `created_at`).
- No ML alias merge (`jdoe@a.com` ≠ `john.doe@a.com` ≠ `"John Doe"`).

## Job params

```json
{
  "scope": "all",
  "include_entity_emails": false,
  "grain": "day",
  "reset": true,
  "batch_size": 200,
  "max_recipients_per_item": 200
}
```

| Param | Default | Notes |
|---|---|---|
| `scope` | `"all"` | P0: all items with address fields |
| `include_entity_emails` | `false` | Headers-only. **`true` is rejected** at params validation (entity-body join residual / deferred) |
| `grain` | `"day"` | Timeline bucket: `day` \| `week` |
| `reset` | **`true`** | Wipe people-graph tables then rebuild. **Desk defaults `reset:true`** |
| `batch_size` | `200` | Pass 1 keyset page size |
| `max_recipients_per_item` | `200` | Cap to+cc+bcc expanded per item |

### `include_entity_emails` (fail-closed residual)

Body entity email hits (`item_entity_hits` pack `email`) are **not** joined into the people graph in this track. The param exists for forward compatibility:

- **`false` (default):** headers-only Pass 1 — supported.
- **`true`:** **error** at parse/validate (`InvalidParams`) and at job start — never silently ignored.

Do not set `true` until a later track implements the join; keep CLI/desk JSON at `false`.

### Fingerprint and soft-stale (`reset:false`)

Fingerprint = `sha256(engine_version + "|" + params_json)` where `engine_version` is `people_graph_v1` and params are the validated job params.

- When `reset:false` and the matter already has a **complete** graph whose stored fingerprint matches current engine+params, Pass 1/2 are **skipped** (soft skip).
- That skip does **not** detect item inventory changes (new/deleted/edited messages). Inventory digests are a residual — re-run with `reset:true` (desk default) after ingest/extract changes.
- When `reset:false` but fingerprint is stale or the graph is incomplete, tables are cleared and a full rebuild runs.

### Two-pass safety

| Pass | Resumable | Work |
|---|---|---|
| 1 | Yes (item cursor checkpoint) | Normalize + upsert `item_participants` + person stubs |
| 2 | No (re-run whole pass) | Delete edges/timeline; rebuild people counts, edges, timeline |

`people_graph_built_at` / fingerprint set **only after Pass 2**. Desk must not treat the graph as complete until `people_graph_pass = complete`.

## CLI recipe (0045 generic job)

```powershell
.\target\release\pst-dedup.exe job run --path $m --kind people_graph --params '{
  "scope":"all","include_entity_emails":false,"grain":"day","reset":true,
  "batch_size":200,"max_recipients_per_item":200
}'
```

## Fixtures

Synthetic multi-party notes live under `fixtures/people/` (`example.com` only).
