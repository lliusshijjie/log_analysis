use std::io::Stdout;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use regex::Regex;

use super::chat::render_chat_interface;
use super::components::{
    render_ai_popup, render_ai_prompt_popup, render_detail_pane, render_export_popup,
    render_help_popup, render_histogram, render_jump_popup, render_log_list, render_search_bar,
    render_sidebar,
};
use super::dashboard::{render_dashboard, render_header};
use super::layout::create_layout;
use super::search_modal::render_search_modal;
use crate::app_state::App;
use crate::filtering::filter_logs_owned;
use crate::live::TailState;
use crate::models::{
    AiState, CurrentView, DisplayEntry, ExportResult, ExportState, ExportType, Focus, InputMode,
};
use crate::search::{LogLevel, SearchCriteria};
use crate::search_form::{FormField, TemplateMode};
use crate::templates::{get_template, get_template_names, save_template};
use crate::time_parser::parse_user_time;


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
            render_log_list(frame, app, layout.log_list);
            if app.search_mode {
                render_search_bar(frame, app, layout.search_bar);
            }
            render_detail_pane(frame, app, layout.detail);
            render_histogram(frame, app, layout.histogram);
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

        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
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

                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => app.exit_search(),
                        KeyCode::Enter => {
                            let query = app.search_query.clone();
                            app.update_search();
                            app.exit_search();
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
                            if app.search_form.focused_field == FormField::SubmitBtn {
                                // Build SearchCriteria from form
                                let form = &app.search_form;
                                let mut criteria = SearchCriteria::default();
                                
                                // Parse start time
                                if !form.start_time_input.is_empty() {
                                    match parse_user_time(&form.start_time_input) {
                                        Some(t) => criteria.start_time = Some(t),
                                        None => {
                                            app.search_form.set_error(
                                                format!("无效的开始时间: {}", form.start_time_input)
                                            );
                                            continue;
                                        }
                                    }
                                }
                                
                                // Parse end time
                                if !form.end_time_input.is_empty() {
                                    match parse_user_time(&form.end_time_input) {
                                        Some(t) => criteria.end_time = Some(t),
                                        None => {
                                            app.search_form.set_error(
                                                format!("无效的结束时间: {}", form.end_time_input)
                                            );
                                            continue;
                                        }
                                    }
                                }
                                
                                // Content regex
                                if !form.content_input.is_empty() {
                                    criteria.content_regex = Some(form.content_input.clone());
                                }
                                
                                // Source file
                                if !form.source_input.is_empty() {
                                    criteria.source_file = Some(form.source_input.clone());
                                }
                                
                                // Levels
                                criteria.levels = form.selected_levels.iter().cloned().collect();
                                
                                // Apply filter
                                app.filtered_entries = filter_logs_owned(&app.all_entries, &criteria);
                                app.list_state.select(if app.filtered_entries.is_empty() {
                                    None
                                } else {
                                    Some(0)
                                });
                                app.update_search_matches();
                                
                                // Close form and show status
                                app.search_form.close();
                                let count = app.filtered_entries.len();
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
                            KeyCode::Char('n') => app.next_match(),
                            KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.prev_match()
                            }
                            KeyCode::Char('t') => app.toggle_thread_filter(),
                            KeyCode::Esc => {
                                app.filter_tid = None;
                                app.apply_filter();
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
                            _ => {}
                        },
                    }
                }
            }
        }
    }
}
