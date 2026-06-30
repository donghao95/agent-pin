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
    ImageBlock, MarkdownBlock, PinBlock, PinDocument, PinHeight, PinSource, PinWindowConfig,
    StatusBlock,
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
    let client = Client::new(cli.endpoint);

    if let Err(e) = run_command(&client, cli.command) {
        // 连接失败的错误统一格式化为友好提示（输出到 stdout，因为这是用户可见状态而非程序错误）
        if e.starts_with("__NOT_RUNNING__") {
            println!("Agent Pin is not running.");
            println!("Please start the desktop app first.");
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
    let mut doc: Value = serde_json::from_str(&content)
        .map_err(|e| format!("failed to parse pin JSON: {}", e))?;

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

    let body = serde_json::to_string(&doc)
        .map_err(|e| format!("failed to serialize pin JSON: {}", e))?;
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
        let title = pin.get("title").and_then(|v| v.as_str()).unwrap_or("(no title)");
        let state = pin.get("state").and_then(|v| v.as_str()).unwrap_or("?");
        println!("{:<width$}  {}  {}", pin_id, title, state, width = max_id);
    }
    Ok(())
}

fn cmd_show(client: &Client, pin_id: &str) -> Result<(), String> {
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

/// 组装 PinDocument 并 POST /api/pins。
fn create_pin(client: &Client, doc: &PinDocument) -> Result<(), String> {
    let body = serde_json::to_string(doc)
        .map_err(|e| format!("failed to serialize pin document: {}", e))?;
    create_pin_raw(client, &body)
}

/// 直接 POST 原始 JSON body（push 命令用）。
fn create_pin_raw(client: &Client, body: &str) -> Result<(), String> {
    let resp = client.post("/api/pins", body)?;
    if let Some(pin_id) = resp.get("pinId").and_then(|v| v.as_str()) {
        println!("Pin created: {}", pin_id);
    } else {
        // 响应没有 pinId 字段，但 HTTP 成功，打印整个响应
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

/// 读取文件内容，失败返回友好错误。
/// 剥离 UTF-8 BOM（Windows PowerShell 默认带 BOM，会导致 JSON 解析失败）。
fn read_file(path: &str) -> Result<String, String> {
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
    let cwd = std::env::current_dir()
        .map_err(|e| format!("failed to get current directory: {}", e))?;
    let joined = cwd.join(p);
    // Windows 上保留反斜杠分隔符，desktop 端 Path::is_absolute 能识别。
    // 不做分隔符标准化，保持路径原始形态。
    let s = joined
        .to_str()
        .ok_or_else(|| format!("path contains invalid UTF-8: {}", path))?;
    Ok(s.to_string())
}

