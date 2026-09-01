//! CPU PDF raster + geometric burn (track **0114**).
//!
//! Burn compose (normative):
//! `IncrementalWriter::new` → `redact_page` × N → `write(Cursor)` →
//! `PdfFile::parse` → `rewrite_pdf`. `iw.document()` is forbidden as rewrite input.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use extract_pdf::{detect_pdf, looks_like_pdf, MAX_NATIVE_INPUT_BYTES};

pub use extract_pdf::MAX_PAGES;

pub mod g4;
pub use g4::{
    count_ifds_or_zero, decode_tiff_ifd, encode_g4_tif, looks_like_tiff, native_image_page_count,
    parse_le_ifd0_tags, raster_and_encode_document, raster_and_encode_page, read_le_rational,
    stamp_bates_lower_right, synthetic_gray8_tiff, tiff_ifd_count, wrap_g4_le_ifd, G4Page,
    BATES_MARGIN_IN, BT601_B, BT601_BLACK_THRESHOLD, BT601_G, BT601_R,
};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zpdf::cpu::{CpuRenderer, RenderedPage};
use zpdf::{
    rewrite_pdf, search_spans, ContentInterpreter, FontCache, ImageCache, IncrementalWriter,
    ParseLimits, PdfDocument, PdfFile, Rect, RenderBackend, RewriteOptions, TextSpan,
};
use zpdf_writer::redact::RedactOptions;

pub mod coords;
pub mod error;

pub use coords::{
    normalize_rotate, pixel_to_user_space, user_space_to_pixel, visual_size, BoxF, PageBoxes,
};
pub use error::{Error, Result};

/// Engine pin stored in cache keys and burn fingerprints.
pub const ENGINE_PIN: &str = "zpdf-0.13.0";
/// Review raster DPI.
pub const DPI_REVIEW: u32 = 150;
/// Produce raster DPI (TIFF G4).
pub const DPI_PRODUCE: u32 = 300;
/// Thumbnail DPI.
pub const DPI_THUMB: u32 = 72;
/// Long-side pixel cap.
pub const LONG_SIDE_CAP: u32 = 4096;
/// In-process raster LRU size (pages).
pub const RASTER_CACHE_PAGES: usize = 32;

/// Raster result: PNG bytes plus page geometry for overlay mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterPage {
    pub png: Vec<u8>,
    pub page_index: u32,
    pub page_count: u32,
    pub media_box: BoxF,
    pub crop_box: BoxF,
    pub rotate: i32,
    pub width: u32,
    pub height: u32,
    /// Pre-cap native pixel size (JPEG/PNG). Equals width/height when uncapped or PDF.
    pub native_width: u32,
    pub native_height: u32,
    pub truncated: bool,
}

/// User-space (PDF) or pixel-space (JPEG/PNG) rect for burn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BurnRect {
    pub page_index: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Kind of native payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKind {
    Pdf,
    Jpeg,
    Png,
    Tiff,
    Other,
}

struct CacheEntry {
    png: Vec<u8>,
    page_count: u32,
    media: BoxF,
    crop: BoxF,
    rotate: i32,
    width: u32,
    height: u32,
    truncated: bool,
}

struct RasterLru {
    map: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
}

impl RasterLru {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &str) -> Option<CacheEntry> {
        if self.map.contains_key(key) {
            self.order.retain(|k| k != key);
            self.order.push_back(key.to_string());
            self.map.get(key).cloned()
        } else {
            None
        }
    }

    fn put(&mut self, key: String, entry: CacheEntry) {
        if self.map.contains_key(&key) {
            self.order.retain(|k| k != &key);
        }
        while self.map.len() >= RASTER_CACHE_PAGES {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, entry);
    }
}

impl Clone for CacheEntry {
    fn clone(&self) -> Self {
        Self {
            png: self.png.clone(),
            page_count: self.page_count,
            media: self.media,
            crop: self.crop,
            rotate: self.rotate,
            width: self.width,
            height: self.height,
            truncated: self.truncated,
        }
    }
}

static RASTER_CACHE: Mutex<Option<RasterLru>> = Mutex::new(None);

fn with_cache<T>(f: impl FnOnce(&mut RasterLru) -> T) -> Option<T> {
    let mut guard = RASTER_CACHE.lock().ok()?;
    if guard.is_none() {
        *guard = Some(RasterLru::new());
    }
    guard.as_mut().map(f)
}

fn cache_key(native_sha256: &str, page: u32, dpi: u32) -> String {
    format!("raster-v1|{native_sha256}|p{page}|dpi{dpi}|{ENGINE_PIN}")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8
}

pub fn looks_like_png(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47
}

fn sniff_path_kind(path: Option<&str>) -> Option<NativeKind> {
    let l = path?.to_ascii_lowercase();
    if l.ends_with(".tif") || l.ends_with(".tiff") {
        return Some(NativeKind::Tiff);
    }
    if l.ends_with(".jpg") || l.ends_with(".jpeg") {
        return Some(NativeKind::Jpeg);
    }
    if l.ends_with(".png") {
        return Some(NativeKind::Png);
    }
    None
}

fn sniff_mime_kind(mime: Option<&str>) -> Option<NativeKind> {
    match mime.map(|m| m.to_ascii_lowercase()).as_deref() {
        Some("image/tiff") | Some("image/tif") => Some(NativeKind::Tiff),
        Some("image/jpeg") | Some("image/jpg") => Some(NativeKind::Jpeg),
        Some("image/png") => Some(NativeKind::Png),
        _ => None,
    }
}

pub fn sniff_kind(path: Option<&str>, mime: Option<&str>, bytes: &[u8]) -> NativeKind {
    if detect_pdf(path, mime, Some(bytes)) || looks_like_pdf(bytes) {
        return NativeKind::Pdf;
    }
    if g4::looks_like_tiff(bytes) {
        return NativeKind::Tiff;
    }
    if looks_like_jpeg(bytes) {
        return NativeKind::Jpeg;
    }
    if looks_like_png(bytes) {
        return NativeKind::Png;
    }
    let mime = mime.map(str::trim).filter(|s| !s.is_empty());
    if let Some(kind) = sniff_path_kind(path) {
        // TIFF path always. JPEG/PNG path after magic beats MIME when a MIME
        // is present; path-only `.jpg`/`.png` without magic or MIME stays Other
        // so native-only garbage files are not image-eligible (track **0121**).
        match kind {
            NativeKind::Tiff => return kind,
            NativeKind::Jpeg | NativeKind::Png if mime.is_some() => return kind,
            _ => {}
        }
    }
    if let Some(kind) = sniff_mime_kind(mime) {
        return kind;
    }
    NativeKind::Other
}

/// True when sniff identifies a PDF/JPEG/PNG/TIFF native the image profile
/// must rasterize (not native-only DAT). Path-only JPEG/PNG without magic
/// or MIME is not eligible.
pub fn is_image_eligible_native(path: Option<&str>, mime: Option<&str>, bytes: &[u8]) -> bool {
    !matches!(sniff_kind(path, mime, bytes), NativeKind::Other)
}

fn reject_size(len: usize) -> Result<()> {
    let n = len as u64;
    if n > MAX_NATIVE_INPUT_BYTES {
        return Err(Error::TooLarge { bytes: n });
    }
    Ok(())
}

fn map_zpdf(err: zpdf::Error) -> Error {
    let s = err.to_string();
    let low = s.to_ascii_lowercase();
    if low.contains("encrypt") || low.contains("password") {
        Error::Encrypted
    } else {
        Error::Corrupt(s)
    }
}

fn box_from_rect(r: Rect) -> BoxF {
    BoxF::from_corners(r.x0, r.y0, r.x1, r.y1)
}

pub(crate) fn encode_png_rgba(width: u32, height: u32, data: Vec<u8>) -> Result<Vec<u8>> {
    let img = RgbaImage::from_raw(width, height, data).ok_or(Error::PngEncodeFailed)?;
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, ImageFormat::Png)
        .map_err(|_| Error::PngEncodeFailed)?;
    Ok(buf.into_inner())
}

pub(crate) fn cap_long_side(rgba: RgbaImage) -> (RgbaImage, bool) {
    let long = rgba.width().max(rgba.height());
    if long <= LONG_SIDE_CAP {
        return (rgba, false);
    }
    let scale = f64::from(LONG_SIDE_CAP) / f64::from(long);
    let w = ((f64::from(rgba.width()) * scale).max(1.0)) as u32;
    let h = ((f64::from(rgba.height()) * scale).max(1.0)) as u32;
    (
        DynamicImage::ImageRgba8(rgba)
            .resize_exact(w, h, image::imageops::FilterType::Triangle)
            .to_rgba8(),
        true,
    )
}

fn raster_image_to_png(bytes: &[u8]) -> Result<RasterPage> {
    let dyn_img = image::load_from_memory(bytes).map_err(|e| Error::ImageDecode(e.to_string()))?;
    let src = dyn_img.to_rgba8();
    let native_width = src.width();
    let native_height = src.height();
    let (rgba, truncated) = cap_long_side(src);
    let width = rgba.width();
    let height = rgba.height();
    let png = encode_png_rgba(width, height, rgba.into_raw())?;
    Ok(RasterPage {
        png,
        page_index: 0,
        page_count: 1,
        media_box: BoxF::from_xywh(0.0, 0.0, width as f64, height as f64),
        crop_box: BoxF::from_xywh(0.0, 0.0, width as f64, height as f64),
        rotate: 0,
        width,
        height,
        native_width,
        native_height,
        truncated,
    })
}

pub(crate) fn open_pdf(bytes: &[u8]) -> Result<PdfDocument> {
    match PdfDocument::open(bytes.to_vec()) {
        Ok(doc) => {
            if doc.is_encrypted() {
                return Err(Error::Encrypted);
            }
            Ok(doc)
        }
        Err(e) => Err(map_zpdf(e)),
    }
}

pub(crate) fn pdf_page_count(bytes: &[u8]) -> Result<u32> {
    let doc = open_pdf(bytes)?;
    let n = doc.page_count();
    if n > MAX_PAGES {
        return Err(Error::TooManyPages { count: n });
    }
    Ok(n as u32)
}

fn raster_pdf_page_inner(bytes: &[u8], page_index: u32, dpi: u32) -> Result<RasterPage> {
    let doc = open_pdf(bytes)?;
    let page_count = doc.page_count();
    if page_count > MAX_PAGES {
        return Err(Error::TooManyPages { count: page_count });
    }
    if page_index as usize >= page_count {
        return Err(Error::PageOutOfRange {
            index: page_index,
            count: page_count as u32,
        });
    }
    let page = doc.page(page_index as usize).map_err(map_zpdf)?;
    let media = box_from_rect(page.media_box);
    let crop = box_from_rect(page.effective_box());
    let rotate = page.rotate;
    let content = doc.page_content_bytes(&page).map_err(map_zpdf)?;
    let mut fonts: FontCache = doc.load_page_fonts(&page);
    let mut images = ImageCache::default();
    let annots = doc.page_annotations(&page);
    let interpreter = ContentInterpreter::new(page.effective_box())
        .with_page_rotation(page.rotate)
        .with_document(doc.file(), &page.resources)
        .with_fonts(&mut fonts)
        .with_images(&mut images)
        .with_annotations(&annots)
        .with_limits(&ParseLimits::default());
    let list = interpreter.interpret(&content);

    let (vis_w, vis_h) = visual_size(crop, rotate);
    let mut scale = dpi as f32 / 72.0;
    let pix_w = vis_w * f64::from(scale);
    let pix_h = vis_h * f64::from(scale);
    let long = pix_w.max(pix_h);
    let mut truncated = false;
    if long > f64::from(LONG_SIDE_CAP) && long > 0.0 {
        scale *= (f64::from(LONG_SIDE_CAP) / long) as f32;
        truncated = true;
    }

    let mut renderer = CpuRenderer::new().with_limits(&ParseLimits::default());
    let rendered: RenderedPage = renderer
        .render_display_list(&list, scale)
        .map_err(|e| Error::Raster(e.to_string()))?;
    let width = rendered.width;
    let height = rendered.height;
    let png = encode_png_rgba(width, height, rendered.data)?;
    Ok(RasterPage {
        png,
        page_index,
        page_count: page_count as u32,
        media_box: media,
        crop_box: crop,
        rotate,
        width,
        height,
        native_width: width,
        native_height: height,
        truncated,
    })
}

/// Raster one page (or a JPEG/PNG native) to PNG bytes.
pub fn raster_page(
    bytes: &[u8],
    page_index: u32,
    dpi: u32,
    native_sha256: Option<&str>,
    path: Option<&str>,
    mime: Option<&str>,
) -> Result<RasterPage> {
    reject_size(bytes.len())?;
    let kind = sniff_kind(path, mime, bytes);
    match kind {
        NativeKind::Jpeg | NativeKind::Png => {
            if page_index != 0 {
                return Err(Error::PageOutOfRange {
                    index: page_index,
                    count: 1,
                });
            }
            match catch_unwind(AssertUnwindSafe(|| raster_image_to_png(bytes))) {
                Ok(r) => r,
                Err(_) => Err(Error::Panicked),
            }
        }
        NativeKind::Tiff => {
            match catch_unwind(AssertUnwindSafe(|| g4::raster_tiff_page(bytes, page_index))) {
                Ok(r) => r,
                Err(_) => Err(Error::Panicked),
            }
        }
        NativeKind::Pdf => {
            let dpi = if dpi == 0 { DPI_REVIEW } else { dpi };
            if let Some(sha) = native_sha256.map(str::trim).filter(|s| !s.is_empty()) {
                let key = cache_key(sha, page_index, dpi);
                if let Some(Some(hit)) = with_cache(|c| c.get(&key)) {
                    return Ok(RasterPage {
                        png: hit.png,
                        page_index,
                        page_count: hit.page_count,
                        media_box: hit.media,
                        crop_box: hit.crop,
                        rotate: hit.rotate,
                        width: hit.width,
                        height: hit.height,
                        native_width: hit.width,
                        native_height: hit.height,
                        truncated: hit.truncated,
                    });
                }
            }
            let page = match catch_unwind(AssertUnwindSafe(|| {
                raster_pdf_page_inner(bytes, page_index, dpi)
            })) {
                Ok(r) => r?,
                Err(_) => return Err(Error::Panicked),
            };
            if let Some(sha) = native_sha256.map(str::trim).filter(|s| !s.is_empty()) {
                let key = cache_key(sha, page_index, dpi);
                let _ = with_cache(|c| {
                    c.put(
                        key,
                        CacheEntry {
                            png: page.png.clone(),
                            page_count: page.page_count,
                            media: page.media_box,
                            crop: page.crop_box,
                            rotate: page.rotate,
                            width: page.width,
                            height: page.height,
                            truncated: page.truncated,
                        },
                    );
                });
            }
            Ok(page)
        }
        NativeKind::Other => Err(Error::UnsupportedKind),
    }
}

fn collect_page_spans(doc: &PdfDocument, page_index: usize) -> Result<Vec<TextSpan>> {
    let page = doc.page(page_index).map_err(map_zpdf)?;
    let content = doc.page_content_bytes(&page).map_err(map_zpdf)?;
    let mut fonts = doc.load_page_fonts(&page);
    let mut images = ImageCache::default();
    let mut spans = Vec::new();
    let interpreter = ContentInterpreter::new(page.effective_box())
        .with_document(doc.file(), &page.resources)
        .with_fonts(&mut fonts)
        .with_images(&mut images)
        .with_text_sink(&mut spans)
        .with_limits(&ParseLimits::default());
    let _list = interpreter.interpret(&content);
    Ok(spans)
}

/// Search PDF user-space hits; dilate each rect by 1 PDF point.
pub fn search_hit_rects(bytes: &[u8], query: &str) -> Result<Vec<BurnRect>> {
    reject_size(bytes.len())?;
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let doc = match catch_unwind(AssertUnwindSafe(|| open_pdf(bytes))) {
        Ok(r) => r?,
        Err(_) => return Err(Error::Panicked),
    };
    if doc.page_count() > MAX_PAGES {
        return Err(Error::TooManyPages {
            count: doc.page_count(),
        });
    }
    let mut out = Vec::new();
    for i in 0..doc.page_count() {
        let spans = collect_page_spans(&doc, i)?;
        for hit in search_spans(&spans, query, true) {
            for r in hit.rects {
                let x0 = r.x0.min(r.x1) - 1.0;
                let y0 = r.y0.min(r.y1) - 1.0;
                let x1 = r.x0.max(r.x1) + 1.0;
                let y1 = r.y0.max(r.y1) + 1.0;
                out.push(BurnRect {
                    page_index: i as u32,
                    x: x0,
                    y: y0,
                    w: (x1 - x0).max(0.0),
                    h: (y1 - y0).max(0.0),
                });
            }
        }
    }
    Ok(out)
}

fn rects_intersect(a: &BurnRect, b: &BurnRect) -> bool {
    if a.page_index != b.page_index {
        return false;
    }
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/// True when `quote` has no search hits, or any hit does not intersect `geoms`.
pub fn quote_unmapped(bytes: &[u8], quote: &str, geoms: &[BurnRect]) -> Result<bool> {
    let q = quote.trim();
    if q.is_empty() {
        return Ok(true);
    }
    let hits = search_hit_rects(bytes, q)?;
    if hits.is_empty() {
        return Ok(true);
    }
    Ok(hits
        .iter()
        .any(|hit| !geoms.iter().any(|g| rects_intersect(g, hit))))
}

fn burn_pdf_inner(original: &[u8], rects: &[BurnRect]) -> Result<Vec<u8>> {
    let mut by_page: HashMap<usize, Vec<Rect>> = HashMap::new();
    for r in rects {
        if r.w <= 0.0 || r.h <= 0.0 {
            continue;
        }
        by_page
            .entry(r.page_index as usize)
            .or_default()
            .push(Rect {
                x0: r.x,
                y0: r.y,
                x1: r.x + r.w,
                y1: r.y + r.h,
            });
    }
    let mut iw = IncrementalWriter::new(original.to_vec()).map_err(map_zpdf)?;
    let opts = RedactOptions::default();
    let mut pages: Vec<usize> = by_page.keys().copied().collect();
    pages.sort_unstable();
    for p in pages {
        if let Some(list) = by_page.get(&p) {
            iw.redact_page(p, list, &opts)
                .map_err(|e| Error::Burn(e.to_string()))?;
        }
    }
    let mut cursor = Cursor::new(Vec::new());
    iw.write(&mut cursor)
        .map_err(|e| Error::Burn(e.to_string()))?;
    let written = cursor.into_inner();
    let parsed = PdfFile::parse(written).map_err(map_zpdf)?;
    rewrite_pdf(&parsed, &RewriteOptions::default()).map_err(|e| Error::Burn(e.to_string()))
}

fn paint_black_rect(img: &mut image::RgbaImage, rect: BurnRect) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let x0 = rect.x.floor() as i64;
    let y0 = rect.y.floor() as i64;
    let x1 = (rect.x + rect.w).ceil() as i64;
    let y1 = (rect.y + rect.h).ceil() as i64;
    let x0 = x0.clamp(0, w);
    let y0 = y0.clamp(0, h);
    let x1 = x1.clamp(0, w);
    let y1 = y1.clamp(0, h);
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x as u32, y as u32, image::Rgba([0, 0, 0, 255]));
        }
    }
}

fn burn_raster_image(bytes: &[u8], rects: &[BurnRect], jpeg: bool) -> Result<Vec<u8>> {
    let dyn_img = image::load_from_memory(bytes).map_err(|e| Error::ImageDecode(e.to_string()))?;
    let mut rgba = dyn_img.to_rgba8();
    for r in rects {
        paint_black_rect(&mut rgba, *r);
    }
    let mut buf = Cursor::new(Vec::new());
    if jpeg {
        DynamicImage::ImageRgba8(rgba)
            .to_rgb8()
            .write_to(&mut buf, ImageFormat::Jpeg)
            .map_err(|_| Error::JpegEncodeFailed)?;
        let out = buf.into_inner();
        if !looks_like_jpeg(&out) {
            return Err(Error::JpegEncodeFailed);
        }
        Ok(out)
    } else {
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut buf, ImageFormat::Png)
            .map_err(|_| Error::PngEncodeFailed)?;
        Ok(buf.into_inner())
    }
}

/// Burn PDF (content-stream + rewrite) or JPEG/PNG (paint + same codec).
pub fn burn_native(
    original: &[u8],
    rects: &[BurnRect],
    path: Option<&str>,
    mime: Option<&str>,
) -> Result<Vec<u8>> {
    reject_size(original.len())?;
    match sniff_kind(path, mime, original) {
        NativeKind::Pdf => match catch_unwind(AssertUnwindSafe(|| burn_pdf_inner(original, rects)))
        {
            Ok(r) => r,
            Err(_) => Err(Error::Panicked),
        },
        NativeKind::Jpeg => match catch_unwind(AssertUnwindSafe(|| {
            burn_raster_image(original, rects, true)
        })) {
            Ok(r) => r,
            Err(_) => Err(Error::Panicked),
        },
        NativeKind::Png => match catch_unwind(AssertUnwindSafe(|| {
            burn_raster_image(original, rects, false)
        })) {
            Ok(r) => r,
            Err(_) => Err(Error::Panicked),
        },
        NativeKind::Tiff => {
            let n = g4::tiff_ifd_count(original)?;
            if n > 1 {
                return Err(Error::Burn(
                    "multi-IFD TIFF burn is unsupported; refuse to collapse pages".into(),
                ));
            }
            match catch_unwind(AssertUnwindSafe(|| {
                burn_raster_image(original, rects, false)
            })) {
                Ok(r) => r,
                Err(_) => Err(Error::Panicked),
            }
        }
        NativeKind::Other => Err(Error::UnsupportedKind),
    }
}

/// Probe encryption without hanging: IncrementalWriter::new errors on encrypted.
pub fn pdf_is_encrypted(bytes: &[u8]) -> Result<bool> {
    reject_size(bytes.len())?;
    match IncrementalWriter::new(bytes.to_vec()) {
        Ok(_) => Ok(false),
        Err(e) => match map_zpdf(e) {
            Error::Encrypted => Ok(true),
            other => Err(other),
        },
    }
}

/// Build a tiny uncompressed Helvetica PDF (test/chrome fixtures).
pub fn synthetic_text_pdf(pages: &[(&str, i32)]) -> Vec<u8> {
    if pages.is_empty() {
        return b"%PDF-1.4\n%%EOF\n".to_vec();
    }
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }
    let mut objs: Vec<Vec<u8>> = Vec::new();
    objs.push(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec());
    let page_count = pages.len();
    let first_page_id = 3u32;
    let font_id = first_page_id + (page_count as u32) * 2;
    let mut kids = String::new();
    for i in 0..page_count {
        kids.push_str(&format!("{} 0 R ", first_page_id + (i as u32) * 2));
    }
    objs.push(
        format!("2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {page_count} >>\nendobj\n")
            .into_bytes(),
    );
    for (i, (text, rotate)) in pages.iter().enumerate() {
        let page_id = first_page_id + (i as u32) * 2;
        let contents_id = page_id + 1;
        let rot = if *rotate == 0 {
            String::new()
        } else {
            format!(" /Rotate {rotate}")
        };
        objs.push(
            format!(
                "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Contents {contents_id} 0 R /Resources << /Font << /F1 {font_id} 0 R >> >>{rot} >>\nendobj\n"
            )
            .into_bytes(),
        );
        let stream = format!("BT /F1 24 Tf 72 400 Td ({}) Tj ET\n", esc(text));
        objs.push(
            format!(
                "{contents_id} 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
                stream.len()
            )
            .into_bytes(),
        );
    }
    objs.push(
        format!(
            "{font_id} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n"
        )
        .into_bytes(),
    );
    let mut body = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0u32; objs.len() + 1];
    for (i, obj) in objs.iter().enumerate() {
        offsets[i + 1] = body.len() as u32;
        body.extend_from_slice(obj);
    }
    let xref_at = body.len();
    let n = objs.len() + 1;
    body.extend_from_slice(format!("xref\n0 {n}\n").as_bytes());
    body.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        body.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    body.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    body
}

/// Two spatially separated labels so a SECRET burn cannot take the neighbor.
pub fn synthetic_two_label_pdf(secret: &str, neighbor: &str, rotate: i32) -> Vec<u8> {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }
    let rot = if rotate == 0 {
        String::new()
    } else {
        format!(" /Rotate {rotate}")
    };
    let stream = format!(
        "BT /F1 24 Tf 72 500 Td ({}) Tj ET\nBT /F1 24 Tf 72 200 Td ({}) Tj ET\n",
        esc(secret),
        esc(neighbor)
    );
    let objs: Vec<Vec<u8>> = vec![
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec(),
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_vec(),
        format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >>{rot} >>\nendobj\n"
        )
        .into_bytes(),
        format!(
            "4 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
            stream.len()
        )
        .into_bytes(),
        b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".to_vec(),
    ];
    let mut body = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0u32; objs.len() + 1];
    for (i, obj) in objs.iter().enumerate() {
        offsets[i + 1] = body.len() as u32;
        body.extend_from_slice(obj);
    }
    let xref_at = body.len();
    let n = objs.len() + 1;
    body.extend_from_slice(format!("xref\n0 {n}\n").as_bytes());
    body.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        body.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    body.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    body
}

pub fn sniff_ext_mime(bytes: &[u8]) -> (&'static str, &'static str) {
    if looks_like_pdf(bytes) {
        ("pdf", "application/pdf")
    } else if looks_like_jpeg(bytes) {
        ("jpg", "image/jpeg")
    } else if looks_like_png(bytes) {
        ("png", "image/png")
    } else if g4::looks_like_tiff(bytes) {
        ("tif", "image/tiff")
    } else {
        ("bin", "application/octet-stream")
    }
}
