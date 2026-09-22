//! AI 聊天：OpenAI 兼容协议 + SSE 流式输出，通过 Channel 边收边推给前端。
//!
//! 刻意走 `{base_url}/chat/completions` 而不是写死某一家 —— 这样 DeepSeek、通义、
//! Kimi、智谱、本地 Ollama 都能直接接，换模型只需要改 base_url。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::ipc::Channel;

use crate::config::get_secret;
use crate::vault::abs_path;
use crate::AppState;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AiEvent {
    Delta { text: String },
    /// 推理模型（如 deepseek-reasoner）的思考过程
    Reasoning { text: String },
    Done { chars: usize },
    Error { message: String },
}

const REF_CHAR_BUDGET: usize = 6000;

fn build_system_prompt(
    base: &str,
    cfg: &crate::config::AppConfig,
    refs: &[String],
) -> (String, Vec<String>) {
    let mut sys = base.to_string();
    let mut used: Vec<String> = Vec::new();
    if refs.is_empty() {
        return (sys, used);
    }
    sys.push_str("\n\n# 上下文笔记\n下面是用户挂载过来的笔记原文，请把它当作事实依据，不要凭空编造其中没有的内容。\n");
    for r in refs {
        if used.len() >= cfg.ai.max_refs {
            break;
        }
        let p = abs_path(cfg, r);
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let trimmed: String = text.chars().take(REF_CHAR_BUDGET).collect();
        let more = if text.chars().count() > REF_CHAR_BUDGET {
            "\n…（已截断）"
        } else {
            ""
        };
        sys.push_str(&format!(
            "\n--- 笔记「{r}」开始 ---\n{trimmed}{more}\n--- 笔记「{r}」结束 ---\n"
        ));
        used.push(r.clone());
    }
    (sys, used)
}

#[tauri::command]
pub async fn ai_chat(
    state: tauri::State<'_, AppState>,
    messages: Vec<ChatMessage>,
    refs: Vec<String>,
    on_event: Channel<AiEvent>,
) -> Result<(), String> {
    let (ai, cfg, key) = {
        let c = state.cfg.lock().unwrap();
        (c.ai.clone(), c.clone(), get_secret("ai_api_key").unwrap_or_default())
    };

    if key.trim().is_empty() {
        let _ = on_event.send(AiEvent::Error {
            message: "还没有配置 API Key。打开「设置 → AI」填入后重试。".into(),
        });
        return Ok(());
    }
    if ai.base_url.trim().is_empty() {
        let _ = on_event.send(AiEvent::Error {
            message: "base_url 不能为空。".into(),
        });
        return Ok(());
    }

    let (sys, used_refs) = build_system_prompt(&ai.system_prompt, &cfg, &refs);
    let mut payload_msgs: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "system",
        "content": sys
    })];
    for m in &messages {
        payload_msgs.push(serde_json::json!({ "role": m.role, "content": m.content }));
    }

    let url = format!(
        "{}/chat/completions",
        ai.base_url.trim().trim_end_matches('/')
    );
    let body = serde_json::json!({
        "model": ai.model,
        "stream": true,
        "temperature": ai.temperature,
        "messages": payload_msgs,
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", key.trim()))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = on_event.send(AiEvent::Error {
                message: format!("连接失败：{e}\n检查一下 base_url 和网络。"),
            });
            return Ok(());
        }
    };

    if !resp.status().is_success() {
        let code = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        let hint = match code.as_u16() {
            401 | 403 => "\n（看起来是 API Key 不对或者没权限）",
            404 => "\n（模型名或 base_url 可能写错了）",
            429 => "\n（触发限流了，缓一缓）",
            _ => "",
        };
        let _ = on_event.send(AiEvent::Error {
            message: format!("请求被拒绝 {code}{hint}\n{}", truncate(&txt, 600)),
        });
        return Ok(());
    }

    let mut stream = resp.bytes_stream();
    let mut raw: Vec<u8> = Vec::new();
    let mut chars = 0usize;
    let mut done_sent = false;
    let _ = used_refs;

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = on_event.send(AiEvent::Error {
                    message: format!("流中断：{e}"),
                });
                return Ok(());
            }
        };
        raw.extend_from_slice(&chunk);

        // 只解码完整的行，避免多字节字符被 chunk 边界切断
        loop {
            let Some(pos) = raw.iter().position(|b| *b == b'\n') else { break };
            let line_bytes: Vec<u8> = raw.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                let _ = on_event.send(AiEvent::Done { chars });
                done_sent = true;
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if let Some(err) = v.get("error") {
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("未知错误");
                let _ = on_event.send(AiEvent::Error { message: msg.to_string() });
                return Ok(());
            }
            let delta = &v["choices"][0]["delta"];
            if let Some(t) = delta.get("reasoning_content").and_then(|x| x.as_str()) {
                if !t.is_empty() {
                    let _ = on_event.send(AiEvent::Reasoning { text: t.to_string() });
                }
            }
            if let Some(t) = delta.get("content").and_then(|x| x.as_str()) {
                if !t.is_empty() {
                    chars += t.chars().count();
                    let _ = on_event.send(AiEvent::Delta { text: t.to_string() });
                }
            }
        }
    }

    if !done_sent {
        let _ = on_event.send(AiEvent::Done { chars });
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// 轻量连通性测试：直接打一次非流式请求，问一句最短的话。
#[tauri::command]
pub async fn ai_test(
    state: tauri::State<'_, AppState>,
    base_url: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    let (ai, key) = {
        let c = state.cfg.lock().unwrap();
        (c.ai.clone(), get_secret("ai_api_key").unwrap_or_default())
    };
    if key.trim().is_empty() {
        return Err("还没有配置 API Key".into());
    }
    let base = base_url.unwrap_or(ai.base_url);
    let mdl = model.unwrap_or(ai.model);
    let url = format!("{}/chat/completions", base.trim().trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", key.trim()))
        .json(&serde_json::json!({
            "model": mdl,
            "stream": false,
            "max_tokens": 16,
            "messages": [{"role":"user","content":"回复两个字：可用"}]
        }))
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;

    let status = resp.status();
    let txt = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{status}: {}", truncate(&txt, 400)));
    }
    let v: serde_json::Value = serde_json::from_str(&txt).map_err(|e| format!("响应解析失败：{e}"))?;
    let reply = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(无内容)")
        .trim()
        .to_string();
    Ok(format!("模型 {mdl} 响应正常：{reply}"))
}
