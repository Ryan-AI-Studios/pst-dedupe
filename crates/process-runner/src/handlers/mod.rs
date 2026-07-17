//! Built-in stage handlers (feature-gated).

#[cfg(feature = "ingest")]
pub mod ingest;

#[cfg(feature = "extract_pst")]
pub mod extract_pst;

#[cfg(feature = "ingest")]
pub use ingest::IngestHandler;

#[cfg(feature = "extract_pst")]
pub use extract_pst::ExtractPstHandler;
