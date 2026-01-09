use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline},
};

use crate::app_state::App;
use crate::models::CurrentView;

pub fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let tab_style = |active: bool| {
        if active {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };
    let logs_style = tab_style(app.current_view == CurrentView::Logs);
    let dash_style = tab_style(app.current_view == CurrentView::Dashboard);
    let chat_style = tab_style(app.current_view == CurrentView::Chat);

    let spans = vec![
        Span::styled(" [F1] ", logs_style),
        Span::styled("Logs", logs_style),
        Span::raw("  "),
        Span::styled("[F2] ", dash_style),
        Span::styled("Dashboard", dash_style),
        Span::raw("  "),
        Span::styled("[F3] ", chat_style),
        Span::styled("Chat", chat_style),
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
            Constraint::Length(5),   // Top: Stats cards + Gauge
            Constraint::Min(10),     // Middle: Charts
            Constraint::Length(10),  // Bottom: Lists
        ])
        .split(area);

    render_top_row(frame, app, chunks[0]);
    render_charts_row(frame, app, chunks[1]);
    render_top_lists(frame, app, chunks[2]);
}

fn render_top_row(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),  // Health
            Constraint::Percentage(12),  // Total
            Constraint::Percentage(12),  // Errors
            Constraint::Percentage(12),  // Warns
            Constraint::Percentage(12),  // Duration
            Constraint::Percentage(37),  // Sparkline
        ])
        .split(area);

    let stats = &app.stats;

    // Health Gauge
    let health_color = if stats.health_score > 80 {
        Color::Green
    } else if stats.health_score > 50 {
        Color::Yellow
    } else {
        Color::Red
    };
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" 💚 "))
        .gauge_style(Style::default().fg(health_color).bg(Color::DarkGray))
        .percent(stats.health_score)
        .label(format!("{}%", stats.health_score));
    frame.render_widget(gauge, chunks[0]);

    // Stats cards
    let total = Paragraph::new(vec![
        Line::from(Span::styled(format!("{}", stats.total_logs), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Total", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" 📊 "));
    frame.render_widget(total, chunks[1]);

    let errors = Paragraph::new(vec![
        Line::from(Span::styled(format!("{}", stats.error_count), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Errors", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" ❌ ").border_style(Style::default().fg(Color::Red)));
    frame.render_widget(errors, chunks[2]);

    let warns = Paragraph::new(vec![
        Line::from(Span::styled(format!("{}", stats.warn_count), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Warns", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" ⚠ ").border_style(Style::default().fg(Color::Yellow)));
    frame.render_widget(warns, chunks[3]);

    let duration = Paragraph::new(vec![
        Line::from(Span::styled(&stats.log_duration, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("Range", Style::default().fg(Color::DarkGray))),
    ])
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title(" ⏱ "));
    frame.render_widget(duration, chunks[4]);

    // Error Sparkline
    let spark = Sparkline::default()
        .data(&stats.sparkline_data)
        .style(Style::default().fg(Color::Red))
        .block(Block::default().borders(Borders::ALL).title(" 📈 Error Pulse "));
    frame.render_widget(spark, chunks[5]);
}

fn render_charts_row(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    render_error_trend(frame, app, chunks[0]);
    render_source_distribution(frame, app, chunks[1]);
}

fn render_error_trend(frame: &mut Frame, app: &App, area: Rect) {
    let full_data = &app.stats.error_trend;
    let bar_width: usize = 12;
    let bar_gap: usize = 5;
    let available_width = area.width.saturating_sub(2) as usize;
    let view_capacity = available_width / (bar_width + bar_gap);

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
        format!(" 📈 错误趋势 (最新 {}/{}) ←/→ ", visible_data.len(), total_len)
    } else {
        format!(" 📈 错误趋势 (-{} 格, {}/{}) ←/→ ", app.chart_scroll, visible_data.len(), total_len)
    };

    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .data(BarGroup::default().bars(&bars))
        .bar_width(bar_width as u16)
        .bar_gap(bar_gap as u16)
        .value_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(chart, area);
}

fn render_source_distribution(frame: &mut Frame, app: &App, area: Rect) {
    let sources = &app.stats.top_sources;
    let total: u64 = sources.iter().map(|(_, c)| *c).sum();
    if total == 0 {
        let empty = Paragraph::new("No data")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" 📊 Sources "));
        frame.render_widget(empty, area);
        return;
    }

    let colors = [Color::Red, Color::Blue, Color::Yellow, Color::Magenta, Color::Cyan];
    
    // Calculate percentages
    let mut percentages: Vec<(Color, u16, String)> = Vec::new();
    for (i, (name, count)) in sources.iter().take(5).enumerate() {
        let pct = (*count as f64 / total as f64 * 100.0) as u16;
        let short_name: String = name.split(['/', '\\'].as_ref())
            .last()
            .unwrap_or(name)
            .chars()
            .take(25)
            .collect();
        percentages.push((colors[i % colors.len()], pct, short_name));
    }

    // Build ASCII pie chart
    let mut pie_lines: Vec<Line> = Vec::new();
    
    // Build single bar from percentages (not from recalculated values)
    let inner_width = (area.width as usize).saturating_sub(4);
    let mut total_len = 0usize;
    let mut segments: Vec<(Color, usize)> = Vec::new();
    
    for (i, (color, pct, _)) in percentages.iter().enumerate() {
        let segment_len = if i == percentages.len() - 1 {
            // Last segment fills remaining space to ensure alignment
            inner_width.saturating_sub(total_len)
        } else {
            ((*pct as f64 / 100.0) * inner_width as f64).floor() as usize
        };
        segments.push((*color, segment_len.max(1)));
        total_len += segment_len.max(1);
    }
    
    // Create single unified bar
    let bar_spans: Vec<Span> = segments.iter()
        .map(|(color, len)| Span::styled("█".repeat(*len), Style::default().fg(*color)))
        .collect();
    
    pie_lines.push(Line::from(bar_spans));
    pie_lines.push(Line::from(""));

    // Legend
    for (color, pct, name) in &percentages {
        pie_lines.push(Line::from(vec![
            Span::styled("██ ", Style::default().fg(*color)),
            Span::styled(format!("{:>2}% ", pct), Style::default().fg(*color).add_modifier(Modifier::BOLD)),
            Span::styled(name.clone(), Style::default().fg(Color::White)),
        ]));
    }

    let chart = Paragraph::new(pie_lines)
        .block(Block::default().borders(Borders::ALL).title(" 📊 Source Distribution "));
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

