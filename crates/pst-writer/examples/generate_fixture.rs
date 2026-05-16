use std::path::PathBuf;

fn main() {
    let eml_dir = PathBuf::from(
        "C:\\Users\\RyanB\\Desktop\\thundertest\\PROMOTIONS_20260516-0706\\PROMOTIONS",
    );
    if !eml_dir.exists() {
        eprintln!("EML directory does not exist: {}", eml_dir.display());
        std::process::exit(1);
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

    let mut emls = Vec::new();
    for path in &eml_files {
        match pst_writer::eml::EmlMessage::from_file(path) {
            Ok(eml) => emls.push(eml),
            Err(e) => eprintln!("Failed to parse {}: {}", path.display(), e),
        }
    }

    println!("Parsed {} EML files", emls.len());

    let output = PathBuf::from("fixtures/promotions_spam.pst");
    pst_writer::write_pst_from_emls(&output, &emls).expect("write PST");

    println!(
        "Wrote PST to {} ({} bytes)",
        output.display(),
        std::fs::metadata(&output).unwrap().len()
    );
}
