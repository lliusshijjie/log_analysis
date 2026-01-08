use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use ratatui::prelude::Color;
use ratatui::widgets::ListState;
use regex::Regex;
use tokio::sync::mpsc;

use crate::models::{AiState, CurrentView, DashboardStats, DisplayEntry, FileInfo, Focus, InputMode, LevelVisibility};

pub struct App {
    pub all_entries: Vec<DisplayEntry>,
    pub filtered_entries: Vec<DisplayEntry>,
    pub list_state: ListState,
    pub filter_tid: Option<String>,
    pub search_mode: bool,
    pub search_query: String,
    pub search_regex: Option<Regex>,
    pub negative_search: bool,
    pub match_indices: Vec<usize>,
    pub current_match: usize,
    pub status_msg: Option<(String, Instant)>,
    pub clipboard: Option<Clipboard>,
    pub histogram: Vec<(String, u64)>,
    pub ai_state: AiState,
    pub ai_tx: mpsc::Sender<(String, Option<String>)>,
    pub ai_rx: mpsc::Receiver<Result<String, String>>,
    pub bookmarks: BTreeSet<usize>,
    pub visible_levels: LevelVisibility,
    pub show_help: bool,
    pub files: Vec<FileInfo>,
    pub focus: Focus,
    pub file_list_state: ListState,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub is_tailing: bool,
    pub current_view: CurrentView,
    pub stats: DashboardStats,
    pub page_size: usize,
}

impl App {
    pub fn new(
        entries: Vec<DisplayEntry>,
        histogram: Vec<(String, u64)>,
        files: Vec<FileInfo>,
        ai_tx: mpsc::Sender<(String, Option<String>)>,
        ai_rx: mpsc::Receiver<Result<String, String>>,
        page_size: usize,
    ) -> Self {
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
            ai_tx,
            ai_rx,
            bookmarks: BTreeSet::new(),
            visible_levels: LevelVisibility::default(),
            show_help: false,
            files,
            focus: Focus::LogList,
            file_list_state,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            is_tailing: false,
            current_view: CurrentView::Logs,
            stats: DashboardStats::default(),
            page_size,
        }
    }

    pub fn entries(&self) -> &Vec<DisplayEntry> { &self.filtered_entries }

    pub fn next(&mut self) {
        let len = self.filtered_entries.len();
        if len == 0 { return; }
        let i = self.list_state.selected().map(|i| (i + 1).min(len - 1)).unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.filtered_entries.is_empty() { return; }
        let i = self.list_state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn next_page(&mut self) {
        let len = self.filtered_entries.len();
        if len == 0 { return; }
        let i = self.list_state.selected().map(|i| i.saturating_add(self.page_size).min(len - 1)).unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn previous_page(&mut self) {
        if self.filtered_entries.is_empty() { return; }
        let i = self.list_state.selected().map(|i| i.saturating_sub(self.page_size)).unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn selected_entry(&self) -> Option<&DisplayEntry> {
        self.list_state.selected().and_then(|i| self.filtered_entries.get(i))
    }

    pub fn toggle_thread_filter(&mut self) {
        if self.filter_tid.is_some() {
            self.filter_tid = None;
            self.apply_filter();
        } else if let Some(tid) = self.selected_entry().and_then(|e| e.get_tid()).map(String::from) {
            self.filter_tid = Some(tid);
            self.apply_filter();
        }
    }

    pub fn apply_filter(&mut self) {
        let enabled_files: Vec<usize> = self.files.iter().filter(|f| f.enabled).map(|f| f.id).collect();
        self.filtered_entries = self.all_entries.iter().enumerate()
            .filter(|(_, e)| {
                if let Some(sid) = e.get_source_id() {
                    if !enabled_files.contains(&sid) { return false; }
                }
                if let Some(tid) = &self.filter_tid {
                    if e.get_tid() != Some(tid) { return false; }
                }
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

    pub fn start_search(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
        self.negative_search = false;
    }

    pub fn exit_search(&mut self) { self.search_mode = false; }

    pub fn update_search(&mut self) {
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

    pub fn update_search_matches(&mut self) {
        self.match_indices.clear();
        if let Some(re) = &self.search_regex {
            for (i, entry) in self.filtered_entries.iter().enumerate() {
                let matches = re.is_match(&entry.get_searchable_text());
                if self.negative_search {
                    if !matches { self.match_indices.push(i); }
                } else {
                    if matches { self.match_indices.push(i); }
                }
            }
        }
        self.current_match = 0;
    }

    pub fn next_match(&mut self) {
        if self.match_indices.is_empty() { return; }
        self.current_match = (self.current_match + 1) % self.match_indices.len();
        self.list_state.select(Some(self.match_indices[self.current_match]));
    }

    pub fn prev_match(&mut self) {
        if self.match_indices.is_empty() { return; }
        self.current_match = self.current_match.checked_sub(1).unwrap_or(self.match_indices.len() - 1);
        self.list_state.select(Some(self.match_indices[self.current_match]));
    }

    pub fn toggle_bookmark(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if !self.bookmarks.remove(&idx) { self.bookmarks.insert(idx); }
        }
    }

    pub fn next_bookmark(&mut self) {
        if self.bookmarks.is_empty() { return; }
        let current = self.list_state.selected().unwrap_or(0);
        let next = self.bookmarks.range((current + 1)..).next()
            .or_else(|| self.bookmarks.iter().next());
        if let Some(&idx) = next { self.list_state.select(Some(idx)); }
    }

    pub fn toggle_level(&mut self, level: u8) {
        match level {
            1 => self.visible_levels.info = !self.visible_levels.info,
            2 => self.visible_levels.warn = !self.visible_levels.warn,
            3 => self.visible_levels.error = !self.visible_levels.error,
            4 => self.visible_levels.debug = !self.visible_levels.debug,
            _ => {}
        }
        self.apply_filter();
    }

    pub fn copy_line(&mut self) {
        let text = self.selected_entry().map(|entry| match entry {
            DisplayEntry::Normal(log) => format!("{} [{}:{}][{}]: {} ({}:{})",
                log.timestamp, log.pid, log.tid, log.level, log.content, log.source_file, log.line_num),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        });
        if let (Some(clip), Some(text)) = (self.clipboard.as_mut(), text) {
            if clip.set_text(text).is_ok() { self.status_msg = Some(("Copied!".into(), Instant::now())); }
        }
    }

    pub fn yank_payload(&mut self) {
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

    pub fn status_message(&self) -> Option<&str> {
        self.status_msg.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(2)).map(|(s, _)| s.as_str())
    }

    pub fn toggle_file(&mut self) {
        if let Some(idx) = self.file_list_state.selected() {
            if let Some(f) = self.files.get_mut(idx) { f.enabled = !f.enabled; }
            self.apply_filter();
        }
    }

    pub fn solo_file(&mut self) {
        if let Some(idx) = self.file_list_state.selected() {
            for (i, f) in self.files.iter_mut().enumerate() { f.enabled = i == idx; }
            self.apply_filter();
        }
    }

    pub fn get_file_color(&self, source_id: usize) -> Color {
        self.files.iter().find(|f| f.id == source_id).map(|f| f.color).unwrap_or(Color::White)
    }

    pub fn enter_jump_mode(&mut self) {
        self.input_mode = InputMode::JumpInput;
        self.input_buffer.clear();
    }

    pub fn exit_jump_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    pub fn submit_jump(&mut self) {
        if let Ok(line_num) = self.input_buffer.parse::<usize>() {
            if let Some(idx) = self.filtered_entries.iter().position(|e| e.get_line_index() == Some(line_num)) {
                self.list_state.select(Some(idx));
            } else {
                self.status_msg = Some(("Line not found".into(), Instant::now()));
            }
        }
        self.exit_jump_mode();
    }

    pub fn jump_to_top(&mut self) {
        if !self.filtered_entries.is_empty() { self.list_state.select(Some(0)); }
    }

    pub fn jump_to_bottom(&mut self) {
        let len = self.filtered_entries.len();
        if len > 0 { self.list_state.select(Some(len - 1)); }
    }

    pub fn enter_ai_prompt_mode(&mut self) {
        self.input_mode = InputMode::AiPromptInput;
        self.input_buffer.clear();
    }

    pub fn exit_ai_prompt_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }
}
