use anyhow::Result;
use serde::{Deserialize, Serialize};

const OLLAMA_URL: &str = "http://localhost:11434/api/generate";
const MODEL: &str = "qwen2.5-coder:7b";

const DEFAULT_PROMPT: &str = "你是一个 Windows C++ 系统安全专家。请分析以下日志片段，找出可能的根因并给出修复建议。\
如果代码存在问题，请根据日志中的源码文件名和行号，大致定位问题代码位置。\
请用中文回答，保持简洁。";

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

pub async fn analyze_error(log_context: String, custom_instruction: Option<String>) -> Result<String> {
    let instruction = custom_instruction
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PROMPT.to_string());
    
    let prompt = format!("{}\n\n日志内容:\n{}", instruction, log_context);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let resp = client.post(OLLAMA_URL)
        .json(&OllamaRequest { model: MODEL.into(), prompt, stream: false })
        .send().await?
        .json::<OllamaResponse>().await?;

    Ok(resp.response)
}

