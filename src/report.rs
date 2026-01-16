use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::models::LogEntry;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ReportPeriod {
    #[default]
    Today,
    Yesterday,
    Week,
}

impl ReportPeriod {
    pub fn label(&self) -> &'static str {
        match self {
            ReportPeriod::Today => "今日报告",
            ReportPeriod::Yesterday => "昨日报告",
            ReportPeriod::Week => "本周报告",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            ReportPeriod::Today => ReportPeriod::Yesterday,
            ReportPeriod::Yesterday => ReportPeriod::Week,
            ReportPeriod::Week => ReportPeriod::Today,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            ReportPeriod::Today => ReportPeriod::Week,
            ReportPeriod::Yesterday => ReportPeriod::Today,
            ReportPeriod::Week => ReportPeriod::Yesterday,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportContext {
    pub period: String,
    pub total_logs: usize,
    pub error_count: usize,
    pub warn_count: usize,
    pub critical_errors: Vec<String>,
    pub active_modules: Vec<(String, u64)>,
    pub peak_hour: String,
    pub health_score: u16,
    pub time_range: String,
}

pub fn generate_report_context(logs: &[LogEntry], period: ReportPeriod) -> ReportContext {
    let now = Local::now().naive_local();
    let today_start = now.date().and_hms_opt(0, 0, 0).unwrap();

    let (start_time, end_time, period_label) = match period {
        ReportPeriod::Today => (today_start, now, "今日"),
        ReportPeriod::Yesterday => {
            let yesterday = today_start - chrono::Duration::days(1);
            (yesterday, today_start, "昨日")
        }
        ReportPeriod::Week => {
            let week_ago = today_start - chrono::Duration::days(7);
            (week_ago, now, "本周")
        }
    };

    let filtered: Vec<&LogEntry> = logs
        .iter()
        .filter(|log| {
            if let Ok(ts) = NaiveDateTime::parse_from_str(&log.timestamp, "%Y-%m-%d %H:%M:%S%.f") {
                ts >= start_time && ts < end_time
            } else if let Ok(ts) = NaiveDateTime::parse_from_str(&log.timestamp, "%Y-%m-%d %H:%M:%S") {
                ts >= start_time && ts < end_time
            } else {
                true
            }
        })
        .collect();

    let mut error_count = 0;
    let mut warn_count = 0;
    let mut source_counts: HashMap<String, u64> = HashMap::new();
    let mut error_patterns: HashMap<String, u64> = HashMap::new();
    let mut hour_counts: HashMap<String, u64> = HashMap::new();

    for log in &filtered {
        let level = log.level.to_lowercase();
        if level.contains("error") {
            error_count += 1;
            let pattern = extract_error_signature(&log.content);
            *error_patterns.entry(pattern).or_insert(0) += 1;
        } else if level.contains("warn") {
            warn_count += 1;
        }

        *source_counts.entry(log.source_file.clone()).or_insert(0) += 1;

        if log.timestamp.len() >= 13 {
            let hour = format!("{}:00", &log.timestamp[11..13]);
            *hour_counts.entry(hour).or_insert(0) += 1;
        }
    }

    let mut top_sources: Vec<_> = source_counts.into_iter().collect();
    top_sources.sort_by(|a, b| b.1.cmp(&a.1));
    top_sources.truncate(5);

    let mut top_errors: Vec<_> = error_patterns.into_iter().collect();
    top_errors.sort_by(|a, b| b.1.cmp(&a.1));
    top_errors.truncate(5);
    let critical_errors: Vec<String> = top_errors
        .into_iter()
        .map(|(pattern, count)| format!("{} ({}次)", pattern, count))
        .collect();

    let peak_hour = hour_counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(h, _)| h)
        .unwrap_or_else(|| "N/A".into());

    let health_score = 100u16
        .saturating_sub((error_count / 10) as u16)
        .saturating_sub((warn_count / 50) as u16)
        .max(50);

    let time_range = format!(
        "{} ~ {}",
        start_time.format("%m-%d %H:%M"),
        end_time.format("%m-%d %H:%M")
    );

    ReportContext {
        period: period_label.into(),
        total_logs: filtered.len(),
        error_count,
        warn_count,
        critical_errors,
        active_modules: top_sources,
        peak_hour,
        health_score,
        time_range,
    }
}

fn extract_error_signature(content: &str) -> String {
    let content = content.trim();
    if content.len() > 60 {
        content[..60].to_string()
    } else {
        content.to_string()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportCache {
    pub today: Option<String>,
    pub yesterday: Option<String>,
    pub week: Option<String>,
}

impl ReportCache {
    fn file_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".loginsight")
            .join("reports.json")
    }

    pub fn load() -> Self {
        let path = Self::file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }

    pub fn get(&self, period: ReportPeriod) -> Option<&String> {
        match period {
            ReportPeriod::Today => self.today.as_ref(),
            ReportPeriod::Yesterday => self.yesterday.as_ref(),
            ReportPeriod::Week => self.week.as_ref(),
        }
    }

    pub fn set(&mut self, period: ReportPeriod, content: String) {
        match period {
            ReportPeriod::Today => self.today = Some(content),
            ReportPeriod::Yesterday => self.yesterday = Some(content),
            ReportPeriod::Week => self.week = Some(content),
        }
        self.save();
    }
}
