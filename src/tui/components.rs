use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use regex::Regex;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;
use unicode_width::UnicodeWidthChar;

use crate::app_state::App;
use crate::models::{AiState, DisplayEntry, ExportState, ExportType, FileInfo, Focus, InputMode, LevelVisibility};
use crate::tui::layout::{centered_rect, centered_rect_with_offset};
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

/// Apply search regex highlighting on top of existing syntax-highlighted spans.
/// Splits spans at match boundaries and applies a yellow background to matched text.
fn apply_search_highlight(spans: Vec<Span<'static>>, regex: &Regex) -> Vec<Span<'static>> {
    let mut result: Vec<Span<'static>> = Vec::new();
    for span in spans {
        let text = span.content.to_string();
        let base_style = span.style;
        let mut last_end = 0;
        let mut had_match = false;
        for m in regex.find_iter(&text) {
            had_match = true;
            if m.start() > last_end {
                result.push(Span::styled(
                    text[last_end..m.start()].to_string(),
                    base_style,
                ));
            }
            result.push(Span::styled(
                m.as_str().to_string(),
                base_style.bg(Color::Yellow).fg(Color::Black),
            ));
            last_end = m.end();
        }
        if had_match && last_end < text.len() {
            result.push(Span::styled(text[last_end..].to_string(), base_style));
        } else if !had_match {
            result.push(Span::styled(text, base_style));
        }
    }
    result
}

/// Apply horizontal scroll offset based on terminal display columns
fn apply_horizontal_scroll(content: &str, offset: usize) -> String {
    if offset == 0 {
        return content.to_string();
    }
    let mut skipped = 0;
    let mut start_byte = content.len();
    for (i, ch) in content.char_indices() {
        if skipped >= offset {
            start_byte = i;
            break;
        }
        skipped += ch.width().unwrap_or(0);
    }
    content[start_byte..].to_string()
}

/// Truncate a Vec<Span> so that the total display width fits within `max_width` terminal columns.
fn truncate_spans_to_width(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }
    let total_width: usize = spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();
    if total_width <= max_width {
        return spans;
    }

    let mut result: Vec<Span<'static>> = Vec::new();
    let mut remaining = max_width.saturating_sub(1); // reserve 1 column for '…'

    for span in spans {
        if remaining == 0 {
            break;
        }
        let span_width = UnicodeWidthStr::width(span.content.as_ref());
        if span_width <= remaining {
            remaining -= span_width;
            result.push(span);
        } else {
            let mut truncated = String::new();
            for ch in span.content.chars() {
                let cw = ch.width().unwrap_or(0);
                if cw > remaining {
                    break;
                }
                remaining -= cw;
                truncated.push(ch);
            }
            if !truncated.is_empty() {
                result.push(Span::styled(truncated, span.style));
            }
            break;
        }
    }

    result.push(Span::styled("…", Style::default().fg(Color::DarkGray)));
    result
}

/// Sanitize control characters for stable terminal rendering.
/// This keeps parsing/export behavior unchanged because it is display-only.
fn sanitize_for_tui_display(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for ch in content.chars() {
        match ch {
            '\t' => out.push_str("    "),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn render_list_item(
    entry: &DisplayEntry,
    search_regex: Option<&Regex>,
    is_match: bool,
    is_bookmarked: bool,
    file_color: Color,
    display_index: Option<usize>,
    horizontal_scroll: usize,
    wrap_lines: bool,
    available_width: usize,
) -> ListItem<'static> {
    let line_idx = if let Some(n) = display_index {
        format!("{:>5} ", n)
    } else {
        entry.get_line_index()
            .map(|n| format!("{:>5} ", n))
            .unwrap_or_else(|| "      ".into())
    };
    let bookmark = if is_bookmarked { "🔖" } else { " " };
    let marker = if is_match { "●" } else { " " };
    match entry {
        DisplayEntry::Normal(log) => {
            // Keep rendering stable by sanitizing control characters before styling/truncation.
            let content = sanitize_for_tui_display(&log.content);
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

            let prefix_width: usize = spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();

            let display_content = if !wrap_lines && horizontal_scroll > 0 {
                apply_horizontal_scroll(&content, horizontal_scroll)
            } else {
                content
            };

            let highlighted = highlight_content_default(&display_content);
            let mut content_spans: Vec<Span<'static>> = highlighted
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect();
            if let Some(re) = search_regex {
                content_spans = apply_search_highlight(content_spans, re);
            }

            if !wrap_lines {
                let content_max = available_width.saturating_sub(prefix_width);
                if horizontal_scroll > 0 {
                    spans.push(Span::styled("…", Style::default().fg(Color::Yellow)));
                    let content_max = content_max.saturating_sub(1);
                    content_spans = truncate_spans_to_width(content_spans, content_max);
                } else {
                    content_spans = truncate_spans_to_width(content_spans, content_max);
                }
            }

            spans.extend(content_spans);
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
        } => {
            let mut folded_spans = vec![
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
            ];
            if !wrap_lines {
                folded_spans = truncate_spans_to_width(folded_spans, available_width);
            }
            ListItem::new(Line::from(folded_spans))
        }
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
            lines.push(Line::from(sanitize_for_tui_display(&log.content)));
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
            let mark = if f.marked { "[●] " } else { "    " };
            let mark_color = if f.marked { Color::Cyan } else { Color::DarkGray };
            ListItem::new(Line::from(vec![
                Span::styled(mark, Style::default().fg(mark_color)),
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

/// Unified render function that accepts all state as parameters
/// This avoids borrow checker issues when rendering from different contexts
fn render_log_list_with_state(
    frame: &mut Frame,
    area: Rect,
    entries: &[DisplayEntry],
    selected: Option<usize>,
    match_indices: &[usize],
    bookmarks: &std::collections::BTreeSet<usize>,
    error_indices: &[usize],
    is_tailing: bool,
    visible_levels: &LevelVisibility,
    filter_tid: &Option<String>,
    filter_trace: &Option<String>,
    search_regex: &Option<Regex>,
    focus: Focus,
    search_mode: bool,
    files: &[FileInfo],
    is_focus_mode: bool,
    focus_query: &str,
    horizontal_scroll: usize,
    wrap_lines: bool,
    advanced_search_summary: Option<&str>,
) {
    let tail_indicator = if is_tailing { "[LIVE] " } else { "" };

    // Level filter status
    let level_status = format!(
        "[{}I {}W {}E {}D]",
        if visible_levels.info { "●" } else { "○" },
        if visible_levels.warn { "●" } else { "○" },
        if visible_levels.error { "●" } else { "○" },
        if visible_levels.debug { "●" } else { "○" },
    );

    let (title, title_style, border_style, help) = if is_focus_mode {
        let focus_title = format!(
            " 🔍 FOCUS: {} ({} 条) {} [Esc退出]",
            focus_query,
            entries.len(),
            level_status
        );
        (
            focus_title,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Cyan),
            "←/→=Page Up/Down  / =Search  Esc=Close  c=Copy  e=Export",
        )
    } else {
        let mut title = match (filter_tid, filter_trace, search_regex) {
            (Some(tid), _, Some(_)) => format!(
                " {}[FILTER: Thread {}] [SEARCH: {} matches] {} ",
                tail_indicator, tid, match_indices.len(), level_status
            ),
            (Some(tid), _, None) => format!(
                " {}[FILTER: Thread {}] {} ",
                tail_indicator, tid, level_status
            ),
            (None, Some(trace), Some(_)) => format!(
                " {}[FILTER: Trace {}] [SEARCH: {} matches] {} ",
                tail_indicator, trace, match_indices.len(), level_status
            ),
            (None, Some(trace), None) => format!(
                " {}[FILTER: Trace {}] {} ",
                tail_indicator, trace, level_status
            ),
            (None, None, Some(_)) => format!(
                " {}[SEARCH: {} matches] {} ",
                tail_indicator, match_indices.len(), level_status
            ),
            (None, None, None) => format!(
                " {}Logs ({}) {} ",
                tail_indicator, entries.len(), level_status
            ),
        };
        if let Some(summary) = advanced_search_summary {
            let short = if summary.chars().count() > 56 {
                let mut s: String = summary.chars().take(56).collect();
                s.push('…');
                s
            } else {
                summary.to_string()
            };
            title.push_str(&format!("[ADV: {}] ", short));
        }
        let title_style = if is_tailing {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if filter_trace.is_some() {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else if filter_tid.is_some() || search_regex.is_some() {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let list_style = if focus == Focus::LogList {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let help = if is_focus_mode {
            "←/→=翻页  /=搜索  h/l=横向  w=换行  Esc=返回"
        } else if search_mode {
            "ESC=exit  F6=Focus模式"
        } else if advanced_search_summary.is_some() {
            "Tab=switch Space=toggle Enter=solo F6=Focus模式 Ctrl+K=清除高级搜索"
        } else {
            "Tab=switch Space=toggle Enter=solo F6=Focus模式 h/l=横向 w=换行"
        };
        (title, title_style, list_style, help)
    };

    // Helper to get file color
    let get_file_color = |source_id: usize| -> Color {
        files.iter()
            .find(|f| f.id == source_id)
            .map(|f| f.color)
            .unwrap_or(Color::White)
    };
    let available_item_width = area.width.saturating_sub(2) as usize;

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let file_color = e.get_source_id()
                .map(|sid| get_file_color(sid))
                .unwrap_or(Color::White);
            let idx = if is_focus_mode { Some(i + 1) } else { None };
            render_list_item(
                e,
                search_regex.as_ref(),
                match_indices.contains(&i),
                bookmarks.contains(&i),
                file_color,
                idx,
                horizontal_scroll,
                wrap_lines,
                available_item_width,
            )
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(selected);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(title_style)
                .title_bottom(Line::from(help).right_aligned())
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut list_state);

    // Custom scrollbar with error markers (only in normal mode)
    if !is_focus_mode {
        render_error_scrollbar_with_state(frame, area, &list_state, entries.len(), error_indices);
    } else {
        render_focus_scrollbar(frame, area, &list_state, entries.len());
    }
}

/// Render error scrollbar with explicit state
fn render_error_scrollbar_with_state(
    frame: &mut Frame,
    area: Rect,
    list_state: &ListState,
    total: usize,
    error_indices: &[usize],
) {
    if total == 0 || area.height < 4 {
        return;
    }

    let track_height = area.height.saturating_sub(2) as usize;
    let scrollbar_x = area.x + area.width - 1;
    let track_start_y = area.y + 1;

    let visible_rows = track_height;
    let selected = list_state.selected().unwrap_or(0);

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

        let has_error = error_indices
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

/// Render log list using app state (convenience wrapper for normal mode)
pub fn render_log_list_from_app(frame: &mut Frame, app: &mut App, area: Rect) {
    // Clone the data we need for rendering
    let entries = app.entries().to_vec();
    let match_indices = app.match_indices.clone();
    let bookmarks = app.bookmarks.clone();
    let error_indices = app.error_indices.clone();

    // Extract display data
    let is_tailing = app.is_tailing;
    let visible_levels = app.visible_levels.clone();
    let filter_tid = app.filter_tid.clone();
    let filter_trace = app.filter_trace.clone();
    let search_regex = app.search_regex.clone();
    let focus = app.focus;
    let search_mode = app.search_mode;
    let files = app.files.clone();
    let horizontal_scroll = app.horizontal_scroll;
    let wrap_lines = app.wrap_lines;
    let advanced_search_summary = app.advanced_search_summary.clone();

    // Get the list state
    let selected = app.list_state.selected();

    // Render the list
    render_log_list_with_state(
        frame,
        area,
        &entries,
        selected,
        &match_indices,
        &bookmarks,
        &error_indices,
        is_tailing,
        &visible_levels,
        &filter_tid,
        &filter_trace,
        &search_regex,
        focus,
        search_mode,
        &files,
        false,
        "",
        horizontal_scroll,
        wrap_lines,
        advanced_search_summary.as_deref(),
    );
}

/// Render log list in focus mode
pub fn render_focus_list(frame: &mut Frame, app: &mut App, area: Rect) {
    // Clone the data we need for rendering
    let entries = app.focus_mode.focus_logs.clone();
    let bookmarks = app.bookmarks.clone();
    let focus_query = app.focus_mode.focus_query.clone();
    let visible_levels = app.visible_levels.clone();
    let files = app.files.clone();
    let horizontal_scroll = app.horizontal_scroll;
    let wrap_lines = app.wrap_lines;

    // Get the list state
    let selected = app.focus_mode.focus_table_state.selected();

    // Render the focus list (empty match_indices to hide yellow dots)
    render_log_list_with_state(
        frame,
        area,
        &entries,
        selected,
        &[], // No match indices in focus mode - all entries are matches
        &bookmarks,
        &[], // No error indices in focus mode
        false, // Not tailing
        &visible_levels,
        &None, // No filter_tid in focus mode
        &None, // No filter_trace in focus mode
        &None, // No search_regex in focus mode
        Focus::LogList, // Always use log list focus in focus mode
        false, // Not search mode
        &files,
        true, // Is focus mode
        &focus_query,
        horizontal_scroll,
        wrap_lines,
        None,
    );
}

/// Render log list in thread view mode
pub fn render_thread_list(frame: &mut Frame, app: &mut App, area: Rect) {
    // Clone the data we need for rendering
    let entries = app.thread_view.thread_logs.clone();
    let bookmarks = app.bookmarks.clone();
    let thread_id = app.thread_view.thread_id.clone();
    let zoom_level = app.thread_view.zoom_level;
    let files = app.files.clone();
    let horizontal_scroll = app.horizontal_scroll;
    let wrap_lines = app.wrap_lines;

    // Get the list state
    let selected = app.thread_view.thread_table_state.selected();

    // Helper to get file color
    let get_file_color = |source_id: usize| -> Color {
        files.iter()
            .find(|f| f.id == source_id)
            .map(|f| f.color)
            .unwrap_or(Color::White)
    };

    // Title with thread ID and top-right buttons
    let title = format!(
        " Thread: {} ({} 条) [Zoom: {}] ",
        thread_id,
        entries.len(),
        zoom_level
    );

    // Top-right buttons: zoom in, zoom out, close
    let buttons = Line::from(vec![
        Span::styled("[", Style::default().fg(Color::DarkGray)),
        Span::styled("+", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" Zoom", Style::default().fg(Color::DarkGray)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("-", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(" Zoom", Style::default().fg(Color::DarkGray)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("x", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(Color::DarkGray)),
        Span::styled("]", Style::default().fg(Color::DarkGray)),
    ]).right_aligned();

    // Help text at bottom
    let help = "←/→=Page Up/Down  / =Search  h/l=横向  w=换行  Esc=Close  c=Copy  e=Export";
    let available_item_width = area.width.saturating_sub(2) as usize;

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let file_color = e.get_source_id()
                .map(|sid| get_file_color(sid))
                .unwrap_or(Color::White);
            let idx = Some(i + 1);
            render_list_item(
                e,
                None, // No search regex in thread view initially
                false, // Not a match
                bookmarks.contains(&i),
                file_color,
                idx,
                horizontal_scroll,
                wrap_lines,
                available_item_width,
            )
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(selected);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .title_top(buttons)
                .title_bottom(Line::from(help).right_aligned())
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut list_state);

    // Render scrollbar
    render_focus_scrollbar(frame, area, &list_state, entries.len());
}


// Note: render_error_scrollbar_internal was removed as it's been replaced by render_error_scrollbar_with_state

/// Render a simplified scrollbar for focus mode
fn render_focus_scrollbar(frame: &mut Frame, area: Rect, list_state: &ListState, total: usize) {
    if total == 0 || area.height < 4 {
        return;
    }

    let track_height = area.height.saturating_sub(2) as usize;
    let scrollbar_x = area.x + area.width - 1;
    let track_start_y = area.y + 1;

    let visible_rows = track_height;
    let selected = list_state.selected().unwrap_or(0);

    // Calculate thumb position and size
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
        let is_thumb = y >= thumb_pos && y < thumb_pos + thumb_size;

        let (ch, style) = if is_thumb {
            ("█", Style::default().fg(Color::Cyan))
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
    let area = centered_rect(70, 75, frame.area());
    frame.render_widget(Clear, area);
    let help_text = "\
━━━━━━━━━━━━━━━━━━━━ 视图切换 ━━━━━━━━━━━━━━━━━━━━
F1 日志列表    F2 仪表盘    F3 AI聊天    F4 历史    F5 报告

━━━━━━━━━━━━━━━━━━━━ 专注模式 (Focus Mode) ━━━━━━━━━━━━━━━━━
F6          进入专注模式 (仅显示搜索结果)
 Esc         返回上一层（无上一层时退出）
e           导出专注视图中的日志

━━━━━━━━━━━━━━━━━━━━ 导航操作 ━━━━━━━━━━━━━━━━━━━━
↑/↓ k/j     上下选择         ←/→        翻页
g/G         顶部/底部         :          跳转到行号
Tab         切换文件/日志焦点

━━━━━━━━━━━━━━━━━━━━ 水平滚动/换行 ━━━━━━━━━━━━━━━
h/l         水平左/右滚动     w          切换自动换行
Shift+H     重置水平滚动

━━━━━━━━━━━━━━━━━━━━ 搜索过滤 ━━━━━━━━━━━━━━━━━━━━
/           正则搜索          !term      反向搜索
Shift+S     高级搜索面板       n/N        下/上一匹配
t           线程过滤          Shift+T    链路追踪 (traceId)
1/2/3/4     Info/Warn/Error/Debug
Ctrl+S      保存搜索模板 (面板内)
Ctrl+L      加载搜索模板 (面板内)

━━━━━━━━━━━━━━━━━━━━ 书签功能 ━━━━━━━━━━━━━━━━━━━━
m           切换书签          b/B        下/上一书签

━━━━━━━━━━━━━━━━━━━━ AI 聊天 ━━━━━━━━━━━━━━━━━━━━━
a           AI 诊断选中日志    p          挂载日志到聊天
i           进入聊天输入 (F3)  c          清空聊天上下文
Shift+C     清空聊天历史 (F3)

━━━━━━━━━━━━━━━━━━━━ 历史记录 (F4) ━━━━━━━━━━━━━━━
Enter       重新执行          d/Delete   删除记录
c           清空历史

━━━━━━━━━━━━━━━━━━━━ 报告生成 (F5) ━━━━━━━━━━━━━━━
↑/↓         选择周期          Enter      生成报告
Ctrl+C      复制报告          Ctrl+S     保存为文件

━━━━━━━━━━━━━━━━━━━━ 导出功能 ━━━━━━━━━━━━━━━━━━━━
c/y         复制日志/JSON      e/E        导出CSV/JSON
r/R         导出报告/AI分析

━━━━━━━━━━━━━━━━━━━━ 其他功能 ━━━━━━━━━━━━━━━━━━━━
f           实时追踪 (LIVE)    ?          显示帮助
Esc         关闭/取消          q          退出程序";
    let popup = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ❓ 快捷键帮助 (按 ? 或 Esc 关闭) ")
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

/// Render the advanced search result popup (floating over the main view)
pub fn render_adv_result_popup(frame: &mut Frame, app: &mut App) {
    let popup = &app.adv_result_popup;
    if !popup.is_open {
        return;
    }

    let area = centered_rect_with_offset(80, 75, frame.area(), app.popup_offset_x, app.popup_offset_y);
    frame.render_widget(Clear, area);

    let entries = &popup.logs;
    let selected = popup.table_state.selected();
    let search_regex = popup.search_regex.as_ref();
    let files = &app.files;
    let horizontal_scroll = app.horizontal_scroll;
    let wrap_lines = app.wrap_lines;

    let get_file_color = |source_id: usize| -> Color {
        files.iter()
            .find(|f| f.id == source_id)
            .map(|f| f.color)
            .unwrap_or(Color::White)
    };

    let title = format!(" {} ({} 条) ", popup.title, entries.len());

    // Calculate live match count for search preview
    let live_match_count = if popup.search_mode && !popup.search_query.is_empty() {
        popup.search_query.chars().count();
        Regex::new(&popup.search_query).ok().map(|re| {
            popup.logs.iter().filter(|e| re.is_match(&e.get_searchable_text())).count()
        })
    } else {
        None
    };

    let help = if popup.search_mode {
        if let Some(count) = live_match_count {
            format!("匹配 {} 条  Enter=应用  Esc=取消  (支持正则)", count)
        } else {
            "Enter=应用搜索  Esc=取消搜索  (支持正则)".to_string()
        }
    } else if popup.copy_mode {
        "Enter=复制  Esc=返回  支持: 1-5, 3, 7-10, * / a / all".to_string()
    } else if !popup.match_indices.is_empty() {
        let current = popup.current_match + 1;
        let total = popup.match_indices.len();
        format!(
            "↑↓=Navigate ←/→=Page  h/l=水平  w=换行  /=Search  n/N=跳转匹配({}/{})  c=Copy  e=Export  Esc=Close",
            current, total
        )
    } else {
        "↑↓=Navigate ←/→=Page  h/l=水平  w=换行  /=Search  Esc=Close  c=Copy  e=Export  Alt+方向键=移动".to_string()
    };

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let file_color = e.get_source_id()
                .map(|sid| get_file_color(sid))
                .unwrap_or(Color::White);
            let idx = Some(i + 1);
            render_list_item(
                e,
                search_regex,
                false,
                false,
                file_color,
                idx,
                horizontal_scroll,
                wrap_lines,
                area.width.saturating_sub(2) as usize,
            )
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(selected);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
                .title_bottom(Line::from(help).right_aligned())
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut list_state);

    render_focus_scrollbar(frame, area, &list_state, entries.len());

    // Copy input overlay
    if popup.copy_mode {
        let copy_area = centered_rect_with_offset(
            40, 15, frame.area(), app.popup_offset_x, app.popup_offset_y,
        );
        frame.render_widget(Clear, copy_area);
        let display_text = if popup.copy_input.is_empty() {
            if let Some(feedback) = &popup.copy_feedback {
                Span::styled(feedback.clone(), Style::default().fg(Color::Yellow))
            } else {
                Span::styled(
                    "请输入行号, 如: 1-5, 3, 7-10, * / a / all",
                    Style::default().fg(Color::DarkGray),
                )
            }
        } else {
            Span::raw(popup.copy_input.clone())
        };
        let copy_title = popup
            .copy_feedback
            .as_deref()
            .unwrap_or("复制行号 (Enter复制 | Esc返回 | Alt+方向键移动 | Ctrl+0复位)");
        let input = Paragraph::new(Line::from(display_text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(format!(" {} ", copy_title))
                    .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            );
        frame.render_widget(input, copy_area);
        frame.set_cursor_position((
            copy_area.x + popup.copy_input.len() as u16 + 1,
            copy_area.y + 1,
        ));
    }

    // Search bar overlay
    if popup.search_mode {
        let max_width = area.width.saturating_sub(2);
        if max_width >= 12 {
            let desired_width = popup.search_query.chars().count() as u16 + 14;
            let search_width = desired_width.max(20).min(max_width);
            let search_area = Rect::new(
                area.x + area.width.saturating_sub(search_width + 1),
                area.y + 1,
                search_width,
                3,
            );
            frame.render_widget(Clear, search_area);
        let search_text = format!("/{}", popup.search_query);
        let search_bar = Paragraph::new(search_text)
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Search ")
                        .border_style(Style::default().fg(Color::Yellow)),
                );
        frame.render_widget(search_bar, search_area);
            let cursor_x = (search_area.x + 2 + popup.search_query.chars().count() as u16)
                .min(search_area.x + search_area.width.saturating_sub(2));
        frame.set_cursor_position((
                cursor_x,
                search_area.y + 1,
        ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_for_tui_display;

    #[test]
    fn sanitize_replaces_tab_with_spaces() {
        let input = "a\tb";
        let got = sanitize_for_tui_display(input);
        assert_eq!(got, "a    b");
    }

    #[test]
    fn sanitize_replaces_control_chars_with_space() {
        let input = "a\x00b\x1fc";
        let got = sanitize_for_tui_display(input);
        assert_eq!(got, "a b c");
    }
}
