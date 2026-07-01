// agent-pin CLI 入口
//
// Phase 2-C：Rust CLI，Agent 的优先入口。
// 底层调用本地 HTTP API（http://127.0.0.1:4317），不复制业务逻辑。
//
// 命令（docs/cli.md + docs/06_phase_plan.md Phase 2-C）：
//   agent-pin health                                              检查桌面应用是否运行
//   agent-pin markdown --title "..." --file ./review.md           从文件创建 Markdown Pin
//   agent-pin markdown --title "..." --text "..."                从文本创建 Markdown Pin
//   agent-pin image --title "..." --path ./img.png --caption ...  创建 Image Pin（相对路径转绝对）
//   agent-pin status --title "..." --level success --text "..."   创建 Status Pin
//   agent-pin push --file ./pin.json                             推送完整 Pin JSON（mixed，路径转绝对）
//   agent-pin list                                               列出最近 Pin
//   agent-pin show <pinId>                                       重新显示已隐藏 Pin
//   agent-pin hide-all                                           隐藏全部可见 Pin
//
// 类型复用：PinDocument 等类型来自 packages/shared（agent-pin-shared），
// 与 desktop 后端共用同一套契约，避免类型漂移。
// push 命令用 serde_json::Value 操作，不强制完整类型，JSON 结构由用户负责（desktop 校验）。

mod client;

use std::path::Path;

use agent_pin_shared::{
    validate, ImageBlock, MarkdownBlock, PinBlock, PinDocument, PinHeight, PinSource,
    PinWindowConfig, StatusBlock,
};
use clap::{Parser, Subcommand};
use serde_json::Value;

use client::Client;

// ---------- CLI 定义 ----------

#[derive(Parser)]
#[command(
    name = "agent-pin",
    version,
    about = "Push important content to desktop pins"
)]
struct Cli {
    /// 覆盖默认 endpoint（默认 http://127.0.0.1:4317）
    #[arg(long, env = "AGENT_PIN_ENDPOINT", global = true)]
    endpoint: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 检查桌面应用是否运行
    Health,
    /// 创建 Markdown Pin
    Markdown(MarkdownArgs),
    /// 创建 Image Pin（相对路径会转绝对）
    Image(ImageArgs),
    /// 创建 Status Pin
    Status(StatusArgs),
    /// 推送完整 Pin JSON（用于 mixed 内容，image 相对路径会转绝对）
    Push(PushArgs),
    /// 列出最近 Pin
    List,
    /// 重新显示已隐藏 Pin
    Show { pin_id: String },
    /// 隐藏全部可见 Pin
    HideAll,
}

// ---------- 命令参数 ----------

/// 公共窗口/来源参数，markdown/image/status 共用。
/// 用 clap flatten 避免重复定义。
#[derive(clap::Args)]
struct CommonArgs {
    /// 窗口宽度
    #[arg(long)]
    width: Option<u32>,
    /// 窗口高度（数字或 "auto"）
    #[arg(long)]
    height: Option<String>,
    /// 关闭窗口置顶（默认置顶）
    #[arg(long = "no-always-on-top")]
    no_always_on_top: bool,
    /// 来源 Agent 名称
    #[arg(long)]
    agent: Option<String>,
    /// 来源 workspace
    #[arg(long)]
    workspace: Option<String>,
    /// 来源 task
    #[arg(long)]
    task: Option<String>,
}

#[derive(clap::Args)]
struct MarkdownArgs {
    /// Pin 标题
    #[arg(long)]
    title: String,
    /// 从文件读取 Markdown 内容
    #[arg(long)]
    file: Option<String>,
    /// 直接指定 Markdown 文本内容
    #[arg(long)]
    text: Option<String>,
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(clap::Args)]
struct ImageArgs {
    /// Pin 标题
    #[arg(long)]
    title: String,
    /// 图片路径（相对路径会转绝对）
    #[arg(long)]
    path: String,
    /// 图片说明（可选）
    #[arg(long)]
    caption: Option<String>,
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(clap::Args)]
struct StatusArgs {
    /// Pin 标题
    #[arg(long)]
    title: String,
    /// 状态级别：info / success / warning / error
    #[arg(long)]
    level: Option<String>,
    /// 状态文本
    #[arg(long)]
    text: String,
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(clap::Args)]
struct PushArgs {
    /// 完整 Pin JSON 文件路径
    #[arg(long)]
    file: String,
}

// ---------- PinDocument CLI 类型 ----------
// 类型定义复用 packages/shared（agent_pin_shared），避免与 desktop 类型漂移。
// StatusBlock 未在此处 import：cmd_status 直接构造 shared::PinBlock::Status，
// 字段名与 shared 一致。

// ---------- main ----------

fn main() {
    let cli = Cli::parse();
    let client = match Client::new(cli.endpoint) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // health 命令的"未运行"是用户可见状态，输出到 stdout；
    // 其他命令的"未运行"是程序错误，输出到 stderr。
    let is_health = matches!(cli.command, Commands::Health);
    if let Err(e) = run_command(&client, cli.command) {
        if e.starts_with("__NOT_RUNNING__") {
            if is_health {
                println!("Agent Pin is not running.");
                println!("Please start the desktop app first.");
            } else {
                eprintln!("Agent Pin is not running.");
                eprintln!("Please start the desktop app first.");
            }
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
}

fn run_command(client: &Client, command: Commands) -> Result<(), String> {
    match command {
        Commands::Health => cmd_health(client),
        Commands::Markdown(args) => cmd_markdown(client, args),
        Commands::Image(args) => cmd_image(client, args),
        Commands::Status(args) => cmd_status(client, args),
        Commands::Push(args) => cmd_push(client, args),
        Commands::List => cmd_list(client),
        Commands::Show { pin_id } => cmd_show(client, &pin_id),
        Commands::HideAll => cmd_hide_all(client),
    }
}

// ---------- 命令实现 ----------

fn cmd_health(client: &Client) -> Result<(), String> {
    match client.get("/api/health") {
        Ok(resp) => {
            let version = resp
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            println!("Agent Pin is running.");
            println!("Version: {}", version);
            println!("Endpoint: {}", client.endpoint());
            Ok(())
        }
        // 未运行时返回 Err，main 统一打印提示 + exit(1)。
        // health 的根本语义是检查是否运行，退出码必须反映检查结果。
        Err(e) => Err(e),
    }
}

fn cmd_markdown(client: &Client, args: MarkdownArgs) -> Result<(), String> {
    // file 和 text 二选一
    let content = match (&args.file, &args.text) {
        (Some(f), None) => read_file(f)?,
        (None, Some(t)) => t.clone(),
        (Some(_), Some(_)) => {
            return Err("--file and --text are mutually exclusive".to_string());
        }
        (None, None) => {
            return Err("either --file or --text is required".to_string());
        }
    };

    let doc = build_pin_doc(
        &args.title,
        vec![PinBlock::Markdown(MarkdownBlock { content })],
        &args.common,
    );
    create_pin(client, &doc)
}

fn cmd_image(client: &Client, args: ImageArgs) -> Result<(), String> {
    let abs_path = to_absolute(&args.path)?;
    let doc = build_pin_doc(
        &args.title,
        vec![PinBlock::Image(ImageBlock {
            path: abs_path,
            caption: args.caption,
        })],
        &args.common,
    );
    create_pin(client, &doc)
}

fn cmd_status(client: &Client, args: StatusArgs) -> Result<(), String> {
    // level 校验：desktop 也会校验，但 CLI 提前报错更友好
    if let Some(level) = &args.level {
        if !matches!(level.as_str(), "info" | "success" | "warning" | "error") {
            return Err(format!(
                "invalid --level '{}': must be one of info/success/warning/error",
                level
            ));
        }
    }
    let doc = build_pin_doc(
        &args.title,
        vec![PinBlock::Status(StatusBlock {
            level: args.level,
            text: args.text,
        })],
        &args.common,
    );
    create_pin(client, &doc)
}

fn cmd_push(client: &Client, args: PushArgs) -> Result<(), String> {
    let content = read_file(&args.file)?;
    // 解析为 Value，转换 image block 的相对路径为绝对路径，再序列化 POST。
    // 不强制完整类型校验：desktop 后端会校验，CLI 只负责路径转换。
    let mut doc: Value =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse pin JSON: {}", e))?;

    if let Some(blocks) = doc.get_mut("blocks").and_then(|b| b.as_array_mut()) {
        for block in blocks.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) == Some("image") {
                if let Some(path) = block.get("path").and_then(|p| p.as_str()).map(String::from) {
                    let abs = to_absolute(&path)?;
                    block["path"] = Value::String(abs);
                }
            }
        }
    }

    let body =
        serde_json::to_string(&doc).map_err(|e| format!("failed to serialize pin JSON: {}", e))?;
    create_pin_raw(client, &body)
}

fn cmd_list(client: &Client) -> Result<(), String> {
    let resp = client.get("/api/pins")?;
    let pins = resp
        .get("pins")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "invalid response: missing pins array".to_string())?;

    if pins.is_empty() {
        println!("No pins yet.");
        return Ok(());
    }

    // 动态对齐：找最长 pinId 宽度
    let max_id = pins
        .iter()
        .filter_map(|p| p.get("pinId").and_then(|v| v.as_str()))
        .map(|s| s.len())
        .max()
        .unwrap_or(20);

    for pin in pins {
        let pin_id = pin.get("pinId").and_then(|v| v.as_str()).unwrap_or("?");
        let title = pin
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("(no title)");
        let state = pin.get("state").and_then(|v| v.as_str()).unwrap_or("?");
        println!("{:<width$}  {}  {}", pin_id, title, state, width = max_id);
    }
    Ok(())
}

fn cmd_show(client: &Client, pin_id: &str) -> Result<(), String> {
    // M5：校验 pin_id 格式，防 URL 路径截断（#、?、.. 等会破坏路由）
    validate_pin_id(pin_id)?;
    let resp = client.post(&format!("/api/pins/{}/show", pin_id), "")?;
    // 防御性校验：HTTP 2xx 但 body ok!=true 视为错误
    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!("unexpected response: {}", resp));
    }
    println!("Pin {} shown.", pin_id);
    Ok(())
}

fn cmd_hide_all(client: &Client) -> Result<(), String> {
    let resp = client.post("/api/pins/hide-all", "")?;
    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!("unexpected response: {}", resp));
    }
    println!("All visible pins are now hidden.");
    Ok(())
}

// ---------- 辅助函数 ----------

/// 校验 pin_id 格式（M5：防 URL 路径截断；M14：改白名单）。
/// pin_id 由后端 generate_pin_id 生成，格式为 pin_<timestamp>_<6位随机>。
/// 白名单：只允许字母数字、下划线、连字符，避免 #、?、/、.. 等破坏路由。
fn validate_pin_id(pin_id: &str) -> Result<(), String> {
    if pin_id.is_empty() {
        return Err("pin_id must be non-empty".to_string());
    }
    // 白名单：只允许字母数字、下划线、连字符（pin_ 前缀格式 + 未来扩展）
    if !pin_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "invalid pin_id (only alphanumeric, underscore, hyphen allowed): {}",
            pin_id
        ));
    }
    Ok(())
}

/// 组装 PinDocument 并 POST /api/pins。
fn create_pin(client: &Client, doc: &PinDocument) -> Result<(), String> {
    // 提前校验，避免 HTTP 往返后才报错（复用 shared 校验逻辑，不复制业务规则）
    if let Err(e) = validate(doc) {
        return Err(format!("invalid pin document: {}", e.message));
    }
    let body = serde_json::to_string(doc)
        .map_err(|e| format!("failed to serialize pin document: {}", e))?;
    create_pin_raw(client, &body)
}

/// 直接 POST 原始 JSON body（push 命令用）。
fn create_pin_raw(client: &Client, body: &str) -> Result<(), String> {
    let resp = client.post("/api/pins", body)?;
    // 防御性校验：HTTP 2xx 但 body ok!=true 视为错误（与 cmd_show/cmd_hide_all 对齐）
    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!("unexpected response: {}", resp));
    }
    if let Some(pin_id) = resp.get("pinId").and_then(|v| v.as_str()) {
        println!("Pin created: {}", pin_id);
    } else {
        println!("Pin created.");
    }
    Ok(())
}

/// 从 CommonArgs 组装 PinDocument。
fn build_pin_doc(title: &str, blocks: Vec<PinBlock>, common: &CommonArgs) -> PinDocument {
    let window = build_window_config(common);
    let source = build_source(common);
    PinDocument {
        version: 1,
        title: title.to_string(),
        blocks,
        window,
        source,
        // created_at 由 desktop 后端在持久化时填充，CLI 不设置。
        created_at: None,
    }
}

/// 从 CommonArgs 组装窗口配置。无任何窗口参数时返回 None。
fn build_window_config(common: &CommonArgs) -> Option<PinWindowConfig> {
    // height 是数字字符串或 "auto"：数字转 PinHeight::Number，其他转 PinHeight::Auto。
    // desktop 端 validate 会校验 Auto 变体必须恰好是 "auto"。
    let height = common.height.as_ref().map(|h| {
        if let Ok(n) = h.parse::<u32>() {
            PinHeight::Number(n)
        } else {
            PinHeight::Auto(h.clone())
        }
    });
    let always_on_top = if common.no_always_on_top {
        Some(false)
    } else {
        None
    };

    if common.width.is_none() && height.is_none() && always_on_top.is_none() {
        return None;
    }

    Some(PinWindowConfig {
        width: common.width,
        height,
        // CLI 不暴露 x/y 定位参数，由 desktop 级联排列。
        x: None,
        y: None,
        always_on_top,
    })
}

/// 从 CommonArgs 组装来源信息。无任何来源参数时返回 None。
fn build_source(common: &CommonArgs) -> Option<PinSource> {
    if common.agent.is_none() && common.workspace.is_none() && common.task.is_none() {
        return None;
    }
    Some(PinSource {
        agent: common.agent.clone(),
        workspace: common.workspace.clone(),
        task: common.task.clone(),
        // CLI 不暴露 conversationId 参数，留给直接构造 JSON 的 push 命令。
        conversation_id: None,
    })
}

/// 文件大小上限：2MB（略大于 HTTP 1MB body 限制，留余量给 JSON 包装）
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 读取文件内容，失败返回友好错误。
/// 读取前先 metadata 检查大小，超过 MAX_FILE_BYTES 报错（防 OOM）。
/// 剥离 UTF-8 BOM（Windows PowerShell 默认带 BOM，会导致 JSON 解析失败）。
fn read_file(path: &str) -> Result<String, String> {
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("failed to read file '{}': {}", path, e))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "file '{}' too large ({} bytes, max {} bytes)",
            path,
            metadata.len(),
            MAX_FILE_BYTES
        ));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read file '{}': {}", path, e))?;
    Ok(content.trim_start_matches('\u{feff}').to_string())
}

/// 相对路径转绝对路径。
/// 不校验文件存在性（与 desktop 一致，前端 <img> onerror 处理）。
/// 绝对路径直接返回，相对路径用 current_dir 拼接。
fn to_absolute(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Ok(path.to_string());
    }
    let cwd =
        std::env::current_dir().map_err(|e| format!("failed to get current directory: {}", e))?;
    let joined = cwd.join(p);
    // Windows 上保留反斜杠分隔符，desktop 端 Path::is_absolute 能识别。
    // 不做分隔符标准化，保持路径原始形态。
    let s = joined
        .to_str()
        .ok_or_else(|| format!("path contains invalid UTF-8: {}", path))?;
    Ok(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    // 辅助：构造临时文件路径（唯一，避免测试间冲突）
    fn temp_file(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("agent-pin-test-{}-{}", std::process::id(), name));
        p
    }

    // 辅助：构造全 None 的 CommonArgs
    fn empty_common() -> CommonArgs {
        CommonArgs {
            width: None,
            height: None,
            no_always_on_top: false,
            agent: None,
            workspace: None,
            task: None,
        }
    }

    // ---------- validate_pin_id ----------

    #[test]
    fn validate_pin_id_rejects_empty() {
        assert!(validate_pin_id("").is_err());
    }

    #[test]
    fn validate_pin_id_accepts_standard_format() {
        assert!(validate_pin_id("pin_1700000000000_abc123").is_ok());
    }

    #[test]
    fn validate_pin_id_accepts_alphanumeric_underscore_hyphen() {
        assert!(validate_pin_id("pin-ABC_123-xyz").is_ok());
        assert!(validate_pin_id("a").is_ok());
        assert!(validate_pin_id("12345").is_ok());
    }

    #[test]
    fn validate_pin_id_rejects_hash() {
        // # 会截断 URL 路径，必须拒绝
        assert!(validate_pin_id("pin_123#frag").is_err());
    }

    #[test]
    fn validate_pin_id_rejects_question() {
        // ? 会截断 URL 路径，必须拒绝
        assert!(validate_pin_id("pin_123?x=1").is_err());
    }

    #[test]
    fn validate_pin_id_rejects_slash() {
        // / 会破坏路由，必须拒绝
        assert!(validate_pin_id("pin_123/abc").is_err());
        assert!(validate_pin_id("pin_123\\abc").is_err());
    }

    #[test]
    fn validate_pin_id_rejects_dot_path_traversal() {
        // .. 路径遍历，必须拒绝
        assert!(validate_pin_id("../etc/passwd").is_err());
        assert!(validate_pin_id("pin_.._abc").is_err());
    }

    #[test]
    fn validate_pin_id_rejects_spaces_and_special() {
        assert!(validate_pin_id("pin 123").is_err());
        assert!(validate_pin_id("pin@123").is_err());
        assert!(validate_pin_id("pin&123").is_err());
    }

    // ---------- to_absolute ----------

    #[test]
    fn to_absolute_returns_absolute_path_unchanged() {
        // Windows 绝对路径
        let win_path = r"C:\foo\bar.png";
        assert_eq!(to_absolute(win_path).unwrap(), win_path);
        let win_path2 = r"C:/foo/bar.png";
        assert_eq!(to_absolute(win_path2).unwrap(), win_path2);
    }

    #[test]
    fn to_absolute_joins_relative_with_cwd() {
        let result = to_absolute("foo.txt").unwrap();
        // 相对路径拼接后应包含原始路径片段
        assert!(
            result.contains("foo.txt"),
            "expected result to contain 'foo.txt', got: {}",
            result
        );
        // 且应该是绝对路径形态
        assert!(
            Path::new(&result).is_absolute(),
            "expected absolute path, got: {}",
            result
        );
    }

    #[test]
    fn to_absolute_handles_subdir_relative() {
        let result = to_absolute("sub/dir/file.png").unwrap();
        assert!(result.contains("file.png"));
        assert!(Path::new(&result).is_absolute());
    }

    // ---------- read_file ----------

    #[test]
    fn read_file_rejects_nonexistent() {
        let err = read_file("/this/path/does/not/exist/anywhere.json").unwrap_err();
        assert!(err.contains("failed to read file"), "got: {}", err);
    }

    #[test]
    fn read_file_strips_utf8_bom() {
        // Windows PowerShell 默认输出带 BOM，必须剥离否则 JSON 解析失败
        let path = temp_file("bom.txt");
        // \u{feff} 是 UTF-8 BOM
        fs::write(&path, "\u{feff}hello world").unwrap();
        let result = read_file(path.to_str().unwrap()).unwrap();
        assert_eq!(result, "hello world");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn read_file_reads_normal_content() {
        let path = temp_file("normal.txt");
        fs::write(&path, "plain content without bom").unwrap();
        let result = read_file(path.to_str().unwrap()).unwrap();
        assert_eq!(result, "plain content without bom");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn read_file_rejects_too_large() {
        // MAX_FILE_BYTES = 2MB，创建刚好超限的文件（防 OOM）
        let path = temp_file("large.txt");
        let over_limit = MAX_FILE_BYTES + 1;
        fs::write(&path, vec![b'X'; over_limit as usize]).unwrap();
        let err = read_file(path.to_str().unwrap()).unwrap_err();
        assert!(err.contains("too large"), "got: {}", err);
        let _ = fs::remove_file(&path);
    }

    // ---------- build_window_config ----------

    #[test]
    fn build_window_config_returns_none_when_no_args() {
        let common = empty_common();
        assert!(build_window_config(&common).is_none());
    }

    #[test]
    fn build_window_config_with_width_only() {
        let mut common = empty_common();
        common.width = Some(500);
        let cfg = build_window_config(&common).expect("expected Some");
        assert_eq!(cfg.width, Some(500));
        assert!(cfg.height.is_none());
        assert!(cfg.always_on_top.is_none());
        assert!(cfg.x.is_none());
        assert!(cfg.y.is_none());
    }

    #[test]
    fn build_window_config_with_no_always_on_top() {
        let mut common = empty_common();
        common.no_always_on_top = true;
        let cfg = build_window_config(&common).expect("expected Some");
        assert_eq!(cfg.always_on_top, Some(false));
    }

    #[test]
    fn build_window_config_height_numeric_string() {
        let mut common = empty_common();
        common.height = Some("100".to_string());
        let cfg = build_window_config(&common).expect("expected Some");
        match cfg.height {
            Some(PinHeight::Number(n)) => assert_eq!(n, 100),
            other => panic!("expected Number(100), got {:?}", other),
        }
    }

    #[test]
    fn build_window_config_height_auto_string() {
        let mut common = empty_common();
        common.height = Some("auto".to_string());
        let cfg = build_window_config(&common).expect("expected Some");
        match cfg.height {
            Some(PinHeight::Auto(s)) => assert_eq!(s, "auto"),
            other => panic!("expected Auto, got {:?}", other),
        }
    }

    // ---------- build_source ----------

    #[test]
    fn build_source_returns_none_when_no_args() {
        let common = empty_common();
        assert!(build_source(&common).is_none());
    }

    #[test]
    fn build_source_with_agent_only() {
        let mut common = empty_common();
        common.agent = Some("claude".to_string());
        let src = build_source(&common).expect("expected Some");
        assert_eq!(src.agent.as_deref(), Some("claude"));
        assert!(src.workspace.is_none());
        assert!(src.task.is_none());
        assert!(src.conversation_id.is_none());
    }

    #[test]
    fn build_source_with_all_fields() {
        let mut common = empty_common();
        common.agent = Some("codex".to_string());
        common.workspace = Some("/repo".to_string());
        common.task = Some("review".to_string());
        let src = build_source(&common).expect("expected Some");
        assert_eq!(src.agent.as_deref(), Some("codex"));
        assert_eq!(src.workspace.as_deref(), Some("/repo"));
        assert_eq!(src.task.as_deref(), Some("review"));
        assert!(src.conversation_id.is_none());
    }

    // ---------- build_pin_doc ----------

    #[test]
    fn build_pin_doc_basic_markdown() {
        let common = empty_common();
        let doc = build_pin_doc(
            "Test Title",
            vec![PinBlock::Markdown(MarkdownBlock {
                content: "## Hello".to_string(),
            })],
            &common,
        );
        assert_eq!(doc.version, 1);
        assert_eq!(doc.title, "Test Title");
        assert_eq!(doc.blocks.len(), 1);
        assert!(doc.window.is_none());
        assert!(doc.source.is_none());
        assert!(doc.created_at.is_none());
    }

    #[test]
    fn build_pin_doc_includes_window_when_args_present() {
        let mut common = empty_common();
        common.width = Some(420);
        let doc = build_pin_doc(
            "T",
            vec![PinBlock::Markdown(MarkdownBlock {
                content: "x".to_string(),
            })],
            &common,
        );
        assert!(doc.window.is_some());
        assert_eq!(doc.window.as_ref().unwrap().width, Some(420));
    }
}
