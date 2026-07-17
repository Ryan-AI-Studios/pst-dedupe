//! Open a PST path and print folder/message summary.
//!
//! ```powershell
//! cargo run -p pst-reader --example open_pst -- "C:\path\to\file.pst"
//! ```

use std::env;
use std::time::Instant;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: open_pst <path-to.pst>");
        std::process::exit(2);
    });

    let start = Instant::now();
    println!("Opening: {path}");

    let mut pst = match pst_reader::PstFile::open(&path) {
        Ok(pst) => pst,
        Err(e) => {
            eprintln!("FAILED to open PST: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "Opened in {:.2}s — file_size={} bytes",
        start.elapsed().as_secs_f64(),
        pst.file_size()
    );

    let folders = match pst.folders() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("FAILED to list folders: {e}");
            std::process::exit(1);
        }
    };

    let mut total_msgs: u64 = 0;
    let mut folder_count = 0usize;
    let mut sample_ok = 0u64;
    let mut sample_err = 0u64;

    for folder in &folders {
        folder_count += 1;
        total_msgs += folder.message_count as u64;
        println!(
            "  [{}] {} — {} messages ({} nids)",
            folder_count,
            folder.path,
            folder.message_count,
            folder.message_nids.len()
        );

        // Sample first few message property reads per folder (smoke, not full scan).
        for &nid in folder.message_nids.iter().take(3) {
            match pst.read_message_properties(nid) {
                Ok(m) => {
                    sample_ok += 1;
                    let subj = m.subject.as_deref().unwrap_or("(no subject)");
                    let from = m.sender_email.as_deref().unwrap_or("(no sender)");
                    let mid = m.message_id.as_deref().unwrap_or("(no Message-ID)");
                    println!("      ok nid={nid:?} | {subj} | {from} | {mid}");
                }
                Err(e) => {
                    sample_err += 1;
                    println!("      ERR nid={nid:?}: {e}");
                }
            }
        }
    }

    println!();
    println!("Summary");
    println!("  folders:          {folder_count}");
    println!("  message_count:    {total_msgs}");
    println!("  sample reads ok:  {sample_ok}");
    println!("  sample reads err: {sample_err}");
    println!("  elapsed:          {:.2}s", start.elapsed().as_secs_f64());
}
