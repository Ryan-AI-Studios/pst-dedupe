# matter-neardup

Matter-level **near-duplicate detection** (track **0023**): classical shingling +
MinHash + banded LSH clusters with pivot/member roles.

Runs as a resumable `process-runner` job (`kind = "neardup"`). Does **not** call
`create_job` (Option C).

## Method tag

`near_dup_method = "minhash_shingle_v1"`

## Algorithm (frozen)

```text
for each eligible item (stream):
  text ← CAS(text_sha256)   // drop after sketch
  prep → mixed-script tokens → unique shingle set
  sig ← MinHash(shingles, H=128, seed)
  record (item_id, token_count, sig)
// banded LSH → candidate pairs
// est Jaccard ≥ threshold → union-find
// component size≥2 → pick pivot; re-score members; demote weak
// size-1 → unique
```

### MinHash expansion — Approach A (required)

**Forbidden:** Kirsch–Mitzenmacher `slot_i = h1.wrapping_add(i.wrapping_mul(h2))`
from a single digest pair.

**Frozen recipe** for each unique shingle string `S` (UTF-8):

1. `digest = SHA-256(S_utf8)`
2. `first_u64 = u64::from_be_bytes(digest[0..8])` (big-endian)
3. `base = first_u64 XOR hash_seed`
4. Seed in-crate **SplitMix64** with `base` (no `rand` crate defaults)
5. Emit next `H` `u64` values as the hash images of this shingle
6. MinHash slot `i` = **min** over shingles of stream value `i`

**SplitMix64 constants** (hard-coded):

| Constant | Value |
|---|---|
| increment / gamma | `0x9E3779B97F4A7C15` |
| mix mul 1 | `0xBF58476D1CE4E5B9` |
| mix mul 2 | `0x94D049BB133111EB` |

**Default `hash_seed`:** `0x4E445F6D685F7631` (`DEFAULT_HASH_SEED` in `params.rs`).

**Similarity estimate:** fraction of MinHash slots equal (Jaccard estimate).

### Tokenizer (mixed-script)

| Script run | Detection | Tokenization |
|---|---|---|
| **CJK** | Han U+4E00–9FFF / Ext-A U+3400–4DBF, Hiragana U+3040–309F, Katakana U+30A0–30FF, Hangul U+AC00–D7AF, Compat U+F900–FAFF | Character n-grams (`cjk_char_n`, default **2**). Each n-gram is a shingle. |
| **Space-delimited** | Remainder | Split on whitespace + simple punctuation → word tokens → word *k*-shingles (`shingle_k`, default **5**), joined with U+001F. Runs with **fewer than `shingle_k` words contribute zero shingles** (no 1-word fallback). |

Shingle set = **set** (unique) for Jaccard. Prep: lowercase + collapse whitespace;
optional `ignore_numbers` drops pure-digit **word** tokens only.

Empty shingles after prep → role `skipped` (includes Latin docs with `< shingle_k` words even when `min_chars` is met).

### Clustering / pivot

- Banded LSH: default **16 bands × 8 rows** (`H=128`)
- Link pairs with est Jaccard ≥ `threshold` (default **0.80**)
- Pivot = max `token_count`; ties → `imported_at ASC`, `path ASC`, `id ASC`
- Re-score each member vs pivot; if &lt; threshold → demote to `unique` (single-link demotion)
- Group id: full 64-hex SHA-256 of `near:v1\n{pivot_item_id}`
- Roles: `pivot` \| `member` \| `unique` \| `skipped`
- Similarity REAL 0–1 vs pivot (`1.0` for pivot); NULL for unique/skipped

**Honesty (Reveal-style NDD):** groups are pivot-dependent and **not** an
equivalence relation. Near-dups must **not** be treated as exact suppress or
used to auto-propagate privilege coding.

## Defaults

| Param | Default |
|---|---|
| `shingle_k` | 5 |
| `cjk_char_n` | 2 |
| `num_hashes` | 128 |
| `num_bands` / `rows_per_band` | 16 / 8 |
| `threshold` | 0.80 |
| `hash_seed` | `DEFAULT_HASH_SEED` |
| `skip_exact_duplicates` | true |
| `ignore_numbers` | true |
| `min_chars` | 80 |
| `reset` | true |
| `batch_size` | 200 |
| `include_attachments` | true |
| `strip_email_quotes` | false (not implemented P0) |

## Eligibility

- Status ∈ `{extracted, partial, normalized}`
- Non-empty text via `text_sha256` → CAS UTF-8
- `skip_exact_duplicates`: `dedup_role=duplicate` → `skipped`
- `min_chars` (default 80) → below → `skipped`
- Parents + attachments with text by default

## Job integration

| Item | Value |
|---|---|
| Kind / stage | `neardup` |
| Checkpoint | `{ phase, cursor_index, completed_count, group_count, member_count, unique_count, skipped_count, params }` |
| Cancel | Cooperative between sketch items / write batches → Paused |
| Atomicity | near_dup field updates + `put_checkpoint` same SQLite txn |

### Memory posture (P0)

Hold signatures only: ≈ `128 × 8` bytes + id/metadata per eligible doc.
Body text is never retained after sketch. Multi-million signature spill → future.

## Audit

`neardup.start` / `neardup.complete` / `neardup.fail`.

## Email vs threading

Pure email reply chains are better served by **0022 threading**. Near-dup helps
drafts, lightly edited copies, attachments, and non-mail. Deep quote stripping
is **not** implemented in P0 (`strip_email_quotes` must stay false).

## Tests

```powershell
cargo test -p matter-neardup
```
