use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::app_state::App;
use crate::models::{AiState, ChatRole, InputMode};

const SPINNERS: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn render_chat_interface(frame: &mut Frame, app: &App, area: Rect) {
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    render_chat_main(frame, app, h_chunks[0]);
    render_context_panel(frame, app, h_chunks[1]);
}

fn render_chat_main(frame: &mut Frame, app: &App, area: Rect) {
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    render_chat_history(frame, app, v_chunks[0]);
    render_chat_input(frame, app, v_chunks[1]);
}

fn render_chat_history(frame: &mut Frame, app: &App, area: Rect) {
    let inner = area.inner(Margin::new(1, 1));
    let available_height = inner.height as usize;

    let mut items: Vec<ListItem> = Vec::new();
    for msg in &app.chat_history {
        let (prefix, style) = match msg.role {
            ChatRole::User => ("You: ", Style::default().fg(Color::Cyan)),
            ChatRole::Assistant => ("AI: ", Style::default().fg(Color::Green)),
            ChatRole::System => ("Sys: ", Style::default().fg(Color::DarkGray)),
        };
        let lines: Vec<Line> = msg.content.lines()
            .map(|l| Line::from(Span::styled(l.to_string(), style)))
            .collect();
        items.push(ListItem::new(vec![Line::from(Span::styled(prefix, style.add_modifier(Modifier::BOLD)))]));
        for line in lines {
            items.push(ListItem::new(line));
        }
        items.push(ListItem::new(Line::from("")));
    }

    // Loading indicator
    if matches!(app.ai_state, AiState::Loading) {
        let spinner = SPINNERS[app.chat_spinner];
        items.push(ListItem::new(Line::from(vec![
            Span::styled("AI: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(format!("思考中 {} ", spinner), Style::default().fg(Color::Yellow)),
        ])));
    }

    // Calculate scroll offset
    let total_lines = items.len();
    let scroll_offset = if total_lines > available_height {
        (total_lines - available_height).saturating_sub(app.chat_scroll)
    } else { 0 };

    let visible_items: Vec<ListItem> = items.into_iter().skip(scroll_offset).collect();

    let title = format!(" AI Chat ({} messages) ", app.chat_history.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Magenta));

    let list = List::new(visible_items).block(block);
    frame.render_widget(list, area);
}

fn render_chat_input(frame: &mut Frame, app: &App, area: Rect) {
    let is_active = app.input_mode == InputMode::ChatInput;
    let border_color = if is_active { Color::Yellow } else { Color::DarkGray };
    
    let hint = if is_active { " (Enter=发送, Esc=退出) " } else { " (i=输入, c=清空上下文, C=清空历史) " };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(hint)
        .border_style(Style::default().fg(border_color));

    let display_text = if is_active {
        format!("{}█", app.chat_input)
    } else if app.chat_input.is_empty() {
        "按 i 开始输入...".to_string()
    } else {
        app.chat_input.clone()
    };

    let style = if is_active { Style::default().fg(Color::White) } else { Style::default().fg(Color::DarkGray) };
    let para = Paragraph::new(display_text).style(style).block(block);
    frame.render_widget(para, area);
}

fn render_context_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Context ({} logs) [p=pin, x=clear] ", app.chat_context.pinned_logs.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Blue));

    if app.chat_context.pinned_logs.is_empty() {
        let hint = Paragraph::new("在 F1 日志视图中\n按 p 挂载日志\n作为聊天上下文")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .block(block);
        frame.render_widget(hint, area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    for (i, log) in app.chat_context.pinned_logs.iter().enumerate() {
        let level_style = match log.level.to_lowercase().as_str() {
            "error" => Style::default().fg(Color::Red),
            "warn" | "warning" => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::Cyan),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("#{} ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("[{}][{}]", log.tid, log.level), level_style),
        ]));
        lines.push(Line::from(Span::styled(log.content.clone(), Style::default().fg(Color::White))));
        lines.push(Line::from(""));
    }

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}
