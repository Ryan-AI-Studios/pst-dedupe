//! Language pack registry + FTS fingerprint strings (track 0054).
//!
//! Fingerprint form (stable):
//! `pack={id};ver={n};ngram={min}-{max};tok={tokenizer_id};schema=fts_v1`

use matter_core::{LANG_PACK_CJK_NGRAM_V1, LANG_PACK_LATIN_DEFAULT, LANG_PACK_VERSION_V1};

use crate::error::{Result, SearchError};

/// Tantivy tokenizer name for the CJK hybrid analyzer.
pub const CJK_HYBRID_TOKENIZER_ID: &str = "cjk_hybrid_v1";

/// Schema dialect id embedded in fingerprints.
pub const FTS_SCHEMA_ID: &str = "fts_v1";

/// CJK n-gram width (character bigrams only).
pub const CJK_MIN_GRAM: u32 = 2;
pub const CJK_MAX_GRAM: u32 = 2;

/// Known FTS language pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangPack {
    LatinDefault,
    CjkNgramV1,
}

impl LangPack {
    /// Parse a pack id string.
    pub fn parse(id: &str) -> Result<Self> {
        match id {
            LANG_PACK_LATIN_DEFAULT => Ok(Self::LatinDefault),
            LANG_PACK_CJK_NGRAM_V1 => Ok(Self::CjkNgramV1),
            other => Err(SearchError::InvalidParams(format!(
                "unknown language pack '{other}' (expected {LANG_PACK_LATIN_DEFAULT}|{LANG_PACK_CJK_NGRAM_V1})"
            ))),
        }
    }

    /// Stable pack id string.
    pub fn id(self) -> &'static str {
        match self {
            Self::LatinDefault => LANG_PACK_LATIN_DEFAULT,
            Self::CjkNgramV1 => LANG_PACK_CJK_NGRAM_V1,
        }
    }

    /// Pack version (P0: always 1).
    pub fn version(self) -> i64 {
        LANG_PACK_VERSION_V1
    }

    /// Tokenizer id embedded in the fingerprint.
    pub fn tokenizer_id(self) -> &'static str {
        match self {
            Self::LatinDefault => "default",
            Self::CjkNgramV1 => CJK_HYBRID_TOKENIZER_ID,
        }
    }

    /// Whether this pack uses the hybrid CJK tokenizer.
    pub fn is_cjk(self) -> bool {
        matches!(self, Self::CjkNgramV1)
    }

    /// Stable fingerprint string stored on `matters.fts_lang_fingerprint`.
    pub fn fingerprint(self) -> String {
        match self {
            Self::LatinDefault => format!(
                "pack={};ver={};ngram=0-0;tok=default;schema={}",
                self.id(),
                self.version(),
                FTS_SCHEMA_ID
            ),
            Self::CjkNgramV1 => format!(
                "pack={};ver={};ngram={}-{};tok={};schema={}",
                self.id(),
                self.version(),
                CJK_MIN_GRAM,
                CJK_MAX_GRAM,
                CJK_HYBRID_TOKENIZER_ID,
                FTS_SCHEMA_ID
            ),
        }
    }
}

/// Compute fingerprint for a pack id.
pub fn fingerprint_for_pack_id(pack_id: &str) -> Result<String> {
    Ok(LangPack::parse(pack_id)?.fingerprint())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_stable() {
        assert_eq!(
            LangPack::LatinDefault.fingerprint(),
            "pack=latin_default;ver=1;ngram=0-0;tok=default;schema=fts_v1"
        );
        assert_eq!(
            LangPack::CjkNgramV1.fingerprint(),
            "pack=cjk_ngram_v1;ver=1;ngram=2-2;tok=cjk_hybrid_v1;schema=fts_v1"
        );
    }
}
