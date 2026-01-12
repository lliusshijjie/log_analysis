use anyhow::Result;
use chrono::Local;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

use crate::models::{ChatMessage, DashboardStats, DisplayEntry, ExportType};

pub fn generate_filename(prefix: &str, extension: &str) -> String {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    format!("{}_export_{}.{}", prefix, timestamp, extension)
}

pub fn export_logs_to_csv(entries: &[DisplayEntry]) -> Result<String> {
    let filename = generate_filename("logs", "csv");
    let mut file = File::create(&filename)?;

    writeln!(
        file,
        "Timestamp,PID,TID,Level,Content,Source File,Line Number,Delta(ms)"
    )?;

    for entry in entries {
        if let DisplayEntry::Normal(log) = entry {
            let escaped_content = log.content.replace("\"", "\"\"");
            let delta = log.delta_ms.map(|d| d.to_string()).unwrap_or_default();
            writeln!(
                file,
                "\"{}\",\"{}\",\"{}\",\"{}\",\"{}\",\"{}\",{},{}",
                log.timestamp,
                log.pid,
                log.tid,
                log.level,
                escaped_content,
                log.source_file,
                log.line_num,
                delta
            )?;
        }
    }

    Ok(filename)
}

#[derive(Serialize)]
struct LogJson {
    timestamp: String,
    pid: String,
    tid: String,
    level: String,
    content: String,
    source_file: String,
    line_number: u32,
    delta_ms: Option<i64>,
    json_payload: Option<serde_json::Value>,
}

pub fn export_logs_to_json(entries: &[DisplayEntry]) -> Result<String> {
    let filename = generate_filename("logs", "json");
    let mut file = File::create(&filename)?;

    let logs: Vec<LogJson> = entries
        .iter()
        .filter_map(|entry| {
            if let DisplayEntry::Normal(log) = entry {
                Some(LogJson {
                    timestamp: log.timestamp.clone(),
                    pid: log.pid.clone(),
                    tid: log.tid.clone(),
                    level: log.level.clone(),
                    content: log.content.clone(),
                    source_file: log.source_file.clone(),
                    line_number: log.line_num,
                    delta_ms: log.delta_ms,
                    json_payload: log.json_payload.clone(),
                })
            } else {
                None
            }
        })
        .collect();

    let json = serde_json::to_string_pretty(&logs)?;
    file.write_all(json.as_bytes())?;

    Ok(filename)
}

#[derive(Serialize)]
struct ReportData {
    export_timestamp: String,
    summary: SummarySection,
    errors: Vec<ErrorSummary>,
    performance: PerformanceSection,
    sources: Vec<SourceSummary>,
}

#[derive(Serialize)]
struct SummarySection {
    total_logs: usize,
    error_count: usize,
    warn_count: usize,
    info_count: usize,
    debug_count: usize,
    log_duration: String,
    health_score: u16,
}

#[derive(Serialize)]
struct ErrorSummary {
    pattern: String,
    count: usize,
    first_occurrence: String,
    latest_occurrence: String,
}

#[derive(Serialize)]
struct PerformanceSection {
    avg_delta_ms: f64,
    max_delta_ms: i64,
    slow_operations: usize,      // > 100ms
    very_slow_operations: usize, // > 1s
}

#[derive(Serialize)]
struct SourceSummary {
    source_file: String,
    log_count: usize,
    error_count: usize,
}

pub fn export_report(entries: &[DisplayEntry], stats: &DashboardStats) -> Result<String> {
    let filename = generate_filename("report", "json");
    let mut file = File::create(&filename)?;

    let export_timestamp = Local::now().to_rfc3339();

    let total = stats.error_count + stats.warn_count + stats.info_count;
    let summary = SummarySection {
        total_logs: entries.len(),
        error_count: stats.error_count,
        warn_count: stats.warn_count,
        info_count: stats.info_count,
        debug_count: entries.len().saturating_sub(total),
        log_duration: stats.log_duration.clone(),
        health_score: stats.health_score,
    };

    let errors = extract_error_patterns(entries);

    let performance = extract_performance_stats(entries);

    let sources = extract_source_stats(entries);

    let report = ReportData {
        export_timestamp,
        summary,
        errors,
        performance,
        sources,
    };

    let json = serde_json::to_string_pretty(&report)?;
    file.write_all(json.as_bytes())?;

    Ok(filename)
}

fn extract_error_patterns(entries: &[DisplayEntry]) -> Vec<ErrorSummary> {
    let mut error_map: std::collections::HashMap<String, (usize, String, String)> =
        std::collections::HashMap::new();

    for entry in entries {
        if let DisplayEntry::Normal(log) = entry {
            if log.level.to_lowercase().contains("error") {
                let pattern = if log.content.len() > 100 {
                    format!("{}...", &log.content[..100])
                } else {
                    log.content.clone()
                };

                let count =
                    error_map
                        .entry(pattern.clone())
                        .or_insert((0, String::new(), String::new()));
                count.0 += 1;
                if count.1.is_empty() || log.timestamp < count.1 {
                    count.1 = log.timestamp.clone();
                }
                if count.2.is_empty() || log.timestamp > count.2 {
                    count.2 = log.timestamp.clone();
                }
            }
        }
    }

    let mut errors: Vec<ErrorSummary> = error_map
        .into_iter()
        .map(|(pattern, (count, first, latest))| ErrorSummary {
            pattern,
            count,
            first_occurrence: first,
            latest_occurrence: latest,
        })
        .collect();

    errors.sort_by(|a, b| b.count.cmp(&a.count));
    errors.truncate(20);

    errors
}

fn extract_performance_stats(entries: &[DisplayEntry]) -> PerformanceSection {
    let mut deltas: Vec<i64> = Vec::new();
    let mut slow_operations = 0;
    let mut very_slow_operations = 0;

    for entry in entries {
        if let DisplayEntry::Normal(log) = entry {
            if let Some(delta) = log.delta_ms {
                deltas.push(delta);
                if delta > 1000 {
                    very_slow_operations += 1;
                } else if delta > 100 {
                    slow_operations += 1;
                }
            }
        }
    }

    let avg_delta_ms = if !deltas.is_empty() {
        deltas.iter().sum::<i64>() as f64 / deltas.len() as f64
    } else {
        0.0
    };

    let max_delta_ms = *deltas.iter().max().unwrap_or(&0);

    PerformanceSection {
        avg_delta_ms,
        max_delta_ms,
        slow_operations,
        very_slow_operations,
    }
}

fn extract_source_stats(entries: &[DisplayEntry]) -> Vec<SourceSummary> {
    let mut source_map: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();

    for entry in entries {
        if let DisplayEntry::Normal(log) = entry {
            let count = source_map.entry(log.source_file.clone()).or_insert((0, 0));
            count.0 += 1;
            if log.level.to_lowercase().contains("error") {
                count.1 += 1;
            }
        }
    }

    let mut sources: Vec<SourceSummary> = source_map
        .into_iter()
        .map(|(source_file, (log_count, error_count))| SourceSummary {
            source_file,
            log_count,
            error_count,
        })
        .collect();

    sources.sort_by(|a, b| b.log_count.cmp(&a.log_count));
    sources.truncate(10);

    sources
}

#[derive(Serialize)]
struct AiAnalysisData {
    export_timestamp: String,
    analysis_results: Vec<AnalysisResult>,
}

#[derive(Serialize)]
struct AnalysisResult {
    timestamp: String,
    question: String,
    answer: String,
    is_error: bool,
    context_logs_count: usize,
}

pub fn export_ai_analysis(chat_history: &[ChatMessage]) -> Result<String> {
    let filename = generate_filename("ai_analysis", "json");
    let mut file = File::create(&filename)?;

    let export_timestamp = Local::now().to_rfc3339();

    let mut analysis_results: Vec<AnalysisResult> = Vec::new();

    let mut current_question: Option<String> = None;
    for msg in chat_history {
        match msg.role {
            crate::models::ChatRole::User => {
                current_question = Some(msg.content.clone());
            }
            crate::models::ChatRole::Assistant => {
                if let Some(question) = current_question.take() {
                    let is_error = msg.content.contains("ERROR:");
                    let cleaned_answer = msg.content.clone();
                    analysis_results.push(AnalysisResult {
                        timestamp: Local::now().to_rfc3339(),
                        question,
                        answer: cleaned_answer,
                        is_error,
                        context_logs_count: 0,
                    });
                }
            }
            _ => {}
        }
    }

    let data = AiAnalysisData {
        export_timestamp,
        analysis_results,
    };

    let json = serde_json::to_string_pretty(&data)?;
    file.write_all(json.as_bytes())?;

    Ok(filename)
}

pub fn perform_export(
    export_type: ExportType,
    entries: &[DisplayEntry],
    stats: &DashboardStats,
    chat_history: &[ChatMessage],
) -> Result<String> {
    match export_type {
        ExportType::LogsCsv => export_logs_to_csv(entries),
        ExportType::LogsJson => export_logs_to_json(entries),
        ExportType::Report => export_report(entries, stats),
        ExportType::AiAnalysis => export_ai_analysis(chat_history),
    }
}
