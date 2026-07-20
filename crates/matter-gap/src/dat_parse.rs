//! Concordance DAT / simple CSV parser for opposing expected docs.

use std::fs;
use std::io::{BufRead, Cursor, Read};
use std::path::Path;

use matter_core::GapExpectedDocInput;
use matter_produce::{DAT_NEWLINE, DAT_QUALIFIER, DAT_SEPARATOR, UTF8_BOM};

use crate::column_map::{DatColumnMap, MappedField};
use crate::error::{GapError, Result};
use crate::params::{DEFAULT_MAX_DAT_BYTES, DEFAULT_MAX_DAT_ROWS};

/// Parsed opposing load file.
#[derive(Debug, Clone)]
pub struct ParsedDat {
    pub headers: Vec<String>,
    pub rows: Vec<GapExpectedDocInput>,
    pub format: DatFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatFormat {
    Concordance,
    Csv,
}

/// Caps for untrusted DAT input.
#[derive(Debug, Clone, Copy)]
pub struct DatCaps {
    pub max_bytes: u64,
    pub max_rows: u64,
}

impl Default for DatCaps {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_DAT_BYTES,
            max_rows: DEFAULT_MAX_DAT_ROWS,
        }
    }
}

/// Check file size against cap before reading fully.
pub fn check_file_size(path: &Path, max_bytes: u64) -> Result<u64> {
    let meta = fs::metadata(path)?;
    let size = meta.len();
    if size > max_bytes {
        return Err(GapError::DatTooLarge {
            size,
            cap: max_bytes,
        });
    }
    Ok(size)
}

/// Enforce max size on in-memory bytes (unit tests / fixtures).
pub fn check_bytes_size(len: u64, max_bytes: u64) -> Result<()> {
    if len > max_bytes {
        return Err(GapError::DatTooLarge {
            size: len,
            cap: max_bytes,
        });
    }
    Ok(())
}

/// Parse a DAT or CSV file at `path` using `column_map`.
pub fn parse_dat_file(path: &Path, column_map: &DatColumnMap, caps: DatCaps) -> Result<ParsedDat> {
    check_file_size(path, caps.max_bytes)?;
    let bytes = fs::read(path)?;
    parse_dat_bytes(&bytes, column_map, caps)
}

/// Parse DAT/CSV from bytes.
pub fn parse_dat_bytes(
    bytes: &[u8],
    column_map: &DatColumnMap,
    caps: DatCaps,
) -> Result<ParsedDat> {
    check_bytes_size(bytes.len() as u64, caps.max_bytes)?;
    let stripped = strip_utf8_bom(bytes);
    if looks_like_concordance(stripped) {
        parse_concordance(stripped, column_map, caps)
    } else {
        parse_simple_csv(stripped, column_map, caps)
    }
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&UTF8_BOM) {
        &bytes[UTF8_BOM.len()..]
    } else {
        bytes
    }
}

fn looks_like_concordance(bytes: &[u8]) -> bool {
    // Presence of thorn qualifier near start strongly indicates Concordance DAT.
    let sample = &bytes[..bytes.len().min(4096)];
    sample.contains(&(DAT_QUALIFIER as u8)) || sample.contains(&0xC3) /* utf8 of þ often */
        || {
            // Check for UTF-8 encoding of þ (U+00FE = 0xC3 0xBE)
            sample.windows(2).any(|w| w == [0xC3, 0xBE])
        }
}

/// De-qualify a Concordance field: strip surrounding þ, replace ® with newlines.
pub fn decode_dat_field(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.starts_with(DAT_QUALIFIER) {
        s = s[DAT_QUALIFIER.len_utf8()..].to_string();
    }
    if s.ends_with(DAT_QUALIFIER) {
        s = s[..s.len() - DAT_QUALIFIER.len_utf8()].to_string();
    }
    s.replace(DAT_NEWLINE, "\n")
}

fn parse_concordance(bytes: &[u8], column_map: &DatColumnMap, caps: DatCaps) -> Result<ParsedDat> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| GapError::Other(format!("DAT is not valid UTF-8: {e}")))?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines
        .next()
        .ok_or_else(|| GapError::Other("DAT is empty".into()))?;
    let headers = split_dat_line(header_line);
    let field_idx = column_map.resolve_indices(&headers)?;

    let mut rows = Vec::new();
    for line in lines {
        if rows.len() as u64 >= caps.max_rows {
            return Err(GapError::DatTooManyRows {
                count: rows.len() as u64 + 1,
                cap: caps.max_rows,
            });
        }
        let fields = split_dat_line(line);
        rows.push(row_from_fields(&fields, &field_idx));
    }

    Ok(ParsedDat {
        headers,
        rows,
        format: DatFormat::Concordance,
    })
}

fn split_dat_line(line: &str) -> Vec<String> {
    line.split(DAT_SEPARATOR).map(decode_dat_field).collect()
}

fn parse_simple_csv(bytes: &[u8], column_map: &DatColumnMap, caps: DatCaps) -> Result<ParsedDat> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(Cursor::new(bytes));
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| GapError::Other(format!("CSV header: {e}")))?
        .iter()
        .map(|h| h.to_string())
        .collect();
    let field_idx = column_map.resolve_indices(&headers)?;

    let mut rows = Vec::new();
    for rec in reader.records() {
        if rows.len() as u64 >= caps.max_rows {
            return Err(GapError::DatTooManyRows {
                count: rows.len() as u64 + 1,
                cap: caps.max_rows,
            });
        }
        let rec = rec?;
        let fields: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
        rows.push(row_from_fields(&fields, &field_idx));
    }

    Ok(ParsedDat {
        headers,
        rows,
        format: DatFormat::Csv,
    })
}

fn row_from_fields(
    fields: &[String],
    field_idx: &std::collections::HashMap<MappedField, usize>,
) -> GapExpectedDocInput {
    let get = |f: MappedField| -> Option<String> {
        field_idx
            .get(&f)
            .and_then(|&i| fields.get(i))
            .and_then(|s| {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            })
    };
    GapExpectedDocInput {
        control_number: get(MappedField::ControlNumber),
        sha256: get(MappedField::Sha256).map(|s| s.to_lowercase()),
        message_id: get(MappedField::MessageId),
        item_id: get(MappedField::ItemId),
        logical_hash: get(MappedField::LogicalHash).map(|s| s.to_lowercase()),
        custodian: get(MappedField::Custodian),
        file_name: get(MappedField::FileName),
        file_category: get(MappedField::FileCategory),
        mime_type: get(MappedField::MimeType),
        file_ext: get(MappedField::FileExt),
        date_sent: get(MappedField::DateSent),
        date_received: get(MappedField::DateReceived),
        date_created: get(MappedField::DateCreated),
    }
}

/// Streaming-friendly size guard used by tests with tiny caps.
pub fn enforce_caps(size: u64, rows: u64, caps: DatCaps) -> Result<()> {
    check_bytes_size(size, caps.max_bytes)?;
    if rows > caps.max_rows {
        return Err(GapError::DatTooManyRows {
            count: rows,
            cap: caps.max_rows,
        });
    }
    Ok(())
}

// Keep BufRead in scope for potential streaming; silence unused via helper.
#[allow(dead_code)]
fn _buf_read_api(r: &mut dyn BufRead) -> Result<usize> {
    let mut buf = String::new();
    Ok(r.read_line(&mut buf)?)
}

#[allow(dead_code)]
fn _read_api(r: &mut dyn Read) -> Result<u64> {
    let mut sink = std::io::sink();
    Ok(std::io::copy(r, &mut sink)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_produce::{encode_dat_field, DAT_FIELDS};

    #[test]
    fn bom_and_concordance_roundtrip_shape() {
        // Build a minimal Concordance line like produce.
        let mut bytes = Vec::from(UTF8_BOM);
        // header: CONTROL_NUMBER ¶ SHA256 ¶ ITEM_ID ¶ CUSTODIAN ¶ FILE_NAME
        let headers = [
            "CONTROL_NUMBER",
            "SHA256",
            "ITEM_ID",
            "CUSTODIAN",
            "FILE_NAME",
        ];
        let mut header_line = String::new();
        for (i, h) in headers.iter().enumerate() {
            if i > 0 {
                header_line.push(DAT_SEPARATOR);
            }
            header_line.push(DAT_QUALIFIER);
            header_line.push_str(h);
            header_line.push(DAT_QUALIFIER);
        }
        bytes.extend(header_line.as_bytes());
        bytes.push(b'\n');
        let vals = ["PROD0001", "aabbcc", "item_1", "Alice", "doc.pdf"];
        let mut data = String::new();
        for (i, v) in vals.iter().enumerate() {
            if i > 0 {
                data.push(DAT_SEPARATOR);
            }
            data.push(DAT_QUALIFIER);
            data.push_str(&encode_dat_field(v));
            data.push(DAT_QUALIFIER);
        }
        bytes.extend(data.as_bytes());
        bytes.push(b'\n');

        let map = DatColumnMap::default_produce_v1();
        let parsed = parse_dat_bytes(&bytes, &map, DatCaps::default()).unwrap();
        assert_eq!(parsed.format, DatFormat::Concordance);
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].control_number.as_deref(), Some("PROD0001"));
    }

    #[test]
    fn oversized_fails_closed() {
        let err = check_bytes_size(100, 50).unwrap_err();
        assert!(matches!(err, GapError::DatTooLarge { .. }));
    }

    /// max_rows: 1 with a 2-row CSV must fail closed as DatTooManyRows.
    #[test]
    fn max_rows_cap_two_row_csv() {
        let bytes = b"CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME\n\
                      C1,aa,i1,Alice,a.pdf\n\
                      C2,bb,i2,Bob,b.pdf\n";
        let caps = DatCaps {
            max_bytes: 10_000,
            max_rows: 1,
        };
        let err = parse_dat_bytes(bytes, &DatColumnMap::default_produce_v1(), caps).unwrap_err();
        assert!(
            matches!(err, GapError::DatTooManyRows { cap: 1, .. }),
            "expected DatTooManyRows, got {err:?}"
        );
    }

    #[test]
    fn dat_fields_constant_available() {
        assert!(DAT_FIELDS.contains(&"CONTROL_NUMBER"));
    }
}
