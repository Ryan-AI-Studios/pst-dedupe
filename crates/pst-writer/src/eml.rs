//! EML/MIME parser for extracting message properties.

use std::fs;
use std::path::Path;

use chrono::{DateTime, FixedOffset, TimeZone, Utc};

#[derive(Debug, Clone)]
pub struct EmlMessage {
    pub subject: String,
    pub sender: String,
    pub message_id: String,
    pub body: String,
    pub submit_time: Option<i64>,
}

impl EmlMessage {
    pub fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| crate::WriterError::EmlParse(format!("read failed: {}", e)))?;
        Self::from_bytes(&content)
    }

    pub fn from_bytes(content: &str) -> crate::Result<Self> {
        let mut subject = String::new();
        let mut sender = String::new();
        let mut message_id = String::new();
        let mut date_str = String::new();
        let mut body = String::new();
        let mut in_headers = true;
        let mut boundary = String::new();

        for line in content.lines() {
            if in_headers {
                if line.is_empty() {
                    in_headers = false;
                    continue;
                }

                if line.starts_with("Subject: ") {
                    subject = parse_header_value(line, "Subject: ");
                } else if line.starts_with("From: ") {
                    sender = parse_header_value(line, "From: ");
                } else if line.starts_with("Message-ID: ") {
                    message_id = parse_header_value(line, "Message-ID: ");
                } else if line.starts_with("Date: ") {
                    date_str = parse_header_value(line, "Date: ");
                } else if line.starts_with("Content-Type: ") {
                    let content_type = parse_header_value(line, "Content-Type: ");
                    if let Some(b) = extract_boundary(&content_type) {
                        boundary = b;
                    }
                } else if line.starts_with(" ") || line.starts_with("\t") {
                    // Continuation of previous header
                    // For simplicity, we don't handle multi-line headers well
                }
            } else {
                // Body parsing: for multipart, extract text/plain part
                if !boundary.is_empty() && line.contains(&boundary) {
                    // We're at a boundary, skip until we find text/plain
                    // For simplicity, just accumulate everything after the last boundary
                    body.clear();
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }

        // If body is empty and we have multipart, try a simpler extraction
        if body.trim().is_empty() && !boundary.is_empty() {
            body = extract_plain_text(content, &boundary);
        }

        // If still empty, use everything after headers as body
        if body.trim().is_empty() {
            if let Some(pos) = content.find("\n\n") {
                body = content[pos + 2..].to_string();
            } else if let Some(pos) = content.find("\r\n\r\n") {
                body = content[pos + 4..].to_string();
            }
        }

        let submit_time = parse_rfc2822_date(&date_str);

        Ok(EmlMessage {
            subject,
            sender,
            message_id,
            body,
            submit_time,
        })
    }
}

fn parse_header_value(line: &str, prefix: &str) -> String {
    let val = line.strip_prefix(prefix).unwrap_or("").trim();
    decode_mime_header(val)
}

fn decode_mime_header(value: &str) -> String {
    // Handle =?charset?B?base64?=
    if value.contains("=?") && value.contains("?=") {
        let mut result = String::new();
        let mut remaining = value;

        while let Some(start) = remaining.find("=?") {
            result.push_str(&remaining[..start]);
            if let Some(end) = remaining[start..].find("?=") {
                let encoded = &remaining[start..start + end + 2];
                if let Some(decoded) = decode_encoded_word(encoded) {
                    result.push_str(&decoded);
                } else {
                    result.push_str(encoded);
                }
                remaining = &remaining[start + end + 2..];
            } else {
                result.push_str(remaining);
                break;
            }
        }
        result.push_str(remaining);
        result.trim().to_string()
    } else {
        value.to_string()
    }
}

fn decode_encoded_word(word: &str) -> Option<String> {
    // Format: =?charset?B?base64?=
    let stripped = word.strip_prefix("=?")?.strip_suffix("?=")?;
    let parts: Vec<&str> = stripped.split('?').collect();
    if parts.len() < 3 {
        return None;
    }

    let encoding = parts[1].to_uppercase();
    let data = parts[2];

    if encoding == "B" {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?;
        // Try UTF-8 first
        String::from_utf8(decoded).ok()
    } else if encoding == "Q" {
        // Quoted-printable
        let decoded = decode_quoted_printable(data);
        Some(decoded)
    } else {
        None
    }
}

fn decode_quoted_printable(data: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = data.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '=' && i + 2 < chars.len() {
            let hex = format!("{}{}", chars[i + 1], chars[i + 2]);
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
                i += 3;
                continue;
            }
        } else if chars[i] == '_' {
            result.push(' ');
            i += 1;
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn extract_boundary(content_type: &str) -> Option<String> {
    if let Some(pos) = content_type.find("boundary=") {
        let rest = &content_type[pos + 9..];
        let boundary = rest.trim().trim_matches('"').trim_matches('\'');
        Some(format!("--{}", boundary))
    } else {
        None
    }
}

fn extract_plain_text(content: &str, boundary: &str) -> String {
    let mut result = String::new();
    let parts: Vec<&str> = content.split(boundary).collect();

    for part in parts {
        if part.contains("Content-Type: text/plain") {
            // Find the blank line after headers
            if let Some(pos) = part.find("\n\n") {
                result = part[pos + 2..].trim().to_string();
                break;
            } else if let Some(pos) = part.find("\r\n\r\n") {
                result = part[pos + 4..].trim().to_string();
                break;
            }
        }
    }

    result
}

fn parse_rfc2822_date(date_str: &str) -> Option<i64> {
    if date_str.is_empty() {
        return None;
    }

    // Try common RFC 2822 formats
    let formats = [
        "%a, %d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M:%S %Z",
        "%d %b %Y %H:%M:%S %z",
        "%d %b %Y %H:%M:%S %Z",
    ];

    for fmt in &formats {
        if let Ok(dt) = DateTime::parse_from_str(date_str, fmt) {
            return Some(datetime_to_filetime(dt));
        }
    }

    // Try with chrono's flexible parsing
    if let Ok(dt) = date_str.parse::<DateTime<FixedOffset>>() {
        return Some(datetime_to_filetime(dt));
    }

    None
}

/// Convert a DateTime to Windows FILETIME (100ns intervals since 1601-01-01).
fn datetime_to_filetime(dt: DateTime<FixedOffset>) -> i64 {
    let epoch_1601 = Utc.with_ymd_and_hms(1601, 1, 1, 0, 0, 0).unwrap();
    let dt_utc = dt.with_timezone(&Utc);
    let diff = dt_utc.signed_duration_since(epoch_1601);
    diff.num_seconds() * 10_000_000 + (diff.subsec_nanos() as i64 / 100)
}
