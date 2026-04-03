mod ai_client;
mod analytics;
mod app_state;
mod config;
mod export;
mod filtering;
mod history;
mod live;
mod logic;
mod models;
mod parser;
mod report;
mod search;
mod search_form;
mod templates;
mod time_parser;
mod tui;
mod web;

use std::fs::File;
use std::io::{stdout, BufReader, Read};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::mpsc as std_mpsc;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use glob::glob;
use memmap2::Mmap;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::prelude::*;
use tokio::sync::mpsc;
use walkdir::WalkDir;

use analytics::compute_dashboard_stats;
use app_state::App;
use config::AppConfig;
use live::TailState;
use logic::fold_noise;
use models::{ChatMessage, DashboardStats, FileInfo, LogEntry};
use parser::{
    build_histogram, calculate_deltas, create_log_regex, decode_line, merge_multiline_bytes,
    parse_line,
};
use tui::run_app;

#[derive(Parser)]
#[command(
    name = "log",
    version,
    about = "TUI 日志分析器 - 支持多文件、实时追踪、AI 分析"
)]
struct Cli {
    /// 要分析的日志文件 (支持通配符，如 *.log)
    #[arg(value_name = "FILE")]
    files: Vec<String>,

    /// 配置文件路径
    #[arg(short, long, value_name = "CONFIG")]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    // 1. Parse CLI args
    let cli = Cli::parse();

    if cli.files.is_empty() {
        Cli::command().print_help()?;
        println!("\n\n示例: log service.log");
        println!("      log logs/*.log");
        std::process::exit(0);
    }

    // 2. Load config
    let config = AppConfig::load_from(cli.config.as_deref())?;

    // 3. Load and parse log files
    let (entries, raw_entries, files, histogram, file_paths, re, stats, startup_warnings) =
        load_logs(&cli.files, &config)?;

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

    // Report generation channels
    let (report_req_tx, mut report_req_rx) = mpsc::channel::<String>(1);
    let (report_resp_tx, report_resp_rx) = mpsc::channel::<Result<String, String>>(1);
    rt.spawn(async move {
        while let Some(context_json) = report_req_rx.recv().await {
            let result = ai_client::generate_report(context_json)
                .await
                .map_err(|e| e.to_string());
            let _ = report_resp_tx.send(result).await;
        }
    });

    // 5. Initialize tail state before App takes ownership of raw_entries
    let mut tail_state = TailState::new();
    for (id, path) in file_paths.iter().enumerate() {
        if let Ok(meta) = std::fs::metadata(path) {
            tail_state.init_offset(id, meta.len());
        }
        let max_line = raw_entries
            .iter()
            .filter(|e| e.source_id == id)
            .map(|e| e.line_index)
            .max()
            .unwrap_or(0);
        tail_state.init_line_count(id, max_line);
    }

    // 6. Initialize App state
    let (export_tx, export_rx) = std::sync::mpsc::channel();
    let mut app = App::new(
        entries,
        raw_entries,
        histogram,
        files.clone(),
        req_tx,
        resp_rx,
        chat_req_tx,
        chat_resp_rx,
        export_rx,
        export_tx,
        report_req_tx,
        report_resp_rx,
        config.theme.page_size,
        startup_warnings,
    );
    app.stats = stats.clone();
    if let Some(export_dir) = config
        .paths
        .export_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        app.export_path = Some(PathBuf::from(export_dir));
    }

    // Initialize correlation regexes for trace filtering
    app.load_correlation_patterns(&config.filters.correlation_patterns);

    // Create shared state for web server
    let web_shared_state = web::state::WebSharedState::new(stats);

    // 7. Setup file watcher for live tailing
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

    // 7. Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // 7.1 Set console window title and icon (Windows only)
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleTitleW(lpConsoleTitle: *const u16) -> i32;
        }
        #[link(name = "user32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
            fn LoadImageW(
                hInst: *mut std::ffi::c_void,
                name: *const u16,
                type_: u32,
                cx: i32,
                cy: i32,
                fuLoad: u32,
            ) -> *mut std::ffi::c_void;
            fn SetClassLongPtrW(
                hWnd: *mut std::ffi::c_void,
                nIndex: i32,
                dwNewLong: isize,
            ) -> isize;
        }

        // Set console window title
        let title: Vec<u16> = OsStr::new("【☺】LogInsight")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            SetConsoleTitleW(title.as_ptr());
        }

        const IMAGE_ICON: u32 = 1;
        const LR_SHARED: u32 = 0x0008;
        const GCL_HICON: i32 = -14;
        const GCL_HICONSM: i32 = -34;

        let hwnd = unsafe { GetConsoleWindow() };
        if !hwnd.is_null() {
            // ID 1 is the icon we defined in resources.rc
            let icon_name: Vec<u16> = OsStr::new("#1")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let hicon = unsafe {
                LoadImageW(
                    ptr::null_mut(),
                    icon_name.as_ptr(),
                    IMAGE_ICON,
                    0,
                    0,
                    LR_SHARED,
                )
            };
            if !hicon.is_null() {
                unsafe {
                    SetClassLongPtrW(hwnd, GCL_HICON, hicon as isize);
                    SetClassLongPtrW(hwnd, GCL_HICONSM, hicon as isize);
                }
            }
        }
    }

    // 8. Spawn web server
    rt.spawn(async move {
        web::server::start_web_server(web_shared_state).await;
    });

    // 9. Run event loop
    let result = run_app(
        &mut terminal,
        &mut app,
        file_rx,
        &mut tail_state,
        &file_paths,
        &re,
    );

    // 9. Restore terminal (always runs)
    drop(watcher);
    disable_raw_mode()?;
    stdout().execute(DisableMouseCapture)?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

/// Split a glob pattern like "dir/*.log" into ("dir/", "*.log")
fn split_dir_and_pattern(pattern: &str) -> Option<(String, String)> {
    if let Some(pos) = pattern.rfind('/') {
        Some((pattern[..pos].to_string(), pattern[pos + 1..].to_string()))
    } else {
        None
    }
}

/// Simple glob pattern matching (supports * and ?)
fn matches_pattern(file_name: &str, pattern: &str) -> bool {
    let mut pattern_chars = pattern.chars().peekable();
    let mut file_chars = file_name.chars().peekable();

    while let Some(&c) = pattern_chars.peek() {
        match c {
            '*' => {
                pattern_chars.next();
                if pattern_chars.peek().is_none() {
                    // * at end matches everything
                    return true;
                }
                // Try matching remaining pattern at each position in file_name
                while file_chars.peek().is_some() {
                    if matches_pattern_end(file_chars.clone(), pattern_chars.clone()) {
                        return true;
                    }
                    file_chars.next();
                }
                return false;
            }
            '?' => {
                pattern_chars.next();
                if file_chars.peek().is_none() {
                    return false;
                }
                file_chars.next();
            }
            _ => {
                if file_chars.peek() != pattern_chars.peek() {
                    return false;
                }
                pattern_chars.next();
                file_chars.next();
            }
        }
    }
    file_chars.peek().is_none()
}

fn matches_pattern_end<'a>(
    mut file_chars: std::iter::Peekable<std::str::Chars<'a>>,
    mut pattern_chars: std::iter::Peekable<std::str::Chars<'a>>,
) -> bool {
    while let Some(&c) = pattern_chars.peek() {
        match c {
            '*' => {
                pattern_chars.next();
                if pattern_chars.peek().is_none() {
                    return true;
                }
                while file_chars.peek().is_some() {
                    if matches_pattern_end(file_chars.clone(), pattern_chars.clone()) {
                        return true;
                    }
                    file_chars.next();
                }
                return false;
            }
            '?' => {
                pattern_chars.next();
                if file_chars.peek().is_none() {
                    return false;
                }
                file_chars.next();
            }
            _ => {
                if file_chars.peek() != pattern_chars.peek() {
                    return false;
                }
                pattern_chars.next();
                file_chars.next();
            }
        }
    }
    file_chars.peek().is_none()
}

fn append_windows_access_hint(mut message: String) -> String {
    #[cfg(windows)]
    {
        if message.contains("(os error 5)") || message.contains("拒绝访问") {
            message.push_str("；提示: 可尝试以管理员身份运行，或将日志复制到当前用户可访问目录。");
        }
    }
    message
}

fn read_file_content(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("无法打开: {:?}", path))?;
    // SAFETY: The file handle `file` stays alive for the lifetime of `mmap`.
    // Mapping is read-only and used only during parsing.
    match unsafe { Mmap::map(&file) } {
        Ok(mmap) => Ok(mmap[..].to_vec()),
        Err(mmap_err) => {
            let mut reader = BufReader::new(file);
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .with_context(|| format!("mmap失败({}) 且回退读取失败: {:?}", mmap_err, path))?;
            Ok(buf)
        }
    }
}

fn load_logs(
    patterns: &[String],
    config: &AppConfig,
) -> Result<(
    Vec<models::DisplayEntry>,
    Vec<models::LogEntry>,
    Vec<FileInfo>,
    Vec<(String, u64)>,
    Vec<PathBuf>,
    regex::Regex,
    DashboardStats,
    Vec<String>,
)> {
    let colors = [
        Color::Red,
        Color::Blue,
        Color::Green,
        Color::Yellow,
        Color::Cyan,
        Color::Magenta,
    ];
    let re = create_log_regex(&config.parser)?;

    let ignore_regexes: Vec<regex::Regex> = config
        .filters
        .ignore_patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    let mut file_paths: Vec<PathBuf> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for pattern in patterns {
        // Normalize path separators for Windows compatibility
        let normalized = pattern.replace('\\', "/");
        let has_wildcard = normalized.contains('*') || normalized.contains('?');

        if has_wildcard {
            // Try glob first, fall back to walkdir for Windows compatibility
            let mut glob_failed = false;
            let mut glob_result: Vec<PathBuf> = Vec::new();
            match glob(&normalized) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(path) => glob_result.push(path),
                            Err(e) => warnings.push(append_windows_access_hint(format!(
                                "通配符匹配失败 {}: {}",
                                normalized, e
                            ))),
                        }
                    }
                }
                Err(e) => {
                    glob_failed = true;
                    warnings.push(format!("通配符格式无效 {}: {}", normalized, e));
                }
            }

            if !glob_failed && !glob_result.is_empty() {
                file_paths.extend(glob_result);
            } else {
                // Fall back to walkdir: extract directory and glob pattern
                if let Some((dir, file_pattern)) = split_dir_and_pattern(&normalized) {
                    let dir_path = PathBuf::from(&dir);
                    match std::fs::metadata(&dir_path) {
                        Ok(meta) if meta.is_dir() => {}
                        Ok(_) => {
                            warnings.push(format!("通配符目录不是目录: {:?}", dir_path));
                            continue;
                        }
                        Err(e) => {
                            warnings.push(append_windows_access_hint(format!(
                                "无法访问通配符目录 {:?}: {}",
                                dir_path, e
                            )));
                            continue;
                        }
                    }
                    for entry in WalkDir::new(&dir_path).max_depth(1).into_iter() {
                        match entry {
                            Ok(e) => {
                                let path = e.path();
                                if path.is_file() {
                                    let file_name = path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    if matches_pattern(&file_name, &file_pattern) {
                                        file_paths.push(path.to_path_buf());
                                    }
                                }
                            }
                            Err(e) => warnings.push(append_windows_access_hint(format!(
                                "遍历目录失败 {:?}: {}",
                                dir_path, e
                            ))),
                        }
                    }
                } else {
                    warnings.push(format!("无法解析通配符路径: {}", pattern));
                }
            }
        } else {
            // No wildcard: treat as directory or file
            let path = PathBuf::from(&normalized);
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry {
                                Ok(e) => {
                                    let entry_path = e.path();
                                    if entry_path.is_file() {
                                        file_paths.push(entry_path);
                                    }
                                }
                                Err(e) => warnings.push(append_windows_access_hint(format!(
                                    "读取目录项失败 {:?}: {}",
                                    path, e
                                ))),
                            }
                        }
                    }
                    Err(e) => warnings.push(append_windows_access_hint(format!(
                        "无法列出目录 {:?}: {}",
                        path, e
                    ))),
                },
                Ok(meta) if meta.is_file() => file_paths.push(path),
                Ok(_) => warnings.push(format!("{:?} 不是普通文件或目录", path)),
                Err(e) => warnings.push(append_windows_access_hint(format!(
                    "无法访问路径 {:?}: {}",
                    path, e
                ))),
            }
        }
    }
    if file_paths.is_empty() {
        if warnings.is_empty() {
            anyhow::bail!("没有找到匹配的文件");
        }
        anyhow::bail!("没有找到可读取的日志文件。\n{}", warnings.join("\n"));
    }

    let mut files: Vec<FileInfo> = Vec::new();
    let mut all_entries: Vec<models::LogEntry> = Vec::new();
    let mut loaded_paths: Vec<PathBuf> = Vec::new();

    for path in &file_paths {
        let data = match read_file_content(path) {
            Ok(d) => d,
            Err(e) => {
                warnings.push(append_windows_access_hint(format!(
                    "跳过文件 {:?}: {}",
                    path, e
                )));
                continue;
            }
        };
        let id = files.len();
        let entries: Vec<models::LogEntry> = merge_multiline_bytes(&data)
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let line = decode_line(b);
                if ignore_regexes.iter().any(|ig| ig.is_match(&line)) {
                    return None;
                }
                parse_line(&line, b, &re, id, i + 1)
            })
            .collect();
        files.push(FileInfo {
            id,
            name: path
                .file_name()
                .map(|s| s.to_string_lossy().into())
                .unwrap_or_else(|| "?".into()),
            color: colors[id % colors.len()],
            enabled: true,
            marked: false,
        });
        all_entries.extend(entries);
        loaded_paths.push(path.clone());
    }

    if loaded_paths.is_empty() {
        if warnings.is_empty() {
            anyhow::bail!("没有成功加载任何日志文件");
        }
        anyhow::bail!("没有成功加载任何可读取日志文件。\n{}", warnings.join("\n"));
    }

    all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    calculate_deltas(&mut all_entries);
    let histogram = build_histogram(&all_entries);
    let stats = compute_dashboard_stats(&all_entries);
    let folded = fold_noise(all_entries.clone(), &config.filters);

    Ok((
        folded,
        all_entries,
        files,
        histogram,
        loaded_paths,
        re,
        stats,
        warnings,
    ))
}
