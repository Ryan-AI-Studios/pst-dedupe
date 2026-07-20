//! # matter-cluster
//!
//! Offline **concept / theme clustering** (track **0048**):
//!
//! - Method **`tfidf_kmeans_v1`**: prep strip → tokenize → sparse TF–IDF →
//!   **mandatory L2** → k-means (deterministic seed) → **drop empty** →
//!   **c-TF-IDF / ICF** labels
//! - Job `concept_cluster`: Phase A cooperative cancel; Phase B atomic membership replace
//! - Orthogonal to near-dup (`near_dup_*`) — separate schema
//!
//! ## Honesty
//!
//! - TF–IDF clusters ≠ true semantic understanding; synonyms/polysemy limited.
//! - English stopwords + header/boilerplate lists P0; other languages degraded (**0054**).
//! - **Requested k** is a target; **actual** non-empty `cluster_count` may be fewer.
//! - Header/disclaimer strip is vocab hygiene — **not** privilege detection.
//! - Not a substitute for privilege review or coding.
//! - **Not** Relativity LSI Conceptual Analytics.
//! - **Not** near-duplicate detection (**0023** / MinHash).
//! - **Not** embeddings / BERTopic / transformers (**0050**).
//! - Deterministic only for same code version + params + input set.
//! - Large matters need `max_docs` discipline (fail closed when exceeded).
//!
//! ## Storage
//!
//! Schema **v27**: `concept_cluster_sets` (`k` requested + `cluster_count` actual),
//! `concept_clusters` (dense ordinals, item_count > 0), `item_concept_membership`.

#![forbid(unsafe_code)]

pub mod ctfidf;
pub mod error;
pub mod kmeans;
pub mod params;
pub mod prep;
pub mod run;
pub mod stopwords;
pub mod tfidf;
pub mod tokenize;

pub use error::{ClusterError, Result};
pub use params::{ConceptClusterParams, DEFAULT_SET_NAME, SCOPE_ALL};
pub use run::{
    concept_cluster_fingerprint, concept_cluster_params_fingerprint_input,
    inventory_digest_from_candidates, inventory_digest_from_fingerprint, run_concept_cluster,
    ConceptClusterOutcome, ConceptClusterReport, ConceptClusterSummary,
    CONCEPT_CLUSTER_ENGINE_VERSION, CONCEPT_CLUSTER_STAGE, JOB_KIND_CONCEPT_CLUSTER,
    METHOD_TFIDF_KMEANS_V1,
};
