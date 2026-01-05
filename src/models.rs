use ratatui::prelude::Color;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub id: usize,
    pub name: String,
    pub color: Color,
    pub enabled: bool,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum Focus {
    #[default]
    LogList,
    FileList,
}

#[derive(Debug, Serialize, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub pid: String,
    pub tid: String,
    pub level: String,
    pub content: String,
    pub source_file: String,
    pub line_num: u32,
    pub json_payload: Option<Value>,
    pub delta_ms: Option<i64>,
    pub source_id: usize,
}

#[derive(Debug, Clone)]
pub enum DisplayEntry {
    Normal(LogEntry),
    Folded {
        start_index: usize,
        end_index: usize,
        count: usize,
        summary_text: String,
    },
}

impl DisplayEntry {
    pub fn get_tid(&self) -> Option<&str> {
        match self {
            DisplayEntry::Normal(log) => Some(&log.tid),
            _ => None,
        }
    }
    pub fn get_searchable_text(&self) -> String {
        match self {
            DisplayEntry::Normal(log) => format!("{} {} {}", log.content, log.source_file, log.tid),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        }
    }
    pub fn get_delta_ms(&self) -> Option<i64> {
        match self {
            DisplayEntry::Normal(log) => log.delta_ms,
            _ => None,
        }
    }
    pub fn get_content(&self) -> String {
        match self {
            DisplayEntry::Normal(log) => {
                format!("{} [{}][{}]: {}", log.timestamp, log.tid, log.level, log.content)
            }
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        }
    }
    pub fn get_source_id(&self) -> Option<usize> {
        match self {
            DisplayEntry::Normal(log) => Some(log.source_id),
            _ => None,
        }
    }
}

#[derive(Default)]
pub enum AiState {
    #[default]
    Idle,
    Loading,
    Completed(String),
    Error(String),
}

#[derive(Clone)]
pub struct LevelVisibility {
    pub info: bool,
    pub warn: bool,
    pub error: bool,
    pub debug: bool,
}

impl Default for LevelVisibility {
    fn default() -> Self {
        Self { info: true, warn: true, error: true, debug: true }
    }
}
