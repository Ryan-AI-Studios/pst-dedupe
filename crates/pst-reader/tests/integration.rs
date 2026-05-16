//! Integration tests for pst-reader against real PST fixtures.
//!
//! These tests require a real Unicode PST file in the workspace `fixtures/` directory.
//! If no fixtures are present, tests skip gracefully rather than fail.

mod fixtures;

use fixtures::{discover_pst_fixtures, first_fixture};
use pst_reader::PstFile;

// ---------------------------------------------------------------------------
// Fixture discovery
// ---------------------------------------------------------------------------

#[test]
fn test_fixture_discovery() {
    let fixtures = discover_pst_fixtures();
    // We expect at least the Aspose sample fixture in CI/dev environments.
    assert!(
        !fixtures.is_empty(),
        "No PST fixtures found in fixtures/. \
         Place .pst files there to run integration tests."
    );
}

// ---------------------------------------------------------------------------
// Smoke: open a real PST
// ---------------------------------------------------------------------------

#[test]
fn test_open_real_pst() {
    let path = match first_fixture() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: no PST fixtures available");
            return;
        }
    };

    let pst = PstFile::open(&path);
    assert!(
        pst.is_ok(),
        "Failed to open fixture {}: {:?}",
        path.display(),
        pst.err()
    );
}

#[test]
fn test_open_missing_file_is_error() {
    let result = PstFile::open("fixtures/does_not_exist.pst");
    assert!(result.is_err(), "Opening a missing file should fail");
}

// ---------------------------------------------------------------------------
// Traversal: folders and messages
// ---------------------------------------------------------------------------

#[test]
fn test_folder_traversal() {
    let path = match first_fixture() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: no PST fixtures available");
            return;
        }
    };

    let mut pst = PstFile::open(&path).expect("open fixture");
    let folders = pst.folders().expect("traverse folders");

    assert!(
        !folders.is_empty(),
        "Expected at least one folder in a real PST"
    );

    // The Aspose sample fixture is known to contain folders.
    let root = folders.iter().find(|f| f.nid.0 == 0x122);
    assert!(root.is_some(), "Expected root folder (NID 0x122)");

    let total_messages: u32 = folders.iter().map(|f| f.message_count).sum();
    eprintln!(
        "Fixture {}: {} folders, {} messages",
        path.display(),
        folders.len(),
        total_messages
    );

    // A real PST fixture may have zero messages (e.g., empty store),
    // so we only log rather than assert.
}

#[test]
fn test_message_property_extraction() {
    let path = match first_fixture() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: no PST fixtures available");
            return;
        }
    };

    let mut pst = PstFile::open(&path).expect("open fixture");
    let folders = pst.folders().expect("traverse folders");

    // Find the first folder with messages and read one.
    for folder in &folders {
        if folder.message_nids.is_empty() {
            continue;
        }

        let nid = folder.message_nids[0];
        let msg = pst
            .read_message_properties(nid)
            .expect("read message properties");

        // At minimum, a real message should have *some* metadata or body.
        // We do not assert specific values because fixtures vary.
        let has_any_property = msg.subject.is_some()
            || msg.sender_email.is_some()
            || msg.submit_time.is_some()
            || msg.body_preview.is_some();

        assert!(
            has_any_property,
            "Message {} in folder '{}' has no extractable properties",
            nid.0, folder.path
        );

        eprintln!(
            "Message {}: subject={:?}, sender={:?}, size={:?}, attachments={:?}",
            nid.0, msg.subject, msg.sender_email, msg.message_size, msg.has_attachments
        );

        // Only test the first message with properties.
        break;
    }
}

// ---------------------------------------------------------------------------
// Negative cases
// ---------------------------------------------------------------------------

#[test]
fn test_ansi_pst_rejected() {
    // Create a minimal file with the wrong version to ensure ANSI rejection.
    let tmp = std::env::temp_dir().join("pst_reader_test_ansi.pst");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).expect("create temp file");
        // Magic !BDN
        f.write_all(b"!BDN").unwrap();
        // Pad to where wVersion would be (offset 10 for Unicode header)
        let padding = vec![0u8; 6];
        f.write_all(&padding).unwrap();
        // Write a low version number (ANSI = 14/15)
        f.write_all(&14u16.to_le_bytes()).unwrap();
    }

    let result = PstFile::open(&tmp);
    assert!(result.is_err(), "Expected ANSI PST to be rejected");

    let _ = std::fs::remove_file(&tmp);
}
