//! whisper.cpp CLI sidecar engine.

use std::path::{Path, PathBuf};

use camino::{Utf8Path, Utf8PathBuf};

use super::trait_::{SttEngine, TranscriptResult};
use crate::error::{Error, Result};
use crate::job_object::{spawn_and_wait_cancellable, CancellableWaitError};
use crate::limits::engines;

/// whisper.cpp CLI executable sidecar.
#[derive(Debug, Clone)]
pub struct WhisperCliEngine {
    /// Resolved executable path.
    pub exe: Utf8PathBuf,
    /// Model weights path (required for production).
    pub model_path: Utf8PathBuf,
}

impl WhisperCliEngine {
    /// Discover whisper CLI: settings path → common names on PATH.
    pub fn discover(settings_path: Option<&str>, model_path: Option<&str>) -> Result<Self> {
        let exe = resolve_whisper_cli(settings_path)?;
        let model = model_path
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                Error::ModelNotFound(
                    "model_path required for whisper_cli (operator must set path; no silent download)"
                        .into(),
                )
            })?;
        let model_path = Utf8PathBuf::from(model);
        if !model_path.as_std_path().is_file() {
            return Err(Error::ModelNotFound(format!(
                "model_path not found: {model_path}"
            )));
        }
        Ok(Self { exe, model_path })
    }

    /// Build argv for a wav transcription (unit-testable).
    ///
    /// Uses common whisper.cpp CLI shape:
    /// `whisper-cli -m <model> -f <wav> -l <lang> -nt` (no timestamps for plain text).
    pub fn build_transcribe_args(
        model: &Utf8Path,
        wav: &Utf8Path,
        language: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec![
            "-m".into(),
            model.as_str().to_string(),
            "-f".into(),
            wav.as_str().to_string(),
            "-nt".into(),
        ];
        if let Some(lang) = language.map(str::trim).filter(|s| !s.is_empty()) {
            args.push("-l".into());
            args.push(lang.to_string());
        }
        args
    }
}

impl SttEngine for WhisperCliEngine {
    fn engine_id(&self) -> &str {
        engines::WHISPER_CLI
    }

    fn model_id(&self) -> &str {
        self.model_path
            .file_name()
            .unwrap_or(self.model_path.as_str())
    }

    fn transcribe_wav_path(
        &self,
        path: &Utf8Path,
        language: Option<&str>,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<TranscriptResult> {
        let args = Self::build_transcribe_args(&self.model_path, path, language);
        let out = match spawn_and_wait_cancellable(self.exe.as_str(), &args, None, cancel) {
            Ok(o) => o,
            Err(CancellableWaitError::Cancelled) => return Err(Error::Cancelled),
            Err(CancellableWaitError::Io(e)) => {
                return Err(Error::Engine(format!("spawn whisper-cli: {e}")));
            }
        };
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(Error::Engine(format!(
                "whisper-cli failed ({}): {}",
                out.status,
                stderr.trim()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(TranscriptResult {
            text,
            language: language.map(|s| s.to_string()),
        })
    }
}

fn resolve_whisper_cli(settings_path: Option<&str>) -> Result<Utf8PathBuf> {
    if let Some(p) = settings_path.map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(p);
        if path.is_file() {
            return Utf8PathBuf::from_path_buf(path.to_path_buf())
                .map_err(|_| Error::EngineNotFound(format!("whisper_cli path is not UTF-8: {p}")));
        }
        return Err(Error::EngineNotFound(format!(
            "whisper_cli_path not found: {p}"
        )));
    }
    for name in ["whisper-cli", "whisper", "main"] {
        if let Some(found) = find_on_path(name) {
            return Utf8PathBuf::from_path_buf(found)
                .map_err(|_| Error::EngineNotFound(format!("{name} on PATH is not UTF-8")));
        }
    }
    #[cfg(windows)]
    {
        for name in ["whisper-cli.exe", "whisper.exe", "main.exe"] {
            if let Some(found) = find_on_path(name) {
                return Utf8PathBuf::from_path_buf(found)
                    .map_err(|_| Error::EngineNotFound(format!("{name} on PATH is not UTF-8")));
            }
        }
    }
    Err(Error::EngineNotFound(
        "whisper-cli not found (set Settings path or install whisper.cpp and add to PATH)".into(),
    ))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{name}.exe"));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

/// True when whisper CLI appears resolvable (Desk enablement helper).
pub fn whisper_cli_looks_available(settings_path: Option<&str>) -> bool {
    resolve_whisper_cli(settings_path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    #[test]
    fn discover_missing_settings_path_is_engine_not_found() {
        let missing = if cfg!(windows) {
            r"C:\nonexistent\stt-plugin-whisper-missing\whisper-cli.exe"
        } else {
            "/nonexistent/stt-plugin-whisper-missing/whisper-cli"
        };
        let err = WhisperCliEngine::discover(Some(missing), Some("/tmp/model.bin"))
            .expect_err("missing path must fail");
        assert_eq!(err.code(), codes::STT_ENGINE_NOT_FOUND);
    }

    #[test]
    fn discover_missing_model_fails_closed() {
        // Even with a fake exe path that does not exist, engine fails first;
        // model absence with empty path:
        let err = WhisperCliEngine::discover(None, None).expect_err("no model");
        // Either engine not found or model not found — both fail closed, no download.
        assert!(
            err.code() == codes::STT_ENGINE_NOT_FOUND || err.code() == codes::STT_MODEL_NOT_FOUND,
            "code={}",
            err.code()
        );
    }

    #[test]
    fn argv_includes_model_and_file() {
        let args = WhisperCliEngine::build_transcribe_args(
            Utf8Path::new("ggml-base.bin"),
            Utf8Path::new("clip.wav"),
            Some("en"),
        );
        assert!(args.contains(&"-m".into()));
        assert!(args.contains(&"ggml-base.bin".into()));
        assert!(args.contains(&"-f".into()));
        assert!(args.contains(&"clip.wav".into()));
        assert!(args.contains(&"-l".into()));
        assert!(args.contains(&"en".into()));
        assert!(args.contains(&"-nt".into()));
    }
}
