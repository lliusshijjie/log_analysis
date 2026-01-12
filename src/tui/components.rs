use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use regex::Regex;
use serde_json::Value;

use crate::app_state::App;
use crate::models::{AiState, DisplayEntry, ExportState, ExportType, Focus, InputMode};
use crate::tui::layout::centered_rect;
use crate::tui::syntax::highlight_content_default;

fn level_color(level: &str) -> Color {
    match level {
        "Error" => Color::Red,
        "Warning" | "Warn" => Color::Yellow,
        "Debug" => Color::Cyan,
        _ => Color::White,
    }
}

fn delta_span(delta_ms: Option<i64>) -> Option<Span<'static>> {
    let d = delta_ms?;
    if d >= 1000 {
        Some(Span::styled(
            format!("[SLOW {:.1}s]", d as f64 / 1000.0),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ))
    } else if d >= 100 {
        Some(Span::styled(
            format!("[+{}ms]", d),
            Style::default().fg(Color::Yellow),
        ))
    } else {
        None
    }
}

fn render_list_item(
    entry: &DisplayEntry,
    _regex: Option<&Regex>,
    is_match: bool,
    is_bookmarked: bool,
    file_color: Color,
) -> ListItem<'static> {
    let line_idx = entry
        .get_line_index()
        .map(|n| format!("{:>5} ", n))
        .unwrap_or_else(|| "      ".into());
    let bookmark = if is_bookmarked { "🔖" } else { " " };
    let marker = if is_match { "●" } else { " " };
    match entry {
        DisplayEntry::Normal(log) => {
            let preview: String = log.content.chars().take(100).collect();
            let mut spans: Vec<Span<'static>> = vec![
                Span::styled(line_idx, Style::default().fg(Color::DarkGray)),
                Span::styled("█ ", Style::default().fg(file_color)),
                Span::styled(bookmark.to_string(), Style::default().fg(Color::Magenta)),
                Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
                Span::styled(
                    log.timestamp[11..19].to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
            ];
            if let Some(ds) = delta_span(log.delta_ms) {
                spans.push(ds);
                spans.push(Span::raw(" "));
            }
            spans.extend(vec![
                Span::styled(
                    format!("[{:5}]", &log.level),
                    Style::default().fg(level_color(&log.level)),
                ),
                Span::raw(" "),
            ]);
            let highlighted = highlight_content_default(&preview);
            for span in highlighted.spans {
                let owned_span = Span::styled(span.content.to_string(), span.style);
                spans.push(owned_span);
            }
            let style = if is_bookmarked {
                Style::default().bg(Color::Rgb(40, 40, 60))
            } else {
                Style::default()
            };
            ListItem::new(Line::from(spans)).style(style)
        }
        DisplayEntry::Folded {
            count,
            summary_text,
            ..
        } => ListItem::new(Line::from(vec![
            Span::styled(line_idx, Style::default().fg(Color::DarkGray)),
            Span::styled("█ ", Style::default().fg(file_color)),
            Span::styled(bookmark.to_string(), Style::default().fg(Color::Magenta)),
            Span::styled(marker.to_string(), Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("▶ [{} lines] ", count),
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
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
                        lines.push(Line::from(vec![
                            Span::raw(format!("{}  ", pre)),
                            Span::styled(format!("\"{}\"", k), Style::default().fg(Color::Cyan)),
                            Span::raw(": "),
                        ]));
                        let mut sub = format_json(v, indent + 2);
                        if let Some(l) = sub.last_mut() {
                            l.spans.push(Span::raw(comma));
                        }
                        lines.extend(sub);
                    }
                    _ => lines.push(Line::from(vec![
                        Span::raw(format!("{}  ", pre)),
                        Span::styled(format!("\"{}\"", k), Style::default().fg(Color::Cyan)),
                        Span::raw(": "),
                        Span::styled(
                            match v {
                                Value::String(s) => format!("\"{}\"", s),
                                _ => v.to_string(),
                            },
                            Style::default().fg(Color::Green),
                        ),
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
                if let Some(f) = sub.first_mut() {
                    f.spans.insert(0, Span::raw(format!("{}  ", pre)));
                }
                if let Some(l) = sub.last_mut() {
                    l.spans.push(Span::raw(comma));
                }
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
                Line::from(vec![
                    Span::styled("Time: ", Style::default().fg(Color::Yellow)),
                    Span::raw(log.timestamp.clone()),
                ]),
                Line::from(vec![
                    Span::styled("TID: ", Style::default().fg(Color::Yellow)),
                    Span::raw(format!("{}:{}", log.pid, log.tid)),
                ]),
                Line::from(vec![
                    Span::styled("Level: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        log.level.clone(),
                        Style::default().fg(level_color(&log.level)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Source: ", Style::default().fg(Color::Yellow)),
                    Span::raw(format!("{}:{}", log.source_file, log.line_num)),
                ]),
            ];
            if let Some(d) = log.delta_ms {
                lines.push(Line::from(vec![
                    Span::styled("Delta: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!("{}ms", d),
                        Style::default().fg(if d >= 1000 {
                            Color::Red
                        } else if d >= 100 {
                            Color::Yellow
                        } else {
                            Color::Green
                        }),
                    ),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Content: ",
                Style::default().fg(Color::Yellow),
            )]));
            lines.push(Line::from(log.content.clone()));
            if let Some(json) = &log.json_payload {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "JSON:",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                )]));
                lines.extend(format_json(json, 0));
            }
            Text::from(lines)
        }
        Some(DisplayEntry::Folded {
            count,
            summary_text,
            start_index,
            end_index,
        }) => Text::from(vec![
            Line::from(vec![Span::styled(
                "FOLDED",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(format!("Range: {}-{}", start_index, end_index)),
            Line::from(format!("Count: {}", count)),
            Line::from(format!("Reason: {}", summary_text)),
        ]),
        None => Text::from("No selection"),
    }
}

pub fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let file_items: Vec<ListItem> = app
        .files
        .iter()
        .map(|f| {
            let mark = if f.enabled { "[x]" } else { "[ ]" };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", mark), Style::default().fg(Color::White)),
                Span::styled(&f.name, Style::default().fg(f.color)),
            ]))
        })
        .collect();
    let sidebar_style = if app.focus == Focus::FileList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let file_list = List::new(file_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Files ")
                .border_style(sidebar_style),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(file_list, area, &mut app.file_list_state);
}

pub fn render_log_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let tail_indicator = if app.is_tailing { "[LIVE] " } else { "" };

    // Level filter status
    let level_status = format!(
        "[{}I {}W {}E {}D]",
        if app.visible_levels.info {
            "●"
        } else {
            "○"
        },
        if app.visible_levels.warn {
            "●"
        } else {
            "○"
        },
        if app.visible_levels.error {
            "●"
        } else {
            "○"
        },
        if app.visible_levels.debug {
            "●"
        } else {
            "○"
        },
    );

    let title = match (&app.filter_tid, &app.search_regex) {
        (Some(tid), Some(_)) => format!(
            " {}[FILTER: Thread {}] [SEARCH: {} matches] {} ",
            tail_indicator,
            tid,
            app.match_indices.len(),
            level_status
        ),
        (Some(tid), None) => format!(
            " {}[FILTER: Thread {}] {} ",
            tail_indicator, tid, level_status
        ),
        (None, Some(_)) => format!(
            " {}[SEARCH: {} matches] {} ",
            tail_indicator,
            app.match_indices.len(),
            level_status
        ),
        (None, None) => format!(
            " {}Logs ({}) {} ",
            tail_indicator,
            app.entries().len(),
            level_status
        ),
    };
    let title_style = if app.is_tailing {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if app.filter_tid.is_some() || app.search_regex.is_some() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let entries = app.entries().clone();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let file_color = e
                .get_source_id()
                .map(|sid| app.get_file_color(sid))
                .unwrap_or(Color::White);
            render_list_item(
                e,
                app.search_regex.as_ref(),
                app.match_indices.contains(&i),
                app.bookmarks.contains(&i),
                file_color,
            )
        })
        .collect();

    let help = if app.search_mode {
        "ESC=exit  !term=exclude"
    } else {
        "Tab=switch Space=toggle Enter=solo"
    };
    let list_style = if app.focus == Focus::LogList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(title_style)
                .title_bottom(Line::from(help).right_aligned())
                .border_style(list_style),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut app.list_state);

    // Custom scrollbar with error markers
    render_error_scrollbar(frame, app, area);
}

fn render_error_scrollbar(frame: &mut Frame, app: &App, area: Rect) {
    let total = app.filtered_entries.len();
    if total == 0 || area.height < 4 {
        return;
    }

    let track_height = area.height.saturating_sub(2) as usize;
    let scrollbar_x = area.x + area.width - 1;
    let track_start_y = area.y + 1;

    let visible_rows = track_height;
    let selected = app.list_state.selected().unwrap_or(0);

    // Calculate thumb position and size based on visible window
    let thumb_size = ((visible_rows * track_height) / total.max(1))
        .max(1)
        .min(track_height);
    let max_scroll = total.saturating_sub(visible_rows);
    let scroll_pos = selected.saturating_sub(visible_rows / 2).min(max_scroll);
    let thumb_pos = if max_scroll == 0 {
        0
    } else {
        (scroll_pos * (track_height - thumb_size)) / max_scroll
    };

    for y in 0..track_height {
        let line_start = (y * total) / track_height;
        let line_end = ((y + 1) * total) / track_height;

        let has_error = app
            .error_indices
            .iter()
            .any(|&i| i >= line_start && i < line_end);
        let is_thumb = y >= thumb_pos && y < thumb_pos + thumb_size;

        let (ch, style) = if is_thumb && has_error {
            ("█", Style::default().fg(Color::Red))
        } else if is_thumb {
            ("█", Style::default().fg(Color::Cyan))
        } else if has_error {
            ("█", Style::default().fg(Color::Red))
        } else {
            ("│", Style::default().fg(Color::DarkGray))
        };

        frame
            .buffer_mut()
            .set_string(scrollbar_x, track_start_y + y as u16, ch, style);
    }
}

pub fn render_search_bar(frame: &mut Frame, app: &App, area: Rect) {
    let search = Paragraph::new(format!("/{}", app.search_query)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Search (regex) "),
    );
    frame.render_widget(search, area);
}

pub fn render_detail_pane(frame: &mut Frame, app: &App, area: Rect) {
    let detail = render_detail(app.selected_entry());
    let detail_title = app
        .status_message()
        .map(|m| format!(" {} ", m))
        .unwrap_or(" Detail ".into());
    let detail_style = if app.status_message().is_some() {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    };
    let detail_widget = Paragraph::new(detail)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(detail_title)
                .title_style(detail_style),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(detail_widget, area);
}

pub fn render_histogram(frame: &mut Frame, app: &App, area: Rect) {
    let max_bars = (area.width as usize).saturating_sub(10) / 10;
    let hist_data: Vec<_> = app
        .histogram
        .iter()
        .rev()
        .take(max_bars)
        .rev()
        .map(|(label, val)| {
            let color = if *val > 500 {
                Color::Red
            } else if *val > 250 {
                Color::Rgb(255, 165, 0)
            } else {
                Color::Cyan
            };
            Bar::default()
                .value(*val)
                .label(Line::from(label.clone()))
                .style(Style::default().fg(color))
                .text_value(format!("{}", val))
        })
        .collect();

    let max_val = app.histogram.iter().map(|(_, v)| *v).max().unwrap_or(1);
    let total: u64 = app.histogram.iter().map(|(_, v)| *v).sum();
    let peak = app
        .histogram
        .iter()
        .max_by_key(|(_, v)| *v)
        .map(|(t, v)| format!("Peak: {} @ {}", v, t))
        .unwrap_or_default();

    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(vec![
                    Span::styled(" 时间轴 ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("(月-日 时:分, 总计:{}, {}) ", total, peak),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
                .title_bottom(
                    Line::from(vec![
                        Span::styled(" █", Style::default().fg(Color::Red)),
                        Span::styled(">500 ", Style::default().fg(Color::DarkGray)),
                        Span::styled("█", Style::default().fg(Color::Rgb(255, 165, 0))),
                        Span::styled(">250 ", Style::default().fg(Color::DarkGray)),
                        Span::styled("█", Style::default().fg(Color::Cyan)),
                        Span::styled("正常 ", Style::default().fg(Color::DarkGray)),
                    ])
                    .right_aligned(),
                ),
        )
        .data(BarGroup::default().bars(&hist_data))
        .bar_width(12)
        .bar_gap(3)
        .direction(Direction::Vertical)
        .value_style(Style::default().fg(Color::White).bg(Color::Black))
        .max(max_val);
    frame.render_widget(chart, area);
}

pub fn render_ai_popup(frame: &mut Frame, app: &App) {
    match &app.ai_state {
        AiState::Loading => {
            let area = centered_rect(40, 5, frame.area());
            frame.render_widget(Clear, area);
            let popup = Paragraph::new("⏳ AI 分析中，等耐心等待...")
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title(" AI 诊断 "));
            frame.render_widget(popup, area);
        }
        AiState::Completed(text) | AiState::Error(text) => {
            let is_error = matches!(app.ai_state, AiState::Error(_));
            let area = centered_rect(80, 60, frame.area());
            frame.render_widget(Clear, area);
            let title = if is_error {
                " ❌ AI 错误 (Esc关闭) "
            } else {
                " ✅ AI 诊断结果 (Esc关闭) "
            };
            let style = if is_error {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            let popup = Paragraph::new(text.clone())
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .title_style(style),
                );
            frame.render_widget(popup, area);
        }
        AiState::Idle => {}
    }
}

pub fn render_help_popup(frame: &mut Frame) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);
    let help_text = "\
Navigation:  ↑/↓ or k/j (Scroll), n/N (Next/Prev Match)
             g (Top), G (Bottom), : (Go to Line)
Search:      / (Find), !term (Exclude matching)
Filters:     t (Thread Focus), 1/2/3/4 (Info/Warn/Err/Debug)
Bookmarks:   m (Toggle Mark), b (Next Bookmark)
Actions:     a (AI Analyze), c (Copy Line), y (Yank JSON)
Quit:        q (Exit), Esc (Close popup)";
    let popup = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ❓ Help (Press ? or Esc to close) ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(popup, area);
}

pub fn render_jump_popup(frame: &mut Frame, app: &App) {
    if app.input_mode != InputMode::JumpInput {
        return;
    }

    let area = frame.area();
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Fill(1),
        ])
        .split(area);
    let area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(20),
            Constraint::Percentage(40),
        ])
        .split(popup_layout[1])[1];

    frame.render_widget(Clear, area);
    let text = Line::from(vec![
        Span::styled(":", Style::default().fg(Color::Yellow)),
        Span::styled(
            app.input_buffer.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(Color::Gray)),
    ]);
    let popup = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Go to Line (Enter确认, Esc取消) ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(popup, area);
}

pub fn render_ai_prompt_popup(frame: &mut Frame, app: &App) {
    if app.input_mode != InputMode::AiPromptInput {
        return;
    }

    let area = frame.area();
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(5),
            Constraint::Fill(1),
        ])
        .split(area);
    let area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(popup_layout[1])[1];

    frame.render_widget(Clear, area);

    let display_text = if app.input_buffer.is_empty() {
        Line::from(vec![
            Span::styled(
                "默认：分析此错误的根本原因...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::styled("█", Style::default().fg(Color::Gray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                app.input_buffer.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(Color::Gray)),
        ])
    };

    let popup = Paragraph::new(display_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" AI 诊断 (Enter=发送, Esc=取消) ")
            .title_bottom(Line::from(" 留空使用默认提示词，或输入自定义指令 ").fg(Color::DarkGray))
            .border_style(Style::default().fg(Color::Magenta)),
    );
    frame.render_widget(popup, area);
}

pub fn render_export_popup(frame: &mut Frame, app: &App) {
    match &app.export_state {
        ExportState::Confirm(export_type) => {
            let export_name = match export_type {
                ExportType::LogsCsv => "日志 CSV",
                ExportType::LogsJson => "日志 JSON",
                ExportType::Report => "统计报告",
                ExportType::AiAnalysis => "AI 分析结果",
            };
            let area = centered_rect(50, 10, frame.area());
            frame.render_widget(Clear, area);
            let content = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("确认导出: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        export_name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Enter ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("确认  ", Style::default().fg(Color::White)),
                    Span::styled(
                        "Esc ",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("取消", Style::default().fg(Color::White)),
                ]),
            ];
            let popup = Paragraph::new(content).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 导出确认 ")
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(popup, area);
        }
        ExportState::Exporting(export_type) => {
            let export_name = match export_type {
                ExportType::LogsCsv => "日志 CSV",
                ExportType::LogsJson => "日志 JSON",
                ExportType::Report => "统计报告",
                ExportType::AiAnalysis => "AI 分析结果",
            };
            let area = centered_rect(50, 8, frame.area());
            frame.render_widget(Clear, area);
            let content = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("正在导出: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        export_name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from("⏳ 请稍候..."),
            ];
            let popup = Paragraph::new(content).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 导出中 ")
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(popup, area);
        }
        ExportState::Success(filename) => {
            let area = centered_rect(60, 8, frame.area());
            frame.render_widget(Clear, area);
            let content = vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "✅ 导出成功!",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("文件: ", Style::default().fg(Color::Yellow)),
                    Span::styled(filename, Style::default().fg(Color::White)),
                ]),
                Line::from(""),
                Line::from("按任意键关闭"),
            ];
            let popup = Paragraph::new(content).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 成功 ")
                    .border_style(Style::default().fg(Color::Green)),
            );
            frame.render_widget(popup, area);
        }
        ExportState::Error(err) => {
            let area = centered_rect(60, 10, frame.area());
            frame.render_widget(Clear, area);
            let content = vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "❌ 导出失败!",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("错误: ", Style::default().fg(Color::Yellow)),
                    Span::styled(err, Style::default().fg(Color::White)),
                ]),
                Line::from(""),
                Line::from("按任意键关闭"),
            ];
            let popup = Paragraph::new(content).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 错误 ")
                    .border_style(Style::default().fg(Color::Red)),
            );
            frame.render_widget(popup, area);
        }
        ExportState::Idle => {}
    }
}
