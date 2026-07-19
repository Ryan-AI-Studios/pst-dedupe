//! Production volume folder layout helpers.

use std::fs;
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use matter_core::{Matter, EXPORTS_DIR};

use crate::error::Result;
use crate::params::ProduceParams;

/// Subdirectory under `exports/` for production volumes.
pub const PRODUCTIONS_DIR: &str = "productions";
pub const DATA_DIR: &str = "DATA";
pub const NATIVES_DIR: &str = "NATIVES";
pub const TEXT_DIR: &str = "TEXT";

/// Resolved volume paths.
#[derive(Debug, Clone)]
pub struct VolumeLayout {
    pub root: Utf8PathBuf,
    pub data: Utf8PathBuf,
    pub natives: Utf8PathBuf,
    pub text: Utf8PathBuf,
    pub load_dat: Utf8PathBuf,
    pub load_csv: Utf8PathBuf,
    pub readme: Utf8PathBuf,
}

impl VolumeLayout {
    /// Build paths under `root` and create directories.
    pub fn create(root: &Utf8Path) -> Result<Self> {
        let data = root.join(DATA_DIR);
        let natives = root.join(NATIVES_DIR);
        let text = root.join(TEXT_DIR);
        fs::create_dir_all(data.as_std_path())?;
        fs::create_dir_all(natives.as_std_path())?;
        fs::create_dir_all(text.as_std_path())?;
        Ok(Self {
            load_dat: data.join("load.dat"),
            load_csv: data.join("load.csv"),
            readme: root.join("README.txt"),
            root: root.to_path_buf(),
            data,
            natives,
            text,
        })
    }

    /// Windows-style relative path for load file (e.g. `NATIVES\PROD000001.eml`).
    pub fn native_relpath(control: &str, ext: &str) -> String {
        let ext = ext.trim_start_matches('.');
        if ext.is_empty() {
            format!("{NATIVES_DIR}\\{control}")
        } else {
            format!("{NATIVES_DIR}\\{control}.{ext}")
        }
    }

    /// Windows-style relative text path.
    pub fn text_relpath(control: &str) -> String {
        format!("{TEXT_DIR}\\{control}.txt")
    }
}

/// Resolve default or operator-chosen output root.
pub fn resolve_output_root(matter: &Matter, params: &ProduceParams) -> Result<Utf8PathBuf> {
    if let Some(dir) = params
        .output_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Ok(Utf8PathBuf::from(dir));
    }
    let stamp = production_stamp(params);
    Ok(matter
        .root()
        .join(EXPORTS_DIR)
        .join(PRODUCTIONS_DIR)
        .join(stamp))
}

/// Folder name: sanitized production name or UTC stamp.
pub fn production_stamp(params: &ProduceParams) -> String {
    if let Some(name) = params
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return sanitize_folder_name(name);
    }
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
    format!("prod_{ts}")
}

/// Sanitize a folder segment (no path separators).
pub fn sanitize_folder_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let t = s.trim().trim_matches('.');
    if t.is_empty() {
        "production".into()
    } else {
        t.to_string()
    }
}

/// Sanitize control number + extension for Windows filenames.
pub fn sanitize_filename_part(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

/// Write volume README with format + privacy notes.
pub fn write_readme(
    path: &Utf8Path,
    production_name: &str,
    expand_family: bool,
    counts_line: &str,
) -> Result<()> {
    let mut f = fs::File::create(path.as_std_path())?;
    writeln!(f, "Dedupe production volume (matter_produce_v1)")?;
    writeln!(f, "Production: {production_name}")?;
    writeln!(f)?;
    writeln!(f, "Layout:")?;
    writeln!(
        f,
        "  DATA/load.dat   Concordance-style load file (required)"
    )?;
    writeln!(f, "  DATA/load.csv   Optional CSV twin (UTF-8 BOM)")?;
    writeln!(f, "  NATIVES/        Produced native files")?;
    writeln!(f, "  TEXT/           Extracted or redacted text (.txt)")?;
    writeln!(f)?;
    writeln!(f, "DAT format:")?;
    writeln!(f, "  Encoding: UTF-8 with BOM (EF BB BF)")?;
    writeln!(f, "  Field qualifier: þ (U+00FE)")?;
    writeln!(f, "  Field separator: ¶ (U+00B6)")?;
    writeln!(f, "  In-field newlines: ® (U+00AE)")?;
    writeln!(f, "  Datetimes: UTC only (YYYY-MM-DDTHH:MM:SSZ)")?;
    writeln!(f, "  Paths: Windows-style relative (NATIVES\\…, TEXT\\…)")?;
    writeln!(f)?;
    writeln!(f, "Privacy / packaging rules:")?;
    writeln!(f, "  - Privilege description / basis narrative: excluded")?;
    writeln!(f, "  - Review notes / highlight quotes: excluded")?;
    writeln!(f, "  - Withheld items: never written to NATIVES/TEXT/DAT")?;
    writeln!(
        f,
        "  - Redacted items: TEXT uses redacted CAS only (never original)"
    )?;
    writeln!(f)?;
    if !expand_family {
        writeln!(f, "Family expand: OFF")?;
        writeln!(
            f,
            "  WARNING: producing a child without its parent (or parent without"
        )?;
        writeln!(
            f,
            "  selected children when protocol requires whole family) is a broken"
        )?;
        writeln!(
            f,
            "  family risk. Ensure review membership is family-complete or accept"
        )?;
        writeln!(
            f,
            "  orphan risk. Full broken-family QC is owned by track 0041."
        )?;
        writeln!(f)?;
    } else {
        writeln!(f, "Family expand: ON (selection expanded)")?;
        writeln!(f)?;
    }
    writeln!(f, "Counts: {counts_line}")?;
    writeln!(
        f,
        "EML note: synthetic .eml files are export-only packaging, not original MIME identity."
    )?;
    Ok(())
}
