use std::io::Stdout;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::ListState;
use regex::Regex;

use crate::app_state::App;
use crate::live::TailState;
use crate::models::{AiState, CurrentView, DisplayEntry, Focus, InputMode};
use super::components::{render_ai_popup, render_ai_prompt_popup, render_detail_pane, render_help_popup, render_histogram, render_jump_popup, render_log_list, render_search_bar, render_sidebar};
use super::dashboard::{render_dashboard, render_header};
use super::layout::create_layout;

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
            if app.search_mode { render_search_bar(frame, app, layout.search_bar); }
            render_detail_pane(frame, app, layout.detail);
            render_histogram(frame, app, layout.histogram);
        }
        CurrentView::Dashboard => {
            render_dashboard(frame, app, main_chunks[1]);
        }
    }
    render_ai_popup(frame, app);
    if app.show_help { render_help_popup(frame); }
    render_jump_popup(frame, app);
    render_ai_prompt_popup(frame, app);
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

        if app.is_tailing {
            while let Ok(paths) = file_rx.try_recv() {
                for changed_path in paths {
                    if let Some((source_id, path)) = file_paths.iter().enumerate()
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
                if key.kind != KeyEventKind::Press { continue; }

                if matches!(app.ai_state, AiState::Completed(_) | AiState::Error(_)) {
                    if key.code == KeyCode::Esc { app.ai_state = AiState::Idle; continue; }
                }

                if app.input_mode == InputMode::JumpInput {
                    match key.code {
                        KeyCode::Esc => app.exit_jump_mode(),
                        KeyCode::Enter => app.submit_jump(),
                        KeyCode::Backspace => { app.input_buffer.pop(); }
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
                                let context: String = app.filtered_entries[start..end].iter()
                                    .map(|e| e.get_content()).collect::<Vec<_>>().join("\n");
                                if app.ai_tx.blocking_send((context, custom_instruction)).is_ok() {
                                    app.ai_state = AiState::Loading;
                                }
                            }
                            app.exit_ai_prompt_mode();
                        }
                        KeyCode::Backspace => { app.input_buffer.pop(); }
                        KeyCode::Char(c) => app.input_buffer.push(c),
                        _ => {}
                    }
                    continue;
                }
                
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => app.exit_search(),
                        KeyCode::Enter => { app.update_search(); app.exit_search(); }
                        KeyCode::Backspace => { app.search_query.pop(); }
                        KeyCode::Char(c) => { app.search_query.push(c); }
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
                
                {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::F(1) => app.current_view = CurrentView::Logs,
                        KeyCode::F(2) => app.current_view = CurrentView::Dashboard,
                        KeyCode::Tab => app.focus = if app.focus == Focus::LogList { Focus::FileList } else { Focus::LogList },
                        KeyCode::Char('?') => app.show_help = true,
                        _ => {}
                    }
                    if app.current_view == CurrentView::Dashboard { continue; }
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
                            KeyCode::Left => app.previous_page(),
                            KeyCode::Right => app.next_page(),
                            KeyCode::Char('g') => app.jump_to_top(),
                            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => app.jump_to_bottom(),
                            KeyCode::Char(':') => app.enter_jump_mode(),
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
                                    app.enter_ai_prompt_mode();
                                }
                            }
                            KeyCode::Char('f') => app.is_tailing = !app.is_tailing,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

