//! Integration tests for ingest-purview (synthetic packages only).

use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use ingest_purview::{
    detect, ingest_path, ingest_path_on_job, resume_ingest, Error as IngestError, ExpandLimits,
    PackageKind,
};
use matter_core::{JobState, Matter};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
    (dir, path)
}

fn write_zip_file(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).expect("create zip");
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, opts).expect("start");
        zip.write_all(data).expect("write");
    }
    zip.finish().expect("finish");
}

fn write_zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buf);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, opts).expect("start");
        zip.write_all(data).expect("write");
    }
    zip.finish().expect("finish").into_inner()
}

/// Minimal ZIP with a raw non-UTF-8 entry name (CP437 café = caf\x82).
fn write_zip_with_raw_name(path: &std::path::Path, raw_name: &[u8], data: &[u8]) {
    // Local file header + data + central directory + EOCD (stored, no extra).
    let mut out = Vec::new();
    let name_len = raw_name.len() as u16;
    let data_len = data.len() as u32;
    let crc = crc32fast_poly(data);

    // Local file header
    out.extend_from_slice(&0x04034b50u32.to_le_bytes()); // sig
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags (no UTF-8 bit)
    out.extend_from_slice(&0u16.to_le_bytes()); // method stored
    out.extend_from_slice(&0u16.to_le_bytes()); // time
    out.extend_from_slice(&0u16.to_le_bytes()); // date
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra len
    out.extend_from_slice(raw_name);
    out.extend_from_slice(data);
    let local_len = out.len() as u32;

    // Central directory
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&0x02014b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); // version made by
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&0u16.to_le_bytes()); // method
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out.extend_from_slice(&0u16.to_le_bytes()); // disk start
    out.extend_from_slice(&0u16.to_le_bytes()); // int attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // ext attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // local header offset
    out.extend_from_slice(raw_name);
    let cd_size = (out.len() as u32) - cd_offset;

    // EOCD
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    let _ = local_len;

    fs::write(path, out).expect("write raw zip");
}

fn crc32fast_poly(data: &[u8]) -> u32 {
    // IEEE CRC-32
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn build_sample_package(pkg: &Utf8PathBuf) {
    fs::create_dir_all(pkg.as_std_path()).expect("pkg dir");
    // Dummy PST (magic only).
    fs::write(
        pkg.join("mail.pst").as_std_path(),
        b"!BDN_dummy_pst_fixture",
    )
    .expect("pst");
    // Nested zip: inner.zip with a text file.
    let inner = write_zip_bytes(&[("note.txt", b"hello inner")]);
    write_zip_file(
        pkg.join("files.zip").as_std_path(),
        &[
            ("readme.txt", b"top of files.zip"),
            ("inner.zip", &inner),
            ("custodian_a/msg.eml", b"From: a@b\r\n\r\nbody"),
        ],
    );
    fs::write(
        pkg.join("ExportSummary.csv").as_std_path(),
        b"Item,Count\nMessages,1\n",
    )
    .expect("csv");
}

#[test]
fn happy_path_purview_package() {
    let (_tmp, base) = utf8_tempdir();
    let pkg = base.join("sample_package");
    build_sample_package(&pkg);

    let det = detect(&pkg).expect("detect");
    assert_eq!(det.kind, PackageKind::PurviewPackage);

    let matter_root = base.join("matter");
    let matter = Matter::create(&matter_root, "Happy").expect("matter");
    let limits = ExpandLimits::for_tests();

    let summary = ingest_path(&matter, &pkg, &limits, None).expect("ingest");
    assert!(summary.completed);
    assert!(!summary.cancelled);
    assert!(
        summary.entries_ok >= 3,
        "expected leaves, got {}",
        summary.entries_ok
    );
    assert!(summary.psts_found >= 1);
    assert!(summary.nested_zips >= 1);
    assert!(summary.bytes_cas > 0);

    let items = matter
        .list_items_for_source(&summary.source_id)
        .expect("items");
    assert!(!items.is_empty());
    // Nested leaf path present.
    assert!(
        items.iter().any(|i| {
            i.path
                .as_deref()
                .map(|p| p.contains("inner.zip") && p.ends_with("note.txt"))
                .unwrap_or(false)
        }),
        "missing nested note.txt inventory: {:?}",
        items.iter().map(|i| i.path.clone()).collect::<Vec<_>>()
    );

    // CAS digests resolve.
    for item in &items {
        if let Some(ref d) = item.native_sha256 {
            assert!(matter.blob_exists(d).expect("exists"));
        }
    }

    let job = matter.get_job(&summary.job_id).expect("job");
    assert_eq!(job.state, JobState::Succeeded);

    matter.verify_audit_chain().expect("audit chain");

    // Source package untouched (still has original files).
    assert!(pkg.join("mail.pst").as_std_path().is_file());
    assert!(pkg.join("files.zip").as_std_path().is_file());
}

#[test]
fn path_traversal_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("evil.zip");
    write_zip_file(
        zip_path.as_std_path(),
        &[("../escape.txt", b"nope"), ("ok.txt", b"yes")],
    );

    let matter = Matter::create(base.join("matter"), "Trav").expect("matter");
    let limits = ExpandLimits::for_tests();
    let summary = ingest_path(&matter, &zip_path, &limits, None).expect("ingest");

    let items = matter
        .list_items_for_source(&summary.source_id)
        .expect("items");
    assert!(
        items
            .iter()
            .all(|i| !i.path.as_deref().unwrap_or("").contains("..")),
        "traversal path should not be inventoried"
    );
    assert!(
        items
            .iter()
            .any(|i| i.path.as_deref() == Some("evil.zip!/ok.txt")
                || i.path.as_deref() == Some("ok.txt")),
        "safe entry should still expand: {:?}",
        items.iter().map(|i| i.path.clone()).collect::<Vec<_>>()
    );
    let errors = matter
        .item_errors_for_source(&summary.source_id)
        .expect("errs");
    assert!(
        errors.iter().any(|e| e.code == "zip_path_traversal"),
        "expected traversal error: {:?}",
        errors
    );
}

#[test]
fn absolute_path_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("abs.zip");
    write_zip_file(
        zip_path.as_std_path(),
        &[("/etc/passwd", b"root"), ("safe.txt", b"ok")],
    );

    let matter = Matter::create(base.join("matter"), "Abs").expect("matter");
    let summary =
        ingest_path(&matter, &zip_path, &ExpandLimits::for_tests(), None).expect("ingest");

    let errors = matter
        .item_errors_for_source(&summary.source_id)
        .expect("errs");
    assert!(
        errors.iter().any(|e| e.code == "zip_absolute_path"),
        "expected absolute path error: {:?}",
        errors
    );
    let items = matter
        .list_items_for_source(&summary.source_id)
        .expect("items");
    assert!(items.iter().any(|i| {
        i.path
            .as_deref()
            .map(|p| p.ends_with("safe.txt"))
            .unwrap_or(false)
    }));
}

#[test]
fn non_utf8_entry_name_expands() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("legacy.zip");
    // CP437: caf\x82.txt → café.txt
    write_zip_with_raw_name(zip_path.as_std_path(), b"caf\x82.txt", b"legacy body");

    let matter = Matter::create(base.join("matter"), "Enc").expect("matter");
    let summary =
        ingest_path(&matter, &zip_path, &ExpandLimits::for_tests(), None).expect("ingest");

    let items = matter
        .list_items_for_source(&summary.source_id)
        .expect("items");
    assert!(
        items.iter().any(|i| {
            i.path
                .as_deref()
                .map(|p| p.contains('é') || p.contains("caf"))
                .unwrap_or(false)
                && i.native_sha256.is_some()
        }),
        "expected UTF-8 inventory path for CP437 name: {:?}",
        items.iter().map(|i| i.path.clone()).collect::<Vec<_>>()
    );
    // Path must be valid UTF-8 (guaranteed by String).
    for i in &items {
        if let Some(p) = &i.path {
            assert!(std::str::from_utf8(p.as_bytes()).is_ok());
        }
    }
}

#[test]
fn zip_bomb_ratio_limit_trips() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("bomb.zip");
    // Highly compressible payload under Deflate.
    let zeros = vec![0u8; 50_000];
    let file = File::create(zip_path.as_std_path()).expect("create");
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("zeros.bin", opts).expect("start");
    zip.write_all(&zeros).expect("write");
    zip.finish().expect("finish");

    let matter = Matter::create(base.join("matter"), "Bomb").expect("matter");
    let mut limits = ExpandLimits::for_tests();
    limits.max_compression_ratio = 2.0;

    let err = ingest_path(&matter, &zip_path, &limits, None).expect_err("should fail closed");
    match err {
        IngestError::ZipBomb { code, .. } => {
            assert!(code.contains("bomb") || code == "zip_bomb_ratio" || code == "zip_bomb_size");
        }
        other => panic!("expected ZipBomb, got {other:?}"),
    }
    let source_failed = matter
        .connection()
        .query_row("SELECT status FROM sources LIMIT 1", [], |r| {
            r.get::<_, String>(0)
        })
        .expect("status");
    assert_eq!(source_failed, "failed");
}

#[test]
fn resume_mid_archive_skips_inventoried() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("multi.zip");
    write_zip_file(
        zip_path.as_std_path(),
        &[
            ("a.txt", b"aaa"),
            ("b.txt", b"bbb"),
            ("c.txt", b"ccc"),
            ("d.txt", b"ddd"),
        ],
    );

    let matter_root = base.join("matter");
    let matter = Matter::create(&matter_root, "Resume").expect("matter");
    let mut limits = ExpandLimits::for_tests();
    limits.checkpoint_every_n_entries = 1;

    // Cancel polls: once per zip entry loop + once per commit_leaf.
    // After first leaf is committed (~2–3 polls), trip cancel so remaining
    // entries are left for resume (mega-zip mid-archive grain).
    let polls = Arc::new(AtomicU64::new(0));
    let polls_c = Arc::clone(&polls);
    let cancel = move || polls_c.fetch_add(1, Ordering::SeqCst) >= 3;

    let summary1 = ingest_path(&matter, &zip_path, &limits, Some(&cancel)).expect("partial");
    assert!(
        summary1.cancelled,
        "expected cancelled partial run, got completed={}",
        summary1.completed
    );
    let items1 = matter
        .list_items_for_source(&summary1.source_id)
        .expect("items1");
    assert!(
        !items1.is_empty(),
        "expected at least one leaf before cancel"
    );
    assert!(
        items1.len() < 4,
        "cancel should leave work for resume, got {} items",
        items1.len()
    );
    let first_paths: Vec<String> = items1.iter().filter_map(|i| i.path.clone()).collect();
    let first_count = items1.len();
    let first_digests: Vec<Option<String>> =
        items1.iter().map(|i| i.native_sha256.clone()).collect();

    let summary2 = resume_ingest(
        &matter,
        &summary1.source_id,
        &summary1.job_id,
        &limits,
        None,
    )
    .expect("resume");
    assert!(summary2.completed);
    assert!(
        summary2.entries_skipped >= first_count as u64,
        "resume must skip already inventoried leaves (skipped={})",
        summary2.entries_skipped
    );

    let items2 = matter
        .list_items_for_source(&summary1.source_id)
        .expect("items2");
    assert_eq!(items2.len(), 4, "expected all four leaves after resume");

    for p in &first_paths {
        let count = items2
            .iter()
            .filter(|i| i.path.as_deref() == Some(p.as_str()))
            .count();
        assert_eq!(count, 1, "path {p} duplicated after resume");
    }
    for (p, dig) in first_paths.iter().zip(first_digests.iter()) {
        let item = items2
            .iter()
            .find(|i| i.path.as_deref() == Some(p.as_str()))
            .expect("still present");
        assert_eq!(&item.native_sha256, dig);
    }
}

#[test]
fn resume_nested_zip_mid_archive() {
    // Outer zip with pre/post leaves + nested zip of three leaves.
    // Cancel after the first *inner* leaf is inventored; resume must finish
    // remaining inner children (and outer post leaf) without duplicates.
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("nested_outer.zip");
    let inner = write_zip_bytes(&[("a.txt", b"aaa"), ("b.txt", b"bbb"), ("c.txt", b"ccc")]);
    write_zip_file(
        zip_path.as_std_path(),
        &[
            ("pre.txt", b"before nested"),
            ("inner.zip", &inner),
            ("post.txt", b"after nested"),
        ],
    );

    let matter_root = base.join("matter");
    let matter = Matter::create(&matter_root, "NestedResume").expect("matter");
    let mut limits = ExpandLimits::for_tests();
    limits.checkpoint_every_n_entries = 1;

    // Cancel once inventory contains a leaf under the nested zip marker.
    // Use open_for_read: exclusive write lock is held by the ingest handle.
    let matter_root_c = matter_root.clone();
    let cancel = move || match Matter::open_for_read(&matter_root_c) {
        Ok(m) => m
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM items WHERE path LIKE '%inner.zip!/%' \
                 AND native_sha256 IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n >= 1)
            .unwrap_or(false),
        Err(_) => false,
    };

    let summary1 = ingest_path(&matter, &zip_path, &limits, Some(&cancel)).expect("partial");
    assert!(
        summary1.cancelled,
        "expected cancelled after first inner leaf, completed={}",
        summary1.completed
    );

    let items1 = matter
        .list_items_for_source(&summary1.source_id)
        .expect("items1");
    let inner_paths: Vec<_> = items1
        .iter()
        .filter_map(|i| i.path.clone())
        .filter(|p| p.contains("inner.zip!/"))
        .collect();
    assert!(
        !inner_paths.is_empty(),
        "expected at least one inner leaf before cancel: {:?}",
        items1.iter().map(|i| i.path.clone()).collect::<Vec<_>>()
    );
    assert!(
        inner_paths.len() < 3,
        "cancel should leave inner work for resume, got {} inner leaves: {:?}",
        inner_paths.len(),
        inner_paths
    );
    let first_count = items1.len();
    let first_paths: Vec<String> = items1.iter().filter_map(|i| i.path.clone()).collect();

    let summary2 = resume_ingest(
        &matter,
        &summary1.source_id,
        &summary1.job_id,
        &limits,
        None,
    )
    .expect("resume");
    assert!(summary2.completed, "resume should complete");
    assert!(
        summary2.entries_skipped > 0,
        "resume must skip already-inventoried leaves (skipped={})",
        summary2.entries_skipped
    );

    let items2 = matter
        .list_items_for_source(&summary1.source_id)
        .expect("items2");
    let paths2: Vec<String> = items2.iter().filter_map(|i| i.path.clone()).collect();

    // Expected inventory: outer pre/post + container blob + three inner leaves.
    let expected = [
        "nested_outer.zip!/pre.txt",
        "nested_outer.zip!/inner.zip",
        "nested_outer.zip!/inner.zip!/a.txt",
        "nested_outer.zip!/inner.zip!/b.txt",
        "nested_outer.zip!/inner.zip!/c.txt",
        "nested_outer.zip!/post.txt",
    ];
    for exp in &expected {
        let count = paths2.iter().filter(|p| p.as_str() == *exp).count();
        assert_eq!(
            count, 1,
            "expected path {exp} exactly once after resume; got {:?}",
            paths2
        );
    }
    assert_eq!(
        items2.len(),
        expected.len(),
        "unexpected extra inventory rows: {:?}",
        paths2
    );
    assert!(
        items2.len() > first_count,
        "resume should add remaining leaves (before={first_count}, after={})",
        items2.len()
    );

    // Prior paths remain unique and present.
    for p in &first_paths {
        let count = items2
            .iter()
            .filter(|i| i.path.as_deref() == Some(p.as_str()))
            .count();
        assert_eq!(count, 1, "path {p} duplicated or missing after resume");
    }

    matter.verify_audit_chain().expect("audit chain");
}

#[test]
fn corrupt_zip_structured_error() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("corrupt.zip");
    fs::write(zip_path.as_std_path(), b"PK\x03\x04not-a-real-zip-payload").expect("write");

    let matter = Matter::create(base.join("matter"), "Corrupt").expect("matter");
    let err =
        ingest_path(&matter, &zip_path, &ExpandLimits::for_tests(), None).expect_err("corrupt");
    // Structured: Zip or ZipBomb or Io wrapping zip corrupt.
    let code = err.code();
    assert!(
        matches!(code, "zip_corrupt" | "io_error" | "other" | "matter_error")
            || code.starts_with("zip"),
        "unexpected code {code} for {err}"
    );
}

#[test]
fn source_package_not_mutated_on_failure() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("keep.zip");
    let original = b"PK\x03\x04garbage-keep-me";
    fs::write(zip_path.as_std_path(), original).expect("write");
    let before = fs::read(zip_path.as_std_path()).expect("read");

    let matter = Matter::create(base.join("matter"), "NoMut").expect("matter");
    let _ = ingest_path(&matter, &zip_path, &ExpandLimits::for_tests(), None);

    let after = fs::read(zip_path.as_std_path()).expect("read after");
    assert_eq!(before, after, "source package must not be mutated");
}

#[test]
fn single_file_7z_records_unsupported_error() {
    let (_tmp, base) = utf8_tempdir();
    let seven = base.join("archive.7z");
    fs::write(seven.as_std_path(), b"7z fake payload").expect("write");

    let matter = Matter::create(base.join("matter"), "SevenZ").expect("matter");
    let sum = ingest_path(&matter, &seven, &ExpandLimits::for_tests(), None).expect("7z path");
    assert!(!sum.completed);
    assert_eq!(sum.entries_err, 1);
    assert_eq!(sum.kind, PackageKind::Unsupported);

    let src = matter.get_source(&sum.source_id).expect("source");
    assert_eq!(src.status, "failed");
    assert_eq!(src.kind, "unsupported");

    let job = matter.get_job(&sum.job_id).expect("job");
    assert_eq!(job.state, JobState::Failed);

    let errs = matter.item_errors_for_source(&sum.source_id).expect("errs");
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "unsupported_7z"));
    matter.verify_audit_chain().expect("audit");
}

#[cfg(unix)]
#[test]
fn directory_symlink_rejected() {
    use std::os::unix::fs::symlink;
    let (_tmp, base) = utf8_tempdir();
    let pkg = base.join("pkg");
    fs::create_dir_all(pkg.as_std_path()).expect("pkg");
    fs::write(pkg.join("ok.txt").as_std_path(), b"ok").expect("file");
    let outside = base.join("outside.txt");
    fs::write(outside.as_std_path(), b"secret").expect("outside");
    symlink(outside.as_std_path(), pkg.join("link.txt").as_std_path()).expect("symlink");

    let matter = Matter::create(base.join("matter"), "Sym").expect("matter");
    let sum = ingest_path(&matter, &pkg, &ExpandLimits::for_tests(), None).expect("ingest");
    // Symlink skipped with error; real file may still expand.
    let errs = matter.item_errors_for_source(&sum.source_id).expect("errs");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("symlink") || e.code.contains("unsafe")),
        "expected symlink rejection, got {errs:?}"
    );
    // Outside secret must not appear in CAS as only content unless via ok.txt
    let items = matter.list_items_for_source(&sum.source_id).expect("items");
    for it in &items {
        if let Some(ref d) = it.native_sha256 {
            let bytes = matter.get_bytes(d).expect("get");
            assert_ne!(
                bytes.as_slice(),
                b"secret",
                "must not ingest symlink target"
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn directory_symlink_rejected_windows() {
    // Create a directory junction/symlink when privileges allow; otherwise skip softly.
    use std::os::windows::fs::symlink_file;
    let (_tmp, base) = utf8_tempdir();
    let pkg = base.join("pkg");
    fs::create_dir_all(pkg.as_std_path()).expect("pkg");
    fs::write(pkg.join("ok.txt").as_std_path(), b"ok").expect("file");
    let outside = base.join("outside.txt");
    fs::write(outside.as_std_path(), b"secret").expect("outside");
    let link = pkg.join("link.txt");
    if symlink_file(outside.as_std_path(), link.as_std_path()).is_err() {
        // Developer Mode / elevation may be required; unit path still covered by FS check.
        eprintln!("skip: could not create symlink (privileges)");
        return;
    }

    let matter = Matter::create(base.join("matter"), "SymWin").expect("matter");
    let sum = ingest_path(&matter, &pkg, &ExpandLimits::for_tests(), None).expect("ingest");
    let errs = matter.item_errors_for_source(&sum.source_id).expect("errs");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("symlink") || e.message.contains("reparse")),
        "expected symlink rejection, got {errs:?}"
    );
    let items = matter.list_items_for_source(&sum.source_id).expect("items");
    for it in &items {
        if let Some(ref d) = it.native_sha256 {
            let bytes = matter.get_bytes(d).expect("get");
            assert_ne!(bytes.as_slice(), b"secret");
        }
    }
}

#[cfg(windows)]
#[test]
fn package_root_symlink_rejected_windows() {
    use std::os::windows::fs::symlink_file;
    let (_tmp, base) = utf8_tempdir();
    let real = base.join("real.txt");
    fs::write(real.as_std_path(), b"secret-root").expect("write");
    let link = base.join("link-root.txt");
    if symlink_file(real.as_std_path(), link.as_std_path()).is_err() {
        eprintln!("skip: could not create root symlink (privileges)");
        return;
    }
    let det = detect(&link).expect("detect");
    assert_eq!(det.kind, PackageKind::Unsupported);
    assert!(
        det.notes
            .iter()
            .any(|n| n.contains("symlink") || n.contains("reparse") || n.contains("rejected")),
        "notes={:?}",
        det.notes
    );
    let matter = Matter::create(base.join("matter"), "RootSym").expect("matter");
    // Must not follow into CAS.
    let res = ingest_path(&matter, &link, &ExpandLimits::for_tests(), None);
    assert!(
        res.is_err(),
        "expected unsupported err for symlink root, got {res:?}"
    );
}

#[test]
fn ingest_path_on_job_does_not_create_second_job() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("one.zip");
    write_zip_file(zip_path.as_std_path(), &[("a.txt", b"hello")]);

    let matter = Matter::create(base.join("matter"), "OnJob").expect("matter");
    let job = matter.create_job("ingest").expect("create_job");
    matter
        .set_job_state(&job.id, JobState::Running, None)
        .expect("running");

    let before = matter.list_jobs().expect("list").len();
    assert_eq!(before, 1, "precondition: single runner-created job");

    let sum = ingest_path_on_job(
        &matter,
        &zip_path,
        &ExpandLimits::for_tests(),
        &job.id,
        None,
    )
    .expect("on_job");
    assert_eq!(sum.job_id, job.id, "must reuse provided job_id");
    assert!(sum.completed);

    let jobs = matter.list_jobs().expect("list after");
    assert_eq!(
        jobs.len(),
        1,
        "on_job must not insert a second job row: {:?}",
        jobs.iter().map(|j| j.id.clone()).collect::<Vec<_>>()
    );
    assert_eq!(jobs[0].id, job.id);
    assert_eq!(jobs[0].state, JobState::Succeeded);
}

#[test]
fn ingest_path_wrapper_creates_exactly_one_job() {
    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("wrap.zip");
    write_zip_file(zip_path.as_std_path(), &[("b.txt", b"world")]);

    let matter = Matter::create(base.join("matter"), "Wrap").expect("matter");
    let sum = ingest_path(&matter, &zip_path, &ExpandLimits::for_tests(), None).expect("ingest");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, sum.job_id);
    assert_eq!(jobs[0].state, JobState::Succeeded);
}

/// Multi-MB loose PST must stream to CAS even when full-buffer cap is tiny
/// (regression: 256 MiB buffer used to reject multi-GB mailboxes).
#[test]
fn loose_pst_streams_past_buffer_cap() {
    let (_tmp, base) = utf8_tempdir();
    let pst_path = base.join("mailbox.pst");
    // 1 MiB payload — above max_entry_buffer_bytes=64 KiB, under max_entry_bytes.
    let payload = vec![0xABu8; 1024 * 1024];
    fs::write(pst_path.as_std_path(), &payload).expect("write pst");

    let matter = Matter::create(base.join("matter"), "BigPst").expect("matter");
    let mut limits = ExpandLimits::for_tests();
    limits.max_entry_buffer_bytes = 64 * 1024;
    limits.max_entry_bytes = 8 * 1024 * 1024;
    limits.max_uncompressed_bytes = 16 * 1024 * 1024;

    let sum = ingest_path(&matter, &pst_path, &limits, None).expect("ingest multi-mb pst");
    assert!(sum.completed);
    assert_eq!(sum.psts_found, 1);
    assert_eq!(sum.entries_ok, 1);

    let items = matter.list_items_for_source(&sum.source_id).expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].size_bytes, Some(payload.len() as i64));
    assert_eq!(items[0].status, "discovered");
    let dig = items[0].native_sha256.as_deref().expect("digest");
    let got = matter.get_bytes(dig).expect("cas get");
    assert_eq!(got.len(), payload.len());
    assert_eq!(got, payload);
}
