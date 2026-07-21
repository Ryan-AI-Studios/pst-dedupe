//! ffmpeg CLI conversion to Whisper-hard PCM WAV (LOCKED flags).

use std::path::{Path, PathBuf};
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};
use crate::job_object::{spawn_and_wait_cancellable, CancellableWaitError};
use crate::limits::{TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// Build ffmpeg argv to convert `input` → 16 kHz mono s16le PCM WAV at `output`.
///
/// **LOCKED flags** (spec §3.3.1): must include `-ar 16000`, `-ac 1`,
/// `-c:a pcm_s16le`. Never demux-as-is.
pub fn build_ffmpeg_pcm_args(input: &Utf8Path, output: &Utf8Path) -> Vec<String> {
    vec![
        "-y".into(),
        "-i".into(),
        input.as_str().to_string(),
        "-ar".into(),
        TARGET_SAMPLE_RATE.to_string(),
        "-ac".into(),
        TARGET_CHANNELS.to_string(),
        "-c:a".into(),
        "pcm_s16le".into(),
        output.as_str().to_string(),
    ]
}

/// Assert helper used by unit tests (and docs).
pub fn args_contain_locked_pcm_flags(args: &[String]) -> bool {
    let has_ar = args.windows(2).any(|w| w[0] == "-ar" && w[1] == "16000");
    let has_ac = args.windows(2).any(|w| w[0] == "-ac" && w[1] == "1");
    let has_codec = args
        .windows(2)
        .any(|w| w[0] == "-c:a" && w[1] == "pcm_s16le")
        || args.iter().any(|a| a == "pcm_s16le");
    has_ar && has_ac && has_codec
}

/// Resolve ffmpeg executable: settings path → PATH.
pub fn resolve_ffmpeg(settings_path: Option<&str>) -> Result<Utf8PathBuf> {
    if let Some(p) = settings_path.map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(p);
        if path.is_file() {
            return Utf8PathBuf::from_path_buf(path.to_path_buf())
                .map_err(|_| Error::FfmpegNotFound(format!("ffmpeg path is not UTF-8: {p}")));
        }
        return Err(Error::FfmpegNotFound(format!("ffmpeg_path not found: {p}")));
    }
    if let Some(found) = find_on_path("ffmpeg") {
        return Utf8PathBuf::from_path_buf(found)
            .map_err(|_| Error::FfmpegNotFound("ffmpeg on PATH is not UTF-8".into()));
    }
    #[cfg(windows)]
    {
        if let Some(found) = find_on_path("ffmpeg.exe") {
            return Utf8PathBuf::from_path_buf(found)
                .map_err(|_| Error::FfmpegNotFound("ffmpeg.exe on PATH is not UTF-8".into()));
        }
    }
    Err(Error::FfmpegNotFound(
        "ffmpeg not found (set Settings path or install and add to PATH)".into(),
    ))
}

/// Convert input media to 16 kHz mono s16le WAV via ffmpeg under a Job Object.
///
/// When `cancel` returns true mid-wait, the Job Object is terminated and
/// [`Error::Cancelled`] is returned.
pub fn convert_to_pcm_wav(
    ffmpeg_exe: &Utf8Path,
    input: &Utf8Path,
    output: &Utf8Path,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<()> {
    let args = build_ffmpeg_pcm_args(input, output);
    let out = match spawn_and_wait_cancellable(ffmpeg_exe.as_str(), &args, None, cancel) {
        Ok(o) => o,
        Err(CancellableWaitError::Cancelled) => return Err(Error::Cancelled),
        Err(CancellableWaitError::Io(e)) => {
            return Err(Error::Engine(format!("spawn ffmpeg: {e}")));
        }
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Engine(format!(
            "ffmpeg failed ({}): {}",
            out.status,
            stderr.trim()
        )));
    }
    if !output.as_std_path().is_file() {
        return Err(Error::Engine(
            "ffmpeg completed but output WAV is missing".into(),
        ));
    }
    Ok(())
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

/// Probe whether ffmpeg appears resolvable (Desk enablement helper).
pub fn ffmpeg_looks_available(settings_path: Option<&str>) -> bool {
    resolve_ffmpeg(settings_path).is_ok()
}

/// Cheap version probe (does not download models).
pub fn ffmpeg_version(ffmpeg_exe: &Utf8Path) -> Result<String> {
    let mut cmd = Command::new(ffmpeg_exe.as_str());
    cmd.arg("-version");
    scrub_proxy_env(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| Error::FfmpegNotFound(format!("failed to run {}: {e}", ffmpeg_exe)))?;
    if !out.status.success() {
        return Err(Error::Engine(format!(
            "ffmpeg -version failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap_or("ffmpeg").trim();
    Ok(first.to_string())
}

fn scrub_proxy_env(cmd: &mut Command) {
    cmd.env_remove("HTTP_PROXY");
    cmd.env_remove("HTTPS_PROXY");
    cmd.env_remove("http_proxy");
    cmd.env_remove("https_proxy");
    cmd.env_remove("ALL_PROXY");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_pcm_flags_present() {
        let args = build_ffmpeg_pcm_args(Utf8Path::new("in.mp4"), Utf8Path::new("out.wav"));
        assert!(
            args_contain_locked_pcm_flags(&args),
            "args must force 16k mono s16le: {args:?}"
        );
        assert!(args.contains(&"-ar".into()));
        assert!(args.contains(&"16000".into()));
        assert!(args.contains(&"-ac".into()));
        assert!(args.contains(&"1".into()));
        assert!(args.contains(&"pcm_s16le".into()));
        // Forbidden: demux-as-is without coerce flags would omit these.
        assert!(!args
            .iter()
            .any(|a| a == "-c" && !args.contains(&"-c:a".into())));
    }

    #[test]
    fn missing_ffmpeg_path_is_error() {
        let missing = if cfg!(windows) {
            r"C:\nonexistent\stt-plugin-ffmpeg-missing\ffmpeg.exe"
        } else {
            "/nonexistent/stt-plugin-ffmpeg-missing/ffmpeg"
        };
        let err = resolve_ffmpeg(Some(missing)).expect_err("missing");
        assert_eq!(err.code(), crate::error::codes::STT_FFMPEG_NOT_FOUND);
    }
}
