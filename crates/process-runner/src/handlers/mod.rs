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

#[cfg(feature = "fts")]
pub mod fts;

#[cfg(feature = "office")]
pub mod office;

#[cfg(feature = "pdf")]
pub mod pdf;

#[cfg(feature = "calendar")]
pub mod ics;

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

#[cfg(feature = "fts")]
pub use fts::MatterFtsIndexHandler;

#[cfg(feature = "office")]
pub use office::MatterOfficeExtractHandler;

#[cfg(feature = "pdf")]
pub use pdf::MatterPdfExtractHandler;

#[cfg(feature = "calendar")]
pub use ics::MatterIcsExtractHandler;
