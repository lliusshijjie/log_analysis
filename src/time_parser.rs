//! Time parsing utilities for user input and log timestamps
//!
//! This module provides flexible parsing for various time formats:
//! - Full datetime: "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DD HH:MM:SS.mmm"
//! - Date only: "YYYY-MM-DD" (assumes start of day)
//! - Time only: "HH:MM:SS" or "HH:MM:SS.mmm" (assumes today's date)

use chrono::{DateTime, Duration, Local, NaiveDateTime, NaiveTime, TimeZone};

/// Parse user input string into DateTime<Local>
///
/// Supported formats:
/// - "HH:MM:SS" or "HH:MM:SS.mmm" - assumes today's date
/// - "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DD HH:MM:SS.mmm" - full datetime
/// - "YYYY-MM-DD" - start of that day (00:00:00)
/// - "-1h", "-30m", "-2d" - relative time (hours, minutes, days ago)
/// - "+1h", "+30m" - relative time in the future
///
/// # Examples
/// ```
/// use log_insight_tui::time_parser::parse_user_time;
///
/// let dt = parse_user_time("2024-01-15 10:30:45");
/// assert!(dt.is_some());
///
/// let time_only = parse_user_time("10:30:45");
/// assert!(time_only.is_some());
///
/// // Relative time: 1 hour ago
/// let relative = parse_user_time("-1h");
/// assert!(relative.is_some());
/// ```
pub fn parse_user_time(input: &str) -> Option<DateTime<Local>> {
    let input = input.trim();

    if input.is_empty() {
        return None;
    }

    // Try relative time first (e.g., -1h, -30m, -2d, +1h)
    if let Some(dt) = parse_relative_time(input) {
        return Some(dt);
    }

    // Try full datetime with milliseconds
    if let Ok(dt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S%.3f") {
        return Local.from_local_datetime(&dt).single();
    }

    // Try full datetime without milliseconds
    if let Ok(dt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        return Local.from_local_datetime(&dt).single();
    }

    // Try date only (start of day)
    if let Ok(date) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        let dt = date.and_hms_opt(0, 0, 0)?;
        return Local.from_local_datetime(&dt).single();
    }

    // Try time only with milliseconds (assume today)
    if let Ok(time) = NaiveTime::parse_from_str(input, "%H:%M:%S%.3f") {
        let today = Local::now().date_naive();
        let dt = today.and_time(time);
        return Local.from_local_datetime(&dt).single();
    }

    // Try time only without milliseconds (assume today)
    if let Ok(time) = NaiveTime::parse_from_str(input, "%H:%M:%S") {
        let today = Local::now().date_naive();
        let dt = today.and_time(time);
        return Local.from_local_datetime(&dt).single();
    }

    // Try HH:MM format (no seconds)
    if let Ok(time) = NaiveTime::parse_from_str(input, "%H:%M") {
        let today = Local::now().date_naive();
        let dt = today.and_time(time);
        return Local.from_local_datetime(&dt).single();
    }

    None
}

/// Parse relative time strings like "-1h", "-30m", "-2d", "+1h"
///
/// Supported units:
/// - `s` or `sec` - seconds
/// - `m` or `min` - minutes  
/// - `h` or `hr` - hours
/// - `d` or `day` - days
fn parse_relative_time(input: &str) -> Option<DateTime<Local>> {
    let input = input.trim();
    
    // Must start with - or +
    if !input.starts_with('-') && !input.starts_with('+') {
        return None;
    }

    let is_negative = input.starts_with('-');
    let rest = &input[1..];

    // Find where the number ends and unit begins
    let num_end = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if num_end == 0 {
        return None;
    }

    let (num_str, unit) = rest.split_at(num_end);
    let num: i64 = num_str.parse().ok()?;
    let unit = unit.trim().to_lowercase();

    let duration = match unit.as_str() {
        "s" | "sec" | "second" | "seconds" => Duration::seconds(num),
        "m" | "min" | "minute" | "minutes" => Duration::minutes(num),
        "h" | "hr" | "hour" | "hours" => Duration::hours(num),
        "d" | "day" | "days" => Duration::days(num),
        "w" | "week" | "weeks" => Duration::weeks(num),
        _ => return None,
    };

    let now = Local::now();
    if is_negative {
        Some(now - duration)
    } else {
        Some(now + duration)
    }
}

/// Parse timestamp string from log entry into DateTime<Local>
///
/// Uses the log format: "YYYY-MM-DD HH:MM:SS.mmm"
/// This is optimized for parsing timestamps already in log entries.
pub fn parse_log_timestamp(ts: &str) -> Option<DateTime<Local>> {
    let ts = ts.trim();

    // Try with milliseconds first (most common in logs)
    if let Ok(dt) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.3f") {
        return Local.from_local_datetime(&dt).single();
    }

    // Fallback to without milliseconds
    if let Ok(dt) = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
        return Local.from_local_datetime(&dt).single();
    }

    None
}

/// Result type for time parsing with error message
/// Part of public API for external consumers
#[allow(dead_code)]
pub type TimeParseResult = Result<DateTime<Local>, String>;

/// Parse user time with detailed error message
#[allow(dead_code)] // Tested, part of public API
pub fn parse_user_time_result(input: &str) -> TimeParseResult {
    parse_user_time(input).ok_or_else(|| {
        format!(
            "无法解析时间: '{}'. 支持格式: HH:MM:SS, YYYY-MM-DD, YYYY-MM-DD HH:MM:SS",
            input
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_datetime_with_ms() {
        let result = parse_user_time("2024-01-15 10:30:45.123");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2024-01-15 10:30:45");
    }

    #[test]
    fn test_parse_full_datetime_without_ms() {
        let result = parse_user_time("2024-01-15 10:30:45");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.format("%H:%M:%S").to_string(), "10:30:45");
    }

    #[test]
    fn test_parse_date_only() {
        let result = parse_user_time("2024-01-15");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.format("%H:%M:%S").to_string(), "00:00:00");
    }

    #[test]
    fn test_parse_time_only() {
        let result = parse_user_time("10:30:45");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.format("%H:%M:%S").to_string(), "10:30:45");
    }

    #[test]
    fn test_parse_time_with_ms() {
        let result = parse_user_time("10:30:45.500");
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_time_hhmm() {
        let result = parse_user_time("10:30");
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.format("%H:%M:%S").to_string(), "10:30:00");
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_user_time("invalid").is_none());
        assert!(parse_user_time("").is_none());
        assert!(parse_user_time("   ").is_none());
    }

    #[test]
    fn test_parse_log_timestamp() {
        let result = parse_log_timestamp("2024-01-15 10:30:45.123");
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_user_time_result_error() {
        let result = parse_user_time_result("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("无法解析时间"));
    }

    #[test]
    fn test_parse_relative_time_hours() {
        let result = parse_user_time("-1h");
        assert!(result.is_some());
        let dt = result.unwrap();
        let now = Local::now();
        // Should be approximately 1 hour ago (within a few seconds)
        let diff = now.signed_duration_since(dt).num_minutes();
        assert!(diff >= 59 && diff <= 61);
    }

    #[test]
    fn test_parse_relative_time_minutes() {
        let result = parse_user_time("-30m");
        assert!(result.is_some());
        let dt = result.unwrap();
        let now = Local::now();
        let diff = now.signed_duration_since(dt).num_minutes();
        assert!(diff >= 29 && diff <= 31);
    }

    #[test]
    fn test_parse_relative_time_days() {
        let result = parse_user_time("-2d");
        assert!(result.is_some());
        let dt = result.unwrap();
        let now = Local::now();
        let diff = now.signed_duration_since(dt).num_days();
        assert_eq!(diff, 2);
    }

    #[test]
    fn test_parse_relative_time_future() {
        let result = parse_user_time("+1h");
        assert!(result.is_some());
        let dt = result.unwrap();
        let now = Local::now();
        let diff = dt.signed_duration_since(now).num_minutes();
        assert!(diff >= 59 && diff <= 61);
    }

    #[test]
    fn test_parse_relative_time_invalid_unit() {
        assert!(parse_user_time("-1x").is_none());
        assert!(parse_user_time("-abc").is_none());
    }
}
