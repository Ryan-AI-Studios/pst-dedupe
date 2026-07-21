//! Language packs + optional item language tags (schema v33 / track 0054).
//!
//! Offline only. Pack ids drive Tantivy tokenizer selection in `matter-search`.
//! Changing pack clears the FTS fingerprint so search hard-fails until rebuild.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use whatlang::Lang;

use crate::error::{Error, Result};
use crate::matter::Matter;

// ---------------------------------------------------------------------------
// Pack constants
// ---------------------------------------------------------------------------

/// Latin / English-friendly default (Tantivy `default` tokenizer).
pub const LANG_PACK_LATIN_DEFAULT: &str = "latin_default";
/// Hybrid CJK character bigrams + Latin simple path.
pub const LANG_PACK_CJK_NGRAM_V1: &str = "cjk_ngram_v1";

/// Built-in pack version for both P0 packs.
pub const LANG_PACK_VERSION_V1: i64 = 1;

/// Min Unicode scalar chars before language detection may emit a non-`und` tag.
pub const LANG_DETECT_MIN_CHARS: usize = 50;

/// Fallback confidence threshold when `is_reliable()` is not used alone.
pub const LANG_DETECT_MIN_CONFIDENCE: f64 = 0.8;

/// Known pack ids (P0).
pub const KNOWN_LANG_PACKS: &[&str] = &[LANG_PACK_LATIN_DEFAULT, LANG_PACK_CJK_NGRAM_V1];

// ---------------------------------------------------------------------------
// CJK script ranges (same blocks as matter-neardup; duplicated to avoid cycle)
// ---------------------------------------------------------------------------

/// Unicode ranges used for CJK run detection (documented standard blocks).
///
/// | Block | Range |
/// |---|---|
/// | CJK Unified Ideographs | U+4E00–U+9FFF |
/// | CJK Unified Ideographs Extension A | U+3400–U+4DBF |
/// | Hiragana | U+3040–U+309F |
/// | Katakana | U+30A0–U+30FF |
/// | Hangul Syllables | U+AC00–U+D7AF |
/// | CJK Compatibility Ideographs | U+F900–U+FAFF |
pub fn is_cjk_char(c: char) -> bool {
    matches!(
        c,
        '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
    )
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Matter-level language pack + last successful FTS build fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangMatterConfig {
    pub lang_pack_id: String,
    pub lang_pack_version: i64,
    pub fts_lang_fingerprint: Option<String>,
    pub fts_lang_built_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Pack validation
// ---------------------------------------------------------------------------

/// True when `pack_id` is a known built-in pack.
pub fn is_known_lang_pack(pack_id: &str) -> bool {
    KNOWN_LANG_PACKS.contains(&pack_id)
}

/// Validate pack id or return an error.
pub fn validate_lang_pack_id(pack_id: &str) -> Result<()> {
    if is_known_lang_pack(pack_id) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "unknown language pack '{pack_id}' (expected {})",
            KNOWN_LANG_PACKS.join("|")
        )))
    }
}

// ---------------------------------------------------------------------------
// Language detection (offline, thin)
// ---------------------------------------------------------------------------

/// Best-effort BCP-47-ish tag: `en` / `zh` / `ja` / `ko` / `und`.
///
/// Rules (locked 0054):
/// - Unicode scalar length **&lt; 50** → `und`
/// - whatlang missing / not reliable and confidence **&lt; 0.8** → `und`
/// - Only maps Eng/Cmn/Jpn/Kor; other langs → `und` in P0
pub fn detect_language_tag(text: &str) -> String {
    let n = text.chars().count();
    if n < LANG_DETECT_MIN_CHARS {
        return "und".into();
    }
    let Some(info) = whatlang::detect(text) else {
        return "und".into();
    };
    let ok = info.is_reliable() || info.confidence() >= LANG_DETECT_MIN_CONFIDENCE;
    if !ok {
        return "und".into();
    }
    match info.lang() {
        Lang::Eng => "en".into(),
        Lang::Cmn => "zh".into(),
        Lang::Jpn => "ja".into(),
        Lang::Kor => "ko".into(),
        _ => "und".into(),
    }
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Load matter language pack config (defaults: `latin_default` / version 1).
    pub fn get_lang_config(&self) -> Result<LangMatterConfig> {
        self.connection()
            .query_row(
                "SELECT lang_pack_id, lang_pack_version, fts_lang_fingerprint, fts_lang_built_at \
                 FROM matters WHERE id = ?1",
                params![self.id()],
                |row| {
                    Ok(LangMatterConfig {
                        lang_pack_id: row.get(0)?,
                        lang_pack_version: row.get(1)?,
                        fts_lang_fingerprint: row.get(2)?,
                        fts_lang_built_at: row.get(3)?,
                    })
                },
            )
            .map_err(Error::from)
    }

    /// Set the active language pack.
    ///
    /// On pack **change**, clears `fts_lang_fingerprint` and `fts_lang_built_at`
    /// so search hard-fails until a successful `fts_index` rebuild.
    pub fn update_lang_pack(&self, pack_id: &str) -> Result<LangMatterConfig> {
        validate_lang_pack_id(pack_id)?;
        let current = self.get_lang_config()?;
        if current.lang_pack_id == pack_id && current.lang_pack_version == LANG_PACK_VERSION_V1 {
            return Ok(current);
        }
        self.connection().execute(
            "UPDATE matters SET \
                lang_pack_id = ?1, \
                lang_pack_version = ?2, \
                fts_lang_fingerprint = NULL, \
                fts_lang_built_at = NULL \
             WHERE id = ?3",
            params![pack_id, LANG_PACK_VERSION_V1, self.id()],
        )?;
        self.get_lang_config()
    }

    /// Record the FTS language fingerprint after a **Succeeded** `fts_index` run.
    pub fn set_fts_lang_fingerprint(&self, fingerprint: &str, built_at: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE matters SET fts_lang_fingerprint = ?1, fts_lang_built_at = ?2 WHERE id = ?3",
            params![fingerprint, built_at, self.id()],
        )?;
        Ok(())
    }

    /// Clear FTS language fingerprint (forces stale until rebuild).
    pub fn clear_fts_lang_fingerprint(&self) -> Result<()> {
        self.connection().execute(
            "UPDATE matters SET fts_lang_fingerprint = NULL, fts_lang_built_at = NULL WHERE id = ?1",
            params![self.id()],
        )?;
        Ok(())
    }

    /// Set optional `items.language_tag` (BCP-47-ish or `und`).
    pub fn set_item_language_tag(&self, item_id: &str, tag: Option<&str>) -> Result<()> {
        self.ensure_item_in_matter(item_id)?;
        let tag = tag.map(str::trim).filter(|s| !s.is_empty());
        self.connection().execute(
            "UPDATE items SET language_tag = ?1 WHERE id = ?2 AND matter_id = ?3",
            params![tag, item_id, self.id()],
        )?;
        Ok(())
    }

    /// Batch set language tags: `(item_id, tag)`.
    pub fn set_item_language_tags_batch(&self, tags: &[(&str, Option<&str>)]) -> Result<()> {
        if tags.is_empty() {
            return Ok(());
        }
        let matter_id = self.id().to_string();
        self.with_transaction(|conn| {
            let mut stmt = conn
                .prepare("UPDATE items SET language_tag = ?1 WHERE id = ?2 AND matter_id = ?3")?;
            for (item_id, tag) in tags {
                let t = tag.map(str::trim).filter(|s| !s.is_empty());
                let n = stmt.execute(params![t, item_id, matter_id])?;
                if n == 0 {
                    return Err(Error::ItemNotFound((*item_id).into()));
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_und() {
        assert_eq!(detect_language_tag("See attached"), "und");
        assert_eq!(detect_language_tag("12345"), "und");
        assert_eq!(detect_language_tag("hello world"), "und");
        assert_eq!(detect_language_tag(""), "und");
    }

    #[test]
    fn known_packs() {
        assert!(is_known_lang_pack(LANG_PACK_LATIN_DEFAULT));
        assert!(is_known_lang_pack(LANG_PACK_CJK_NGRAM_V1));
        assert!(!is_known_lang_pack("jieba_v1"));
        assert!(validate_lang_pack_id("nope").is_err());
    }

    #[test]
    fn is_cjk_ranges() {
        assert!(is_cjk_char('中'));
        assert!(is_cjk_char('あ'));
        assert!(is_cjk_char('ア'));
        assert!(is_cjk_char('한'));
        assert!(!is_cjk_char('a'));
        assert!(!is_cjk_char('@'));
    }
}
