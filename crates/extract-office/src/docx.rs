//! DOCX text extract via zip + quick-xml (`word/document.xml` `w:t` nodes).

use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::{Error, Result};
use crate::limits::methods;
use crate::text_buf::TextBuf;
use crate::zip_safe::{open_zip, read_named_entry, try_read_named_entry};
use crate::ExtractedText;

/// Extract plain text from a DOCX package.
pub fn extract_docx(data: &[u8]) -> Result<ExtractedText> {
    let mut archive = open_zip(data)?;
    let doc_xml = read_named_entry(&mut archive, "word/document.xml")
        .map_err(|e| Error::parse(format!("docx document.xml: {e}")))?;

    let mut buf = TextBuf::default();
    extract_wt_text(&doc_xml, &mut buf)?;

    // Optional headers/footers (best-effort, only if not already full).
    if !buf.is_full() {
        let names: Vec<String> = {
            let mut names = Vec::new();
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().to_string();
                    if (name.starts_with("word/header") || name.starts_with("word/footer"))
                        && name.ends_with(".xml")
                    {
                        names.push(name);
                    }
                }
            }
            names
        };
        for name in names {
            if buf.is_full() {
                break;
            }
            if let Some(xml) = try_read_named_entry(&mut archive, &name)? {
                if !buf.is_empty() {
                    let _ = buf.push_str("\n");
                }
                extract_wt_text(&xml, &mut buf)?;
            }
        }
    }

    let (text, partial) = buf.into_string();
    if text.trim().is_empty() {
        return Err(Error::EmptyText("docx produced zero text".into()));
    }
    Ok(ExtractedText {
        text,
        method: methods::DOCX_XML_V1.into(),
        partial,
        format: crate::detect::OfficeFormat::Docx,
    })
}

/// Pull text from `w:t` (and `w:tab` / paragraph breaks).
fn extract_wt_text(xml: &[u8], out: &mut TextBuf) -> Result<()> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut xml_buf = Vec::new();
    let mut in_t = false;
    let mut para_has_text = false;

    loop {
        if out.is_full() {
            break;
        }
        match reader.read_event_into(&mut xml_buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name_owned(name.as_ref());
                match local.as_slice() {
                    b"t" => in_t = true,
                    b"tab" if !out.push_str("\t") => break,
                    b"tab" => para_has_text = true,
                    b"br" | b"cr" if !out.push_str("\n") => break,
                    b"p" => para_has_text = false,
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = local_name_owned(name.as_ref());
                match local.as_slice() {
                    b"tab" if !out.push_str("\t") => break,
                    b"tab" => para_has_text = true,
                    b"br" | b"cr" if !out.push_str("\n") => break,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name_owned(name.as_ref());
                match local.as_slice() {
                    b"t" => in_t = false,
                    b"p" if (para_has_text || !out.is_empty()) && !out.push_str("\n") => break,
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
                if !decoded.is_empty() {
                    para_has_text = true;
                    if !out.push_str(&decoded) {
                        break;
                    }
                }
            }
            Ok(Event::CData(t)) if in_t => {
                let decoded = String::from_utf8_lossy(t.as_ref());
                if !decoded.is_empty() {
                    para_has_text = true;
                    if !out.push_str(&decoded) {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::parse(format!("docx xml: {e}"))),
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
    fn extracts_simple_wt() {
        let xml = br#"<?xml version="1.0"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body><w:p><w:r><w:t>OFFICE_DOCX_MARKER</w:t></w:r></w:p></w:body>
        </w:document>"#;
        let mut buf = TextBuf::default();
        extract_wt_text(xml, &mut buf).unwrap();
        assert!(buf.as_str().contains("OFFICE_DOCX_MARKER"));
    }
}
