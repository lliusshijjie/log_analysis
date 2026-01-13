use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::app_state::App;
use crate::history::CommandType;

pub fn render_history(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .history
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let (type_str, type_color) = match entry.kind {
                CommandType::Search => ("[搜索]", Color::Yellow),
                CommandType::Jump => ("[跳转]", Color::Blue),
                CommandType::AiPrompt => ("[AI]", Color::Green),
            };

            let line = Line::from(vec![
                Span::styled(format!("{:>3} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.timestamp, Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(type_str, Style::default().fg(type_color).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(&entry.content, Style::default().fg(Color::White)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" 📜 Command History ({}) ", app.history.len()))
                .title_bottom(" Enter:执行 | Delete:删除 | c:清空 | Esc:返回 "),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

    let mut state = ListState::default();
    state.select(Some(app.history.selected));

    frame.render_stateful_widget(list, area, &mut state);
}
