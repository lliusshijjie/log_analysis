use std::io::Stdout;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use regex::Regex;

use super::chat::render_chat_interface;
use super::components::{
    render_adv_result_popup, render_ai_popup, render_ai_prompt_popup, render_detail_pane,
    render_export_popup, render_focus_list, render_help_popup, render_histogram,
    render_jump_popup, render_log_list_from_app, render_search_bar, render_sidebar,
    render_thread_list,
};
use super::dashboard::{render_dashboard, render_header};
use super::layout::{centered_rect_with_offset, create_focus_layout, create_layout};
use super::search_modal::render_search_modal;
use crate::app_state::App;
use crate::filtering::filter_logs_owned;
use crate::live::TailState;
use crate::models::{
    AiState, CurrentView, DisplayEntry, ExportResult, ExportState, ExportType, Focus, InputMode,
};
use crate::search::{LogLevel, SearchCriteria};
use crate::search_form::{FormField, SearchFormState, TemplateMode};
use crate::templates::{get_template, get_template_names, save_template};
use crate::time_parser::parse_user_time;

fn parse_copy_indices(input: &str, total: usize) -> Option<Vec<usize>> {
    if total == 0 {
        return None;
    }
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    if matches!(normalized.as_str(), "*" | "a" | "all") {
        return Some((1..=total).collect());
    }

    let mut indices: Vec<usize> = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(start), Ok(end)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                if start <= end {
                    for i in start..=end {
                        indices.push(i);
                    }
                } else {
                    for i in end..=start {
                        indices.push(i);
                    }
                }
            }
        } else if let Ok(n) = part.parse::<usize>() {
            indices.push(n);
        }
    }

    indices.sort_unstable();
    indices.dedup();
    indices.retain(|&i| i >= 1 && i <= total);
    if indices.is_empty() {
        None
    } else {
        Some(indices)
    }
}

fn build_copy_text(entries: &[DisplayEntry], indices: &[usize]) -> String {
    indices
        .iter()
        .map(|&i| entries[i - 1].get_content())
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_advanced_search_criteria(form: &SearchFormState) -> std::result::Result<SearchCriteria, String> {
    let mut criteria = SearchCriteria::default();

    let start_raw = form.start_time_input.trim();
    if !start_raw.is_empty() {
        let start = parse_user_time(start_raw)
            .ok_or_else(|| format!("无效的开始时间: {}", start_raw))?;
        criteria.start_time = Some(start);
    }

    let end_raw = form.end_time_input.trim();
    if !end_raw.is_empty() {
        let end = parse_user_time(end_raw)
            .ok_or_else(|| format!("无效的结束时间: {}", end_raw))?;
        criteria.end_time = Some(end);
    }

    if let (Some(start), Some(end)) = (criteria.start_time, criteria.end_time) {
        if start > end {
            return Err("开始时间不能晚于结束时间".to_string());
        }
    }

    let content_raw = form.content_input.trim();
    if !content_raw.is_empty() {
        let normalized = if let Some(stripped) = content_raw.strip_prefix('/') {
            stripped
        } else {
            content_raw
        };
        if normalized.is_empty() {
            return Err("内容正则不能为空".to_string());
        }
        Regex::new(normalized).map_err(|e| format!("无效的内容正则: {}", e))?;
        criteria.content_regex = Some(normalized.to_string());
    }

    let source_raw = form.source_input.trim();
    if !source_raw.is_empty() {
        criteria.source_file = Some(source_raw.to_string());
    }

    criteria.levels = form.selected_levels.iter().cloned().collect();
    Ok(criteria)
}

fn build_advanced_search_summary(criteria: &SearchCriteria) -> Option<String> {
    if criteria.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(start) = criteria.start_time {
        parts.push(format!("start>={}", start.format("%m-%d %H:%M")));
    }
    if let Some(end) = criteria.end_time {
        parts.push(format!("end<={}", end.format("%m-%d %H:%M")));
    }
    if let Some(pattern) = &criteria.content_regex {
        parts.push(format!("re={}", pattern));
    }
    if let Some(source) = &criteria.source_file {
        parts.push(format!("src~{}", source));
    }
    if !criteria.levels.is_empty() {
        let levels = criteria
            .levels
            .iter()
            .map(|lv| match lv {
                LogLevel::Debug => "D",
                LogLevel::Info => "I",
                LogLevel::Warn => "W",
                LogLevel::Error => "E",
            })
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("lvl={}", levels));
    }
    Some(parts.join(" | "))
}

fn is_movable_popup_active(app: &App) -> bool {
    app.search_form.is_open || app.input_mode == InputMode::FocusCopyInput || app.adv_result_popup.is_open
}

fn apply_advanced_search(app: &mut App, criteria: &SearchCriteria) -> usize {
    match app.current_view {
        CurrentView::Focus => {
            app.focus_mode.push_snapshot();
            let base = app.focus_mode.focus_logs.clone();
            app.focus_mode.focus_logs = filter_logs_owned(&base, criteria);
            app.focus_mode.original_focus_logs = app.focus_mode.focus_logs.clone();
            app.focus_mode.focus_table_state.select(if app.focus_mode.focus_logs.is_empty() {
                None
            } else {
                Some(0)
            });
            app.focus_mode.focus_logs.len()
        }
        CurrentView::Thread => {
            let base = app.thread_view.thread_logs.clone();
            app.thread_view.thread_logs = filter_logs_owned(&base, criteria);
            app.thread_view.original_thread_logs = app.thread_view.thread_logs.clone();
            app.thread_view.thread_table_state.select(if app.thread_view.thread_logs.is_empty() {
                None
            } else {
                Some(0)
            });
            app.thread_view.thread_logs.len()
        }
        _ => {
            let results = filter_logs_owned(&app.filtered_entries, criteria);
            let summary = build_advanced_search_summary(criteria)
                .unwrap_or_else(|| "高级搜索".to_string());
            let title = format!("高级搜索: {}", summary);
            let count = results.len();
            if count > 0 {
                app.adv_result_popup.open(results, title);
            } else {
                app.status_msg = Some(("高级搜索无匹配结果".into(), Instant::now()));
            }
            count
        }
    }
}


fn ui(frame: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(frame.area());

    render_header(frame, app, main_chunks[0]);

    match app.current_view {
        CurrentView::Logs => {
            let layout = create_layout(main_chunks[1], app.search_mode);
            render_sidebar(frame, app, layout.sidebar);
            render_log_list_from_app(frame, app, layout.log_list);
            if app.search_mode {
                render_search_bar(frame, app, layout.search_bar);
            }
            render_detail_pane(frame, app, layout.detail);
            render_histogram(frame, app, layout.histogram);
        }
        CurrentView::Focus => {
            // Focus mode: full-width layout without sidebar
            let focus_layout = create_focus_layout(main_chunks[1], app.search_mode);
            render_focus_list(frame, app, focus_layout.log_list);
            if app.search_mode {
                render_search_bar(frame, app, focus_layout.search_bar);
            }
            render_detail_pane(frame, app, focus_layout.detail);
            if app.input_mode == InputMode::FocusCopyInput {
                let popup_area = centered_rect_with_offset(
                    40,
                    15,
                    frame.area(),
                    app.popup_offset_x,
                    app.popup_offset_y,
                );
                frame.render_widget(Clear, popup_area);
                let display_text = if app.focus_mode.copy_input.is_empty() {
                    Span::styled("请输入行号, 如: 1-5, 3, 7-10, *", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw(app.focus_mode.copy_input.clone())
                };
                let input = Paragraph::new(Line::from(display_text))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .title(" 复制行号 (Alt+方向键移动 | Ctrl+0复位) ")
                            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    );
                frame.render_widget(input, popup_area);
                frame.set_cursor_position((
                    popup_area.x + app.focus_mode.copy_input.len() as u16 + 1,
                    popup_area.y + 1,
                ));
            }
        }
        CurrentView::Thread => {
            // Thread view: full-width layout showing all logs from a thread
            let thread_layout = create_focus_layout(main_chunks[1], app.search_mode);
            render_thread_list(frame, app, thread_layout.log_list);
            if app.search_mode {
                render_search_bar(frame, app, thread_layout.search_bar);
            }
            render_detail_pane(frame, app, thread_layout.detail);
            if app.input_mode == InputMode::FocusCopyInput {
                let popup_area = centered_rect_with_offset(
                    40,
                    15,
                    frame.area(),
                    app.popup_offset_x,
                    app.popup_offset_y,
                );
                frame.render_widget(Clear, popup_area);
                let display_text = if app.thread_view.copy_input.is_empty() {
                    Span::styled("请输入行号, 如: 1-5, 3, 7-10, *", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw(app.thread_view.copy_input.clone())
                };
                let input = Paragraph::new(Line::from(display_text))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .title(" 复制行号 (Alt+方向键移动 | Ctrl+0复位) ")
                            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    );
                frame.render_widget(input, popup_area);
                frame.set_cursor_position((
                    popup_area.x + app.thread_view.copy_input.len() as u16 + 1,
                    popup_area.y + 1,
                ));
            }
        }
        CurrentView::Dashboard => {
            render_dashboard(frame, app, main_chunks[1]);
        }
        CurrentView::Chat => {
            render_chat_interface(frame, app, main_chunks[1]);
        }
        CurrentView::History => {
            super::history::render_history(frame, app, main_chunks[1]);
        }
        CurrentView::Report => {
            super::report::render_report(frame, app, main_chunks[1]);
        }
    }
    render_ai_popup(frame, app);
    if app.show_help {
        render_help_popup(frame);
    }
    render_jump_popup(frame, app);
    render_ai_prompt_popup(frame, app);
    render_export_popup(frame, app);
    render_adv_result_popup(frame, app);
    render_search_modal(frame, app);
}

pub fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    file_rx: Receiver<Vec<PathBuf>>,
    tail_state: &mut TailState,
    file_paths: &[PathBuf],
    re: &Regex,
) -> Result<()> {
    loop {
        // State updates
        if let Ok(result) = app.ai_rx.try_recv() {
            app.ai_state = match result {
                Ok(s) => AiState::Completed(s),
                Err(e) => AiState::Error(e),
            };
        }
        if let Ok(result) = app.chat_rx.try_recv() {
            match result {
                Ok(s) => app.receive_chat_response(s),
                Err(e) => {
                    app.receive_chat_response(format!("Error: {}", e));
                    app.ai_state = AiState::Idle;
                }
            }
        }
        if let Ok(result) = app.export_rx.try_recv() {
            app.export_state = match result {
                ExportResult::Success(filename) => ExportState::Success(filename),
                ExportResult::Error(e) => ExportState::Error(e),
            };
        }
        // Poll report generation result
        if let Ok(result) = app.report_rx.try_recv() {
            app.report_generating = false;
            match result {
                Ok(content) => {
                    app.report_content = content.clone();
                    app.report_cache.set(app.report_period, content);
                }
                Err(e) => app.report_content = format!("生成报告失败: {}", e),
            }
        }
        if matches!(app.ai_state, AiState::Loading) && app.current_view == CurrentView::Chat {
            app.tick_spinner();
        }

        if app.is_tailing {
            while let Ok(paths) = file_rx.try_recv() {
                for changed_path in paths {
                    if let Some((source_id, path)) = file_paths
                        .iter()
                        .enumerate()
                        .find(|(_, p)| p.as_path() == changed_path.as_path())
                    {
                        let base_idx = app.all_entries.len();
                        let new_entries = tail_state.read_new_lines(path, source_id, &re, base_idx);
                        for entry in new_entries {
                            let display = DisplayEntry::Normal(entry);
                            app.all_entries.push(display.clone());
                            app.filtered_entries.push(display);
                        }
                    }
                }
            }
            let len = app.filtered_entries.len();
            if len > 0 {
                app.list_state.select(Some(len - 1));
            }
        }

        // Render UI
        terminal.draw(|f| ui(f, app))?;

        // Drain all pending key events before next render
        if !event::poll(Duration::from_millis(16))? {
            continue;
        }
        while event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if is_movable_popup_active(app) {
                    if key.modifiers.contains(KeyModifiers::ALT) {
                        match key.code {
                            KeyCode::Left => {
                                app.move_popup(-8, 0);
                                continue;
                            }
                            KeyCode::Right => {
                                app.move_popup(8, 0);
                                continue;
                            }
                            KeyCode::Up => {
                                app.move_popup(0, -2);
                                continue;
                            }
                            KeyCode::Down => {
                                app.move_popup(0, 2);
                                continue;
                            }
                            _ => {}
                        }
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('0'))
                    {
                        app.reset_popup_position();
                        continue;
                    }
                }

                if matches!(app.ai_state, AiState::Completed(_) | AiState::Error(_)) {
                    if key.code == KeyCode::Esc {
                        app.ai_state = AiState::Idle;
                        continue;
                    }
                }

                if matches!(
                    app.export_state,
                    ExportState::Success(_) | ExportState::Error(_)
                ) {
                    app.export_state = ExportState::Idle;
                    continue;
                }

                if matches!(app.export_state, ExportState::Confirm(_)) {
                    match key.code {
                        KeyCode::Enter => app.confirm_export(),
                        KeyCode::Esc => app.cancel_export(),
                        _ => {}
                    }
                    continue;
                }

                if app.input_mode == InputMode::ReportSaveInput {
                    match key.code {
                        KeyCode::Esc => {
                            app.input_buffer.clear();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Enter => {
                            let filename = app.input_buffer.clone();
                            if !filename.is_empty() {
                                match std::fs::write(&filename, &app.report_content) {
                                    Ok(_) => app.status_msg = Some((format!("报告已保存到 {}", filename), Instant::now())),
                                    Err(e) => app.status_msg = Some((format!("保存失败: {}", e), Instant::now())),
                                }
                            }
                            app.input_buffer.clear();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Backspace => {
                            app.input_buffer.pop();
                        }
                        KeyCode::Char(c) => app.input_buffer.push(c),
                        _ => {}
                    }
                    continue;
                }

                if app.input_mode == InputMode::JumpInput {
                    match key.code {
                        KeyCode::Esc => app.exit_jump_mode(),
                        KeyCode::Enter => {
                            let line = app.input_buffer.clone();
                            app.submit_jump();
                            app.history.add(crate::history::CommandType::Jump, line);
                        }
                        KeyCode::Backspace => {
                            app.input_buffer.pop();
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => app.input_buffer.push(c),
                        _ => {}
                    }
                    continue;
                }

                if app.input_mode == InputMode::AiPromptInput {
                    match key.code {
                        KeyCode::Esc => app.exit_ai_prompt_mode(),
                        KeyCode::Enter => {
                            let custom_instruction = if app.input_buffer.trim().is_empty() {
                                None
                            } else {
                                Some(app.input_buffer.clone())
                            };
                            if let Some(idx) = app.list_state.selected() {
                                let start = idx.saturating_sub(10);
                                let end = (idx + 11).min(app.filtered_entries.len());
                                let context: String = app.filtered_entries[start..end]
                                    .iter()
                                    .map(|e| e.get_content())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                // Use try_send to avoid blocking the UI thread
                                match app.ai_tx.try_send((context, custom_instruction.clone())) {
                                    Ok(()) => {
                                        app.ai_state = AiState::Loading;
                                    }
                                    Err(_) => {
                                        app.status_msg = Some(("AI 正忙，请稍后重试".into(), Instant::now()));
                                    }
                                }
                            }
                            let prompt_text = custom_instruction.unwrap_or_else(|| "(默认分析)".to_string());
                            app.history.add(crate::history::CommandType::AiPrompt, prompt_text);
                            app.exit_ai_prompt_mode();
                        }
                        KeyCode::Backspace => {
                            app.input_buffer.pop();
                        }
                        KeyCode::Char(c) => app.input_buffer.push(c),
                        _ => {}
                    }
                    continue;
                }

                if app.input_mode == InputMode::ChatInput {
                    match key.code {
                        KeyCode::Esc => app.input_mode = InputMode::Normal,
                        KeyCode::Enter => app.submit_chat(),
                        KeyCode::Backspace => {
                            app.chat_input.pop();
                        }
                        KeyCode::Char(c) => app.chat_input.push(c),
                        _ => {}
                    }
                    continue;
                }

                if app.input_mode == InputMode::FocusCopyInput {
                    match key.code {
                        KeyCode::Esc => {
                            if app.current_view == CurrentView::Thread {
                                app.thread_view.copy_input.clear();
                            } else {
                                app.focus_mode.copy_input.clear();
                            }
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Enter => {
                            let (input, entries): (String, &[DisplayEntry]) = if app.current_view == CurrentView::Thread {
                                (app.thread_view.copy_input.clone(), &app.thread_view.thread_logs)
                            } else {
                                (app.focus_mode.copy_input.clone(), &app.focus_mode.focus_logs)
                            };

                            let indices = parse_copy_indices(&input, entries.len());
                            let text = indices
                                .as_ref()
                                .map(|v| build_copy_text(entries, v))
                                .unwrap_or_default();
                            if !text.is_empty() {
                                if let Some(ref mut clipboard) = app.clipboard {
                                    if clipboard.set_text(text).is_ok() {
                                        app.status_msg = Some((
                                            format!("已复制 {} 行", indices.as_ref().map_or(0, |v| v.len())),
                                            Instant::now(),
                                        ));
                                    }
                                }
                            } else {
                                app.status_msg = Some(("无效的行号".into(), Instant::now()));
                            }
                            if app.current_view == CurrentView::Thread {
                                app.thread_view.copy_input.clear();
                            } else {
                                app.focus_mode.copy_input.clear();
                            }
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Backspace => {
                            if app.current_view == CurrentView::Thread {
                                app.thread_view.copy_input.pop();
                            } else {
                                app.focus_mode.copy_input.pop();
                            }
                        }
                        KeyCode::Char(c)
                            if c.is_ascii_digit()
                                || c == '-'
                                || c == ','
                                || c == '*'
                                || c == 'a'
                                || c == 'A'
                                || c == 'l'
                                || c == 'L' =>
                        {
                            if app.current_view == CurrentView::Thread {
                                app.thread_view.copy_input.push(c);
                            } else {
                                app.focus_mode.copy_input.push(c);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Advanced search result popup handling
                if app.adv_result_popup.is_open {
                    if app.adv_result_popup.copy_mode {
                        match key.code {
                            KeyCode::Esc => {
                                app.adv_result_popup.copy_mode = false;
                                app.adv_result_popup.copy_input.clear();
                                app.adv_result_popup.copy_feedback = None;
                            }
                            KeyCode::Enter => {
                                let total = app.adv_result_popup.logs.len();
                                let input = app.adv_result_popup.copy_input.clone();
                                if let Some(indices) = parse_copy_indices(&input, total) {
                                    let text: String = indices.iter()
                                        .filter_map(|&i| app.adv_result_popup.logs.get(i - 1))
                                        .map(|e| e.get_content())
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    if let Some(ref mut clip) = app.clipboard {
                                        match clip.set_text(&text) {
                                            Ok(_) => {
                                                let msg = format!("已复制 {} 行到剪贴板", indices.len());
                                                app.status_msg = Some((msg.clone(), Instant::now()));
                                                app.adv_result_popup.copy_feedback = Some(format!(
                                                    "{}，可继续输入并回车再次复制",
                                                    msg
                                                ));
                                            }
                                            Err(e) => app.status_msg = Some((
                                                format!("复制失败: {}", e),
                                                Instant::now(),
                                            )),
                                        }
                                    } else {
                                        app.status_msg = Some((
                                            "复制失败: 系统剪贴板不可用".into(),
                                            Instant::now(),
                                        ));
                                        app.adv_result_popup.copy_feedback =
                                            Some("复制失败: 系统剪贴板不可用".into());
                                    }
                                    app.adv_result_popup.copy_input.clear();
                                } else {
                                    app.status_msg = Some(("无效的行号输入".into(), Instant::now()));
                                    app.adv_result_popup.copy_feedback =
                                        Some("输入无效，请使用 1-5, 3, 7-10, * / a / all".into());
                                }
                            }
                            KeyCode::Backspace => {
                                app.adv_result_popup.copy_input.pop();
                                app.adv_result_popup.copy_feedback = None;
                            }
                            KeyCode::Char(c)
                                if c.is_ascii_digit() || c == '-' || c == ','
                                || c == '*' || c == 'a' || c == 'A'
                                || c == 'l' || c == 'L' =>
                            {
                                app.adv_result_popup.copy_input.push(c);
                                app.adv_result_popup.copy_feedback = None;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if app.adv_result_popup.search_mode {
                        match key.code {
                            KeyCode::Esc => {
                                app.adv_result_popup.search_mode = false;
                                app.adv_result_popup.search_query.clear();
                                app.adv_result_popup.search_regex = None;
                            }
                            KeyCode::Enter => {
                                let q = app.adv_result_popup.search_query.clone();
                                if q.is_empty() {
                                    app.adv_result_popup.search_regex = None;
                                    app.adv_result_popup.search_mode = false;
                                    app.adv_result_popup.match_indices.clear();
                                } else {
                                    match Regex::new(&q) {
                                        Ok(re) => {
                                            app.adv_result_popup.search_regex = Some(re);
                                            app.adv_result_popup.search_mode = false;
                                            app.adv_result_popup.update_match_indices();
                                        }
                                        Err(e) => {
                                            app.status_msg = Some((
                                                format!("搜索正则无效: {}", e),
                                                Instant::now(),
                                            ));
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace => { app.adv_result_popup.search_query.pop(); }
                            KeyCode::Char(c) => { app.adv_result_popup.search_query.push(c); }
                            _ => {}
                        }
                        continue;
                    }
                    match key.code {
                        KeyCode::Esc => app.adv_result_popup.close(),
                        KeyCode::Up | KeyCode::Char('k') => app.adv_result_popup.previous(),
                        KeyCode::Down | KeyCode::Char('j') => app.adv_result_popup.next(),
                        KeyCode::Left => app.adv_result_popup.previous_page(app.page_size),
                        KeyCode::Right => app.adv_result_popup.next_page(app.page_size),
                        KeyCode::Char('g') => app.adv_result_popup.jump_to_top(),
                        KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            app.adv_result_popup.jump_to_bottom()
                        }
                        KeyCode::Char('/') => {
                            app.adv_result_popup.search_mode = true;
                            app.adv_result_popup.search_query.clear();
                        }
                        KeyCode::Char('n') => app.adv_result_popup.next_match(),
                        KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            app.adv_result_popup.prev_match()
                        }
                        KeyCode::Char('c') => {
                            app.adv_result_popup.copy_mode = true;
                            app.adv_result_popup.copy_input.clear();
                            app.adv_result_popup.copy_feedback =
                                Some("输入行号后按 Enter 复制，Esc 返回".into());
                        }
                        KeyCode::Char('e') => {
                            let filename = format!(
                                "adv_search_{}.log",
                                chrono::Local::now().format("%Y%m%d_%H%M%S")
                            );
                            let content: String = app.adv_result_popup.logs
                                .iter()
                                .map(|e| e.get_content())
                                .collect::<Vec<_>>()
                                .join("\n");
                            match std::fs::write(&filename, content) {
                                Ok(_) => app.status_msg = Some((
                                    format!("已导出到 {}", filename),
                                    Instant::now(),
                                )),
                                Err(e) => app.status_msg = Some((
                                    format!("导出失败: {}", e),
                                    Instant::now(),
                                )),
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => app.clear_search(),
                        KeyCode::Enter => {
                            let query = app.search_query.clone();
                            if app.current_view == CurrentView::Focus {
                                // In focus mode: filter focus_logs
                                app.focus_update_search();
                                app.exit_search();
                            } else if app.current_view == CurrentView::Thread {
                                // In thread view: filter thread_logs
                                app.thread_update_search();
                                app.exit_search();
                            } else if key.modifiers.contains(KeyModifiers::ALT) {
                                // Alt+Enter: Enter focus mode with current search
                                app.update_search();
                                app.clear_search();
                                app.enter_focus_mode(query.clone());
                            } else {
                                // Normal Enter: Apply search (show highlights) and hide input overlay
                                app.update_search();
                                app.exit_search();
                            }
                            app.history.add(crate::history::CommandType::Search, query);
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                if app.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter => app.show_help = false,
                        _ => {}
                    }
                    continue;
                }

                // Advanced search form modal handling
                if app.search_form.is_open {
                    // Handle template mode dialogs first
                    match app.search_form.template_mode {
                        TemplateMode::Saving => {
                            match key.code {
                                KeyCode::Esc => {
                                    app.search_form.exit_template_mode();
                                }
                                KeyCode::Enter => {
                                    let name = app.search_form.template_name_input.trim();
                                    if name.is_empty() {
                                        app.search_form.set_error("模板名称不能为空".to_string());
                                    } else {
                                        let criteria = app.search_form.to_serializable_criteria();
                                        match save_template(name, &criteria) {
                                            Ok(()) => {
                                                app.search_form.set_status(format!("模板 '{}' 保存成功", name));
                                                app.search_form.exit_template_mode();
                                            }
                                            Err(e) => {
                                                app.search_form.set_error(e);
                                            }
                                        }
                                    }
                                }
                                KeyCode::Backspace => {
                                    app.search_form.template_name_input.pop();
                                }
                                KeyCode::Char(c) => {
                                    app.search_form.template_name_input.push(c);
                                }
                                _ => {}
                            }
                            continue;
                        }
                        TemplateMode::Loading => {
                            match key.code {
                                KeyCode::Esc => {
                                    app.search_form.exit_template_mode();
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    app.search_form.prev_template();
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    app.search_form.next_template();
                                }
                                KeyCode::Enter => {
                                    if let Some(name) = app.search_form.selected_template_name().cloned() {
                                        if let Some(template) = get_template(&name) {
                                            app.search_form.load_from_criteria(&template.criteria);
                                            app.search_form.set_status(format!("已加载模板 '{}'", name));
                                            app.search_form.exit_template_mode();
                                        }
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }
                        TemplateMode::None => {}
                    }

                    match key.code {
                        KeyCode::Esc => {
                            app.search_form.close();
                        }
                        KeyCode::Up => {
                            app.search_form.prev_field();
                        }
                        KeyCode::Down => {
                            app.search_form.next_field();
                        }
                        KeyCode::Tab => {
                            if key.modifiers.contains(KeyModifiers::SHIFT) {
                                app.search_form.prev_field();
                            } else {
                                app.search_form.next_field();
                            }
                        }
                        KeyCode::BackTab => {
                            app.search_form.prev_field();
                        }
                        KeyCode::Enter => {
                            let focused_on_content = app.search_form.focused_field == FormField::Content;
                            let simple_content_only = focused_on_content
                                && !app.search_form.content_input.trim().is_empty()
                                && app.search_form.start_time_input.trim().is_empty()
                                && app.search_form.end_time_input.trim().is_empty()
                                && app.search_form.source_input.trim().is_empty()
                                && app.search_form.selected_levels.is_empty();
                            let submit_now = app.search_form.focused_field == FormField::SubmitBtn
                                || key.modifiers.contains(KeyModifiers::CONTROL)
                                || simple_content_only;
                            if submit_now {
                                let criteria = match build_advanced_search_criteria(&app.search_form) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        app.search_form.set_error(e);
                                        continue;
                                    }
                                };
                                let count = apply_advanced_search(app, &criteria);
                                app.search_form.close();
                                app.status_msg = Some((
                                    format!("高级搜索: {} 条匹配", count),
                                    std::time::Instant::now(),
                                ));
                            } else {
                                // Move to next field on Enter in input fields
                                app.search_form.next_field();
                            }
                        }
                        KeyCode::Backspace => {
                            if let Some(input) = app.search_form.current_input_mut() {
                                input.pop();
                            }
                        }
                        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+S: Save template
                            app.search_form.start_save_template();
                        }
                        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+L: Load template
                            let names = get_template_names();
                            app.search_form.start_load_template(names);
                        }
                        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+R: Clear form quickly
                            app.search_form.clear();
                        }
                        KeyCode::Char(c) => {
                            match app.search_form.focused_field {
                                FormField::LevelSelect => {
                                    // Toggle levels with 1-4
                                    match c {
                                        '1' => app.search_form.toggle_level(LogLevel::Debug),
                                        '2' => app.search_form.toggle_level(LogLevel::Info),
                                        '3' => app.search_form.toggle_level(LogLevel::Warn),
                                        '4' => app.search_form.toggle_level(LogLevel::Error),
                                        _ => {}
                                    }
                                }
                                FormField::SubmitBtn => {
                                    // No char input on submit button
                                }
                                _ => {
                                    if let Some(input) = app.search_form.current_input_mut() {
                                        input.push(c);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }


                {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::F(1) => app.current_view = CurrentView::Logs,
                        KeyCode::F(2) => app.current_view = CurrentView::Dashboard,
                        KeyCode::F(3) => app.current_view = CurrentView::Chat,
                        KeyCode::F(4) => app.current_view = CurrentView::History,
                        KeyCode::F(5) => app.current_view = CurrentView::Report,
                        KeyCode::F(6) => {
                            let query = app.search_regex.as_ref()
                                .map(|r| r.as_str().to_string())
                                .unwrap_or_else(|| app.search_query.clone());
                            app.enter_focus_mode(if query.is_empty() { "全部".to_string() } else { query });
                        }
                        KeyCode::Tab => {
                            app.focus = if app.focus == Focus::LogList {
                                Focus::FileList
                            } else {
                                Focus::LogList
                            }
                        }
                        KeyCode::Char('?') => app.show_help = true,
                        _ => {}
                    }
                    if app.current_view == CurrentView::Dashboard {
                        match key.code {
                            KeyCode::Left => app.scroll_chart_left(app.stats.error_trend.len(), 10),
                            KeyCode::Right => app.scroll_chart_right(),
                            _ => {}
                        }
                        continue;
                    }
                    if app.current_view == CurrentView::History {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => app.history.previous(),
                            KeyCode::Down | KeyCode::Char('j') => app.history.next(),
                            KeyCode::Enter => {
                                if let Some(entry) = app.history.selected_entry().cloned() {
                                    app.execute_history_entry(&entry);
                                }
                            }
                            KeyCode::Delete | KeyCode::Char('d') => {
                                let idx = app.history.selected;
                                app.history.delete(idx);
                            }
                            KeyCode::Char('c') => app.history.clear(),
                            KeyCode::Esc => app.current_view = CurrentView::Logs,
                            _ => {}
                        }
                        continue;
                    }
                    if app.current_view == CurrentView::Report {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.report_period = app.report_period.prev();
                                app.report_content = app.report_cache.get(app.report_period).cloned().unwrap_or_default();
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.report_period = app.report_period.next();
                                app.report_content = app.report_cache.get(app.report_period).cloned().unwrap_or_default();
                            }
                            KeyCode::Enter => {
                                if !app.report_generating {
                                    // Generate report context and send to AI
                                    let logs: Vec<_> = app.all_entries.iter().filter_map(|e| {
                                        if let crate::models::DisplayEntry::Normal(log) = e {
                                            Some(log.clone())
                                        } else {
                                            None
                                        }
                                    }).collect();
                                    let context = crate::report::generate_report_context(&logs, app.report_period);
                                    if let Ok(json) = serde_json::to_string_pretty(&context) {
                                        // Use try_send to avoid blocking the UI thread
                                        match app.report_tx.try_send(json) {
                                            Ok(()) => {
                                                app.report_generating = true;
                                            }
                                            Err(_) => {
                                                app.status_msg = Some(("报告生成器正忙，请稍后".into(), Instant::now()));
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                if !app.report_content.is_empty() {
                                    app.input_buffer = format!("report_{}.md", chrono::Local::now().format("%Y%m%d_%H%M%S"));
                                    app.input_mode = InputMode::ReportSaveInput;
                                }
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                if !app.report_content.is_empty() {
                                    if let Some(ref mut clipboard) = app.clipboard {
                                        let _ = clipboard.set_text(&app.report_content);
                                        app.status_msg = Some(("报告已复制到剪贴板".into(), Instant::now()));
                                    }
                                }
                            }
                            KeyCode::Esc => app.current_view = CurrentView::Logs,
                            _ => {}
                        }
                        continue;
                    }
                    // Focus Mode handling
                    if app.current_view == CurrentView::Focus {
                        match key.code {
                            KeyCode::Esc => {
                                if !app.focus_go_back() {
                                    app.exit_focus_mode();
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => app.focus_previous(),
                            KeyCode::Down | KeyCode::Char('j') => app.focus_next(),
                            KeyCode::Left => app.focus_previous_page(),
                            KeyCode::Right => app.focus_next_page(),
                            KeyCode::Char('g') => app.focus_jump_to_top(),
                            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.focus_jump_to_bottom()
                            }
                            KeyCode::Char('c') => {
                                app.focus_mode.copy_input.clear();
                                app.input_mode = InputMode::FocusCopyInput;
                            }
                            KeyCode::Char('e') => {
                                // Export focus mode entries to file
                                let filename = format!("focus_{}.log", chrono::Local::now().format("%Y%m%d_%H%M%S"));
                                let content: String = app.focus_mode.focus_logs
                                    .iter()
                                    .map(|e| e.get_content())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                match std::fs::write(&filename, content) {
                                    Ok(_) => app.status_msg = Some((format!("已导出到 {}", filename), Instant::now())),
                                    Err(e) => app.status_msg = Some((format!("导出失败: {}", e), Instant::now())),
                                }
                            }
                            KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                // Open advanced search form in focus mode
                                app.search_form.open();
                            }
                            KeyCode::Char('/') => {
                                // Quick search in focus mode
                                app.start_search();
                            }
                            _ => {}
                        }
                        continue;
                    }
                    // Thread View handling
                    if app.current_view == CurrentView::Thread {
                        match key.code {
                            KeyCode::Esc => app.exit_thread_view(),
                            KeyCode::Up | KeyCode::Char('k') => app.thread_previous(),
                            KeyCode::Down | KeyCode::Char('j') => app.thread_next(),
                            KeyCode::Left => app.thread_previous_page(),
                            KeyCode::Right => app.thread_next_page(),
                            KeyCode::Char('g') => app.thread_jump_to_top(),
                            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.thread_jump_to_bottom()
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => app.thread_zoom_in(),
                            KeyCode::Char('-') => app.thread_zoom_out(),
                            KeyCode::Char('c') => {
                                app.thread_view.copy_input.clear();
                                app.input_mode = InputMode::FocusCopyInput;
                            }
                            KeyCode::Char('e') => {
                                // Export thread view entries to file
                                let filename = format!("thread_{}_{}.log",
                                    app.thread_view.thread_id,
                                    chrono::Local::now().format("%Y%m%d_%H%M%S"));
                                let content: String = app.thread_view.thread_logs
                                    .iter()
                                    .map(|e| e.get_content())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                match std::fs::write(&filename, content) {
                                    Ok(_) => app.status_msg = Some((format!("已导出到 {}", filename), Instant::now())),
                                    Err(e) => app.status_msg = Some((format!("导出失败: {}", e), Instant::now())),
                                }
                            }
                            KeyCode::Char('/') => {
                                // Quick search in thread view
                                app.start_search();
                            }
                            KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                // Open advanced search form in thread view
                                app.search_form.open();
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if app.current_view == CurrentView::Chat {
                        match key.code {
                            KeyCode::Char('i') => app.input_mode = InputMode::ChatInput,
                            KeyCode::Char('c') => app.clear_chat_context(),
                            KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.clear_chat_history()
                            }
                            KeyCode::Up | KeyCode::Char('k') => app.chat_scroll_up(),
                            KeyCode::Down | KeyCode::Char('j') => app.chat_scroll_down(),
                            KeyCode::Char('g') => {
                                app.chat_scroll = 999;
                            } // scroll to top
                            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.chat_scroll_to_bottom()
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match app.focus {
                        Focus::FileList => match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                let len = app.files.len();
                                if len > 0 {
                                    let i = app
                                        .file_list_state
                                        .selected()
                                        .map(|i| i.saturating_sub(1))
                                        .unwrap_or(0);
                                    app.file_list_state.select(Some(i));
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let len = app.files.len();
                                if len > 0 {
                                    let i = app
                                        .file_list_state
                                        .selected()
                                        .map(|i| (i + 1).min(len - 1))
                                        .unwrap_or(0);
                                    app.file_list_state.select(Some(i));
                                }
                            }
                            KeyCode::Char(' ') => app.toggle_file(),
                            KeyCode::Enter => app.solo_file(),
                            _ => {}
                        },
                        Focus::LogList => match key.code {
                            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.clear_advanced_search()
                            }
                            KeyCode::Up | KeyCode::Char('k') => app.previous(),
                            KeyCode::Down | KeyCode::Char('j') => app.next(),
                            KeyCode::Left => app.previous_page(),
                            KeyCode::Right => app.next_page(),
                            KeyCode::Char('g') => app.jump_to_top(),
                            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.jump_to_bottom()
                            }
                            KeyCode::Char(':') => app.enter_jump_mode(),
                            KeyCode::Char('/') => app.start_search(),
                            KeyCode::Enter => {
                                if key.modifiers.contains(KeyModifiers::ALT) {
                                    // Alt+Enter: Enter focus mode with current search results
                                    let query = app.search_regex.as_ref()
                                        .map(|r| r.as_str().to_string())
                                        .unwrap_or_else(|| app.search_query.clone());
                                    app.enter_focus_mode(if query.is_empty() { "全部".to_string() } else { query });
                                }
                            }
                            KeyCode::Char('n') => app.next_match(),
                            KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.prev_match()
                            }
                            KeyCode::Char('t') => app.toggle_thread_filter(),
                            KeyCode::Char('T') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.toggle_trace_filter()
                            }
                            KeyCode::Esc => {
                                // Clear search highlights if active
                                if app.search_regex.is_some() {
                                    app.clear_search();
                                } else if app.filter_tid.is_some() || app.filter_trace.is_some() {
                                    // Only clear filters if they exist
                                    app.filter_tid = None;
                                    app.filter_trace = None;
                                    app.apply_filter();
                                }
                            }
                            KeyCode::Char('c') => app.copy_line(),
                            KeyCode::Char('y') => app.yank_payload(),
                            KeyCode::Char('m') => app.toggle_bookmark(),
                            KeyCode::Char('b') => app.next_bookmark(),
                            KeyCode::Char('B') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.prev_bookmark()
                            }
                            KeyCode::Char('1') => app.toggle_level(1),
                            KeyCode::Char('2') => app.toggle_level(2),
                            KeyCode::Char('3') => app.toggle_level(3),
                            KeyCode::Char('4') => app.toggle_level(4),
                            KeyCode::Char('a') => {
                                if matches!(app.ai_state, AiState::Idle) {
                                    app.enter_ai_prompt_mode();
                                }
                            }
                            KeyCode::Char('p') => app.pin_selected_log(),
                            KeyCode::Char('f') => app.is_tailing = !app.is_tailing,
                            KeyCode::Char('e') => app.request_export(ExportType::LogsCsv),
                            KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.request_export(ExportType::LogsJson)
                            }
                            KeyCode::Char('r') => app.request_export(ExportType::Report),
                            KeyCode::Char('R') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.request_export(ExportType::AiAnalysis)
                            }
                            KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.search_form.open();
                            }
                            // Horizontal scroll and wrap controls
                            KeyCode::Char('h') => {
                                if !app.wrap_lines {
                                    app.scroll_horizontal_left(5);
                                }
                            }
                            KeyCode::Char('l') => {
                                if !app.wrap_lines {
                                    app.scroll_horizontal_right(5);
                                }
                            }
                            KeyCode::Char('H') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.reset_horizontal_scroll();
                            }
                            KeyCode::Char('w') => app.toggle_wrap_lines(),
                            _ => {}
                        },
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_advanced_search_criteria, parse_copy_indices};
    use crate::search_form::SearchFormState;

    #[test]
    fn parse_copy_indices_supports_all_shortcuts() {
        assert_eq!(parse_copy_indices("*", 3), Some(vec![1, 2, 3]));
        assert_eq!(parse_copy_indices("a", 3), Some(vec![1, 2, 3]));
        assert_eq!(parse_copy_indices("all", 3), Some(vec![1, 2, 3]));
    }

    #[test]
    fn parse_copy_indices_supports_mixed_ranges() {
        assert_eq!(
            parse_copy_indices("1-3,5,7-6,2", 10),
            Some(vec![1, 2, 3, 5, 6, 7])
        );
    }

    #[test]
    fn parse_copy_indices_rejects_invalid_input() {
        assert_eq!(parse_copy_indices("", 10), None);
        assert_eq!(parse_copy_indices("x,y", 10), None);
        assert_eq!(parse_copy_indices("99", 10), None);
    }

    #[test]
    fn advanced_criteria_rejects_invalid_regex() {
        let mut form = SearchFormState::new();
        form.content_input = "[".to_string();
        let result = build_advanced_search_criteria(&form);
        assert!(result.is_err());
    }

    #[test]
    fn advanced_criteria_rejects_invalid_time_range() {
        let mut form = SearchFormState::new();
        form.start_time_input = "2026-03-30 12:00:00".to_string();
        form.end_time_input = "2026-03-30 10:00:00".to_string();
        let result = build_advanced_search_criteria(&form);
        assert!(result.is_err());
    }

    #[test]
    fn advanced_criteria_accepts_slash_prefixed_content() {
        let mut form = SearchFormState::new();
        form.content_input = "/error".to_string();
        let result = build_advanced_search_criteria(&form).unwrap();
        assert_eq!(result.content_regex.as_deref(), Some("error"));
    }

    #[test]
    fn advanced_criteria_rejects_empty_slash_content() {
        let mut form = SearchFormState::new();
        form.content_input = "/".to_string();
        let result = build_advanced_search_criteria(&form);
        assert!(result.is_err());
    }
}
