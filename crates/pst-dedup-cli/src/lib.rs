//! Library surface for `pst-dedup` orchestration (GUI / in-process callers).
//!
//! The `pst-dedup` binary is a thin clap front-end over these modules.
//! Track **0072** depends on [`unique_pst_cmd::run_unique_pst_with_options`] so the
//! GUI wizard shares the same pipeline as the CLI (no process spawn, no dual path).

pub mod convenience;
pub mod error;
pub mod inspect;
pub mod job_cmd;
pub mod json_io;
pub mod keep_set_cmd;
pub mod matter_cmd;
pub mod paths;
pub mod platform_cmd;
pub mod production_profile_cmd;
pub mod profile_cmd;
pub mod pst_materializer;
pub mod runner_util;
pub mod scan;
pub mod service_cmd;
pub mod unique_eml_cmd;
pub mod unique_export_report;
pub mod unique_pst_cmd;
pub mod workflow_cmd;

pub use unique_pst_cmd::{
    run_unique_pst, run_unique_pst_with_options, FolderLayoutArg, UniquePstClapArgs,
    UniquePstCliArgs, UniquePstOutcome, UniquePstProgress, UniquePstRunOptions, UniqueVolumeDigest,
};
