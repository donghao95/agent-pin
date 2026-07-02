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
// 契约来源：docs/03_api.md §2 通用响应格式

use serde_json::{json, Value};

use crate::error::CliError;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:4317";

pub struct Client {
    endpoint: String,
    agent: ureq::Agent,
}

impl Client {
    /// 创建 Client。endpoint 校验失败返回 Err（M6：不直接 exit，让 main 统一处理）。
    pub fn new(endpoint: Option<String>) -> Result<Self, CliError> {
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
                .redirects(0) // M6：禁止重定向，防 SSRF 数据外泄
                .build(),
        })
    }

    /// GET 请求，返回 JSON Value。
    /// 连接失败返回 "not running" 错误（供 health 命令区分）。
    pub fn get(&self, path: &str) -> Result<Value, CliError> {
        let url = format!("{}{}", self.endpoint, path);
        match self.agent.get(&url).call() {
            Ok(resp) => resp.into_json::<Value>().map_err(|e| {
                CliError::new("INVALID_RESPONSE", format!("解析服务端响应失败: {}", e))
            }),
            Err(ureq::Error::Status(code, resp)) => {
                // 3xx 重定向视为错误（我们禁止重定向，但防御性处理）
                if (300..400).contains(&code) {
                    return Err(CliError::new(
                        "REDIRECT_BLOCKED",
                        format!("服务端返回重定向（{}），已因安全原因阻止", code),
                    ));
                }
                Err(parse_error_response(code, resp))
            }
            Err(e) => Err(classify_transport_error(&e)),
        }
    }

    /// POST 请求，body 为 JSON 字符串，返回 JSON Value。
    pub fn post(&self, path: &str, body: &str) -> Result<Value, CliError> {
        let url = format!("{}{}", self.endpoint, path);
        match self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(body)
        {
            Ok(resp) => resp.into_json::<Value>().map_err(|e| {
                CliError::new("INVALID_RESPONSE", format!("解析服务端响应失败: {}", e))
            }),
            Err(ureq::Error::Status(code, resp)) => {
                if (300..400).contains(&code) {
                    return Err(CliError::new(
                        "REDIRECT_BLOCKED",
                        format!("服务端返回重定向（{}），已因安全原因阻止", code),
                    ));
                }
                Err(parse_error_response(code, resp))
            }
            Err(e) => Err(classify_transport_error(&e)),
        }
    }

    /// 返回 endpoint 字符串（供 health 命令输出）。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// 从 HTTP 错误响应体解析错误消息。
/// 错误响应格式：{"ok":false,"error":{"code":"...","message":"..."}}
/// 非 JSON 响应或缺 error.message 时保留原始内容（截断到 500 字节避免过长）。
fn parse_error_response(code: u16, resp: ureq::Response) -> CliError {
    let raw = resp.into_string().unwrap_or_default();
    parse_error_body(code, &raw)
}

/// 解析错误响应体的纯逻辑（IO 解耦，便于测试）。
/// 错误响应格式：{"ok":false,"error":{"code":"...","message":"..."}}
/// 非 JSON 响应或缺 error.message 时保留原始内容（按 char 边界截断到 500 字节避免过长）。
fn parse_error_body(code: u16, raw: &str) -> CliError {
    let body: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!({}));
    let error_msg = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let error_code = body
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str())
        .unwrap_or("UNKNOWN");
    if error_msg.is_empty() {
        // 非 JSON 响应或缺少 error.message，展示原始内容（按 char 边界截断到 500 字节，避免 panic）
        let truncated = if raw.len() > 500 {
            let mut end = 500;
            while !raw.is_char_boundary(end) {
                end -= 1;
            }
            &raw[..end]
        } else {
            raw
        };
        CliError::new(error_code, format!("HTTP {} 原始响应: {}", code, truncated))
    } else {
        CliError::new(error_code, format!("{} (HTTP {})", error_msg, code))
    }
}

/// 分类传输层错误，区分"未运行"和"其他网络错误"。
/// health 命令依赖 `__NOT_RUNNING__` 前缀判断是否输出 "not running" 提示。
/// 连接失败视为"未运行"；超时/DNS 等其他错误不误导用户。
fn classify_transport_error(e: &ureq::Error) -> CliError {
    match e {
        ureq::Error::Status(_, _) => unreachable!("Status errors handled separately"),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::ConnectionFailed => CliError::not_running(e.to_string()),
            _ => {
                // 超时、DNS、TLS 等其他网络错误，不标记为"未运行"
                CliError::new("NETWORK_ERROR", format!("网络错误: {} ({:?})", e, t.kind()))
            }
        },
    }
}

/// 校验 endpoint host 必须是本地回环地址（C2 SSRF 防护）。
/// 用 url::Url::parse 解析，正确处理 URL 规范：
/// - 拒绝 userinfo（防 http://127.0.0.1@evil.com → host=evil.com 绕过）
/// - 只允许 http scheme（本地只监听 http，https 语义不一致）
/// - 拒绝 path/query/fragment（endpoint 应只含 scheme://host:port）
/// - 大小写不敏感匹配 host（m11：LOCALHOST 也可接受）
///
/// 允许：127.0.0.1 / localhost / ::1（IPv6）
/// 拒绝：其他任何 host，避免 Pin 内容泄露到远程主机。
fn validate_endpoint(endpoint: &str) -> Result<(), CliError> {
    let url = url::Url::parse(endpoint)
        .map_err(|e| CliError::new("INVALID_ENDPOINT", format!("endpoint URL 无效: {}", e)))?;

    // 拒绝 userinfo（防 SSRF：http://127.0.0.1@evil.com 被 CLI 误判为 host=127.0.0.1）
    if !url.username().is_empty() {
        return Err(CliError::new(
            "INVALID_ENDPOINT",
            format!("endpoint 不能包含 userinfo: {}", endpoint),
        ));
    }

    // 校验 scheme：只允许 http（本地只监听 http，https 语义不一致）
    match url.scheme() {
        "http" => {}
        s => {
            return Err(CliError::new(
                "INVALID_ENDPOINT",
                format!("endpoint scheme 必须是 http（仅本地）: {}", s),
            ))
        }
    }

    // 校验 host 必须是本地回环
    let host = url.host_str().ok_or_else(|| {
        CliError::new(
            "INVALID_ENDPOINT",
            format!("endpoint 必须包含 host: {}", endpoint),
        )
    })?;

    match host.to_lowercase().as_str() {
        // url::Url::host_str() 对 IPv6 返回带方括号的形式 "[::1]"，而非 "::1"
        "127.0.0.1" | "localhost" | "[::1]" => {}
        _ => {
            return Err(CliError::new(
                "INVALID_ENDPOINT",
                format!(
                    "endpoint host '{}' 不允许：仅允许 127.0.0.1、localhost、::1（仅本地）",
                    host
                ),
            ))
        }
    }

    // 拒绝 path/query/fragment（endpoint 应该只有 scheme://host:port）
    if !url.path().is_empty() && url.path() != "/" {
        return Err(CliError::new(
            "INVALID_ENDPOINT",
            format!("endpoint 不能包含 path: {}", endpoint),
        ));
    }
    if url.query().is_some() {
        return Err(CliError::new(
            "INVALID_ENDPOINT",
            format!("endpoint 不能包含 query: {}", endpoint),
        ));
    }
    if url.fragment().is_some() {
        return Err(CliError::new(
            "INVALID_ENDPOINT",
            format!("endpoint 不能包含 fragment: {}", endpoint),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- validate_endpoint ----------

    #[test]
    fn validate_endpoint_accepts_default_local() {
        assert!(validate_endpoint("http://127.0.0.1:4317").is_ok());
    }

    #[test]
    fn validate_endpoint_accepts_localhost() {
        assert!(validate_endpoint("http://localhost:4317").is_ok());
    }

    #[test]
    fn validate_endpoint_accepts_ipv6_loopback() {
        // url::Url::host_str() 对 IPv6 返回 "[::1]"，validate_endpoint 已适配
        assert!(validate_endpoint("http://[::1]:4317").is_ok());
    }

    #[test]
    fn validate_endpoint_accepts_custom_port() {
        assert!(validate_endpoint("http://127.0.0.1:8080").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:1").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:65535").is_ok());
    }

    #[test]
    fn validate_endpoint_accepts_uppercase_localhost() {
        // m11：大小写不敏感匹配 host
        assert!(validate_endpoint("http://LOCALHOST:4317").is_ok());
        assert!(validate_endpoint("http://Localhost:4317").is_ok());
    }

    #[test]
    fn validate_endpoint_accepts_trailing_slash() {
        // url::Url::parse 把 "http://127.0.0.1:4317/" 的 path 规范化为 "/"
        assert!(validate_endpoint("http://127.0.0.1:4317/").is_ok());
    }

    #[test]
    fn validate_endpoint_rejects_https_scheme() {
        let err = validate_endpoint("https://127.0.0.1:4317").unwrap_err();
        assert!(err.message.contains("scheme 必须是 http"), "got: {}", err);
    }

    #[test]
    fn validate_endpoint_rejects_other_schemes() {
        assert!(validate_endpoint("ftp://127.0.0.1:4317").is_err());
        assert!(validate_endpoint("file:///etc/passwd").is_err());
        assert!(validate_endpoint("ws://127.0.0.1:4317").is_err());
    }

    #[test]
    fn validate_endpoint_rejects_non_loopback_host() {
        // 私网、公网、0.0.0.0 都必须拒绝（SSRF 防护核心）
        assert!(validate_endpoint("http://192.168.1.1:4317").is_err());
        assert!(validate_endpoint("http://10.0.0.1:4317").is_err());
        assert!(validate_endpoint("http://0.0.0.0:4317").is_err());
        assert!(validate_endpoint("http://evil.com:4317").is_err());
        assert!(validate_endpoint("http://example.com").is_err());
    }

    #[test]
    fn validate_endpoint_rejects_userinfo_injection() {
        // 防 SSRF：http://127.0.0.1@evil.com 会被 url crate 解析为 host=evil.com
        // 但有 userinfo 就直接拒绝，双重防御
        let err = validate_endpoint("http://127.0.0.1@evil.com:4317").unwrap_err();
        assert!(err.message.contains("userinfo"), "got: {}", err);
    }

    #[test]
    fn validate_endpoint_rejects_path() {
        let err = validate_endpoint("http://127.0.0.1:4317/api/pins").unwrap_err();
        assert!(err.message.contains("path"), "got: {}", err);
    }

    #[test]
    fn validate_endpoint_rejects_query() {
        let err = validate_endpoint("http://127.0.0.1:4317?x=1").unwrap_err();
        assert!(err.message.contains("query"), "got: {}", err);
    }

    #[test]
    fn validate_endpoint_rejects_fragment() {
        let err = validate_endpoint("http://127.0.0.1:4317#frag").unwrap_err();
        assert!(err.message.contains("fragment"), "got: {}", err);
    }

    #[test]
    fn validate_endpoint_rejects_invalid_url() {
        assert!(validate_endpoint("not a url").is_err());
        assert!(validate_endpoint("http://").is_err());
        assert!(validate_endpoint("://no-scheme").is_err());
    }

    #[test]
    fn validate_endpoint_accepts_uppercase_scheme() {
        // url crate 把 scheme 规范化为小写，validate_endpoint 的 scheme 比较不漏大小写
        // 固化该行为：用户输入大写 HTTP 也能用
        assert!(validate_endpoint("HTTP://127.0.0.1:4317").is_ok());
        assert!(validate_endpoint("Http://localhost:4317").is_ok());
    }

    #[test]
    fn validate_endpoint_decimal_ip_normalized_to_loopback() {
        // 经典 SSRF 向量：十进制 IP 2130706433 = 127.0.0.1
        // url crate 会规范化为 host_str="127.0.0.1"，被白名单接受
        // 固化该行为：若未来 url crate 升级改变规范化逻辑，测试会捕获回归
        // （无论规范化后接受还是不规范化时拒绝，都不会导致 SSRF，但行为必须稳定）
        let result = validate_endpoint("http://2130706433:4317");
        assert!(
            result.is_ok(),
            "expected url crate to normalize decimal IP to 127.0.0.1, got: {:?}",
            result
        );
    }

    // ---------- parse_error_body ----------

    #[test]
    fn parse_error_body_standard_error_response() {
        let raw = r#"{"ok":false,"error":{"code":"VALIDATION_ERROR","message":"title is empty"}}"#;
        let result = parse_error_body(400, raw);
        assert_eq!(result.code, "VALIDATION_ERROR");
        assert_eq!(result.message, "title is empty (HTTP 400)");
    }

    #[test]
    fn parse_error_body_non_json_response() {
        let raw = "Internal Server Error";
        let result = parse_error_body(500, raw);
        // 非 JSON：error_code 回退为 UNKNOWN，展示原始内容
        assert_eq!(result.code, "UNKNOWN");
        assert!(result.message.contains("HTTP 500"), "got: {}", result);
        assert!(
            result.message.contains("Internal Server Error"),
            "got: {}",
            result
        );
    }

    #[test]
    fn parse_error_body_missing_message_field() {
        // 只有 code 没有 message：error_code 取到 "INTERNAL"，走 truncation 分支
        let raw = r#"{"ok":false,"error":{"code":"INTERNAL"}}"#;
        let result = parse_error_body(500, raw);
        assert_eq!(result.code, "INTERNAL");
        assert!(result.message.contains("HTTP 500"), "got: {}", result);
        // 缺 message 时展示原始 raw 内容
        assert!(result.message.contains("原始响应:"), "got: {}", result);
    }

    #[test]
    fn parse_error_body_missing_error_object() {
        // 完全没有 error 字段
        let raw = r#"{"ok":false}"#;
        let result = parse_error_body(422, raw);
        assert_eq!(result.code, "UNKNOWN");
        assert!(result.message.contains("HTTP 422"), "got: {}", result);
    }

    #[test]
    fn parse_error_body_empty_raw() {
        let result = parse_error_body(502, "");
        assert_eq!(result.code, "UNKNOWN");
        assert!(result.message.contains("HTTP 502"), "got: {}", result);
    }

    #[test]
    fn parse_error_body_truncates_long_raw() {
        // 超过 500 字节的非 JSON 内容必须截断，避免日志爆炸
        let raw = "X".repeat(1000);
        let result = parse_error_body(500, &raw);
        assert!(result.message.contains("HTTP 500"), "got: {}", result);
        // 截断后不应包含完整的 1000 个 X
        assert!(
            !result.message.contains(&"X".repeat(600)),
            "raw content not truncated"
        );
    }

    #[test]
    fn parse_error_body_truncation_respects_char_boundary() {
        // 多字节字符（中文）在 500 字节边界截断时不能产生 panic 或乱码
        // 每个中文字符 3 字节 UTF-8，构造长度接近 500 的中文串
        let chinese = "中".repeat(200); // 600 字节
        let result = parse_error_body(500, &chinese);
        // 不 panic 即通过；且仍包含 HTTP 标记
        assert!(result.message.contains("HTTP 500"), "got: {}", result);
    }
}
