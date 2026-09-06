//! Local app config: optional X-App-Key, last license key, last token.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Optional app key sent as X-App-Key. Empty = omit header.
    #[serde(default)]
    pub app_key: String,
    #[serde(default)]
    pub last_key: String,
    /// Last delivered / pasted token line (username----token). Never logged in full.
    #[serde(default)]
    pub last_token: String,
}

fn config_path() -> Result<PathBuf, String> {
    let dir = dirs::config_dir()
        .ok_or_else(|| "Could not resolve config directory.".to_string())?
        .join("nocheater");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("config.json"))
}

pub fn load() -> AppConfig {
    let Ok(path) = config_path() else {
        return AppConfig::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return AppConfig::default();
    };
    // Ignore unknown legacy keys (tokenChecks, steamApiKey, …).
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

pub fn mask_key(key: &str) -> String {
    let t = key.trim();
    if t.len() <= 6 {
        return "***".to_string();
    }
    format!("{}…{}", &t[..3], &t[t.len() - 3..])
}

pub fn get_last_token() -> String {
    load().last_token
}

pub fn save_last_token(token: String) -> Result<(), String> {
    let mut cfg = load();
    cfg.last_token = token.trim().to_string();
    save(&cfg)
}

pub fn clear_last_token() {
    let mut cfg = load();
    cfg.last_token.clear();
    let _ = save(&cfg);
}
