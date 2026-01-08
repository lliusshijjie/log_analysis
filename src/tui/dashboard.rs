use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph},
};

use crate::app_state::App;
use crate::models::CurrentView;

pub fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let logs_style = if app.current_view == CurrentView::Logs {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let dash_style = if app.current_view == CurrentView::Dashboard {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let spans = vec![
        Span::styled(" [F1] ", logs_style),
        Span::styled("Logs", logs_style),
        Span::raw("  "),
        Span::styled("[F2] ", dash_style),
        Span::styled("Dashboard", dash_style),
        Span::raw("  "),
        Span::styled(
            if app.is_tailing { "● LIVE" } else { "" },
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        ),
    ];
    let header = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, area);
}

pub fn render_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(10),
            Constraint::Length(10),
        ])
        .split(area);

    render_stats_cards(frame, app, chunks[0]);
    render_error_trend(frame, app, chunks[1]);
    render_top_lists(frame, app, chunks[2]);
}

fn render_stats_cards(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats;
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    let total = Paragraph::new(vec![
        Line::from(Span::styled(format!("{}", stats.total_logs), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Total Logs", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" 📊 Total "));
    frame.render_widget(total, cards[0]);

    let errors = Paragraph::new(vec![
        Line::from(Span::styled(format!("{}", stats.error_count), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Errors", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" ❌ Errors ").border_style(Style::default().fg(Color::Red)));
    frame.render_widget(errors, cards[1]);

    let warns = Paragraph::new(vec![
        Line::from(Span::styled(format!("{}", stats.warn_count), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Warnings", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" ⚠ Warnings ").border_style(Style::default().fg(Color::Yellow)));
    frame.render_widget(warns, cards[2]);

    let duration = Paragraph::new(vec![
        Line::from(Span::styled(&stats.log_duration, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Time Range", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" ⏱ Duration "));
    frame.render_widget(duration, cards[3]);
}

fn render_error_trend(frame: &mut Frame, app: &App, area: Rect) {
    let full_data = &app.stats.error_trend;
    let bar_width: usize = 12;
    let bar_gap: usize = 5;
    let available_width = area.width.saturating_sub(2) as usize; // exclude borders
    let view_capacity = available_width / (bar_width + bar_gap);

    // Sliding window calculation
    let total_len = full_data.len();
    let end_index = total_len.saturating_sub(app.chart_scroll);
    let start_index = end_index.saturating_sub(view_capacity);
    let visible_data = &full_data[start_index..end_index];

    let bars: Vec<Bar> = visible_data.iter().map(|(label, val)| {
        Bar::default()
            .value(*val)
            .label(Line::from(label.to_string()).centered())
            .text_value(format!("{}条", val))
            .style(Style::default().fg(Color::Red))
    }).collect();

    let title = if app.chart_scroll == 0 {
        format!(" 📈 错误趋势 (最新, {}/{}) ←/→滚动 ", visible_data.len(), total_len)
    } else {
        format!(" 📈 错误趋势 (历史 -{} 格, {}/{}) ←/→滚动 ", app.chart_scroll, visible_data.len(), total_len)
    };

    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .data(BarGroup::default().bars(&bars))
        .bar_width(bar_width as u16)
        .bar_gap(bar_gap as u16)
        .value_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(chart, area);
}

fn render_top_lists(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let source_items: Vec<ListItem> = app.stats.top_sources.iter().map(|(name, count)| {
        let short_name = name.split('/').last().unwrap_or(name);
        ListItem::new(Line::from(vec![
            Span::styled(format!("{:>5} ", count), Style::default().fg(Color::Cyan)),
            Span::raw(short_name),
        ]))
    }).collect();
    let sources = List::new(source_items)
        .block(Block::default().borders(Borders::ALL).title(" 🔥 Top Sources "));
    frame.render_widget(sources, chunks[0]);

    let thread_items: Vec<ListItem> = app.stats.top_threads.iter().map(|(tid, count)| {
        ListItem::new(Line::from(vec![
            Span::styled(format!("{:>5} ", count), Style::default().fg(Color::Magenta)),
            Span::raw(tid),
        ]))
    }).collect();
    let threads = List::new(thread_items)
        .block(Block::default().borders(Borders::ALL).title(" 🧵 Active Threads "));
    frame.render_widget(threads, chunks[1]);
}
