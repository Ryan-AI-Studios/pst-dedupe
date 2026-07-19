//! Integration tests for extract-calendar (spec §3.9 ICS items).

use std::fs;
use std::path::PathBuf;

use extract_calendar::{
    count_vevents_in_ics, extract_ics_catch_unwind, looks_like_ics, parse_ics, run_ics_extract,
    IcsExtractOutcome, IcsExtractParams, JOB_KIND_ICS_EXTRACT,
};
use matter_core::{item_status, FilterSpec, ItemInput, Matter};

fn fixtures_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("calendar");
    p
}

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

#[test]
fn single_vevent_fields_and_marker() {
    let data = load_fixture("single.ics");
    assert!(looks_like_ics(&data));
    let p = parse_ics(&data).expect("parse");
    assert!(!p.is_container);
    assert_eq!(p.events.len(), 1);
    let ev = &p.events[0];
    assert_eq!(ev.fields.subject.as_deref(), Some("Single Event Marker"));
    assert!(ev
        .fields
        .description
        .as_deref()
        .unwrap_or("")
        .contains("ICS_SINGLE_MARKER"));
    assert_eq!(
        ev.fields.cal_start_at.as_deref(),
        Some("2026-07-18T15:00:00Z")
    );
    assert_eq!(count_vevents_in_ics(&ev.single_event_ics).unwrap(), 1);
}

#[test]
fn multi_vevent_container_isolated_natives() {
    let data = load_fixture("multi.ics");
    let parent_digest = matter_core::sha256_hex(&data);
    let p = parse_ics(&data).expect("parse");
    assert!(p.is_container);
    assert_eq!(p.events.len(), 3);

    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "MultiIcs").unwrap();
    let parent_native = matter.put_bytes(&data).unwrap();
    assert_eq!(parent_native, parent_digest);

    let parent = matter
        .insert_item(ItemInput {
            path: Some("export/calendar.ics".into()),
            native_sha256: Some(parent_native.clone()),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/calendar".into()),
            file_category: Some("attachment".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_ICS_EXTRACT).unwrap();
    let outcome =
        run_ics_extract(&matter, &job.id, &IcsExtractParams::default(), None, |_| {}).expect("run");
    match outcome {
        IcsExtractOutcome::Succeeded(s) => {
            assert!(s.extracted_count >= 1);
            assert_eq!(s.child_count, 3);
        }
        other => panic!("unexpected {other:?}"),
    }

    let parent_after = matter.get_item(&parent.id).unwrap();
    assert_eq!(parent_after.file_category.as_deref(), Some("archive"));
    assert_eq!(parent_after.ics_extract_status.as_deref(), Some("ok"));

    let children = matter.list_attachments(&parent.id).unwrap();
    assert_eq!(children.len(), 3);
    for c in &children {
        assert_eq!(c.file_category.as_deref(), Some("calendar"));
        let child_native = c.native_sha256.as_deref().unwrap();
        assert_ne!(
            child_native,
            parent_native.as_str(),
            "child must not share mega-file digest"
        );
        let bytes = matter.get_bytes(child_native).unwrap();
        assert_eq!(
            count_vevents_in_ics(&bytes).unwrap(),
            1,
            "child native must re-parse to one VEVENT"
        );
        let text = c
            .text_sha256
            .as_ref()
            .map(|d| String::from_utf8(matter.get_bytes(d).unwrap()).unwrap())
            .unwrap_or_default();
        assert!(
            text.contains("MULTI_") || text.contains("Multi Event"),
            "text={text}"
        );
    }
}

#[test]
fn rrule_one_child_no_expansion() {
    let data = load_fixture("rrule.ics");
    let p = parse_ics(&data).unwrap();
    assert_eq!(p.events.len(), 1);
    assert_eq!(p.events[0].fields.cal_is_recurring, Some(1));
}

#[test]
fn corrupt_ics_no_panic() {
    let data = load_fixture("corrupt.ics");
    // May or may not look like ICS (has BEGIN:VCALENDAR) — parse must not panic.
    let r = extract_ics_catch_unwind(&data);
    assert!(r.is_err(), "corrupt should error, got {r:?}");
}

#[test]
fn tzid_dst_offsets_differ() {
    let summer = parse_ics(&load_fixture("tz_summer.ics")).unwrap();
    let winter = parse_ics(&load_fixture("tz_winter.ics")).unwrap();
    let so = summer.events[0]
        .fields
        .cal_start_at
        .as_deref()
        .expect("summer start");
    let wo = winter.events[0]
        .fields
        .cal_start_at
        .as_deref()
        .expect("winter start");
    assert!(so.contains("-04:00"), "summer={so}");
    assert!(wo.contains("-05:00"), "winter={wo}");
    assert_ne!(so, wo);
}

#[test]
fn unknown_tzid_null_start() {
    let p = parse_ics(&load_fixture("unknown_tz.ics")).unwrap();
    assert!(p.events[0].fields.cal_start_at.is_none());
    assert!(p.events[0].fields.tz_unresolved);
}

#[test]
fn job_single_event_apply() {
    let data = load_fixture("single.ics");
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "SingleIcs").unwrap();
    let native = matter.put_bytes(&data).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("meetings/single.ics".into()),
            native_sha256: Some(native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/calendar".into()),
            ..Default::default()
        })
        .unwrap();
    let job = matter.create_job(JOB_KIND_ICS_EXTRACT).unwrap();
    let outcome =
        run_ics_extract(&matter, &job.id, &IcsExtractParams::default(), None, |_| {}).unwrap();
    assert!(matches!(outcome, IcsExtractOutcome::Succeeded(_)));
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.file_category.as_deref(), Some("calendar"));
    assert_eq!(after.ics_extract_status.as_deref(), Some("ok"));
    assert_eq!(after.subject.as_deref(), Some("Single Event Marker"));
    assert!(after.sent_at.is_some());
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("ICS_SINGLE_MARKER"));
    assert!(text.contains("Subject:"));
}

#[test]
fn filter_file_category_calendar() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "FilterCal").unwrap();
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            file_category: Some("calendar".into()),
            subject: Some("Appt".into()),
            ..Default::default()
        })
        .unwrap();
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            file_category: Some("email".into()),
            subject: Some("Mail".into()),
            ..Default::default()
        })
        .unwrap();

    let cals = matter.list_items_by_file_category("calendar").unwrap();
    assert_eq!(cals.len(), 1);
    assert_eq!(cals[0].subject.as_deref(), Some("Appt"));
    assert!(FilterSpec::preset_calendar()
        .conditions
        .iter()
        .any(|c| c.field == "file_category"));
}

/// Simulate a crash after parent was marked ok with only 1 of N children, then
/// re-run without force — must finish expansion without duplicates.
#[test]
fn multi_vevent_resume_partial_expansion() {
    use matter_core::{ics_extract_status, item_role, ApplyIcsExtractInput};

    let data = load_fixture("multi.ics");
    let p = parse_ics(&data).expect("parse");
    assert_eq!(p.events.len(), 3);

    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "PartialIcs").unwrap();
    let parent_native = matter.put_bytes(&data).unwrap();
    let parent_path = "export/calendar.ics";

    let parent = matter
        .insert_item(ItemInput {
            path: Some(parent_path.into()),
            native_sha256: Some(parent_native.clone()),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/calendar".into()),
            file_category: Some("attachment".into()),
            ..Default::default()
        })
        .unwrap();

    // Simulate legacy bug: parent terminal ok + archive before all children exist.
    let fam = matter.insert_family("ics-events").unwrap();
    matter
        .update_item(
            &parent.id,
            matter_core::ItemUpdate {
                family_id: Some(Some(fam.id.clone())),
                role: Some(Some(item_role::PARENT.into())),
                file_category: Some(Some("archive".into())),
                message_class: Some(Some("VCALENDAR".into())),
                extra_json: Some(Some(
                    serde_json::json!({
                        "ics_container": true,
                        "vevent_count": 3,
                        "extract_tool": "extract-calendar",
                    })
                    .to_string(),
                )),
                ..Default::default()
            },
        )
        .unwrap();
    matter
        .apply_ics_extract(ApplyIcsExtractInput {
            item_id: parent.id.clone(),
            force: true,
            text: None,
            method: Some(p.method.clone()),
            status: Some(ics_extract_status::OK.into()),
            source_native_sha256: Some(parent_native.clone()),
            file_category: Some("archive".into()),
            refine_file_category: true,
            message_class: Some("VCALENDAR".into()),
            extra_json: Some(
                serde_json::json!({
                    "ics_container": true,
                    "vevent_count": 3,
                })
                .to_string(),
            ),
            ..Default::default()
        })
        .unwrap();

    // Only first VEVENT child exists (crash mid-loop).
    let ev0 = &p.events[0];
    let child_native = matter.put_bytes(&ev0.single_event_ics).unwrap();
    let leaf = ev0
        .fields
        .cal_uid
        .as_deref()
        .map(|u| {
            u.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .unwrap_or_else(|| "vevent-0".into());
    let child_path = format!("{parent_path}!/{leaf}.ics");
    let child = matter
        .insert_item(ItemInput {
            path: Some(child_path),
            native_sha256: Some(child_native.clone()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(parent.id.clone()),
            family_id: Some(fam.id),
            mime_type: Some("text/calendar".into()),
            file_category: Some("calendar".into()),
            cal_uid: ev0.fields.cal_uid.clone(),
            subject: ev0.fields.subject.clone(),
            ..Default::default()
        })
        .unwrap();
    matter
        .apply_ics_extract(ApplyIcsExtractInput {
            item_id: child.id,
            force: true,
            text: None,
            method: Some(p.method.clone()),
            status: Some(ics_extract_status::OK.into()),
            source_native_sha256: Some(child_native),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(matter.list_attachments(&parent.id).unwrap().len(), 1);

    // Re-run without force — must resume missing children, no duplicates.
    let job = matter.create_job(JOB_KIND_ICS_EXTRACT).unwrap();
    let outcome =
        run_ics_extract(&matter, &job.id, &IcsExtractParams::default(), None, |_| {}).expect("run");
    assert!(
        matches!(outcome, IcsExtractOutcome::Succeeded(_)),
        "{outcome:?}"
    );

    let children = matter.list_attachments(&parent.id).unwrap();
    assert_eq!(children.len(), 3, "resume must create all VEVENT children");
    let mut paths = std::collections::HashSet::new();
    for c in &children {
        assert_eq!(c.file_category.as_deref(), Some("calendar"));
        let path = c.path.as_deref().expect("path");
        assert!(paths.insert(path.to_string()), "duplicate path {path}");
        assert_ne!(c.native_sha256.as_deref(), Some(parent_native.as_str()));
    }

    let parent_after = matter.get_item(&parent.id).unwrap();
    assert_eq!(parent_after.ics_extract_status.as_deref(), Some("ok"));
    assert_eq!(parent_after.file_category.as_deref(), Some("archive"));
}

/// Distinct VEVENT UIDs that sanitize identically must still get unique children.
#[test]
fn multi_vevent_sanitize_uid_collision_unique_paths() {
    // a/b and a:b both sanitize to a_b without disambiguation.
    let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\n\
BEGIN:VEVENT\r\nUID:a/b\r\nSUMMARY:First Collision\r\n\
DTSTART:20260718T150000Z\r\nDTEND:20260718T160000Z\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a:b\r\nSUMMARY:Second Collision\r\n\
DTSTART:20260719T150000Z\r\nDTEND:20260719T160000Z\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

    let p = parse_ics(ics).expect("parse");
    assert_eq!(p.events.len(), 2);
    assert!(p.is_container);

    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "UidCollision").unwrap();
    let parent_native = matter.put_bytes(ics).unwrap();
    let parent = matter
        .insert_item(ItemInput {
            path: Some("export/collide.ics".into()),
            native_sha256: Some(parent_native.clone()),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/calendar".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_ICS_EXTRACT).unwrap();
    let outcome =
        run_ics_extract(&matter, &job.id, &IcsExtractParams::default(), None, |_| {}).expect("run");
    match outcome {
        IcsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.child_count, 2, "both VEVENTs must expand");
            assert_eq!(s.error_count, 0, "expansion must complete without error");
        }
        other => panic!("unexpected {other:?}"),
    }

    let children = matter.list_attachments(&parent.id).unwrap();
    assert_eq!(children.len(), 2, "two unique children required");
    let mut paths = std::collections::HashSet::new();
    for c in &children {
        let path = c.path.as_deref().expect("path");
        assert!(
            paths.insert(path.to_string()),
            "duplicate child path {path}"
        );
        assert_eq!(c.file_category.as_deref(), Some("calendar"));
        assert_ne!(c.native_sha256.as_deref(), Some(parent_native.as_str()));
    }
    let parent_after = matter.get_item(&parent.id).unwrap();
    assert_eq!(parent_after.ics_extract_status.as_deref(), Some("ok"));
    assert_eq!(parent_after.file_category.as_deref(), Some("archive"));
}

/// Oversized single-event native is rejected (limit) without panic.
#[test]
fn oversized_single_event_native_rejected() {
    use extract_calendar::{
        reject_oversized_single_event_native, reject_oversized_single_event_native_with_max,
        Error as CalError, MAX_SINGLE_EVENT_NATIVE_BYTES,
    };

    // Production constant gate.
    assert!(reject_oversized_single_event_native(MAX_SINGLE_EVENT_NATIVE_BYTES).is_ok());
    let err = reject_oversized_single_event_native(MAX_SINGLE_EVENT_NATIVE_BYTES + 1).unwrap_err();
    assert_eq!(err.code(), "ics_limit_exceeded");
    match err {
        CalError::LimitExceeded { message, .. } => {
            assert!(message.contains("single-event"), "{message}");
            assert!(message.contains(&format!("{}", MAX_SINGLE_EVENT_NATIVE_BYTES + 1)));
        }
        other => panic!("expected LimitExceeded, got {other:?}"),
    }

    // Injectable max for boundary tests (used by container child CAS put gate).
    assert!(reject_oversized_single_event_native_with_max(64, 64).is_ok());
    let err = reject_oversized_single_event_native_with_max(65, 64).unwrap_err();
    assert_eq!(err.code(), "ics_limit_exceeded");
    assert!(!err.short_message().is_empty());
}

/// Force re-extract must upsert by path — child count stays == VEVENT count.
#[test]
fn multi_vevent_force_twice_no_duplicates() {
    let data = load_fixture("multi.ics");
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "ForceIcs").unwrap();
    let parent_native = matter.put_bytes(&data).unwrap();
    let parent = matter
        .insert_item(ItemInput {
            path: Some("export/calendar.ics".into()),
            native_sha256: Some(parent_native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/calendar".into()),
            ..Default::default()
        })
        .unwrap();

    let force_params = IcsExtractParams {
        force: true,
        batch_size: 50,
    };

    for i in 0..2 {
        let job = matter.create_job(JOB_KIND_ICS_EXTRACT).unwrap();
        let outcome = run_ics_extract(&matter, &job.id, &force_params, None, |_| {})
            .unwrap_or_else(|e| panic!("force run {i}: {e}"));
        assert!(
            matches!(outcome, IcsExtractOutcome::Succeeded(_)),
            "run {i}: {outcome:?}"
        );
        let children = matter.list_attachments(&parent.id).unwrap();
        assert_eq!(
            children.len(),
            3,
            "force run {i}: child count must equal VEVENT count"
        );
        let mut paths = std::collections::HashSet::new();
        for c in &children {
            let path = c.path.as_deref().expect("path");
            assert!(
                paths.insert(path.to_string()),
                "force run {i}: duplicate path {path}"
            );
        }
    }
}
