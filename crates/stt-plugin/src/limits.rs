//! Safety limits for STT (spec §3.4 / §3.9).

/// Default max media duration (1 hour).
pub const DEFAULT_MAX_DURATION_SECS: u64 = 3600;

/// Default max native input size (500 MiB).
pub const DEFAULT_MAX_NATIVE_BYTES: u64 = 500_000_000;

/// Max transcript plain-text output (10 MiB).
pub const MAX_TRANSCRIPT_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// Marker appended when text is truncated at the output cap.
pub const TRUNCATION_MARKER: &str = "\n[… truncated …]\n";

/// Subdirectory under `workspace/temp/` for STT temps.
pub const STT_TEMP_SUBDIR: &str = "stt";

/// Whisper-hard PCM target sample rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Target channel count (mono).
pub const TARGET_CHANNELS: u16 = 1;

/// `transcript_status` values (mirror matter-core).
pub mod status {
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const SKIPPED: &str = "skipped";
    pub const PENDING: &str = "pending";
    pub const DISABLED: &str = "disabled";
}

/// Engine ids.
pub mod engines {
    pub const WHISPER_CLI: &str = "whisper_cli";
    pub const MOCK: &str = "mock";
    pub const AUTO: &str = "auto";
}
