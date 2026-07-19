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
