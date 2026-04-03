use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app_state::App;
use crate::models::InputMode;
use crate::report::ReportPeriod;

pub fn render_report(frame: &mut Frame, app: &mut App, area: Rect) {
    let has_status = app
        .status_msg
        .as_ref()
        .map(|(_, t)| t.elapsed().as_secs() < 3)
        .unwrap_or(false);
    let main_area = if has_status {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);
        // Render status bar
        if let Some((msg, _)) = &app.status_msg {
            let status = Paragraph::new(msg.as_str()).style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(status, chunks[1]);
        }
        chunks[0]
    } else {
        area
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(0)])
        .split(main_area);

    render_period_selector(frame, app, chunks[0]);
    render_report_content(frame, app, chunks[1]);

    // Render save path input popup if in ReportSaveInput mode
    if app.input_mode == InputMode::ReportSaveInput {
        let popup_area = centered_rect(60, 3, area);
        frame.render_widget(Clear, popup_area);
        let input = Paragraph::new(app.input_buffer.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 保存路径 (Enter确认, Esc取消) ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(input, popup_area);
    }
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_period_selector(frame: &mut Frame, app: &mut App, area: Rect) {
    let periods = [
        ReportPeriod::Today,
        ReportPeriod::Yesterday,
        ReportPeriod::Week,
    ];
    let items: Vec<ListItem> = periods
        .iter()
        .map(|p| {
            let style = if *p == app.report_period {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(p.label(), style)))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 📅 报告周期 "),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    let selected_idx = match app.report_period {
        ReportPeriod::Today => 0,
        ReportPeriod::Yesterday => 1,
        ReportPeriod::Week => 2,
    };
    let mut state = ListState::default();
    state.select(Some(selected_idx));

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_report_content(frame: &mut Frame, app: &App, area: Rect) {
    let content = if app.report_generating {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            / 100) as usize
            % spinner.len();
        format!("\n\n    {} 正在生成报告，请稍候...", spinner[idx])
    } else if app.report_content.is_empty() {
        "\n\n    按 Enter 生成报告\n\n    Ctrl+C: 复制到剪贴板\n    Ctrl+S: 保存为 .md 文件".into()
    } else {
        app.report_content.clone()
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" 📊 {} ", app.report_period.label()))
                .title_bottom(" Enter:生成 | Ctrl+C:复制 | Ctrl+S:保存 "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}
