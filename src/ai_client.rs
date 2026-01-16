use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::models::{ChatMessage, ChatRole, LogEntry};

const OLLAMA_CHAT_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "qwen2.5-coder:7b";

const SYSTEM_PROMPT: &str = "你是一个 Windows C++ 系统安全专家，专门分析服务日志。\
请根据用户提供的日志上下文回答问题，找出可能的根因并给出修复建议。\
如果代码存在问题，请根据日志中的源码文件名和行号定位问题。请用中文回答，保持简洁。";

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

fn format_logs_context(logs: &[LogEntry]) -> String {
    if logs.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n【已挂载的日志上下文】:\n");
    for log in logs {
        s.push_str(&format!(
            "[{}][{}][{}]: {} ({}:{})\n",
            log.timestamp, log.tid, log.level, log.content, log.source_file, log.line_num
        ));
    }
    s
}

pub async fn send_chat_request(
    history: &[ChatMessage],
    context_logs: &[LogEntry],
) -> Result<String> {
    let mut messages = Vec::new();

    // System message with context
    let system_content = if context_logs.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{}{}", SYSTEM_PROMPT, format_logs_context(context_logs))
    };
    messages.push(OllamaMessage {
        role: "system".into(),
        content: system_content,
    });

    // Convert history
    for msg in history {
        let role = match msg.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::System => "system",
        };
        messages.push(OllamaMessage {
            role: role.into(),
            content: msg.content.clone(),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client
        .post(OLLAMA_CHAT_URL)
        .json(&OllamaChatRequest {
            model: MODEL.into(),
            messages,
            stream: false,
        })
        .send()
        .await?
        .json::<OllamaChatResponse>()
        .await?;

    Ok(resp.message.content)
}

// Keep legacy function for backward compatibility
pub async fn analyze_error(
    log_context: String,
    custom_instruction: Option<String>,
) -> Result<String> {
    let user_msg = custom_instruction
        .filter(|s| !s.trim().is_empty())
        .map(|inst| format!("{}\n\n日志内容:\n{}", inst, log_context))
        .unwrap_or_else(|| format!("请分析以下日志:\n{}", log_context));

    let history = vec![ChatMessage {
        role: ChatRole::User,
        content: user_msg,
    }];
    send_chat_request(&history, &[]).await
}

const REPORT_SYSTEM_PROMPT: &str = "你是一名 Site Reliability Engineer (SRE)。\
根据提供的 JSON 格式日志统计数据生成技术报告。\
报告结构：\
1. **摘要**: 一句话描述系统健康状况。\
2. **关键问题**: 列出主要错误及其潜在影响。\
3. **趋势分析**: 分析错误模式和活跃模块。\
4. **建议**: 可执行的下一步措施。\
输出格式: Markdown，使用中文。";

pub async fn generate_report(context_json: String) -> Result<String> {
    let messages = vec![
        OllamaMessage {
            role: "system".into(),
            content: REPORT_SYSTEM_PROMPT.into(),
        },
        OllamaMessage {
            role: "user".into(),
            content: format!("请根据以下统计数据生成报告:\n\n```json\n{}\n```", context_json),
        },
    ];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client
        .post(OLLAMA_CHAT_URL)
        .json(&OllamaChatRequest {
            model: MODEL.into(),
            messages,
            stream: false,
        })
        .send()
        .await?
        .json::<OllamaChatResponse>()
        .await?;

    Ok(resp.message.content)
}
