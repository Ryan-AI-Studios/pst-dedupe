//! Character-based overlapping chunker.

use serde::{Deserialize, Serialize};

/// One text chunk with offsets (char indices into the original string).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextChunk {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub ordinal: u32,
}

/// Result of chunking, including honesty about dropped tail chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkResult {
    pub chunks: Vec<TextChunk>,
    /// Number of additional chunks that would have been produced past max.
    pub dropped_chunks: u32,
}

/// Split `text` into overlapping char windows.
///
/// - `chunk_chars`: window size in Unicode scalar values
/// - `chunk_overlap`: overlap between consecutive windows (`< chunk_chars`)
/// - `max_chunks`: hard cap; further windows counted in `dropped_chunks`
pub fn chunk_text(
    text: &str,
    chunk_chars: u32,
    chunk_overlap: u32,
    max_chunks: u32,
) -> ChunkResult {
    let chunk_chars = chunk_chars.max(1) as usize;
    let max_overlap = chunk_chars.saturating_sub(1);
    let chunk_overlap = (chunk_overlap as usize).min(max_overlap);
    let max_chunks = max_chunks.max(1);
    let step = (chunk_chars - chunk_overlap).max(1);

    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return ChunkResult {
            chunks: Vec::new(),
            dropped_chunks: 0,
        };
    }

    let mut chunks = Vec::new();
    let mut dropped = 0u32;
    let mut start = 0usize;
    let mut ordinal = 0u32;

    while start < chars.len() {
        let end = (start + chunk_chars).min(chars.len());
        if ordinal < max_chunks {
            let slice: String = chars[start..end].iter().collect();
            chunks.push(TextChunk {
                text: slice,
                start,
                end,
                ordinal,
            });
            ordinal += 1;
        } else {
            dropped += 1;
        }
        if end >= chars.len() {
            break;
        }
        start += step;
        // Safety: if step somehow 0 (shouldn't), break.
        if step == 0 {
            break;
        }
    }

    ChunkResult {
        chunks,
        dropped_chunks: dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_and_cap() {
        let text: String = (0..100).map(|_| 'a').collect();
        let r = chunk_text(&text, 30, 10, 3);
        assert_eq!(r.chunks.len(), 3);
        assert!(r.dropped_chunks > 0);
        assert_eq!(r.chunks[0].start, 0);
        assert_eq!(r.chunks[0].end, 30);
        assert_eq!(r.chunks[1].start, 20);
    }

    #[test]
    fn two_theme_mid_chunk() {
        let mut text = String::new();
        text.push_str(&"alpha theme words ".repeat(40));
        text.push_str(&"beta special midtopic keyword ".repeat(40));
        text.push_str(&"gamma footer words ".repeat(40));
        let r = chunk_text(&text, 200, 40, 48);
        assert!(r.chunks.len() > 1);
        let mid = r.chunks.iter().any(|c| c.text.contains("midtopic"));
        assert!(mid, "expected mid-doc theme in some chunk");
    }
}
