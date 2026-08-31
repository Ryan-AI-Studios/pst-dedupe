//! Integration tests for geometric redaction (schema v40 / track 0114).

use matter_core::{
    burn_required, burned_native_fresh, geom_source, item_role, item_status, redaction_reason,
    redaction_status, CreateGeomRedactionInput, CreateRedactionInput, ItemInput, ItemUpdate,
    Matter, SetBurnedNativeInput, RASTER_ENGINE_PIN, RASTER_ENGINE_ZPDF, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn insert_pdf_item(matter: &Matter, native: &[u8]) -> matter_core::Item {
    let digest = matter.put_bytes(native).expect("cas");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            native_sha256: Some(digest),
            path: Some("doc.pdf".into()),
            mime_type: Some("application/pdf".into()),
            file_category: Some("pdf".into()),
            ..Default::default()
        })
        .expect("item")
}

#[allow(clippy::too_many_arguments)]
fn make_geom(
    matter: &Matter,
    item_id: &str,
    page: i64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    source: &str,
) -> matter_core::ItemGeomRedaction {
    matter
        .create_geom_redaction(CreateGeomRedactionInput {
            item_id: item_id.to_string(),
            page_index: page,
            x,
            y,
            w,
            h,
            reason: redaction_reason::PRIVILEGE.into(),
            label: None,
            source: source.to_string(),
            actor: "tester".into(),
        })
        .expect("create geom")
}

#[test]
fn schema_v40_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v40");
    let matter = Matter::create(&root, "V40").expect("create");
    assert_eq!(SCHEMA_VERSION, 41);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);

    let item = insert_pdf_item(&matter, b"%PDF-1.4 SECRET_TOKEN_0114");
    assert_eq!(item.geom_redaction_count, 0);
    assert!(item.burned_native_sha256.is_none());
    assert!(item.burned_source_digest.is_none());
}

#[test]
fn create_list_delete_geom_original_cas_unchanged() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-geom-crud");
    let matter = Matter::create(&root, "Geom").expect("create");
    let native = b"%PDF-1.4 SECRET_TOKEN_0114";
    let item = insert_pdf_item(&matter, native);
    let orig_sha = item.native_sha256.clone().expect("native");
    let orig_bytes = matter.get_bytes(&orig_sha).expect("orig cas");

    let g = make_geom(
        &matter,
        &item.id,
        0,
        10.0,
        20.0,
        30.0,
        40.0,
        geom_source::DRAW,
    );
    assert_eq!(g.status, redaction_status::ACTIVE);
    assert_eq!(g.source, geom_source::DRAW);
    assert_eq!(g.page_index, 0);
    assert_eq!(g.w, 30.0);

    let listed = matter.list_geom_redactions(&item.id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, g.id);

    let reloaded = matter.get_item(&item.id).expect("reload");
    assert_eq!(reloaded.geom_redaction_count, 1);
    assert_eq!(reloaded.native_sha256.as_deref(), Some(orig_sha.as_str()));
    let after = matter.get_bytes(&orig_sha).expect("cas after");
    assert_eq!(after, orig_bytes);

    matter
        .delete_geom_redaction(&g.id, "tester")
        .expect("delete");
    assert!(matter
        .list_geom_redactions(&item.id)
        .expect("list")
        .is_empty());
    let after_del = matter.get_item(&item.id).expect("reload");
    assert_eq!(after_del.geom_redaction_count, 0);
    assert_eq!(after_del.native_sha256.as_deref(), Some(orig_sha.as_str()));
}

#[test]
fn fingerprint_mismatch_after_text_redaction() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-geom-fp");
    let matter = Matter::create(&root, "Fp").expect("create");
    let body = "Alpha SECRET_TOKEN_0114 beta";
    let text_sha = matter.put_bytes(body.as_bytes()).expect("text");
    let native_sha = matter
        .put_bytes(b"%PDF-1.4 SECRET_TOKEN_0114")
        .expect("nat");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            native_sha256: Some(native_sha),
            text_sha256: Some(text_sha.clone()),
            path: Some("doc.pdf".into()),
            mime_type: Some("application/pdf".into()),
            ..Default::default()
        })
        .expect("item");

    make_geom(&matter, &item.id, 0, 1.0, 2.0, 3.0, 4.0, geom_source::DRAW);
    let fp1 = matter.geom_burn_fingerprint(&item.id).expect("fp1");
    assert!(!fp1.is_empty());
    assert!(fp1.chars().all(|c| c.is_ascii_hexdigit()));

    let burned = matter.put_bytes(b"%PDF-1.4 burned").expect("burned cas");
    let after_burn = matter
        .set_burned_native(SetBurnedNativeInput {
            item_id: item.id.clone(),
            burned_native_sha256: burned,
            expected_fingerprint: fp1.clone(),
            actor: "tester".into(),
        })
        .expect("set burned");
    assert_eq!(
        after_burn.raster_engine.as_deref(),
        Some(RASTER_ENGINE_ZPDF)
    );
    assert_eq!(
        after_burn.burned_source_digest.as_deref(),
        Some(fp1.as_str())
    );
    assert!(burned_native_fresh(&after_burn, &fp1));
    assert!(burn_required(&after_burn, &fp1));

    matter
        .create_redaction(CreateRedactionInput {
            item_id: item.id.clone(),
            start_utf8: 6,
            end_utf8: 23,
            exact_quote: "SECRET_TOKEN_0114".into(),
            display_body: body.into(),
            body_digest: text_sha,
            reason: redaction_reason::CONFIDENTIAL.into(),
            label: None,
            actor: "tester".into(),
        })
        .expect("text redaction");

    let fp2 = matter.geom_burn_fingerprint(&item.id).expect("fp2");
    assert_ne!(fp1, fp2);
    let stale = matter.get_item(&item.id).expect("reload");
    assert!(!burned_native_fresh(&stale, &fp2));
    assert!(burn_required(&stale, &fp2));
    let _ = RASTER_ENGINE_PIN;
}

#[test]
fn native_change_nulls_burned_and_stales_geom() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-geom-native");
    let matter = Matter::create(&root, "Native").expect("create");
    let item = insert_pdf_item(&matter, b"%PDF-1.4 first");
    let g = make_geom(
        &matter,
        &item.id,
        0,
        5.0,
        6.0,
        7.0,
        8.0,
        geom_source::FULL_PAGE,
    );
    let burned = matter.put_bytes(b"%PDF-1.4 burned").expect("burned");
    let fp_native = matter.geom_burn_fingerprint(&item.id).expect("fp native");
    matter
        .set_burned_native(SetBurnedNativeInput {
            item_id: item.id.clone(),
            burned_native_sha256: burned,
            expected_fingerprint: fp_native,
            actor: "tester".into(),
        })
        .expect("set burned");

    let new_native = matter.put_bytes(b"%PDF-1.4 second").expect("new native");
    matter
        .update_item(
            &item.id,
            ItemUpdate {
                native_sha256: Some(Some(new_native.clone())),
                ..Default::default()
            },
        )
        .expect("update native");

    let reloaded = matter.get_item(&item.id).expect("reload");
    assert_eq!(reloaded.native_sha256.as_deref(), Some(new_native.as_str()));
    assert!(reloaded.burned_native_sha256.is_none());
    assert!(reloaded.burned_native_at.is_none());
    assert!(reloaded.burned_source_digest.is_none());
    assert!(reloaded.raster_engine.is_none());
    assert_eq!(reloaded.geom_redaction_count, 0);

    let listed = matter.list_geom_redactions(&item.id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, g.id);
    assert_eq!(listed[0].status, redaction_status::STALE);
}

#[test]
fn invalid_geom_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-geom-bad");
    let matter = Matter::create(&root, "Bad").expect("create");
    let item = insert_pdf_item(&matter, b"%PDF-1.4 x");
    let err = matter
        .create_geom_redaction(CreateGeomRedactionInput {
            item_id: item.id.clone(),
            page_index: 0,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 10.0,
            reason: redaction_reason::OTHER.into(),
            label: None,
            source: geom_source::DRAW.into(),
            actor: "tester".into(),
        })
        .expect_err("zero w");
    assert!(err.to_string().contains("w and h"));
}
