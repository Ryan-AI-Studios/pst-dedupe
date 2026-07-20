# Cluster fixtures (track 0048)

Synthetic multi-topic plaintexts for offline concept clustering tests.
**Never** put real case bodies here.

| File | Theme |
|---|---|
| `invoice_*.txt` | Vendor / payment / invoice |
| `clinical_*.txt` | Patient / clinical / dosage |
| `sports_*.txt` | Tournament / soccer / league |
| `email_header_junk_*.txt` | Shared headers + disclaimers + distinct topical bodies |

Method under test: `tfidf_kmeans_v1` (TF–IDF + L2 + k-means + c-TF-IDF/ICF).
Not near-dup, not embeddings, not Relativity LSI.
