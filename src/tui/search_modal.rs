//! Search modal UI component
//!
//! This module renders the advanced search form modal with multiple input fields
//! for time range, content regex, source file, and log level selection.
//! Also supports saving/loading search templates.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app_state::App;
use crate::search::LogLevel;
use crate::search_form::{FormField, TemplateMode};
use crate::tui::layout::centered_rect_with_offset;

/// Render the advanced search modal
pub fn render_search_modal(frame: &mut Frame, app: &App) {
    if !app.search_form.is_open {
        return;
    }

    let form = &app.search_form;

    // Check if we're in a template mode
    match form.template_mode {
        TemplateMode::Saving => {
            render_save_template_dialog(frame, app);
            return;
        }
        TemplateMode::Loading => {
            render_load_template_dialog(frame, app);
            return;
        }
        TemplateMode::None => {}
    }

    // Modal dimensions: 65% width, 60% height, centered
    let area =
        centered_rect_with_offset(65, 60, frame.area(), app.popup_offset_x, app.popup_offset_y);

    // Clear background and draw outer border
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 🔍 高级搜索 ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .title_bottom(
            Line::from(
                " Tab/↑↓=切换 | Enter=下一项 | Ctrl+Enter=搜索 | Ctrl+R=清空 | Ctrl+S/ Ctrl+L=模板 | Alt+方向键移动 | Ctrl+0复位 | Esc=取消 ",
            )
            .fg(Color::DarkGray)
            .right_aligned(),
        )
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into rows for each field
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Start time
            Constraint::Length(3), // End time
            Constraint::Length(3), // Content regex
            Constraint::Length(3), // Source file
            Constraint::Length(5), // Level selection
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Submit button
            Constraint::Min(0),    // Status/error message area
        ])
        .split(inner);

    // Helper to determine field style based on focus
    let field_style = |field: FormField| -> Style {
        if form.focused_field == field {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Gray)
        }
    };

    // Start Time input
    render_input_field(
        frame,
        chunks[0],
        "开始时间",
        &form.start_time_input,
        "例: -1h (1小时前), 10:30:00, 2024-01-15 10:30:00",
        field_style(FormField::StartTime),
        form.focused_field == FormField::StartTime,
    );

    // End Time input
    render_input_field(
        frame,
        chunks[1],
        "结束时间",
        &form.end_time_input,
        "例: -30m (30分钟前), 12:00:00, 留空表示现在",
        field_style(FormField::EndTime),
        form.focused_field == FormField::EndTime,
    );

    // Content regex input
    render_input_field(
        frame,
        chunks[2],
        "内容正则",
        &form.content_input,
        "例: error|fail|exception",
        field_style(FormField::Content),
        form.focused_field == FormField::Content,
    );

    // Source file input
    render_input_field(
        frame,
        chunks[3],
        "来源文件",
        &form.source_input,
        "例: database 或 auth.rs",
        field_style(FormField::Source),
        form.focused_field == FormField::Source,
    );

    // Level selection - show hint that empty means all
    render_level_selector(
        frame,
        chunks[4],
        &form.selected_levels,
        field_style(FormField::LevelSelect),
        form.focused_field == FormField::LevelSelect,
    );

    // Submit button
    render_submit_button(frame, chunks[6], form.focused_field == FormField::SubmitBtn);

    // Status or error message
    if let Some(ref error) = form.error_message {
        let error_widget = Paragraph::new(format!("❌ {}", error))
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        frame.render_widget(error_widget, chunks[7]);
    } else if let Some(ref status) = form.status_message {
        let status_widget = Paragraph::new(format!("✅ {}", status))
            .style(Style::default().fg(Color::Green))
            .alignment(Alignment::Center);
        frame.render_widget(status_widget, chunks[7]);
    }
}

/// Render a single input field
fn render_input_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    placeholder: &str,
    border_style: Style,
    is_focused: bool,
) {
    let display_text = if value.is_empty() && !is_focused {
        Line::from(Span::styled(
            placeholder,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ))
    } else {
        let mut spans = vec![Span::styled(
            value.to_string(),
            Style::default().fg(Color::White),
        )];
        if is_focused {
            spans.push(Span::styled("█", Style::default().fg(Color::Yellow)));
        }
        Line::from(spans)
    };

    let widget = Paragraph::new(display_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", label))
            .border_style(border_style),
    );

    frame.render_widget(widget, area);
}

/// Render the log level selector
fn render_level_selector(
    frame: &mut Frame,
    area: Rect,
    selected: &std::collections::HashSet<LogLevel>,
    border_style: Style,
    is_focused: bool,
) {
    let levels = [
        (LogLevel::Debug, "Debug", Color::Blue, '1'),
        (LogLevel::Info, "Info", Color::Green, '2'),
        (LogLevel::Warn, "Warn", Color::Yellow, '3'),
        (LogLevel::Error, "Error", Color::Red, '4'),
    ];

    let mut spans: Vec<Span> = Vec::new();

    for (i, (level, name, color, key)) in levels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }

        let checkbox = if selected.contains(level) {
            "[x] ".to_string()
        } else {
            "[ ] ".to_string()
        };

        let checkbox_style = if selected.contains(level) {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Gray)
        };

        spans.push(Span::styled(checkbox, checkbox_style));
        spans.push(Span::styled(
            format!("{} ", name),
            Style::default().fg(*color).add_modifier(Modifier::BOLD),
        ));

        if is_focused {
            spans.push(Span::styled(
                format!("({})", key),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    let content = Line::from(spans);
    let hint = if selected.is_empty() {
        " 不选 = 全部级别 "
    } else if is_focused {
        " 按 1-4 切换 "
    } else {
        ""
    };

    let widget = Paragraph::new(content).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" 日志级别 (可多选) ")
            .title_bottom(Line::from(hint).right_aligned().fg(Color::DarkGray))
            .border_style(border_style),
    );

    frame.render_widget(widget, area);
}

/// Render the submit button
fn render_submit_button(frame: &mut Frame, area: Rect, is_focused: bool) {
    let style = if is_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    };

    let widget = Paragraph::new("  🔍 开始搜索  ")
        .style(style)
        .alignment(Alignment::Center);

    // Center the button horizontally
    let button_width = 20u16;
    let x = area.x + (area.width.saturating_sub(button_width)) / 2;
    let button_area = Rect::new(x, area.y, button_width, area.height);

    frame.render_widget(widget, button_area);
}

/// Render the save template dialog
fn render_save_template_dialog(frame: &mut Frame, app: &App) {
    let area =
        centered_rect_with_offset(50, 25, frame.area(), app.popup_offset_x, app.popup_offset_y);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 💾 保存搜索模板 ")
        .title_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Label
            Constraint::Length(3), // Input
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Hint
            Constraint::Min(0),    // Error
        ])
        .split(inner);

    // Label
    let label = Paragraph::new("请输入模板名称:").style(Style::default().fg(Color::White));
    frame.render_widget(label, chunks[0]);

    // Input
    let input_text = format!("{}█", app.search_form.template_name_input);
    let input = Paragraph::new(input_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(input, chunks[1]);

    // Hint
    let hint = Paragraph::new("Enter=保存  Esc=取消")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(hint, chunks[3]);

    // Error
    if let Some(ref error) = app.search_form.error_message {
        let error_widget = Paragraph::new(format!("❌ {}", error))
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        frame.render_widget(error_widget, chunks[4]);
    }
}

/// Render the load template dialog
fn render_load_template_dialog(frame: &mut Frame, app: &App) {
    let area =
        centered_rect_with_offset(50, 50, frame.area(), app.popup_offset_x, app.popup_offset_y);
    frame.render_widget(Clear, area);

    let form = &app.search_form;

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 📂 加载搜索模板 ")
        .title_style(
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .title_bottom(
            Line::from(" ↑↓=选择  Enter=加载  Esc=取消 ")
                .fg(Color::DarkGray)
                .right_aligned(),
        )
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if form.template_list.is_empty() {
        let msg = Paragraph::new("没有保存的模板\n\n按 Esc 返回")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    // Template list
    let items: Vec<ListItem> = form
        .template_list
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == form.template_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if i == form.template_selected {
                "▶ "
            } else {
                "  "
            };
            ListItem::new(format!("{}{}", prefix, name)).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::NONE));

    frame.render_widget(list, inner);
}
