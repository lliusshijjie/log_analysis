use std::collections::HashMap;
use std::fs::File;
use std::io::stdout;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use arboard::Clipboard;
use chrono::NaiveDateTime;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use encoding_rs::GB18030;
use memmap2::Mmap;
use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use serde::Serialize;
use serde_json::Value;

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
}

struct App {
    all_entries: Vec<DisplayEntry>,
    filtered_entries: Vec<DisplayEntry>,
    list_state: ListState,
    filter_tid: Option<String>,
    search_mode: bool,
    search_query: String,
    search_regex: Option<Regex>,
    match_indices: Vec<usize>,
    current_match: usize,
    status_msg: Option<(String, Instant)>,
    clipboard: Option<Clipboard>,
    histogram: Vec<(String, u64)>,
}

impl App {
    fn new(entries: Vec<DisplayEntry>, histogram: Vec<(String, u64)>) -> Self {
        let mut list_state = ListState::default();
        if !entries.is_empty() { list_state.select(Some(0)); }
        Self {
            all_entries: entries.clone(),
            filtered_entries: entries,
            list_state,
            filter_tid: None,
            search_mode: false,
            search_query: String::new(),
            search_regex: None,
            match_indices: Vec::new(),
            current_match: 0,
            status_msg: None,
            clipboard: Clipboard::new().ok(),
            histogram,
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
        self.filtered_entries = self.all_entries.iter()
            .filter(|e| self.filter_tid.as_ref().map_or(true, |tid| e.get_tid() == Some(tid)))
            .cloned().collect();
        self.list_state.select(if self.filtered_entries.is_empty() { None } else { Some(0) });
        self.update_search_matches();
    }

    fn start_search(&mut self) { self.search_mode = true; self.search_query.clear(); }
    fn exit_search(&mut self) { self.search_mode = false; }

    fn update_search(&mut self) {
        self.search_regex = Regex::new(&self.search_query).ok();
        self.update_search_matches();
    }

    fn update_search_matches(&mut self) {
        self.match_indices.clear();
        if let Some(re) = &self.search_regex {
            for (i, entry) in self.filtered_entries.iter().enumerate() {
                if re.is_match(&entry.get_searchable_text()) { self.match_indices.push(i); }
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
}

fn parse_timestamp(ts: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.3f").ok()
}

fn extract_json_from_bytes(line_bytes: &[u8]) -> Option<Value> {
    let start = line_bytes.windows(2).position(|w| w == b"<{")?;
    let end = line_bytes.windows(2).rposition(|w| w == b"}>")?;
    serde_json::from_str(&String::from_utf8_lossy(&line_bytes[start + 1..end + 1])).ok()
}

fn parse_line(line: &str, line_bytes: &[u8], re: &Regex) -> Option<LogEntry> {
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

fn render_list_item(entry: &DisplayEntry, regex: Option<&Regex>, is_match: bool) -> ListItem<'static> {
    let marker = if is_match { "● " } else { "  " };
    match entry {
        DisplayEntry::Normal(log) => {
            let preview: String = log.content.chars().take(40).collect();
            let mut spans: Vec<Span<'static>> = vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
                Span::styled(log.timestamp[11..19].to_string(), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
            ];
            if let Some(ds) = delta_span(log.delta_ms) {
                spans.push(ds);
                spans.push(Span::raw(" "));
            }
            spans.extend(vec![
                Span::styled(format!("{}:{}", &log.pid, &log.tid), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(format!("[{:5}]", &log.level), Style::default().fg(level_color(&log.level))),
                Span::raw(" "),
            ]);
            spans.extend(highlight_text(&preview, regex, Style::default()));
            ListItem::new(Line::from(spans))
        }
        DisplayEntry::Folded { count, summary_text, .. } => ListItem::new(Line::from(vec![
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
    let show_search = app.search_mode;
    let constraints = if show_search {
        vec![Constraint::Min(10), Constraint::Length(3), Constraint::Length(12), Constraint::Length(7)]
    } else {
        vec![Constraint::Min(10), Constraint::Length(0), Constraint::Length(12), Constraint::Length(7)]
    };
    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(frame.area());

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

    // List
    let entries = app.entries().clone();
    let items: Vec<ListItem> = entries.iter().enumerate().map(|(i, e)| {
        render_list_item(e, app.search_regex.as_ref(), app.match_indices.contains(&i))
    }).collect();

    let help = if show_search { "ESC=exit" } else { "/=search t=thread c=copy y=yank q=quit" };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title).title_style(title_style).title_bottom(Line::from(help).right_aligned()))
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
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).map(|s| s.as_str()).unwrap_or("AisEsmEmc.log");
    let file = File::open(path).with_context(|| format!("无法打开日志文件: {}", path))?;
    let mmap = unsafe { Mmap::map(&file)? };
    let re = Regex::new(r"^(\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}\.\d+)\[([0-9a-f]+):([0-9a-f]+)\]\[(\w+)\]:\s*(.*)\((.+):(\d+)\)\s*$")?;
    
    let mut entries: Vec<LogEntry> = merge_multiline_bytes(&mmap).iter()
        .filter_map(|b| { let (d, _, _) = GB18030.decode(b); parse_line(&d, b, &re) }).collect();
    calculate_deltas(&mut entries);
    let histogram = build_histogram(&entries);
    let mut app = App::new(fold_noise(entries), histogram);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => app.exit_search(),
                        KeyCode::Enter => { app.update_search(); app.exit_search(); }
                        KeyCode::Backspace => { app.search_query.pop(); app.update_search(); }
                        KeyCode::Char(c) => { app.search_query.push(c); app.update_search(); }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Char('/') => app.start_search(),
                        KeyCode::Char('n') => app.next_match(),
                        KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::SHIFT) => app.prev_match(),
                        KeyCode::Char('t') | KeyCode::Esc => app.toggle_thread_filter(),
                        KeyCode::Char('c') => app.copy_line(),
                        KeyCode::Char('y') => app.yank_payload(),
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
