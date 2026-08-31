//! G4 encode + explicit IFD oracles (track 0115).

use image::{ImageFormat, RgbaImage};
use pdf_raster::{
    decode_tiff_ifd, encode_g4_tif, looks_like_tiff, native_image_page_count, parse_le_ifd0_tags,
    raster_and_encode_document, raster_page, read_le_rational, synthetic_text_pdf, tiff_ifd_count,
    wrap_g4_le_ifd, NativeKind, BT601_BLACK_THRESHOLD, DPI_PRODUCE,
};
use std::io::Cursor;

fn tag_map(bytes: &[u8]) -> std::collections::HashMap<u16, (u16, u32, u32)> {
    parse_le_ifd0_tags(bytes)
        .expect("ifd")
        .into_iter()
        .map(|(tag, typ, n, val)| (tag, (typ, n, val)))
        .collect()
}

fn oracle_g4_ifd(bytes: &[u8], expect_dpi: u32) -> Result<(), String> {
    if !looks_like_tiff(bytes) || bytes.len() < 4 || bytes[0] != b'I' {
        return Err("not little-endian TIFF II*".into());
    }
    let tags = tag_map(bytes);
    let get = |t: u16| {
        tags.get(&t)
            .copied()
            .ok_or_else(|| format!("missing tag {t}"))
    };
    let (t258, _, v258) = get(258)?;
    if t258 != 3 || (v258 & 0xFFFF) != 1 {
        return Err(format!("BitsPerSample tag not 1: typ={t258} val={v258}"));
    }
    let (_, _, v259) = get(259)?;
    if (v259 & 0xFFFF) != 4 {
        return Err(format!("Compression != 4: {v259}"));
    }
    let (_, _, v262) = get(262)?;
    if (v262 & 0xFFFF) != 0 {
        return Err(format!("Photometric != 0: {v262}"));
    }
    let (_, _, v296) = get(296)?;
    if (v296 & 0xFFFF) != 2 {
        return Err(format!("ResolutionUnit != 2: {v296}"));
    }
    let (_, _, xoff) = get(282)?;
    let (_, _, yoff) = get(283)?;
    let (xn, xd) = read_le_rational(bytes, xoff).map_err(|e| e.to_string())?;
    let (yn, yd) = read_le_rational(bytes, yoff).map_err(|e| e.to_string())?;
    if xd == 0 || yd == 0 {
        return Err("zero rational denominator".into());
    }
    if xn / xd != expect_dpi || yn / yd != expect_dpi {
        return Err(format!("X/YRes {xn}/{xd} {yn}/{yd} != {expect_dpi}"));
    }
    let n = tiff_ifd_count(bytes).map_err(|e| e.to_string())?;
    if n != 1 {
        return Err(format!("expected one IFD, got {n}"));
    }
    Ok(())
}

fn solid_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for p in img.pixels_mut() {
        *p = image::Rgba([rgb[0], rgb[1], rgb[2], 255]);
    }
    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, ImageFormat::Png)
        .expect("png");
    buf.into_inner()
}

fn solid_jpeg(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for p in img.pixels_mut() {
        *p = image::Rgba([rgb[0], rgb[1], rgb[2], 255]);
    }
    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .to_rgb8()
        .write_to(&mut buf, ImageFormat::Jpeg)
        .expect("jpeg");
    buf.into_inner()
}

fn gray8_tiff_pages(pages: &[Vec<u8>], w: u32, h: u32) -> Vec<u8> {
    pdf_raster::synthetic_gray8_tiff(pages, w, h).expect("gray tiff")
}

#[test]
fn wrap_fails_g4_oracle_our_ifd_passes() {
    let png = solid_png(32, 32, [240, 240, 240]);
    let ours = encode_g4_tif(&png, "PROD000001", DPI_PRODUCE).expect("encode");
    oracle_g4_ifd(&ours, DPI_PRODUCE).expect("ours must pass oracle");

    let tags = tag_map(&ours);
    let w = tags.get(&256).expect("w").2;
    let h = tags.get(&257).expect("h").2;
    let strip_off = tags.get(&273).expect("off").2 as usize;
    let strip_len = tags.get(&279).expect("len").2 as usize;
    let g4 = ours[strip_off..strip_off + strip_len].to_vec();
    let wrapped = fax::tiff::wrap(&g4, w, h);
    let wrap_err = oracle_g4_ifd(&wrapped, DPI_PRODUCE);
    assert!(
        wrap_err.is_err(),
        "fax::tiff::wrap must fail the 300dpi+BitsPerSample oracle, got Ok"
    );
}

#[test]
fn two_page_pdf_two_single_ifd_g4() {
    let pdf = synthetic_text_pdf(&[("PAGE ONE", 0), ("PAGE TWO", 0)]);
    let pages = raster_and_encode_document(
        &pdf,
        &|i| format!("PROD{:06}", i + 1),
        DPI_PRODUCE,
        None,
        Some("doc.pdf"),
        Some("application/pdf"),
    )
    .expect("encode");
    assert_eq!(pages.len(), 2);
    for p in &pages {
        oracle_g4_ifd(&p.tiff, p.dpi).expect("oracle");
        assert_eq!(tiff_ifd_count(&p.tiff).expect("ifd"), 1);
    }
}

#[test]
fn inbound_two_ifd_tiff_emits_two_g4() {
    let p0 = vec![200u8; 8 * 8];
    let p1 = vec![30u8; 8 * 8];
    let tiff = gray8_tiff_pages(&[p0, p1], 8, 8);
    assert_eq!(tiff_ifd_count(&tiff).expect("count"), 2);
    let pages = raster_and_encode_document(
        &tiff,
        &|i| format!("TIF{:06}", i + 1),
        DPI_PRODUCE,
        None,
        Some("scan.tif"),
        Some("image/tiff"),
    )
    .expect("encode");
    assert_eq!(pages.len(), 2);
    for p in &pages {
        oracle_g4_ifd(&p.tiff, p.dpi).expect("oracle");
        assert_eq!(tiff_ifd_count(&p.tiff).expect("ifd"), 1);
    }
}

#[test]
fn jpeg_one_g4_page() {
    let jpeg = solid_jpeg(48, 32, [180, 180, 180]);
    let pages = raster_and_encode_document(
        &jpeg,
        &|_| "JPG000001".into(),
        DPI_PRODUCE,
        None,
        Some("pic.jpg"),
        Some("image/jpeg"),
    )
    .expect("encode");
    assert_eq!(pages.len(), 1);
    oracle_g4_ifd(&pages[0].tiff, pages[0].dpi).expect("oracle");
}

#[test]
fn stamp_region_not_uniformly_white() {
    let png = solid_png(400, 300, [255, 255, 255]);
    let tiff = encode_g4_tif(&png, "PROD000001", DPI_PRODUCE).expect("encode");
    let decoded = image::load_from_memory(&tiff).expect("decode g4");
    let gray = decoded.to_luma8();
    let mut black = 0u32;
    let mut white = 0u32;
    for p in gray.pixels() {
        if p.0[0] < 128 {
            black += 1;
        } else {
            white += 1;
        }
    }
    assert!(
        black > 20 && white > 20,
        "stamp should create contrast on a white page (black={black} white={white})"
    );
}

#[test]
fn wrap_g4_le_ifd_roundtrip_tags() {
    let g4 = vec![0u8; 16];
    let tiff = wrap_g4_le_ifd(&g4, 8, 8, 300).expect("ifd");
    oracle_g4_ifd(&tiff, 300).expect("oracle");
}

#[test]
fn sniff_tiff_kind_and_page_count() {
    let page = vec![128u8; 4 * 4];
    let tiff = gray8_tiff_pages(&[page.clone(), page], 4, 4);
    assert_eq!(
        pdf_raster::sniff_kind(Some("a.tif"), Some("image/tiff"), &tiff),
        NativeKind::Tiff
    );
    assert_eq!(
        native_image_page_count(&tiff, Some("a.tif"), None).expect("n"),
        2
    );
    let r = raster_page(&tiff, 1, 150, None, Some("a.tif"), None).expect("p1");
    assert_eq!(r.page_index, 1);
    assert_eq!(r.page_count, 2);
}

#[test]
fn one_bit_g4_roundtrip_keeps_black_and_white() {
    let mut img = RgbaImage::from_pixel(400, 400, image::Rgba([255, 255, 255, 255]));
    img.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .expect("png");
    let tiff = encode_g4_tif(&png, "PROD000001", 300).expect("g4");
    oracle_g4_ifd(&tiff, 300).expect("ifd");
    let page = raster_page(&tiff, 0, 300, None, Some("x.tif"), Some("image/tiff")).expect("decode");
    let decoded = image::load_from_memory(&page.png).expect("png").to_rgba8();
    let origin = decoded.get_pixel(0, 0);
    assert!(
        origin[0] < 40 && origin[1] < 40 && origin[2] < 40,
        "origin must stay black, got {origin:?}"
    );
    let field = decoded.get_pixel(20, 20);
    assert!(
        field[0] > 200 && field[1] > 200 && field[2] > 200,
        "page field must stay white, got {field:?}"
    );
}

#[test]
fn zero_ifd_tiff_is_not_a_zero_page_success() {
    let mut bytes = vec![b'I', b'I', 0x2A, 0, 0, 0, 0, 0];
    bytes.resize(8, 0);
    assert_eq!(tiff_ifd_count(&bytes).expect("count"), 0);
    let err = native_image_page_count(&bytes, Some("empty.tif"), Some("image/tiff"))
        .expect_err("zero IFDs must fail");
    assert!(
        err.to_string().to_ascii_lowercase().contains("zero"),
        "unexpected: {err}"
    );
}

#[test]
fn threshold_constant_is_160() {
    assert!((BT601_BLACK_THRESHOLD - 160.0).abs() < f32::EPSILON);
}

fn two_ifd_gray16() -> Vec<u8> {
    use tiff::encoder::{colortype, TiffEncoder};
    let mut cur = Cursor::new(Vec::new());
    {
        let mut enc = TiffEncoder::new(&mut cur).expect("enc");
        enc.write_image::<colortype::Gray16>(2, 2, &[0, 0, 0, 0])
            .expect("p0");
        enc.write_image::<colortype::Gray16>(2, 2, &[u16::MAX, u16::MAX, u16::MAX, u16::MAX])
            .expect("p1");
    }
    cur.into_inner()
}

#[test]
fn gray16_second_ifd_decodes() {
    let bytes = two_ifd_gray16();
    assert_eq!(tiff_ifd_count(&bytes).expect("count"), 2);
    decode_tiff_ifd(&bytes, 0).expect("ifd0");
    decode_tiff_ifd(&bytes, 1).expect("ifd1");
    let pages = raster_and_encode_document(&bytes, &|i| format!("P{i}"), 300, None, None, None)
        .expect("encode");
    assert_eq!(pages.len(), 2);
}
