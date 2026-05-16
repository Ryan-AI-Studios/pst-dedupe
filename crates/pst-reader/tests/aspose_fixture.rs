use std::path::PathBuf;

#[test]
fn test_aspose_sample_opens() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
    if !fixture.exists() {
        return;
    }
    let mut pst = pst_reader::PstFile::open(&fixture).unwrap();
    let folders = pst.folders().unwrap();
    println!("Outlook.pst: {} folders", folders.len());
    let total_msgs: u32 = folders.iter().map(|f| f.message_count).sum();
    println!("Total messages: {}", total_msgs);
}

#[test]
fn test_aspose_sub_opens() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_sub.pst");
    if !fixture.exists() {
        return;
    }
    let mut pst = pst_reader::PstFile::open(&fixture).unwrap();
    let folders = pst.folders().unwrap();
    println!("Sub.pst: {} folders", folders.len());
    let total_msgs: u32 = folders.iter().map(|f| f.message_count).sum();
    println!("Total messages: {}", total_msgs);
}

#[test]
fn test_aspose_personalstorage_opens() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_personalstorage.pst");
    if !fixture.exists() {
        return;
    }
    let mut pst = pst_reader::PstFile::open(&fixture).unwrap();
    let folders = pst.folders().unwrap();
    println!("PersonalStorage.pst: {} folders", folders.len());
    let total_msgs: u32 = folders.iter().map(|f| f.message_count).sum();
    println!("Total messages: {}", total_msgs);
}
