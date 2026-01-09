use once_cell::sync::Lazy;
use ratatui::prelude::*;
use regex::Regex;

use crate::config::ThemeConfig;

static IP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap());
static URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[^\s]+").unwrap());
static PATH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[a-zA-Z]:\\[^<>:\|\?\*\n\r]+\.\w{2,}").unwrap());

fn color_from_name(name: &str) -> Color {
    match name.to_lowercase().as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "blue" => Color::Blue,
        "yellow" => Color::Yellow,
        "cyan" => Color::Cyan,
        "magenta" => Color::Magenta,
        "white" => Color::White,
        _ => Color::Gray,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MatchType {
    Ip,
    Url,
    Path,
}

pub fn highlight_content<'a>(content: &'a str, theme: &ThemeConfig) -> Line<'a> {
    let mut matches: Vec<(usize, usize, MatchType)> = Vec::new();

    for m in IP_RE.find_iter(content) {
        matches.push((m.start(), m.end(), MatchType::Ip));
    }
    for m in URL_RE.find_iter(content) {
        matches.push((m.start(), m.end(), MatchType::Url));
    }
    for m in PATH_RE.find_iter(content) {
        matches.push((m.start(), m.end(), MatchType::Path));
    }

    matches.sort_by_key(|m| m.0);

    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut last_end = 0;

    for (start, end, match_type) in matches {
        if start < last_end {
            continue;
        }
        if start > last_end {
            spans.push(Span::raw(&content[last_end..start]));
        }
        let color = match match_type {
            MatchType::Ip => color_from_name(&theme.ip_color),
            MatchType::Url => color_from_name(&theme.url_color),
            MatchType::Path => color_from_name(&theme.path_color),
        };
        spans.push(Span::styled(
            &content[start..end],
            Style::default().fg(color),
        ));
        last_end = end;
    }

    if last_end < content.len() {
        spans.push(Span::raw(&content[last_end..]));
    }

    Line::from(spans)
}

pub fn highlight_content_default(content: &str) -> Line<'_> {
    highlight_content(content, &ThemeConfig::default())
}
