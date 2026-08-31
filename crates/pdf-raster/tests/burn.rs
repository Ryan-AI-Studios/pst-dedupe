//! Burn + raster oracles for track 0114.

use pdf_raster::{
    burn_native, looks_like_jpeg, looks_like_png, pdf_is_encrypted, pixel_to_user_space,
    quote_unmapped, raster_page, search_hit_rects, sniff_ext_mime, BoxF, BurnRect, Error,
    DPI_REVIEW,
};
use zpdf::{rewrite_pdf, PdfFile, RewriteOptions};
use zpdf_writer::encrypt::EncryptionConfig;

const SECRET: &str = "SECRET_TOKEN_0114";
const NEIGHBOR: &str = "NEIGHBOR_TOKEN_0114";

fn pdf_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Uncompressed PDF with Helvetica text on each page. `rotate` is `/Rotate`.
fn uncompressed_pdf(pages: &[(&str, i32)]) -> Vec<u8> {
    assert!(!pages.is_empty());
    let mut objs: Vec<Vec<u8>> = Vec::new();
    // obj 1: catalog
    let pages_id = 2u32;
    objs.push(
        format!("1 0 obj\n<< /Type /Catalog /Pages {pages_id} 0 R >>\nendobj\n").into_bytes(),
    );
    let page_count = pages.len();
    let first_page_id = 3u32;
    let font_id = first_page_id + (page_count as u32) * 2;
    let mut kids = String::new();
    for i in 0..page_count {
        let id = first_page_id + (i as u32) * 2;
        kids.push_str(&format!("{id} 0 R "));
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
        let stream = format!("BT /F1 24 Tf 72 400 Td ({}) Tj ET\n", pdf_escape(text));
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

/// Two spatially separated text operators so a SECRET burn cannot take the neighbor.
fn uncompressed_pdf_two_labels(secret: &str, neighbor: &str, rotate: i32) -> Vec<u8> {
    let rot = if rotate == 0 {
        String::new()
    } else {
        format!(" /Rotate {rotate}")
    };
    let stream = format!(
        "BT /F1 24 Tf 72 500 Td ({}) Tj ET\nBT /F1 24 Tf 72 200 Td ({}) Tj ET\n",
        pdf_escape(secret),
        pdf_escape(neighbor)
    );
    let page = format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >>{rot} >>\nendobj\n"
    );
    let contents = format!(
        "4 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
        stream.len()
    );
    let font = "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n";
    let objs: Vec<Vec<u8>> = vec![
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec(),
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_vec(),
        page.into_bytes(),
        contents.into_bytes(),
        font.as_bytes().to_vec(),
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

fn utf16le_contains(hay: &[u8], needle: &str) -> bool {
    let mut enc = Vec::with_capacity(needle.len() * 2);
    for u in needle.encode_utf16() {
        enc.extend_from_slice(&u.to_le_bytes());
    }
    hay.windows(enc.len()).any(|w| w == enc.as_slice())
}

fn token_in_bytes(hay: &[u8]) -> bool {
    hay.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()) || utf16le_contains(hay, SECRET)
}

#[test]
fn burn_removes_secret_rewrite_not_incremental() {
    let original = uncompressed_pdf(&[(SECRET, 0)]);
    assert!(token_in_bytes(&original), "fixture must contain token");

    let raster = raster_page(&original, 0, DPI_REVIEW, None, Some("a.pdf"), None).expect("raster");
    assert!(raster.png.starts_with(&[0x89, 0x50, 0x4E, 0x47]), "PNG");
    assert_eq!(raster.page_count, 1);

    let hits = search_hit_rects(&original, SECRET).expect("hits");
    assert!(!hits.is_empty(), "search must find the token");
    let burned = burn_native(&original, &hits, Some("a.pdf"), None).expect("burn");
    assert_ne!(
        pdf_raster::sha256_hex(&original),
        pdf_raster::sha256_hex(&burned)
    );
    assert!(
        !token_in_bytes(&burned),
        "burned bytes must not contain SECRET_TOKEN_0114 (utf8/utf16)"
    );
    assert!(token_in_bytes(&original), "original CAS still has token");

    let after_hits = search_hit_rects(&burned, SECRET).expect("search burned");
    assert!(
        after_hits.is_empty(),
        "extract/search of burned must miss token"
    );

    // Incremental-only (parse original graph) would keep the secret — this
    // oracle fails that shortcut because rewrite_pdf GCs the unredacted stream.
    let parsed = PdfFile::parse(original.clone()).expect("parse orig");
    let rewritten_orig = rewrite_pdf(&parsed, &RewriteOptions::default()).expect("rewrite orig");
    assert!(
        token_in_bytes(&rewritten_orig),
        "rewrite of unredacted base still has token"
    );
}

#[test]
fn rotate90_box_on_visible_token() {
    let original = uncompressed_pdf_two_labels(SECRET, NEIGHBOR, 90);
    let hits = search_hit_rects(&original, SECRET).expect("hits");
    assert!(!hits.is_empty());
    let raster = raster_page(&original, 0, DPI_REVIEW, None, Some("r.pdf"), None).expect("raster");
    // Convert first hit (user space) to pixels then back — the burn uses user space.
    let user = BoxF::from_xywh(hits[0].x, hits[0].y, hits[0].w, hits[0].h);
    let px = pdf_raster::user_space_to_pixel(
        user,
        raster.width as f64,
        raster.height as f64,
        raster.crop_box,
        raster.rotate,
    );
    let mapped = pixel_to_user_space(
        px,
        raster.width as f64,
        raster.height as f64,
        raster.crop_box,
        raster.rotate,
    );
    let burned = burn_native(
        &original,
        &[BurnRect {
            page_index: 0,
            x: mapped.x,
            y: mapped.y,
            w: mapped.w.max(1.0),
            h: mapped.h.max(1.0),
        }],
        Some("r.pdf"),
        None,
    )
    .expect("burn");
    assert!(!token_in_bytes(&burned), "visible token must burn");
    assert!(
        burned
            .windows(NEIGHBOR.len())
            .any(|w| w == NEIGHBOR.as_bytes())
            || !search_hit_rects(&burned, NEIGHBOR).expect("n").is_empty(),
        "neighbor token must survive a box on the visible secret"
    );
}

#[test]
fn jpeg_burn_keeps_jpeg_magic() {
    let mut img = image::RgbImage::new(32, 32);
    for p in img.pixels_mut() {
        *p = image::Rgb([200, 40, 40]);
    }
    let mut jpeg = Vec::new();
    {
        let mut c = std::io::Cursor::new(&mut jpeg);
        img.write_to(&mut c, image::ImageFormat::Jpeg)
            .expect("encode jpeg");
    }
    assert!(looks_like_jpeg(&jpeg));
    let burned = burn_native(
        &jpeg,
        &[BurnRect {
            page_index: 0,
            x: 4.0,
            y: 4.0,
            w: 8.0,
            h: 8.0,
        }],
        Some("x.jpg"),
        Some("image/jpeg"),
    )
    .expect("jpeg burn");
    assert!(looks_like_jpeg(&burned), "JPEG magic FF D8");
    assert!(!looks_like_png(&burned));
    let (ext, mime) = sniff_ext_mime(&burned);
    assert_eq!(ext, "jpg");
    assert_eq!(mime, "image/jpeg");
}

#[test]
fn multi_ifd_tiff_burn_fails_closed() {
    let page = vec![128u8; 4];
    let tiff = pdf_raster::synthetic_gray8_tiff(&[page.clone(), page], 2, 2).expect("tiff");
    let err = burn_native(
        &tiff,
        &[BurnRect {
            page_index: 1,
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }],
        Some("scan.tif"),
        Some("image/tiff"),
    )
    .expect_err("must not collapse IFDs");
    assert!(
        err.to_string().to_ascii_lowercase().contains("multi-ifd")
            || err.to_string().to_ascii_lowercase().contains("collapse"),
        "unexpected: {err}"
    );
}

#[test]
fn encrypted_pdf_is_honest() {
    let original = uncompressed_pdf(&[(SECRET, 0)]);
    let parsed = PdfFile::parse(original.clone()).expect("parse");
    let enc = rewrite_pdf(
        &parsed,
        &RewriteOptions {
            encrypt: Some(EncryptionConfig::aes256("user-secret", "owner-secret")),
            ..RewriteOptions::default()
        },
    )
    .expect("encrypt");
    let is_enc = pdf_is_encrypted(&enc).expect("probe");
    assert!(is_enc);
    let err = raster_page(&enc, 0, DPI_REVIEW, None, Some("e.pdf"), None).expect_err("enc raster");
    assert_eq!(err.kind(), "pdf_encrypted");
    match err {
        Error::Encrypted => {}
        other => panic!("expected Encrypted, got {other}"),
    }
}

#[test]
fn pdf_long_side_cap_sets_truncated() {
    let original = uncompressed_pdf(&[(SECRET, 0)]);
    let page = raster_page(&original, 0, 600, None, Some("cap.pdf"), None).expect("raster");
    assert!(
        page.truncated,
        "150DPI*page can stay under cap; 600 DPI must cap"
    );
    assert!(page.width.max(page.height) <= pdf_raster::LONG_SIDE_CAP);
}

#[test]
fn pdf_long_side_cap_survives_lru_hit() {
    let original = uncompressed_pdf(&[(SECRET, 0)]);
    let key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let first = raster_page(&original, 0, 600, Some(key), Some("cap.pdf"), None).expect("cold");
    let second = raster_page(&original, 0, 600, Some(key), Some("cap.pdf"), None).expect("warm");
    assert!(first.truncated);
    assert!(
        second.truncated,
        "LRU hit must keep truncated=true for a capped PDF page"
    );
    assert_eq!(first.width, second.width);
    assert_eq!(first.height, second.height);
}

#[test]
fn quote_unmapped_when_neighbor_not_covered() {
    let original = uncompressed_pdf(&[("SECRET_TOKEN_0114 NEIGHBOR_TOKEN_0114", 0)]);
    let secret_hits = search_hit_rects(&original, SECRET).expect("secret hits");
    assert!(!secret_hits.is_empty());
    assert!(
        !quote_unmapped(&original, SECRET, &secret_hits).expect("covered"),
        "secret hits must cover SECRET"
    );
    assert!(
        quote_unmapped(&original, NEIGHBOR, &secret_hits).expect("neighbor"),
        "SECRET boxes must not cover NEIGHBOR"
    );
}

#[test]
fn two_page_page_one_reachable() {
    let original = uncompressed_pdf(&[(SECRET, 0), ("PAGE_TWO_0114", 0)]);
    let p0 = raster_page(&original, 0, DPI_REVIEW, None, Some("t.pdf"), None).expect("p0");
    let p1 = raster_page(&original, 1, DPI_REVIEW, None, Some("t.pdf"), None).expect("p1");
    assert_eq!(p0.page_count, 2);
    assert_eq!(p1.page_count, 2);
    assert_eq!(p1.page_index, 1);
    assert!(p0.png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    assert!(p1.png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
}
