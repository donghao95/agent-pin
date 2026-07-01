// HTTP 客户端封装
//
// 职责：
// - 封装 endpoint 配置（默认 http://127.0.0.1:4317，可通过 --endpoint 或 AGENT_PIN_ENDPOINT 覆盖）
// - 提供 get / post 方法，返回反序列化的 JSON Value
// - 统一错误处理：连接失败、HTTP 错误状态码、网络错误
//
// 设计：
// - 用 ureq 同步客户端，CLI 不需要 async
// - 错误类型用 String，main 中统一 eprintln + exit(1)
// - HTTP 错误响应体格式：{"ok":false,"error":{"code":"...","message":"..."}}
//   成功响应体格式：{"ok":true,...}
//
// 契约来源：docs/api.md §2 通用响应格式

use serde_json::{json, Value};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:4317";

pub struct Client {
    endpoint: String,
    agent: ureq::Agent,
}

impl Client {
    /// 创建 Client。endpoint 校验失败返回 Err（M6：不直接 exit，让 main 统一处理）。
    pub fn new(endpoint: Option<String>) -> Result<Self, String> {
        let endpoint = endpoint.unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
        // 校验 endpoint host 必须是本地回环，避免把 Pin 内容发送到远程主机。
        // AGENTS.md：HTTP 只监听 127.0.0.1，不开放局域网。CLI 作为客户端也应限制。
        validate_endpoint(&endpoint)?;
        // 去掉尾部斜杠，避免 format!("{}{}", endpoint, path) 产生双斜杠
        let endpoint = endpoint.trim_end_matches('/').to_string();
        Ok(Self {
            endpoint,
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(10))
                .build(),
        })
    }

    /// GET 请求，返回 JSON Value。
    /// 连接失败返回 "not running" 错误（供 health 命令区分）。
    pub fn get(&self, path: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.endpoint, path);
        match self.agent.get(&url).call() {
            Ok(resp) => resp
                .into_json::<Value>()
                .map_err(|e| format!("failed to parse response: {}", e)),
            Err(ureq::Error::Status(code, resp)) => Err(parse_error_response(code, resp)),
            Err(e) => Err(not_running_error(&e)),
        }
    }

    /// POST 请求，body 为 JSON 字符串，返回 JSON Value。
    pub fn post(&self, path: &str, body: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.endpoint, path);
        match self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(body)
        {
            Ok(resp) => resp
                .into_json::<Value>()
                .map_err(|e| format!("failed to parse response: {}", e)),
            Err(ureq::Error::Status(code, resp)) => Err(parse_error_response(code, resp)),
            Err(e) => Err(not_running_error(&e)),
        }
    }

    /// 返回 endpoint 字符串（供 health 命令输出）。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// 从 HTTP 错误响应体解析错误消息。
/// 错误响应格式：{"ok":false,"error":{"code":"...","message":"..."}}
fn parse_error_response(code: u16, resp: ureq::Response) -> String {
    let body: Value = resp.into_json().unwrap_or_else(|_| json!({}));
    let error_msg = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("unknown error");
    let error_code = body
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str())
        .unwrap_or("UNKNOWN");
    format!("[{}] {} (HTTP {})", error_code, error_msg, code)
}

/// 连接失败时的统一错误消息。
/// health 命令依赖此消息格式判断是否输出 "not running" 提示。
fn not_running_error(e: &ureq::Error) -> String {
    format!("__NOT_RUNNING__: {}", e)
}

/// 校验 endpoint host 必须是本地回环地址（C2 SSRF 防护）。
/// 用 url::Url::parse 解析，正确处理 URL 规范：
/// - 拒绝 userinfo（防 http://127.0.0.1@evil.com → host=evil.com 绕过）
/// - 大小写不敏感匹配 host（m11：LOCALHOST 也可接受）
/// 允许：127.0.0.1 / localhost / ::1（IPv6）
/// 拒绝：其他任何 host，避免 Pin 内容泄露到远程主机。
fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    let url = url::Url::parse(endpoint)
        .map_err(|e| format!("invalid endpoint URL: {}", e))?;

    // 拒绝 userinfo（防 SSRF：http://127.0.0.1@evil.com 被 CLI 误判为 host=127.0.0.1）
    if !url.username().is_empty() {
        return Err(format!(
            "endpoint must not contain userinfo: {}",
            endpoint
        ));
    }

    // 校验 scheme
    match url.scheme() {
        "http" | "https" => {}
        s => return Err(format!("endpoint scheme must be http or https: {}", s)),
    }

    // 校验 host 必须是本地回环
    let host = url
        .host_str()
        .ok_or_else(|| format!("endpoint must have a host: {}", endpoint))?;

    match host.to_lowercase().as_str() {
        "127.0.0.1" | "localhost" | "::1" => Ok(()),
        _ => Err(format!(
            "endpoint host '{}' is not allowed: only 127.0.0.1, localhost, ::1 are permitted (local-only)",
            host
        )),
    }
}
