//! Simple last-path / recent matters persistence (JSON under user config).

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const APP_DIR: &str = "dedupe-desk";
const FILE_NAME: &str = "settings.json";
const MAX_RECENT: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeskSettings {
    /// Most recently opened/created matter roots (newest first).
    #[serde(default)]
    pub recent_matters: Vec<String>,
    /// Last parent directory used for Create Matter.
    #[serde(default)]
    pub last_parent_dir: Option<String>,
    /// Reviewer name used as coding audit actor (0027). Empty → `"desk"`.
    #[serde(default)]
    pub reviewer_name: String,
    /// Enable local OCR job (default **false** — opt-in).
    #[serde(default)]
    pub ocr_enabled: bool,
    /// Optional path to `tesseract` / `tesseract.exe`.
    #[serde(default)]
    pub tesseract_path: Option<String>,
    /// Optional tessdata directory (`TESSDATA_PREFIX`).
    #[serde(default)]
    pub tessdata_dir: Option<String>,
    /// Optional path to `pdftoppm` or `mutool` for PDF page render.
    #[serde(default)]
    pub pdf_renderer_path: Option<String>,
    /// Enable local STT job (default **false** — opt-in).
    #[serde(default)]
    pub stt_enabled: bool,
    /// Optional path to `whisper-cli` / whisper.cpp binary.
    #[serde(default)]
    pub whisper_cli_path: Option<String>,
    /// Path to Whisper model weights (operator-installed; never downloaded).
    #[serde(default)]
    pub stt_model_path: Option<String>,
    /// Optional path to `ffmpeg` for video / complex audio conversion.
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    /// Prefer semantic search features in Desk (default **false** — opt-in).
    /// Dual-writes to open matter `semantic_enabled` when toggled with a matter open.
    #[serde(default)]
    pub semantic_enabled: bool,
    /// AI assist enabled (default **false** — opt-in). Dual-writes to matter when open.
    #[serde(default)]
    pub ai_enabled: bool,
    /// Allow non-loopback (cloud) AI providers (default **false**).
    #[serde(default)]
    pub ai_allow_remote: bool,
    /// OpenAI-compatible base URL (e.g. `http://127.0.0.1:11434/v1`).
    #[serde(default)]
    pub ai_base_url: Option<String>,
    /// Model id string (e.g. `llama3.2` or `mock`).
    #[serde(default)]
    pub ai_model: Option<String>,
    /// `none` | `mock` | `openai_compatible`.
    #[serde(default)]
    pub ai_provider_kind: Option<String>,
    /// Matter language pack for FTS (`latin_default` | `cjk_ngram_v1`).
    /// Dual-writes to open matter when changed; hydrated from matter on open.
    #[serde(default = "default_lang_pack_id")]
    pub lang_pack_id: String,
}

fn default_lang_pack_id() -> String {
    "latin_default".into()
}

impl DeskSettings {
    pub fn load() -> Self {
        let path = settings_path();
        let mut s = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        if s.lang_pack_id.trim().is_empty() {
            s.lang_pack_id = default_lang_pack_id();
        }
        s
    }

    pub fn save(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn remember_matter(&mut self, root: &str) {
        self.recent_matters.retain(|p| p != root);
        self.recent_matters.insert(0, root.to_string());
        self.recent_matters.truncate(MAX_RECENT);
    }

    /// Coding / audit actor: trimmed `reviewer_name`, or `"desk"` when empty.
    pub fn actor(&self) -> &str {
        let t = self.reviewer_name.trim();
        if t.is_empty() {
            "desk"
        } else {
            t
        }
    }
}

fn settings_path() -> PathBuf {
    config_dir().join(APP_DIR).join(FILE_NAME)
}

fn config_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata);
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        return PathBuf::from(home).join("AppData").join("Roaming");
    }
    std::env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_matter_dedupes_and_caps() {
        let mut s = DeskSettings::default();
        for i in 0..12 {
            s.remember_matter(&format!("m{i}"));
        }
        assert_eq!(s.recent_matters.len(), MAX_RECENT);
        assert_eq!(s.recent_matters[0], "m11");
        s.remember_matter("m5");
        assert_eq!(s.recent_matters[0], "m5");
        assert_eq!(s.recent_matters.iter().filter(|p| *p == "m5").count(), 1);
    }

    #[test]
    fn actor_defaults_to_desk_when_empty() {
        let mut s = DeskSettings::default();
        assert_eq!(s.actor(), "desk");
        s.reviewer_name = "  ".into();
        assert_eq!(s.actor(), "desk");
        s.reviewer_name = "  alice  ".into();
        assert_eq!(s.actor(), "alice");
    }
}
