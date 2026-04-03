//! Search criteria and log level definitions for advanced filtering
//!
//! This module provides the data structures for multi-condition search:
//! - `LogLevel` enum for type-safe level filtering
//! - `SearchCriteria` struct for combining multiple filter conditions

use chrono::{DateTime, Local};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Parsed regex query representation.
///
/// `Single` means one regex must match; `And` means all regexes must match.
#[derive(Debug, Clone)]
pub enum ParsedRegexQuery {
    Single(Regex),
    And(Vec<Regex>),
}

/// Split a regex query by unescaped `&` (AND delimiter).
///
/// Returns `None` when no unescaped `&` delimiter exists.
/// Empty terms around delimiters are ignored.
pub fn split_unescaped_and_terms(pattern: &str) -> Option<Vec<String>> {
    let mut has_delimiter = false;
    let mut escaped = false;
    let mut current = String::new();
    let mut terms = Vec::new();

    for ch in pattern.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            current.push(ch);
            continue;
        }

        if ch == '&' {
            has_delimiter = true;
            let term = current.trim();
            if !term.is_empty() {
                terms.push(term.to_string());
            }
            current.clear();
            continue;
        }

        current.push(ch);
    }

    let term = current.trim();
    if !term.is_empty() {
        terms.push(term.to_string());
    }

    if has_delimiter {
        Some(terms)
    } else {
        None
    }
}

/// Parse a regex query.
///
/// - No `&`: compile as a single regex.
/// - Has unescaped `&`: compile as AND terms.
pub fn parse_regex_query(pattern: &str) -> Result<ParsedRegexQuery, regex::Error> {
    if let Some(terms) = split_unescaped_and_terms(pattern) {
        let mut compiled = Vec::new();
        for term in terms {
            compiled.push(Regex::new(&term)?);
        }
        if compiled.is_empty() {
            Ok(ParsedRegexQuery::And(Vec::new()))
        } else if compiled.len() == 1 {
            Ok(ParsedRegexQuery::Single(compiled.remove(0)))
        } else {
            Ok(ParsedRegexQuery::And(compiled))
        }
    } else {
        Ok(ParsedRegexQuery::Single(Regex::new(pattern)?))
    }
}

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
    /// Reserved for future use (e.g., parsing level from config or CLI)
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)] // Used in tests
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

    /// Build compiled regex query from content_regex string.
    pub fn compile_content_query(&self) -> Option<ParsedRegexQuery> {
        self.content_regex.as_ref().and_then(|p| {
            parse_regex_query(p).ok().and_then(|query| {
                if matches!(&query, ParsedRegexQuery::And(terms) if terms.is_empty()) {
                    None
                } else {
                    Some(query)
                }
            })
        })
    }

    /// Builder method: set start time
    #[allow(dead_code)] // Used in tests, part of public builder API
    pub fn with_start_time(mut self, time: DateTime<Local>) -> Self {
        self.start_time = Some(time);
        self
    }

    /// Builder method: set end time
    #[allow(dead_code)] // Used in tests, part of public builder API
    pub fn with_end_time(mut self, time: DateTime<Local>) -> Self {
        self.end_time = Some(time);
        self
    }

    /// Builder method: set content regex
    #[allow(dead_code)] // Used in tests, part of public builder API
    pub fn with_content_regex(mut self, pattern: &str) -> Self {
        self.content_regex = Some(pattern.to_string());
        self
    }

    /// Builder method: set source file filter
    #[allow(dead_code)] // Used in tests, part of public builder API
    pub fn with_source_file(mut self, source: &str) -> Self {
        self.source_file = Some(source.to_string());
        self
    }

    /// Builder method: add a log level to filter
    #[allow(dead_code)] // Used in tests, part of public builder API
    pub fn with_level(mut self, level: LogLevel) -> Self {
        if !self.levels.contains(&level) {
            self.levels.push(level);
        }
        self
    }

    /// Builder method: set multiple log levels
    #[allow(dead_code)] // Used in tests, part of public builder API
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

    #[test]
    fn parse_regex_query_builds_and_terms() {
        let query = parse_regex_query(r"error & timeout").unwrap();
        match query {
            ParsedRegexQuery::And(terms) => assert_eq!(terms.len(), 2),
            ParsedRegexQuery::Single(_) => panic!("expected AND terms"),
        }
    }

    #[test]
    fn split_unescaped_and_terms_uses_ampersand_delimiter() {
        let terms = split_unescaped_and_terms(r"error & timeout").unwrap();
        assert_eq!(terms, vec!["error".to_string(), "timeout".to_string()]);
    }

    #[test]
    fn split_unescaped_and_terms_keeps_space_when_no_ampersand() {
        // Spaces should remain part of a single regex term.
        assert_eq!(split_unescaped_and_terms("error timeout"), None);
    }

    #[test]
    fn split_unescaped_and_terms_supports_escaped_ampersand() {
        assert_eq!(split_unescaped_and_terms(r"error\&timeout"), None);
    }
}
