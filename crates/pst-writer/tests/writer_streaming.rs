//! Track 0070 — streaming scale: AMap-aware layout, chunked attach, progress,
//! stop_and_finalize, inline hashes, same-dir temp.

use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use md5::{Digest as Md5Digest, Md5};
use pst_writer::{
    is_amap_page_offset, temp_sibling_path, write_unicode_pst, write_unicode_pst_streaming,
    write_unicode_pst_with_streams, AttachRead, AttachStreamSource, WriteAttachment, WriteMessage,
    WriteProgress, WriteProgressSink, WritePstOpts, WriteStage, AMAP_FIRST_OFFSET, AMAP_INTERVAL,
};
use sha2::{Digest as Sha2Digest, Sha256};

fn scratch(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("pst_writer_0070_{name}_{}.pst", std::process::id()));
    p
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    // Best-effort temp siblings.
    if let Some(parent) = path.parent() {
        if let Ok(rd) = std::fs::read_dir(parent) {
            let stem = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with(&stem) && n.contains(".tmp-") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn base_msg(mid: &str, subject: &str) -> WriteMessage {
    WriteMessage {
        message_id: Some(mid.into()),
        subject: subject.into(),
        sender: Some("a@ex.com".into()),
        body_plain: Some("hello".into()),
        ..WriteMessage::default()
    }
}

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let b = bytes.as_ref();
    let mut s = String::with_capacity(b.len() * 2);
    for &byte in b {
        s.push(HEX[(byte >> 4) as usize] as char);
        s.push(HEX[(byte & 0xf) as usize] as char);
    }
    s
}

fn file_sha256_md5(path: &Path) -> (String, String) {
    let data = std::fs::read(path).expect("read");
    let mut sha = Sha256::new();
    sha.update(&data);
    let mut md5 = Md5::new();
    md5.update(&data);
    (digest_hex(sha.finalize()), digest_hex(md5.finalize()))
}

// ── 1: Streaming small N ≈ collect path counts ───────────────────────────────

#[test]
fn streaming_small_n_matches_collect_counts() {
    let path_a = scratch("stream_vs_collect_a");
    let path_b = scratch("stream_vs_collect_b");
    cleanup(&path_a);
    cleanup(&path_b);

    let msgs: Vec<WriteMessage> = (0..5)
        .map(|i| base_msg(&format!("<s{i}@ex.com>"), &format!("Subj {i}")))
        .collect();

    let r1 =
        write_unicode_pst(&path_a, msgs.clone(), &[], &WritePstOpts::default()).expect("collect");
    let r2 = write_unicode_pst_streaming(&path_b, msgs, &[], &WritePstOpts::default(), None, None)
        .expect("stream");

    assert_eq!(r1.messages_written, r2.messages_written);
    assert_eq!(r1.folders_created, r2.folders_created);
    assert!(!r2.finalized_early);
    assert_eq!(r2.sha256_hex.len(), 64);
    assert_eq!(r2.md5_hex.len(), 32);

    cleanup(&path_a);
    cleanup(&path_b);
}

// ── 2: Chunked attach mid-size bytes match buffered ──────────────────────────

struct ChunkStream {
    bytes: Vec<u8>,
    open_attach_calls: u32,
    open_stream_calls: u32,
}

impl AttachStreamSource for ChunkStream {
    fn open_attach(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        // Must not be used on the chunked-stream path — fail hard so a
        // regression that prefers full-buffer open_attach cannot soft-pass.
        self.open_attach_calls += 1;
        Err("must use open_attach_stream".into())
    }

    fn open_attach_stream(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<AttachRead>, String> {
        self.open_stream_calls += 1;
        Ok(Some(AttachRead::from_reader(Box::new(Cursor::new(
            self.bytes.clone(),
        )))))
    }
}

#[test]
fn chunked_attach_mid_size_matches_buffered() {
    let payload: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();

    let path_buf = scratch("chunk_buf");
    let path_stream = scratch("chunk_stream");
    cleanup(&path_buf);
    cleanup(&path_stream);

    let mut msg_buf = base_msg("<c1@ex.com>", "Buffered attach");
    msg_buf.attachments.push(WriteAttachment {
        filename: "blob.bin".into(),
        size: payload.len() as u32,
        data: Some(payload.clone()),
        ..WriteAttachment::default()
    });

    let mut msg_stream = base_msg("<c2@ex.com>", "Streamed attach");
    msg_stream.attachments.push(WriteAttachment {
        filename: "blob.bin".into(),
        size: payload.len() as u32,
        data: None,
        stream_available: true,
        ..WriteAttachment::default()
    });

    let r_buf =
        write_unicode_pst(&path_buf, vec![msg_buf], &[], &WritePstOpts::default()).expect("buf");
    let mut src = ChunkStream {
        bytes: payload.clone(),
        open_attach_calls: 0,
        open_stream_calls: 0,
    };
    let r_stream = write_unicode_pst_with_streams(
        &path_stream,
        vec![msg_stream],
        &[],
        &WritePstOpts::default(),
        Some(&mut src),
    )
    .expect("stream");

    assert_eq!(
        src.open_attach_calls, 0,
        "chunked path must not call open_attach"
    );
    assert!(
        src.open_stream_calls >= 1,
        "expected open_attach_stream; got {}",
        src.open_stream_calls
    );
    assert_eq!(r_buf.attachments_written, 1);
    assert_eq!(r_stream.attachments_written, 1);

    let mut pst = pst_reader::PstFile::open(&path_stream).expect("open");
    let folders = pst.folders().expect("folders");
    let mut found = false;
    for f in &folders {
        for &nid in &f.message_nids {
            if let Ok(m) = pst.read_message_extract(nid) {
                if m.subject.as_deref() == Some("Streamed attach") {
                    found = true;
                    let attaches = pst.list_attachments(nid).expect("list");
                    assert_eq!(attaches.len(), 1);
                    let mut reader = pst
                        .open_attachment_data(nid, attaches[0].nid)
                        .expect("data");
                    let mut data = Vec::new();
                    reader.read_to_end(&mut data).expect("read");
                    assert_eq!(data.len(), payload.len());
                    assert_eq!(data, payload);
                }
            }
        }
    }
    assert!(found, "streamed message not found");

    cleanup(&path_buf);
    cleanup(&path_stream);
}

// ── 3: Many small messages reader count N ────────────────────────────────────

#[test]
fn many_small_messages_reader_count() {
    let path = scratch("many_small");
    cleanup(&path);
    let n = 80u64;
    let msgs: Vec<_> = (0..n)
        .map(|i| base_msg(&format!("<m{i}@ex.com>"), &format!("M{i}")))
        .collect();
    let report =
        write_unicode_pst_streaming(&path, msgs, &[], &WritePstOpts::default(), None, None)
            .expect("write");
    assert_eq!(report.messages_written, n);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let total: usize = folders.iter().map(|f| f.message_nids.len()).sum();
    assert_eq!(total as u64, n);
    cleanup(&path);
}

// ── 4: Large body XBLOCK round-trip ──────────────────────────────────────────

#[test]
fn large_body_xblock_round_trip_streaming() {
    let path = scratch("large_body");
    cleanup(&path);
    let body: String = "Z".repeat(40_000);
    let mut msg = base_msg("<lb@ex.com>", "Large body");
    msg.body_plain = Some(body.clone());
    write_unicode_pst_streaming(
        &path,
        std::iter::once(msg),
        &[],
        &WritePstOpts::default(),
        None,
        None,
    )
    .expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let mut ok = false;
    for f in &folders {
        for &nid in &f.message_nids {
            if let Ok(m) = pst.read_message_extract(nid) {
                if let Some(p) = m.body_text.as_deref() {
                    assert_eq!(p.len(), body.len());
                    ok = true;
                }
            }
        }
    }
    assert!(ok);
    cleanup(&path);
}

// ── 5: AMap boundary cross ───────────────────────────────────────────────────

#[test]
fn amap_boundary_cross_structure_valid() {
    // Payload large enough that layout + data crosses first AMap at 0x4400
    // and ideally approaches/crosses further intervals under synthetic load.
    let path = scratch("amap_cross");
    cleanup(&path);

    // ~300 KB body forces multi-block + substantial file size past 0x4400.
    let body = "A".repeat(300_000);
    let mut msg = base_msg("<amap@ex.com>", "AMap cross");
    msg.body_plain = Some(body);
    // Extra attach to grow file.
    let attach_bytes = vec![0x5Au8; 200_000];
    msg.attachments.push(WriteAttachment {
        filename: "big.bin".into(),
        size: attach_bytes.len() as u32,
        data: Some(attach_bytes),
        ..WriteAttachment::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert!(
        report.bytes > AMAP_FIRST_OFFSET + 512,
        "file should extend past first AMap; bytes={}",
        report.bytes
    );

    // Raw file: AMap page type 0x84 at first AMap offset.
    let raw = std::fs::read(&path).expect("read raw");
    let amap_off = AMAP_FIRST_OFFSET as usize;
    assert!(raw.len() > amap_off + 512);
    // Page trailer ptype at end of 512-byte page (offset 496 within page).
    let ptype = raw[amap_off + 496];
    assert_eq!(ptype, 0x84, "AMap ptype at {AMAP_FIRST_OFFSET:#x}");

    // If file crosses second AMap slot, check it too.
    let second = AMAP_FIRST_OFFSET + AMAP_INTERVAL;
    if report.bytes > second + 512 {
        let p2 = raw[second as usize + 496];
        assert_eq!(p2, 0x84, "AMap ptype at second slot {second:#x}");
        assert!(is_amap_page_offset(second));
    }

    let mut pst = pst_reader::PstFile::open(&path).expect("reader open");
    let _ = pst.folders().expect("folders");
    cleanup(&path);
}

// ── 6: Progress physical size ────────────────────────────────────────────────

struct CaptureProgress {
    last: Option<WriteProgress>,
    ticks: u32,
}

impl WriteProgressSink for CaptureProgress {
    fn on_progress(&mut self, p: &WriteProgress) {
        self.ticks += 1;
        self.last = Some(p.clone());
    }
}

#[test]
fn progress_exposes_physical_size_and_stages() {
    let path = scratch("progress_phys");
    cleanup(&path);
    let mut sink = CaptureProgress {
        last: None,
        ticks: 0,
    };
    let msgs: Vec<_> = (0..3)
        .map(|i| base_msg(&format!("<p{i}@ex.com>"), &format!("P{i}")))
        .collect();
    let report = write_unicode_pst_streaming(
        &path,
        msgs,
        &[],
        &WritePstOpts::default(),
        None,
        Some(&mut sink),
    )
    .expect("write");

    assert!(sink.ticks >= 2);
    let last = sink.last.expect("last progress");
    assert_eq!(last.stage, WriteStage::Renaming);
    assert_eq!(last.messages_written, 3);
    // Final physical size should match report bytes (± we use exact file len).
    assert_eq!(last.current_physical_size, report.bytes);
    let on_disk = std::fs::metadata(&path).expect("meta").len();
    assert_eq!(report.bytes, on_disk);
    cleanup(&path);
}

// ── 7: stop_and_finalize early stop exact N ──────────────────────────────────

struct StopAfter {
    n: u64,
}

impl WriteProgressSink for StopAfter {
    fn on_progress(&mut self, _p: &WriteProgress) {}
    fn should_stop_and_finalize(&self, p: &WriteProgress) -> bool {
        p.stage == WriteStage::WritingMessages && p.messages_written >= self.n
    }
}

#[test]
fn stop_and_finalize_exact_n_openable() {
    let path = scratch("stop_n");
    cleanup(&path);
    let msgs: Vec<_> = (0..10)
        .map(|i| base_msg(&format!("<st{i}@ex.com>"), &format!("Stop{i}")))
        .collect();
    let mut sink = StopAfter { n: 3 };
    let report = write_unicode_pst_streaming(
        &path,
        msgs,
        &[],
        &WritePstOpts::default(),
        None,
        Some(&mut sink),
    )
    .expect("write");

    assert!(report.finalized_early);
    assert_eq!(report.messages_written, 3);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let total: usize = folders.iter().map(|f| f.message_nids.len()).sum();
    assert_eq!(total, 3);
    cleanup(&path);
}

// ── 7b: stop when physical size threshold crossed (0071 volume-cut hook) ─────

struct StopAtPhysical {
    threshold: u64,
    /// Peak physical size observed during WritingMessages.
    max_writing_phys: u64,
    ticks_writing: u32,
}

impl WriteProgressSink for StopAtPhysical {
    fn on_progress(&mut self, p: &WriteProgress) {
        if p.stage == WriteStage::WritingMessages {
            self.ticks_writing += 1;
            self.max_writing_phys = self.max_writing_phys.max(p.current_physical_size);
        }
    }
    fn should_stop_and_finalize(&self, p: &WriteProgress) -> bool {
        p.stage == WriteStage::WritingMessages && p.current_physical_size >= self.threshold
    }
}

#[test]
fn stop_on_physical_size_threshold_openable() {
    let path = scratch("stop_phys");
    cleanup(&path);
    // Messages with mid-size attaches so physical temp grows while writing.
    let msgs: Vec<_> = (0..20)
        .map(|i| {
            let mut m = base_msg(&format!("<sp{i}@ex.com>"), &format!("Phys{i}"));
            m.body_plain = Some("X".repeat(8_000));
            m.attachments.push(WriteAttachment {
                filename: format!("blob{i}.bin"),
                size: 50_000,
                data: Some(vec![0xBBu8; 50_000]),
                ..WriteAttachment::default()
            });
            m
        })
        .collect();

    // Threshold low enough that we stop mid-batch after a few messages.
    let mut sink = StopAtPhysical {
        threshold: 80_000,
        max_writing_phys: 0,
        ticks_writing: 0,
    };
    let report = write_unicode_pst_streaming(
        &path,
        msgs,
        &[],
        &WritePstOpts::default(),
        None,
        Some(&mut sink),
    )
    .expect("write");

    assert!(
        report.finalized_early,
        "expected early stop on physical size"
    );
    assert!(
        report.messages_written > 0 && report.messages_written < 20,
        "expected partial write, got {}",
        report.messages_written
    );
    assert!(
        sink.max_writing_phys >= sink.threshold,
        "WritingMessages progress must expose true physical growth (max={})",
        sink.max_writing_phys
    );
    assert!(sink.ticks_writing >= 1);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let total: usize = folders.iter().map(|f| f.message_nids.len()).sum();
    assert_eq!(total as u64, report.messages_written);
    cleanup(&path);
}

// ── 8: Inline hash matches file ──────────────────────────────────────────────

#[test]
fn inline_hash_matches_on_disk() {
    let path = scratch("hash_match");
    cleanup(&path);
    let report = write_unicode_pst(
        &path,
        vec![base_msg("<h@ex.com>", "Hash me")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");
    let (sha, md5) = file_sha256_md5(&path);
    assert_eq!(report.sha256_hex, sha);
    assert_eq!(report.md5_hex, md5);
    cleanup(&path);
}

// ── 9: Same-dir temp parent ──────────────────────────────────────────────────

#[test]
fn same_dir_temp_parent_equals_out_parent() {
    let path = scratch("same_dir");
    let tmp = temp_sibling_path(&path);
    assert_eq!(
        path.parent().map(|p| p.to_path_buf()),
        tmp.parent().map(|p| p.to_path_buf())
    );
    // And a real write uses that scheme (no leftover cross-volume temp).
    cleanup(&path);
    write_unicode_pst(
        &path,
        vec![base_msg("<sd@ex.com>", "SD")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");
    assert!(path.exists());
    cleanup(&path);
}

// ── 10: Soft attach fail mid-stream ──────────────────────────────────────────

struct FailStream;

impl AttachStreamSource for FailStream {
    fn open_attach(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        Err("boom".into())
    }

    fn open_attach_stream(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<AttachRead>, String> {
        // Stream that errors on first read.
        Ok(Some(AttachRead::from_reader(Box::new(FailingRead))))
    }
}

struct FailingRead;
impl Read for FailingRead {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("mid-stream fail"))
    }
}

/// Yields one full chunk then errors — exercises transactional rollback of
/// already-spilled leaf blocks (Codex P2: no orphan BBT entries).
struct PartialThenFail {
    sent: bool,
}
impl Read for PartialThenFail {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.sent {
            self.sent = true;
            // Fill one STREAM_CHUNK-sized read (or buffer len).
            for b in buf.iter_mut() {
                *b = 0xAB;
            }
            Ok(buf.len())
        } else {
            Err(io::Error::other("fail after first chunk"))
        }
    }
}

struct PartialFailStream;
impl AttachStreamSource for PartialFailStream {
    fn open_attach(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        Err("must use open_attach_stream".into())
    }

    fn open_attach_stream(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<AttachRead>, String> {
        Ok(Some(AttachRead::from_reader(Box::new(PartialThenFail {
            sent: false,
        }))))
    }
}

#[test]
fn soft_attach_fail_mid_stream_keeps_message() {
    let path = scratch("soft_fail_stream");
    cleanup(&path);
    let mut msg = base_msg("<sf@ex.com>", "Soft fail stream");
    msg.attachments.push(WriteAttachment {
        filename: "x.bin".into(),
        size: 100,
        data: None,
        stream_available: true,
        ..WriteAttachment::default()
    });
    let mut src = FailStream;
    let report = write_unicode_pst_with_streams(
        &path,
        vec![msg],
        &[],
        &WritePstOpts::default(),
        Some(&mut src),
    )
    .expect("write");
    assert_eq!(report.messages_written, 1);
    assert_eq!(report.attachments_failed, 1);
    assert_eq!(report.attachments_written, 0);
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let _ = pst.folders().expect("folders");
    cleanup(&path);
}

#[test]
fn soft_attach_fail_after_partial_chunk_rolls_back_orphans() {
    let path = scratch("soft_fail_partial");
    cleanup(&path);
    let mut msg = base_msg("<pf@ex.com>", "Partial stream fail");
    msg.body_plain = Some("kept".into());
    msg.attachments.push(WriteAttachment {
        filename: "partial.bin".into(),
        size: 100_000,
        data: None,
        stream_available: true,
        ..WriteAttachment::default()
    });
    // Second message with a small real attach proves writer still healthy after rollback.
    let mut msg2 = base_msg("<pf2@ex.com>", "After rollback");
    msg2.body_plain = Some("ok".into());
    msg2.attachments.push(WriteAttachment {
        filename: "ok.txt".into(),
        size: 3,
        data: Some(b"ok!".to_vec()),
        ..WriteAttachment::default()
    });
    let mut src = PartialFailStream;
    let report = write_unicode_pst_with_streams(
        &path,
        vec![msg, msg2],
        &[],
        &WritePstOpts::default(),
        Some(&mut src),
    )
    .expect("write");
    assert_eq!(report.messages_written, 2);
    assert_eq!(report.attachments_failed, 1);
    assert_eq!(report.attachments_written, 1);
    // File must open; partial attach must not prevent successful second attach.
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    assert!(!pst.folders().expect("folders").is_empty());
    cleanup(&path);
}

// ── 12: Protected source refuse ──────────────────────────────────────────────

#[test]
fn streaming_refuses_protected_source() {
    let path = scratch("prot");
    cleanup(&path);
    // Create a dummy "source" that matches destination.
    std::fs::write(&path, b"not-a-pst").expect("seed");
    let opts = WritePstOpts {
        overwrite: true,
        ..WritePstOpts::default()
    };
    let err = write_unicode_pst_streaming(
        &path,
        std::iter::once(base_msg("<x@ex.com>", "X")),
        std::slice::from_ref(&path),
        &opts,
        None,
        None,
    );
    assert!(err.is_err());
    cleanup(&path);
}

// ── 13: CI-capped stress (~16–32 MB attach stream) ───────────────────────────

struct BigStream {
    size: usize,
    /// Pattern byte.
    fill: u8,
    open_attach_calls: u32,
    open_stream_calls: u32,
}

impl AttachStreamSource for BigStream {
    fn open_attach(
        &mut self,
        _: Option<&str>,
        _: Option<u64>,
        _: Option<u64>,
        _: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        // CI stress must not succeed via full-buffer open_attach.
        self.open_attach_calls += 1;
        Err("must use open_attach_stream".into())
    }

    fn open_attach_stream(
        &mut self,
        _: Option<&str>,
        _: Option<u64>,
        _: Option<u64>,
        _: &str,
    ) -> Result<Option<AttachRead>, String> {
        self.open_stream_calls += 1;
        let size = self.size;
        let fill = self.fill;
        // Streaming reader that never holds the full buffer.
        Ok(Some(AttachRead::from_reader(Box::new(PatternReader {
            remaining: size,
            fill,
        }))))
    }
}

struct PatternReader {
    remaining: usize,
    fill: u8,
}

impl Read for PatternReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let n = buf.len().min(self.remaining);
        buf[..n].fill(self.fill);
        self.remaining -= n;
        Ok(n)
    }
}

#[test]
fn ci_capped_stress_16mb_attach_stream() {
    let path = scratch("stress_16m");
    cleanup(&path);
    const SIZE: usize = 16 * 1024 * 1024; // 16 MiB — CI-friendly
    let mut msg = base_msg("<stress@ex.com>", "Stress");
    msg.body_plain = Some("stress body".into());
    msg.attachments.push(WriteAttachment {
        filename: "stress.bin".into(),
        size: SIZE as u32,
        data: None,
        stream_available: true,
        ..WriteAttachment::default()
    });
    // Also several small messages.
    let mut batch = vec![msg];
    for i in 0..20 {
        batch.push(base_msg(&format!("<s{i}@ex.com>"), &format!("S{i}")));
    }
    let mut src = BigStream {
        size: SIZE,
        fill: 0xA5,
        open_attach_calls: 0,
        open_stream_calls: 0,
    };
    let report = write_unicode_pst_streaming(
        &path,
        batch,
        &[],
        &WritePstOpts::default(),
        Some(&mut src),
        None,
    )
    .expect("write");
    assert_eq!(
        src.open_attach_calls, 0,
        "stress path must not call open_attach"
    );
    assert!(
        src.open_stream_calls >= 1,
        "expected open_attach_stream; got {}",
        src.open_stream_calls
    );
    assert_eq!(report.messages_written, 21);
    assert_eq!(report.attachments_written, 1);
    assert!(report.bytes > SIZE as u64);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let total: usize = folders.iter().map(|f| f.message_nids.len()).sum();
    assert_eq!(total, 21);
    cleanup(&path);
}

// ── 14: One-pass streaming — no full WriteMessage pre-collect (DoD-1) ────────

/// Yields messages lazily. `next()` panics if another message is produced
/// before the previous one was written (shared `written` counter from the
/// progress sink). A full `collect()` before the write loop would pull all
/// items with `written == 0` and fail on the second `next()`.
struct OnePassIter {
    n: usize,
    next_i: usize,
    produced: std::rc::Rc<std::cell::Cell<usize>>,
    /// Updated by [`OnePassSink`] after each fully written message.
    written: std::rc::Rc<std::cell::Cell<usize>>,
}

impl OnePassIter {
    fn new(
        n: usize,
    ) -> (
        Self,
        std::rc::Rc<std::cell::Cell<usize>>,
        std::rc::Rc<std::cell::Cell<usize>>,
    ) {
        let produced = std::rc::Rc::new(std::cell::Cell::new(0));
        let written = std::rc::Rc::new(std::cell::Cell::new(0));
        (
            Self {
                n,
                next_i: 0,
                produced: produced.clone(),
                written: written.clone(),
            },
            produced,
            written,
        )
    }
}

impl Iterator for OnePassIter {
    type Item = WriteMessage;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_i >= self.n {
            return None;
        }
        let produced = self.produced.get();
        let written = self.written.get();
        // In-flight bound: at most one produced message may be ahead of written.
        // Pre-collect exhausts the iterator first → second next() sees written=0,
        // produced=1 and panics.
        assert!(
            written >= produced,
            "DTO pre-collect regression: next() for index {} while produced={produced} written={written}",
            self.next_i
        );
        self.produced.set(produced + 1);
        let i = self.next_i;
        self.next_i += 1;

        let body = format!("payload-{i}-{}", "X".repeat(1000));
        Some(WriteMessage {
            message_id: Some(format!("<count{i}@ex.com>")),
            subject: format!("OnePass {i}"),
            sender: Some("a@ex.com".into()),
            body_plain: Some(body),
            ..WriteMessage::default()
        })
    }
}

struct OnePassSink {
    produced: std::rc::Rc<std::cell::Cell<usize>>,
    written: std::rc::Rc<std::cell::Cell<usize>>,
    /// Max of (produced − messages_written) during WritingMessages.
    max_ahead: u64,
}

impl WriteProgressSink for OnePassSink {
    fn on_progress(&mut self, p: &WriteProgress) {
        if p.stage == WriteStage::WritingMessages {
            self.written.set(p.messages_written as usize);
            let produced = self.produced.get() as u64;
            let ahead = produced.saturating_sub(p.messages_written);
            self.max_ahead = self.max_ahead.max(ahead);
        }
    }
}

#[test]
fn one_pass_streaming_no_dto_precollect() {
    let path = scratch("one_pass");
    cleanup(&path);

    let n = 8usize;
    let (iter, produced, written) = OnePassIter::new(n);
    let mut sink = OnePassSink {
        produced: produced.clone(),
        written: written.clone(),
        max_ahead: 0,
    };

    let report = write_unicode_pst_streaming(
        &path,
        iter,
        &[],
        &WritePstOpts::default(),
        None,
        Some(&mut sink),
    )
    .expect("write");

    assert_eq!(report.messages_written, n as u64);
    assert_eq!(produced.get(), n);
    assert_eq!(written.get(), n);
    // Pre-collect: produced=N while written still 0 → max_ahead=N.
    // One-pass: produced stays within 1 of written (the message in flight).
    assert!(
        sink.max_ahead <= 1,
        "DTO pre-collect regression: max produced-ahead-of-written was {}",
        sink.max_ahead
    );

    cleanup(&path);
}

// ── Unit: AMap helpers ───────────────────────────────────────────────────────

#[test]
fn amap_helper_constants() {
    assert_eq!(AMAP_FIRST_OFFSET, 0x4400);
    assert_eq!(AMAP_INTERVAL, 253_952);
    assert!(is_amap_page_offset(AMAP_FIRST_OFFSET));
    assert!(is_amap_page_offset(AMAP_FIRST_OFFSET + AMAP_INTERVAL));
    assert!(!is_amap_page_offset(AMAP_FIRST_OFFSET + 1));
    assert!(!is_amap_page_offset(0x1000));
}
