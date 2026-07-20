# matter-cluster

Offline **concept / theme clustering** for Dedupe Desk (track **0048**).

## Method `tfidf_kmeans_v1`

1. **Prep** — strip email header lines + confidentiality boilerplate (vocab hygiene, **not** privilege detection)
2. **Tokenize** — lowercase Unicode word tokens; English stopwords; drop structural tokens (`from`/`sent`/`subject`/`mailto`)
3. **Sparse TF–IDF** — DF filters (`min_df`, `max_df_ratio`), `max_vocab`
4. **Mandatory L2** row normalization (spherical / cosine-like geometry; zero vectors stay zero)
5. **k-means** — deterministic seed (SplitMix64); max iterations capped
6. **Drop empty** clusters; dense ordinals `0..cluster_count-1`
7. **c-TF-IDF + ICF** labels (cluster-as-document; not doc-corpus TF–IDF alone)

## Honesty

| Claim | Reality |
|---|---|
| Relativity Conceptual Analytics / LSI | **No** — classical TF–IDF themes only |
| Near-duplicate detection | **No** — orthogonal to `near_dup_*` (**0023**) |
| Embeddings / BERTopic / transformers | **No** — deferred **0050** |
| True semantic understanding | **No** — lexical only; synonyms/polysemy limited |
| Requested `k` always realized | **No** — empty centroids dropped; UI must use actual `cluster_count` |
| Multilingual | English stopwords P0; other languages degraded (**0054**) |
| Privilege detection | Header/disclaimer strip is **not** privilege review |

## Job `concept_cluster`

```powershell
.\target\release\pst-dedup.exe job run --path $matter --kind concept_cluster --json --params '{
  "set_name":"default","k":20,"seed":42,"max_docs":50000,"max_text_bytes":200000,
  "min_df":2,"max_df_ratio":0.9,"max_vocab":20000,"label_terms":8,"scope":"all","reset":true
}'
```

- **Phase A** — load text, prep, tokenize (cooperative cancel; no complete membership)
- **Phase B** — matrix + k-means + labels + **atomic** set replace; `built_at` only after commit
- **`max_docs`** — **fail closed** if candidate count exceeds cap
- Opt-in — **not** in profile/workflow allowlists

## Schema v27

- `concept_cluster_sets` — `k` (requested) + `cluster_count` (actual)
- `concept_clusters` — dense ordinals, `item_count > 0`
- `item_concept_membership`
- Optional denorm on `items` for default set: `concept_cluster_id`, `concept_cluster_set_id`, `concept_clustered_at`

## FilterSpec

- `concept_cluster_id` — `eq` / `any_of`
- `concept_cluster_set_id` — `eq`
- `has_concept_cluster` — `eq` true/false

## Fixtures

See `fixtures/cluster/` for multi-topic plaintexts and header-junk emails.
