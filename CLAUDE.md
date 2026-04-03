# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```powershell
# Compile (release)
cargo build --release

# Run with log files
cargo run -- "service.log"
cargo run -- logs/*.log
cargo run -- file1.log file2.log file3.log
```

## Architecture

**LogInsight** is a TUI log analyzer built with `ratatui` and `crossterm`. The application:

1. Parses log files at startup (mmap for performance, GB18030/UTF-8 auto-detection, multiline JSON merge)
2. Maintains `App` state in `app_state.rs` with filtering, search, and navigation
3. Renders views via `tui/` module using ratatui widgets

### Core Data Flow

```
main.rs → load_logs() → App (app_state.rs)
                          ↓
                      tui/runner.rs → event loop → tui/components.rs
```

### Key Modules

| Module | Purpose |
|--------|---------|
| `app_state.rs` | Central `App` struct holding all state (entries, filters, views) |
| `models.rs` | `LogEntry`, `DisplayEntry`, `FileInfo` data structures |
| `parser.rs` | Log parsing, encoding detection, multiline merge, delta calculation |
| `filtering.rs` | Log filtering logic (level, file, trace, thread) |
| `search.rs` / `search_form.rs` | Search criteria and advanced search form |
| `tui/runner.rs` | Main event loop and key handling dispatcher |
| `tui/components.rs` | All ratatui widget rendering functions |
| `tui/chat.rs` | AI chat interface rendering |
| `tui/dashboard.rs` | Statistics dashboard rendering |
| `ai_client.rs` | Ollama API integration for AI diagnosis/chat |
| `web/` | Embedded HTTP server for health/stats |

### View System

`App.current_view` (`CurrentView` enum) switches between:
- `Logs` - main log list
- `Dashboard` - statistics view
- `Chat` - AI chat interface
- `History` - command history
- `Report` - report generator
- `Focus` - isolated search results view
- `Thread` - isolated thread view

### Entry Types

`DisplayEntry::Normal(LogEntry)` - regular log line
`DisplayEntry::Folded{...}` - collapsed noise (duplicate/USB polling lines)

## Important Patterns

- **Horizontal scroll state**: `app.horizontal_scroll` and `app.wrap_lines` are used together in all list rendering - pass both to `render_list_item`
- **Thread view uses raw_entries**: Thread view filters `app.raw_entries` (unfolded) while normal view uses `app.filtered_entries`
- **Movable popups**: All popup positions offset by `app.popup_offset_x` / `app.popup_offset_y`
- **Focus mode history**: Browser-like back navigation via `FocusModeState.history` vector of snapshots

## Notes

- Clipboard uses `arboard` - requires Windows clipboard access
- Live tailing uses `notify` crate for file system watching
- AI features require local Ollama service running (`ollama serve`)
- Web server spawns on a random port for health dashboard
