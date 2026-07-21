//! STT engines (whisper.cpp CLI + mock).

// `trait` is a keyword; module lives in trait.rs via path attribute.
#[path = "trait.rs"]
mod trait_;

pub mod mock;
pub mod whisper_cli;

pub use mock::MockSttEngine;
pub use trait_::{SttEngine, TranscriptResult};
pub use whisper_cli::{whisper_cli_looks_available, WhisperCliEngine};
