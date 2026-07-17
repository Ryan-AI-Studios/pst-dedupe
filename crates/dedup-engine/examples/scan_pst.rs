//! Headless PST dedup scan using pst-reader + dedup-engine.
//!
//! ```powershell
//! cargo run -p dedup-engine --example scan_pst --release -- "C:\path\to\file.pst"
//! ```

use std::env;
use std::path::PathBuf;
use std::time::Instant;

use dedup_engine::{
    hasher::{self, AttachmentInfo},
    DedupIndex, DedupResult, MessageRef,
};
use pst_reader::PstFile;

fn main() {
    let files: Vec<PathBuf> = env::args().skip(1).map(PathBuf::from).collect();
    if files.is_empty() {
        eprintln!("usage: scan_pst <file.pst> [more.pst ...]");
        std::process::exit(2);
    }

    let start = Instant::now();
    let mut index = DedupIndex::with_capacity_and_tier2(100_000, true);
    let mut total_msgs = 0u64;
    let mut skipped = 0u64;
    let mut savings = 0u64;

    for (file_idx, path) in files.iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("file_{file_idx}"));
        println!("Opening {name}...");

        let mut pst = match PstFile::open(path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("  FAILED open: {e}");
                continue;
            }
        };

        let folders = match pst.folders() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("  FAILED folders: {e}");
                continue;
            }
        };

        let estimated: u64 = folders.iter().map(|f| f.message_count as u64).sum();
        println!(
            "  {} folders, ~{} messages (from contents tables)",
            folders.len(),
            estimated
        );

        for folder in &folders {
            for &msg_nid in &folder.message_nids {
                let props = match pst.read_message_properties(msg_nid) {
                    Ok(p) => p,
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                };

                let attachments = if props.has_attachments.unwrap_or(false) {
                    match pst.read_attachment_metadata(msg_nid) {
                        Ok(atts) => atts
                            .into_iter()
                            .map(|a| AttachmentInfo {
                                filename: a.filename,
                                size: a.size,
                            })
                            .collect(),
                        Err(_) => {
                            skipped += 1;
                            continue;
                        }
                    }
                } else {
                    Vec::new()
                };

                let keys = hasher::compute_dedup_keys(
                    props.message_id.as_deref(),
                    props.subject.as_deref(),
                    props.submit_time,
                    props.sender_email.as_deref(),
                    props.body_preview.as_deref(),
                    &attachments,
                );

                let msg_ref = MessageRef {
                    pst_index: file_idx,
                    pst_name: name.clone(),
                    folder_path: folder.path.clone(),
                    nid: msg_nid.0,
                    subject: props.subject.clone().unwrap_or_default(),
                    submit_time: props.submit_time,
                    sender: props.sender_email.clone().unwrap_or_default(),
                    size: props.message_size.unwrap_or(0) as u32,
                };

                let result = index.check_and_insert(
                    keys.message_id.as_deref(),
                    keys.content_hash,
                    msg_ref.clone(),
                );
                if let DedupResult::DuplicateOf { .. } = result {
                    savings += msg_ref.size as u64;
                }
                total_msgs += 1;
            }
        }
    }

    println!();
    println!("=== Dedup summary ({:.2}s) ===", start.elapsed().as_secs_f64());
    println!("  processed:   {total_msgs}");
    println!("  skipped:     {skipped}");
    println!("  unique:      {}", index.unique_count);
    println!("  duplicates:  {}", index.duplicate_count);
    println!("  tier1 hits:  {}", index.tier1_hits);
    println!("  tier2 hits:  {}", index.tier2_hits);
    println!("  savings B:   {savings}");
}
