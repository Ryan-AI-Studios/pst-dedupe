//! Audio / video candidate sniffing for STT.

/// Audio extensions eligible for STT (lowercase, no dot).
pub const AUDIO_EXTS: &[&str] = &["wav", "mp3", "m4a", "flac", "ogg"];

/// Video extensions (require ffmpeg for conversion).
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "webm"];

/// True when path/mime/file_category look like audio.
pub fn is_audio_meta(path: Option<&str>, mime: Option<&str>, file_category: Option<&str>) -> bool {
    if file_category
        .map(|c| c.eq_ignore_ascii_case("audio"))
        .unwrap_or(false)
    {
        return true;
    }
    if mime
        .map(|m| m.to_ascii_lowercase().starts_with("audio/"))
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(p) = path {
        let lower = p.to_ascii_lowercase();
        for ext in AUDIO_EXTS {
            if lower.ends_with(&format!(".{ext}")) {
                return true;
            }
        }
    }
    false
}

/// True when path/mime/file_category look like video.
pub fn is_video_meta(path: Option<&str>, mime: Option<&str>, file_category: Option<&str>) -> bool {
    if file_category
        .map(|c| c.eq_ignore_ascii_case("video"))
        .unwrap_or(false)
    {
        return true;
    }
    if mime
        .map(|m| m.to_ascii_lowercase().starts_with("video/"))
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(p) = path {
        let lower = p.to_ascii_lowercase();
        for ext in VIDEO_EXTS {
            if lower.ends_with(&format!(".{ext}")) {
                return true;
            }
        }
    }
    false
}

/// WAV RIFF magic.
pub fn looks_like_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

/// True when bytes are already 16 kHz mono PCM s16le WAV (Whisper target).
pub fn is_whisper_compliant_wav(bytes: &[u8]) -> bool {
    wav_pcm_meta(bytes).is_some_and(|m| {
        m.audio_format == 1 && m.channels == 1 && m.sample_rate == 16_000 && m.bits_per_sample == 16
    })
}

/// Minimal PCM WAV header fields (canonical `fmt ` at offset 12).
#[derive(Debug, Clone, Copy)]
pub struct WavPcmMeta {
    pub audio_format: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub data_bytes: u32,
}

/// Parse canonical PCM WAV meta when layout is standard.
pub fn wav_pcm_meta(bytes: &[u8]) -> Option<WavPcmMeta> {
    if !looks_like_wav(bytes) || bytes.len() < 44 {
        return None;
    }
    if &bytes[12..16] != b"fmt " {
        return None;
    }
    let audio_format = u16::from_le_bytes([bytes[20], bytes[21]]);
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
    // data chunk size at typical offset 40 when fmt is 16 bytes.
    let data_bytes = if &bytes[36..40] == b"data" {
        u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]])
    } else {
        bytes.len().saturating_sub(44) as u32
    };
    Some(WavPcmMeta {
        audio_format,
        channels,
        sample_rate,
        bits_per_sample,
        data_bytes,
    })
}

/// Estimate duration in seconds for a canonical PCM WAV (None if unreadable).
pub fn estimate_wav_duration_secs(bytes: &[u8]) -> Option<f64> {
    let m = wav_pcm_meta(bytes)?;
    if m.sample_rate == 0 || m.channels == 0 || m.bits_per_sample == 0 {
        return None;
    }
    let bytes_per_sec =
        f64::from(m.sample_rate) * f64::from(m.channels) * f64::from(m.bits_per_sample) / 8.0;
    if bytes_per_sec <= 0.0 {
        return None;
    }
    Some(f64::from(m.data_bytes) / bytes_per_sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_meta_path() {
        assert!(is_audio_meta(Some("a.WAV"), None, None));
        assert!(is_audio_meta(Some("x.mp3"), None, None));
        assert!(!is_audio_meta(Some("a.mp4"), None, None));
    }

    #[test]
    fn video_meta() {
        assert!(is_video_meta(Some("a.mp4"), None, None));
        assert!(is_video_meta(None, Some("video/mp4"), None));
        assert!(!is_video_meta(Some("a.wav"), None, None));
    }
}
