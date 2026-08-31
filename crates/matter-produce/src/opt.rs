//! Opticon `IMAGE.opt` writer (seven fields, CRLF, no qualifier).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::error::Result;
use crate::layout::opt_volume_token;

/// One OPT line (one image page).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptRow {
    pub alias: String,
    pub volume: String,
    pub relative_path: String,
    pub page_index: u32,
    pub page_count: u32,
}

impl OptRow {
    pub fn encode_line(&self) -> String {
        encode_opt_line(
            &self.alias,
            &self.volume,
            &self.relative_path,
            self.page_index,
            self.page_count,
        )
    }
}

/// Encode one OPT line. Field 4 `Y` iff first page; field 7 page count iff first page.
///
/// Alias, volume, and relative path are constrained to ASCII (no comma, no
/// control bytes). Non-ASCII code points become `_`.
pub fn encode_opt_line(
    alias: &str,
    volume: &str,
    relative_path: &str,
    page_index: u32,
    page_count: u32,
) -> String {
    let alias = opt_ascii_field(alias, false);
    let vol = opt_volume_token(volume);
    let relative_path = opt_ascii_field(relative_path, true);
    let (doc_break, count) = if page_index == 0 {
        ("Y", page_count.to_string())
    } else {
        ("", String::new())
    };
    // ALIAS, VOLUME, RELATIVE_PATH, DOCUMENT_BREAK, FOLDER_BREAK, BOX_BREAK, PAGE_COUNT
    format!("{alias},{vol},{relative_path},{doc_break},,,{count}")
}

/// ASCII-constrain an OPT field. `allow_backslash` keeps `\` / `/` as `\`.
fn opt_ascii_field(s: &str, allow_backslash: bool) -> String {
    let out: String = s
        .chars()
        .filter_map(|c| {
            if c == ',' || c.is_ascii_control() || c.is_whitespace() {
                return None;
            }
            if c == '\\' || c == '/' {
                return if allow_backslash { Some('\\') } else { None };
            }
            if c.is_ascii() {
                Some(c)
            } else {
                Some('_')
            }
        })
        .collect();
    if out.is_empty() {
        "_".into()
    } else {
        out
    }
}

/// Write `IMAGE.opt` (ASCII, CRLF, no BOM, no text qualifier).
pub fn write_image_opt(path: &Path, lines: &[String]) -> Result<()> {
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    for line in lines {
        if !line.is_ascii() {
            return Err(crate::error::ProduceError::Other(
                "OPT line is not ASCII".into(),
            ));
        }
        w.write_all(line.as_bytes())?;
        w.write_all(b"\r\n")?;
    }
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_and_interior_lines() {
        let a = encode_opt_line("PROD000001", "VOL001", "IMAGES\\001\\PROD000001.TIF", 0, 2);
        let b = encode_opt_line("PROD000002", "VOL001", "IMAGES\\001\\PROD000002.TIF", 1, 2);
        assert_eq!(a, "PROD000001,VOL001,IMAGES\\001\\PROD000001.TIF,Y,,,2");
        assert_eq!(b, "PROD000002,VOL001,IMAGES\\001\\PROD000002.TIF,,,,");
    }

    #[test]
    fn volume_strips_comma_and_space() {
        let line = encode_opt_line(
            "PROD000001",
            "VOL, 001",
            "IMAGES\\001\\PROD000001.TIF",
            0,
            1,
        );
        assert_eq!(line, "PROD000001,VOL001,IMAGES\\001\\PROD000001.TIF,Y,,,1");
        assert!(!line.contains(' '));
    }

    #[test]
    fn unicode_volume_and_name_become_ascii() {
        let line = encode_opt_line(
            "PROD000001",
            "VOL日本語",
            "IMAGES\\001\\PROD000001.TIF",
            0,
            1,
        );
        assert!(line.is_ascii(), "{line}");
        assert!(!line.contains('日'));
        assert!(line.starts_with("PROD000001,VOL"));
        assert!(line.contains("IMAGES\\001\\PROD000001.TIF"));
    }
}
