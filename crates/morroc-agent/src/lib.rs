//! Morroc 内置远程 LLM Agent。
//!
//! 提供一个基于 HTTP 的 Agent 服务，能够把外部 LLM 的自然语言请求
//! 转换成对服务端工具（Tools）的调用。工具由上层（`morroc-daemon`）实现。

use anyhow::{anyhow, Context};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{error, info, warn};

pub mod ontology;
pub use ontology::*;

/// 一个可被 LLM 调用的工具描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl Tool {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    pub fn empty_params() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }
}

/// LLM 对话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

/// 远程 LLM 配置。
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub timeout_seconds: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_base: String::new(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            timeout_seconds: 60,
        }
    }
}

impl LlmConfig {
    /// 从环境变量读取配置。
    ///
    /// - `MORROC_AGENT_API_BASE`：OpenAI 兼容端点，例如 `https://api.openai.com/v1`。
    /// - `MORROC_AGENT_API_KEY`：API Key。
    /// - `MORROC_AGENT_MODEL`：模型名称，默认 `gpt-4o-mini`。
    /// - `MORROC_AGENT_TIMEOUT`：请求超时秒数，默认 60。
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("MORROC_AGENT_API_BASE") {
            cfg.api_base = v;
        }
        if let Ok(v) = std::env::var("MORROC_AGENT_API_KEY") {
            cfg.api_key = v;
        }
        if let Ok(v) = std::env::var("MORROC_AGENT_MODEL") {
            cfg.model = v;
        }
        if let Ok(v) = std::env::var("MORROC_AGENT_TIMEOUT") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.timeout_seconds = n;
            }
        }
        cfg
    }
}

/// 远程 LLM 客户端。
#[derive(Debug, Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    config: LlmConfig,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let timeout = std::time::Duration::from_secs(config.timeout_seconds);
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();
        Self { http, config }
    }

    /// 发送对话请求并返回助手的回复文本。
    ///
    /// 当未配置 API Base 时，返回 mock 响应，便于无网络环境测试。
    pub async fn complete(
        &self,
        messages: &[LlmMessage],
        tools: &[Tool],
    ) -> anyhow::Result<LlmMessage> {
        if self.config.api_base.is_empty() || self.config.api_base.eq_ignore_ascii_case("mock") {
            return Ok(mock_response(messages, tools));
        }

        let url = format!(
            "{}/chat/completions",
            self.config.api_base.trim_end_matches('/')
        );
        let mut request_body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "response_format": { "type": "json_object" }
        });

        // 如果底层模型支持 OpenAI 原生 tools，则同时传入；否则仍依赖 prompt 中的 JSON 指令。
        let tools_json: Value = serde_json::to_value(tools).unwrap_or(Value::Array(vec![]));
        if let Some(obj) = request_body.as_object_mut() {
            obj.insert("tools".to_string(), tools_json);
        }

        let mut req = self.http.post(&url).json(&request_body);
        if !self.config.api_key.is_empty() {
            req = req.bearer_auth(&self.config.api_key);
        }

        let resp = req.send().await.with_context(|| "LLM API 请求失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("LLM API 返回错误 {}: {}", status, text));
        }

        let json: Value =
            serde_json::from_str(&text).with_context(|| format!("解析 LLM 响应失败: {}", text))?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(LlmMessage {
            role: "assistant".to_string(),
            content,
        })
    }
}

fn mock_response(messages: &[LlmMessage], _tools: &[Tool]) -> LlmMessage {
    let last = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("你好");

    let content = if last.contains("状态") || last.contains("status") {
        r#"{"message":"服务器正在运行，当前工具调用已就绪。","tool_calls":[{"name":"server_status","arguments":{}}]}"#
    } else if last.contains("账户") || last.contains("account") {
        r#"{"message":"正在查询账户列表。","tool_calls":[{"name":"list_accounts","arguments":{}}]}"#
    } else {
        r#"{"message":"请配置 LLM API 或询问具体命令，例如查询服务器状态、列出账户、创建脚本等。"}"#
    };

    LlmMessage {
        role: "assistant".to_string(),
        content: content.to_string(),
    }
}

/// 工具执行器。
///
/// 上层模块实现此 trait，把工具调用转换成对数据库、脚本、配置等的实际操作。
pub trait ToolExecutor: Send + Sync {
    fn execute(
        &self,
        name: &str,
        args: &Value,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send + '_>>;
}

/// 解析 LLM 返回的 JSON 内容，提取其中的工具调用。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParsedResponse {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

fn parse_response(content: &str) -> ParsedResponse {
    let trimmed = content.trim();
    let json_part = if trimmed.starts_with("```") {
        trimmed
            .lines()
            .skip(1)
            .take_while(|l| !l.starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        trimmed.to_string()
    };

    serde_json::from_str::<ParsedResponse>(&json_part).unwrap_or_else(|_| ParsedResponse {
        message: content.to_string(),
        tool_calls: vec![],
    })
}

/// Agent 主体。
#[derive(Clone)]
pub struct Agent {
    tools: Arc<Vec<Tool>>,
    executor: Arc<dyn ToolExecutor>,
    llm: LlmClient,
}

impl Agent {
    pub fn new(tools: Vec<Tool>, executor: Arc<dyn ToolExecutor>, llm: LlmClient) -> Self {
        Self {
            tools: Arc::new(tools),
            executor,
            llm,
        }
    }

    fn system_prompt(&self) -> String {
        let mut prompt = String::from(
            "你叫 Morroc Agent，是 Morroc 游戏服务端的内置助手。\n\
            你可以根据用户请求调用以下工具完成操作。\n\
            请尽量用中文回复，并在需要时输出 JSON 格式的工具调用。\n\
            响应 JSON 格式：\n\
            {\n  \"message\": \"对用户的说明\",\n  \"tool_calls\": [{ \"name\": \"工具名\", \"arguments\": { ... } }]\n}\n\n"
        );
        prompt.push_str("可用工具：\n");
        for tool in self.tools.iter() {
            prompt.push_str(&format!("- {}：{}\n", tool.name, tool.description));
            prompt.push_str(&format!("  参数：{}\n\n", tool.parameters));
        }
        prompt
    }

    pub async fn chat(&self, messages: Vec<LlmMessage>) -> anyhow::Result<ChatResponse> {
        let system = LlmMessage {
            role: "system".to_string(),
            content: self.system_prompt(),
        };
        let mut full_messages = vec![system];
        full_messages.extend(messages);

        let reply = self.llm.complete(&full_messages, &self.tools).await?;
        let parsed = parse_response(&reply.content);

        let mut tool_results = Vec::new();
        for call in &parsed.tool_calls {
            match self.executor.execute(&call.name, &call.arguments).await {
                Ok(result) => tool_results.push(ToolResult {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                    result,
                    error: None,
                }),
                Err(e) => {
                    warn!("工具 {} 执行失败: {}", call.name, e);
                    tool_results.push(ToolResult {
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                        result: Value::Null,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(ChatResponse {
            role: "assistant".to_string(),
            content: parsed.message,
            tool_results,
        })
    }

    pub async fn run_http(self: Arc<Self>, addr: SocketAddr) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/agent/health", get(health_handler))
            .route("/agent/tools", get(tools_handler))
            .route("/agent/chat", post(chat_handler))
            .with_state(self);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!("Agent HTTP 服务已启动于 {}", addr);
        axum::serve(listener, app).await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRequest {
    #[serde(default)]
    pub messages: Vec<LlmMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub role: String,
    pub content: String,
    pub tool_results: Vec<ToolResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub name: String,
    pub arguments: Value,
    pub result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

async fn tools_handler(State(agent): State<Arc<Agent>>) -> impl IntoResponse {
    Json((*agent.tools).clone())
}

async fn chat_handler(
    State(agent): State<Arc<Agent>>,
    Json(req): Json<ChatRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    match agent.chat(req.messages).await {
        Ok(resp) => Ok(Json(resp)),
        Err(e) => {
            error!("Agent chat 失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockExecutor {
        results: HashMap<String, Value>,
    }

    impl ToolExecutor for MockExecutor {
        fn execute(
            &self,
            name: &str,
            _args: &Value,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send + '_>> {
            let result = self.results.get(name).cloned().unwrap_or(Value::Null);
            Box::pin(async move { Ok(result) })
        }
    }

    fn test_tools() -> Vec<Tool> {
        vec![Tool::new(
            "server_status",
            "返回服务器运行状态",
            Tool::empty_params(),
        )]
    }

    #[tokio::test]
    async fn agent_parses_tool_call_and_executes() {
        let executor = Arc::new(MockExecutor {
            results: {
                let mut m = HashMap::new();
                m.insert(
                    "server_status".to_string(),
                    serde_json::json!({"accounts": 1}),
                );
                m
            },
        });
        let llm = LlmClient::new(LlmConfig::from_env());
        let agent = Agent::new(test_tools(), executor, llm);

        let messages = vec![LlmMessage {
            role: "user".to_string(),
            content: "查询服务器状态".to_string(),
        }];
        let resp = agent.chat(messages).await.unwrap();
        assert!(!resp.tool_results.is_empty());
        assert_eq!(resp.tool_results[0].name, "server_status");
        assert_eq!(resp.tool_results[0].result["accounts"], 1);
    }

    #[test]
    fn parse_tool_calls_from_json() {
        let content = r#"{"message":"ok","tool_calls":[{"name":"server_status","arguments":{}}]}"#;
        let parsed = parse_response(content);
        assert_eq!(parsed.message, "ok");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].name, "server_status");
    }
}
