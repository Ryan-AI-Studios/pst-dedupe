//! Generate synthetic PDF fixtures under `fixtures/pdf/`.
//!
//! ```text
//! cargo run -p extract-pdf --example gen_fixtures
//! ```

use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    out.pop(); // crates
    out.pop(); // workspace
    out.push("fixtures");
    out.push("pdf");
    fs::create_dir_all(&out)?;

    // Enough non-ws chars for status `ok` (>= MIN_TEXT_CHARS_TOTAL=50) while
    // still containing the required marker token.
    let minimal = minimal_text_pdf(
        "PDF_TEXT_MARKER This is enough embedded body text for ok classification threshold.",
    );
    fs::write(out.join("minimal.pdf"), &minimal)?;
    println!("wrote minimal.pdf ({} bytes)", minimal.len());

    // Truncated / corrupt: start of a valid header then cut.
    let mut corrupt = minimal.clone();
    corrupt.truncate(40);
    fs::write(out.join("corrupt.pdf"), &corrupt)?;
    println!("wrote corrupt.pdf ({} bytes)", corrupt.len());

    // Empty-ish PDF with no text operators.
    let empty = empty_page_pdf();
    fs::write(out.join("empty.pdf"), &empty)?;
    println!("wrote empty.pdf ({} bytes)", empty.len());

    // Low-text: single short token (well under MIN_TEXT_CHARS_TOTAL=50).
    let low = minimal_text_pdf("BATES001");
    fs::write(out.join("low_text.pdf"), &low)?;
    println!("wrote low_text.pdf ({} bytes)", low.len());

    Ok(())
}

/// Minimal valid one-page PDF with a single `(text) Tj` operator.
pub fn minimal_text_pdf(text: &str) -> Vec<u8> {
    // Escape PDF string specials.
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let content = format!("BT\n/F1 12 Tf\n72 720 Td\n({escaped}) Tj\nET\n");
    build_one_page_pdf(&content)
}

fn empty_page_pdf() -> Vec<u8> {
    // No text operators — content stream is empty.
    build_one_page_pdf("")
}

fn build_one_page_pdf(content: &str) -> Vec<u8> {
    let content_len = content.len();
    // Build as bytes so we can include binary comment octets > 0x7f.
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"%PDF-1.4\n%");
    body.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3]);
    body.push(b'\n');

    let mut offsets = Vec::new();
    offsets.push(body.len());
    body.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(body.len());
    body.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    offsets.push(body.len());
    body.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
    );
    offsets.push(body.len());
    let content_obj =
        format!("4 0 obj\n<< /Length {content_len} >>\nstream\n{content}endstream\nendobj\n");
    body.extend_from_slice(content_obj.as_bytes());
    offsets.push(body.len());
    body.extend_from_slice(
        b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );

    let xref_pos = body.len();
    let mut xref = String::from("xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        xref.push_str(&format!("{off:010} 00000 n \n"));
    }
    body.extend_from_slice(xref.as_bytes());
    let trailer = format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n");
    body.extend_from_slice(trailer.as_bytes());
    body
}

// Allow re-use of generator from tests via include! or copy — tests build their own.
#[allow(dead_code)]
fn write_stdout(bytes: &[u8]) {
    let _ = std::io::stdout().write_all(bytes);
}
