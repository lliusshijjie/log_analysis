use std::collections::{BTreeSet, HashSet};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use ratatui::prelude::Color;
use ratatui::widgets::ListState;
use regex::Regex;
use tokio::sync::mpsc;

use crate::filtering::filter_indices;
use crate::history::HistoryManager;
use crate::models::{
    AiState, ChatContext, ChatMessage, ChatRole, CurrentView, DashboardStats, DisplayEntry,
    ExportResult, ExportState, ExportType, FileInfo, Focus, InputMode, LevelVisibility, LogEntry,
};
use crate::report::{ReportCache, ReportPeriod};
use crate::search::SearchCriteria;
use crate::search_form::SearchFormState;

/// Snapshot of focus mode state for back-navigation
pub struct FocusSnapshot {
    pub focus_logs: Vec<DisplayEntry>,
    pub original_focus_logs: Vec<DisplayEntry>,
    pub focus_query: String,
}

/// Focus mode state for isolated search results
#[derive(Default)]
pub struct FocusModeState {
    /// Isolated search results
    pub focus_logs: Vec<DisplayEntry>,
    /// Original focus logs before any sub-search
    pub original_focus_logs: Vec<DisplayEntry>,
    /// Separate scroll state for focus mode
    pub focus_table_state: ListState,
    /// Query that generated the focus results
    pub focus_query: String,
    /// Original match indices from the search
    pub focus_match_indices: Vec<usize>,
    /// Current match index in focus mode
    pub focus_current_match: usize,
    pub copy_input: String,
    /// History stack for browser-like back navigation
    pub history: Vec<FocusSnapshot>,
}

impl FocusModeState {
    pub fn new() -> Self {
        Self {
            focus_logs: Vec::new(),
            original_focus_logs: Vec::new(),
            focus_table_state: ListState::default(),
            focus_query: String::new(),
            focus_match_indices: Vec::new(),
            focus_current_match: 0,
            copy_input: String::new(),
            history: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.focus_logs.clear();
        self.original_focus_logs.clear();
        self.focus_table_state = ListState::default();
        self.focus_query.clear();
        self.focus_match_indices.clear();
        self.focus_current_match = 0;
        self.copy_input.clear();
        self.history.clear();
    }

    /// Push current state onto history stack before narrowing down
    pub fn push_snapshot(&mut self) {
        self.history.push(FocusSnapshot {
            focus_logs: self.focus_logs.clone(),
            original_focus_logs: self.original_focus_logs.clone(),
            focus_query: self.focus_query.clone(),
        });
    }

    /// Pop and restore previous state; returns false if no history (should exit focus)
    pub fn pop_snapshot(&mut self) -> bool {
        if let Some(snapshot) = self.history.pop() {
            self.focus_logs = snapshot.focus_logs;
            self.original_focus_logs = snapshot.original_focus_logs;
            self.focus_query = snapshot.focus_query;
            self.focus_table_state = ListState::default();
            if !self.focus_logs.is_empty() {
                self.focus_table_state.select(Some(0));
            }
            self.focus_match_indices = (0..self.focus_logs.len()).collect();
            self.focus_current_match = 0;
            true
        } else {
            false
        }
    }
}

/// Thread view state for isolated thread logs
#[derive(Default)]
pub struct ThreadViewState {
    /// Isolated thread logs
    pub thread_logs: Vec<DisplayEntry>,
    /// Original thread logs before any sub-search
    pub original_thread_logs: Vec<DisplayEntry>,
    /// Separate scroll state for thread view
    pub thread_table_state: ListState,
    /// Thread ID being displayed
    pub thread_id: String,
    /// Zoom level (1 = normal, 2 = zoomed out more)
    pub zoom_level: u8,
    /// Copy input for line selection
    pub copy_input: String,
}

impl ThreadViewState {
    pub fn new() -> Self {
        Self {
            thread_logs: Vec::new(),
            original_thread_logs: Vec::new(),
            thread_table_state: ListState::default(),
            thread_id: String::new(),
            zoom_level: 1,
            copy_input: String::new(),
        }
    }

    pub fn reset(&mut self) {
        self.thread_logs.clear();
        self.original_thread_logs.clear();
        self.thread_table_state = ListState::default();
        self.thread_id.clear();
        self.zoom_level = 1;
        self.copy_input.clear();
    }

    pub fn zoom_in(&mut self) {
        if self.zoom_level > 1 {
            self.zoom_level -= 1;
        }
    }

    pub fn zoom_out(&mut self) {
        if self.zoom_level < 5 {
            self.zoom_level += 1;
        }
    }
}

/// Floating popup for displaying advanced search results over the main view
pub struct AdvancedResultPopup {
    pub is_open: bool,
    pub logs: Vec<DisplayEntry>,
    pub table_state: ListState,
    pub title: String,
    pub search_mode: bool,
    pub search_query: String,
    pub search_regex: Option<Regex>,
    pub copy_mode: bool,
    pub copy_input: String,
    pub copy_feedback: Option<String>,
    /// Match indices for search navigation within popup
    pub match_indices: Vec<usize>,
    pub current_match: usize,
}

impl Default for AdvancedResultPopup {
    fn default() -> Self {
        Self {
            is_open: false,
            logs: Vec::new(),
            table_state: ListState::default(),
            title: String::new(),
            search_mode: false,
            search_query: String::new(),
            search_regex: None,
            copy_mode: false,
            copy_input: String::new(),
            copy_feedback: None,
            match_indices: Vec::new(),
            current_match: 0,
        }
    }
}

impl AdvancedResultPopup {
    pub fn open(&mut self, logs: Vec<DisplayEntry>, title: String) {
        self.is_open = true;
        self.logs = logs;
        self.title = title;
        self.table_state = ListState::default();
        if !self.logs.is_empty() {
            self.table_state.select(Some(0));
        }
        self.search_mode = false;
        self.search_query.clear();
        self.search_regex = None;
        self.copy_mode = false;
        self.copy_input.clear();
        self.copy_feedback = None;
        self.match_indices.clear();
        self.current_match = 0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.logs.clear();
        self.table_state = ListState::default();
        self.search_query.clear();
        self.search_regex = None;
        self.copy_mode = false;
        self.copy_input.clear();
        self.copy_feedback = None;
        self.match_indices.clear();
        self.current_match = 0;
    }

    pub fn next(&mut self) {
        if self.logs.is_empty() { return; }
        let i = self.table_state.selected().map_or(0, |i| {
            if i + 1 >= self.logs.len() { i } else { i + 1 }
        });
        self.table_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.logs.is_empty() { return; }
        let i = self.table_state.selected().map_or(0, |i| i.saturating_sub(1));
        self.table_state.select(Some(i));
    }

    pub fn next_page(&mut self, page_size: usize) {
        if self.logs.is_empty() { return; }
        let i = self.table_state.selected().map_or(0, |i| {
            (i + page_size).min(self.logs.len() - 1)
        });
        self.table_state.select(Some(i));
    }

    pub fn previous_page(&mut self, page_size: usize) {
        if self.logs.is_empty() { return; }
        let i = self.table_state.selected().map_or(0, |i| i.saturating_sub(page_size));
        self.table_state.select(Some(i));
    }

    pub fn jump_to_top(&mut self) {
        if !self.logs.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    pub fn jump_to_bottom(&mut self) {
        if !self.logs.is_empty() {
            self.table_state.select(Some(self.logs.len() - 1));
        }
    }

    /// Update match indices based on current search_regex
    pub fn update_match_indices(&mut self) {
        self.match_indices.clear();
        if let Some(re) = &self.search_regex {
            for (i, entry) in self.logs.iter().enumerate() {
                if entry.matches_search(re) {
                    self.match_indices.push(i);
                }
            }
        }
        self.current_match = 0;
    }

    /// Navigate to next match
    pub fn next_match(&mut self) {
        if self.match_indices.is_empty() {
            return;
        }
        self.current_match = (self.current_match + 1) % self.match_indices.len();
        self.table_state.select(Some(self.match_indices[self.current_match]));
    }

    /// Navigate to previous match
    pub fn prev_match(&mut self) {
        if self.match_indices.is_empty() {
            return;
        }
        self.current_match = self
            .current_match
            .checked_sub(1)
            .unwrap_or(self.match_indices.len() - 1);
        self.table_state.select(Some(self.match_indices[self.current_match]));
    }
}

pub struct App {
    pub all_entries: Vec<DisplayEntry>,
    pub filtered_indices: Vec<usize>,
    pub raw_entries: Vec<LogEntry>, // Original unfurled entries for thread view
    pub list_state: ListState,
    pub focus_mode: FocusModeState,
    pub thread_view: ThreadViewState,
    pub filter_tid: Option<String>,
    pub filter_trace: Option<String>,
    pub correlation_regexes: Vec<Regex>,
    pub search_mode: bool,
    pub search_query: String,
    pub search_regex: Option<Regex>,
    pub negative_search: bool,
    pub match_indices: Vec<usize>,
    pub match_index_set: HashSet<usize>,
    pub current_match: usize,
    pub status_msg: Option<(String, Instant)>,
    pub clipboard: Option<Clipboard>,
    pub histogram: Vec<(String, u64)>,
    pub ai_state: AiState,
    pub ai_tx: mpsc::Sender<(String, Option<String>)>,
    pub ai_rx: mpsc::Receiver<Result<String, String>>,
    pub chat_tx: mpsc::Sender<(Vec<ChatMessage>, Vec<LogEntry>)>,
    pub chat_rx: mpsc::Receiver<Result<String, String>>,
    pub export_rx: std_mpsc::Receiver<ExportResult>,
    pub export_tx: std_mpsc::Sender<ExportResult>,
    pub bookmarks: BTreeSet<usize>,
    pub bookmark_index_set: HashSet<usize>,
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
    pub error_indices: Vec<usize>,
    pub chart_scroll: usize,
    // Horizontal scroll and wrap settings
    pub horizontal_scroll: usize,
    pub wrap_lines: bool,
    // Chat state
    pub chat_history: Vec<ChatMessage>,
    pub chat_context: ChatContext,
    pub chat_input: String,
    pub chat_scroll: usize,
    pub chat_spinner: usize,
    pub export_state: ExportState,
    pub history: HistoryManager,
    // Advanced search form state
    pub search_form: SearchFormState,
    // Persistent advanced search filter (normal log view)
    pub advanced_search_criteria: Option<SearchCriteria>,
    pub advanced_search_summary: Option<String>,
    // Advanced search result popup
    pub adv_result_popup: AdvancedResultPopup,
    // Shared popup offset for movable floating dialogs
    pub popup_offset_x: i16,
    pub popup_offset_y: i16,
    // Report state
    pub report_period: ReportPeriod,
    pub report_content: String,
    pub report_generating: bool,
    pub report_tx: mpsc::Sender<String>,
    pub report_rx: mpsc::Receiver<Result<String, String>>,
    pub report_cache: ReportCache,
    pub needs_redraw: bool,
}

impl App {
    pub fn new(
        entries: Vec<DisplayEntry>,
        raw_entries: Vec<LogEntry>,
        histogram: Vec<(String, u64)>,
        files: Vec<FileInfo>,
        ai_tx: mpsc::Sender<(String, Option<String>)>,
        ai_rx: mpsc::Receiver<Result<String, String>>,
        chat_tx: mpsc::Sender<(Vec<ChatMessage>, Vec<LogEntry>)>,
        chat_rx: mpsc::Receiver<Result<String, String>>,
        export_rx: std_mpsc::Receiver<ExportResult>,
        export_tx: std_mpsc::Sender<ExportResult>,
        report_tx: mpsc::Sender<String>,
        report_rx: mpsc::Receiver<Result<String, String>>,
        page_size: usize,
    ) -> Self {
        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }
        let mut file_list_state = ListState::default();
        if !files.is_empty() {
            file_list_state.select(Some(0));
        }
        let error_indices = Self::compute_error_indices(&entries);
        Self {
            all_entries: entries.clone(),
            filtered_indices: (0..entries.len()).collect(),
            raw_entries,
            list_state,
            focus_mode: FocusModeState::new(),
            thread_view: ThreadViewState::new(),
            filter_tid: None,
            filter_trace: None,
            correlation_regexes: Vec::new(),
            search_mode: false,
            search_query: String::new(),
            search_regex: None,
            negative_search: false,
            match_indices: Vec::new(),
            match_index_set: HashSet::new(),
            current_match: 0,
            status_msg: None,
            clipboard: Clipboard::new().ok(),
            histogram,
            ai_state: AiState::Idle,
            ai_tx,
            ai_rx,
            chat_tx,
            chat_rx,
            export_rx,
            export_tx,
            bookmarks: BTreeSet::new(),
            bookmark_index_set: HashSet::new(),
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
            error_indices,
            chart_scroll: 0,
            horizontal_scroll: 0,
            wrap_lines: false,
            chat_history: Vec::new(),
            chat_context: ChatContext::default(),
            chat_input: String::new(),
            chat_scroll: 0,
            chat_spinner: 0,
            export_state: ExportState::Idle,
            history: HistoryManager::new(),
            search_form: SearchFormState::new(),
            advanced_search_criteria: None,
            advanced_search_summary: None,
            adv_result_popup: AdvancedResultPopup::default(),
            popup_offset_x: 0,
            popup_offset_y: 0,
            report_period: ReportPeriod::default(),
            report_content: String::new(),
            report_generating: false,
            report_tx,
            report_rx,
            report_cache: ReportCache::load(),
            needs_redraw: true,
        }
    }

    fn compute_error_indices(entries: &[DisplayEntry]) -> Vec<usize> {
        entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                DisplayEntry::Normal(log) if log.level.to_lowercase().contains("error") => Some(i),
                _ => None,
            })
            .collect()
    }

    fn compute_error_indices_from_indices(&self) -> Vec<usize> {
        self.filtered_indices
            .iter()
            .enumerate()
            .filter_map(|(filtered_idx, &all_idx)| match self.all_entries.get(all_idx) {
                Some(DisplayEntry::Normal(log))
                    if matches!(log.level_kind, crate::models::LogLevelKind::Error) =>
                {
                    Some(filtered_idx)
                }
                _ => None,
            })
            .collect()
    }

    pub fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    pub fn filtered_entry(&self, filtered_idx: usize) -> Option<&DisplayEntry> {
        self.filtered_indices
            .get(filtered_idx)
            .and_then(|&idx| self.all_entries.get(idx))
    }

    pub fn filtered_entries_owned(&self) -> Vec<DisplayEntry> {
        self.filtered_indices
            .iter()
            .filter_map(|&idx| self.all_entries.get(idx))
            .cloned()
            .collect()
    }

    pub fn filtered_entries_window_content(&self, start: usize, end: usize) -> String {
        self.filtered_indices[start..end]
            .iter()
            .filter_map(|&idx| self.all_entries.get(idx))
            .map(|e| e.get_content())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn next(&mut self) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let i = self
            .list_state
            .selected()
            .map(|i| (i + 1).min(len - 1))
            .unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let i = self
            .list_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn next_page(&mut self) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let i = self
            .list_state
            .selected()
            .map(|i| i.saturating_add(self.page_size).min(len - 1))
            .unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn previous_page(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let i = self
            .list_state
            .selected()
            .map(|i| i.saturating_sub(self.page_size))
            .unwrap_or(0);
        self.list_state.select(Some(i));
    }

    pub fn scroll_chart_left(&mut self, max_len: usize, view_width: usize) {
        let max_scroll = max_len.saturating_sub(view_width);
        self.chart_scroll = (self.chart_scroll + 1).min(max_scroll);
    }

    pub fn scroll_chart_right(&mut self) {
        self.chart_scroll = self.chart_scroll.saturating_sub(1);
    }

    /// Scroll content horizontally to the right (show more content on the right)
    pub fn scroll_horizontal_right(&mut self, step: usize) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_add(step);
    }

    /// Scroll content horizontally to the left (show more content on the left)
    pub fn scroll_horizontal_left(&mut self, step: usize) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_sub(step);
    }

    /// Reset horizontal scroll to beginning
    pub fn reset_horizontal_scroll(&mut self) {
        self.horizontal_scroll = 0;
    }

    /// Toggle line wrapping mode
    pub fn toggle_wrap_lines(&mut self) {
        self.wrap_lines = !self.wrap_lines;
        if self.wrap_lines {
            // When wrap is enabled, reset horizontal scroll
            self.horizontal_scroll = 0;
        }
        let status = if self.wrap_lines { "已开启" } else { "已关闭" };
        self.status_msg = Some((format!("自动换行: {}", status), Instant::now()));
    }

    pub fn selected_entry(&self) -> Option<&DisplayEntry> {
        self.list_state
            .selected()
            .and_then(|i| self.filtered_entry(i))
    }

    /// Load correlation regex patterns from config
    pub fn load_correlation_patterns(&mut self, patterns: &[String]) {
        self.correlation_regexes = patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
    }

    /// Extract a correlation ID from the selected log entry using configured patterns
    fn extract_correlation_id(&self, entry: &DisplayEntry) -> Option<String> {
        if let DisplayEntry::Normal(log) = entry {
            let text = format!("{} {}", log.content, log.tid);
            for re in &self.correlation_regexes {
                if let Some(caps) = re.captures(&text) {
                    // Try capture group 1 first (named ID), fall back to group 0
                    let id = caps.get(1)
                        .or_else(|| caps.get(0))
                        .map(|m| m.as_str().to_string());
                    if let Some(id) = id {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    pub fn toggle_trace_filter(&mut self) {
        if self.filter_trace.is_some() {
            self.filter_trace = None;
            self.status_msg = Some(("已清除链路追踪".into(), Instant::now()));
            self.apply_filter();
        } else if let Some(entry) = self.selected_entry().cloned() {
            if let Some(trace_id) = self.extract_correlation_id(&entry) {
                self.status_msg = Some((format!("追踪链路: {}", &trace_id), Instant::now()));
                self.filter_trace = Some(trace_id);
                self.apply_filter();
            } else {
                self.status_msg = Some(("未找到关联 ID (traceId/requestId/UUID)".into(), Instant::now()));
            }
        }
    }

    /// Clear trace filter directly (reserved for future use)
    #[allow(dead_code)]
    pub fn clear_trace_filter(&mut self) {
        if self.filter_trace.is_some() {
            self.filter_trace = None;
            self.status_msg = Some(("已清除链路追踪".into(), Instant::now()));
            self.apply_filter();
        }
    }

    pub fn toggle_thread_filter(&mut self) {
        if self.is_thread_view() {
            self.exit_thread_view();
        } else {
            self.enter_thread_view();
        }
    }

    pub fn apply_filter(&mut self) {
        let enabled_files: HashSet<usize> = self
            .files
            .iter()
            .filter(|f| f.enabled)
            .map(|f| f.id)
            .collect();
        let mut filtered_indices: Vec<usize> = self
            .all_entries
            .iter()
            .enumerate()
            .filter_map(|(idx, e)| {
                if let Some(sid) = e.get_source_id() {
                    if !enabled_files.contains(&sid) {
                        return None;
                    }
                }
                if let Some(tid) = &self.filter_tid {
                    if e.get_tid() != Some(tid) {
                        return None;
                    }
                }
                if let Some(trace_id) = &self.filter_trace {
                    if let DisplayEntry::Normal(log) = e {
                        if !log.content.contains(trace_id.as_str())
                            && !log.tid.contains(trace_id.as_str())
                        {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                if let DisplayEntry::Normal(log) = e {
                    use crate::models::LogLevelKind;
                    if matches!(log.level_kind, LogLevelKind::Info) && !self.visible_levels.info {
                        return None;
                    }
                    if matches!(log.level_kind, LogLevelKind::Warn) && !self.visible_levels.warn {
                        return None;
                    }
                    if matches!(log.level_kind, LogLevelKind::Error) && !self.visible_levels.error {
                        return None;
                    }
                    if matches!(log.level_kind, LogLevelKind::Debug) && !self.visible_levels.debug {
                        return None;
                    }
                }
                Some(idx)
            })
            .collect();
        if let Some(criteria) = &self.advanced_search_criteria {
            filtered_indices = filter_indices(&self.all_entries, &filtered_indices, criteria);
        }
        self.filtered_indices = filtered_indices;
        self.list_state.select(if self.filtered_indices.is_empty() {
            None
        } else {
            Some(0)
        });
        self.update_search_matches();
        self.error_indices = self.compute_error_indices_from_indices();
        self.needs_redraw = true;
    }

    pub fn clear_advanced_search(&mut self) {
        self.advanced_search_criteria = None;
        self.advanced_search_summary = None;
        self.apply_filter();
        self.status_msg = Some(("已清除高级搜索条件".into(), Instant::now()));
    }

    pub fn move_popup(&mut self, dx: i16, dy: i16) {
        self.popup_offset_x = self.popup_offset_x.saturating_add(dx);
        self.popup_offset_y = self.popup_offset_y.saturating_add(dy);
    }

    pub fn reset_popup_position(&mut self) {
        self.popup_offset_x = 0;
        self.popup_offset_y = 0;
    }

    pub fn start_search(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
        self.negative_search = false;
    }

    pub fn exit_search(&mut self) {
        self.search_mode = false;
    }

    /// Clear search results and exit search mode (called on Esc)
    pub fn clear_search(&mut self) {
        self.search_mode = false;
        self.search_regex = None;
        self.match_indices.clear();
        self.match_index_set.clear();
        self.needs_redraw = true;
    }

    pub fn update_search(&mut self) {
        if self.search_query.starts_with('!') {
            self.negative_search = true;
            let pattern = &self.search_query[1..];
            self.search_regex = if pattern.is_empty() {
                None
            } else {
                Regex::new(pattern).ok()
            };
        } else {
            self.negative_search = false;
            self.search_regex = Regex::new(&self.search_query).ok();
        }
        self.update_search_matches();
    }

    pub fn update_search_matches(&mut self) {
        self.match_indices.clear();
        self.match_index_set.clear();
        if let Some(re) = &self.search_regex {
            for (i, &all_idx) in self.filtered_indices.iter().enumerate() {
                let Some(entry) = self.all_entries.get(all_idx) else {
                    continue;
                };
                let matches = entry.matches_search(re);
                if self.negative_search {
                    if !matches {
                        self.match_indices.push(i);
                        self.match_index_set.insert(i);
                    }
                } else {
                    if matches {
                        self.match_indices.push(i);
                        self.match_index_set.insert(i);
                    }
                }
            }
        }
        self.current_match = 0;
        // Select the first match if there are any matches
        if !self.match_indices.is_empty() {
            self.list_state.select(Some(self.match_indices[0]));
            self.status_msg = Some((format!("{} 个匹配", self.match_indices.len()), Instant::now()));
        } else {
            self.status_msg = Some(("No Result".into(), Instant::now()));
        }
        self.needs_redraw = true;
    }

    pub fn next_match(&mut self) {
        if self.match_indices.is_empty() {
            return;
        }
        self.current_match = (self.current_match + 1) % self.match_indices.len();
        self.list_state
            .select(Some(self.match_indices[self.current_match]));
    }

    pub fn prev_match(&mut self) {
        if self.match_indices.is_empty() {
            return;
        }
        self.current_match = self
            .current_match
            .checked_sub(1)
            .unwrap_or(self.match_indices.len() - 1);
        self.list_state
            .select(Some(self.match_indices[self.current_match]));
    }

    pub fn toggle_bookmark(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if !self.bookmarks.remove(&idx) {
                self.bookmarks.insert(idx);
                self.bookmark_index_set.insert(idx);
            } else {
                self.bookmark_index_set.remove(&idx);
            }
        }
        self.needs_redraw = true;
    }

    pub fn next_bookmark(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = self
            .bookmarks
            .range((current + 1)..)
            .next()
            .or_else(|| self.bookmarks.iter().next());
        if let Some(&idx) = next {
            self.list_state.select(Some(idx));
        }
    }

    pub fn prev_bookmark(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = self
            .bookmarks
            .range(..current)
            .next_back()
            .or_else(|| self.bookmarks.iter().next_back());
        if let Some(&idx) = prev {
            self.list_state.select(Some(idx));
        }
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
            DisplayEntry::Normal(log) => format!(
                "{} [{}:{}][{}]: {} ({}:{})",
                log.timestamp,
                log.pid,
                log.tid,
                log.level,
                log.content,
                log.source_file,
                log.line_num
            ),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        });
        if let (Some(clip), Some(text)) = (self.clipboard.as_mut(), text) {
            if clip.set_text(text).is_ok() {
                self.status_msg = Some(("Copied!".into(), Instant::now()));
            }
        }
    }

    pub fn yank_payload(&mut self) {
        let text = self.selected_entry().map(|entry| match entry {
            DisplayEntry::Normal(log) => log
                .json_payload
                .as_ref()
                .map(|j| serde_json::to_string_pretty(j).unwrap_or_default())
                .unwrap_or_else(|| log.content.clone()),
            DisplayEntry::Folded { summary_text, .. } => summary_text.clone(),
        });
        if let (Some(clip), Some(text)) = (self.clipboard.as_mut(), text) {
            if clip.set_text(text).is_ok() {
                self.status_msg = Some(("Yanked!".into(), Instant::now()));
            }
        }
    }

    pub fn status_message(&self) -> Option<&str> {
        self.status_msg
            .as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_secs(2))
            .map(|(s, _)| s.as_str())
    }

    pub fn toggle_file(&mut self) {
        if let Some(idx) = self.file_list_state.selected() {
            if let Some(f) = self.files.get_mut(idx) {
                f.enabled = !f.enabled;
            }
            self.apply_filter();
        }
    }

    pub fn solo_file(&mut self) {
        if let Some(idx) = self.file_list_state.selected() {
            let is_marked = self.files.get(idx).map(|f| f.marked).unwrap_or(false);

            if is_marked {
                // Second Enter: activate solo mode for the marked file
                for (i, file) in self.files.iter_mut().enumerate() {
                    file.enabled = i == idx;
                    // Keep marked=true to show the dot indicator for solo file
                }
                self.apply_filter();
                // Auto-switch focus to LogList after solo activation
                self.focus = Focus::LogList;
            } else {
                // First Enter: mark the file with a dot
                // Clear any other marked files first
                for file in self.files.iter_mut() {
                    file.marked = false;
                }
                if let Some(f) = self.files.get_mut(idx) {
                    f.marked = true;
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_file_color(&self, source_id: usize) -> Color {
        self.files
            .iter()
            .find(|f| f.id == source_id)
            .map(|f| f.color)
            .unwrap_or(Color::White)
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
            if let Some(idx) = self.filtered_indices.iter().position(|&all_idx| {
                self.all_entries
                    .get(all_idx)
                    .and_then(DisplayEntry::get_line_index)
                    == Some(line_num)
            }) {
                self.list_state.select(Some(idx));
            } else {
                self.status_msg = Some(("Line not found".into(), Instant::now()));
            }
        }
        self.exit_jump_mode();
        self.needs_redraw = true;
    }

    pub fn jump_to_top(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    pub fn jump_to_bottom(&mut self) {
        let len = self.filtered_indices.len();
        if len > 0 {
            self.list_state.select(Some(len - 1));
        }
        self.needs_redraw = true;
    }

    pub fn enter_ai_prompt_mode(&mut self) {
        self.input_mode = InputMode::AiPromptInput;
        self.input_buffer.clear();
    }

    pub fn exit_ai_prompt_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    // Chat methods
    pub fn pin_selected_log(&mut self) {
        if let Some(DisplayEntry::Normal(log)) = self.selected_entry().cloned() {
            if !self
                .chat_context
                .pinned_logs
                .iter()
                .any(|l| l.line_index == log.line_index && l.source_id == log.source_id)
            {
                self.chat_context.pinned_logs.push(log);
                self.status_msg = Some(("Pinned to chat".into(), Instant::now()));
            }
        }
    }

    pub fn clear_chat_context(&mut self) {
        self.chat_context.pinned_logs.clear();
        self.status_msg = Some(("Context cleared".into(), Instant::now()));
    }

    pub fn clear_chat_history(&mut self) {
        self.chat_history.clear();
        self.chat_scroll = 0;
    }

    pub fn submit_chat(&mut self) {
        let msg = self.chat_input.trim();
        if msg.is_empty() {
            return;
        }
        self.chat_history.push(ChatMessage {
            role: ChatRole::User,
            content: msg.to_string(),
        });
        self.chat_input.clear();
        let data = (
            self.chat_history.clone(),
            self.chat_context.pinned_logs.clone(),
        );
        // Use try_send to avoid blocking the UI thread
        match self.chat_tx.try_send(data) {
            Ok(()) => {
                self.ai_state = AiState::Loading;
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.status_msg = Some(("AI 正忙，请稍后重试".into(), Instant::now()));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.status_msg = Some(("AI 服务不可用".into(), Instant::now()));
            }
        }
        self.chat_scroll_to_bottom();
    }

    pub fn receive_chat_response(&mut self, response: String) {
        self.chat_history.push(ChatMessage {
            role: ChatRole::Assistant,
            content: response,
        });
        self.ai_state = AiState::Idle;
        self.chat_scroll_to_bottom();
    }

    pub fn chat_scroll_up(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_add(1);
    }

    pub fn chat_scroll_down(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_sub(1);
    }

    pub fn chat_scroll_to_bottom(&mut self) {
        self.chat_scroll = 0;
    }

    pub fn tick_spinner(&mut self) {
        self.chat_spinner = (self.chat_spinner + 1) % 10;
    }

    pub fn request_export(&mut self, export_type: ExportType) {
        self.export_state = ExportState::Confirm(export_type);
    }

    pub fn confirm_export(&mut self) {
        if let ExportState::Confirm(export_type) = self.export_state.clone() {
            self.export_state = ExportState::Exporting(export_type.clone());

            let filtered_entries = self.filtered_entries_owned();
            let stats = self.stats.clone();
            let chat_history = self.chat_history.clone();
            let export_type_clone = export_type.clone();
            let tx = self.export_tx.clone();

            std::thread::spawn(move || {
                let result = match crate::export::perform_export(
                    export_type_clone,
                    &filtered_entries,
                    &stats,
                    &chat_history,
                ) {
                    Ok(filename) => ExportResult::Success(filename),
                    Err(e) => ExportResult::Error(e.to_string()),
                };
                let _ = tx.send(result);
            });
        }
    }

    pub fn cancel_export(&mut self) {
        self.export_state = ExportState::Idle;
    }

    pub fn execute_history_entry(&mut self, entry: &crate::history::HistoryEntry) {
        use crate::history::CommandType;
        match entry.kind {
            CommandType::Search => {
                self.current_view = CurrentView::Logs;
                self.search_query = entry.content.clone();
                self.update_search();
            }
            CommandType::Jump => {
                self.current_view = CurrentView::Logs;
                if let Ok(line) = entry.content.parse::<u32>() {
                    for (i, &all_idx) in self.filtered_indices.iter().enumerate() {
                        if self
                            .all_entries
                            .get(all_idx)
                            .and_then(DisplayEntry::get_line_num)
                            == Some(line)
                        {
                            self.list_state.select(Some(i));
                            break;
                        }
                    }
                }
            }
            CommandType::AiPrompt => {
                self.current_view = CurrentView::Chat;
                self.chat_input = entry.content.clone();
            }
        }
    }

    // ========== Focus Mode Methods ==========

    /// Enter focus mode with the current search query
    /// Creates a filtered view containing only matching log lines
    pub fn enter_focus_mode(&mut self, query: String) {
        let filtered_entries = self.filtered_entries_owned();
        // Clone the matching entries to focus_logs
        self.focus_mode.focus_logs = if let Some(re) = &self.search_regex {
            filtered_entries
                .iter()
                .filter(|e| {
                    let matches = e.matches_search(re);
                    if self.negative_search {
                        !matches
                    } else {
                        matches
                    }
                })
                .cloned()
                .collect()
        } else {
            // If no search regex, enter focus mode with all currently filtered entries
            filtered_entries
        };

        // Store original focus logs for sub-search
        self.focus_mode.original_focus_logs = self.focus_mode.focus_logs.clone();

        // Store the query for display
        self.focus_mode.focus_query = query;

        // Reset focus table state and select first item
        self.focus_mode.focus_table_state = ListState::default();
        if !self.focus_mode.focus_logs.is_empty() {
            self.focus_mode.focus_table_state.select(Some(0));
        }

        // Store match indices (all indices in focus mode are "matches")
        self.focus_mode.focus_match_indices = (0..self.focus_mode.focus_logs.len()).collect();
        self.focus_mode.focus_current_match = 0;

        // Switch to focus view
        self.current_view = CurrentView::Focus;
        self.needs_redraw = true;
    }

    /// Update search within focus mode - filters original_focus_logs
    pub fn focus_update_search(&mut self) {
        if self.search_query.is_empty() {
            return;
        }

        // Save current state before narrowing down
        self.focus_mode.push_snapshot();

        let negative = self.search_query.starts_with('!');
        let pattern = if negative { &self.search_query[1..] } else { &self.search_query };
        
        if let Ok(re) = Regex::new(pattern) {
            let filtered: Vec<DisplayEntry> = self.focus_mode.original_focus_logs
                .iter()
                .filter(|e| {
                    let matches = e.matches_search(&re);
                    if negative { !matches } else { matches }
                })
                .cloned()
                .collect();
            self.focus_mode.focus_logs = filtered;
        }

        // The new filtered set becomes the base for further sub-searches
        self.focus_mode.original_focus_logs = self.focus_mode.focus_logs.clone();
        self.focus_mode.focus_query = self.search_query.clone();

        // Reset selection
        self.focus_mode.focus_table_state = ListState::default();
        if !self.focus_mode.focus_logs.is_empty() {
            self.focus_mode.focus_table_state.select(Some(0));
        }
        self.focus_mode.focus_match_indices = (0..self.focus_mode.focus_logs.len()).collect();
        self.focus_mode.focus_current_match = 0;
        self.needs_redraw = true;
    }

    /// Go back one level in focus mode history; returns false if no history left
    pub fn focus_go_back(&mut self) -> bool {
        self.focus_mode.pop_snapshot()
    }

    /// Exit focus mode and return to normal log view
    pub fn exit_focus_mode(&mut self) {
        self.focus_mode.reset();
        self.current_view = CurrentView::Logs;
    }

    /// Check if we're currently in focus mode
    #[allow(dead_code)]
    pub fn is_focus_mode(&self) -> bool {
        matches!(self.current_view, CurrentView::Focus)
    }

    /// Check if we're currently in thread view
    #[allow(dead_code)]
    pub fn is_thread_view(&self) -> bool {
        matches!(self.current_view, CurrentView::Thread)
    }

    /// Enter thread view with logs from the selected thread
    pub fn enter_thread_view(&mut self) {
        let tid = match self
            .selected_entry()
            .and_then(|e| e.get_tid())
            .map(String::from)
        {
            Some(tid) => tid,
            None => return,
        };

        // Filter raw_entries by thread ID and convert to DisplayEntry::Normal
        let thread_logs: Vec<DisplayEntry> = self
            .raw_entries
            .iter()
            .filter(|log| log.tid == tid)
            .map(|log| DisplayEntry::Normal(log.clone()))
            .collect();

        if thread_logs.is_empty() {
            return;
        }

        self.thread_view.thread_logs = thread_logs.clone();
        self.thread_view.original_thread_logs = thread_logs;
        self.thread_view.thread_id = tid;
        self.thread_view.thread_table_state = ListState::default();
        self.thread_view.thread_table_state.select(Some(0));
        self.thread_view.zoom_level = 1;

        self.current_view = CurrentView::Thread;
    }

    /// Exit thread view and return to normal log view
    pub fn exit_thread_view(&mut self) {
        self.thread_view.reset();
        self.current_view = CurrentView::Logs;
    }

    /// Update search within thread view
    pub fn thread_update_search(&mut self) {
        if self.search_query.is_empty() {
            self.thread_view.thread_logs = self.thread_view.original_thread_logs.clone();
        } else {
            let negative = self.search_query.starts_with('!');
            let pattern = if negative { &self.search_query[1..] } else { &self.search_query };

            if let Ok(re) = Regex::new(pattern) {
                self.thread_view.thread_logs = self
                    .thread_view
                    .original_thread_logs
                    .iter()
                    .filter(|e| {
                        let matches = e.matches_search(&re);
                        if negative { !matches } else { matches }
                    })
                    .cloned()
                    .collect();
            }
        }

        self.thread_view.thread_table_state = ListState::default();
        if !self.thread_view.thread_logs.is_empty() {
            self.thread_view.thread_table_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Navigation methods for thread view
    pub fn thread_next(&mut self) {
        let len = self.thread_view.thread_logs.len();
        if len == 0 {
            return;
        }
        let i = self
            .thread_view
            .thread_table_state
            .selected()
            .map(|i| (i + 1).min(len - 1))
            .unwrap_or(0);
        self.thread_view.thread_table_state.select(Some(i));
    }

    pub fn thread_previous(&mut self) {
        if self.thread_view.thread_logs.is_empty() {
            return;
        }
        let i = self
            .thread_view
            .thread_table_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.thread_view.thread_table_state.select(Some(i));
    }

    pub fn thread_next_page(&mut self) {
        let len = self.thread_view.thread_logs.len();
        if len == 0 {
            return;
        }
        let i = self
            .thread_view
            .thread_table_state
            .selected()
            .map(|i| i.saturating_add(self.page_size).min(len - 1))
            .unwrap_or(0);
        self.thread_view.thread_table_state.select(Some(i));
    }

    pub fn thread_previous_page(&mut self) {
        if self.thread_view.thread_logs.is_empty() {
            return;
        }
        let i = self
            .thread_view
            .thread_table_state
            .selected()
            .map(|i| i.saturating_sub(self.page_size))
            .unwrap_or(0);
        self.thread_view.thread_table_state.select(Some(i));
    }

    pub fn thread_jump_to_top(&mut self) {
        if !self.thread_view.thread_logs.is_empty() {
            self.thread_view.thread_table_state.select(Some(0));
        }
    }

    pub fn thread_jump_to_bottom(&mut self) {
        let len = self.thread_view.thread_logs.len();
        if len > 0 {
            self.thread_view.thread_table_state.select(Some(len - 1));
        }
    }

    /// Zoom in (show fewer lines per entry)
    pub fn thread_zoom_in(&mut self) {
        self.thread_view.zoom_in();
    }

    /// Zoom out (show more lines per entry)
    pub fn thread_zoom_out(&mut self) {
        self.thread_view.zoom_out();
    }

    /// Get the current entries based on view mode
    #[allow(dead_code)]
    pub fn get_current_entries(&self) -> Vec<&DisplayEntry> {
        if self.is_focus_mode() {
            self.focus_mode.focus_logs.iter().collect()
        } else {
            self.filtered_indices
                .iter()
                .filter_map(|&idx| self.all_entries.get(idx))
                .collect()
        }
    }

    /// Get the current list state based on view mode
    #[allow(dead_code)]
    pub fn get_current_list_state(&mut self) -> &mut ListState {
        if self.is_focus_mode() {
            &mut self.focus_mode.focus_table_state
        } else {
            &mut self.list_state
        }
    }

    /// Get the selected entry in the current view
    #[allow(dead_code)]
    pub fn get_current_selected(&self) -> Option<&DisplayEntry> {
        if self.is_focus_mode() {
            self.focus_mode
                .focus_table_state
                .selected()
                .and_then(|i| self.focus_mode.focus_logs.get(i))
        } else {
            self.list_state
                .selected()
                .and_then(|i| self.filtered_entry(i))
        }
    }

    /// Navigation methods for focus mode
    pub fn focus_next(&mut self) {
        let len = self.focus_mode.focus_logs.len();
        if len == 0 {
            return;
        }
        let i = self
            .focus_mode
            .focus_table_state
            .selected()
            .map(|i| (i + 1).min(len - 1))
            .unwrap_or(0);
        self.focus_mode.focus_table_state.select(Some(i));
    }

    pub fn focus_previous(&mut self) {
        if self.focus_mode.focus_logs.is_empty() {
            return;
        }
        let i = self
            .focus_mode
            .focus_table_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.focus_mode.focus_table_state.select(Some(i));
    }

    pub fn focus_next_page(&mut self) {
        let len = self.focus_mode.focus_logs.len();
        if len == 0 {
            return;
        }
        let i = self
            .focus_mode
            .focus_table_state
            .selected()
            .map(|i| i.saturating_add(self.page_size).min(len - 1))
            .unwrap_or(0);
        self.focus_mode.focus_table_state.select(Some(i));
    }

    pub fn focus_previous_page(&mut self) {
        if self.focus_mode.focus_logs.is_empty() {
            return;
        }
        let i = self
            .focus_mode
            .focus_table_state
            .selected()
            .map(|i| i.saturating_sub(self.page_size))
            .unwrap_or(0);
        self.focus_mode.focus_table_state.select(Some(i));
    }

    pub fn focus_jump_to_top(&mut self) {
        if !self.focus_mode.focus_logs.is_empty() {
            self.focus_mode.focus_table_state.select(Some(0));
        }
    }

    pub fn focus_jump_to_bottom(&mut self) {
        let len = self.focus_mode.focus_logs.len();
        if len > 0 {
            self.focus_mode.focus_table_state.select(Some(len - 1));
        }
    }

    /// Get bookmarks in the current view
    #[allow(dead_code)]
    pub fn get_current_bookmarks(&self) -> &BTreeSet<usize> {
        if self.is_focus_mode() {
            // In focus mode, we use the main bookmarks set
            &self.bookmarks
        } else {
            &self.bookmarks
        }
    }
}
