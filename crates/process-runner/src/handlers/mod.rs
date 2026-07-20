//! Built-in stage handlers (feature-gated).

#[cfg(feature = "ingest")]
pub mod ingest;

#[cfg(feature = "extract_pst")]
pub mod extract_pst;

#[cfg(feature = "dedupe")]
pub mod dedupe;

#[cfg(feature = "thread")]
pub mod thread;

#[cfg(feature = "neardup")]
pub mod neardup;

#[cfg(feature = "cull")]
pub mod cull;

#[cfg(feature = "promote")]
pub mod promote;

#[cfg(feature = "produce")]
pub mod produce;

#[cfg(feature = "qc")]
pub mod qc;

#[cfg(feature = "gap")]
pub mod gap;

#[cfg(feature = "fts")]
pub mod fts;

#[cfg(feature = "office")]
pub mod office;

#[cfg(feature = "pdf")]
pub mod pdf;

#[cfg(feature = "calendar")]
pub mod ics;

#[cfg(feature = "ocr")]
pub mod ocr;

#[cfg(feature = "classify")]
pub mod classify;

#[cfg(feature = "entity")]
pub mod entity_scan;

#[cfg(feature = "people")]
pub mod people_graph;

/// Sequential processing-profile runner (always available; depends only on matter-core).
pub mod profile_run;

/// Sequential workflow runner (always available; depends only on matter-core + profile_run).
pub mod workflow_run;

#[cfg(feature = "ingest")]
pub use ingest::IngestHandler;

#[cfg(feature = "extract_pst")]
pub use extract_pst::ExtractPstHandler;

#[cfg(feature = "dedupe")]
pub use dedupe::MatterDedupeHandler;

#[cfg(feature = "thread")]
pub use thread::MatterThreadHandler;

#[cfg(feature = "neardup")]
pub use neardup::MatterNearDupHandler;

#[cfg(feature = "cull")]
pub use cull::MatterCullHandler;

#[cfg(feature = "promote")]
pub use promote::MatterPromoteHandler;

#[cfg(feature = "produce")]
pub use produce::{MatterProduceHandler, MatterProductionExportHandler};

#[cfg(feature = "qc")]
pub use qc::MatterQcHandler;

#[cfg(feature = "gap")]
pub use gap::MatterGapHandler;

#[cfg(feature = "fts")]
pub use fts::MatterFtsIndexHandler;

#[cfg(feature = "office")]
pub use office::MatterOfficeExtractHandler;

#[cfg(feature = "pdf")]
pub use pdf::MatterPdfExtractHandler;

#[cfg(feature = "calendar")]
pub use ics::MatterIcsExtractHandler;

#[cfg(feature = "ocr")]
pub use ocr::MatterOcrHandler;

#[cfg(feature = "classify")]
pub use classify::MatterClassifyHandler;

#[cfg(feature = "entity")]
pub use entity_scan::MatterEntityScanHandler;

#[cfg(feature = "people")]
pub use people_graph::MatterPeopleGraphHandler;

pub use profile_run::MatterProfileRunHandler;
pub use workflow_run::MatterWorkflowRunHandler;
