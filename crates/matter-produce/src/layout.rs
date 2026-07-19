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
///
/// - **Default path** (`output_dir` unset): under `exports/productions/<stamp>/`.
///   If that folder already has production content, a unique timestamp suffix is
///   appended so a prior complete volume is never silently overwritten.
/// - **Explicit `output_dir`**: must not exist as a non-empty directory (any entry).
///   Incomplete resume of the *same* job reuses `cursor.output_root` and never
///   calls this function.
pub fn resolve_output_root(matter: &Matter, params: &ProduceParams) -> Result<Utf8PathBuf> {
    if let Some(dir) = params
        .output_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let root = Utf8PathBuf::from(dir);
        if path_is_nonempty(&root) {
            return Err(crate::error::ProduceError::Other(format!(
                "output_dir '{}' is non-empty; refuse to overwrite. \
                 Choose an empty directory or omit output_dir for a unique \
                 exports/productions path",
                root
            )));
        }
        return Ok(root);
    }
    let stamp = production_stamp(params);
    let base = matter
        .root()
        .join(EXPORTS_DIR)
        .join(PRODUCTIONS_DIR)
        .join(&stamp);
    if volume_has_production_content(&base) {
        // Unique suffix so named re-runs never clobber a prior complete volume.
        let unique = format!("{stamp}_{}", Utc::now().timestamp_millis());
        return Ok(matter
            .root()
            .join(EXPORTS_DIR)
            .join(PRODUCTIONS_DIR)
            .join(unique));
    }
    Ok(base)
}

/// Whether `root` already looks like a production volume with content that
/// must not be silently overwritten (load files, natives, text, or mid-flight JSONL).
pub fn volume_has_production_content(root: &Utf8Path) -> bool {
    if !root.as_std_path().exists() {
        return false;
    }
    let data = root.join(DATA_DIR);
    if data.join("load.dat").as_std_path().exists() {
        return true;
    }
    if data.join("load.csv").as_std_path().exists() {
        return true;
    }
    if data.join("rows.jsonl").as_std_path().exists() {
        return true;
    }
    if dir_has_any_file(&root.join(NATIVES_DIR)) {
        return true;
    }
    if dir_has_any_file(&root.join(TEXT_DIR)) {
        return true;
    }
    false
}

/// True when `path` exists and is a non-empty directory, or is an existing file.
fn path_is_nonempty(path: &Utf8Path) -> bool {
    let std = path.as_std_path();
    if !std.exists() {
        return false;
    }
    if std.is_file() {
        return true;
    }
    if !std.is_dir() {
        return true;
    }
    fs::read_dir(std)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

fn dir_has_any_file(dir: &Utf8Path) -> bool {
    let std = dir.as_std_path();
    if !std.is_dir() {
        return false;
    }
    fs::read_dir(std)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
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
