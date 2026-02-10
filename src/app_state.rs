use std::collections::BTreeSet;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use ratatui::prelude::Color;
use ratatui::widgets::ListState;
use regex::Regex;
use tokio::sync::mpsc;

use crate::history::HistoryManager;
use crate::models::{
    AiState, ChatContext, ChatMessage, ChatRole, CurrentView, DashboardStats, DisplayEntry,
    ExportResult, ExportState, ExportType, FileInfo, Focus, InputMode, LevelVisibility, LogEntry,
};
use crate::report::{ReportCache, ReportPeriod};
use crate::search_form::SearchFormState;

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
    }
}

pub struct App {
    pub all_entries: Vec<DisplayEntry>,
    pub filtered_entries: Vec<DisplayEntry>,
    pub list_state: ListState,
    pub focus_mode: FocusModeState,
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
    pub chat_tx: mpsc::Sender<(Vec<ChatMessage>, Vec<LogEntry>)>,
    pub chat_rx: mpsc::Receiver<Result<String, String>>,
    pub export_rx: std_mpsc::Receiver<ExportResult>,
    pub export_tx: std_mpsc::Sender<ExportResult>,
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
    pub error_indices: Vec<usize>,
    pub chart_scroll: usize,
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
    // Report state
    pub report_period: ReportPeriod,
    pub report_content: String,
    pub report_generating: bool,
    pub report_tx: mpsc::Sender<String>,
    pub report_rx: mpsc::Receiver<Result<String, String>>,
    pub report_cache: ReportCache,
}

impl App {
    pub fn new(
        entries: Vec<DisplayEntry>,
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
            filtered_entries: entries,
            list_state,
            focus_mode: FocusModeState::new(),
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
            chat_tx,
            chat_rx,
            export_rx,
            export_tx,
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
            error_indices,
            chart_scroll: 0,
            chat_history: Vec::new(),
            chat_context: ChatContext::default(),
            chat_input: String::new(),
            chat_scroll: 0,
            chat_spinner: 0,
            export_state: ExportState::Idle,
            history: HistoryManager::new(),
            search_form: SearchFormState::new(),
            report_period: ReportPeriod::default(),
            report_content: String::new(),
            report_generating: false,
            report_tx,
            report_rx,
            report_cache: ReportCache::load(),
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

    pub fn entries(&self) -> &Vec<DisplayEntry> {
        &self.filtered_entries
    }

    pub fn next(&mut self) {
        let len = self.filtered_entries.len();
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
        if self.filtered_entries.is_empty() {
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
        let len = self.filtered_entries.len();
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
        if self.filtered_entries.is_empty() {
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

    pub fn selected_entry(&self) -> Option<&DisplayEntry> {
        self.list_state
            .selected()
            .and_then(|i| self.filtered_entries.get(i))
    }

    pub fn toggle_thread_filter(&mut self) {
        if self.filter_tid.is_some() {
            self.filter_tid = None;
            self.apply_filter();
        } else if let Some(tid) = self
            .selected_entry()
            .and_then(|e| e.get_tid())
            .map(String::from)
        {
            self.filter_tid = Some(tid);
            self.apply_filter();
        }
    }

    pub fn apply_filter(&mut self) {
        let enabled_files: Vec<usize> = self
            .files
            .iter()
            .filter(|f| f.enabled)
            .map(|f| f.id)
            .collect();
        self.filtered_entries = self
            .all_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if let Some(sid) = e.get_source_id() {
                    if !enabled_files.contains(&sid) {
                        return false;
                    }
                }
                if let Some(tid) = &self.filter_tid {
                    if e.get_tid() != Some(tid) {
                        return false;
                    }
                }
                if let DisplayEntry::Normal(log) = e {
                    let level = log.level.to_lowercase();
                    if level.contains("info") && !self.visible_levels.info {
                        return false;
                    }
                    if level.contains("warn") && !self.visible_levels.warn {
                        return false;
                    }
                    if level.contains("error") && !self.visible_levels.error {
                        return false;
                    }
                    if level.contains("debug") && !self.visible_levels.debug {
                        return false;
                    }
                }
                true
            })
            .map(|(_, e)| e.clone())
            .collect();
        self.list_state.select(if self.filtered_entries.is_empty() {
            None
        } else {
            Some(0)
        });
        self.update_search_matches();
        self.error_indices = Self::compute_error_indices(&self.filtered_entries);
    }

    pub fn start_search(&mut self) {
        self.search_mode = true;
        self.search_query.clear();
        self.negative_search = false;
    }

    pub fn exit_search(&mut self) {
        self.search_mode = false;
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
        if let Some(re) = &self.search_regex {
            for (i, entry) in self.filtered_entries.iter().enumerate() {
                let matches = re.is_match(&entry.get_searchable_text());
                if self.negative_search {
                    if !matches {
                        self.match_indices.push(i);
                    }
                } else {
                    if matches {
                        self.match_indices.push(i);
                    }
                }
            }
        }
        self.current_match = 0;
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
            }
        }
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
            for (i, f) in self.files.iter_mut().enumerate() {
                f.enabled = i == idx;
            }
            self.apply_filter();
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
            if let Some(idx) = self
                .filtered_entries
                .iter()
                .position(|e| e.get_line_index() == Some(line_num))
            {
                self.list_state.select(Some(idx));
            } else {
                self.status_msg = Some(("Line not found".into(), Instant::now()));
            }
        }
        self.exit_jump_mode();
    }

    pub fn jump_to_top(&mut self) {
        if !self.filtered_entries.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    pub fn jump_to_bottom(&mut self) {
        let len = self.filtered_entries.len();
        if len > 0 {
            self.list_state.select(Some(len - 1));
        }
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

            let filtered_entries = self.filtered_entries.clone();
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
                    for (i, e) in self.filtered_entries.iter().enumerate() {
                        if e.get_line_num() == Some(line) {
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
        // Clone the matching entries to focus_logs
        self.focus_mode.focus_logs = if let Some(re) = &self.search_regex {
            self.filtered_entries
                .iter()
                .filter(|e| {
                    let matches = re.is_match(&e.get_searchable_text());
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
            self.filtered_entries.clone()
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
    }

    /// Update search within focus mode - filters original_focus_logs
    pub fn focus_update_search(&mut self) {
        if self.search_query.is_empty() {
            // Reset to original logs if search is cleared
            self.focus_mode.focus_logs = self.focus_mode.original_focus_logs.clone();
        } else {
            let negative = self.search_query.starts_with('!');
            let pattern = if negative { &self.search_query[1..] } else { &self.search_query };
            
            if let Ok(re) = Regex::new(pattern) {
                self.focus_mode.focus_logs = self.focus_mode.original_focus_logs
                    .iter()
                    .filter(|e| {
                        let matches = re.is_match(&e.get_searchable_text());
                        if negative { !matches } else { matches }
                    })
                    .cloned()
                    .collect();
            }
        }
        
        // Update focus query display
        self.focus_mode.focus_query = if self.search_query.is_empty() {
            "全部".to_string()
        } else {
            self.search_query.clone()
        };

        // Reset selection
        self.focus_mode.focus_table_state = ListState::default();
        if !self.focus_mode.focus_logs.is_empty() {
            self.focus_mode.focus_table_state.select(Some(0));
        }
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

    /// Get the current entries based on view mode
    #[allow(dead_code)]
    pub fn get_current_entries(&self) -> &[DisplayEntry] {
        if self.is_focus_mode() {
            &self.focus_mode.focus_logs
        } else {
            &self.filtered_entries
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
                .and_then(|i| self.filtered_entries.get(i))
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
