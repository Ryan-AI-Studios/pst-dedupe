//! CCITT Group 4 encode + explicit little-endian TIFF IFD (track **0115**).
//!
//! Produce artifacts are written by [`wrap_g4_le_ifd`]. `fax::tiff::wrap` is
//! forbidden on this path (XResolution=200 and no BitsPerSample tag).

use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

use fax::encoder::Encoder;
use fax::{Color, VecWriter};
use image::RgbaImage;

use crate::coords::visual_size;
use crate::error::{Error, Result};
use crate::{
    raster_page, sniff_kind, NativeKind, RasterPage, DPI_PRODUCE, LONG_SIDE_CAP, MAX_PAGES,
};

/// ITU-R BT.601 luma weights.
pub const BT601_R: f32 = 0.299;
pub const BT601_G: f32 = 0.587;
pub const BT601_B: f32 = 0.114;
/// Luma below this value is Black (`fax::Color::Black`).
pub const BT601_BLACK_THRESHOLD: f32 = 160.0;
/// Bates stamp margin from the page edge, in inches.
pub const BATES_MARGIN_IN: f32 = 0.25;

/// One produced single-page G4 TIFF.
#[derive(Debug, Clone)]
pub struct G4Page {
    pub tiff: Vec<u8>,
    pub page_index: u32,
    pub page_count: u32,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    pub truncated: bool,
    pub sha256: String,
}

fn infallible<T>(r: std::result::Result<T, std::convert::Infallible>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => match e {},
    }
}

/// Little-endian TIFF magic `II*\0`.
pub fn looks_like_tiff_le(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'I' && bytes[1] == b'I' && bytes[2] == 0x2A && bytes[3] == 0
}

/// Big-endian TIFF magic `MM\0*`.
pub fn looks_like_tiff_be(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'M' && bytes[1] == b'M' && bytes[2] == 0 && bytes[3] == 0x2A
}

pub fn looks_like_tiff(bytes: &[u8]) -> bool {
    looks_like_tiff_le(bytes) || looks_like_tiff_be(bytes)
}

/// Count IFDs by walking next-IFD offsets (no decode).
pub fn tiff_ifd_count(bytes: &[u8]) -> Result<u32> {
    if bytes.len() < 8 {
        return Err(Error::ImageDecode("tiff too short".into()));
    }
    let le = looks_like_tiff_le(bytes);
    if !le && !looks_like_tiff_be(bytes) {
        return Err(Error::ImageDecode("not a tiff".into()));
    }
    let u16_at = |off: usize| -> Result<u16> {
        let b = bytes
            .get(off..off + 2)
            .ok_or_else(|| Error::ImageDecode("tiff truncated at u16".into()))?;
        Ok(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |off: usize| -> Result<u32> {
        let b = bytes
            .get(off..off + 4)
            .ok_or_else(|| Error::ImageDecode("tiff truncated at u32".into()))?;
        Ok(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    let mut off = u32_at(4)? as usize;
    let mut n = 0u32;
    let mut guard = 0u32;
    while off != 0 {
        if n as usize >= MAX_PAGES {
            return Err(Error::TooManyPages {
                count: MAX_PAGES + 1,
            });
        }
        let count = u16_at(off)? as usize;
        let next_at = off
            .saturating_add(2)
            .saturating_add(count.saturating_mul(12));
        let next = u32_at(next_at)?;
        n += 1;
        off = next as usize;
        guard += 1;
        if guard > MAX_PAGES as u32 + 2 {
            return Err(Error::ImageDecode("tiff IFD walk exceeded cap".into()));
        }
    }
    Ok(n)
}

fn bt601_luma(r: u8, g: u8, b: u8) -> f32 {
    BT601_R * f32::from(r) + BT601_G * f32::from(g) + BT601_B * f32::from(b)
}

fn is_black(r: u8, g: u8, b: u8) -> bool {
    bt601_luma(r, g, b) < BT601_BLACK_THRESHOLD
}

/// 5×7 monospace glyphs for Bates endorsement (ASCII alphanumerics + `-_`).
fn glyph_5x7(ch: char) -> [u8; 7] {
    // Each row is a 5-bit mask in the low bits (bit 4 = leftmost).
    match ch.to_ascii_uppercase() {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        _ => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    }
}

/// Paint a solid-box Bates stamp in the lower-right **before** bilevel threshold.
pub fn stamp_bates_lower_right(img: &mut RgbaImage, bates: &str, dpi: u32) {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 || bates.is_empty() {
        return;
    }
    let dpi = dpi.max(1);
    let margin = ((dpi as f32) * BATES_MARGIN_IN).round().max(1.0) as u32;
    let scale: u32 = 4;
    let gap: u32 = 1;
    let pad: u32 = 4;
    let chars: Vec<char> = bates.chars().collect();
    let glyph_w = 5 * scale;
    let glyph_h = 7 * scale;
    let text_w = chars.len() as u32 * glyph_w + chars.len().saturating_sub(1) as u32 * gap;
    let box_w = text_w.saturating_add(pad.saturating_mul(2)).max(1);
    let box_h = glyph_h.saturating_add(pad.saturating_mul(2)).max(1);
    let margin = margin.min(w.saturating_sub(1)).min(h.saturating_sub(1));
    let box_x = w.saturating_sub(margin).saturating_sub(box_w);
    let box_y = h.saturating_sub(margin).saturating_sub(box_h);
    let box_x2 = (box_x + box_w).min(w);
    let box_y2 = (box_y + box_h).min(h);
    for y in box_y..box_y2 {
        for x in box_x..box_x2 {
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    let mut cx = box_x + pad;
    let cy = box_y + pad;
    for ch in chars {
        let rows = glyph_5x7(ch);
        for (ry, mask) in rows.iter().enumerate() {
            for rx in 0..5u32 {
                if (mask >> (4 - rx)) & 1 == 1 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = cx + rx * scale + dx;
                            let py = cy + ry as u32 * scale + dy;
                            if px < w && py < h {
                                img.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
                            }
                        }
                    }
                }
            }
        }
        cx = cx.saturating_add(glyph_w + gap);
    }
}

fn encode_g4_bitstream(img: &RgbaImage) -> Result<Vec<u8>> {
    let width = img.width();
    let height = img.height();
    if width == 0 || height == 0 {
        return Err(Error::G4EncodeFailed);
    }
    let writer = VecWriter::new();
    let mut encoder = Encoder::new(writer);
    for y in 0..height {
        let pels = (0..width).map(|x| {
            let p = img.get_pixel(x, y).0;
            if is_black(p[0], p[1], p[2]) {
                Color::Black
            } else {
                Color::White
            }
        });
        infallible(encoder.encode_line(pels, width));
    }
    let writer = infallible(encoder.finish());
    Ok(writer.finish())
}

/// Wrap G4 bytes in an explicit little-endian TIFF IFD (one IFD, Compression=4).
///
/// Does **not** call `fax::tiff::wrap`.
pub fn wrap_g4_le_ifd(g4: &[u8], width: u32, height: u32, dpi: u32) -> Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(Error::TiffIfd("zero dimension".into()));
    }
    if g4.len() > u32::MAX as usize {
        return Err(Error::TiffIfd("strip too large".into()));
    }
    let dpi = dpi.max(1);
    let n_tags: u16 = 13;
    let ifd_at: u32 = 8;
    let ifd_bytes: u32 = 2 + 12 * u32::from(n_tags) + 4;
    let rat_at: u32 = ifd_at + ifd_bytes;
    let strip_at: u32 = rat_at + 16;
    let mut out = Vec::with_capacity(strip_at as usize + g4.len());
    out.extend_from_slice(&[b'I', b'I', 0x2A, 0x00]);
    out.extend_from_slice(&ifd_at.to_le_bytes());
    out.extend_from_slice(&n_tags.to_le_bytes());

    let short = |v: u16| u32::from(v);
    let tags: [(u16, u16, u32, u32); 13] = [
        (256, 4, 1, width),           // ImageWidth LONG
        (257, 4, 1, height),          // ImageLength LONG
        (258, 3, 1, short(1)),        // BitsPerSample SHORT = 1
        (259, 3, 1, short(4)),        // Compression SHORT = 4 (G4)
        (262, 3, 1, short(0)),        // Photometric SHORT = 0 WhiteIsZero
        (266, 3, 1, short(1)),        // FillOrder SHORT = 1
        (273, 4, 1, strip_at),        // StripOffsets LONG
        (277, 3, 1, short(1)),        // SamplesPerPixel SHORT = 1
        (278, 4, 1, height),          // RowsPerStrip LONG
        (279, 4, 1, g4.len() as u32), // StripByteCounts LONG
        (282, 5, 1, rat_at),          // XResolution RATIONAL
        (283, 5, 1, rat_at + 8),      // YResolution RATIONAL
        (296, 3, 1, short(2)),        // ResolutionUnit SHORT = 2 (inch)
    ];
    for (tag, typ, count, val) in tags {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&typ.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&dpi.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&dpi.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    if out.len() != strip_at as usize {
        return Err(Error::TiffIfd(format!(
            "IFD size mismatch: {} vs {strip_at}",
            out.len()
        )));
    }
    out.extend_from_slice(g4);
    Ok(out)
}

fn png_to_rgba(png: &[u8]) -> Result<RgbaImage> {
    let dyn_img = image::load_from_memory(png).map_err(|e| Error::ImageDecode(e.to_string()))?;
    Ok(dyn_img.to_rgba8())
}

fn effective_dpi(image_dpi: u32, truncated: bool, native_long: u32) -> u32 {
    if truncated && native_long > LONG_SIDE_CAP && native_long > 0 {
        let v = (u64::from(image_dpi) * u64::from(LONG_SIDE_CAP)) / u64::from(native_long);
        v.max(1) as u32
    } else {
        image_dpi.max(1)
    }
}

fn effective_dpi_from_page(image_dpi: u32, kind: NativeKind, page: &RasterPage) -> u32 {
    match kind {
        NativeKind::Pdf => {
            if !page.truncated {
                return image_dpi.max(1);
            }
            let (vis_w, vis_h) = visual_size(page.crop_box, page.rotate);
            let intended_long = vis_w.max(vis_h) * f64::from(image_dpi) / 72.0;
            if intended_long > 0.0 {
                let v = (f64::from(image_dpi) * f64::from(LONG_SIDE_CAP) / intended_long).round();
                v.max(1.0) as u32
            } else {
                image_dpi.max(1)
            }
        }
        NativeKind::Jpeg | NativeKind::Png | NativeKind::Tiff => {
            let native_long = page.native_width.max(page.native_height);
            if page.truncated && native_long > page.width.max(page.height) && native_long > 0 {
                effective_dpi(image_dpi, true, native_long)
            } else {
                image_dpi.max(1)
            }
        }
        NativeKind::Other => image_dpi.max(1),
    }
}

/// Encode one RGBA (or PNG) page to a single-IFD G4 TIFF with Bates stamp.
pub fn encode_g4_tif(rgba_or_png: &[u8], bates: &str, image_dpi: u32) -> Result<Vec<u8>> {
    let mut rgba = if looks_like_png_bytes(rgba_or_png) {
        png_to_rgba(rgba_or_png)?
    } else {
        let w_h = infer_raw_rgba_square(rgba_or_png)?;
        RgbaImage::from_raw(w_h, w_h, rgba_or_png.to_vec()).ok_or(Error::G4EncodeFailed)?
    };
    stamp_bates_lower_right(&mut rgba, bates, image_dpi.max(1));
    let g4 = encode_g4_bitstream(&rgba)?;
    wrap_g4_le_ifd(&g4, rgba.width(), rgba.height(), image_dpi.max(1))
}

fn looks_like_png_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47
}

fn infer_raw_rgba_square(bytes: &[u8]) -> Result<u32> {
    if !bytes.len().is_multiple_of(4) {
        return Err(Error::G4EncodeFailed);
    }
    let px = bytes.len() / 4;
    let side = (px as f64).sqrt() as u32;
    if side > 0 && side * side == px as u32 {
        Ok(side)
    } else {
        Err(Error::G4EncodeFailed)
    }
}

/// Raster one native page and encode a G4 TIFF (Bates stamped, explicit IFD).
pub fn raster_and_encode_page(
    bytes: &[u8],
    page_index: u32,
    bates: &str,
    image_dpi: u32,
    native_sha256: Option<&str>,
    path: Option<&str>,
    mime: Option<&str>,
) -> Result<G4Page> {
    let dpi = if image_dpi == 0 {
        DPI_PRODUCE
    } else {
        image_dpi
    };
    let kind = sniff_kind(path, mime, bytes);
    let raster_dpi = match kind {
        NativeKind::Pdf => dpi,
        _ => 0,
    };
    let page = raster_page(bytes, page_index, raster_dpi, native_sha256, path, mime)?;
    let mut rgba = png_to_rgba(&page.png)?;
    stamp_bates_lower_right(&mut rgba, bates, dpi);
    let g4 = encode_g4_bitstream(&rgba)?;
    let out_dpi = effective_dpi_from_page(dpi, kind, &page);
    let tiff = wrap_g4_le_ifd(&g4, rgba.width(), rgba.height(), out_dpi)?;
    let sha256 = crate::sha256_hex(&tiff);
    Ok(G4Page {
        tiff,
        page_index: page.page_index,
        page_count: page.page_count,
        width: rgba.width(),
        height: rgba.height(),
        dpi: out_dpi,
        truncated: page.truncated,
        sha256,
    })
}

/// Predicted image page count: PDF pages / TIFF IFDs / JPEG PNG = 1 / other = 0.
pub fn native_image_page_count(
    bytes: &[u8],
    path: Option<&str>,
    mime: Option<&str>,
) -> Result<u32> {
    match sniff_kind(path, mime, bytes) {
        NativeKind::Pdf => {
            let n = crate::pdf_page_count(bytes)?;
            if n == 0 {
                Err(Error::Corrupt("pdf has zero pages".into()))
            } else {
                Ok(n)
            }
        }
        NativeKind::Tiff => {
            let n = tiff_ifd_count(bytes)?;
            if n == 0 {
                Err(Error::ImageDecode("tiff has zero IFDs".into()))
            } else {
                Ok(n)
            }
        }
        NativeKind::Jpeg | NativeKind::Png => Ok(1),
        NativeKind::Other => Ok(0),
    }
}

/// Encode every image page of a native. `catch_unwind` at the document boundary.
pub fn raster_and_encode_document(
    bytes: &[u8],
    bates_for_page: &dyn Fn(u32) -> String,
    image_dpi: u32,
    native_sha256: Option<&str>,
    path: Option<&str>,
    mime: Option<&str>,
) -> Result<Vec<G4Page>> {
    match catch_unwind(AssertUnwindSafe(|| {
        raster_and_encode_document_inner(
            bytes,
            bates_for_page,
            image_dpi,
            native_sha256,
            path,
            mime,
        )
    })) {
        Ok(r) => r,
        Err(_) => Err(Error::Panicked),
    }
}

fn raster_and_encode_document_inner(
    bytes: &[u8],
    bates_for_page: &dyn Fn(u32) -> String,
    image_dpi: u32,
    native_sha256: Option<&str>,
    path: Option<&str>,
    mime: Option<&str>,
) -> Result<Vec<G4Page>> {
    let count = native_image_page_count(bytes, path, mime)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if count as usize > MAX_PAGES {
        return Err(Error::TooManyPages {
            count: count as usize,
        });
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let bates = bates_for_page(i);
        out.push(raster_and_encode_page(
            bytes,
            i,
            &bates,
            image_dpi,
            native_sha256,
            path,
            mime,
        )?);
    }
    Ok(out)
}

fn decoding_to_rgba(
    color: tiff::ColorType,
    img: tiff::decoder::DecodingResult,
    w: u32,
    h: u32,
) -> Result<RgbaImage> {
    match (color, img) {
        (tiff::ColorType::Gray(8), tiff::decoder::DecodingResult::U8(buf)) => {
            if buf.len() != (w * h) as usize {
                return Err(Error::ImageDecode("gray8 size mismatch".into()));
            }
            let mut rgba = RgbaImage::new(w, h);
            for (i, v) in buf.into_iter().enumerate() {
                let x = (i as u32) % w;
                let y = (i as u32) / w;
                rgba.put_pixel(x, y, image::Rgba([v, v, v, 255]));
            }
            Ok(rgba)
        }
        (tiff::ColorType::Gray(1), tiff::decoder::DecodingResult::U8(buf)) => {
            let mut rgba = RgbaImage::new(w, h);
            let row_bytes = (w as usize).div_ceil(8);
            if buf.len() >= row_bytes * h as usize {
                for y in 0..h {
                    for x in 0..w {
                        let byte = buf[y as usize * row_bytes + (x as usize) / 8];
                        let bit = 7 - (x as usize % 8);
                        let on = (byte >> bit) & 1 == 1;
                        // Decoder already normalizes WhiteIsZero → BlackIsZero
                        // (0 = black, 1 = white).
                        let v = if on { 255 } else { 0 };
                        rgba.put_pixel(x, y, image::Rgba([v, v, v, 255]));
                    }
                }
            } else if buf.len() == (w * h) as usize {
                for (i, v) in buf.into_iter().enumerate() {
                    let x = (i as u32) % w;
                    let y = (i as u32) / w;
                    let g = if v == 0 { 0 } else { 255 };
                    rgba.put_pixel(x, y, image::Rgba([g, g, g, 255]));
                }
            } else {
                return Err(Error::ImageDecode("gray1 size mismatch".into()));
            }
            Ok(rgba)
        }
        (tiff::ColorType::RGB(8), tiff::decoder::DecodingResult::U8(buf)) => {
            if buf.len() != (w * h * 3) as usize {
                return Err(Error::ImageDecode("rgb8 size mismatch".into()));
            }
            let mut rgba = RgbaImage::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 3) as usize;
                    rgba.put_pixel(x, y, image::Rgba([buf[i], buf[i + 1], buf[i + 2], 255]));
                }
            }
            Ok(rgba)
        }
        (tiff::ColorType::Gray(16), tiff::decoder::DecodingResult::U16(buf)) => {
            if buf.len() != (w * h) as usize {
                return Err(Error::ImageDecode("gray16 size mismatch".into()));
            }
            let mut rgba = RgbaImage::new(w, h);
            for (i, v) in buf.into_iter().enumerate() {
                let x = (i as u32) % w;
                let y = (i as u32) / w;
                let g = (v >> 8) as u8;
                rgba.put_pixel(x, y, image::Rgba([g, g, g, 255]));
            }
            Ok(rgba)
        }
        (tiff::ColorType::RGB(16), tiff::decoder::DecodingResult::U16(buf)) => {
            if buf.len() != (w * h * 3) as usize {
                return Err(Error::ImageDecode("rgb16 size mismatch".into()));
            }
            let mut rgba = RgbaImage::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 3) as usize;
                    rgba.put_pixel(
                        x,
                        y,
                        image::Rgba([
                            (buf[i] >> 8) as u8,
                            (buf[i + 1] >> 8) as u8,
                            (buf[i + 2] >> 8) as u8,
                            255,
                        ]),
                    );
                }
            }
            Ok(rgba)
        }
        (tiff::ColorType::RGBA(8), tiff::decoder::DecodingResult::U8(buf)) => {
            RgbaImage::from_raw(w, h, buf).ok_or(Error::ImageDecode("rgba8 size mismatch".into()))
        }
        _ => {
            // Fall back to the `image` crate (first IFD only) when types are exotic.
            Err(Error::ImageDecode(format!(
                "unsupported tiff color type {color:?}"
            )))
        }
    }
}

/// Decode one TIFF IFD to RGBA at native pixel size (no dpi scale).
pub fn decode_tiff_ifd(bytes: &[u8], page_index: u32) -> Result<(RgbaImage, u32)> {
    let cursor = Cursor::new(bytes);
    let mut decoder =
        tiff::decoder::Decoder::new(cursor).map_err(|e| Error::ImageDecode(e.to_string()))?;
    let mut idx = 0u32;
    loop {
        if idx == page_index {
            let (w, h) = decoder
                .dimensions()
                .map_err(|e| Error::ImageDecode(e.to_string()))?;
            let color = decoder
                .colortype()
                .map_err(|e| Error::ImageDecode(e.to_string()))?;
            let img = decoder
                .read_image()
                .map_err(|e| Error::ImageDecode(e.to_string()))?;
            let rgba = match decoding_to_rgba(color, img, w, h) {
                Ok(r) => r,
                Err(_) => {
                    // First-IFD fallback via `image` (handles more photometric variants).
                    if page_index == 0 {
                        let dyn_img = image::load_from_memory(bytes)
                            .map_err(|e| Error::ImageDecode(e.to_string()))?;
                        dyn_img.to_rgba8()
                    } else {
                        return Err(Error::ImageDecode(
                            "tiff IFD color type not supported".into(),
                        ));
                    }
                }
            };
            let count = tiff_ifd_count(bytes).unwrap_or(idx + 1);
            return Ok((rgba, count));
        }
        if !decoder.more_images() {
            let count = tiff_ifd_count(bytes).unwrap_or(idx + 1);
            return Err(Error::PageOutOfRange {
                index: page_index,
                count,
            });
        }
        decoder
            .next_image()
            .map_err(|e| Error::ImageDecode(e.to_string()))?;
        idx += 1;
    }
}

/// Raster a TIFF IFD at 1:1 then [`LONG_SIDE_CAP`] (JPEG/PNG path).
pub fn raster_tiff_page(bytes: &[u8], page_index: u32) -> Result<crate::RasterPage> {
    let (src, page_count) = decode_tiff_ifd(bytes, page_index)?;
    let native_width = src.width();
    let native_height = src.height();
    let (rgba, truncated) = crate::cap_long_side(src);
    let width = rgba.width();
    let height = rgba.height();
    let png = crate::encode_png_rgba(width, height, rgba.into_raw())?;
    Ok(crate::RasterPage {
        png,
        page_index,
        page_count,
        media_box: crate::coords::BoxF::from_xywh(0.0, 0.0, width as f64, height as f64),
        crop_box: crate::coords::BoxF::from_xywh(0.0, 0.0, width as f64, height as f64),
        rotate: 0,
        width,
        height,
        native_width,
        native_height,
        truncated,
    })
}

/// Parse little-endian IFD0 tags for tests / QC (do not trust `fax::tiff::wrap`).
pub fn parse_le_ifd0_tags(bytes: &[u8]) -> Result<Vec<(u16, u16, u32, u32)>> {
    if !looks_like_tiff_le(bytes) || bytes.len() < 8 {
        return Err(Error::ImageDecode("not little-endian tiff".into()));
    }
    let ifd = u32::from_le_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| Error::ImageDecode("tiff header".into()))?,
    ) as usize;
    let count = u16::from_le_bytes(
        bytes
            .get(ifd..ifd + 2)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::ImageDecode("missing IFD count".into()))?,
    ) as usize;
    let mut tags = Vec::with_capacity(count);
    for i in 0..count {
        let off = ifd + 2 + i * 12;
        let slice = bytes
            .get(off..off + 12)
            .ok_or_else(|| Error::ImageDecode("truncated IFD entry".into()))?;
        let tag = u16::from_le_bytes([slice[0], slice[1]]);
        let typ = u16::from_le_bytes([slice[2], slice[3]]);
        let n = u32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]);
        let val = u32::from_le_bytes([slice[8], slice[9], slice[10], slice[11]]);
        tags.push((tag, typ, n, val));
    }
    Ok(tags)
}

/// Read a RATIONAL tag (type 5) from little-endian TIFF bytes.
pub fn read_le_rational(bytes: &[u8], value_or_offset: u32) -> Result<(u32, u32)> {
    let off = value_or_offset as usize;
    let slice = bytes
        .get(off..off + 8)
        .ok_or_else(|| Error::ImageDecode("rational truncated".into()))?;
    let num = u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]);
    let den = u32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]);
    Ok((num, den))
}

/// Count IFDs by walking next-IFD; returns 0 when not TIFF.
pub fn count_ifds_or_zero(bytes: &[u8]) -> u32 {
    tiff_ifd_count(bytes).unwrap_or(0)
}

/// Uncompressed 8-bit Gray little-endian multi-IFD TIFF (inbound test fixture).
pub fn synthetic_gray8_tiff(pages: &[Vec<u8>], w: u32, h: u32) -> Result<Vec<u8>> {
    if pages.is_empty() || w == 0 || h == 0 {
        return Err(Error::ImageDecode("empty gray tiff".into()));
    }
    let strip_len = (w as usize).saturating_mul(h as usize);
    for p in pages {
        if p.len() != strip_len {
            return Err(Error::ImageDecode("gray page size mismatch".into()));
        }
    }
    let n_tags: u16 = 11;
    let ifd_bytes = 2 + 12 * usize::from(n_tags) + 4;
    let rat_bytes = 16;
    let header = 8usize;
    let mut ifd_offsets = Vec::new();
    let mut strip_offsets = Vec::new();
    let mut cursor = header;
    for page in pages {
        ifd_offsets.push(cursor);
        cursor += ifd_bytes + rat_bytes;
        strip_offsets.push(cursor);
        cursor += page.len();
    }
    let mut out = vec![0u8; cursor];
    out[0] = b'I';
    out[1] = b'I';
    out[2] = 0x2A;
    out[3] = 0;
    out[4..8].copy_from_slice(&(ifd_offsets[0] as u32).to_le_bytes());
    for (i, page) in pages.iter().enumerate() {
        let ifd_at = ifd_offsets[i];
        let strip_at = strip_offsets[i] as u32;
        let next = if i + 1 < pages.len() {
            ifd_offsets[i + 1] as u32
        } else {
            0
        };
        let rat_at = (ifd_at + ifd_bytes) as u32;
        let mut pos = ifd_at;
        out[pos..pos + 2].copy_from_slice(&n_tags.to_le_bytes());
        pos += 2;
        let short = |v: u16| u32::from(v);
        let tags: [(u16, u16, u32, u32); 11] = [
            (256, 4, 1, w),
            (257, 4, 1, h),
            (258, 3, 1, short(8)),
            (259, 3, 1, short(1)),
            (262, 3, 1, short(1)),
            (273, 4, 1, strip_at),
            (277, 3, 1, short(1)),
            (278, 4, 1, h),
            (279, 4, 1, page.len() as u32),
            (282, 5, 1, rat_at),
            (296, 3, 1, short(2)),
        ];
        for (tag, typ, count, val) in tags {
            out[pos..pos + 2].copy_from_slice(&tag.to_le_bytes());
            out[pos + 2..pos + 4].copy_from_slice(&typ.to_le_bytes());
            out[pos + 4..pos + 8].copy_from_slice(&count.to_le_bytes());
            out[pos + 8..pos + 12].copy_from_slice(&val.to_le_bytes());
            pos += 12;
        }
        out[pos..pos + 4].copy_from_slice(&next.to_le_bytes());
        out[rat_at as usize..rat_at as usize + 4].copy_from_slice(&72u32.to_le_bytes());
        out[rat_at as usize + 4..rat_at as usize + 8].copy_from_slice(&1u32.to_le_bytes());
        let s = strip_at as usize;
        out[s..s + page.len()].copy_from_slice(page);
    }
    Ok(out)
}
