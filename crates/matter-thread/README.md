# matter-thread

Matter-level **email threading** over Normalized Items (track **0022**).

Runs as a resumable `process-runner` job (`kind = "thread"`). Does **not** call
`create_job` (Option C — the runner owns job rows).

## Algorithm

| Phase | Signal | Scope |
|---|---|---|
| **A — Headers** | Own Message-ID ∪ In-Reply-To ∪ References (union-find) | All eligible parents |
| **B — Subject** | `normalize_subject_thread` (strip RE/FW/FWD) | **Singletons only** |
| **C — ConversationIndex** | Opaque first **44 hex** chars (22 bytes) | **Singletons only** |
| **Family** | Inherit parent `thread_*` | Attachment children |

### Stable order

`imported_at ASC`, `path ASC`, `id ASC` (root = earliest member).

### `thread_id` (full 64-char SHA-256 hex)

| Method | Preimage |
|---|---|
| `headers` | `"thread:v1\n" \|\| min(normalized matter MID in component)` |
| `subject` | `"thread-subj:v1\n" \|\| subject_key` |
| `conversation_index` | `"thread-ci:v1\n" \|\| prefix44` |
| `singleton` (no MID) | own item id |

### Conservatism (intentional)

- Subject fallback **never** glues an orphan into an existing multi-member
  **headers** thread (avoids false merges of unrelated “RE: Invoice” chains).
- ConversationIndex uses the **same singleton-only** rule.
- Phantom MIDs (referenced but absent) still link children that share them.
- Header **storage** columns (`in_reply_to`, `references_json`, …) are **not**
  cleared by the thread job (`reset` only clears result `thread_*` fields).

### Eligible parents

`(role = 'parent' OR (file_category = 'email' AND role ≠ attachment))` and
`status IN ('extracted','partial','normalized')`.

Unique **and** duplicate parents are threaded (conversation structure ≠ suppress).

## Memory posture

- Thin `ThreadCandidate` rows only (no body text / no full `Item`).
- Candidates are loaded via **paged** `list_email_parents_for_thread_range`
  (page size 500). Phase A union-find still holds the full **thin** set so
  header components can be built; that is intentional and still far smaller
  than materializing full Normalized Items.
- Assignment writes and family inherit commit in **batches** (`batch_size`).
- Union-find / maps use **`[u8; 32]`** keys (SHA-256 of normalized MID / subject / CI prefix).
- Reverse MID strings kept only for matter members as needed for `thread_id`.

## Transactions

Batch `thread_*` updates + `put_checkpoint` stage `thread` commit in **one**
SQLite transaction via `Matter::apply_thread_batch_with_checkpoint`. Cancel
between batches → `Paused`.

## Params JSON

```json
{
  "use_headers": true,
  "use_subject_fallback": true,
  "use_conversation_index": true,
  "reset": true,
  "batch_size": 500,
  "family_inherit": true
}
```

## Header normalize contracts (extract)

| Field | Contract |
|---|---|
| References | Unfold RFC 2822 folds; extract via `<…>`; `normalize_message_id`; ordered JSON |
| ConversationIndex | MAPI **bytes** or Base64 Thread-Index → **lowercase hex only**; invalid Base64 → NULL |

**Re-extract required** to populate reply headers on matters extracted before
track 0022. Until then the job is best-effort (singletons / subject / CI only).

## Audit

`thread.start` / `thread.complete` / `thread.fail`.
