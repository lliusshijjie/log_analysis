use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;

use crate::app_state::App;
use crate::models::{AiState, Focus};
use super::components::{render_ai_popup, render_detail_pane, render_help_popup, render_histogram, render_log_list, render_search_bar, render_sidebar};
use super::layout::create_layout;

fn ui(frame: &mut Frame, app: &mut App) {
    let layout = create_layout(frame.area(), app.search_mode);
    render_sidebar(frame, app, layout.sidebar);
    render_log_list(frame, app, layout.log_list);
    if app.search_mode { render_search_bar(frame, app, layout.search_bar); }
    render_detail_pane(frame, app, layout.detail);
    render_histogram(frame, app, layout.histogram);
    render_ai_popup(frame, app);
    if app.show_help { render_help_popup(frame); }
}

pub fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        if let Ok(result) = app.ai_rx.try_recv() {
            app.ai_state = match result {
                Ok(s) => AiState::Completed(s),
                Err(e) => AiState::Error(e),
            };
        }

        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(16))? {
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
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Tab => app.focus = if app.focus == Focus::LogList { Focus::FileList } else { Focus::LogList },
                        KeyCode::Char('?') => app.show_help = true,
                        _ => {}
                    }
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
}

