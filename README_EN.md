# LogInsight TUI - Log Analyzer

## 1. Environment Setup
- **Operating System**: Windows 11 (Windows Terminal recommended for best display)
- **Development Tools**: Rust Compiler (rustc/cargo 1.75+)
- **System Components**: Clipboard access required (uses `arboard` library)

## 2. Compile & Run
Run the following in PowerShell from the project root:

```powershell
# Compile project
cargo build --release

# Run (Single file)
cargo run -- "service.log"

# Run (Multi-file - Wildcard)
cargo run -- logs/*.log

# Run (Multi-file - Explicit list)
cargo run -- file1.log file2.log file3.log
```

## 3. Core Features
- **Custom Window Identity**: Console title displays "【☺】LogInsight" with a dedicated LogInsight.ico icon for taskbar and window, improving visibility.
- **Smart Encoding**: Automatically detects and converts GB18030 encoding (Chinese) and handles nested UTF-8 JSON string escaping.
- **Enhanced Path Compatibility**: Supports Windows paths with spaces (e.g., `C:\Program Files\...`) and gracefully falls back to directory enumeration when wildcard matching fails.
- **Multi-line Merging**: Identifies cross-line JSON structures and restores them into single structured records.
- **Focus Mode** (Enhanced):
    - **Search Highlighting**: When searching (`/`) within focus view, matches are highlighted in real-time.
    - Original features preserved:
        - **Trigger**: Press `Alt+Enter` in search/list mode, or use `F6`.
        - **Function**: Creates an isolated view with only current matches; supports sub-searching (`/`).
        - **Line Numbering**: Shows sequential 1-based numbering for easy multi-line reference.
        - **Visual ID**: Cyan border and title displaying `🔍 FOCUS: query`.
        - **Actions**:
            - **Multi-line Copy (`c`)**: Supports ranges (`1-5`), lists (`1,3,5`), or mixed input. Use `*` / `a` / `all` to copy all lines at once. Copy dialog shows "Copy line numbers - e.g. 1-5, 3, 7-10, */a/all".
            - **Sub-search (`/`)**: Further filter results within focus view.
            - **Export (`e`)**: Export focus view logs.
            - **Exit**: Press `Esc` to return to normal view.
- **Trace Filtering**:
    - **Trigger**: Press `Shift+T` to extract correlation ID (traceId/requestId/UUID) from selected log.
    - **Function**: Automatically filters logs containing the same correlation ID for complete request tracing.
    - **Visual ID**: Magenta title bar showing `[FILTER: Trace <ID>]`.
    - **Configurable**: Custom correlation ID regex patterns in `log_config.toml`.
- **Horizontal Scroll & Word Wrap** (Enhanced):
    - **Horizontal Scroll**: Press `h` to scroll left, `l` to scroll right for viewing long log lines.
    - **Touch Gesture**: Touchpad two-finger swipe for horizontal scrolling (works in Focus Mode, Thread View, and Advanced Search popup).
    - **Word Wrap**: Press `w` to toggle word wrap mode for automatic line breaking.
    - **Reset Scroll**: Press `Shift+H` to reset horizontal scroll to line start.
    - **Mutual Exclusion**: Horizontal scroll is disabled when word wrap is enabled.
- **Original Line Numbers**: Displays line numbers (Ln) from the original file for easy cross-referencing.
- **Quick Jump**: Direct jump by line number or top/bottom navigation.
- **Live Tailing** (Enhanced): `tail -f` like real-time monitoring. Automatically detects and incremental loads new lines. **Path Handling**: Improved file watcher to handle Windows path case-sensitivity and symbolic links.
- **Noise Folding**: Merges continuous USB polling, thread cleaning, or duplicate logs to improve readability.
- **Startup Warnings Popup**: When some files cannot be accessed due to permissions, a warning popup displays which files were skipped along with admin-run hints.
- **File Solo Mode** (Enhanced):
    - **Pagination**: Press `Tab` to switch to file list focus, use `←` / `→` to navigate pages. Title bar shows current page `[Page 1/5]`.
    - **Workflow**: Press `Enter` once to mark the file (`[●]` indicator), press `Enter` again to enter Solo mode.
    - **Solo Mode**: Only shows logs from the selected file, auto-switches focus to log list.
    - **Visual**: Selected file shows `[●]` indicator.
- **Thread View** (Enhanced):
    - **Search Highlighting**: When searching (`/`) within thread view, matches are highlighted in real-time.
    - Original features preserved:
        - **Trigger**: Press `t` in log list view.
        - **Function**: Opens an isolated full-screen view containing all logs from the selected thread.
        - **Actions**:
            - `←` / `→` page navigation, `/` quick sub-search, `Shift+S` advanced search in thread view.
            - `c` multi-line copy (supports `*` / `a` / `all` for copy-all, copy dialog shows "Copy line numbers - e.g. 1-5, 3, 7-10, */a/all"), `e` export, `Esc` close.
- **Advanced Search**:
    - **Complex Filtering**: `Shift+S` opens a panel for time range, regex content, source file, and log level combinations.
    - **Relative Time**: Supports `-1h`, `-30m`, `-2d`, etc.
    - **Fast Actions**: `Ctrl+Enter` to apply immediately, `Ctrl+R` to clear all form inputs.
    - **Validation**: Explicit errors for invalid time formats, start time later than end time, and invalid regex.
    - **Persistent Indicator**: Active advanced filters are shown in title as `[ADV: ...]`; clear with `Ctrl+K`.
    - **Result Popup**: Submitting advanced search in main log view opens a floating result popup (1-based line numbering) with `↑↓` navigation, `←→` paging, `/` search, `c` copy, `e` export, and `Esc` to close. The popup is draggable via `Alt+Arrow` keys and does not block the main view.
- **Movable Popups**:
    - Works for advanced search panel, advanced search result popup, template save/load dialogs, and focus/thread copy dialogs.
    - Use `Alt+←/→/↑/↓` to move popup position and `Ctrl+0` to recenter.
- **Performance Profiling**:
    - **Delta Time**: Calculates time difference between logs in the same thread.
    - **Latency Highlighting**: Yellow `[+100ms]` for >100ms, Red `[SLOW]` for >1s.
- **Smart Dashboard**:
    - **Health Gauge**: Green dashboard showing system health score (0-100) based on error density.
    - **Error Pulse**: Sparkline for recent error frequency trends.
    - **Source Distribution**: Pie-chart style visualization for top 5 log sources.
    - **Trend Histogram**: Real-time minute/hour log volume and error trends.
- **Error Mini-map**: Right sidebar indicates relative positions of Error logs for quick navigation. (Note: Focus mode uses simplified scrollbar style)
- **AI Chat Interface (F3)**:
    - Dedicated view for multi-turn AI dialogue.
    - **Log Mounting**: Press `p` in log view to attach logs to AI context for deep analysis.
    - **Real-time Feedback**: Automatic scrolling and thinking animation.
    - **Context Panel**: Displays mounted log details with word wrapping.
- **Persistence (F4)**:
    - **Command History**: Records searches, jumps, and AI analysis for one-click re-execution.
    - **Templates**: Save complex filters via `Ctrl+S` and load via `Ctrl+L`.
    - **Storage**: Data saved to `~/.loginsight/history.json` and `templates.json`.
- **Smart Report Generator (F5)**:
    - Generates daily/weekly reports based on stats and AI insights.
    - **Period Selection**: Today, Yesterday, or This Week.
    - **Export**: `Ctrl+C` to clipboard, `Ctrl+S` to .md file.

## 4. Keybindings

| Key | Category | Description |
| :--- | :--- | :--- |
| `↑` / `↓` | Nav | Select up/down |
| `←` / `→` | Nav | Page up/down |
| `k` / `j` | Nav | Vim-style up/down |
| `g` / `G` | Nav | Jump to Top / Jump to Bottom |
| `:` | Nav | **Jump to specific line number** |
| `h` / `l` | Scroll | Horizontal scroll left/right (20 chars) |
| Two-finger swipe | Scroll | (Touchpad) Two-finger horizontal swipe for scrolling |
| `Shift+H` | Scroll | Reset horizontal scroll to start |
| `w` | Display | **Toggle word wrap mode** |
| `F1` | View | **Log List View** |
| `F2` | View | **Dashboard View** |
| `F3` | View | **AI Chat View** |
| `F4` | View | **History View** |
| `F5` | View | **Report Generator View** |
| `F6` | View | **Enter Focus Mode** |
| `Tab` | Focus | Switch sidebar/list focus |
| `p` | Chat | **Mount selected log to AI Context** |
| `i` | Chat | Enter input mode (Esc to exit, Enter to send) |
| `c` | Chat | Clear mounted context |
| `Shift+C` | Chat | Clear all chat history |
| `Enter` | History | Re-execute selected command |
| `Delete` / `d` | History | Delete selected history entry |
| `c` | History | Clear all history |
| `Enter` | Report | Generate AI report |
| `Ctrl+C` | Report | Copy report to clipboard |
| `Ctrl+S` | Report | Save report as .md |
| `Space` | File | Toggle file enabled state |
| `Tab` | File | Switch to file list focus |
| `←` / `→` | File | (File list) Page navigation |
| `Enter` | File | (File list) **First press** mark file `[●]` / **Second press** enter Solo mode and auto-switch focus to log list |
| `/` | Search | Quick regex search; use `&` delimiter for AND terms (Placeholder: `Normal: keyword, Combined: keyword & keyword & ...`) (Esc=clear highlights, Enter=apply) |
| `Shift+S` | Search | **Advanced Search Panel** |
| `Ctrl+Enter` | Search | (Advanced Search) apply filter immediately |
| `Ctrl+R` | Search | (Advanced Search) clear all form inputs |
| `Ctrl+K` | Search | (Main log view) clear active advanced filter `[ADV]` |
| `Alt+←/→/↑/↓` | Popup | Move active popup window (faster step) |
| `Ctrl+0` | Popup | Recenter popup window |
| `n` / `N` | Search | Next/Previous match (first match auto-selected after Enter) |
| `t` | Thread | Open/Close thread view for selected log |
| `Shift+S` | Thread | Open advanced search in thread view |
| `Shift+T` | Filter | **Trace Filtering** - Extract correlation ID and filter |
| `1/2/3/4` | Filter | Toggle Info/Warn/Error/Debug levels |
| `m` | Bookmark | Toggle bookmark (Purple 🔖) |
| `b` / `B` | Bookmark | Next/Previous bookmark |
| `f` | Tail | **Toggle Live Tailing** (Green `[LIVE]`) |
| `a` | AI | Quick AI diagnosis for selected log |
| `c` | Export | (Focus/Thread View) **Multi-line copy** (`*` / `a` / `all` supported) |
| `e` | Export | Export filtered logs to CSV (default: Windows Downloads folder) |
| `E` (Shift+E) | Export | Export filtered logs to JSON |
| `r` | Export | Export stats report |
| `R` (Shift+R) | Export | Export AI analysis |
| `?` | Help | Show help popup |
| `Esc` | State | Close popup / Clear filters / Cancel input |
| `q` | System | Quit |

## 5. AI Diagnosis & Chat
- **Dependency**: Requires local [Ollama](https://ollama.ai/) service (`ollama serve`).
- **Model**: Defaults to `qwen2.5-coder:7b`.
- **Quick Diagnosis**: Press `a` on a log line to analyze with surrounding context.
- **Multi-turn Chat**: 
    1. Select logs in `F1` and press `p` to mount.
    2. Press `F3` for the chat interface.
    3. Press `i` to ask questions regarding the mounted context.

## 6. Configuration
Generates `log_config.toml` on first run:
- **log_pattern**: Regex for parsing logs.
- **fold_rules**: Custom noise folding rules.
- **ignore_patterns**: Regex to skip loading specific lines.
- **theme**: Latency thresholds and colors.
- **paths.export_dir**: Optional export directory; defaults to Windows Downloads if omitted.

## 8. User Guide

For detailed usage instructions, see [docs/user-guide.md](docs/user-guide.md).

## 9. Troubleshooting
- **Garbage Characters**: Use `Windows Terminal` or set `chcp 65001`.
- **Input Blocker**: **Do not** run in VS Code/Cursor integrated terminals; they intercept functional keys. Use a standalone terminal.
- **Access Denied**: If you see `拒绝访问 (os error 5)`, try running as administrator, or copy log files to a user-accessible directory. The startup warning popup will suggest solutions.
- **Permission Issues**: When accessing `Program Files` directories fails, a warning popup appears with guidance. Run as administrator or copy logs to your user directory.
