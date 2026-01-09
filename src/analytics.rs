use std::collections::HashMap;

use crate::models::{DashboardStats, LogEntry};

pub fn compute_dashboard_stats(logs: &[LogEntry]) -> DashboardStats {
    if logs.is_empty() {
        return DashboardStats::default();
    }

    let mut error_count = 0;
    let mut warn_count = 0;
    let mut info_count = 0;
    let mut source_counts: HashMap<String, u64> = HashMap::new();
    let mut thread_counts: HashMap<String, u64> = HashMap::new();
    let mut error_by_hour: HashMap<String, u64> = HashMap::new();

    for log in logs {
        let level = log.level.to_lowercase();
        if level.contains("error") {
            error_count += 1;
            let hour_key = if log.timestamp.len() >= 13 {
                format!(
                    "{}-{} {}:00",
                    &log.timestamp[5..7],
                    &log.timestamp[8..10],
                    &log.timestamp[11..13]
                )
            } else {
                log.timestamp.clone()
            };
            *error_by_hour.entry(hour_key).or_insert(0) += 1;
        } else if level.contains("warn") {
            warn_count += 1;
        } else if level.contains("info") {
            info_count += 1;
        }

        *source_counts.entry(log.source_file.clone()).or_insert(0) += 1;
        *thread_counts.entry(log.tid.clone()).or_insert(0) += 1;
    }

    let mut top_sources: Vec<_> = source_counts.into_iter().collect();
    top_sources.sort_by(|a, b| b.1.cmp(&a.1));
    top_sources.truncate(5);

    let mut top_threads: Vec<_> = thread_counts.into_iter().collect();
    top_threads.sort_by(|a, b| b.1.cmp(&a.1));
    top_threads.truncate(5);

    let mut error_trend: Vec<_> = error_by_hour.into_iter().collect();
    error_trend.sort_by(|a, b| a.0.cmp(&b.0));
    error_trend = error_trend.into_iter().rev().take(12).collect();
    error_trend.reverse();

    let first_ts = &logs
        .first()
        .map(|l| l.timestamp.clone())
        .unwrap_or_default();
    let last_ts = &logs.last().map(|l| l.timestamp.clone()).unwrap_or_default();
    let log_duration = if first_ts.len() >= 16 && last_ts.len() >= 16 {
        format!("{} ~ {}", &first_ts[11..16], &last_ts[11..16])
    } else {
        "N/A".into()
    };

    // Calculate health score (50-100, never below 50)
    let health_score = 100u16
        .saturating_sub((error_count / 10) as u16)
        .saturating_sub((warn_count / 50) as u16)
        .max(50);

    // Generate sparkline data from error trend
    let sparkline_data: Vec<u64> = error_trend.iter().map(|(_, v)| *v).collect();

    DashboardStats {
        total_logs: logs.len(),
        error_count,
        warn_count,
        info_count,
        log_duration,
        error_trend,
        top_sources,
        top_threads,
        health_score,
        sparkline_data,
    }
}
