//! XLSX text extract via calamine with **running length** + early break.

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};

use crate::error::{Error, Result};
use crate::limits::{methods, MAX_EXTRACTED_TEXT_BYTES, MAX_SHEETS_OR_SLIDES};
use crate::text_buf::TextBuf;
use crate::ExtractedText;

/// Extract plain text from an XLSX workbook.
///
/// Builds output **incrementally** while iterating rows/cells. When
/// `output.len() >= MAX_EXTRACTED_TEXT_BYTES`, **stops immediately** — does
/// not stringify the whole workbook first.
pub fn extract_xlsx(data: &[u8]) -> Result<ExtractedText> {
    extract_xlsx_with_limit(data, MAX_EXTRACTED_TEXT_BYTES)
}

/// Same as [`extract_xlsx`] with an injectable text cap (tests).
pub fn extract_xlsx_with_limit(data: &[u8], max_text_bytes: usize) -> Result<ExtractedText> {
    let cursor = Cursor::new(data);
    let mut workbook: Xlsx<_> =
        Xlsx::new(cursor).map_err(|e| Error::parse(format!("calamine open: {e}")))?;

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(Error::EmptyText("xlsx has no sheets".into()));
    }

    let mut buf = TextBuf::with_limit(max_text_bytes);
    let mut hit_sheet_cap = false;

    for (sheets_visited, name) in sheet_names.into_iter().enumerate() {
        if buf.is_full() {
            break;
        }
        if sheets_visited >= MAX_SHEETS_OR_SLIDES {
            hit_sheet_cap = true;
            break;
        }

        if !buf.is_empty() && !buf.push_str("\n") {
            break;
        }
        // Sheet name line
        if !buf.push_str(&name) {
            break;
        }
        if !buf.push_str("\n") {
            break;
        }

        let range = match workbook.worksheet_range(&name) {
            Ok(r) => r,
            Err(e) => {
                return Err(Error::parse(format!("sheet '{name}': {e}")));
            }
        };

        // Iterate rows — check length after each cell/row; do not pre-join.
        for row in range.rows() {
            if buf.is_full() {
                break;
            }
            let mut first_cell = true;
            for cell in row {
                if buf.is_full() {
                    break;
                }
                if !first_cell && !buf.push_str("\t") {
                    break;
                }
                first_cell = false;
                let cell_text = cell_to_string(cell);
                if !cell_text.is_empty() && !buf.push_str(&cell_text) {
                    break;
                }
            }
            if buf.is_full() {
                break;
            }
            if !buf.push_str("\n") {
                break;
            }
        }
    }

    let (mut text, mut partial) = buf.into_string();
    if hit_sheet_cap {
        partial = true;
        if !text.contains(crate::limits::TRUNCATION_MARKER) {
            text.push_str(crate::limits::TRUNCATION_MARKER);
        }
    }

    // Trim trailing whitespace for empty-check only; keep content as built.
    if text.trim().is_empty() {
        return Err(Error::EmptyText("xlsx produced zero text".into()));
    }

    Ok(ExtractedText {
        text,
        method: methods::CALAMINE_XLSX_V1.into(),
        partial,
        format: crate::detect::OfficeFormat::Xlsx,
    })
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            // Prefer integer display when whole
            if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#ERR({e:?})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_to_string_basic() {
        assert_eq!(cell_to_string(&Data::String("hi".into())), "hi");
        assert_eq!(cell_to_string(&Data::Int(42)), "42");
        assert_eq!(cell_to_string(&Data::Empty), "");
    }
}
