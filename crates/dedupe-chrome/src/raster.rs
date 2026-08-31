//! Image-tab raster + geometric burn commands (track 0114).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use matter_core::{
    burn_required_pdf_known, burned_native_fresh, geom_source, item_is_pdf_native,
    CreateGeomRedactionInput, Item, Matter, SetBurnedNativeInput,
};
use pdf_raster::{
    burn_native, pixel_to_user_space, quote_unmapped, raster_page, search_hit_rects, BoxF,
    BurnRect, NativeKind, DPI_REVIEW,
};
use serde::{Deserialize, Serialize};

use crate::error::{map_core, CommandError};
use crate::open_root::{open_matter_read, open_matter_write};

const ACTOR: &str = "chrome";

fn map_raster(err: pdf_raster::Error) -> CommandError {
    CommandError {
        kind: err.kind().to_string(),
        message: err.to_string(),
    }
}

fn host_raster_dims(
    claimed_w: f64,
    claimed_h: f64,
    host_w: u32,
    host_h: u32,
) -> Result<(f64, f64), CommandError> {
    let hw = f64::from(host_w.max(1));
    let hh = f64::from(host_h.max(1));
    if (claimed_w - hw).abs() > 0.5 || (claimed_h - hh).abs() > 0.5 {
        return Err(CommandError::failed("raster dimensions mismatch host page"));
    }
    Ok((hw, hh))
}

fn load_native(matter: &Matter, item: &Item) -> Result<Vec<u8>, CommandError> {
    let Some(sha) = item
        .native_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Err(CommandError::failed("item has no native_sha256"));
    };
    matter
        .get_bytes(sha)
        .map_err(|e| CommandError::failed(e.to_string()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewRasterPageArgs {
    pub root: String,
    pub item_id: String,
    pub page_index: Option<u32>,
    pub dpi: Option<u32>,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewRasterPageResponse {
    pub item_id: String,
    pub generation: u64,
    pub png_base64: String,
    pub page_index: u32,
    pub page_count: u32,
    pub media_box: BoxF,
    pub crop_box: BoxF,
    pub rotate: i32,
    pub width: u32,
    pub height: u32,
    pub native_width: u32,
    pub native_height: u32,
    pub kind: String,
    pub truncated: bool,
}

pub fn review_raster_page_blocking(
    args: ReviewRasterPageArgs,
) -> Result<ReviewRasterPageResponse, CommandError> {
    if args.item_id.trim().is_empty() {
        return Err(CommandError::not_found("item not found: "));
    }
    let matter = open_matter_read(&args.root)?;
    let item = matter.get_item(&args.item_id).map_err(map_core)?;
    if item
        .native_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err(CommandError {
            kind: "unsupported_kind".into(),
            message: "Native-only (no print-to-TIFF)".into(),
        });
    }
    let bytes = load_native(&matter, &item)?;
    let page_index = args.page_index.unwrap_or(0);
    let dpi = args.dpi.unwrap_or(DPI_REVIEW);
    let sha = item.native_sha256.clone();
    let page = raster_page(
        &bytes,
        page_index,
        dpi,
        sha.as_deref(),
        item.path.as_deref(),
        item.mime_type.as_deref(),
    )
    .map_err(map_raster)?;
    Ok(ReviewRasterPageResponse {
        item_id: args.item_id,
        generation: args.generation.unwrap_or(0),
        png_base64: STANDARD.encode(&page.png),
        page_index: page.page_index,
        page_count: page.page_count,
        media_box: page.media_box,
        crop_box: page.crop_box,
        rotate: page.rotate,
        width: page.width,
        height: page.height,
        native_width: page.native_width,
        native_height: page.native_height,
        kind: match pdf_raster::sniff_kind(item.path.as_deref(), item.mime_type.as_deref(), &bytes)
        {
            NativeKind::Pdf => "pdf".into(),
            NativeKind::Jpeg => "jpeg".into(),
            NativeKind::Png => "png".into(),
            NativeKind::Tiff => "tiff".into(),
            NativeKind::Other => "other".into(),
        },
        truncated: page.truncated,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GeomDto {
    pub id: String,
    pub item_id: String,
    pub page_index: i64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub reason: String,
    pub label: Option<String>,
    pub status: String,
    pub source: String,
}

impl From<matter_core::ItemGeomRedaction> for GeomDto {
    fn from(g: matter_core::ItemGeomRedaction) -> Self {
        Self {
            id: g.id,
            item_id: g.item_id,
            page_index: g.page_index,
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
            reason: g.reason,
            label: g.label,
            status: g.status,
            source: g.source,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewGeomListArgs {
    pub root: String,
    pub item_id: String,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewGeomListResponse {
    pub item_id: String,
    pub generation: u64,
    pub boxes: Vec<GeomDto>,
}

pub fn review_geom_list_blocking(
    args: ReviewGeomListArgs,
) -> Result<ReviewGeomListResponse, CommandError> {
    let matter = open_matter_read(&args.root)?;
    let boxes = matter
        .list_geom_redactions(&args.item_id)
        .map_err(map_core)?
        .into_iter()
        .map(GeomDto::from)
        .collect();
    Ok(ReviewGeomListResponse {
        item_id: args.item_id,
        generation: args.generation.unwrap_or(0),
        boxes,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewGeomUpsertArgs {
    pub root: String,
    pub item_id: String,
    pub page_index: u32,
    pub px: f64,
    pub py: f64,
    pub pw: f64,
    pub ph: f64,
    pub raster_width: f64,
    pub raster_height: f64,
    pub reason: Option<String>,
    pub label: Option<String>,
    pub source: Option<String>,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewGeomUpsertResponse {
    pub item_id: String,
    pub generation: u64,
    pub geom: GeomDto,
}

pub fn review_geom_upsert_blocking(
    args: ReviewGeomUpsertArgs,
) -> Result<ReviewGeomUpsertResponse, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let item = matter.get_item(&args.item_id).map_err(map_core)?;
    let bytes = load_native(&matter, &item)?;
    let kind = pdf_raster::sniff_kind(item.path.as_deref(), item.mime_type.as_deref(), &bytes);
    let source = args
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(geom_source::DRAW);
    let pixel = BoxF::from_xywh(args.px, args.py, args.pw, args.ph);
    let user = match kind {
        NativeKind::Pdf => {
            let page = raster_page(
                &bytes,
                args.page_index,
                DPI_REVIEW,
                item.native_sha256.as_deref(),
                item.path.as_deref(),
                item.mime_type.as_deref(),
            )
            .map_err(map_raster)?;
            if source == geom_source::FULL_PAGE {
                page.media_box
            } else {
                let (hw, hh) = host_raster_dims(
                    args.raster_width,
                    args.raster_height,
                    page.width,
                    page.height,
                )?;
                pixel_to_user_space(pixel, hw, hh, page.crop_box, page.rotate)
            }
        }
        NativeKind::Jpeg | NativeKind::Png | NativeKind::Tiff => {
            let page = raster_page(
                &bytes,
                if kind == NativeKind::Tiff {
                    args.page_index
                } else {
                    0
                },
                DPI_REVIEW,
                item.native_sha256.as_deref(),
                item.path.as_deref(),
                item.mime_type.as_deref(),
            )
            .map_err(map_raster)?;
            if source == geom_source::FULL_PAGE {
                BoxF::from_xywh(
                    0.0,
                    0.0,
                    f64::from(page.native_width.max(1)),
                    f64::from(page.native_height.max(1)),
                )
            } else {
                let (dw, dh) = host_raster_dims(
                    args.raster_width,
                    args.raster_height,
                    page.width,
                    page.height,
                )?;
                let nw = f64::from(page.native_width.max(1));
                let nh = f64::from(page.native_height.max(1));
                BoxF::from_xywh(
                    pixel.x * nw / dw,
                    pixel.y * nh / dh,
                    pixel.w * nw / dw,
                    pixel.h * nh / dh,
                )
            }
        }
        NativeKind::Other => {
            return Err(CommandError {
                kind: "unsupported_kind".into(),
                message: "Native-only (no print-to-TIFF)".into(),
            });
        }
    };
    let geom = matter
        .create_geom_redaction(CreateGeomRedactionInput {
            item_id: args.item_id.clone(),
            page_index: args.page_index as i64,
            x: user.x,
            y: user.y,
            w: user.w,
            h: user.h,
            reason: args.reason.unwrap_or_else(|| "privilege".into()),
            label: args.label,
            source: source.to_string(),
            actor: ACTOR.into(),
        })
        .map_err(map_core)?;
    Ok(ReviewGeomUpsertResponse {
        item_id: args.item_id,
        generation: args.generation.unwrap_or(0),
        geom: GeomDto::from(geom),
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewGeomDeleteArgs {
    pub root: String,
    pub geom_id: String,
}

pub fn review_geom_delete_blocking(args: ReviewGeomDeleteArgs) -> Result<(), CommandError> {
    let matter = open_matter_write(&args.root)?;
    matter
        .delete_geom_redaction(&args.geom_id, ACTOR)
        .map_err(map_core)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewGeomFromHitsArgs {
    pub root: String,
    pub item_id: String,
    pub query: Option<String>,
    pub reason: Option<String>,
    pub generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewGeomFromHitsResponse {
    pub item_id: String,
    pub generation: u64,
    pub inserted: u64,
    pub hit_count: u64,
    pub unmapped: bool,
}

pub fn review_geom_from_hits_blocking(
    args: ReviewGeomFromHitsArgs,
) -> Result<ReviewGeomFromHitsResponse, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let item = matter.get_item(&args.item_id).map_err(map_core)?;
    let mut queries: Vec<String> = Vec::new();
    if let Some(q) = args
        .query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        queries.push(q.to_string());
    }
    let reds = matter.list_redactions(&args.item_id).map_err(map_core)?;
    for r in reds.iter().filter(|r| r.status == "active") {
        let q = r.exact_quote.trim();
        if !q.is_empty() && !queries.iter().any(|existing| existing == q) {
            queries.push(q.to_string());
        }
    }
    let bytes = load_native(&matter, &item)?;
    let mut hits = Vec::new();
    for q in &queries {
        hits.extend(search_hit_rects(&bytes, q).map_err(map_raster)?);
    }
    let reason = args.reason.unwrap_or_else(|| "privilege".into());
    let mut inserted = 0u64;
    for h in &hits {
        matter
            .create_geom_redaction(CreateGeomRedactionInput {
                item_id: args.item_id.clone(),
                page_index: h.page_index as i64,
                x: h.x,
                y: h.y,
                w: h.w,
                h: h.h,
                reason: reason.clone(),
                label: None,
                source: geom_source::HIT.into(),
                actor: ACTOR.into(),
            })
            .map_err(map_core)?;
        inserted += 1;
    }
    let item2 = matter.get_item(&args.item_id).map_err(map_core)?;
    let active = matter
        .list_active_geom_redactions(&args.item_id)
        .map_err(map_core)?;
    let rects: Vec<BurnRect> = active
        .iter()
        .map(|g| BurnRect {
            page_index: g.page_index.max(0) as u32,
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
        })
        .collect();
    let mut unmapped = item2.redaction_count > 0 && item2.geom_redaction_count == 0;
    if item2.redaction_count > 0 {
        for r in reds.iter().filter(|r| r.status == "active") {
            if quote_unmapped(&bytes, &r.exact_quote, &rects).map_err(map_raster)? {
                unmapped = true;
                break;
            }
        }
    }
    Ok(ReviewGeomFromHitsResponse {
        item_id: args.item_id,
        generation: args.generation.unwrap_or(0),
        inserted,
        hit_count: hits.len() as u64,
        unmapped,
    })
}

fn burn_one_item(matter: &Matter, item_id: &str) -> Result<Item, CommandError> {
    let item = matter.get_item(item_id).map_err(map_core)?;
    let fp = matter.geom_burn_fingerprint(item_id).map_err(map_core)?;
    let is_pdf = item_is_pdf_native(matter, &item).map_err(map_core)?;
    if !burn_required_pdf_known(&item, &fp, is_pdf) {
        return Ok(item);
    }
    let bytes = load_native(matter, &item)?;
    let geoms = matter
        .list_active_geom_redactions(item_id)
        .map_err(map_core)?;
    let rects: Vec<BurnRect> = geoms
        .iter()
        .map(|g| BurnRect {
            page_index: g.page_index.max(0) as u32,
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
        })
        .collect();
    if is_pdf && item.redaction_count > 0 {
        if geoms.is_empty() {
            return Err(CommandError::failed("text_redact_unmapped_on_pdf"));
        }
        let reds = matter.list_redactions(item_id).map_err(map_core)?;
        for r in reds.iter().filter(|r| r.status == "active") {
            if quote_unmapped(&bytes, &r.exact_quote, &rects).map_err(map_raster)? {
                return Err(CommandError::failed("text_redact_unmapped_on_pdf"));
            }
        }
    }
    let burned = burn_native(
        &bytes,
        &rects,
        item.path.as_deref(),
        item.mime_type.as_deref(),
    )
    .map_err(map_raster)?;
    let sha = matter
        .put_bytes(&burned)
        .map_err(|e| CommandError::failed(e.to_string()))?;
    matter
        .set_burned_native(SetBurnedNativeInput {
            item_id: item_id.to_string(),
            burned_native_sha256: sha,
            expected_fingerprint: fp,
            actor: ACTOR.into(),
        })
        .map_err(map_core)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewBurnNativeArgs {
    pub root: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewBurnNativeResponse {
    pub item_id: String,
    pub burned_native_sha256: Option<String>,
    pub native_sha256: Option<String>,
}

pub fn review_burn_native_blocking(
    args: ReviewBurnNativeArgs,
) -> Result<ReviewBurnNativeResponse, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let item = burn_one_item(&matter, &args.item_id)?;
    Ok(ReviewBurnNativeResponse {
        item_id: args.item_id,
        burned_native_sha256: item.burned_native_sha256,
        native_sha256: item.native_sha256,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProduceBurnSetArgs {
    pub root: String,
    pub item_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProduceBurnSetResponse {
    pub burned: u64,
    pub skipped: u64,
    pub errors: Vec<String>,
}

pub fn produce_burn_set_blocking(
    args: ProduceBurnSetArgs,
) -> Result<ProduceBurnSetResponse, CommandError> {
    let matter = open_matter_write(&args.root)?;
    let ids = args.item_ids.unwrap_or_default();
    let mut burned = 0u64;
    let mut skipped = 0u64;
    let mut errors = Vec::new();
    for id in ids {
        let item = match matter.get_item(&id) {
            Ok(i) => i,
            Err(e) => {
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };
        let fp = match matter.geom_burn_fingerprint(&id) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };
        let is_pdf = match item_is_pdf_native(&matter, &item) {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };
        if !burn_required_pdf_known(&item, &fp, is_pdf) || burned_native_fresh(&item, &fp) {
            skipped += 1;
            continue;
        }
        match burn_one_item(&matter, &id) {
            Ok(_) => burned += 1,
            Err(e) => errors.push(format!("{id}: {e}")),
        }
    }
    Ok(ProduceBurnSetResponse {
        burned,
        skipped,
        errors,
    })
}

pub(crate) fn burn_counts_for_ids(
    matter: &Matter,
    ids: &[String],
) -> Result<(u64, u64, u64), CommandError> {
    let mut need = 0u64;
    let mut fresh = 0u64;
    let mut unmapped = 0u64;
    for id in ids {
        let item = matter.get_item(id).map_err(map_core)?;
        let fp = matter.geom_burn_fingerprint(id).map_err(map_core)?;
        let is_pdf = item_is_pdf_native(matter, &item).map_err(map_core)?;
        if burn_required_pdf_known(&item, &fp, is_pdf) {
            if burned_native_fresh(&item, &fp) {
                fresh += 1;
            } else {
                need += 1;
            }
        }
        if is_pdf && item.redaction_count > 0 && item.geom_redaction_count == 0 {
            unmapped += 1;
        }
    }
    Ok((need, fresh, unmapped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use crate::open_root::open_matter_read;
    use crate::produce::{
        produce_page_blocking, produce_qc_findings_blocking, produce_qc_run_blocking,
        ProducePageResponse, ProduceQcRunArgs, ProduceQcRunResponse,
    };
    use matter_core::{
        is_encrypted_matter, item_role, item_status, CreateRedactionInput, ItemInput, ItemUpdate,
        Matter,
    };
    use pdf_raster::{
        search_hit_rects, synthetic_text_pdf, synthetic_two_label_pdf, user_space_to_pixel,
    };
    use process_runner::{register_default_handlers, ProcessRunner, RunnerConfig};
    use std::time::Duration;
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    fn test_runner() -> ProcessRunner {
        let mut r = ProcessRunner::new(RunnerConfig::default());
        register_default_handlers(&mut r);
        r
    }

    fn qc_wait(runner: &ProcessRunner, args: ProduceQcRunArgs) -> ProduceQcRunResponse {
        let started = produce_qc_run_blocking(runner, args.clone()).expect("qc start");
        if started.job_id.is_none() {
            return started;
        }
        assert!(
            runner.wait_until_idle(Duration::from_secs(120)),
            "qc did not idle"
        );
        produce_qc_findings_blocking(&args.root, started.job_id.clone()).expect("qc findings")
    }

    fn seed_pdf_item(root: &camino::Utf8Path, id: &str, pdf: &[u8]) -> String {
        let matter = Matter::open(root).expect("open");
        let sha = matter.put_bytes(pdf).expect("cas");
        matter
            .insert_item(ItemInput {
                id: Some(id.into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                native_sha256: Some(sha.clone()),
                path: Some(format!("{id}.pdf")),
                mime_type: Some("application/pdf".into()),
                file_category: Some("pdf".into()),
                in_review: Some(1),
                ..Default::default()
            })
            .expect("item");
        sha
    }

    #[test]
    fn raster_returns_png_not_stub() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "RasterPng").expect("create");
        let pdf = synthetic_text_pdf(&[("SECRET_TOKEN_0114", 0)]);
        assert!(extract_pdf::looks_like_pdf(&pdf));
        seed_pdf_item(&root, "itm_pdf", &pdf);
        let resp = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "itm_pdf".into(),
            page_index: Some(0),
            dpi: Some(72),
            generation: Some(7),
        })
        .expect("raster");
        let png = STANDARD.decode(&resp.png_base64).expect("b64");
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert_eq!(resp.generation, 7);
        assert_eq!(resp.page_count, 1);
    }

    #[test]
    fn geom_create_does_not_change_native() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "GeomNative").expect("create");
        let pdf = synthetic_text_pdf(&[("SECRET_TOKEN_0114", 0)]);
        let sha = seed_pdf_item(&root, "itm_g", &pdf);
        let page = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "itm_g".into(),
            page_index: Some(0),
            dpi: Some(DPI_REVIEW),
            generation: Some(1),
        })
        .expect("raster");
        let up = review_geom_upsert_blocking(ReviewGeomUpsertArgs {
            root: root.to_string(),
            item_id: "itm_g".into(),
            page_index: 0,
            px: 10.0,
            py: 10.0,
            pw: 40.0,
            ph: 20.0,
            raster_width: f64::from(page.width),
            raster_height: f64::from(page.height),
            reason: Some("privilege".into()),
            label: None,
            source: Some("draw".into()),
            generation: Some(1),
        })
        .expect("upsert");
        assert_eq!(up.geom.source, "draw");
        let matter = Matter::open_for_read(&root).expect("read");
        let item = matter.get_item("itm_g").expect("item");
        assert_eq!(item.native_sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(item.geom_redaction_count, 1);
    }

    #[test]
    fn produce_without_burn_fail_closes_then_burn_resolves() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "BurnClose").expect("create");
        let pdf = synthetic_text_pdf(&[("SECRET_TOKEN_0114", 0)]);
        let orig = seed_pdf_item(&root, "itm_b", &pdf);
        let page = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "itm_b".into(),
            page_index: Some(0),
            dpi: Some(DPI_REVIEW),
            generation: Some(1),
        })
        .expect("raster");
        review_geom_upsert_blocking(ReviewGeomUpsertArgs {
            root: root.to_string(),
            item_id: "itm_b".into(),
            page_index: 0,
            px: 8.0,
            py: 8.0,
            pw: 200.0,
            ph: 40.0,
            raster_width: f64::from(page.width),
            raster_height: f64::from(page.height),
            reason: Some("privilege".into()),
            label: None,
            source: Some("draw".into()),
            generation: None,
        })
        .expect("geom");
        let runner = test_runner();
        let qc_need = qc_wait(
            &runner,
            ProduceQcRunArgs {
                root: root.to_string(),
                filter_json: None,
                item_ids: None,
                production_profile: None,
                source_entire_corpus: Some(true),
            },
        );
        assert!(
            qc_need.need_burn >= 1,
            "entire-corpus QC should count the geom item; need_burn={} ordered={:?}",
            qc_need.need_burn,
            qc_need.ordered_ids
        );
        let natives = root.join("NATIVES");
        std::fs::create_dir_all(natives.as_std_path()).expect("dir");
        {
            let matter = Matter::open(&root).expect("open");
            let item = matter.get_item("itm_b").expect("item");
            let r = matter_produce::resolve::resolve_native(
                &matter,
                &item,
                false,
                natives.as_std_path(),
                "PROD000001",
                None,
            )
            .expect("resolve");
            assert_eq!(r.err().as_deref(), Some("burned_native_missing"));
        }

        let burned = review_burn_native_blocking(ReviewBurnNativeArgs {
            root: root.to_string(),
            item_id: "itm_b".into(),
        })
        .expect("burn");
        assert_ne!(burned.burned_native_sha256.as_deref(), Some(orig.as_str()));
        let matter = Matter::open(&root).expect("open2");
        let item = matter.get_item("itm_b").expect("item2");
        let r2 = matter_produce::resolve::resolve_native(
            &matter,
            &item,
            false,
            natives.as_std_path(),
            "PROD000002",
            None,
        )
        .expect("resolve2")
        .expect("ok");
        assert_eq!(
            r2.sha256,
            item.burned_native_sha256.clone().unwrap_or_default()
        );
        let orig_bytes = matter.get_bytes(&orig).expect("orig cas");
        assert!(orig_bytes
            .windows(b"SECRET_TOKEN_0114".len())
            .any(|w| w == b"SECRET_TOKEN_0114"));
        let burned_bytes = std::fs::read(&r2.abs_path).expect("read burned");
        assert!(!burned_bytes
            .windows(b"SECRET_TOKEN_0114".len())
            .any(|w| w == b"SECRET_TOKEN_0114"));
    }

    #[test]
    fn rotate90_visible_token_burns_via_host_upsert() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Rot90Host").expect("create");
        let pdf = synthetic_two_label_pdf("SECRET_TOKEN_0114", "NEIGHBOR_TOKEN_0114", 90);
        let orig = seed_pdf_item(&root, "itm_r90", &pdf);
        let page = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "itm_r90".into(),
            page_index: Some(0),
            dpi: Some(DPI_REVIEW),
            generation: Some(1),
        })
        .expect("raster");
        let hits = search_hit_rects(&pdf, "SECRET_TOKEN_0114").expect("hits");
        assert!(!hits.is_empty());
        let user = BoxF::from_xywh(hits[0].x, hits[0].y, hits[0].w, hits[0].h);
        let px = user_space_to_pixel(
            user,
            f64::from(page.width.max(1)),
            f64::from(page.height.max(1)),
            page.crop_box,
            page.rotate,
        );
        review_geom_upsert_blocking(ReviewGeomUpsertArgs {
            root: root.to_string(),
            item_id: "itm_r90".into(),
            page_index: 0,
            px: px.x,
            py: px.y,
            pw: px.w.max(2.0),
            ph: px.h.max(2.0),
            raster_width: f64::from(page.width.max(1)),
            raster_height: f64::from(page.height.max(1)),
            reason: Some("privilege".into()),
            label: None,
            source: Some("draw".into()),
            generation: Some(1),
        })
        .expect("upsert");
        let burned = review_burn_native_blocking(ReviewBurnNativeArgs {
            root: root.to_string(),
            item_id: "itm_r90".into(),
        })
        .expect("burn");
        let sha = burned.burned_native_sha256.expect("burned sha");
        assert_ne!(sha, orig);
        let matter = Matter::open_for_read(&root).expect("read");
        let bytes = matter.get_bytes(&sha).expect("burned cas");
        assert!(!bytes
            .windows(b"SECRET_TOKEN_0114".len())
            .any(|w| w == b"SECRET_TOKEN_0114"));
        assert!(
            bytes
                .windows(b"NEIGHBOR_TOKEN_0114".len())
                .any(|w| w == b"NEIGHBOR_TOKEN_0114")
                || !search_hit_rects(&bytes, "NEIGHBOR_TOKEN_0114")
                    .expect("n")
                    .is_empty(),
            "neighbor must survive rotate-90 host burn"
        );
    }

    #[test]
    fn geom_upsert_scales_from_claimed_raster_size() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "GeomScale").expect("create");
        let pdf = synthetic_text_pdf(&[("SECRET_TOKEN_0114", 0)]);
        seed_pdf_item(&root, "itm_sc", &pdf);
        let page = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "itm_sc".into(),
            page_index: Some(0),
            dpi: Some(DPI_REVIEW),
            generation: Some(1),
        })
        .expect("raster");
        let rw = f64::from(page.width.max(1));
        let rh = f64::from(page.height.max(1));
        let a = review_geom_upsert_blocking(ReviewGeomUpsertArgs {
            root: root.to_string(),
            item_id: "itm_sc".into(),
            page_index: 0,
            px: 20.0,
            py: 30.0,
            pw: 40.0,
            ph: 16.0,
            raster_width: rw,
            raster_height: rh,
            reason: Some("privilege".into()),
            label: None,
            source: Some("draw".into()),
            generation: Some(1),
        })
        .expect("a");
        let err = review_geom_upsert_blocking(ReviewGeomUpsertArgs {
            root: root.to_string(),
            item_id: "itm_sc".into(),
            page_index: 0,
            px: 20.0,
            py: 30.0,
            pw: 40.0,
            ph: 16.0,
            raster_width: rw * 2.0,
            raster_height: rh * 2.0,
            reason: Some("privilege".into()),
            label: None,
            source: Some("draw".into()),
            generation: Some(1),
        })
        .expect_err("mismatch");
        assert!(
            err.message.contains("raster dimensions mismatch"),
            "got {}",
            err.message
        );
        assert!(a.geom.w > 0.0);
    }

    #[test]
    fn second_text_redaction_without_geom_blocks_reburn() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "SecondText").expect("create");
        let pdf = synthetic_text_pdf(&[("SECRET_TOKEN_0114 NEIGHBOR_TOKEN_0114", 0)]);
        seed_pdf_item(&root, "itm_t2", &pdf);
        let hits = review_geom_from_hits_blocking(ReviewGeomFromHitsArgs {
            root: root.to_string(),
            item_id: "itm_t2".into(),
            query: Some("SECRET_TOKEN_0114".into()),
            reason: Some("privilege".into()),
            generation: Some(1),
        })
        .expect("hits");
        assert!(hits.hit_count > 0);
        review_burn_native_blocking(ReviewBurnNativeArgs {
            root: root.to_string(),
            item_id: "itm_t2".into(),
        })
        .expect("burn A");

        let body = "Alpha NEIGHBOR_TOKEN_0114 beta";
        let quote = "NEIGHBOR_TOKEN_0114";
        let start = i64::try_from(body.find(quote).expect("quote in body")).expect("start");
        let end = start + i64::try_from(quote.chars().count()).expect("len");
        {
            let matter = Matter::open(&root).expect("open");
            let text_sha = matter.put_bytes(body.as_bytes()).expect("text");
            matter
                .update_item(
                    "itm_t2",
                    ItemUpdate {
                        text_sha256: Some(Some(text_sha.clone())),
                        ..Default::default()
                    },
                )
                .expect("set text");
            matter
                .create_redaction(CreateRedactionInput {
                    item_id: "itm_t2".into(),
                    start_utf8: start,
                    end_utf8: end,
                    exact_quote: quote.into(),
                    display_body: body.into(),
                    body_digest: text_sha,
                    reason: "confidential".into(),
                    label: None,
                    actor: "tester".into(),
                })
                .expect("redact B");
        }
        let err = review_burn_native_blocking(ReviewBurnNativeArgs {
            root: root.to_string(),
            item_id: "itm_t2".into(),
        })
        .expect_err("unmapped B");
        assert!(
            err.message.contains("text_redact_unmapped_on_pdf"),
            "got {}",
            err.message
        );
    }

    #[test]
    fn eml_without_native_is_not_a_page_image() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "EmlEmpty").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_eml".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    path: Some("msg.eml".into()),
                    mime_type: Some("message/rfc822".into()),
                    in_review: Some(1),
                    ..Default::default()
                })
                .expect("eml");
        }
        let err = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "itm_eml".into(),
            page_index: None,
            dpi: None,
            generation: None,
        })
        .expect_err("eml");
        assert_eq!(err.kind, "unsupported_kind");
        assert!(
            err.message.to_ascii_lowercase().contains("native-only")
                || err
                    .message
                    .to_ascii_lowercase()
                    .contains("not a page image"),
            "got {}",
            err.message
        );
    }

    #[test]
    fn produce_extras_warn_when_pdf_has_no_native() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "PdfNoNative").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_pdf_empty".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    path: Some("empty.pdf".into()),
                    mime_type: Some("application/pdf".into()),
                    file_category: Some("pdf".into()),
                    in_review: Some(1),
                    ..Default::default()
                })
                .expect("pdf");
        }
        let runner = test_runner();
        let qc = qc_wait(
            &runner,
            ProduceQcRunArgs {
                root: root.to_string(),
                filter_json: None,
                item_ids: None,
                production_profile: None,
                source_entire_corpus: Some(true),
            },
        );
        assert!(
            qc.extras.iter().any(|e| {
                e.kind == "pdf_raster_failed"
                    && e.severity == "warning"
                    && e.item_id.as_deref() == Some("itm_pdf_empty")
            }),
            "pdf_raster_failed extra: {:?}",
            qc.extras
        );
    }

    #[test]
    fn encrypted_matter_kind() {
        let tmp = tempdir().expect("tmp");
        let parent = utf8_tmp(&tmp);
        let root = parent.join("EncRaster");
        {
            let _m =
                Matter::create_encrypted(&root, "EncRaster", "test-passphrase-0114").expect("enc");
        }
        assert!(is_encrypted_matter(&root));
        let err = review_raster_page_blocking(ReviewRasterPageArgs {
            root: root.to_string(),
            item_id: "x".into(),
            page_index: None,
            dpi: None,
            generation: None,
        })
        .expect_err("enc");
        assert_eq!(err.kind, "encrypted");
        assert!(
            open_matter_read(root.as_str()).is_err(),
            "encrypted matter must reject open_root"
        );
    }

    #[test]
    fn allow_permission_files_exist() {
        let raster = include_str!("../permissions/autogenerated/review_raster_page.toml");
        let list = include_str!("../permissions/autogenerated/review_geom_list.toml");
        let up = include_str!("../permissions/autogenerated/review_geom_upsert.toml");
        let del = include_str!("../permissions/autogenerated/review_geom_delete.toml");
        let hits = include_str!("../permissions/autogenerated/review_geom_from_hits.toml");
        let burn = include_str!("../permissions/autogenerated/review_burn_native.toml");
        let set = include_str!("../permissions/autogenerated/produce_burn_set.toml");
        assert!(raster.contains("allow-review-raster-page"));
        assert!(list.contains("allow-review-geom-list"));
        assert!(up.contains("allow-review-geom-upsert"));
        assert!(del.contains("allow-review-geom-delete"));
        assert!(hits.contains("allow-review-geom-from-hits"));
        assert!(burn.contains("allow-review-burn-native"));
        assert!(set.contains("allow-produce-burn-set"));
        let _p: Option<ProducePageResponse> = None;
        let _ = produce_page_blocking;
    }
}
