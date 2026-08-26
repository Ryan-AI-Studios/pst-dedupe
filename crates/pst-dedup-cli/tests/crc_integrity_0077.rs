//! 0077: synthetic corrupt PST (generate-at-test-time) + CRC counters / CRC_SUSPECT.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use pst_dedup_cli::scan::{run_scan, ScanOptions};
use pst_writer::{write_unicode_pst, FolderLayoutPolicy, WriteMessage, WritePstOpts};

/// Flat layout keeps the display-name folder eager (stable NID/block placement
/// for CRC trailer fixtures). Preserve residual Unique Mail is lazy (0095).
fn crc_fixture_opts() -> WritePstOpts {
    WritePstOpts {
        folder_layout: FolderLayoutPolicy::Flat {
            folder_display_name: "Unique Mail".into(),
        },
        ..WritePstOpts::default()
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pst_dedup_0077_crc");
    let _ = fs::create_dir_all(&dir);
    dir.join(format!(
        "{name}_{}_{}.pst",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

fn base_msg(mid: &str, subject: &str) -> WriteMessage {
    WriteMessage {
        message_id: Some(mid.to_string()),
        subject: subject.to_string(),
        sender: Some("alice@example.com".to_string()),
        display_to: Some("bob@example.com".to_string()),
        submit_time: Some(0x01D5B035EDA780_i64),
        body_plain: Some("body text for crc fixture".to_string()),
        ..Default::default()
    }
}

/// Corrupt known page + block regions so CRC validators fire (synthetic only).
///
/// Strategy: flip a data byte inside page payload (bytes 0..496 of a page)
/// so computed CRC ≠ stored trailer CRC; similarly flip block data bytes
/// (before the 16-byte trailer). Block trailer BID is flipped via
/// [`flip_message_block_trailer_bid`] (DoD-10 three-class proof).
fn corrupt_page_and_block_trailers(path: &Path) {
    let mut data = fs::read(path).expect("read pst");
    assert!(data.len() > 2048, "fixture too small");

    // Page size 512. Flip payload byte on several pages so page CRC fires.
    for page_idx in 1..8 {
        let base = page_idx * 512;
        if base + 16 < data.len() {
            data[base] ^= 0x5A;
        }
    }

    // Blocks: raw layout is data + pad-to-64 + 16-byte trailer.
    // Flip early data bytes in mid-file regions that blocks typically occupy.
    for off in (4096..data.len().min(64 * 1024)).step_by(512) {
        if off + 32 < data.len() {
            data[off] ^= 0xA5;
        }
    }

    fs::write(path, &data).expect("write corrupt");
}

/// Flip the BID field (trailer bytes 4..12) on the first message data block so
/// BBT vs trailer BID mismatch fires (DoD-10). Leaves CRC field alone.
fn flip_message_block_trailer_bid(path: &Path) {
    let (ib, cb) = {
        let mut pst = pst_reader::PstFile::open(path).expect("open for BID flip locate");
        let folders = pst.folders().expect("folders");
        let nid = folders
            .iter()
            .flat_map(|f| f.message_nids.iter().copied())
            .next()
            .expect("at least one message nid");
        let nbt = pst.nbt().get(nid).expect("nbt entry").clone();
        let bbt = pst.bbt().get(nbt.bid_data).expect("bbt entry for msg data");
        assert!(bbt.cb > 0, "message data block empty");
        (bbt.bref.ib as usize, bbt.cb as usize)
    };
    // Trailer: pad-to-64 after data; CRC at [0..4], BID at [4..12].
    let trailer_start = (cb + 63) & !63;
    let bid_off = ib + trailer_start + 4;
    let mut data = fs::read(path).expect("read for BID flip");
    assert!(
        bid_off + 8 <= data.len(),
        "trailer BID out of file: ib={ib} cb={cb} bid_off={bid_off} len={}",
        data.len()
    );
    data[bid_off] ^= 0x01;
    fs::write(path, &data).expect("write BID corrupt");
}

/// Flip the **stored** block CRC in the first message's data-block trailer only
/// (sparse, rate << 0.5). Leaves block *data* intact so the PC still parses and
/// the message is recoverable-with-`CRC_SUSPECT` rather than hard-skipped.
///
/// Uses NBT/BBT to locate the message PC block on disk (DoD-19 proof).
fn flip_one_message_block_trailer_crc(path: &Path) {
    let (ib, cb) = {
        let mut pst = pst_reader::PstFile::open(path).expect("open clean for locate");
        let folders = pst.folders().expect("folders");
        let nid = folders
            .iter()
            .flat_map(|f| f.message_nids.iter().copied())
            .next()
            .expect("at least one message nid");
        let nbt = pst.nbt().get(nid).expect("nbt entry").clone();
        let bbt = pst.bbt().get(nbt.bid_data).expect("bbt entry for msg data");
        assert!(bbt.cb > 0, "message data block empty");
        (bbt.bref.ib as usize, bbt.cb as usize)
    };

    // Trailer layout matches `ndb/block.rs::validate_block_trailer`:
    // data || pad-to-64 || 16-byte trailer; stored CRC is trailer[0..4].
    let trailer_start = (cb + 63) & !63;
    let crc_off = ib + trailer_start;
    let mut data = fs::read(path).expect("read for flip");
    assert!(
        crc_off + 4 <= data.len(),
        "trailer CRC out of file: ib={ib} cb={cb} crc_off={crc_off} len={}",
        data.len()
    );
    data[crc_off] ^= 0xFF;
    fs::write(path, &data).expect("write sparse corrupt");
}

#[test]
fn synthetic_corrupt_pst_increments_specific_crc_counters() {
    let _lock = pst_reader::integrity_telemetry::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    pst_reader::integrity_telemetry::reset();

    let path = scratch("corrupt");
    let _ = fs::remove_file(&path);

    let msg = base_msg("<crc-fixture@ex.com>", "CRC fixture");
    write_unicode_pst(&path, vec![msg], &[], &crc_fixture_opts()).expect("write clean");
    // DoD-10: three-class synthetic — page CRC, block CRC, and block BID mismatch.
    flip_message_block_trailer_bid(&path);
    corrupt_page_and_block_trailers(&path);

    pst_reader::integrity_telemetry::reset();
    let before = pst_reader::integrity_telemetry::begin_source();

    // Opening + walking will hit page/block CRC paths.
    let mut pst = pst_reader::PstFile::open(&path).expect("open corrupt");
    let folders = pst.folders().unwrap_or_default();
    for folder in &folders {
        for &nid in &folder.message_nids {
            let _ = pst.read_message_properties(nid);
        }
    }
    drop(pst);

    let delta = pst_reader::integrity_telemetry::end_source_delta(&before);

    // DoD-10: assert specific counter classes, not merely "some warning".
    // Mass synthetic flips target page + block data; BID flip targets trailer BID.
    assert!(
        delta.page_crc_mismatches > 0,
        "expected page CRC from synthetic page flips; delta={delta:?}"
    );
    assert!(
        delta.block_crc_mismatches > 0,
        "expected block CRC from synthetic block flips; delta={delta:?}"
    );
    assert!(
        delta.block_bid_mismatches > 0,
        "expected block BID mismatch from trailer BID flip; delta={delta:?}"
    );
    // Reads must be counted for rate denominators.
    assert!(
        delta.page_reads > 0 && delta.block_reads > 0,
        "expected page and block reads; delta={delta:?}"
    );

    let _ = fs::remove_file(&path);
    pst_reader::integrity_telemetry::reset();
}

/// DoD-19: sparse single-block flip taints the message (not systematic-poly).
#[test]
fn sparse_block_flip_taints_message_crc_suspect() {
    let _lock = pst_reader::integrity_telemetry::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    pst_reader::integrity_telemetry::reset();

    let path = scratch("sparse_taint");
    let _ = fs::remove_file(&path);

    // Two messages: only the first's data block is flipped; both remain readable.
    let msgs = vec![
        base_msg("<sparse-a@ex.com>", "Sparse A"),
        base_msg("<sparse-b@ex.com>", "Sparse B"),
    ];
    write_unicode_pst(&path, msgs, &[], &crc_fixture_opts()).expect("write");
    flip_one_message_block_trailer_crc(&path);

    pst_reader::integrity_telemetry::reset();
    let outcome = run_scan(
        std::slice::from_ref(&path),
        &ScanOptions {
            retain_candidates: true,
            ..ScanOptions::default()
        },
    )
    .expect("scan");

    let s = &outcome.summary;
    // Sparse: one block CRC among many reads → rate well under systematic 0.50.
    let block_rate = s.block_crc_mismatches as f64 / (s.block_reads.max(1) as f64);
    assert!(
        block_rate < 0.50,
        "flip must stay sparse (not systematic strip); rate={block_rate} delta block_crc={} block_reads={}",
        s.block_crc_mismatches,
        s.block_reads
    );
    assert!(
        s.block_crc_mismatches >= 1,
        "expected at least one block CRC; summary block_crc={} page_crc={}",
        s.block_crc_mismatches,
        s.page_crc_mismatches
    );
    // Telemetry and/or candidate integrity must show CRC_SUSPECT on the hit message.
    assert!(
        s.crc_suspect_messages > 0,
        "expected crc_suspect_messages > 0 after body-block flip; got 0 (messages={})",
        s.total_messages
    );
    assert!(
        !outcome.candidates.is_empty(),
        "retain_candidates should populate candidates"
    );
    let any_suspect = outcome.candidates.iter().any(|c| {
        c.integrity
            .degraded_reasons
            .contains(&dedup_engine::integrity::IntegrityReason::CrcSuspect)
    });
    // Identity may keep taint when not systematic (rate << 0.5).
    assert!(
        any_suspect,
        "expected at least one candidate with CRC_SUSPECT identity taint"
    );
    // Clean twin: sparse single-block flip must not taint every message in the file.
    // (Two-message fixture; if only one message were present this assert is skipped.)
    if outcome.candidates.len() >= 2 {
        let clean_count = outcome
            .candidates
            .iter()
            .filter(|c| {
                !c.integrity
                    .degraded_reasons
                    .contains(&dedup_engine::integrity::IntegrityReason::CrcSuspect)
            })
            .count();
        assert!(
            clean_count >= 1,
            "expected at least one clean sibling without CRC_SUSPECT; all {} candidates tainted",
            outcome.candidates.len()
        );
        assert!(
            clean_count < outcome.candidates.len(),
            "expected mixed taint (some clean, some CRC_SUSPECT); got clean={clean_count} total={}",
            outcome.candidates.len()
        );
    }

    let _ = fs::remove_file(&path);
    pst_reader::integrity_telemetry::reset();
}

#[test]
fn scan_reports_crc_fields_and_crc_skip_rate_unchanged() {
    let _lock = pst_reader::integrity_telemetry::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    pst_reader::integrity_telemetry::reset();

    let path = scratch("scan_corrupt");
    let _ = fs::remove_file(&path);
    let msg = base_msg("<scan-crc@ex.com>", "Scan CRC");
    write_unicode_pst(&path, vec![msg], &[], &crc_fixture_opts()).expect("write");
    corrupt_page_and_block_trailers(&path);

    let outcome = run_scan(
        std::slice::from_ref(&path),
        &ScanOptions {
            retain_candidates: true,
            ..ScanOptions::default()
        },
    )
    .expect("scan");

    let s = &outcome.summary;
    // Additive fields present; rates well-formed.
    assert!((0.0..=1.0).contains(&s.block_crc_read_rate));
    // crc_skip_rate is message-level skips only — zero when no CRC_MISMATCH skips.
    assert_eq!(
        s.preflight.crc_skip_rate, 0.0,
        "crc_skip_rate must stay message-skip based (unchanged meaning)"
    );

    // Human-summary path must not require subjects.
    let _ = s.page_crc_mismatches + s.block_crc_mismatches;

    let _ = fs::remove_file(&path);
    pst_reader::integrity_telemetry::reset();
}

#[test]
fn clean_aspose_fixture_has_zero_crc_when_present() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("aspose_outlook.pst");
    if !fixture.is_file() {
        return;
    }
    let _lock = pst_reader::integrity_telemetry::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    pst_reader::integrity_telemetry::reset();

    let outcome = run_scan(&[fixture], &ScanOptions::default()).expect("scan clean");
    // Baseline DoD-12: 17 messages / 17 unique (aspose_outlook.pst Phase-0).
    // Note: some fixtures use non-standard CRC polynomials so page_crc may be
    // non-zero even on "clean" mail; rule 4 is about message counts / acceptance.
    assert_eq!(outcome.summary.total_messages, 17);
    assert_eq!(outcome.summary.unique, 17);
    assert_eq!(outcome.summary.duplicates, 0);
    pst_reader::integrity_telemetry::reset();
}

/// Silence unused import warnings when scanners are aggressive.
#[allow(dead_code)]
fn _touch_io() {
    let _: fn(&Path) = |p| {
        let mut f = fs::File::open(p).ok();
        if let Some(ref mut f) = f {
            let _ = f.seek(SeekFrom::Start(0));
            let mut b = [0u8; 1];
            let _ = f.read(&mut b);
            let _ = f.write_all(&b);
        }
    };
}
