//! PPTX text extract via zip + quick-xml (`ppt/slides/slide*.xml` `a:t` nodes).

use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::{Error, Result};
use crate::limits::{methods, MAX_SHEETS_OR_SLIDES};
use crate::text_buf::TextBuf;
use crate::zip_safe::{open_zip, read_entry_capped, validate_entry_name};
use crate::ExtractedText;

/// Extract plain text from a PPTX package.
pub fn extract_pptx(data: &[u8]) -> Result<ExtractedText> {
    let mut archive = open_zip(data)?;

    // Collect slide entry names and sort numerically.
    let mut slides: Vec<(u32, String)> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| Error::parse(format!("pptx zip index: {e}")))?;
        let name = entry.name().to_string();
        if let Some(n) = parse_slide_index(&name) {
            validate_entry_name(&name)?;
            slides.push((n, name));
        }
    }
    slides.sort_by_key(|(n, _)| *n);

    if slides.is_empty() {
        return Err(Error::parse("pptx has no slides"));
    }

    let mut buf = TextBuf::default();
    let mut hit_slide_cap = false;

    for (idx, (_num, name)) in slides.into_iter().enumerate() {
        if buf.is_full() {
            break;
        }
        if idx >= MAX_SHEETS_OR_SLIDES {
            hit_slide_cap = true;
            break;
        }
        let slide_no = idx + 1;
        let header = format!("--- Slide {slide_no} ---\n");
        if !buf.push_str(&header) {
            break;
        }

        let mut entry = archive
            .by_name(&name)
            .map_err(|e| Error::parse(format!("slide '{name}': {e}")))?;
        let xml = read_entry_capped(&mut entry)?;
        extract_at_text(&xml, &mut buf)?;
        if !buf.is_full() {
            let _ = buf.push_str("\n");
        }
    }

    let (mut text, mut partial) = buf.into_string();
    if hit_slide_cap {
        partial = true;
        if !text.contains(crate::limits::TRUNCATION_MARKER) {
            text.push_str(crate::limits::TRUNCATION_MARKER);
        }
    }

    if text.trim().is_empty() {
        return Err(Error::EmptyText("pptx produced zero text".into()));
    }
    Ok(ExtractedText {
        text,
        method: methods::PPTX_XML_V1.into(),
        partial,
        format: crate::detect::OfficeFormat::Pptx,
    })
}

fn parse_slide_index(name: &str) -> Option<u32> {
    // ppt/slides/slideN.xml
    let name = name.replace('\\', "/");
    let prefix = "ppt/slides/slide";
    let suffix = ".xml";
    if !name.starts_with(prefix) || !name.ends_with(suffix) {
        return None;
    }
    let mid = &name[prefix.len()..name.len() - suffix.len()];
    if mid.is_empty() || !mid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    mid.parse().ok()
}

fn extract_at_text(xml: &[u8], out: &mut TextBuf) -> Result<()> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut xml_buf = Vec::new();
    let mut in_t = false;

    loop {
        if out.is_full() {
            break;
        }
        match reader.read_event_into(&mut xml_buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name_owned(name.as_ref());
                if local.as_slice() == b"t" {
                    in_t = true;
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = local_name_owned(name.as_ref());
                if local.as_slice() == b"br" && !out.push_str("\n") {
                    break;
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name_owned(name.as_ref());
                match local.as_slice() {
                    b"t" => in_t = false,
                    b"p" if !out.push_str("\n") => break,
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_t => {
                // Prefer decoded XML text; on encoding failure use lossy UTF-8
                // replacement rather than silently dropping the run.
                let decoded = match t.decode() {
                    Ok(s) => s.into_owned(),
                    Err(_) => String::from_utf8_lossy(t.as_ref()).into_owned(),
                };
                if !decoded.is_empty() && !out.push_str(&decoded) {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::parse(format!("pptx xml: {e}"))),
            _ => {}
        }
        xml_buf.clear();
    }
    Ok(())
}

fn local_name_owned(qname: &[u8]) -> Vec<u8> {
    if let Some(pos) = qname.iter().position(|&b| b == b':') {
        qname[pos + 1..].to_vec()
    } else {
        qname.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_slide_names() {
        assert_eq!(parse_slide_index("ppt/slides/slide1.xml"), Some(1));
        assert_eq!(parse_slide_index("ppt/slides/slide12.xml"), Some(12));
        assert_eq!(parse_slide_index("ppt/slides/_rels/slide1.xml.rels"), None);
    }

    #[test]
    fn extracts_at() {
        let xml = br#"<?xml version="1.0"?>
        <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
               xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
          <p:cSld><p:spTree><p:sp><p:txBody>
            <a:p><a:r><a:t>OFFICE_PPTX_MARKER</a:t></a:r></a:p>
          </p:txBody></p:sp></p:spTree></p:cSld>
        </p:sld>"#;
        let mut buf = TextBuf::default();
        extract_at_text(xml, &mut buf).unwrap();
        assert!(buf.as_str().contains("OFFICE_PPTX_MARKER"));
    }
}
