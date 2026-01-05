mod ai_client;
mod app_state;
mod logic;
mod models;
mod parser;
mod tui;

use std::fs::File;
use std::io::stdout;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use glob::glob;
use memmap2::Mmap;
use ratatui::prelude::*;
use tokio::sync::mpsc;

use app_state::App;
use logic::fold_noise;
use models::FileInfo;
use parser::{build_histogram, calculate_deltas, decode_line, log_regex, merge_multiline_bytes, parse_line};
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

    // 2. Load and parse log files
    let (entries, files, histogram) = load_logs(&args.files)?;

    // 3. Setup AI background task
    let rt = tokio::runtime::Runtime::new()?;
    let (req_tx, mut req_rx) = mpsc::channel::<String>(1);
    let (resp_tx, resp_rx) = mpsc::channel::<Result<String, String>>(1);
    rt.spawn(async move {
        while let Some(context) = req_rx.recv().await {
            let result = ai_client::analyze_error(context).await.map_err(|e| e.to_string());
            let _ = resp_tx.send(result).await;
        }
    });

    // 4. Initialize App state
    let mut app = App::new(entries, histogram, files, req_tx, resp_rx);

    // 5. Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // 6. Run event loop
    let result = run_app(&mut terminal, &mut app);

    // 7. Restore terminal (always runs)
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn load_logs(patterns: &[String]) -> Result<(Vec<models::DisplayEntry>, Vec<FileInfo>, Vec<(String, u64)>)> {
    let colors = [Color::Red, Color::Blue, Color::Green, Color::Yellow, Color::Cyan, Color::Magenta];
    let re = log_regex();

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
            .filter_map(|(i, b)| parse_line(&decode_line(b), b, &re, id, i + 1)).collect();
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
    let folded = fold_noise(all_entries);

    Ok((folded, files, histogram))
}
