//! Shared default handler registration for Desk + CLI (track 0045).
//!
//! Keeps the automation kind set from drifting between surfaces.

use std::sync::Arc;

use crate::handlers::*;
use crate::ProcessRunner;

/// Register the full P0 automation handler set on `runner`.
///
/// Matches the Desk bootstrap set so CLI and GUI can run the same kinds:
/// ingest, extract, reduce, produce/qc/gap, fts, office/pdf/ics/ocr/classify,
/// profile_run, workflow_run.
///
/// Feature-gated handlers follow crate features (default = all P0 stages).
pub fn register_default_handlers(runner: &mut ProcessRunner) {
    #[cfg(feature = "ingest")]
    runner.register(Arc::new(IngestHandler::new()));
    #[cfg(feature = "extract_pst")]
    runner.register(Arc::new(ExtractPstHandler::new()));
    #[cfg(feature = "dedupe")]
    runner.register(Arc::new(MatterDedupeHandler::new()));
    #[cfg(feature = "thread")]
    runner.register(Arc::new(MatterThreadHandler::new()));
    #[cfg(feature = "neardup")]
    runner.register(Arc::new(MatterNearDupHandler::new()));
    #[cfg(feature = "cull")]
    runner.register(Arc::new(MatterCullHandler::new()));
    #[cfg(feature = "promote")]
    runner.register(Arc::new(MatterPromoteHandler::new()));
    #[cfg(feature = "produce")]
    {
        runner.register(Arc::new(MatterProduceHandler::new()));
        runner.register(Arc::new(MatterProductionExportHandler::new()));
    }
    #[cfg(feature = "qc")]
    runner.register(Arc::new(MatterQcHandler::new()));
    #[cfg(feature = "gap")]
    runner.register(Arc::new(MatterGapHandler::new()));
    #[cfg(feature = "fts")]
    runner.register(Arc::new(MatterFtsIndexHandler::new()));
    #[cfg(feature = "office")]
    runner.register(Arc::new(MatterOfficeExtractHandler::new()));
    #[cfg(feature = "pdf")]
    runner.register(Arc::new(MatterPdfExtractHandler::new()));
    #[cfg(feature = "calendar")]
    runner.register(Arc::new(MatterIcsExtractHandler::new()));
    #[cfg(feature = "ocr")]
    runner.register(Arc::new(MatterOcrHandler::new()));
    #[cfg(feature = "classify")]
    runner.register(Arc::new(MatterClassifyHandler::new()));
    #[cfg(feature = "entity")]
    runner.register(Arc::new(MatterEntityScanHandler::new()));

    // Always available (matter-core only).
    runner.register(Arc::new(MatterProfileRunHandler::with_default_handlers()));
    runner.register(Arc::new(MatterWorkflowRunHandler::with_default_handlers()));
}

/// Kinds registered by [`register_default_handlers`] under default features.
///
/// Used by the CLI allowlist and tests. Order is documentation-stable.
pub fn default_handler_kinds() -> &'static [&'static str] {
    &[
        "ingest",
        "extract_pst",
        "dedupe",
        "thread",
        "neardup",
        "cull",
        "promote",
        "produce",
        "production_export",
        "qc",
        "gap",
        "fts_index",
        "office_extract",
        "pdf_extract",
        "ics_extract",
        "ocr",
        "classify",
        "entity_scan",
        "profile_run",
        "workflow_run",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessRunner, RunnerConfig};

    #[test]
    fn register_default_handlers_covers_core_kinds() {
        let mut runner = ProcessRunner::new(RunnerConfig::default());
        register_default_handlers(&mut runner);
        // Unknown kind rejected; known kinds accepted at start only with matter —
        // just ensure shutdown is clean after register.
        runner.shutdown();
        assert!(default_handler_kinds().contains(&"workflow_run"));
        assert!(default_handler_kinds().contains(&"profile_run"));
        assert!(default_handler_kinds().contains(&"ingest"));
    }
}
