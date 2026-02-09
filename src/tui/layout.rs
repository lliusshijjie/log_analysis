use ratatui::prelude::*;

pub struct UiLayout {
    pub sidebar: Rect,
    pub log_list: Rect,
    pub search_bar: Rect,
    pub detail: Rect,
    pub histogram: Rect,
}

pub struct FocusLayout {
    pub log_list: Rect,
    pub detail: Rect,
}

pub fn create_layout(area: Rect, show_search: bool) -> UiLayout {
    let main_chunks =
        Layout::horizontal([Constraint::Percentage(18), Constraint::Percentage(82)]).split(area);
    let sidebar = main_chunks[0];
    let content_area = main_chunks[1];

    let constraints = if show_search {
        vec![
            Constraint::Min(10),
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(10),
        ]
    } else {
        vec![
            Constraint::Min(10),
            Constraint::Length(0),
            Constraint::Length(10),
            Constraint::Length(10),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(content_area);

    UiLayout {
        sidebar,
        log_list: chunks[0],
        search_bar: chunks[1],
        detail: chunks[2],
        histogram: chunks[3],
    }
}

/// Create a layout for Focus Mode - full width, no sidebar
pub fn create_focus_layout(area: Rect) -> FocusLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),  // log_list
            Constraint::Length(10), // detail
        ])
        .split(area);

    FocusLayout {
        log_list: chunks[0],
        detail: chunks[1],
    }
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
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
