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
}

impl DeskSettings {
    pub fn load() -> Self {
        let path = settings_path();
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
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
}
