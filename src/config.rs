use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default = "default_layout_pair")]
    pub layout_pair: [String; 2],
    #[serde(default = "default_double_shift_timeout_ms")]
    pub double_shift_timeout_ms: u64,
    #[serde(default = "default_max_shift_hold_ms")]
    pub max_shift_hold_ms: u64,
    #[serde(default = "default_buffer_len")]
    pub buffer_len: usize,
    #[serde(default = "default_post_switch_delay_ms")]
    pub post_switch_delay_ms: u64,
    #[serde(default = "default_enable_selected_text")]
    pub enable_selected_text: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub path: PathBuf,
    pub layout_pair: [String; 2],
    pub double_shift_timeout_ms: u64,
    pub max_shift_hold_ms: u64,
    pub buffer_len: usize,
    pub post_switch_delay_ms: u64,
    pub enable_selected_text: bool,
    pub log_level: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            path: default_config_path(),
            layout_pair: default_layout_pair(),
            double_shift_timeout_ms: default_double_shift_timeout_ms(),
            max_shift_hold_ms: default_max_shift_hold_ms(),
            buffer_len: default_buffer_len(),
            post_switch_delay_ms: default_post_switch_delay_ms(),
            enable_selected_text: default_enable_selected_text(),
            log_level: default_log_level(),
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        let config_path = path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_config_path);
        if !config_path.exists() {
            let mut config = Self::default();
            config.path = config_path;
            return Ok(config);
        }

        let contents = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let parsed: ConfigFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        Ok(Self {
            path: config_path,
            layout_pair: parsed.layout_pair,
            double_shift_timeout_ms: parsed.double_shift_timeout_ms,
            max_shift_hold_ms: parsed.max_shift_hold_ms,
            buffer_len: parsed.buffer_len,
            post_switch_delay_ms: parsed.post_switch_delay_ms,
            enable_selected_text: parsed.enable_selected_text,
            log_level: parsed.log_level,
        })
    }

    pub fn save(&self) -> Result<()> {
        self.ensure_parent_dir()?;
        let file = ConfigFile {
            layout_pair: self.layout_pair.clone(),
            double_shift_timeout_ms: self.double_shift_timeout_ms,
            max_shift_hold_ms: self.max_shift_hold_ms,
            buffer_len: self.buffer_len,
            post_switch_delay_ms: self.post_switch_delay_ms,
            enable_selected_text: self.enable_selected_text,
            log_level: self.log_level.clone(),
        };
        let serialized = toml::to_string_pretty(&file).context("failed to serialize config")?;
        fs::write(&self.path, serialized)
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        Ok(())
    }

    pub fn ensure_parent_dir(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(())
    }
}

fn default_config_path() -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("lang-switcher/config.toml")
}

fn default_layout_pair() -> [String; 2] {
    ["us".to_owned(), "ru".to_owned()]
}

fn default_double_shift_timeout_ms() -> u64 {
    300
}

fn default_max_shift_hold_ms() -> u64 {
    250
}

fn default_buffer_len() -> usize {
    96
}

fn default_post_switch_delay_ms() -> u64 {
    40
}

fn default_enable_selected_text() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_valid() {
        let config = AppConfig::default();
        assert_eq!(config.layout_pair, ["us".to_owned(), "ru".to_owned()]);
        assert!(config.double_shift_timeout_ms > 0);
    }

    #[test]
    fn config_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");

        let mut config = AppConfig::default();
        config.path = path.clone();
        config.log_level = "debug".to_owned();
        config.double_shift_timeout_ms = 123;
        config.save().unwrap();

        let loaded = AppConfig::load_or_default(Some(&path)).unwrap();
        assert_eq!(loaded.log_level, "debug");
        assert_eq!(loaded.double_shift_timeout_ms, 123);
    }
}
