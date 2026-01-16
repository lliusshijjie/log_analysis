//! Advanced filtering logic for log entries
//!
//! This module provides multi-condition filtering with AND semantics:
//! - Time range filtering (start_time, end_time)
//! - Log level filtering
//! - Source file filtering
//! - Content regex filtering
//!
//! All active conditions must match for an entry to pass the filter.

use crate::models::{DisplayEntry, LogEntry};
use crate::search::SearchCriteria;
use crate::time_parser::parse_log_timestamp;
use regex::Regex;

/// Filter log entries based on search criteria
///
/// All active conditions must match (AND logic).
/// Returns references to matching entries.
/// Note: Use `filter_logs_owned` when storing the result; this is kept for potential future use.
#[allow(dead_code)]
pub fn filter_logs<'a>(
    entries: &'a [DisplayEntry],
    criteria: &SearchCriteria,
) -> Vec<&'a DisplayEntry> {
    // Early return if no criteria set
    if criteria.is_empty() {
        return entries.iter().collect();
    }

    // Pre-compile regex for performance
    let content_re = criteria.compile_content_regex();

    entries
        .iter()
        .filter(|entry| matches_criteria(entry, criteria, &content_re))
        .collect()
}

/// Filter and clone matching entries
///
/// Same as `filter_logs` but returns owned entries.
/// Use this when you need to store the filtered result.
pub fn filter_logs_owned(
    entries: &[DisplayEntry],
    criteria: &SearchCriteria,
) -> Vec<DisplayEntry> {
    if criteria.is_empty() {
        return entries.to_vec();
    }

    let content_re = criteria.compile_content_regex();

    entries
        .iter()
        .filter(|entry| matches_criteria(entry, criteria, &content_re))
        .cloned()
        .collect()
}

/// Check if a single entry matches all active criteria
fn matches_criteria(
    entry: &DisplayEntry,
    criteria: &SearchCriteria,
    content_re: &Option<Regex>,
) -> bool {
    match entry {
        DisplayEntry::Normal(log) => matches_log_entry(log, criteria, content_re),
        DisplayEntry::Folded { summary_text, .. } => {
            // For folded entries, only check content regex if present
            // Time and level filters don't apply to folded entries
            if let Some(re) = content_re {
                if !re.is_match(summary_text) {
                    return false;
                }
            }
            true
        }
    }
}

/// Check if a LogEntry matches all active criteria (AND logic)
///
/// Each check returns false immediately if the condition fails,
/// implementing short-circuit evaluation for performance.
fn matches_log_entry(
    log: &LogEntry,
    criteria: &SearchCriteria,
    content_re: &Option<Regex>,
) -> bool {
    // 1. Time range check - start time (inclusive)
    if let Some(ref start) = criteria.start_time {
        if let Some(ts) = parse_log_timestamp(&log.timestamp) {
            if ts < *start {
                return false;
            }
        }
        // If timestamp can't be parsed, skip this check (be lenient)
    }

    // 2. Time range check - end time (inclusive)
    if let Some(ref end) = criteria.end_time {
        if let Some(ts) = parse_log_timestamp(&log.timestamp) {
            if ts > *end {
                return false;
            }
        }
    }

    // 3. Level check - entry must match at least one of the specified levels
    if !criteria.levels.is_empty() {
        let matches_level = criteria.levels.iter().any(|level| level.matches(&log.level));
        if !matches_level {
            return false;
        }
    }

    // 4. Source file check (case-insensitive contains)
    if let Some(ref source) = criteria.source_file {
        if !log
            .source_file
            .to_lowercase()
            .contains(&source.to_lowercase())
        {
            return false;
        }
    }

    // 5. Content regex check
    if let Some(ref re) = content_re {
        if !re.is_match(&log.content) {
            return false;
        }
    }

    // All checks passed
    true
}

/// Count entries matching criteria without allocating a result vector
/// Reserved for future use (e.g., status bar display)
#[allow(dead_code)]
pub fn count_matching(entries: &[DisplayEntry], criteria: &SearchCriteria) -> usize {
    if criteria.is_empty() {
        return entries.len();
    }

    let content_re = criteria.compile_content_regex();

    entries
        .iter()
        .filter(|entry| matches_criteria(entry, criteria, &content_re))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::LogLevel;
    use serde_json::Value;

    fn make_test_log(timestamp: &str, level: &str, content: &str, source: &str) -> DisplayEntry {
        DisplayEntry::Normal(LogEntry {
            timestamp: timestamp.to_string(),
            pid: "1234".to_string(),
            tid: "5678".to_string(),
            level: level.to_string(),
            content: content.to_string(),
            source_file: source.to_string(),
            line_num: 1,
            json_payload: None,
            delta_ms: None,
            source_id: 0,
            line_index: 0,
        })
    }

    #[test]
    fn test_empty_criteria_returns_all() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "INFO", "test message", "test.rs"),
            make_test_log("2024-01-15 10:00:01.000", "ERROR", "error message", "test.rs"),
        ];
        let criteria = SearchCriteria::default();
        let result = filter_logs(&entries, &criteria);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_level_filter_single() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "INFO", "info msg", "test.rs"),
            make_test_log("2024-01-15 10:00:01.000", "ERROR", "error msg", "test.rs"),
            make_test_log("2024-01-15 10:00:02.000", "WARN", "warn msg", "test.rs"),
        ];
        let criteria = SearchCriteria {
            levels: vec![LogLevel::Error],
            ..Default::default()
        };
        let result = filter_logs(&entries, &criteria);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_level_filter_multiple() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "INFO", "info msg", "test.rs"),
            make_test_log("2024-01-15 10:00:01.000", "ERROR", "error msg", "test.rs"),
            make_test_log("2024-01-15 10:00:02.000", "WARN", "warn msg", "test.rs"),
        ];
        let criteria = SearchCriteria {
            levels: vec![LogLevel::Error, LogLevel::Warn],
            ..Default::default()
        };
        let result = filter_logs(&entries, &criteria);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_content_regex_filter() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "INFO", "user login success", "auth.rs"),
            make_test_log("2024-01-15 10:00:01.000", "ERROR", "database error", "db.rs"),
            make_test_log("2024-01-15 10:00:02.000", "INFO", "user logout", "auth.rs"),
        ];
        let criteria = SearchCriteria {
            content_regex: Some("user".to_string()),
            ..Default::default()
        };
        let result = filter_logs(&entries, &criteria);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_source_file_filter() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "INFO", "msg1", "auth.rs"),
            make_test_log("2024-01-15 10:00:01.000", "INFO", "msg2", "database.rs"),
            make_test_log("2024-01-15 10:00:02.000", "INFO", "msg3", "Auth.rs"),
        ];
        let criteria = SearchCriteria {
            source_file: Some("auth".to_string()),
            ..Default::default()
        };
        let result = filter_logs(&entries, &criteria);
        // Case-insensitive, should match both auth.rs and Auth.rs
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_combined_filters_and_logic() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "ERROR", "database error", "db.rs"),
            make_test_log("2024-01-15 10:00:01.000", "ERROR", "auth error", "auth.rs"),
            make_test_log("2024-01-15 10:00:02.000", "INFO", "database info", "db.rs"),
        ];
        // Must be ERROR level AND from db.rs
        let criteria = SearchCriteria {
            levels: vec![LogLevel::Error],
            source_file: Some("db".to_string()),
            ..Default::default()
        };
        let result = filter_logs(&entries, &criteria);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_folded_entry_content_filter() {
        let entries = vec![
            DisplayEntry::Folded {
                start_index: 0,
                end_index: 5,
                count: 6,
                summary_text: "Folded 6 USB polling".to_string(),
            },
            DisplayEntry::Folded {
                start_index: 6,
                end_index: 10,
                count: 5,
                summary_text: "Folded 5 identical".to_string(),
            },
        ];
        let criteria = SearchCriteria {
            content_regex: Some("USB".to_string()),
            ..Default::default()
        };
        let result = filter_logs(&entries, &criteria);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_count_matching() {
        let entries = vec![
            make_test_log("2024-01-15 10:00:00.000", "INFO", "msg1", "test.rs"),
            make_test_log("2024-01-15 10:00:01.000", "ERROR", "msg2", "test.rs"),
        ];
        let criteria = SearchCriteria {
            levels: vec![LogLevel::Error],
            ..Default::default()
        };
        assert_eq!(count_matching(&entries, &criteria), 1);
    }
}
