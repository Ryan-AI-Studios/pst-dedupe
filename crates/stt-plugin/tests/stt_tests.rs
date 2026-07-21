//! Integration tests for stt-plugin (spec §3) — no Whisper weights / no ffmpeg.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use matter_core::{
    item_role, transcript_status, ApplyTranscriptInput, ItemInput, Matter, TRANSCRIPT_MARKER,
};
use stt_plugin::{
    args_contain_locked_pcm_flags, build_ffmpeg_pcm_args, is_whisper_compliant_wav,
    kill_on_close_limit_flags, looks_like_wav, minimal_wav_bytes, purge_stt_temp_dir,
    run_transcribe, run_transcribe_with_engine, spawn_and_wait_cancellable, stereo_44100_wav_bytes,
    CancellableWaitError, MockSttEngine, SttOutcome, SttParams, SttTempFile, JOB_KIND_TRANSCRIBE,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

fn make_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let dir = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join(name), name).unwrap();
    (dir, matter)
}

fn enabled_mock_params() -> SttParams {
    SttParams {
        enabled: true,
        engine: "mock".into(),
        batch_size: 10,
        ..SttParams::default()
    }
}

#[test]
fn mock_audio_sets_transcript_and_text_cas() {
    let (_tmp, matter) = make_matter("stt-wav");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("voicemail.wav".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            file_category: Some("audio".into()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockSttEngine::new("MOCK_SPEECH_MARKER unique phrase");
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let outcome = run_transcribe_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        SttOutcome::Succeeded(s) => {
            assert_eq!(s.transcript_count, 1);
            assert_eq!(s.error_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(
        after.transcript_status.as_deref(),
        Some(transcript_status::DONE)
    );
    assert!(after.text_sha256.is_some());
    assert_eq!(
        after.transcript_native_sha256.as_deref(),
        Some(native.as_str())
    );
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("MOCK_SPEECH_MARKER"));
}

#[test]
fn concat_preserves_existing_metadata_text() {
    let (_tmp, matter) = make_matter("stt-concat");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let meta_sha = matter
        .put_bytes(b"Title: Client call 2024-01-01\nOrganizer: counsel@example.com")
        .unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("call.wav".into()),
            native_sha256: Some(native),
            text_sha256: Some(meta_sha),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            file_category: Some("audio".into()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockSttEngine::new("we should discuss the settlement");
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let outcome = run_transcribe_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    assert!(matches!(outcome, SttOutcome::Succeeded(_)));

    let after = matter.get_item(&item.id).unwrap();
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(
        text.contains("Client call 2024-01-01"),
        "must preserve metadata: {text}"
    );
    assert!(
        text.contains(TRANSCRIPT_MARKER),
        "must include marker: {text}"
    );
    assert!(
        text.contains("we should discuss the settlement"),
        "must include STT: {text}"
    );
}

#[test]
fn parent_email_body_unchanged_when_child_transcribed() {
    let (_tmp, matter) = make_matter("stt-parent");
    let family = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .unwrap();
    let body_sha = matter
        .put_bytes(b"Parent email body must stay intact.")
        .unwrap();
    let parent = matter
        .insert_item(ItemInput {
            path: Some("msg.eml".into()),
            text_sha256: Some(body_sha.clone()),
            status: "extracted".into(),
            mime_type: Some("message/rfc822".into()),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            ..Default::default()
        })
        .unwrap();

    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let child = matter
        .insert_item(ItemInput {
            path: Some("attach/voicemail.wav".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            file_category: Some("audio".into()),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(parent.id.clone()),
            family_id: Some(family.id.clone()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockSttEngine::new("attachment only speech");
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    run_transcribe_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();

    let parent_after = matter.get_item(&parent.id).unwrap();
    assert_eq!(parent_after.text_sha256.as_deref(), Some(body_sha.as_str()));
    assert!(parent_after.transcript_status.is_none());
    let parent_text = String::from_utf8(
        matter
            .get_bytes(parent_after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(parent_text, "Parent email body must stay intact.");
    assert!(!parent_text.contains(TRANSCRIPT_MARKER));

    let child_after = matter.get_item(&child.id).unwrap();
    assert_eq!(
        child_after.transcript_status.as_deref(),
        Some(transcript_status::DONE)
    );
}

#[test]
fn disabled_job_fails_no_mutation() {
    let (_tmp, matter) = make_matter("stt-off");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.wav".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            ..Default::default()
        })
        .unwrap();

    let params = SttParams {
        enabled: false,
        engine: "mock".into(),
        ..SttParams::default()
    };
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let outcome = run_transcribe(&matter, &job.id, &params, None, |_| {}).unwrap();
    match outcome {
        SttOutcome::Failed { message, summary } => {
            assert!(message.to_lowercase().contains("disabled"));
            assert_eq!(summary.completed_count, 0);
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert!(after.transcript_status.is_none());
    assert!(after.text_sha256.is_none());
}

#[test]
fn production_rejects_mock_engine() {
    let (_tmp, matter) = make_matter("stt-no-mock-prod");
    let params = SttParams {
        enabled: true,
        engine: "mock".into(),
        ..SttParams::default()
    };
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let err = run_transcribe(&matter, &job.id, &params, None, |_| {}).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("mock"));
}

#[test]
fn digest_skip_when_native_unchanged() {
    let (_tmp, matter) = make_matter("stt-idem");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.wav".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockSttEngine::new("first pass speech");
    let job1 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let o1 = run_transcribe_with_engine(
        &matter,
        &job1.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    assert!(matches!(o1, SttOutcome::Succeeded(ref s) if s.transcript_count == 1));

    let job2 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let o2 = run_transcribe_with_engine(
        &matter,
        &job2.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    match o2 {
        SttOutcome::Succeeded(s) => {
            assert_eq!(s.skipped_count, 1);
            assert_eq!(s.transcript_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(
        after.transcript_status.as_deref(),
        Some(transcript_status::DONE),
        "digest skip must leave done status intact (not demote to skipped)"
    );
}

#[test]
fn reset_replaces_transcript_section_only() {
    let (_tmp, matter) = make_matter("stt-reset");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let meta_sha = matter.put_bytes(b"Purview title line").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.wav".into()),
            native_sha256: Some(native.clone()),
            text_sha256: Some(meta_sha),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            ..Default::default()
        })
        .unwrap();

    let e1 = MockSttEngine::new("old transcript words");
    let job1 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    run_transcribe_with_engine(&matter, &job1.id, &enabled_mock_params(), &e1, None, |_| {})
        .unwrap();

    let e2 = MockSttEngine::new("new transcript words");
    let mut params = enabled_mock_params();
    params.reset = true;
    let job2 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    run_transcribe_with_engine(&matter, &job2.id, &params, &e2, None, |_| {}).unwrap();

    let after = matter.get_item(&item.id).unwrap();
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("Purview title line"));
    assert!(text.contains("new transcript words"));
    assert!(!text.contains("old transcript words"));
    assert_eq!(text.matches(TRANSCRIPT_MARKER).count(), 1);
    assert_eq!(
        after.transcript_native_sha256.as_deref(),
        Some(native.as_str())
    );
}

#[test]
fn ffmpeg_arg_contract_locked() {
    let args = build_ffmpeg_pcm_args(
        camino::Utf8Path::new("in.mp4"),
        camino::Utf8Path::new("out.wav"),
    );
    assert!(args_contain_locked_pcm_flags(&args), "{args:?}");
    assert!(args.windows(2).any(|w| w[0] == "-ar" && w[1] == "16000"));
    assert!(args.windows(2).any(|w| w[0] == "-ac" && w[1] == "1"));
    assert!(args.iter().any(|a| a == "pcm_s16le"));
}

#[test]
fn job_object_flag_constant() {
    assert_eq!(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0x2000);
    assert_eq!(kill_on_close_limit_flags(), 0x2000);
}

#[test]
fn drop_guard_and_purge() {
    let dir = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let path = {
        let mut t = SttTempFile::new_in(&root, ".wav").unwrap();
        t.write_all(b"x").unwrap();
        t.path_buf()
    };
    assert!(!path.exists());

    let stt_dir = stt_plugin::ensure_stt_temp_dir(&root).unwrap();
    let orphan = stt_dir.as_std_path().join("orphan.wav");
    std::fs::write(&orphan, b"leak").unwrap();
    assert_eq!(purge_stt_temp_dir(&root).unwrap(), 1);
    assert!(!orphan.exists());
}

#[test]
fn apply_transcript_concat_unit_via_matter() {
    let (_tmp, matter) = make_matter("stt-apply");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let meta = matter.put_bytes(b"short meta").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.wav".into()),
            native_sha256: Some(native.clone()),
            text_sha256: Some(meta),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            ..Default::default()
        })
        .unwrap();

    matter
        .apply_transcript_text(ApplyTranscriptInput {
            item_id: item.id.clone(),
            force: true,
            text: Some("spoken words".into()),
            engine: Some("mock".into()),
            model: Some("mock-1.0".into()),
            language: Some("en".into()),
            status: Some(transcript_status::DONE.into()),
            error: None,
            source_native_sha256: Some(native),
            job_id: Some("job1".into()),
        })
        .unwrap();

    let after = matter.get_item(&item.id).unwrap();
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("short meta"));
    assert!(text.contains(TRANSCRIPT_MARKER));
    assert!(text.contains("spoken words"));
    // FTS bookkeeping cleared on text change.
    // (fts columns not on Item struct — verify via SQL)
    let fts: Option<String> = matter
        .connection()
        .query_row(
            "SELECT fts_text_sha256 FROM items WHERE id = ?1",
            [&item.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(fts.is_none());
}

#[test]
fn no_silent_model_download_on_missing_model() {
    // Production path with auto engine and no model → fail closed, no network.
    let (_tmp, matter) = make_matter("stt-no-dl");
    let params = SttParams {
        enabled: true,
        engine: "auto".into(),
        model_path: None,
        whisper_cli_path: Some(if cfg!(windows) {
            r"C:\nonexistent\whisper-cli.exe".into()
        } else {
            "/nonexistent/whisper-cli".into()
        }),
        ..SttParams::default()
    };
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let err = run_transcribe(&matter, &job.id, &params, None, |_| {}).unwrap_err();
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("not found") || msg.contains("model"),
        "must fail closed without download: {err}"
    );
}

#[test]
fn fixture_silence_16k_mono_is_whisper_compliant() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("stt")
        .join("silence_16k_mono.wav");
    assert!(path.is_file(), "expected fixture at {}", path.display());
    let bytes = std::fs::read(&path).expect("read fixture");
    assert!(looks_like_wav(&bytes));
    assert!(
        is_whisper_compliant_wav(&bytes),
        "fixtures/stt/silence_16k_mono.wav must be 16k mono s16le PCM"
    );

    let (_tmp, matter) = make_matter("stt-fixture");
    let native = matter.put_bytes(&bytes).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("silence_16k_mono.wav".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            file_category: Some("audio".into()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockSttEngine::new("fixture silence ok");
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let outcome = run_transcribe_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    assert!(matches!(
        outcome,
        SttOutcome::Succeeded(ref s) if s.transcript_count == 1
    ));
}

#[test]
fn common_audio_stereo_wav_without_ffmpeg() {
    // P2-1: non-canonical stereo 44.1 kHz WAV must decode via Symphonia without ffmpeg.
    let (_tmp, matter) = make_matter("stt-stereo");
    let wav = stereo_44100_wav_bytes();
    assert!(!is_whisper_compliant_wav(&wav));
    let native = matter.put_bytes(&wav).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("interview_stereo.wav".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            file_category: Some("audio".into()),
            ..Default::default()
        })
        .unwrap();

    // Point ffmpeg at a guaranteed-missing path so any ffmpeg fallback would fail.
    let mut params = enabled_mock_params();
    params.ffmpeg_path = Some(if cfg!(windows) {
        r"C:\nonexistent\stt-plugin-no-ffmpeg\ffmpeg.exe".into()
    } else {
        "/nonexistent/stt-plugin-no-ffmpeg/ffmpeg".into()
    });

    let engine = MockSttEngine::new("STEREO_NO_FFMPEG_OK");
    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let outcome =
        run_transcribe_with_engine(&matter, &job.id, &params, &engine, None, |_| {}).unwrap();
    match outcome {
        SttOutcome::Succeeded(s) => {
            assert_eq!(
                s.transcript_count, 1,
                "symphonia path must succeed without ffmpeg"
            );
            assert_eq!(s.error_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(
        after.transcript_status.as_deref(),
        Some(transcript_status::DONE)
    );
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("STEREO_NO_FFMPEG_OK"));
}

#[test]
fn missing_ffmpeg_video_not_permanently_digest_skipped() {
    // P2-2: first run without ffmpeg → skipped; second run must not digest-skip.
    let (_tmp, matter) = make_matter("stt-ffmpeg-retry");
    // Tiny non-audio bytes labeled as video — Symphonia fails; ffmpeg path required.
    let native = matter.put_bytes(b"not-a-real-video-container").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("clip.mp4".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            mime_type: Some("video/mp4".into()),
            file_category: Some("video".into()),
            ..Default::default()
        })
        .unwrap();

    let mut params = enabled_mock_params();
    params.ffmpeg_path = Some(if cfg!(windows) {
        r"C:\nonexistent\stt-plugin-no-ffmpeg\ffmpeg.exe".into()
    } else {
        "/nonexistent/stt-plugin-no-ffmpeg/ffmpeg".into()
    });

    let engine = MockSttEngine::new("should not run on video without convert");
    let job1 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let o1 = run_transcribe_with_engine(&matter, &job1.id, &params, &engine, None, |_| {}).unwrap();
    match o1 {
        SttOutcome::Succeeded(s) => {
            assert_eq!(s.skipped_count, 1, "missing ffmpeg → skipped");
            assert_eq!(s.transcript_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }
    let after1 = matter.get_item(&item.id).unwrap();
    assert_eq!(
        after1.transcript_status.as_deref(),
        Some(transcript_status::SKIPPED)
    );
    // Retryable: must not claim native digest for permanent skip.
    assert!(
        after1.transcript_native_sha256.is_none(),
        "tool-missing skip must not claim transcript_native_sha256"
    );

    // Second run with same missing ffmpeg must still process (not digest-skip as done).
    let job2 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let o2 = run_transcribe_with_engine(&matter, &job2.id, &params, &engine, None, |_| {}).unwrap();
    match o2 {
        SttOutcome::Succeeded(s) => {
            assert_eq!(
                s.skipped_count, 1,
                "second run must re-attempt, not silent digest-skip with transcript_count path"
            );
            assert_eq!(s.transcript_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }

    // Simulate tool becoming available by materializing a compliant WAV as the native
    // and re-labeling as audio — proves skipped status alone does not block reprocessing.
    // (true ffmpeg install is env-dependent; digest-skip contract is what we assert.)
    let wav = minimal_wav_bytes();
    let wav_native = matter.put_bytes(&wav).unwrap();
    matter
        .connection()
        .execute(
            "UPDATE items SET native_sha256 = ?1, path = 'clip.wav', mime_type = 'audio/wav', \
             file_category = 'audio', transcript_status = 'skipped', transcript_native_sha256 = NULL \
             WHERE id = ?2",
            rusqlite::params![wav_native, item.id],
        )
        .unwrap();
    let job3 = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let o3 = run_transcribe_with_engine(
        &matter,
        &job3.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    match o3 {
        SttOutcome::Succeeded(s) => {
            assert_eq!(
                s.transcript_count, 1,
                "after prior skipped, audio with no native claim must not be digest-skipped"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let after3 = matter.get_item(&item.id).unwrap();
    assert_eq!(
        after3.transcript_status.as_deref(),
        Some(transcript_status::DONE)
    );
}

#[test]
fn prior_text_missing_cas_fails_closed() {
    // P2-4: text_sha256 set but CAS missing → fail closed; do not write transcript-only.
    let (_tmp, matter) = make_matter("stt-prior-cas");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let fake_text_sha = "a".repeat(64);
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.wav".into()),
            native_sha256: Some(native.clone()),
            text_sha256: Some(fake_text_sha.clone()),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            ..Default::default()
        })
        .unwrap();

    let result = matter
        .apply_transcript_text(ApplyTranscriptInput {
            item_id: item.id.clone(),
            force: true,
            text: Some("new speech only".into()),
            engine: Some("mock".into()),
            model: Some("mock-1.0".into()),
            language: Some("en".into()),
            status: Some(transcript_status::DONE.into()),
            error: None,
            source_native_sha256: Some(native.clone()),
            job_id: Some("job1".into()),
        })
        .unwrap();
    match result {
        matter_core::TranscriptApplyResult::Error { error } => {
            assert!(
                error.contains("prior_text") || error.contains("cas"),
                "expected prior-text CAS error, got {error}"
            );
        }
        other => panic!("expected Error bookkeeping, got {other:?}"),
    }
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(
        after.text_sha256.as_deref(),
        Some(fake_text_sha.as_str()),
        "must not replace text_sha256 with transcript-only"
    );
    assert_eq!(
        after.transcript_status.as_deref(),
        Some(transcript_status::FAILED)
    );
}

#[test]
fn prior_text_invalid_utf8_fails_closed() {
    // P2-4: text_sha256 points at non-UTF-8 CAS → fail closed.
    let (_tmp, matter) = make_matter("stt-prior-utf8");
    let wav = minimal_wav_bytes();
    let native = matter.put_bytes(&wav).unwrap();
    let bad_sha = matter.put_bytes(&[0xff, 0xfe, 0xfd, 0x00, 0x80]).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.wav".into()),
            native_sha256: Some(native.clone()),
            text_sha256: Some(bad_sha.clone()),
            status: "extracted".into(),
            mime_type: Some("audio/wav".into()),
            ..Default::default()
        })
        .unwrap();

    let result = matter
        .apply_transcript_text(ApplyTranscriptInput {
            item_id: item.id.clone(),
            force: true,
            text: Some("speech".into()),
            engine: Some("mock".into()),
            model: Some("mock-1.0".into()),
            language: Some("en".into()),
            status: Some(transcript_status::DONE.into()),
            error: None,
            source_native_sha256: Some(native),
            job_id: Some("job1".into()),
        })
        .unwrap();
    assert!(matches!(
        result,
        matter_core::TranscriptApplyResult::Error { .. }
    ));
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.text_sha256.as_deref(), Some(bad_sha.as_str()));
    assert_eq!(
        after.transcript_status.as_deref(),
        Some(transcript_status::FAILED)
    );
    assert!(after
        .transcript_error
        .as_deref()
        .is_some_and(|e| e.contains("invalid_utf8")));
}

#[test]
fn cancel_between_items_pauses() {
    let (_tmp, matter) = make_matter("stt-cancel-items");
    let wav = minimal_wav_bytes();
    for i in 0..4 {
        let native = matter.put_bytes(&wav).unwrap();
        matter
            .insert_item(ItemInput {
                path: Some(format!("clip{i}.wav")),
                native_sha256: Some(native),
                status: "extracted".into(),
                mime_type: Some("audio/wav".into()),
                file_category: Some("audio".into()),
                ..Default::default()
            })
            .unwrap();
    }

    let engine = MockSttEngine::new("partial run");
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let mut params = enabled_mock_params();
    params.batch_size = 1;

    let job = matter.create_job(JOB_KIND_TRANSCRIBE).unwrap();
    let outcome = run_transcribe_with_engine(
        &matter,
        &job.id,
        &params,
        &engine,
        Some(&|| cancel_flag2.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .unwrap();

    match outcome {
        SttOutcome::Paused(s) => {
            assert!(s.completed_count >= 1, "should complete at least one item");
            assert!(
                s.completed_count < 4,
                "should not finish all items after cancel"
            );
        }
        other => panic!("expected Paused after cancel, got {other:?}"),
    }
}

#[test]
fn cancellable_spawn_terminates_long_running_child() {
    // Spec §3.3.2 / DoD-3: cancel mid-wait must kill Job Object / process group.
    #[cfg(windows)]
    let (program, args) = ("ping", vec!["-n".into(), "60".into(), "127.0.0.1".into()]);
    #[cfg(not(windows))]
    let (program, args) = ("sleep", vec!["60".into()]);

    let flag = Arc::new(AtomicBool::new(false));
    let flag2 = flag.clone();
    let cancel: &dyn Fn() -> bool = &|| flag2.load(Ordering::SeqCst);

    let arm = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::SeqCst);
    });

    let started = Instant::now();
    let result = spawn_and_wait_cancellable(program, &args, None, Some(cancel));
    let elapsed = started.elapsed();
    arm.join().expect("arm thread");

    assert!(
        matches!(result, Err(CancellableWaitError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "child kill must not wait full duration: {elapsed:?}"
    );
}
