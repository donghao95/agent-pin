// 更新检查（方案 B：轻量检查 + 手动跳转）
//
// 设计原则（见 AGENTS.md 第一性原理）：
// - 轻量、本地优先、不打扰——只读 GitHub API，不自建更新服务器
// - 无签名、无自动下载安装——用户手动下载安装
// - 24 小时缓存避免重复请求（GitHub API 未认证限流 60 次/小时/IP）
// - 静默失败：网络错误不阻塞应用、不弹错误对话框
//
// 流程：
//   应用启动 → spawn_blocking 异步检查（静默）
//   托盘"检查更新" → 同步检查（弹提示）
//   有新版 → 返回 latest 版本号 + Release URL
//   用户点击 → 用 tauri-plugin-shell 打开浏览器到 Release 页
//
// 缓存：~/.agent-pin/update-cache.json
//   { "lastCheckedAt": "2026-06-30T12:00:00Z", "latestVersion": "0.1.1" }
//   24 小时内不重复请求 GitHub API。
//
// 不变量：不破坏"HTTP 只监听 127.0.0.1"——更新检查是出站请求，不影响本地 HTTP 服务。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::storage;

/// GitHub API 端点：获取最新 release。
/// 未认证限流 60 次/小时/IP，配合 24h 缓存足够。
const GITHUB_RELEASES_API: &str =
    "https://api.github.com/repos/donghao95/agent-pin/releases/latest";

/// 缓存有效期：24 小时（避免频繁请求 GitHub API 触发限流）
const CACHE_TTL_SECS: i64 = 24 * 60 * 60;

/// HTTP 请求超时：10 秒（网络慢时不阻塞应用启动太久）
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// GitHub 仓库名（用于构建 Release URL）
const REPO_URL: &str = "https://github.com/donghao95/agent-pin/releases/latest";

// ---------- 缓存文件结构 ----------

/// 缓存结构：记录上次检查时间和最新版本。
/// 缓存命中（24h 内）时直接返回 latestVersion，不再请求 GitHub API。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCache {
    /// 上次检查时间（RFC3339 / ISO 8601）
    last_checked_at: String,
    /// 上次检查到的最新版本号（不含 v 前缀，如 "0.1.1"）
    latest_version: String,
}

// ---------- GitHub API 响应结构 ----------

/// GitHub releases/latest API 返回的部分字段。
/// 只解析 tag_name，其余字段忽略。
/// html_url 不解析：不信任 GitHub 返回的 URL，统一用硬编码 REPO_URL（防命令注入）。
#[derive(Debug, Deserialize)]
struct GithubRelease {
    /// release tag，如 "v0.1.0"
    tag_name: String,
}

// ---------- 对外结果 ----------

/// 检查结果：用于 invoke 命令返回前端 / 托盘判断是否提示用户。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    /// 是否有新版本
    pub has_update: bool,
    /// 当前版本（不含 v 前缀）
    pub current_version: String,
    /// 最新版本（不含 v 前缀）
    pub latest_version: String,
    /// Release 页面 URL（用于打开浏览器）
    pub release_url: String,
    /// 本次检查是否命中缓存（命中则未实际请求 GitHub）
    pub from_cache: bool,
}

// ---------- 入口函数 ----------

/// 检查更新（同步阻塞，调用方应在 spawn_blocking 中调）。
///
/// force=true 时跳过缓存，强制请求 GitHub API。
///
/// 失败不 panic，返回 Err（调用方决定如何处理：静默忽略或弹提示）。
pub fn check(force: bool) -> Result<UpdateCheckResult, String> {
    let current = env!("CARGO_PKG_VERSION");
    let current_norm = strip_v(current);

    // 1. 尝试读缓存（非 force 时）
    if !force {
        if let Some(cache) = read_cache() {
            if is_cache_fresh(&cache) {
                let latest_norm = strip_v(&cache.latest_version);
                let has_update = compare_version(&current_norm, &latest_norm).is_lt();
                return Ok(UpdateCheckResult {
                    has_update,
                    current_version: current_norm.to_string(),
                    latest_version: latest_norm.to_string(),
                    // 统一用硬编码 REPO_URL，不信任 GitHub 返回的 html_url（防命令注入）
                    release_url: REPO_URL.to_string(),
                    from_cache: true,
                });
            }
        }
    }

    // 2. 请求 GitHub API
    let release = fetch_latest_release()?;
    let latest_tag = release.tag_name;
    let latest_norm = strip_v(&latest_tag).to_string();

    // 3. 写缓存（失败不影响返回结果）
    if let Err(e) = write_cache(&latest_norm) {
        eprintln!("[agent-pin] updater write_cache: {}", e);
    }

    let has_update = compare_version(&current_norm, &latest_norm).is_lt();
    Ok(UpdateCheckResult {
        has_update,
        current_version: current_norm.to_string(),
        latest_version: latest_norm,
        // 统一用硬编码 REPO_URL，不信任 GitHub 返回的 html_url（防命令注入）
        release_url: REPO_URL.to_string(),
        from_cache: false,
    })
}

/// 读缓存中的最新版本号（不发起网络请求）。
/// 用于托盘菜单构建时显示版本提示，不触发请求。
/// 返回 None 表示无缓存或缓存已过期。
pub fn cached_latest_version() -> Option<String> {
    let cache = read_cache()?;
    if is_cache_fresh(&cache) {
        Some(cache.latest_version)
    } else {
        None
    }
}

// ---------- 内部函数 ----------

/// 请求 GitHub API 获取最新 release。
fn fetch_latest_release() -> Result<GithubRelease, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(GITHUB_RELEASES_API)
        // GitHub API 要求 User-Agent，否则 403
        .set("User-Agent", "agent-pin-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("github api request: {}", e))?;
    resp.into_json::<GithubRelease>()
        .map_err(|e| format!("github api parse: {}", e))
}

/// 读缓存文件。文件不存在或解析失败返回 None（不阻塞）。
fn read_cache() -> Option<UpdateCache> {
    let path = cache_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 写缓存文件。失败返回 Err（调用方 eprintln）。
fn write_cache(latest_version: &str) -> Result<(), String> {
    let path = cache_path().ok_or_else(|| "cache path unavailable".to_string())?;
    let cache = UpdateCache {
        last_checked_at: now_rfc3339(),
        latest_version: latest_version.to_string(),
    };
    let json = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
    // 原子写：先 .tmp 再 rename（与 storage.rs 一致）
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// 缓存文件路径：~/.agent-pin/update-cache.json
/// 与 storage.rs 的 data_dir 一致。
fn cache_path() -> Option<std::path::PathBuf> {
    let dir = storage::data_dir();
    Some(std::path::PathBuf::from(dir).join("update-cache.json"))
}

/// 判断缓存是否在有效期内（24h）。
fn is_cache_fresh(cache: &UpdateCache) -> bool {
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&cache.last_checked_at) else {
        return false;
    };
    let now = chrono::Utc::now();
    let elapsed = now.signed_duration_since(parsed);
    elapsed.num_seconds() < CACHE_TTL_SECS
}

/// 当前时间 RFC3339 格式。
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 去掉版本号前的 v 前缀（"v0.1.0" → "0.1.0"，"0.1.0" → "0.1.0"）。
fn strip_v(s: &str) -> &str {
    s.strip_prefix('v').unwrap_or(s)
}

/// 简单版本对比：解析 "X.Y.Z" 为 (major, minor, patch, is_release) 比较。
///
/// 不引入 semver crate（会拉一堆依赖），手写 10 行够用。
/// 语义：主版本号按元组比较；主版本号相等时，正式版（无后缀） > 预发布版（有 -suffix）。
///   例如 0.1.0-beta.1 < 0.1.0 < 0.1.1
/// 元组比较：is_release=true（正式版）排在 is_release=false（预发布）之后。
#[derive(Debug, PartialEq, Eq)]
enum VersionCmp {
    Lt,
    Eq,
    Gt,
}

impl VersionCmp {
    fn is_lt(&self) -> bool {
        matches!(self, VersionCmp::Lt)
    }
}

fn compare_version(a: &str, b: &str) -> VersionCmp {
    let pa = parse_version(a);
    let pb = parse_version(b);
    match pa.cmp(&pb) {
        std::cmp::Ordering::Less => VersionCmp::Lt,
        std::cmp::Ordering::Greater => VersionCmp::Gt,
        std::cmp::Ordering::Equal => VersionCmp::Eq,
    }
}

/// 解析 "X.Y.Z" 或 "X.Y.Z-suffix" 为 (major, minor, patch, is_release)。
/// is_release=true 表示正式版（无 -suffix），false 表示预发布版。
/// 解析失败的部分当 0。
fn parse_version(s: &str) -> (u32, u32, u32, bool) {
    // 分离预发布后缀：无 "-" 即正式版
    let (main, is_release) = match s.split_once('-') {
        Some((m, _)) => (m, false),
        None => (s, true),
    };
    let mut parts = main.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor, patch, is_release)
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_v() {
        assert_eq!(strip_v("v0.1.0"), "0.1.0");
        assert_eq!(strip_v("0.1.0"), "0.1.0");
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.1.0"), (0, 1, 0, true));
        assert_eq!(parse_version("1.2.3"), (1, 2, 3, true));
        // 预发布后缀被识别
        assert_eq!(parse_version("0.1.0-beta.1"), (0, 1, 0, false));
        // 解析失败部分当 0
        assert_eq!(parse_version("0.1"), (0, 1, 0, true));
        assert_eq!(parse_version("0"), (0, 0, 0, true));
    }

    #[test]
    fn test_compare_version() {
        assert_eq!(compare_version("0.1.0", "0.1.1"), VersionCmp::Lt);
        assert_eq!(compare_version("0.1.0", "0.1.0"), VersionCmp::Eq);
        assert_eq!(compare_version("0.2.0", "0.1.9"), VersionCmp::Gt);
        assert_eq!(compare_version("1.0.0", "0.9.9"), VersionCmp::Gt);
        // 预发布 < 正式版（同主版本号）
        assert_eq!(compare_version("0.1.0-beta.1", "0.1.0"), VersionCmp::Lt);
        // 正式版 > 预发布（同主版本号）
        assert_eq!(compare_version("0.1.0", "0.1.0-beta.1"), VersionCmp::Gt);
        // 同为预发布，主版本号相同
        assert_eq!(
            compare_version("0.1.0-beta.1", "0.1.0-beta.1"),
            VersionCmp::Eq
        );
    }
}
