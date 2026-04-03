# LogInsight / 日志分析器

> Rust TUI log analyzer with filtering, search, focus/thread views, live tailing, dashboard, and AI chat.
>
> 基于 Rust + ratatui 的终端日志分析器，支持过滤、搜索、专注/线程视图、实时追踪、统计仪表盘与 AI 对话分析。

## 中文说明

### 1. 环境要求
- Windows 10/11（推荐 Windows Terminal）
- Rust 1.75+（`rustc` / `cargo`）
- 需要本地终端剪贴板访问权限（`arboard`）

### 2. 编译与运行
```powershell
# 编译
cargo build --release

# 运行（单文件）
cargo run -- "service.log"

# 运行（多文件）
cargo run -- logs/*.log
cargo run -- file1.log file2.log
```

### 3. 核心功能
- **多文件日志解析**：支持普通路径和通配符，Windows 路径兼容增强。
- **自动解码与容错**：优先 UTF-8，失败回退 GB18030，再回退 lossless-ish 显示策略。
- **多行日志合并**：自动合并跨行 JSON/堆栈文本。
- **过滤与搜索**：
  - `/` 正则搜索（支持 `!term` 反向搜索）
  - `Shift+S` 高级搜索（时间范围/级别/来源/内容正则）
  - `Ctrl+K` 清除高级搜索条件
- **专注模式（F6）**：仅展示结果集，可二次搜索并高亮匹配词。
- **线程视图（t）**：按线程查看完整链路，支持二次搜索和高亮。
- **折叠日志处理**：
  - 自动折叠噪声/重复日志
  - 选中折叠行后按 `Enter` 可展开查看该折叠块
- **实时追踪（f）**：文件增长时自动增量读取并刷新视图。
- **可视化统计**：顶部健康度 + 错误趋势 + 来源分布 + 底部直方图。
- **AI 分析**：
  - `a` 对当前上下文做诊断
  - `F3` 进入聊天视图，支持挂载日志上下文（`p`）

### 4. 导出说明
- `e`：导出当前可见日志为 CSV
- `E`（Shift+E）：导出当前可见日志为 JSON
- `r`：导出统计报告
- `R`（Shift+R）：导出 AI 分析结果

#### JSON 导出中的 `content` 规范化
- 若日志 `content` 本身是合法 JSON（对象/数组/值），导出时会作为 JSON 值写入（结构化、格式化）。
- 若 `content` 不是 JSON，则保持字符串原样导出。

### 5. 常用快捷键
- `↑/↓` 或 `j/k`：上下移动
- `←/→`：翻页
- `g/G`：跳到顶部/底部
- `:`：按行号跳转
- `h/l`：水平滚动
- `Shift+H`：重置水平滚动
- `w`：切换自动换行
- `Tab`：切换文件列表/日志列表焦点
- `f`：切换 Live 模式
- `m` / `b` / `B`：书签切换与导航
- `?`：帮助
- `q`：退出

### 6. 配置
首次运行会在当前目录生成 `log_config.toml`，可配置：
- 日志正则（`log_pattern`）
- 折叠规则（`fold_rules`）
- 忽略规则（`ignore_patterns`）
- UI 参数（如 `page_size`）

### 7. 开发验证
```powershell
cargo check
cargo test
```

---

## English

### 1. Requirements
- Windows 10/11 (Windows Terminal recommended)
- Rust 1.75+ (`rustc` / `cargo`)
- Clipboard access for terminal session (`arboard`)

### 2. Build and Run
```powershell
# Build
cargo build --release

# Run (single file)
cargo run -- "service.log"

# Run (multiple files)
cargo run -- logs/*.log
cargo run -- file1.log file2.log
```

### 3. Key Features
- **Multi-file log ingestion** with wildcard and Windows path compatibility.
- **Robust decoding**: UTF-8 first, then GB18030 fallback, then tolerant fallback text decoding.
- **Multiline merge** for JSON/stack-like log blocks.
- **Filter and search**:
  - `/` regex search (supports negative search: `!term`)
  - `Shift+S` advanced search (time/level/source/content regex)
  - `Ctrl+K` clear advanced search criteria
- **Focus mode (`F6`)** with in-view search and term highlighting.
- **Thread view (`t`)** with thread-local filtering and highlighting.
- **Folded logs**:
  - Noise/identical lines can be folded
  - Press `Enter` on a folded row to open expanded content
- **Live tailing (`f`)** with incremental updates.
- **Dashboard** with health, trend, source distribution, and histogram.
- **AI diagnostics/chat**:
  - `a` for quick diagnostics
  - `F3` chat view with pinned context (`p`)

### 4. Export
- `e`: export visible logs to CSV
- `E` (Shift+E): export visible logs to JSON
- `r`: export statistics report
- `R` (Shift+R): export AI analysis

#### `content` normalization in JSON export
- If a log `content` is valid JSON, it is exported as a structured JSON value.
- Otherwise, `content` remains a plain string.

### 5. Common Shortcuts
- `↑/↓` or `j/k`: move selection
- `←/→`: page up/down
- `g/G`: jump to top/bottom
- `:`: jump to line number
- `h/l`: horizontal scroll
- `Shift+H`: reset horizontal scroll
- `w`: toggle wrap mode
- `Tab`: switch focus (file list / log list)
- `f`: toggle live mode
- `m` / `b` / `B`: bookmark toggle/next/previous
- `?`: help
- `q`: quit

### 6. Configuration
On first run, `log_config.toml` is generated in the working directory.
You can customize parsing pattern, fold rules, ignore rules, and UI settings (e.g. `page_size`).

### 7. Dev Verification
```powershell
cargo check
cargo test
```
