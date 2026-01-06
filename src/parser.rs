use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDateTime;
use encoding_rs::GB18030;
use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use serde_json::Value;

use crate::config::ParserConfig;
use crate::models::LogEntry;

pub fn parse_timestamp(ts: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.3f").ok()
}

fn extract_json_from_bytes(line_bytes: &[u8]) -> Option<Value> {
    let start = line_bytes.windows(2).position(|w| w == b"<{")?;
    let end = line_bytes.windows(2).rposition(|w| w == b"}>")?;
    serde_json::from_str(&String::from_utf8_lossy(&line_bytes[start + 1..end + 1])).ok()
}

pub fn parse_line(line: &str, line_bytes: &[u8], re: &Regex, source_id: usize, line_index: usize) -> Option<LogEntry> {
    let caps = re.captures(line)?;
    Some(LogEntry {
        timestamp: caps.get(1)?.as_str().into(),
        pid: caps.get(2)?.as_str().into(),
        tid: caps.get(3)?.as_str().into(),
        level: caps.get(4)?.as_str().into(),
        content: caps.get(5)?.as_str().into(),
        source_file: caps.get(6)?.as_str().into(),
        line_num: caps.get(7)?.as_str().parse().ok()?,
        json_payload: extract_json_from_bytes(line_bytes),
        delta_ms: None,
        source_id,
        line_index,
    })
}

pub fn merge_multiline_bytes(data: &[u8]) -> Vec<Vec<u8>> {
    let ts_re = BytesRegex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d+").unwrap();
    let mut merged = Vec::new();
    let mut current = Vec::new();
    for line in data.split(|&b| b == b'\n') {
        let line = if line.last() == Some(&b'\r') { &line[..line.len()-1] } else { line };
        if ts_re.is_match(line) {
            if !current.is_empty() { merged.push(current); }
            current = line.to_vec();
        } else if !line.is_empty() { current.extend_from_slice(line); }
    }
    if !current.is_empty() { merged.push(current); }
    merged
}

pub fn calculate_deltas(entries: &mut [LogEntry]) {
    let mut last_time: HashMap<String, NaiveDateTime> = HashMap::new();
    for entry in entries.iter_mut() {
        if let Some(ts) = parse_timestamp(&entry.timestamp) {
            if let Some(prev) = last_time.get(&entry.tid) {
                let delta = ts.signed_duration_since(*prev).num_milliseconds();
                if delta > 0 { entry.delta_ms = Some(delta); }
            }
            last_time.insert(entry.tid.clone(), ts);
        }
    }
}

pub fn build_histogram(entries: &[LogEntry]) -> Vec<(String, u64)> {
    if entries.is_empty() { return vec![]; }
    let mut counts: HashMap<String, u64> = HashMap::new();
    for entry in entries {
        if let Some(ts) = parse_timestamp(&entry.timestamp) {
            let key = ts.format("%m-%d %H:00").to_string();
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted
}

pub fn decode_line(bytes: &[u8]) -> String {
    let (decoded, _, _) = GB18030.decode(bytes);
    decoded.into_owned()
}

pub fn create_log_regex(config: &ParserConfig) -> Result<Regex> {
    Regex::new(&config.log_pattern).map_err(|e| anyhow::anyhow!("无效的日志正则表达式: {}", e))
}

