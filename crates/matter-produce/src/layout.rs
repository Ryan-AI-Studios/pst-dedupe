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
pub const IMAGES_DIR: &str = "IMAGES";
pub const IMAGE_OPT_NAME: &str = "IMAGE.opt";

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
    /// Folder segment for natives (default `NATIVES`).
    pub natives_name: String,
    /// Folder segment for text (default `TEXT`).
    pub text_name: String,
    /// Folder segment for data (default `DATA`).
    pub data_name: String,
    /// Image folder when `include_images` (not created for DAT-only).
    pub images: Option<Utf8PathBuf>,
    pub images_name: Option<String>,
    pub opt_path: Option<Utf8PathBuf>,
}

impl VolumeLayout {
    /// Build paths under `root` and create directories (default folder names).
    pub fn create(root: &Utf8Path) -> Result<Self> {
        Self::create_with_names(root, DATA_DIR, NATIVES_DIR, TEXT_DIR)
    }

    /// Build paths under `root` with profile-configured folder names.
    pub fn create_with_names(
        root: &Utf8Path,
        data_name: &str,
        natives_name: &str,
        text_name: &str,
    ) -> Result<Self> {
        Self::create_with_layout(root, data_name, natives_name, text_name, None)
    }

    /// Like [`create_with_names`], optionally creating the images folder.
    ///
    /// DAT-only callers must pass `images_name = None` so `IMAGES/` is not mkdir'd.
    pub fn create_with_layout(
        root: &Utf8Path,
        data_name: &str,
        natives_name: &str,
        text_name: &str,
        images_name: Option<&str>,
    ) -> Result<Self> {
        let data_name = data_name.trim();
        let natives_name = natives_name.trim();
        let text_name = text_name.trim();
        let data = root.join(data_name);
        let natives = root.join(natives_name);
        let text = root.join(text_name);
        fs::create_dir_all(data.as_std_path())?;
        fs::create_dir_all(natives.as_std_path())?;
        fs::create_dir_all(text.as_std_path())?;
        let (images, images_name_owned, opt_path) =
            if let Some(name) = images_name.map(str::trim).filter(|s| !s.is_empty()) {
                let name = opt_safe_folder_segment(name);
                let images = root.join(&name);
                fs::create_dir_all(images.as_std_path())?;
                (Some(images), Some(name), Some(root.join(IMAGE_OPT_NAME)))
            } else {
                (None, None, None)
            };
        Ok(Self {
            load_dat: data.join("load.dat"),
            load_csv: data.join("load.csv"),
            readme: root.join("README.txt"),
            root: root.to_path_buf(),
            data,
            natives,
            text,
            natives_name: natives_name.to_string(),
            text_name: text_name.to_string(),
            data_name: data_name.to_string(),
            images,
            images_name: images_name_owned,
            opt_path,
        })
    }

    /// Windows-style relative path for load file (e.g. `NATIVES\PROD000001.eml`).
    pub fn native_relpath(&self, control: &str, ext: &str) -> String {
        let ext = ext.trim_start_matches('.');
        if ext.is_empty() {
            format!("{}\\{control}", self.natives_name)
        } else {
            format!("{}\\{control}.{ext}", self.natives_name)
        }
    }

    /// Windows-style relative text path.
    pub fn text_relpath(&self, control: &str) -> String {
        format!("{}\\{control}.txt", self.text_name)
    }

    /// Static helpers used when layout folder names are defaults.
    pub fn native_relpath_default(control: &str, ext: &str) -> String {
        let ext = ext.trim_start_matches('.');
        if ext.is_empty() {
            format!("{NATIVES_DIR}\\{control}")
        } else {
            format!("{NATIVES_DIR}\\{control}.{ext}")
        }
    }

    /// Static text relpath with default `TEXT` folder.
    pub fn text_relpath_default(control: &str) -> String {
        format!("{TEXT_DIR}\\{control}.txt")
    }

    /// Windows-style image relpath `IMAGES\001\PROD000001.TIF`.
    pub fn image_relpath(&self, folder_idx: u32, bates: &str) -> String {
        let name = self.images_name.as_deref().unwrap_or(IMAGES_DIR);
        format!(
            "{}\\{:03}\\{}.TIF",
            name,
            folder_idx.max(1),
            sanitize_filename_part(bates)
        )
    }

    /// Absolute directory for image folder shard `NNN`.
    pub fn image_folder_dir(&self, folder_idx: u32) -> Option<Utf8PathBuf> {
        self.images
            .as_ref()
            .map(|root| root.join(format!("{:03}", folder_idx.max(1))))
    }
}

/// Count `.TIF`/`.tif` files in a folder (0 if missing).
pub fn count_tif_files(dir: &Utf8Path) -> u32 {
    count_tif_files_excluding(dir, &[])
}

/// Like [`count_tif_files`], ignoring in-flight / rematerialized Bates names
/// so an orphan page of the current document cannot bump the folder.
pub fn count_tif_files_excluding(dir: &Utf8Path, exclude_names: &[String]) -> u32 {
    let std = dir.as_std_path();
    if !std.is_dir() {
        return 0;
    }
    fs::read_dir(std)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| {
                    let path = e.path();
                    let ext_ok = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| {
                            ext.eq_ignore_ascii_case("tif") || ext.eq_ignore_ascii_case("tiff")
                        })
                        .unwrap_or(false);
                    if !ext_ok {
                        return false;
                    }
                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default();
                    !exclude_names.iter().any(|ex| ex.eq_ignore_ascii_case(name))
                })
                .count() as u32
        })
        .unwrap_or(0)
}

/// First folder index (1-based) that can hold `page_count` more files.
///
/// Never splits a document: if the current folder cannot fit all pages, the
/// next folder is used. `page_count > cap` is an error for the caller.
pub fn choose_image_folder(
    layout: &VolumeLayout,
    page_count: u32,
    cap: u32,
) -> crate::error::Result<u32> {
    choose_image_folder_excluding(layout, page_count, cap, &[])
}

/// Like [`choose_image_folder`], ignoring TIFF names already reserved for
/// this document (resume rematerialize / crash orphans).
pub fn choose_image_folder_excluding(
    layout: &VolumeLayout,
    page_count: u32,
    cap: u32,
    exclude_names: &[String],
) -> crate::error::Result<u32> {
    if page_count == 0 {
        return Ok(1);
    }
    if page_count > cap {
        return Err(crate::error::ProduceError::Other(format!(
            "image page_count {page_count} exceeds image_folder_cap {cap}; refuse to split document"
        )));
    }
    let mut folder = 1u32;
    loop {
        let Some(dir) = layout.image_folder_dir(folder) else {
            return Err(crate::error::ProduceError::Other(
                "image folder not configured".into(),
            ));
        };
        let n = count_tif_files_excluding(&dir, exclude_names);
        if n.saturating_add(page_count) <= cap {
            return Ok(folder);
        }
        folder = folder.saturating_add(1);
        if folder > 100_000 {
            return Err(crate::error::ProduceError::Other(
                "image folder index overflow".into(),
            ));
        }
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
    resolve_output_root_with_layout(matter, params, DATA_DIR, NATIVES_DIR, TEXT_DIR)
}

/// Resolve output root using profile layout folder names for collision detection.
pub fn resolve_output_root_with_layout(
    matter: &Matter,
    params: &ProduceParams,
    data_dir: &str,
    natives_dir: &str,
    text_dir: &str,
) -> Result<Utf8PathBuf> {
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
    if volume_has_production_content_with_layout(&base, data_dir, natives_dir, text_dir) {
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
///
/// Uses default folder names (`DATA` / `NATIVES` / `TEXT`). Prefer
/// [`volume_has_production_content_with_layout`] when the profile may rename folders.
pub fn volume_has_production_content(root: &Utf8Path) -> bool {
    volume_has_production_content_with_layout(root, DATA_DIR, NATIVES_DIR, TEXT_DIR)
}

/// Like [`volume_has_production_content`] but for profile-custom layout folder names.
///
/// Also treats **any non-empty root** as occupied so a prior volume that used
/// different folder names cannot be silently overwritten when the stamp collides.
pub fn volume_has_production_content_with_layout(
    root: &Utf8Path,
    data_dir: &str,
    natives_dir: &str,
    text_dir: &str,
) -> bool {
    if !root.as_std_path().exists() {
        return false;
    }
    // Conservative: any entry under the stamp root means do not clobber.
    if path_is_nonempty(root) {
        // Still allow truly empty dirs created as placeholders.
        // path_is_nonempty already false for empty dirs.
    }
    let data = root.join(data_dir);
    if data.join("load.dat").as_std_path().exists() {
        return true;
    }
    if data.join("load.csv").as_std_path().exists() {
        return true;
    }
    if data.join("rows.jsonl").as_std_path().exists() {
        return true;
    }
    if dir_has_any_file(&root.join(natives_dir)) {
        return true;
    }
    if dir_has_any_file(&root.join(text_dir)) {
        return true;
    }
    if root.join(IMAGE_OPT_NAME).as_std_path().exists() {
        return true;
    }
    if dir_has_any_file(&root.join(IMAGES_DIR)) {
        return true;
    }
    // Also check default folder names (prior volume may have used defaults while
    // this run uses a custom layout, or vice versa).
    if (data_dir != DATA_DIR || natives_dir != NATIVES_DIR || text_dir != TEXT_DIR)
        && volume_has_production_content_with_layout(root, DATA_DIR, NATIVES_DIR, TEXT_DIR)
    {
        return true;
    }
    // Any other non-empty content under root (e.g. LOAD/ORIGINALS from a custom pack).
    path_is_nonempty(root)
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

/// Folder segment safe for OPT (ASCII, no comma/whitespace).
fn opt_safe_folder_segment(name: &str) -> String {
    let s: String = name
        .chars()
        .filter_map(|c| {
            if c == ',' || c.is_whitespace() || c.is_ascii_control() {
                None
            } else if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                Some(c)
            } else {
                Some('_')
            }
        })
        .collect();
    if s.is_empty() {
        IMAGES_DIR.into()
    } else {
        s
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

/// OPT volume token: strip comma + whitespace, ASCII-only, then [`sanitize_filename_part`].
pub fn opt_volume_token(name: &str) -> String {
    let stripped: String = name
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = sanitize_filename_part(&stripped);
    if s.is_empty() {
        "VOL".into()
    } else {
        s
    }
}

/// Write volume README with format + privacy notes.
pub fn write_readme(
    path: &Utf8Path,
    production_name: &str,
    expand_family: bool,
    counts_line: &str,
) -> Result<()> {
    write_readme_ex(path, production_name, expand_family, counts_line, false)
}

/// Write volume README; mention IMAGES + IMAGE.opt when `include_images`.
pub fn write_readme_ex(
    path: &Utf8Path,
    production_name: &str,
    expand_family: bool,
    counts_line: &str,
    include_images: bool,
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
    if include_images {
        writeln!(f, "  IMAGES/         Single-page TIFF G4 (CCITT Group 4)")?;
        writeln!(
            f,
            "  IMAGE.opt       Opticon load file (seven fields, CRLF)"
        )?;
    }
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

#[cfg(test)]
mod folder_cap_tests {
    use super::*;
    use std::fs;

    #[test]
    fn choose_folder_ignores_orphan_bates_of_current_document() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = camino::Utf8Path::from_path(tmp.path()).expect("utf8");
        let layout =
            VolumeLayout::create_with_layout(root, "DATA", "NATIVES", "TEXT", Some("IMAGES"))
                .expect("layout");
        let dir = layout.image_folder_dir(1).expect("dir");
        fs::create_dir_all(dir.as_std_path()).expect("mkdir");
        for i in 0..497u32 {
            fs::write(dir.join(format!("FILL{i:06}.TIF")).as_std_path(), b"x").expect("fill");
        }
        fs::write(dir.join("PROD000001.TIF").as_std_path(), b"orphan").expect("orphan");
        let exclude = vec![
            "PROD000001.TIF".into(),
            "PROD000002.TIF".into(),
            "PROD000003.TIF".into(),
        ];
        let folder = choose_image_folder_excluding(&layout, 3, 500, &exclude).expect("choose");
        assert_eq!(
            folder, 1,
            "orphan of the in-flight document must not force folder 002"
        );
        let without = choose_image_folder(&layout, 3, 500).expect("no exclude");
        assert_eq!(without, 2, "counting the orphan should overflow folder 001");
    }
}
