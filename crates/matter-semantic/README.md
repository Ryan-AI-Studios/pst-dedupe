# matter-semantic

Opt-in **local semantic search** for Dedupe Desk (track **0050**).

## Architecture locks

| Rule | Detail |
|---|---|
| Keyword FTS primary | Tantivy (**0029**) unchanged; semantic is additive |
| Default OFF | No required models for core Desk |
| Local only | No cloud embeddings in P0 |
| No silent download | Model install is explicit operator action |
| Chunk + overlap | Retrieval unit = chunk → item |
| L2 + cosine | Vectors always L2-normalized |
| Pre-filter | Resolve FilterSpec → eligible ids **before** cosine |
| Group-before-limit | `best_score(item) = max(chunk)` then top_n **items** |
| Model namespace | `{matter}/semantic/{sanitized_model_id}/` |
| Single-exe | Prefer pure-Rust Candle residual; no mandatory ONNX DLL |

## vs keyword FTS

| | Keyword FTS (0029) | Semantic (0050) |
|---|---|---|
| Engine | Tantivy Boolean / phrase | Local embeddings + cosine |
| Strength | Precise Boolean, required for many workflows | Paraphrase / related concepts |
| UI | Review **Keyword** bar | Review **Semantic** bar (separate) |
| Job | `fts_index` | `semantic_index` |
| Default | Index built as needed for keyword review | **OFF** until operator runs index |

Never silently replace keyword results with semantic-only rankings.

## Offline / single-exe

- **CI / default:** `MockEmbedder` (`mock:hash_v1`, 32-d bag-of-hash) — no weights, no GPU, no network
- **Weights:** **never** committed to git; live under matter path or operator-chosen cache after explicit install
- **Production residual:** `local:minilm-l6-v2` via feature `semantic-candle` (fail-closed without weights; no silent download)
- Prefer **Candle** pure-Rust for single-exe; ONNX/FastEmbed only with documented DLL bundle (not P0 happy path)

## Defaults

```json
{
  "model_id": "mock:hash_v1",
  "chunk_chars": 800,
  "chunk_overlap": 120,
  "max_chunks_per_item": 48,
  "max_text_bytes": 200000,
  "max_docs": 50000,
  "reset": false,
  "batch_size": 16,
  "scope": "all"
}
```

## Job (CLI parity)

Kind: `semantic_index` (registered via process-runner `register_default_handlers`).

```powershell
.\target\release\pst-dedup.exe job run --path $m --kind semantic_index --json
# Full rebuild:
.\target\release\pst-dedup.exe job run --path $m --kind semantic_index --params-json '{"reset":true}' --json
```

No separate CLI search subcommand in P0 — Desk uses `search_semantic`; automation can call the library API.

Fingerprint includes model_id, dims, chunk params, engine tag. Digest skip when
`semantic_embedded_text_sha256 == text_sha256` and fingerprint matches.

## License / supply chain

Runtime deps: `matter-core`, `serde`, `serde_json`, `thiserror`, `chrono`, `camino` — MIT/Apache-2.0 class.

Weights are **never** committed to git. Candle feature is optional and fail-closed.
Model licenses (e.g. Apache MiniLM) are accepted by the operator on explicit install.

## Honesty

- Semantic ranks by embedding similarity — not Boolean precision; false friends expected
- Empty results under a filter mean “no in-set hit,” not case-wide none
- English-centric mock / small models; multilingual residual **0054**
- Chunk boundaries can split context; long docs are multi-chunk (group-before-limit still returns **items**)
- Not privilege/responsiveness prediction
- Offline only after local model available (mock works without weights); **no silent cloud**
- Keyword FTS remains required for many review workflows
