//! Concordance-style DAT writer (UTF-8 BOM, þ/¶, ® newlines) + CSV twin.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::error::{ProduceError, Result};

/// UTF-8 BOM bytes (required on load.dat).
pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Field qualifier: thorn `þ` (U+00FE).
pub const DAT_QUALIFIER: char = '\u{00FE}';

/// Field separator: pilcrow `¶` (U+00B6).
pub const DAT_SEPARATOR: char = '\u{00B6}';

/// In-field newline replacement: registered mark `®` (U+00AE).
pub const DAT_NEWLINE: char = '\u{00AE}';

/// Stable field order for `matter_produce_v1` load file.
pub const DAT_FIELDS: &[&str] = &[
    "BEGBATES",
    "ENDBATES",
    "CONTROL_NUMBER",
    "ITEM_ID",
    "PARENT_ITEM_ID",
    "FAMILY_ID",
    "CUSTODIAN",
    "FILE_NAME",
    "FILE_EXT",
    "FILE_CATEGORY",
    "MIME_TYPE",
    "FILE_SIZE",
    "SHA256",
    "DATE_SENT",
    "DATE_RECEIVED",
    "DATE_CREATED",
    "FROM",
    "TO",
    "CC",
    "BCC",
    "SUBJECT",
    "NATIVE_PATH",
    "TEXT_PATH",
    "HAS_REDACTED_TEXT",
    "WITHHELD",
    "PROD_STATUS",
];

/// One load-file row (produced document only — withheld never appear).
#[derive(Debug, Clone, Default)]
pub struct LoadRow {
    pub control_number: String,
    pub item_id: String,
    pub parent_item_id: String,
    pub family_id: String,
    pub custodian: String,
    pub file_name: String,
    pub file_ext: String,
    pub file_category: String,
    pub mime_type: String,
    pub file_size: String,
    pub sha256: String,
    pub date_sent: String,
    pub date_received: String,
    pub date_created: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub native_path: String,
    pub text_path: String,
    pub has_redacted_text: String,
    pub withheld: String,
    pub prod_status: String,
    /// Recovery-only integrity hash for TEXT (not a DAT column).
    pub text_sha256: String,
}

impl LoadRow {
    /// Ordered field values matching [`DAT_FIELDS`].
    pub fn field_values(&self) -> [&str; 26] {
        [
            self.control_number.as_str(), // BEGBATES
            self.control_number.as_str(), // ENDBATES
            self.control_number.as_str(), // CONTROL_NUMBER
            self.item_id.as_str(),
            self.parent_item_id.as_str(),
            self.family_id.as_str(),
            self.custodian.as_str(),
            self.file_name.as_str(),
            self.file_ext.as_str(),
            self.file_category.as_str(),
            self.mime_type.as_str(),
            self.file_size.as_str(),
            self.sha256.as_str(),
            self.date_sent.as_str(),
            self.date_received.as_str(),
            self.date_created.as_str(),
            self.from.as_str(),
            self.to.as_str(),
            self.cc.as_str(),
            self.bcc.as_str(),
            self.subject.as_str(),
            self.native_path.as_str(),
            self.text_path.as_str(),
            self.has_redacted_text.as_str(),
            self.withheld.as_str(),
            self.prod_status.as_str(),
        ]
    }
}

/// Map CR / LF / CRLF sequences inside a field to Concordance `®`.
pub fn encode_dat_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                out.push(DAT_NEWLINE);
            }
            '\n' => out.push(DAT_NEWLINE),
            _ => out.push(c),
        }
    }
    out
}

/// Format a datetime field as UTC ISO `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Accepts only RFC3339 values with an explicit offset or `Z`.
/// Zone-less / unparsable inputs → empty string (never invent a timezone).
pub fn format_utc_datetime(raw: Option<&str>) -> String {
    let Some(s) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return String::new();
    };
    // RFC3339 requires an explicit offset or Z — do not append Z to zone-less strings.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true);
    }
    String::new()
}

/// Write Concordance DAT with UTF-8 BOM + header + rows.
pub fn write_load_dat(path: &Path, rows: &[LoadRow]) -> Result<()> {
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    w.write_all(&UTF8_BOM)?;
    write_dat_line(&mut w, DAT_FIELDS.iter().copied())?;
    for row in rows {
        write_dat_line(&mut w, row.field_values())?;
    }
    w.flush()?;
    Ok(())
}

fn write_dat_line<'a, I>(w: &mut impl Write, fields: I) -> Result<()>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut first = true;
    for field in fields {
        if !first {
            write!(w, "{DAT_SEPARATOR}")?;
        }
        first = false;
        let encoded = encode_dat_field(field);
        write!(w, "{DAT_QUALIFIER}{encoded}{DAT_QUALIFIER}")?;
    }
    writeln!(w)?;
    Ok(())
}

/// Write optional CSV twin (UTF-8 BOM, RFC4180).
pub fn write_load_csv(path: &Path, rows: &[LoadRow]) -> Result<()> {
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    w.write_all(&UTF8_BOM)?;
    let mut writer = csv::Writer::from_writer(w);
    writer
        .write_record(DAT_FIELDS)
        .map_err(|e| ProduceError::Other(format!("csv header: {e}")))?;
    for row in rows {
        writer
            .write_record(row.field_values())
            .map_err(|e| ProduceError::Other(format!("csv row: {e}")))?;
    }
    writer
        .flush()
        .map_err(|e| ProduceError::Other(format!("csv flush: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_becomes_registered_mark() {
        assert_eq!(encode_dat_field("a\nb"), format!("a{DAT_NEWLINE}b"));
        assert_eq!(encode_dat_field("a\r\nb"), format!("a{DAT_NEWLINE}b"));
        assert_eq!(encode_dat_field("a\rb"), format!("a{DAT_NEWLINE}b"));
    }

    #[test]
    fn utc_datetime_formats_with_z() {
        let s = format_utc_datetime(Some("2026-07-19T12:00:00+00:00"));
        assert!(s.ends_with('Z'), "got {s}");
        assert!(s.starts_with("2026-07-19T12:00:00"));
    }

    #[test]
    fn offset_converted_to_utc() {
        let s = format_utc_datetime(Some("2026-07-19T15:00:00+03:00"));
        assert_eq!(s, "2026-07-19T12:00:00Z");
    }

    #[test]
    fn empty_datetime_stays_empty() {
        assert_eq!(format_utc_datetime(None), "");
        assert_eq!(format_utc_datetime(Some("")), "");
    }

    #[test]
    fn zoneless_datetime_stays_empty() {
        // Must not invent Z / assume UTC for floating local times.
        assert_eq!(format_utc_datetime(Some("2026-07-19T12:00:00")), "");
        assert_eq!(format_utc_datetime(Some("2026-07-19 12:00:00")), "");
        assert_eq!(format_utc_datetime(Some("2026-07-19T12:00:00 UTC")), "");
    }
}
