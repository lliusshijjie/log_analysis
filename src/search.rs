//! Search criteria and log level definitions for advanced filtering
//!
//! This module provides the data structures for multi-condition search:
//! - `LogLevel` enum for type-safe level filtering
//! - `SearchCriteria` struct for combining multiple filter conditions

use chrono::{DateTime, Local};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Log level enum for type-safe level filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse level from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" | "warning" => Some(LogLevel::Warn),
            "error" | "err" => Some(LogLevel::Error),
            _ => None,
        }
    }

    /// Check if a level string matches this LogLevel
    pub fn matches(&self, level_str: &str) -> bool {
        let lower = level_str.to_lowercase();
        match self {
            LogLevel::Debug => lower.contains("debug"),
            LogLevel::Info => lower.contains("info"),
            LogLevel::Warn => lower.contains("warn"),
            LogLevel::Error => lower.contains("error") || lower.contains("err"),
        }
    }
}

/// Search criteria for advanced filtering
///
/// All active conditions are combined with AND logic.
/// An empty criteria (all fields None/empty) matches everything.
#[derive(Debug, Clone, Default)]
pub struct SearchCriteria {
    /// Filter logs after this time (inclusive)
    pub start_time: Option<DateTime<Local>>,
    /// Filter logs before this time (inclusive)
    pub end_time: Option<DateTime<Local>>,
    /// Regex pattern for content search
    pub content_regex: Option<String>,
    /// Filter by source file name (contains match)
    pub source_file: Option<String>,
    /// Allowed log levels (empty = all levels)
    pub levels: Vec<LogLevel>,
}

/// Serializable version of SearchCriteria for saving templates
/// Uses string representation for times since DateTime doesn't serialize well
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SerializableSearchCriteria {
    /// Start time as string (e.g., "10:30:00" or "-1h")
    pub start_time: Option<String>,
    /// End time as string
    pub end_time: Option<String>,
    /// Regex pattern for content search
    pub content_regex: Option<String>,
    /// Filter by source file name
    pub source_file: Option<String>,
    /// Allowed log levels
    pub levels: Vec<LogLevel>,
}

/// A named search template for saving/loading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTemplate {
    /// Name of the template
    pub name: String,
    /// The search criteria
    pub criteria: SerializableSearchCriteria,
}

impl SearchTemplate {
    /// Create a new search template
    pub fn new(name: String, criteria: SerializableSearchCriteria) -> Self {
        Self { name, criteria }
    }
}

impl SearchCriteria {
    /// Create a new empty SearchCriteria
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any criteria is active
    pub fn is_empty(&self) -> bool {
        self.start_time.is_none()
            && self.end_time.is_none()
            && self.content_regex.is_none()
            && self.source_file.is_none()
            && self.levels.is_empty()
    }

    /// Build compiled regex from content_regex string
    pub fn compile_content_regex(&self) -> Option<Regex> {
        self.content_regex
            .as_ref()
            .and_then(|p| Regex::new(p).ok())
    }

    /// Builder method: set start time
    pub fn with_start_time(mut self, time: DateTime<Local>) -> Self {
        self.start_time = Some(time);
        self
    }

    /// Builder method: set end time
    pub fn with_end_time(mut self, time: DateTime<Local>) -> Self {
        self.end_time = Some(time);
        self
    }

    /// Builder method: set content regex
    pub fn with_content_regex(mut self, pattern: &str) -> Self {
        self.content_regex = Some(pattern.to_string());
        self
    }

    /// Builder method: set source file filter
    pub fn with_source_file(mut self, source: &str) -> Self {
        self.source_file = Some(source.to_string());
        self
    }

    /// Builder method: add a log level to filter
    pub fn with_level(mut self, level: LogLevel) -> Self {
        if !self.levels.contains(&level) {
            self.levels.push(level);
        }
        self
    }

    /// Builder method: set multiple log levels
    pub fn with_levels(mut self, levels: Vec<LogLevel>) -> Self {
        self.levels = levels;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from_str("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("Warning"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("ERROR"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("unknown"), None);
    }

    #[test]
    fn test_log_level_matches() {
        assert!(LogLevel::Error.matches("ERROR"));
        assert!(LogLevel::Error.matches("Error"));
        assert!(LogLevel::Error.matches("err"));
        assert!(!LogLevel::Error.matches("INFO"));
    }

    #[test]
    fn test_search_criteria_default_is_empty() {
        let criteria = SearchCriteria::default();
        assert!(criteria.is_empty());
    }

    #[test]
    fn test_search_criteria_builder() {
        let criteria = SearchCriteria::new()
            .with_content_regex("test")
            .with_level(LogLevel::Error);

        assert!(!criteria.is_empty());
        assert_eq!(criteria.content_regex, Some("test".to_string()));
        assert_eq!(criteria.levels, vec![LogLevel::Error]);
    }
}
