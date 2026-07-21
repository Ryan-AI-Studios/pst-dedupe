//! Pure-Rust common-audio prep via Symphonia → Whisper-hard PCM WAV.
//!
//! Target: 16 kHz mono s16le WAV. Used for wav/flac/ogg/mp3 (and non-canonical
//! PCM WAV) so STT works without ffmpeg. Video / containers Symphonia cannot
//! handle fall back to the ffmpeg path in [`crate::run`].

use std::io::Cursor;

use symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{Error, Result};
use crate::limits::{TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// Decode common audio bytes to Whisper-compliant 16 kHz mono s16le WAV.
///
/// Supports formats enabled via crate features (wav/flac/ogg+vorbis/mp3/pcm).
/// Linear-resamples when the source rate ≠ 16 kHz; mixes channels to mono.
pub fn decode_to_whisper_wav(bytes: &[u8]) -> Result<Vec<u8>> {
    let (samples, sample_rate) = decode_to_mono_f32(bytes)?;
    if samples.is_empty() {
        return Err(Error::Engine("symphonia: decoded zero samples".into()));
    }
    let mono_16k = if sample_rate == TARGET_SAMPLE_RATE {
        samples
    } else {
        linear_resample_mono(&samples, sample_rate, TARGET_SAMPLE_RATE)
    };
    let pcm: Vec<i16> = mono_16k.iter().map(|&s| float_to_i16(s)).collect();
    Ok(write_pcm_s16le_wav(
        &pcm,
        TARGET_SAMPLE_RATE,
        TARGET_CHANNELS,
    ))
}

/// Decode all packets to mono f32 + source sample rate.
fn decode_to_mono_f32(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let hint = Hint::new();
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| Error::Engine(format!("symphonia probe: {e}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Engine("symphonia: no decodable audio track".into()))?
        .clone();

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| Error::Engine("symphonia: missing sample rate".into()))?;
    if sample_rate == 0 {
        return Err(Error::Engine("symphonia: zero sample rate".into()));
    }

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| Error::Engine(format!("symphonia decoder: {e}")))?;

    let track_id = track.id;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1)
        .max(1);

    let mut mono = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut buf_spec: Option<SignalSpec> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => {
                let msg = e.to_string().to_ascii_lowercase();
                if msg.contains("end of stream") || msg.contains("eof") {
                    break;
                }
                return Err(Error::Engine(format!("symphonia packet: {e}")));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                append_decoded_mono(
                    &audio_buf,
                    &mut sample_buf,
                    &mut buf_spec,
                    channels,
                    &mut mono,
                )?;
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                let msg = e.to_string().to_ascii_lowercase();
                if msg.contains("end of stream") || msg.contains("eof") {
                    break;
                }
                return Err(Error::Engine(format!("symphonia decode: {e}")));
            }
        }
    }

    if mono.is_empty() {
        return Err(Error::Engine("symphonia: no PCM samples decoded".into()));
    }
    Ok((mono, sample_rate))
}

fn append_decoded_mono(
    audio_buf: &AudioBufferRef<'_>,
    sample_buf: &mut Option<SampleBuffer<f32>>,
    buf_spec: &mut Option<SignalSpec>,
    channels: usize,
    mono: &mut Vec<f32>,
) -> Result<()> {
    let spec = *audio_buf.spec();
    let capacity = audio_buf.capacity() as u64;
    let needs_new = match buf_spec {
        None => true,
        Some(prev) => prev.rate != spec.rate || prev.channels.count() != spec.channels.count(),
    };
    if needs_new {
        *sample_buf = Some(SampleBuffer::<f32>::new(capacity, spec));
        *buf_spec = Some(spec);
    }
    let buf = sample_buf
        .as_mut()
        .ok_or_else(|| Error::Engine("symphonia: sample buffer missing".into()))?;
    buf.copy_interleaved_ref(audio_buf.clone());
    let interleaved = buf.samples();
    let ch = channels.max(1);
    if ch == 1 {
        mono.extend_from_slice(interleaved);
    } else {
        for frame in interleaved.chunks_exact(ch) {
            let sum: f32 = frame.iter().sum();
            mono.push(sum / ch as f32);
        }
    }
    Ok(())
}

/// Linear resample mono f32 from `from_rate` → `to_rate`.
pub fn linear_resample_mono(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = f64::from(from_rate) / f64::from(to_rate);
    let out_len = ((samples.len() as f64) / ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let s0 = samples.get(idx).copied().unwrap_or(0.0);
        let s1 = samples.get(idx + 1).copied().unwrap_or(s0);
        out.push(s0 + (s1 - s0) * frac);
    }
    out
}

fn float_to_i16(s: f32) -> i16 {
    let clamped = s.clamp(-1.0, 1.0);
    if clamped >= 0.0 {
        (clamped * f32::from(i16::MAX)) as i16
    } else {
        (clamped * -f32::from(i16::MIN)) as i16
    }
}

/// Write a canonical PCM s16le WAV (fmt @ 12, data @ 36).
pub fn write_pcm_s16le_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let data_size = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Build a stereo 44.1 kHz s16le WAV (~0.1s silence) for no-ffmpeg path tests.
pub fn stereo_44100_wav_bytes() -> Vec<u8> {
    let sample_rate: u32 = 44_100;
    let channels: u16 = 2;
    let num_frames: usize = 4_410; // 0.1s
    let samples = vec![0i16; num_frames * channels as usize];
    write_pcm_s16le_wav(&samples, sample_rate, channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::is_whisper_compliant_wav;

    #[test]
    fn stereo_44100_becomes_whisper_compliant() {
        let src = stereo_44100_wav_bytes();
        assert!(!is_whisper_compliant_wav(&src));
        let out = decode_to_whisper_wav(&src).expect("decode stereo wav");
        assert!(
            is_whisper_compliant_wav(&out),
            "symphonia path must emit 16k mono s16le"
        );
        // ~0.1s at 16k ≈ 1600 samples → 3200 bytes + 44 header.
        assert!(
            out.len() > 44 + 1000,
            "expected non-trivial PCM, got {}",
            out.len()
        );
    }

    #[test]
    fn linear_resample_identity() {
        let s = vec![0.0, 0.5, -0.5, 1.0];
        let out = linear_resample_mono(&s, 16_000, 16_000);
        assert_eq!(out, s);
    }

    #[test]
    fn linear_resample_halves_rate() {
        // 4 samples @ 32k → ~2 samples @ 16k
        let s = vec![0.0, 1.0, 0.0, -1.0];
        let out = linear_resample_mono(&s, 32_000, 16_000);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn write_wav_header_canonical() {
        let wav = write_pcm_s16le_wav(&[0, 100, -100], 16_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(is_whisper_compliant_wav(&wav));
    }
}
