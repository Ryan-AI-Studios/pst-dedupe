use std::path::PathBuf;

#[test]
fn test_create_pst_from_eml() {
    let eml_dir = PathBuf::from(
        "C:\\Users\\RyanB\\Desktop\\thundertest\\PROMOTIONS_20260516-0706\\PROMOTIONS",
    );
    if !eml_dir.exists() {
        eprintln!("Skipping: EML directory does not exist");
        return;
    }

    let eml_files: Vec<PathBuf> = std::fs::read_dir(&eml_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("eml"))
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();

    if eml_files.is_empty() {
        eprintln!("Skipping: no EML files found");
        return;
    }

    let mut emls = Vec::new();
    for path in &eml_files {
        match pst_writer::eml::EmlMessage::from_file(path) {
            Ok(eml) => emls.push(eml),
            Err(e) => eprintln!("Failed to parse {}: {}", path.display(), e),
        }
    }

    println!("Parsed {} EML files", emls.len());

    let output = std::env::temp_dir().join("pst_writer_test.pst");
    pst_writer::write_pst_from_emls(&output, &emls).expect("write PST");

    println!(
        "Wrote PST to {} ({} bytes)",
        output.display(),
        std::fs::metadata(&output).unwrap().len()
    );

    // Verify with pst-reader
    let mut pst = pst_reader::PstFile::open(&output).expect("open written PST");
    let folders = pst.folders().expect("traverse folders");

    let total_messages: u32 = folders.iter().map(|f| f.message_count).sum();
    println!(
        "PST has {} folders, {} messages",
        folders.len(),
        total_messages
    );

    assert!(
        total_messages > 0,
        "Expected at least one message in the PST"
    );

    // Verify we can read message properties
    let mut found_properties = false;
    for folder in &folders {
        for &nid in &folder.message_nids {
            let msg = pst
                .read_message_properties(nid)
                .expect("read message properties");
            println!(
                "Message {}: subject={:?}, sender={:?}, mid={:?}",
                nid.0, msg.subject, msg.sender_email, msg.message_id
            );
            if msg.subject.is_some() || msg.sender_email.is_some() || msg.message_id.is_some() {
                found_properties = true;
            }
        }
    }

    assert!(found_properties, "Expected to find some message properties");

    // Clean up
    let _ = std::fs::remove_file(&output);
}
