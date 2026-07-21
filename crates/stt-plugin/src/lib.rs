//! # stt-plugin
//!
//! Opt-in **local speech-to-text** for Dedupe Desk (track **0053**):
//!
//! | Role | Stack |
//! |---|---|
//! | Primary engine | **whisper.cpp CLI** sidecar |
//! | Tests / CI | [`MockSttEngine`] (no Whisper weights) |
//! | Common audio prep | **Symphonia** pure-Rust decode/resample → 16 kHz mono s16le |
//! | Video / complex audio | Optional **ffmpeg** sidecar (`-ar 16000 -ac 1 -c:a pcm_s16le`) |
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`run_transcribe`], [`run_transcribe_with_engine`] are **CPU- and IO-bound**.
//! Callers **must** run them on a dedicated blocking worker (`process-runner`
//! matter worker). Never call on the GUI or Tokio async worker.
//!
//! ## Enable gate
//!
//! Default **OFF**. Job fails closed when `params.enabled` is false — no item
//! mutation. Desk passes enable flag + tool/model paths in job params JSON.
//!
//! ## Body write policy
//!
//! Existing non-empty `text_sha256` is **concatenated** with
//! `--- TRANSCRIPT ---` + STT output (never blind-replaced). See matter-core
//! `apply_transcript_text`.
//!
//! ## Safety
//!
//! - Windows **Job Object** kill-on-close for whisper/ffmpeg children
//! - Cooperative cancel terminates the active Job Object mid ffmpeg/whisper wait
//! - Drop-guarded temps under `workspace/temp/stt/`
//! - Startup purge of residual temps
//! - Native size / duration caps (post-ffmpeg WAV re-check)
//! - No cloud STT; no silent model download; proxy env scrubbed on children

// Job Object / process control uses Windows FFI in `job_object` (cfg-gated).
#![cfg_attr(not(windows), forbid(unsafe_code))]

pub mod decode;
pub mod detect;
pub mod engine;
pub mod error;
pub mod ffmpeg;
pub mod job_object;
pub mod limits;
pub mod params;
pub mod run;
pub mod temp;

pub use decode::{
    decode_to_whisper_wav, linear_resample_mono, stereo_44100_wav_bytes, write_pcm_s16le_wav,
};
pub use detect::{
    is_audio_meta, is_video_meta, is_whisper_compliant_wav, looks_like_wav, AUDIO_EXTS, VIDEO_EXTS,
};
pub use engine::{
    whisper_cli_looks_available, MockSttEngine, SttEngine, TranscriptResult, WhisperCliEngine,
};
pub use error::{Error, Result};
pub use ffmpeg::{
    args_contain_locked_pcm_flags, build_ffmpeg_pcm_args, ffmpeg_looks_available, resolve_ffmpeg,
};
pub use job_object::{
    kill_on_close_limit_flags, spawn_and_wait, spawn_and_wait_cancellable, CancellableWaitError,
    ManagedChild, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
pub use limits::{
    engines, status, DEFAULT_MAX_DURATION_SECS, DEFAULT_MAX_NATIVE_BYTES,
    MAX_TRANSCRIPT_TEXT_BYTES, STT_TEMP_SUBDIR, TARGET_CHANNELS, TARGET_SAMPLE_RATE,
    TRUNCATION_MARKER,
};
pub use params::SttParams;
pub use run::{
    minimal_wav_bytes, reject_oversized_native_len, run_transcribe, run_transcribe_with_engine,
    truncate_transcript_text, SttOutcome, SttSummary, JOB_KIND_TRANSCRIBE, TRANSCRIBE_STAGE,
};
pub use temp::{ensure_stt_temp_dir, purge_stt_temp_dir, stt_temp_dir, SttTempFile};
