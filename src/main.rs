use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use arboard::Clipboard;
use chrono::NaiveDateTime;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use encoding_rs::GB18030;
use glob::glob;
use memmap2::Mmap;
use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;

mod ai_client;

#[derive(Parser)]
#[command(name = "log_analysis", about = "TUI log analyzer")]
struct Args {
    #[arg(required = true)]
    files: Vec<String>,
}

#[derive(Debug, Clone)]
struct FileInfo {
    id: usize,
    name: String,
    color: Color,
    enabled: bool,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Focus { #[default] LogList, FileList }

#[derive(Debug, Serialize, Clone)]
struct LogEntry {
    timestamp: String,
    pid: String,
    tid: String,
    level: String,
    content: String,
    source_file: String,
    line_num: u32,
    json_payload: Option<Value>,
    delta_ms: Option<i64>,
    source_id: usize,
}

#[derive(Debug, Clone)]
enum DisplayEntry {
    Normal(LogEntry),
    Folded { start_index: usize, end_index: usize, count: usize, summary_text: String },
}

impl DisplayEntry {
    fn get_tid(&self) -> Option<&str> {
        match self { DisplayEntry::Normal(log) => Some(&log.tid), _ => None }
    }
    fn get_searchable_text(&self) -> String {
        match self {
            DisplayEntry::Normal(log) => format!("{} {} {}", log.content, log.source_file, log.tid),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        }
    }
    fn get_delta_ms(&self) -> Option<i64> {
        match self { DisplayEntry::Normal(log) => log.delta_ms, _ => None }
    }
    fn get_content(&self) -> String {
        match self {
            DisplayEntry::Normal(log) => format!("{} [{}][{}]: {}", log.timestamp, log.tid, log.level, log.content),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        }
    }
    fn get_source_id(&self) -> Option<usize> {
        match self { DisplayEntry::Normal(log) => Some(log.source_id), _ => None }
    }
}

#[derive(Default)]
enum AiState {
    #[default]
    Idle,
    Loading,
    Completed(String),
    Error(String),
}

#[derive(Clone)]
struct LevelVisibility { info: bool, warn: bool, error: bool, debug: bool }
impl Default for LevelVisibility {
    fn default() -> Self { Self { info: true, warn: true, error: true, debug: true } }
}

struct App {
    all_entries: Vec<DisplayEntry>,
    filtered_entries: Vec<DisplayEntry>,
    list_state: ListState,
    filter_tid: Option<String>,
    search_mode: bool,
    search_query: String,
    search_regex: Option<Regex>,
    negative_search: bool,
    match_indices: Vec<usize>,
    current_match: usize,
    status_msg: Option<(String, Instant)>,
    clipboard: Option<Clipboard>,
    histogram: Vec<(String, u64)>,
    ai_state: AiState,
    ai_tx: mpsc::Sender<String>,
    ai_rx: mpsc::Receiver<Result<String, String>>,
    bookmarks: BTreeSet<usize>,
    visible_levels: LevelVisibility,
    show_help: bool,
    files: Vec<FileInfo>,
    focus: Focus,
    file_list_state: ListState,
}

impl App {
    fn new(entries: Vec<DisplayEntry>, histogram: Vec<(String, u64)>, files: Vec<FileInfo>, ai_tx: mpsc::Sender<String>, ai_rx: mpsc::Receiver<Result<String, String>>) -> Self {
        let mut list_state = ListState::default();
        if !entries.is_empty() { list_state.select(Some(0)); }
        let mut file_list_state = ListState::default();
        if !files.is_empty() { file_list_state.select(Some(0)); }
        Self {
            all_entries: entries.clone(),
            filtered_entries: entries,
            list_state,
            filter_tid: None,
            search_mode: false,
            search_query: String::new(),
            search_regex: None,
            negative_search: false,
            match_indices: Vec::new(),
            current_match: 0,
            status_msg: None,
            clipboard: Clipboard::new().ok(),
            histogram,
            ai_state: AiState::Idle,
            ai_tx, ai_rx,
            bookmarks: BTreeSet::new(),
            visible_levels: LevelVisibility::default(),
            show_help: false,
            files, focus: Focus::LogList, file_list_state,
        }
    }

    fn entries(&self) -> &Vec<DisplayEntry> { &self.filtered_entries }

    fn next(&mut self) {
        let len = self.filtered_entries.len();
        if len == 0 { return; }
        let i = self.list_state.selected().map(|i| (i + 1).min(len - 1)).unwrap_or(0);
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.filtered_entries.is_empty() { return; }
        let i = self.list_state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
        self.list_state.select(Some(i));
    }

    fn selected_entry(&self) -> Option<&DisplayEntry> {
        self.list_state.selected().and_then(|i| self.filtered_entries.get(i))
    }

    fn toggle_thread_filter(&mut self) {
        if self.filter_tid.is_some() {
            self.filter_tid = None;
            self.apply_filter();
        } else if let Some(tid) = self.selected_entry().and_then(|e| e.get_tid()).map(String::from) {
            self.filter_tid = Some(tid);
            self.apply_filter();
        }
    }

    fn apply_filter(&mut self) {
        let enabled_files: Vec<usize> = self.files.iter().filter(|f| f.enabled).map(|f| f.id).collect();
        self.filtered_entries = self.all_entries.iter().enumerate()
            .filter(|(_, e)| {
                // File filter
                if let Some(sid) = e.get_source_id() {
                    if !enabled_files.contains(&sid) { return false; }
                }
                // Thread filter
                if let Some(tid) = &self.filter_tid {
                    if e.get_tid() != Some(tid) { return false; }
                }
                // Level filter
                if let DisplayEntry::Normal(log) = e {
                    let level = log.level.to_lowercase();
                    if level.contains("info") && !self.visible_levels.info { return false; }
                    if level.contains("warn") && !self.visible_levels.warn { return false; }
                    if level.contains("error") && !self.visible_levels.error { return false; }
                    if level.contains("debug") && !self.visible_levels.debug { return false; }
                }
                true
            })
            .map(|(_, e)| e.clone()).collect();
        self.list_state.select(if self.filtered_entries.is_empty() { None } else { Some(0) });
        self.update_search_matches();
    }

    fn start_search(&mut self) { self.search_mode = true; self.search_query.clear(); self.negative_search = false; }
    fn exit_search(&mut self) { self.search_mode = false; }

    fn update_search(&mut self) {
        if self.search_query.starts_with('!') {
            self.negative_search = true;
            let pattern = &self.search_query[1..];
            self.search_regex = if pattern.is_empty() { None } else { Regex::new(pattern).ok() };
        } else {
            self.negative_search = false;
            self.search_regex = Regex::new(&self.search_query).ok();
        }
        self.update_search_matches();
    }

    fn update_search_matches(&mut self) {
        self.match_indices.clear();
        if let Some(re) = &self.search_regex {
            for (i, entry) in self.filtered_entries.iter().enumerate() {
                let matches = re.is_match(&entry.get_searchable_text());
                if self.negative_search { if !matches { self.match_indices.push(i); } }
                else { if matches { self.match_indices.push(i); } }
            }
        }
        self.current_match = 0;
    }

    fn next_match(&mut self) {
        if self.match_indices.is_empty() { return; }
        self.current_match = (self.current_match + 1) % self.match_indices.len();
        self.list_state.select(Some(self.match_indices[self.current_match]));
    }

    fn prev_match(&mut self) {
        if self.match_indices.is_empty() { return; }
        self.current_match = self.current_match.checked_sub(1).unwrap_or(self.match_indices.len() - 1);
        self.list_state.select(Some(self.match_indices[self.current_match]));
    }

    fn toggle_bookmark(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if !self.bookmarks.remove(&idx) { self.bookmarks.insert(idx); }
        }
    }

    fn next_bookmark(&mut self) {
        if self.bookmarks.is_empty() { return; }
        let current = self.list_state.selected().unwrap_or(0);
        let next = self.bookmarks.range((current + 1)..).next()
            .or_else(|| self.bookmarks.iter().next());
        if let Some(&idx) = next { self.list_state.select(Some(idx)); }
    }

    fn toggle_level(&mut self, level: u8) {
        match level {
            1 => self.visible_levels.info = !self.visible_levels.info,
            2 => self.visible_levels.warn = !self.visible_levels.warn,
            3 => self.visible_levels.error = !self.visible_levels.error,
            4 => self.visible_levels.debug = !self.visible_levels.debug,
            _ => {}
        }
        self.apply_filter();
    }

    fn copy_line(&mut self) {
        let text = self.selected_entry().map(|entry| match entry {
            DisplayEntry::Normal(log) => format!("{} [{}:{}][{}]: {} ({}:{})",
                log.timestamp, log.pid, log.tid, log.level, log.content, log.source_file, log.line_num),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        });
        if let (Some(clip), Some(text)) = (self.clipboard.as_mut(), text) {
            if clip.set_text(text).is_ok() { self.status_msg = Some(("Copied!".into(), Instant::now())); }
        }
    }

    fn yank_payload(&mut self) {
        let text = self.selected_entry().map(|entry| match entry {
            DisplayEntry::Normal(log) => log.json_payload.as_ref()
                .map(|j| serde_json::to_string_pretty(j).unwrap_or_default())
                .unwrap_or_else(|| log.content.clone()),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        });
        if let (Some(clip), Some(text)) = (self.clipboard.as_mut(), text) {
            if clip.set_text(text).is_ok() { self.status_msg = Some(("Yanked!".into(), Instant::now())); }
        }
    }

    fn status_message(&self) -> Option<&str> {
        self.status_msg.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(2)).map(|(s, _)| s.as_str())
    }

    fn toggle_file(&mut self) {
        if let Some(idx) = self.file_list_state.selected() {
            if let Some(f) = self.files.get_mut(idx) { f.enabled = !f.enabled; }
            self.apply_filter();
        }
    }

    fn solo_file(&mut self) {
        if let Some(idx) = self.file_list_state.selected() {
            for (i, f) in self.files.iter_mut().enumerate() { f.enabled = i == idx; }
            self.apply_filter();
        }
    }

    fn get_file_color(&self, source_id: usize) -> Color {
        self.files.iter().find(|f| f.id == source_id).map(|f| f.color).unwrap_or(Color::White)
    }
}

fn parse_timestamp(ts: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.3f").ok()
}

fn extract_json_from_bytes(line_bytes: &[u8]) -> Option<Value> {
    let start = line_bytes.windows(2).position(|w| w == b"<{")?;
    let end = line_bytes.windows(2).rposition(|w| w == b"}>")?;
    serde_json::from_str(&String::from_utf8_lossy(&line_bytes[start + 1..end + 1])).ok()
}

fn parse_line(line: &str, line_bytes: &[u8], re: &Regex, source_id: usize) -> Option<LogEntry> {
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
    })
}

fn calculate_deltas(entries: &mut [LogEntry]) {
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

fn build_histogram(entries: &[LogEntry]) -> Vec<(String, u64)> {
    if entries.is_empty() { return vec![]; }
    let mut counts: HashMap<String, u64> = HashMap::new();
    for entry in entries {
        if let Some(ts) = parse_timestamp(&entry.timestamp) {
            let key = ts.format("%m-%d %H:%M").to_string();
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted
}

fn merge_multiline_bytes(data: &[u8]) -> Vec<Vec<u8>> {
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

fn fold_noise(logs: Vec<LogEntry>) -> Vec<DisplayEntry> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < logs.len() {
        let matches_a = |e: &LogEntry| e.level == "Info" && (e.source_file.contains("UsbCtrl") || e.source_file.contains("EnumDevice"));
        let matches_b = |e: &LogEntry| e.content.contains("DestroyThread") || e.content.contains("Terminate");
        
        let mut j = i;
        while j < logs.len() && matches_a(&logs[j]) { j += 1; }
        if j - i >= 3 {
            result.push(DisplayEntry::Folded { start_index: i, end_index: j-1, count: j-i, summary_text: format!("Folded {} USB polling", j-i) });
            i = j; continue;
        }
        j = i;
        while j < logs.len() && matches_b(&logs[j]) { j += 1; }
        if j - i >= 3 {
            result.push(DisplayEntry::Folded { start_index: i, end_index: j-1, count: j-i, summary_text: format!("Folded {} thread cleanup", j-i) });
            i = j; continue;
        }
        j = i;
        while j < logs.len() && logs[i].content == logs[j].content { j += 1; }
        if j - i >= 5 {
            result.push(DisplayEntry::Folded { start_index: i, end_index: j-1, count: j-i, summary_text: format!("Folded {} identical", j-i) });
            i = j; continue;
        }
        result.push(DisplayEntry::Normal(logs[i].clone()));
        i += 1;
    }
    result
}

fn level_color(level: &str) -> Color {
    match level { "Error" => Color::Red, "Warning"|"Warn" => Color::Yellow, "Debug" => Color::Cyan, _ => Color::White }
}

fn delta_span(delta_ms: Option<i64>) -> Option<Span<'static>> {
    let d = delta_ms?;
    if d >= 1000 {
        Some(Span::styled(format!("[SLOW {:.1}s]", d as f64 / 1000.0), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
    } else if d >= 100 {
        Some(Span::styled(format!("[+{}ms]", d), Style::default().fg(Color::Yellow)))
    } else { None }
}

fn highlight_text(text: &str, regex: Option<&Regex>, base_style: Style) -> Vec<Span<'static>> {
    let Some(re) = regex else { return vec![Span::styled(text.to_string(), base_style)]; };
    let mut spans = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text) {
        if m.start() > last { spans.push(Span::styled(text[last..m.start()].to_string(), base_style)); }
        spans.push(Span::styled(m.as_str().to_string(), Style::default().bg(Color::Yellow).fg(Color::Black)));
        last = m.end();
    }
    if last < text.len() { spans.push(Span::styled(text[last..].to_string(), base_style)); }
    spans
}

fn render_list_item(entry: &DisplayEntry, regex: Option<&Regex>, is_match: bool, is_bookmarked: bool, file_color: Color) -> ListItem<'static> {
    let color_bar = "█ ";
    let bookmark = if is_bookmarked { "🔖" } else { " " };
    let marker = if is_match { "●" } else { " " };
    match entry {
        DisplayEntry::Normal(log) => {
            let preview: String = log.content.chars().take(35).collect();
            let mut spans: Vec<Span<'static>> = vec![
                Span::styled(color_bar.to_string(), Style::default().fg(file_color)),
                Span::styled(bookmark.to_string(), Style::default().fg(Color::Magenta)),
                Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
                Span::styled(log.timestamp[11..19].to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
            ];
            if let Some(ds) = delta_span(log.delta_ms) {
                spans.push(ds);
                spans.push(Span::raw(" "));
            }
            spans.extend(vec![
                Span::styled(format!("[{:5}]", &log.level), Style::default().fg(level_color(&log.level))),
                Span::raw(" "),
            ]);
            spans.extend(highlight_text(&preview, regex, Style::default()));
            let style = if is_bookmarked { Style::default().bg(Color::Rgb(40, 40, 60)) } else { Style::default() };
            ListItem::new(Line::from(spans)).style(style)
        }
        DisplayEntry::Folded { count, summary_text, .. } => ListItem::new(Line::from(vec![
            Span::styled(color_bar.to_string(), Style::default().fg(file_color)),
            Span::styled(bookmark.to_string(), Style::default().fg(Color::Magenta)),
            Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
            Span::styled(format!("▶ [{} lines] ", count), Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::styled(summary_text.clone(), Style::default().fg(Color::DarkGray)),
        ])),
    }
}

fn format_json(value: &Value, indent: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let pre = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            lines.push(Line::from("{"));
            for (i, (k, v)) in map.iter().enumerate() {
                let comma = if i < map.len() - 1 { "," } else { "" };
                match v {
                    Value::Object(_) | Value::Array(_) => {
                        lines.push(Line::from(vec![Span::raw(format!("{}  ", pre)), Span::styled(format!("\"{}\"", k), Style::default().fg(Color::Cyan)), Span::raw(": ")]));
                        let mut sub = format_json(v, indent + 2);
                        if let Some(l) = sub.last_mut() { l.spans.push(Span::raw(comma)); }
                        lines.extend(sub);
                    }
                    _ => lines.push(Line::from(vec![
                        Span::raw(format!("{}  ", pre)),
                        Span::styled(format!("\"{}\"", k), Style::default().fg(Color::Cyan)),
                        Span::raw(": "),
                        Span::styled(match v { Value::String(s) => format!("\"{}\"", s), _ => v.to_string() }, Style::default().fg(Color::Green)),
                        Span::raw(comma),
                    ])),
                }
            }
            lines.push(Line::from(format!("{}}}", pre)));
        }
        Value::Array(arr) => {
            lines.push(Line::from("["));
            for (i, v) in arr.iter().enumerate() {
                let comma = if i < arr.len() - 1 { "," } else { "" };
                let mut sub = format_json(v, indent + 2);
                if let Some(f) = sub.first_mut() { f.spans.insert(0, Span::raw(format!("{}  ", pre))); }
                if let Some(l) = sub.last_mut() { l.spans.push(Span::raw(comma)); }
                lines.extend(sub);
            }
            lines.push(Line::from(format!("{}]", pre)));
        }
        _ => lines.push(Line::from(value.to_string())),
    }
    lines
}

fn render_detail(entry: Option<&DisplayEntry>) -> Text<'static> {
    match entry {
        Some(DisplayEntry::Normal(log)) => {
            let mut lines = vec![
                Line::from(vec![Span::styled("Time: ", Style::default().fg(Color::Yellow)), Span::raw(log.timestamp.clone())]),
                Line::from(vec![Span::styled("TID: ", Style::default().fg(Color::Yellow)), Span::raw(format!("{}:{}", log.pid, log.tid))]),
                Line::from(vec![Span::styled("Level: ", Style::default().fg(Color::Yellow)), Span::styled(log.level.clone(), Style::default().fg(level_color(&log.level)))]),
                Line::from(vec![Span::styled("Source: ", Style::default().fg(Color::Yellow)), Span::raw(format!("{}:{}", log.source_file, log.line_num))]),
            ];
            if let Some(d) = log.delta_ms {
                lines.push(Line::from(vec![
                    Span::styled("Delta: ", Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{}ms", d), Style::default().fg(if d >= 1000 { Color::Red } else if d >= 100 { Color::Yellow } else { Color::Green })),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled("Content: ", Style::default().fg(Color::Yellow))]));
            lines.push(Line::from(log.content.clone()));
            if let Some(json) = &log.json_payload {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled("JSON:", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))]));
                lines.extend(format_json(json, 0));
            }
            Text::from(lines)
        }
        Some(DisplayEntry::Folded { count, summary_text, start_index, end_index }) => Text::from(vec![
            Line::from(vec![Span::styled("FOLDED", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))]),
            Line::from(""), Line::from(format!("Range: {}-{}", start_index, end_index)),
            Line::from(format!("Count: {}", count)), Line::from(format!("Reason: {}", summary_text)),
        ]),
        None => Text::from("No selection"),
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    // Main horizontal split: sidebar (20%) | content (80%)
    let main_chunks = Layout::horizontal([Constraint::Percentage(18), Constraint::Percentage(82)]).split(frame.area());
    let sidebar_area = main_chunks[0];
    let content_area = main_chunks[1];

    // Sidebar: File list
    let file_items: Vec<ListItem> = app.files.iter().map(|f| {
        let mark = if f.enabled { "[x]" } else { "[ ]" };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{} ", mark), Style::default().fg(Color::White)),
            Span::styled(&f.name, Style::default().fg(f.color)),
        ]))
    }).collect();
    let sidebar_style = if app.focus == Focus::FileList { Style::default().fg(Color::Cyan) } else { Style::default() };
    let file_list = List::new(file_items)
        .block(Block::default().borders(Borders::ALL).title(" Files ").border_style(sidebar_style))
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(file_list, sidebar_area, &mut app.file_list_state);

    // Content area vertical split
    let show_search = app.search_mode;
    let constraints = if show_search {
        vec![Constraint::Min(10), Constraint::Length(3), Constraint::Length(12), Constraint::Length(7)]
    } else {
        vec![Constraint::Min(10), Constraint::Length(0), Constraint::Length(12), Constraint::Length(7)]
    };
    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(content_area);

    // Title
    let title = match (&app.filter_tid, &app.search_regex) {
        (Some(tid), Some(_)) => format!(" [FILTER: Thread {}] [SEARCH: {} matches] ", tid, app.match_indices.len()),
        (Some(tid), None) => format!(" [FILTER: Thread {}] ", tid),
        (None, Some(_)) => format!(" [SEARCH: {} matches] (n/N navigate) ", app.match_indices.len()),
        (None, None) => format!(" Logs ({}) ", app.entries().len()),
    };
    let title_style = if app.filter_tid.is_some() || app.search_regex.is_some() {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else { Style::default() };

    // Log List with color bar
    let entries = app.entries().clone();
    let items: Vec<ListItem> = entries.iter().enumerate().map(|(i, e)| {
        let file_color = e.get_source_id().map(|sid| app.get_file_color(sid)).unwrap_or(Color::White);
        render_list_item(e, app.search_regex.as_ref(), app.match_indices.contains(&i), app.bookmarks.contains(&i), file_color)
    }).collect();

    let help = if show_search { "ESC=exit  !term=exclude" } else { "Tab=switch Space=toggle Enter=solo" };
    let list_style = if app.focus == Focus::LogList { Style::default().fg(Color::Cyan) } else { Style::default() };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title).title_style(title_style).title_bottom(Line::from(help).right_aligned()).border_style(list_style))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[0], &mut app.list_state);

    // Search bar
    if show_search {
        let search = Paragraph::new(format!("/{}", app.search_query))
            .block(Block::default().borders(Borders::ALL).title(" Search (regex) "));
        frame.render_widget(search, chunks[1]);
    }

    // Detail
    let detail = render_detail(app.selected_entry());
    let detail_title = app.status_message().map(|m| format!(" {} ", m)).unwrap_or(" Detail ".into());
    let detail_style = if app.status_message().is_some() { Style::default().fg(Color::Green) } else { Style::default() };
    let detail_widget = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title(detail_title).title_style(detail_style))
        .wrap(Wrap { trim: false });
    frame.render_widget(detail_widget, chunks[2]);

    // Histogram
    let hist_area = chunks[3];
    let max_bars = (hist_area.width as usize).saturating_sub(10) / 10;
    let hist_data: Vec<_> = app.histogram.iter().rev().take(max_bars).rev()
        .map(|(label, val)| {
            let color = if *val > 100 { Color::Red } else if *val > 50 { Color::Yellow } else { Color::Cyan };
            Bar::default().value(*val).label(Line::from(label.clone())).style(Style::default().fg(color))
        }).collect();

    let max_val = app.histogram.iter().map(|(_, v)| *v).max().unwrap_or(1);
    
    let total: u64 = app.histogram.iter().map(|(_, v)| *v).sum();
    let peak = app.histogram.iter().max_by_key(|(_, v)| *v).map(|(t, v)| format!("Peak: {} @ {}", v, t)).unwrap_or_default();
    
    let chart = BarChart::default()
        .block(Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![
                Span::styled(" 时间轴 ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("(月-日 时:分, 总计:{}, {}) ", total, peak), Style::default().fg(Color::DarkGray)),
            ]))
            .title_bottom(Line::from(vec![
                Span::styled(" █", Style::default().fg(Color::Red)),
                Span::styled(">100 ", Style::default().fg(Color::DarkGray)),
                Span::styled("█", Style::default().fg(Color::Yellow)),
                Span::styled(">50 ", Style::default().fg(Color::DarkGray)),
                Span::styled("█", Style::default().fg(Color::Cyan)),
                Span::styled("正常 ", Style::default().fg(Color::DarkGray)),
            ]).right_aligned()))
        .data(BarGroup::default().bars(&hist_data))
        .bar_width(12)
        .bar_gap(3)
        .value_style(Style::default().fg(Color::White).bg(Color::Black))
        .max(max_val);
    frame.render_widget(chart, hist_area);

    // AI Popup
    match &app.ai_state {
        AiState::Loading => {
            let area = centered_rect(40, 5, frame.area());
            frame.render_widget(Clear, area);
            let popup = Paragraph::new("⏳ AI 分析中...")
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title(" AI 诊断 "));
            frame.render_widget(popup, area);
        }
        AiState::Completed(text) | AiState::Error(text) => {
            let is_error = matches!(app.ai_state, AiState::Error(_));
            let area = centered_rect(80, 60, frame.area());
            frame.render_widget(Clear, area);
            let title = if is_error { " ❌ AI 错误 (Esc关闭) " } else { " ✅ AI 诊断结果 (Esc关闭) " };
            let style = if is_error { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Green) };
            let popup = Paragraph::new(text.clone())
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(title).title_style(style));
            frame.render_widget(popup, area);
        }
        AiState::Idle => {}
    }

    // Help Popup
    if app.show_help {
        let area = centered_rect(60, 50, frame.area());
        frame.render_widget(Clear, area);
        let help_text = "\
Navigation:  ↑/↓ or k/j (Scroll), n/N (Next/Prev Match)
Search:      / (Find), !term (Exclude matching)
Filters:     t (Thread Focus), 1/2/3/4 (Info/Warn/Err/Debug)
Bookmarks:   m (Toggle Mark), Tab (Next Bookmark)
Actions:     a (AI Analyze), c (Copy Line), y (Yank JSON)
Quit:        q (Exit), Esc (Close popup)";
        let popup = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL)
                .title(" ❓ Help (Press ? or Esc to close) ")
                .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
        frame.render_widget(popup, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ]).split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ]).split(popup_layout[1])[1]
}

fn main() -> Result<()> {
    let args = Args::parse();
    let colors = [Color::Red, Color::Blue, Color::Green, Color::Yellow, Color::Cyan, Color::Magenta];
    let re = Regex::new(r"^(\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}\.\d+)\[([0-9a-f]+):([0-9a-f]+)\]\[(\w+)\]:\s*(.*)\((.+):(\d+)\)\s*$")?;
    
    // Expand globs and collect file paths
    let mut file_paths: Vec<PathBuf> = Vec::new();
    for pattern in &args.files {
        for entry in glob(pattern).with_context(|| format!("无效模式: {}", pattern))? {
            file_paths.push(entry?);
        }
    }
    if file_paths.is_empty() { anyhow::bail!("没有找到匹配的文件"); }

    // Load files
    let mut files: Vec<FileInfo> = Vec::new();
    let mut all_entries: Vec<LogEntry> = Vec::new();
    for (id, path) in file_paths.iter().enumerate() {
        let file = File::open(path).with_context(|| format!("无法打开: {:?}", path))?;
        let mmap = unsafe { Mmap::map(&file)? };
        let entries: Vec<LogEntry> = merge_multiline_bytes(&mmap).iter()
            .filter_map(|b| { let (d, _, _) = GB18030.decode(b); parse_line(&d, b, &re, id) }).collect();
        files.push(FileInfo {
            id, name: path.file_name().map(|s| s.to_string_lossy().into()).unwrap_or_else(|| "?".into()),
            color: colors[id % colors.len()], enabled: true,
        });
        all_entries.extend(entries);
    }

    // Sort by timestamp
    all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    calculate_deltas(&mut all_entries);
    let histogram = build_histogram(&all_entries);
    let folded = fold_noise(all_entries);

    let rt = tokio::runtime::Runtime::new()?;
    let (req_tx, mut req_rx) = mpsc::channel::<String>(1);
    let (resp_tx, resp_rx) = mpsc::channel::<Result<String, String>>(1);

    rt.spawn(async move {
        while let Some(context) = req_rx.recv().await {
            let result = ai_client::analyze_error(context).await
                .map_err(|e| e.to_string());
            let _ = resp_tx.send(result).await;
        }
    });

    let mut app = App::new(folded, histogram, files, req_tx, resp_rx);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    loop {
        if let Ok(result) = app.ai_rx.try_recv() {
            app.ai_state = match result {
                Ok(s) => AiState::Completed(s),
                Err(e) => AiState::Error(e),
            };
        }

        terminal.draw(|f| ui(f, &mut app))?;
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                
                if matches!(app.ai_state, AiState::Completed(_) | AiState::Error(_)) {
                    if key.code == KeyCode::Esc { app.ai_state = AiState::Idle; continue; }
                }
                
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => app.exit_search(),
                        KeyCode::Enter => { app.update_search(); app.exit_search(); }
                        KeyCode::Backspace => { app.search_query.pop(); app.update_search(); }
                        KeyCode::Char(c) => { app.search_query.push(c); app.update_search(); }
                        _ => {}
                    }
                } else if app.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => app.show_help = false,
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Tab => app.focus = if app.focus == Focus::LogList { Focus::FileList } else { Focus::LogList },
                        KeyCode::Char('?') => app.show_help = true,
                        _ => {}
                    }
                    // Focus-specific actions
                    match app.focus {
                        Focus::FileList => match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                let len = app.files.len();
                                if len > 0 {
                                    let i = app.file_list_state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
                                    app.file_list_state.select(Some(i));
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let len = app.files.len();
                                if len > 0 {
                                    let i = app.file_list_state.selected().map(|i| (i + 1).min(len - 1)).unwrap_or(0);
                                    app.file_list_state.select(Some(i));
                                }
                            }
                            KeyCode::Char(' ') => app.toggle_file(),
                            KeyCode::Enter => app.solo_file(),
                            _ => {}
                        }
                        Focus::LogList => match key.code {
                            KeyCode::Up | KeyCode::Char('k') => app.previous(),
                            KeyCode::Down | KeyCode::Char('j') => app.next(),
                            KeyCode::Char('/') => app.start_search(),
                            KeyCode::Char('n') => app.next_match(),
                            KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::SHIFT) => app.prev_match(),
                            KeyCode::Char('t') => app.toggle_thread_filter(),
                            KeyCode::Esc => { app.filter_tid = None; app.apply_filter(); }
                            KeyCode::Char('c') => app.copy_line(),
                            KeyCode::Char('y') => app.yank_payload(),
                            KeyCode::Char('m') => app.toggle_bookmark(),
                            KeyCode::Char('b') => app.next_bookmark(),
                            KeyCode::Char('1') => app.toggle_level(1),
                            KeyCode::Char('2') => app.toggle_level(2),
                            KeyCode::Char('3') => app.toggle_level(3),
                            KeyCode::Char('4') => app.toggle_level(4),
                            KeyCode::Char('a') => {
                                if matches!(app.ai_state, AiState::Idle) {
                                    if let Some(idx) = app.list_state.selected() {
                                        let start = idx.saturating_sub(10);
                                        let end = (idx + 11).min(app.filtered_entries.len());
                                        let context: String = app.filtered_entries[start..end].iter()
                                            .map(|e| e.get_content()).collect::<Vec<_>>().join("\n");
                                        if app.ai_tx.blocking_send(context).is_ok() {
                                            app.ai_state = AiState::Loading;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
