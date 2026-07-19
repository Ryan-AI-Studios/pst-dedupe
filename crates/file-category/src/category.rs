//! Canonical `taxonomy_v1` category vocabulary.

use std::fmt;
use std::str::FromStr;

/// Taxonomy version string stored on items / emitted by the classifier.
pub const TAXONOMY_V1: &str = "taxonomy_v1";

/// Closed set of workstation-grade file categories (`taxonomy_v1`).
///
/// Stable **lowercase snake** strings via [`Category::as_str`].
/// **`attachment` is not a category** — keep `role=attachment` separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Email,
    Calendar,
    Contact,
    Chat,
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Image,
    Multimedia,
    Archive,
    Database,
    Log,
    Executable,
    System,
    Pst,
    Mobile,
    Cloud,
    Other,
    Unrecognized,
}

/// All categories in stable display / docs order.
pub const ALL: &[Category] = &[
    Category::Email,
    Category::Calendar,
    Category::Contact,
    Category::Chat,
    Category::Document,
    Category::Spreadsheet,
    Category::Presentation,
    Category::Pdf,
    Category::Image,
    Category::Multimedia,
    Category::Archive,
    Category::Database,
    Category::Log,
    Category::Executable,
    Category::System,
    Category::Pst,
    Category::Mobile,
    Category::Cloud,
    Category::Other,
    Category::Unrecognized,
];

impl Category {
    /// Canonical lowercase snake string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Calendar => "calendar",
            Self::Contact => "contact",
            Self::Chat => "chat",
            Self::Document => "document",
            Self::Spreadsheet => "spreadsheet",
            Self::Presentation => "presentation",
            Self::Pdf => "pdf",
            Self::Image => "image",
            Self::Multimedia => "multimedia",
            Self::Archive => "archive",
            Self::Database => "database",
            Self::Log => "log",
            Self::Executable => "executable",
            Self::System => "system",
            Self::Pst => "pst",
            Self::Mobile => "mobile",
            Self::Cloud => "cloud",
            Self::Other => "other",
            Self::Unrecognized => "unrecognized",
        }
    }

    /// True when the category is a legacy / weak label that the job should reclassify.
    pub fn is_legacy_or_weak(self) -> bool {
        matches!(self, Self::Other | Self::Unrecognized)
    }

    /// Parse canonical or alias input (case-insensitive).
    ///
    /// Returns `None` for unknown strings (including forbidden `attachment`).
    pub fn parse_loose(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        if t.is_empty() {
            return None;
        }
        // Forbidden as a category.
        if t == "attachment" {
            return None;
        }
        // Canonical first.
        for c in ALL {
            if c.as_str() == t {
                return Some(*c);
            }
        }
        // Aliases (input only).
        Some(match t.as_str() {
            "doc" | "docs" | "word" => Self::Document,
            "xls" | "xlsx" | "sheet" => Self::Spreadsheet,
            "ppt" | "slides" => Self::Presentation,
            "container" | "zip" => Self::Archive,
            "video" | "audio" | "media" => Self::Multimedia,
            "exe" | "binary" => Self::Executable,
            // Documented choice: unknown → unrecognized
            "unknown" => Self::Unrecognized,
            _ => return None,
        })
    }

    /// True when `s` is a closed-set category (not attachment/empty) and not weak.
    pub fn is_decisive_existing(s: Option<&str>) -> bool {
        match s {
            None => false,
            Some(raw) => match Self::parse_loose(raw) {
                Some(c) => !c.is_legacy_or_weak(),
                None => false,
            },
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Category {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_loose(s).ok_or(())
    }
}

/// How the category was decided (stored in `category_method`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CategoryMethod {
    MessageClass,
    Magic,
    MagicOoxml,
    Mime,
    Extension,
    Alias,
    Extractor,
    Fallback,
    ContainerTiebreak,
    Manual,
}

impl CategoryMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MessageClass => "message_class",
            Self::Magic => "magic",
            Self::MagicOoxml => "magic_ooxml",
            Self::Mime => "mime",
            Self::Extension => "extension",
            Self::Alias => "alias",
            Self::Extractor => "extractor",
            Self::Fallback => "fallback",
            Self::ContainerTiebreak => "container_tiebreak",
            Self::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "message_class" => Some(Self::MessageClass),
            "magic" => Some(Self::Magic),
            "magic_ooxml" => Some(Self::MagicOoxml),
            "mime" => Some(Self::Mime),
            "extension" => Some(Self::Extension),
            "alias" => Some(Self::Alias),
            "extractor" => Some(Self::Extractor),
            "fallback" => Some(Self::Fallback),
            "container_tiebreak" => Some(Self::ContainerTiebreak),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl fmt::Display for CategoryMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Optional confidence for audit/debug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Result of a single classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub category: Category,
    /// Refined mime when stronger than empty/generic.
    pub mime_type: Option<String>,
    pub method: CategoryMethod,
    pub confidence: Confidence,
}

impl Classification {
    pub fn taxonomy(&self) -> &'static str {
        TAXONOMY_V1
    }

    pub fn new(category: Category, method: CategoryMethod, confidence: Confidence) -> Self {
        Self {
            category,
            mime_type: None,
            method,
            confidence,
        }
    }

    pub fn with_mime(mut self, mime: Option<String>) -> Self {
        self.mime_type = mime;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_round_trip_as_str_parse() {
        for c in ALL {
            assert_eq!(Category::parse_loose(c.as_str()), Some(*c));
            assert_eq!(c.as_str().parse::<Category>().ok(), Some(*c));
        }
        assert_eq!(ALL.len(), 20);
    }

    #[test]
    fn aliases_map() {
        assert_eq!(Category::parse_loose("doc"), Some(Category::Document));
        assert_eq!(Category::parse_loose("docs"), Some(Category::Document));
        assert_eq!(Category::parse_loose("word"), Some(Category::Document));
        assert_eq!(Category::parse_loose("xls"), Some(Category::Spreadsheet));
        assert_eq!(Category::parse_loose("sheet"), Some(Category::Spreadsheet));
        assert_eq!(Category::parse_loose("ppt"), Some(Category::Presentation));
        assert_eq!(
            Category::parse_loose("slides"),
            Some(Category::Presentation)
        );
        assert_eq!(Category::parse_loose("container"), Some(Category::Archive));
        assert_eq!(Category::parse_loose("zip"), Some(Category::Archive));
        assert_eq!(Category::parse_loose("video"), Some(Category::Multimedia));
        assert_eq!(Category::parse_loose("media"), Some(Category::Multimedia));
        assert_eq!(Category::parse_loose("exe"), Some(Category::Executable));
        assert_eq!(Category::parse_loose("binary"), Some(Category::Executable));
        assert_eq!(
            Category::parse_loose("unknown"),
            Some(Category::Unrecognized)
        );
    }

    #[test]
    fn attachment_forbidden() {
        assert_eq!(Category::parse_loose("attachment"), None);
        assert!(!Category::is_decisive_existing(Some("attachment")));
        assert!(Category::is_decisive_existing(Some("document")));
        assert!(!Category::is_decisive_existing(Some("other")));
        assert!(!Category::is_decisive_existing(Some("unrecognized")));
    }
}
