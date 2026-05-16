//! EML file export — writes unique messages as RFC 5322 .eml files.
//!
//! Since we're reading from PST (MAPI properties), we reconstruct a minimal
//! RFC 5322 message from available properties. This won't be a perfect round-trip
//! of the original MIME, but it preserves the content needed for archival.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Minimal message data needed for EML export.
pub struct EmlMessage {
    pub message_id: Option<String>,
    pub subject: String,
    pub sender: String,
    pub recipients: String,
    pub date: Option<String>,
    pub body: String,
    /// Filename hint (derived from subject + counter).
    pub filename: String,
}

/// Export a single message as an EML file into the output directory.
///
/// Returns the path to the written file.
pub fn export_eml(
    output_dir: &Path,
    message: &EmlMessage,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = output_dir.join(&message.filename);
    let mut file = std::fs::File::create(&path)?;

    // RFC 5322 headers
    if let Some(mid) = &message.message_id {
        writeln!(file, "Message-ID: <{}>", mid)?;
    }
    writeln!(file, "Subject: {}", encode_header_value(&message.subject))?;
    writeln!(file, "From: {}", &message.sender)?;
    writeln!(file, "To: {}", &message.recipients)?;
    if let Some(date) = &message.date {
        writeln!(file, "Date: {}", date)?;
    }
    writeln!(file, "MIME-Version: 1.0")?;
    writeln!(file, "Content-Type: text/plain; charset=UTF-8")?;
    writeln!(file, "Content-Transfer-Encoding: 8bit")?;
    writeln!(file)?; // blank line separating headers from body
    write!(file, "{}", &message.body)?;

    Ok(path)
}

/// Generate a safe filename from a subject line and counter.
pub fn make_eml_filename(subject: &str, counter: u64) -> String {
    let safe: String = subject
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let truncated = if safe.len() > 80 { &safe[..80] } else { &safe };
    format!("{:06}_{}.eml", counter, truncated.trim())
}

/// Simple RFC 2047 encoding for header values with non-ASCII characters.
fn encode_header_value(value: &str) -> String {
    if value.is_ascii() {
        value.to_string()
    } else {
        // UTF-8 B-encoding: =?UTF-8?B?<base64>?=
        let encoded = base64_encode(value.as_bytes());
        format!("=?UTF-8?B?{}?=", encoded)
    }
}

/// Minimal base64 encoder (no external dependency needed).
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        result.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_eml_filename() {
        let name = make_eml_filename("Re: Q3 Budget Review!", 42);
        assert!(name.starts_with("000042_"));
        assert!(name.ends_with(".eml"));
        assert!(!name.contains('!'));
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b"Hi"), "SGk=");
        assert_eq!(base64_encode(b"Hey"), "SGV5");
    }
}
