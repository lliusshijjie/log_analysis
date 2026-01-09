mod ai_client;
mod analytics;
mod app_state;
mod config;
mod live;
mod logic;
mod models;
mod parser;
mod tui;

use std::fs::File;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use glob::glob;
use memmap2::Mmap;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use analytics::compute_dashboard_stats;
use app_state::App;
use config::AppConfig;
use live::TailState;
use logic::fold_noise;
use models::{ChatMessage, DashboardStats, FileInfo, LogEntry};
use parser::{build_histogram, calculate_deltas, decode_line, create_log_regex, merge_multiline_bytes, parse_line};
use tui::run_app;

#[derive(Parser)]
#[command(name = "log_analysis", about = "TUI log analyzer")]
struct Args {
    #[arg(required = true)]
    files: Vec<String>,
}

fn main() -> Result<()> {
    // 1. Parse CLI args
    let args = Args::parse();

    // 2. Load config
    let config = AppConfig::load()?;

    // 3. Load and parse log files
    let (entries, files, histogram, file_paths, re, stats) = load_logs(&args.files, &config)?;

    // 4. Setup AI background task
    let rt = tokio::runtime::Runtime::new()?;
    let (req_tx, mut req_rx) = mpsc::channel::<(String, Option<String>)>(1);
    let (resp_tx, resp_rx) = mpsc::channel::<Result<String, String>>(1);
    let (chat_req_tx, mut chat_req_rx) = mpsc::channel::<(Vec<ChatMessage>, Vec<LogEntry>)>(1);
    let (chat_resp_tx, chat_resp_rx) = mpsc::channel::<Result<String, String>>(1);
    rt.spawn(async move {
        loop {
            tokio::select! {
                Some((context, custom_instruction)) = req_rx.recv() => {
                    let result = ai_client::analyze_error(context, custom_instruction).await.map_err(|e| e.to_string());
                    let _ = resp_tx.send(result).await;
                }
                Some((history, logs)) = chat_req_rx.recv() => {
                    let result = ai_client::send_chat_request(&history, &logs).await.map_err(|e| e.to_string());
                    let _ = chat_resp_tx.send(result).await;
                }
                else => break,
            }
        }
    });

    // 5. Initialize App state
    let mut app = App::new(entries, histogram, files.clone(), req_tx, resp_rx, chat_req_tx, chat_resp_rx, config.theme.page_size);
    app.stats = stats;

    // 6. Setup file watcher for live tailing
    let (file_tx, file_rx) = std_mpsc::channel();
    let watch_paths = file_paths.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    let _ = file_tx.send(event.paths);
                }
            }
        },
        Config::default(),
    )?;
    for path in &watch_paths {
        watcher.watch(path, RecursiveMode::NonRecursive)?;
    }

    // Initialize tail state with current file sizes
    let mut tail_state = TailState::new();
    for (id, path) in file_paths.iter().enumerate() {
        if let Ok(meta) = std::fs::metadata(path) {
            tail_state.init_offset(id, meta.len());
        }
    }

    // 7. Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // 8. Run event loop
    let result = run_app(&mut terminal, &mut app, file_rx, &mut tail_state, &file_paths, &re);

    // 9. Restore terminal (always runs)
    drop(watcher);
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn load_logs(patterns: &[String], config: &AppConfig) -> Result<(Vec<models::DisplayEntry>, Vec<FileInfo>, Vec<(String, u64)>, Vec<PathBuf>, regex::Regex, DashboardStats)> {
    let colors = [Color::Red, Color::Blue, Color::Green, Color::Yellow, Color::Cyan, Color::Magenta];
    let re = create_log_regex(&config.parser)?;

    let ignore_regexes: Vec<regex::Regex> = config.filters.ignore_patterns.iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    let mut file_paths: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        for entry in glob(pattern).with_context(|| format!("无效模式: {}", pattern))? {
            file_paths.push(entry?);
        }
    }
    if file_paths.is_empty() { anyhow::bail!("没有找到匹配的文件"); }

    let mut files: Vec<FileInfo> = Vec::new();
    let mut all_entries: Vec<models::LogEntry> = Vec::new();

    for (id, path) in file_paths.iter().enumerate() {
        let file = File::open(path).with_context(|| format!("无法打开: {:?}", path))?;
        let mmap = unsafe { Mmap::map(&file)? };
        let entries: Vec<models::LogEntry> = merge_multiline_bytes(&mmap).iter().enumerate()
            .filter_map(|(i, b)| {
                let line = decode_line(b);
                if ignore_regexes.iter().any(|ig| ig.is_match(&line)) { return None; }
                parse_line(&line, b, &re, id, i + 1)
            }).collect();
        files.push(FileInfo {
            id,
            name: path.file_name().map(|s| s.to_string_lossy().into()).unwrap_or_else(|| "?".into()),
            color: colors[id % colors.len()],
            enabled: true,
        });
        all_entries.extend(entries);
    }

    all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    calculate_deltas(&mut all_entries);
    let histogram = build_histogram(&all_entries);
    let stats = compute_dashboard_stats(&all_entries);
    let folded = fold_noise(all_entries, &config.filters);

    Ok((folded, files, histogram, file_paths, re, stats))
}
