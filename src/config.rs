use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "log_config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub parser: ParserConfig,
    pub filters: FiltersConfig,
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserConfig {
    pub log_pattern: String,
    pub timestamp_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiltersConfig {
    pub fold_threshold: usize,
    pub fold_rules: Vec<FoldRule>,
    pub ignore_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldRule {
    pub name: String,
    pub match_type: String,
    pub patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub slow_threshold_ms: i64,
    pub very_slow_threshold_ms: i64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            parser: ParserConfig::default(),
            filters: FiltersConfig::default(),
            theme: ThemeConfig::default(),
        }
    }
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            log_pattern: r"^(\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}\.\d+)\[([0-9a-f]+):([0-9a-f]+)\]\[(\w+)\]:\s*(.*)\((.+):(\d+)\)\s*$".into(),
            timestamp_format: "%Y-%m-%d %H:%M:%S%.3f".into(),
        }
    }
}

impl Default for FiltersConfig {
    fn default() -> Self {
        Self {
            fold_threshold: 3,
            fold_rules: vec![
                FoldRule {
                    name: "USB polling".into(),
                    match_type: "source_file".into(),
                    patterns: vec!["UsbCtrl".into(), "EnumDevice".into()],
                },
                FoldRule {
                    name: "thread cleanup".into(),
                    match_type: "content".into(),
                    patterns: vec!["DestroyThread".into(), "Terminate".into()],
                },
            ],
            ignore_patterns: vec![],
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            slow_threshold_ms: 100,
            very_slow_threshold_ms: 1000,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = Path::new(CONFIG_FILE);
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let config: AppConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            let content = toml::to_string_pretty(&config)?;
            fs::write(path, content)?;
            Ok(config)
        }
    }
}
