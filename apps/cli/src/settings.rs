use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppSettings {
    pub library_dir: String,
    pub generated_dir: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        AppSettings {
            library_dir: format!("{home}/Music/Musicum"),
            generated_dir: None,
        }
    }
}

pub fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home)
        .join(".config")
        .join("com.musicum.app")
        .join("settings.json")
}

pub fn load() -> Result<AppSettings> {
    let path = settings_path();
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading settings from {}", path.display()))?;
    serde_json::from_str(&text).context("parsing settings JSON")
}

#[allow(dead_code)]
pub fn save(settings: &AppSettings) -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(&path, json)
        .with_context(|| format!("writing settings to {}", path.display()))
}
