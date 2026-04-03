use ratatui::prelude::Color;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub id: usize,
    pub name: String,
    pub color: Color,
    pub enabled: bool,
    /// Whether the file is marked with a dot (pending activation)
    pub marked: bool,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum Focus {
    #[default]
    LogList,
    FileList,
}

#[derive(Default, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Editing variant reserved for inline edit mode
pub enum InputMode {
    #[default]
    Normal,
    Editing,
    JumpInput,
    AiPromptInput,
    ChatInput,
    ReportSaveInput,
    FocusCopyInput,
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
    pub line_index: usize,
    #[serde(skip_serializing)]
    pub level_kind: LogLevelKind,
    #[serde(skip_serializing)]
    pub source_file_lower: String,
    #[serde(skip_serializing)]
    pub searchable_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevelKind {
    Info,
    Warn,
    Error,
    Debug,
    Other,
}

impl LogLevelKind {
    pub fn from_level(level: &str) -> Self {
        let level_lower = level.to_ascii_lowercase();
        if level_lower.contains("error") {
            Self::Error
        } else if level_lower.contains("warn") {
            Self::Warn
        } else if level_lower.contains("debug") {
            Self::Debug
        } else if level_lower.contains("info") {
            Self::Info
        } else {
            Self::Other
        }
    }
}

impl LogEntry {
    pub fn build_searchable_text(content: &str, source_file: &str, tid: &str) -> String {
        let mut text = String::with_capacity(content.len() + source_file.len() + tid.len() + 2);
        text.push_str(content);
        text.push(' ');
        text.push_str(source_file);
        text.push(' ');
        text.push_str(tid);
        text
    }
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
    pub fn searchable_text(&self) -> &str {
        match self {
            DisplayEntry::Normal(log) => log.searchable_text.as_str(),
            DisplayEntry::Folded { summary_text, .. } => summary_text.as_str(),
        }
    }

    pub fn matches_search(&self, re: &Regex) -> bool {
        re.is_match(self.searchable_text())
    }

    /// Reserved for future use (e.g., performance analysis display)
    #[allow(dead_code)]
    pub fn get_delta_ms(&self) -> Option<i64> {
        match self {
            DisplayEntry::Normal(log) => log.delta_ms,
            _ => None,
        }
    }
    pub fn get_content(&self) -> String {
        match self {
            DisplayEntry::Normal(log) => {
                format!(
                    "{} [{}][{}]: {}",
                    log.timestamp, log.tid, log.level, log.content
                )
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
    pub fn get_line_index(&self) -> Option<usize> {
        match self {
            DisplayEntry::Normal(log) => Some(log.line_index),
            _ => None,
        }
    }
    pub fn get_line_num(&self) -> Option<u32> {
        match self {
            DisplayEntry::Normal(log) => Some(log.line_num),
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
        Self {
            info: true,
            warn: true,
            error: true,
            debug: true,
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum CurrentView {
    #[default]
    Logs,
    Dashboard,
    Chat,
    History,
    Report,
    Focus,
    Thread,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // System variant used internally by AI client
pub enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChatContext {
    pub pinned_logs: Vec<LogEntry>,
}

#[derive(Clone, Default, Serialize)]
pub struct DashboardStats {
    pub total_logs: usize,
    pub error_count: usize,
    pub warn_count: usize,
    pub info_count: usize,
    pub log_duration: String,
    pub error_trend: Vec<(String, u64)>,
    pub top_sources: Vec<(String, u64)>,
    pub top_threads: Vec<(String, u64)>,
    pub health_score: u16,
    pub sparkline_data: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportType {
    LogsCsv,
    LogsJson,
    Report,
    AiAnalysis,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportState {
    Idle,
    Confirm(ExportType),
    Exporting(ExportType),
    Success(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ExportResult {
    Success(String),
    Error(String),
}
