//! Resumable `transcribe` job.

use std::time::Instant;

use matter_core::{
    transcript_status, ApplyTranscriptInput, AuditEventInput, Matter, TranscriptApplyResult,
    TranscriptCandidate,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::decode::decode_to_whisper_wav;
use crate::detect::{
    estimate_wav_duration_secs, is_audio_meta, is_video_meta, is_whisper_compliant_wav,
};
use crate::engine::{SttEngine, WhisperCliEngine};
use crate::error::{codes, Error, Result};
use crate::ffmpeg::{convert_to_pcm_wav, resolve_ffmpeg};
use crate::limits::{engines, MAX_TRANSCRIPT_TEXT_BYTES, TRUNCATION_MARKER};
use crate::params::SttParams;
use crate::temp::{purge_stt_temp_dir, SttTempFile};

/// Job kind string for process-runner.
pub const JOB_KIND_TRANSCRIBE: &str = "transcribe";
/// Checkpoint stage name.
pub const TRANSCRIBE_STAGE: &str = "transcribe";

/// Summary counts after a transcribe run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SttSummary {
    pub completed_count: u64,
    pub transcript_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
}

/// Outcome of [`run_transcribe`] / [`run_transcribe_with_engine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttOutcome {
    Succeeded(SttSummary),
    Paused(SttSummary),
    Failed {
        message: String,
        summary: SttSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    transcript_count: u64,
    skipped_count: u64,
    error_count: u64,
    params: serde_json::Value,
}

/// Reject oversized native length before any full CAS load.
pub fn reject_oversized_native_len(len: u64, max: u64) -> Result<()> {
    if len > max {
        return Err(Error::limit(format!("native size {len} exceeds max {max}")));
    }
    Ok(())
}

/// Truncate at a UTF-8 char boundary.
pub fn truncate_transcript_text(mut text: String) -> String {
    if text.len() <= MAX_TRANSCRIPT_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_TRANSCRIPT_TEXT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATION_MARKER);
    text
}

/// Run transcription on `matter` for the runner-created `job_id` (production path).
///
/// Production only allows **whisper_cli** / **auto**. `"engine": "mock"` is
/// rejected. Tests inject via [`run_transcribe_with_engine`]. Fails closed when
/// `enabled=false`. Never downloads model weights.
pub fn run_transcribe(
    matter: &Matter,
    job_id: &str,
    params: &SttParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<SttOutcome> {
    if !params.enabled {
        return fail_disabled(matter, job_id, params);
    }
    if let Err(e) = params.validate() {
        return fail_preflight(matter, job_id, params, Error::InvalidParams(e));
    }

    let _purged = purge_stt_temp_dir(matter.root())?;

    let engine_name = params.engine.to_ascii_lowercase();
    if engine_name == "mock" || engine_name == engines::MOCK {
        return fail_preflight(
            matter,
            job_id,
            params,
            Error::InvalidParams(
                "engine=mock is not allowed on the production STT path; use whisper_cli".into(),
            ),
        );
    }
    if engine_name != engines::WHISPER_CLI
        && engine_name != engines::AUTO
        && engine_name != "whisper"
    {
        return fail_preflight(
            matter,
            job_id,
            params,
            Error::InvalidParams(format!(
                "unknown STT engine '{engine_name}' (production supports whisper_cli / auto)"
            )),
        );
    }

    let engine = match WhisperCliEngine::discover(
        params.whisper_cli_path.as_deref(),
        params.model_path.as_deref(),
    ) {
        Ok(e) => e,
        Err(e) => return fail_preflight(matter, job_id, params, e),
    };
    run_transcribe_with_engine(matter, job_id, params, &engine, cancel, progress)
}

/// Run transcription with an injected engine (tests + production after resolve).
pub fn run_transcribe_with_engine(
    matter: &Matter,
    job_id: &str,
    params: &SttParams,
    engine: &dyn SttEngine,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<SttOutcome> {
    let started = Instant::now();

    if !params.enabled {
        return fail_disabled(matter, job_id, params);
    }
    params.validate().map_err(Error::InvalidParams)?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    let engine_label = format!("{} {}", engine.engine_id(), engine.model_id());

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "transcribe.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "enabled": effective.enabled,
            "engine": engine.engine_id(),
            "model": engine.model_id(),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_inner(
        matter,
        job_id,
        &effective,
        engine,
        &engine_label,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(SttOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "transcribe.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "transcript_count": s.transcript_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "completed_count": s.completed_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                    "engine": engine.engine_id(),
                    "enabled": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(SttOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(SttOutcome::Paused(_)) => {}
        Ok(SttOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "transcribe.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "transcript_count": summary.transcript_count,
                    "enabled": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(Error::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "transcribe.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": e.to_string(),
                    "enabled": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(Error::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn fail_disabled(matter: &Matter, job_id: &str, params: &SttParams) -> Result<SttOutcome> {
    let msg = "STT disabled — enable local STT in Settings before running";
    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "transcribe.fail".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "error": msg,
            "enabled": false,
            "params": serde_json::to_value(params).unwrap_or(json!({})),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    });
    Ok(SttOutcome::Failed {
        message: msg.into(),
        summary: SttSummary::default(),
    })
}

fn fail_preflight(
    matter: &Matter,
    job_id: &str,
    params: &SttParams,
    err: Error,
) -> Result<SttOutcome> {
    let msg = err.short_message();
    let code = err.code();
    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "transcribe.fail".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "error": msg,
            "code": code,
            "enabled": params.enabled,
            "params": serde_json::to_value(params).unwrap_or(json!({})),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    });
    Err(err)
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, TRANSCRIBE_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(Error::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(call_site: &SttParams, prior: Option<&CheckpointCursor>) -> Result<SttParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<SttParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(Error::Other(format!("checkpoint params unreadable: {e}")));
                }
            }
        }
    }
    Ok(call_site.clone())
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &SttParams,
    engine: &dyn SttEngine,
    engine_label: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<SttOutcome> {
    let mut summary = SttSummary::default();
    let mut cursor_index = 0u64;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        summary.completed_count = p.completed_count;
        summary.transcript_count = p.transcript_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
    }

    let batch = params.batch_size.max(1);
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            write_checkpoint(matter, job_id, cursor_index, &summary, params_json, None)?;
            progress(summary.completed_count);
            return Ok(SttOutcome::Paused(summary));
        }

        let candidates =
            matter.list_transcript_candidates(cursor_index, batch as u64, params.reset)?;
        if candidates.is_empty() {
            break;
        }

        for cand in candidates {
            if cancel.map(|c| c()).unwrap_or(false) {
                write_checkpoint(
                    matter,
                    job_id,
                    cursor_index,
                    &summary,
                    params_json,
                    Some(&cand.id),
                )?;
                progress(summary.completed_count);
                return Ok(SttOutcome::Paused(summary));
            }

            match process_one(
                matter,
                job_id,
                &cand,
                params,
                engine,
                engine_label,
                cancel,
                &mut summary,
            )? {
                ProcessOneResult::Done => {
                    cursor_index += 1;
                    progress(summary.completed_count);
                    write_checkpoint(
                        matter,
                        job_id,
                        cursor_index,
                        &summary,
                        params_json,
                        Some(&cand.id),
                    )?;
                }
                ProcessOneResult::CancelledMidItem => {
                    // Do not advance cursor — resume retries this item.
                    write_checkpoint(
                        matter,
                        job_id,
                        cursor_index,
                        &summary,
                        params_json,
                        Some(&cand.id),
                    )?;
                    progress(summary.completed_count);
                    return Ok(SttOutcome::Paused(summary));
                }
            }
        }
    }

    Ok(SttOutcome::Succeeded(summary))
}

enum ProcessOneResult {
    Done,
    CancelledMidItem,
}

/// Digest skip only when status is **done** for the matching native.
///
/// `skipped` is **not** terminal for digest skip — tool-missing skips (e.g. no
/// ffmpeg) must be retriable when the tool becomes available. Permanent
/// unsupported skips may re-classify cheaply each run.
fn already_done(cand: &TranscriptCandidate, native_sha: &str, reset: bool) -> bool {
    if reset {
        return false;
    }
    cand.transcript_native_sha256.as_deref() == Some(native_sha)
        && cand.transcript_status.as_deref() == Some(transcript_status::DONE)
}

#[allow(clippy::too_many_arguments)]
fn process_one(
    matter: &Matter,
    job_id: &str,
    cand: &TranscriptCandidate,
    params: &SttParams,
    engine: &dyn SttEngine,
    engine_label: &str,
    cancel: Option<&dyn Fn() -> bool>,
    summary: &mut SttSummary,
) -> Result<ProcessOneResult> {
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    };

    if already_done(cand, native_sha, params.reset) {
        // Job-level skip count only — leave item `transcript_status=done` intact
        // so permanent digest skip remains stable across runs.
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    }

    match matter.cas_len(native_sha) {
        Ok(len) => {
            if let Err(e) = reject_oversized_native_len(len, params.max_native_bytes) {
                record_error(matter, job_id, &cand.id, engine_label, engine, params, &e)?;
                summary.error_count += 1;
                summary.completed_count += 1;
                return Ok(ProcessOneResult::Done);
            }
        }
        Err(e) => {
            record_error(
                matter,
                job_id,
                &cand.id,
                engine_label,
                engine,
                params,
                &Error::Other(format!("CAS stat: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(ProcessOneResult::Done);
        }
    }

    let native_bytes = match matter.get_bytes_capped(native_sha, params.max_native_bytes) {
        Ok(b) => b,
        Err(e) => {
            let err = {
                let msg = e.to_string();
                if msg.contains("exceeds cap") {
                    Error::limit(msg)
                } else {
                    Error::Other(format!("CAS read: {e}"))
                }
            };
            record_error(matter, job_id, &cand.id, engine_label, engine, params, &err)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(ProcessOneResult::Done);
        }
    };

    let is_video = is_video_meta(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        cand.file_category.as_deref(),
    );
    let is_audio = is_audio_meta(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        cand.file_category.as_deref(),
    );

    // Duration cap for canonical WAV before convert/STT.
    // Residual: non-WAV duration is enforced after ffmpeg conversion (see prep).
    if let Some(secs) = estimate_wav_duration_secs(&native_bytes) {
        if secs > params.max_duration_secs as f64 {
            record_error(
                matter,
                job_id,
                &cand.id,
                engine_label,
                engine,
                params,
                &Error::limit(format!(
                    "duration {secs:.1}s exceeds max_duration_secs {}",
                    params.max_duration_secs
                )),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(ProcessOneResult::Done);
        }
    }

    if !is_audio && !is_video {
        matter.apply_transcript_text(ApplyTranscriptInput {
            item_id: cand.id.clone(),
            force: true,
            text: None,
            engine: Some(engine_label.into()),
            model: Some(engine.model_id().into()),
            language: Some(params.language.clone()),
            status: Some(transcript_status::SKIPPED.into()),
            error: Some("stt_not_audio_or_video".into()),
            source_native_sha256: Some(native_sha.into()),
            job_id: Some(job_id.into()),
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    }

    match prepare_and_transcribe(
        matter,
        cand,
        params,
        engine,
        &native_bytes,
        is_video,
        cancel,
    ) {
        Ok(tr) => {
            if tr.text.trim().is_empty() {
                record_error(
                    matter,
                    job_id,
                    &cand.id,
                    engine_label,
                    engine,
                    params,
                    &Error::Engine(codes::STT_EMPTY_TEXT.into()),
                )?;
                summary.error_count += 1;
                summary.completed_count += 1;
                return Ok(ProcessOneResult::Done);
            }
            let text = truncate_transcript_text(tr.text);
            let apply = matter.apply_transcript_text(ApplyTranscriptInput {
                item_id: cand.id.clone(),
                force: params.reset,
                text: Some(text),
                engine: Some(engine_label.into()),
                model: Some(engine.model_id().into()),
                language: tr.language.or_else(|| Some(params.language.clone())),
                status: Some(transcript_status::DONE.into()),
                error: None,
                source_native_sha256: Some(native_sha.into()),
                job_id: Some(job_id.into()),
            })?;
            match apply {
                TranscriptApplyResult::Skipped => summary.skipped_count += 1,
                TranscriptApplyResult::Applied { .. } => summary.transcript_count += 1,
                TranscriptApplyResult::Error { .. } => summary.error_count += 1,
            }
            summary.completed_count += 1;
            Ok(ProcessOneResult::Done)
        }
        Err(e) if e.is_cancelled() => Ok(ProcessOneResult::CancelledMidItem),
        Err(e) => {
            // Retryable tool-missing: record skipped **without** claiming native
            // digest so a later run with ffmpeg available is not permanently
            // digest-skipped (spec §3.6 — permanent digest skip only for done).
            if e.code() == codes::STT_FFMPEG_NOT_FOUND || e.code() == codes::STT_VIDEO_NEEDS_FFMPEG
            {
                matter.apply_transcript_text(ApplyTranscriptInput {
                    item_id: cand.id.clone(),
                    force: true,
                    text: None,
                    engine: Some(engine_label.into()),
                    model: Some(engine.model_id().into()),
                    language: Some(params.language.clone()),
                    status: Some(transcript_status::SKIPPED.into()),
                    error: Some(format!("{}: {}", e.code(), e.short_message())),
                    source_native_sha256: None,
                    job_id: Some(job_id.into()),
                })?;
                summary.skipped_count += 1;
            } else {
                record_error(matter, job_id, &cand.id, engine_label, engine, params, &e)?;
                summary.error_count += 1;
            }
            summary.completed_count += 1;
            Ok(ProcessOneResult::Done)
        }
    }
}

fn prepare_and_transcribe(
    matter: &Matter,
    cand: &TranscriptCandidate,
    params: &SttParams,
    engine: &dyn SttEngine,
    native_bytes: &[u8],
    is_video: bool,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<crate::engine::TranscriptResult> {
    if cancel.map(|c| c()).unwrap_or(false) {
        return Err(Error::Cancelled);
    }

    // Materialize native to temp (never mutate source media / CAS).
    let suffix = cand
        .path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).extension())
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_else(|| ".bin".into());
    let mut source_temp = SttTempFile::new_in(matter.root(), &suffix)?;
    source_temp.write_all(native_bytes)?;

    if is_whisper_compliant_wav(native_bytes) {
        // Already Whisper-target PCM WAV — use as-is.
        let result = engine.transcribe_wav_path(
            source_temp.path(),
            Some(params.language.as_str()),
            cancel,
        )?;
        drop(source_temp);
        return Ok(result);
    }

    // Common audio: pure-Rust Symphonia decode + linear resample (no ffmpeg).
    if !is_video {
        match decode_to_whisper_wav(native_bytes) {
            Ok(pcm_wav) => {
                if let Some(secs) = estimate_wav_duration_secs(&pcm_wav) {
                    if secs > params.max_duration_secs as f64 {
                        return Err(Error::limit(format!(
                            "duration {secs:.1}s exceeds max_duration_secs {}",
                            params.max_duration_secs
                        )));
                    }
                }
                let mut out_wav = SttTempFile::new_in(matter.root(), ".wav")?;
                out_wav.write_all(&pcm_wav)?;
                let result = engine.transcribe_wav_path(
                    out_wav.path(),
                    Some(params.language.as_str()),
                    cancel,
                )?;
                drop(out_wav);
                drop(source_temp);
                return Ok(result);
            }
            Err(_decode_err) => {
                // Fall through to ffmpeg for formats Symphonia cannot handle.
            }
        }
    }

    // Video / complex containers: ffmpeg with locked flags (cancellable mid-wait).
    let ffmpeg = match resolve_ffmpeg(params.ffmpeg_path.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            if is_video {
                return Err(Error::FfmpegNotFound(format!(
                    "video requires ffmpeg ({}) — {}",
                    codes::STT_VIDEO_NEEDS_FFMPEG,
                    e.short_message()
                )));
            }
            return Err(e);
        }
    };
    let out_wav = SttTempFile::new_in(matter.root(), ".wav")?;
    convert_to_pcm_wav(&ffmpeg, source_temp.path(), out_wav.path(), cancel)?;

    // Enforce max_duration_secs on converted WAV (pre-convert probe residual for non-WAV).
    enforce_converted_wav_duration(out_wav.path(), params.max_duration_secs)?;

    let result =
        engine.transcribe_wav_path(out_wav.path(), Some(params.language.as_str()), cancel)?;
    drop(out_wav);
    drop(source_temp);
    Ok(result)
}

/// After ffmpeg conversion, re-read the WAV header and enforce duration cap.
fn enforce_converted_wav_duration(path: &camino::Utf8Path, max_duration_secs: u64) -> Result<()> {
    // Header-only read is enough for canonical PCM layout (fmt @ 12, data size @ 40).
    let mut hdr = [0u8; 44];
    let mut f = std::fs::File::open(path.as_std_path())?;
    use std::io::Read;
    let n = f.read(&mut hdr)?;
    if n < 44 {
        return Ok(()); // unreadable — let STT fail later if needed
    }
    if let Some(secs) = estimate_wav_duration_secs(&hdr) {
        if secs > max_duration_secs as f64 {
            return Err(Error::limit(format!(
                "converted duration {secs:.1}s exceeds max_duration_secs {max_duration_secs}"
            )));
        }
    }
    Ok(())
}

fn record_error(
    matter: &Matter,
    job_id: &str,
    item_id: &str,
    engine_label: &str,
    engine: &dyn SttEngine,
    params: &SttParams,
    err: &Error,
) -> Result<()> {
    matter.apply_transcript_text(ApplyTranscriptInput {
        item_id: item_id.into(),
        force: true,
        text: None,
        engine: Some(engine_label.into()),
        model: Some(engine.model_id().into()),
        language: Some(params.language.clone()),
        status: Some(transcript_status::FAILED.into()),
        error: Some(format!("{}: {}", err.code(), err.short_message())),
        source_native_sha256: None,
        job_id: Some(job_id.into()),
    })?;
    matter
        .record_item_error(matter_core::ItemErrorInput {
            item_id: Some(item_id.into()),
            source_id: None,
            job_id: Some(job_id.into()),
            stage: TRANSCRIBE_STAGE.into(),
            code: err.code().into(),
            message: err.short_message(),
            detail: None,
        })
        .map_err(|e| Error::Other(format!("record_item_error failed: {e}")))?;
    Ok(())
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &SttSummary,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        transcript_count: summary.transcript_count,
        skipped_count: summary.skipped_count,
        error_count: summary.error_count,
        params: params_json.clone(),
    };
    let cursor_json = serde_json::to_string(&cursor).unwrap_or_else(|_| "{}".into());
    matter.put_checkpoint(
        job_id,
        TRANSCRIBE_STAGE,
        &cursor_json,
        summary.completed_count as i64,
    )?;
    Ok(())
}

/// Tiny valid 16 kHz mono s16le WAV (~0.1s silence) for fixtures / tests.
pub fn minimal_wav_bytes() -> Vec<u8> {
    let sample_rate: u32 = 16_000;
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let num_samples: u32 = 1600; // 0.1s
    let data_size = num_samples * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.resize(out.len() + data_size as usize, 0); // silence
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_oversized_native_len_unit() {
        assert!(reject_oversized_native_len(10, 10).is_ok());
        let err = reject_oversized_native_len(11, 10).unwrap_err();
        assert_eq!(err.code(), "stt_limit_exceeded");
    }

    #[test]
    fn already_done_respects_reset() {
        let cand = TranscriptCandidate {
            id: "i1".into(),
            path: Some("a.wav".into()),
            mime_type: None,
            native_sha256: Some("abc".into()),
            text_sha256: Some("txt".into()),
            transcript_native_sha256: Some("abc".into()),
            transcript_status: Some("done".into()),
            file_category: None,
            parent_item_id: None,
            role: None,
        };
        assert!(already_done(&cand, "abc", false));
        assert!(!already_done(&cand, "abc", true));
        assert!(!already_done(&cand, "other", false));
    }

    #[test]
    fn already_done_does_not_treat_skipped_as_terminal() {
        let cand = TranscriptCandidate {
            id: "i1".into(),
            path: Some("a.mp4".into()),
            mime_type: Some("video/mp4".into()),
            native_sha256: Some("abc".into()),
            text_sha256: None,
            transcript_native_sha256: Some("abc".into()),
            transcript_status: Some("skipped".into()),
            file_category: Some("video".into()),
            parent_item_id: None,
            role: None,
        };
        assert!(
            !already_done(&cand, "abc", false),
            "skipped must not permanently digest-skip (ffmpeg may become available)"
        );
    }

    #[test]
    fn minimal_wav_is_compliant() {
        let b = minimal_wav_bytes();
        assert!(crate::detect::looks_like_wav(&b));
        assert!(crate::detect::is_whisper_compliant_wav(&b));
    }
}
