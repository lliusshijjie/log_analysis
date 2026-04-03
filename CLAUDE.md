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
- **Thread view uses raw_entries**: Thread view filters `app.raw_entries` (unfolded) while normal view tracks filtered data via `app.filtered_indices` over `app.all_entries`
- **Movable popups**: All popup positions offset by `app.popup_offset_x` / `app.popup_offset_y`
- **Focus mode history**: Browser-like back navigation via `FocusModeState.history` vector of snapshots

## Coding Best Practices

These practices are specific to this repository and should be treated as default constraints for any change.

### 1) Keep Rendering O(visible_rows), not O(total_rows)

- Never rebuild widgets for the full dataset on every frame.
- Build `ListItem`s only for visible rows in viewport-sized ranges.
- Avoid per-row linear lookups (`Vec::contains`, repeated `iter().find`) in render paths.
- Prefer prebuilt maps/sets (`HashMap`, `HashSet`) for render-time membership and color lookup.

### 2) Preserve On-Demand Redraw Behavior

- `run_app` should render only when state changes (`app.needs_redraw`).
- Any state mutation that affects UI must set `app.needs_redraw = true`.
- Do not reintroduce unconditional frame redraw loops.

### 3) Prefer Index Views over Cloning Entries

- For large log sets, pass around indices (`filtered_indices`) instead of cloning `DisplayEntry`.
- When an owned slice is required (export/background work), materialize it explicitly and locally.
- Keep hot paths zero-copy where possible; clone only at clear boundaries.

### 4) Avoid Repeated String Allocation in Hot Paths

- Reuse precomputed fields on `LogEntry` (`level_kind`, `source_file_lower`, `searchable_text`).
- Avoid `to_lowercase()`/`format!()` per-entry inside filtering/search/render loops.
- For search checks, use `DisplayEntry::matches_search(...)` rather than rebuilding search text.

### 5) Filtering/Search Performance Rules

- Filtering should be single-pass over candidate indices whenever possible.
- Compile regex once per operation, not once per row.
- Keep ordered vectors for navigation (`match_indices`) and companion sets for O(1) membership (`match_index_set`).

### 6) TUI State Consistency

- Maintain selection as filtered-view indices in normal log view.
- When changing filter/search modes, reset selection/match cursor intentionally.
- Ensure scrollbar calculations are based on total rows, not only rendered rows.

### 7) Concurrency and Responsiveness

- Do not block the UI thread for slow work (AI/export/report generation).
- Use background tasks/threads/channels for IO-heavy operations.
- For channel polling updates, only trigger redraw when state actually changed.

### 8) Testing and Verification Expectations

- For non-trivial behavior/performance-sensitive refactors, run:
  - `cargo check`
  - `cargo test`
- Keep existing behavior intact for navigation/search/focus/thread/report flows.
- Prefer adding focused unit tests when touching parser/filter/search logic.

### 9) Scope Discipline for Refactors

- Keep each performance change narrowly scoped and measurable.
- Avoid mixing unrelated architectural changes in one pass.
- Document invariants in comments when logic is non-obvious (especially selection/index mappings).

## Notes

- Clipboard uses `arboard` - requires Windows clipboard access
- Live tailing uses `notify` crate for file system watching
- AI features require local Ollama service running (`ollama serve`)
- Web server spawns on a random port for health dashboard
