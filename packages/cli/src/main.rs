// agent-pin CLI 入口
//
// Phase 2-C：Rust CLI，Agent 的优先入口。
// 底层调用本地 HTTP API（http://127.0.0.1:4317），不复制业务逻辑。
//
// 命令（docs/04_cli.md + docs/06_phase_plan.md Phase 2-C）：
//   agent-pin health                                              检查桌面应用是否运行
//   agent-pin markdown --title "..." --file ./review.md           从文件创建 Markdown Pin
//   agent-pin markdown --title "..." --text "..."                从文本创建 Markdown Pin
//   agent-pin image --title "..." --path ./img.png --caption ...  创建 Image Pin（复制图片到数据目录）
//   agent-pin status --title "..." --level success --text "..."   创建 Status Pin
//   agent-pin push --file ./pin.json                             推送完整 Pin JSON（mixed，复制图片）
//   agent-pin list                                               列出最近 Pin
//   agent-pin show <pinId>                                       重新显示已隐藏 Pin
//   agent-pin hide <pinId>                                       隐藏指定 Pin
//   agent-pin hide-all                                           隐藏全部可见 Pin
//   agent-pin --json ...                                         输出机器可解析 JSON
//
// Agent-facing 约定：
// - --file - 从 stdin 读取 markdown 或 Pin JSON。
// - image/push 中图片先复制到 ~/.agent-pin/images/，再把副本路径写入 PinDocument。
// - push 中 image 相对路径按 JSON 文件所在目录解析；--file - 时按当前工作目录解析。
// - --json 下成功/失败均输出固定 JSON 结构，便于 Agent 解析 pinId 和错误码。
//
// 类型复用：PinDocument 等类型来自 packages/shared（agent-pin-shared），
// 与 desktop 后端共用同一套契约，避免类型漂移。
// push 命令用 serde_json::Value 操作，不强制完整类型，JSON 结构由用户负责（desktop 校验）。

mod client;
mod error;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_pin_shared::{
    validate, ImageBlock, MarkdownBlock, PinBlock, PinDocument, PinHeight, PinSource,
    PinWindowConfig, StatusBlock,
};
use clap::{ArgAction, ArgGroup, Parser, Subcommand};
use serde_json::{json, Value};

use client::Client;
use error::CliError;

// ---------- CLI 定义 ----------

#[derive(Parser)]
#[command(
    name = "agent-pin",
    version,
    about = "把重要内容推送成桌面 Pin",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
    after_help = "Agent 使用建议：程序调用默认加 --json。短状态用 status，可读摘要用 markdown，单张图片用 image，混合内容用 push。"
)]
struct Cli {
    /// 覆盖默认 endpoint（默认 http://127.0.0.1:4317）
    #[arg(long, env = "AGENT_PIN_ENDPOINT", global = true)]
    endpoint: Option<String>,
    /// 输出机器可解析 JSON（成功和失败都使用固定结构）
    #[arg(long, global = true)]
    json: bool,
    /// 显示帮助
    #[arg(short = 'h', long = "help", action = ArgAction::Help, global = true)]
    help: Option<bool>,
    /// 显示版本
    #[arg(short = 'V', long = "version", action = ArgAction::Version)]
    version: Option<bool>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 检查桌面应用是否运行
    Health,
    /// 创建 Markdown Pin（适合结论、摘要、审查结果）
    Markdown(MarkdownArgs),
    /// 创建 Image Pin（适合图片结果；图片会复制到 ~/.agent-pin/images/）
    Image(ImageArgs),
    /// 创建 Status Pin（适合任务状态、成功、警告、失败）
    Status(StatusArgs),
    /// 推送完整 Pin JSON（用于 mixed 内容；图片会复制到 ~/.agent-pin/images/）
    Push(PushArgs),
    /// 列出最近 Pin
    List,
    /// 重新显示已隐藏 Pin
    Show { pin_id: String },
    /// 隐藏一个可见 Pin（记录保留，可重新 show）
    Hide { pin_id: String },
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
#[command(group(
    ArgGroup::new("markdown_input")
        .required(true)
        .multiple(false)
        .args(["file", "text"])
))]
#[command(
    after_help = "示例：\n  agent-pin --json markdown --title \"审查结果\" --file ./review.md\n  Get-Content -Encoding UTF8 .\\review.md | agent-pin --json markdown --title \"审查结果\" --file -"
)]
struct MarkdownArgs {
    /// Pin 标题
    #[arg(long)]
    title: String,
    /// 从文件读取 Markdown 内容；使用 "-" 从 stdin 读取
    #[arg(long)]
    file: Option<String>,
    /// 直接指定 Markdown 文本内容
    #[arg(long)]
    text: Option<String>,
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(clap::Args)]
#[command(
    after_help = "示例：\n  agent-pin --json image --title \"效果图\" --path ./render.png --caption \"最终效果\""
)]
struct ImageArgs {
    /// Pin 标题
    #[arg(long)]
    title: String,
    /// 图片路径（会复制到 ~/.agent-pin/images/）
    #[arg(long)]
    path: String,
    /// 图片说明（可选）
    #[arg(long)]
    caption: Option<String>,
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(clap::Args)]
#[command(
    after_help = "示例：\n  agent-pin --json status --title \"构建\" --level success --text \"所有检查通过\"\n  agent-pin --json status --title \"构建\" --level error --text \"测试失败\""
)]
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
#[command(
    after_help = "示例：\n  agent-pin --json push --file ./pin.json\n  Get-Content -Encoding UTF8 .\\pin.json | agent-pin --json push --file -\n\nJSON 文件中的相对图片路径按该 JSON 文件所在目录解析；使用 --file - 时按当前工作目录解析。"
)]
struct PushArgs {
    /// 完整 Pin JSON 文件路径；使用 "-" 从 stdin 读取
    #[arg(long)]
    file: String,
}

#[derive(Clone, Copy)]
struct OutputMode {
    json: bool,
}

// ---------- PinDocument CLI 类型 ----------
// 类型定义复用 packages/shared（agent_pin_shared），避免与 desktop 类型漂移。
// StatusBlock 未在此处 import：cmd_status 直接构造 shared::PinBlock::Status，
// 字段名与 shared 一致。

// ---------- main ----------

fn main() {
    let json_requested = json_flag_requested(std::env::args_os());
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            if json_requested {
                let output = OutputMode { json: true };
                emit_error(
                    output,
                    &CliError::new("CLI_USAGE_ERROR", e.to_string()),
                    false,
                );
                std::process::exit(e.exit_code());
            }
            e.exit();
        }
    };
    let output = OutputMode { json: cli.json };
    let client = match Client::new(cli.endpoint) {
        Ok(c) => c,
        Err(e) => {
            emit_error(output, &e, false);
            std::process::exit(1);
        }
    };

    let is_health = matches!(cli.command, Commands::Health);
    if let Err(e) = run_command(&client, cli.command, output) {
        emit_error(output, &e, is_health);
        std::process::exit(1);
    }
}

fn run_command(client: &Client, command: Commands, output: OutputMode) -> Result<(), CliError> {
    match command {
        Commands::Health => cmd_health(client, output),
        Commands::Markdown(args) => cmd_markdown(client, args, output),
        Commands::Image(args) => cmd_image(client, args, output),
        Commands::Status(args) => cmd_status(client, args, output),
        Commands::Push(args) => cmd_push(client, args, output),
        Commands::List => cmd_list(client, output),
        Commands::Show { pin_id } => cmd_show(client, &pin_id, output),
        Commands::Hide { pin_id } => cmd_hide(client, &pin_id, output),
        Commands::HideAll => cmd_hide_all(client, output),
    }
}

// ---------- 命令实现 ----------

fn cmd_health(client: &Client, output: OutputMode) -> Result<(), CliError> {
    let resp = client.get("/api/health")?;
    let version = resp
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    if output.json {
        print_json(&json!({
            "ok": true,
            "running": true,
            "version": version,
            "endpoint": client.endpoint()
        }));
    } else {
        println!("Agent Pin 正在运行。");
        println!("版本: {}", version);
        println!("Endpoint: {}", client.endpoint());
    }
    Ok(())
}

fn cmd_markdown(client: &Client, args: MarkdownArgs, output: OutputMode) -> Result<(), CliError> {
    let content = match (&args.file, &args.text) {
        (Some(f), None) => read_file(f)?,
        (None, Some(t)) => t.clone(),
        (Some(_), Some(_)) => unreachable!("clap ArgGroup enforces mutual exclusion"),
        (None, None) => unreachable!("clap ArgGroup requires one input"),
    };

    let doc = build_pin_doc(
        &args.title,
        vec![PinBlock::Markdown(MarkdownBlock { content })],
        &args.common,
    );
    create_pin(client, &doc, output)
}

fn cmd_image(client: &Client, args: ImageArgs, output: OutputMode) -> Result<(), CliError> {
    let mut copies = ImageCopyTracker::default();
    let cwd = current_dir()?;
    let stored_path = copies.copy_image(&args.path, &cwd)?;
    let doc = build_pin_doc(
        &args.title,
        vec![PinBlock::Image(ImageBlock {
            path: stored_path,
            caption: args.caption,
        })],
        &args.common,
    );
    match create_pin(client, &doc, output) {
        Ok(()) => Ok(()),
        Err(e) => {
            copies.cleanup();
            Err(e)
        }
    }
}

fn cmd_status(client: &Client, args: StatusArgs, output: OutputMode) -> Result<(), CliError> {
    if let Some(level) = &args.level {
        if !matches!(level.as_str(), "info" | "success" | "warning" | "error") {
            return Err(CliError::invalid_input(format!(
                "非法 --level '{}'；必须是 info、success、warning、error 之一",
                level
            )));
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
    create_pin(client, &doc, output)
}

fn cmd_push(client: &Client, args: PushArgs, output: OutputMode) -> Result<(), CliError> {
    let mut copies = ImageCopyTracker::default();
    let content = read_file(&args.file)?;
    let base_dir = input_base_dir(&args.file)?;
    // 解析为 Value，复制 image block 的源图片并改写为托管副本路径，再序列化 POST。
    // 不强制完整类型校验：desktop 后端会校验，CLI 只负责图片托管。
    let mut doc: Value = serde_json::from_str(&content)
        .map_err(|e| CliError::invalid_input(format!("解析 Pin JSON 失败: {}", e)))?;

    if let Some(blocks) = doc.get_mut("blocks").and_then(|b| b.as_array_mut()) {
        for block in blocks.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) == Some("image") {
                if let Some(path) = block.get("path").and_then(|p| p.as_str()).map(String::from) {
                    let stored = match copies.copy_image(&path, &base_dir) {
                        Ok(stored) => stored,
                        Err(e) => {
                            copies.cleanup();
                            return Err(e);
                        }
                    };
                    block["path"] = Value::String(stored);
                }
            }
        }
    }

    let body = match serde_json::to_string(&doc) {
        Ok(body) => body,
        Err(e) => {
            copies.cleanup();
            return Err(CliError::new(
                "SERIALIZE_FAILED",
                format!("序列化 Pin JSON 失败: {}", e),
            ));
        }
    };
    match create_pin_raw(client, &body, output) {
        Ok(()) => Ok(()),
        Err(e) => {
            copies.cleanup();
            Err(e)
        }
    }
}

fn cmd_list(client: &Client, output: OutputMode) -> Result<(), CliError> {
    let resp = client.get("/api/pins")?;
    if output.json {
        print_json(&resp);
        return Ok(());
    }
    let pins = resp
        .get("pins")
        .and_then(|p| p.as_array())
        .ok_or_else(|| CliError::new("INVALID_RESPONSE", "missing pins array"))?;

    if pins.is_empty() {
        println!("还没有 Pin。");
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

fn cmd_show(client: &Client, pin_id: &str, output: OutputMode) -> Result<(), CliError> {
    // M5：校验 pin_id 格式，防 URL 路径截断（#、?、.. 等会破坏路由）
    validate_pin_id(pin_id)?;
    let resp = client.post(&format!("/api/pins/{}/show", pin_id), "")?;
    // 防御性校验：HTTP 2xx 但 body ok!=true 视为错误
    ensure_ok_response(&resp)?;
    if output.json {
        print_json(&json!({"ok": true, "pinId": pin_id, "state": "visible"}));
    } else {
        println!("Pin {} 已显示。", pin_id);
    }
    Ok(())
}

fn cmd_hide(client: &Client, pin_id: &str, output: OutputMode) -> Result<(), CliError> {
    validate_pin_id(pin_id)?;
    let resp = client.post(&format!("/api/pins/{}/hide", pin_id), "")?;
    ensure_ok_response(&resp)?;
    if output.json {
        print_json(&json!({"ok": true, "pinId": pin_id, "state": "hidden"}));
    } else {
        println!("Pin {} 已隐藏。", pin_id);
    }
    Ok(())
}

fn cmd_hide_all(client: &Client, output: OutputMode) -> Result<(), CliError> {
    let resp = client.post("/api/pins/hide-all", "")?;
    ensure_ok_response(&resp)?;
    if output.json {
        print_json(&json!({"ok": true}));
    } else {
        println!("所有可见 Pin 已隐藏。");
    }
    Ok(())
}

// ---------- 辅助函数 ----------

static IMAGE_COPY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 校验 pin_id 格式（M5：防 URL 路径截断；M14：改白名单）。
/// pin_id 由后端 generate_pin_id 生成，格式为 pin_<timestamp>_<6位随机>。
/// 白名单：只允许字母数字、下划线、连字符，避免 #、?、/、.. 等破坏路由。
fn validate_pin_id(pin_id: &str) -> Result<(), CliError> {
    if pin_id.is_empty() {
        return Err(CliError::invalid_input("pin_id 不能为空"));
    }
    // 长度上限与 desktop storage::validate_pin_id 对齐，防 HashMap 内存膨胀
    if pin_id.len() > 128 {
        return Err(CliError::invalid_input(format!(
            "pin_id 过长（最大 128 字符）: {} 字符",
            pin_id.len()
        )));
    }
    // 保留字与 desktop 对齐：防 Tauri window label 滥用
    if pin_id == "manager" {
        return Err(CliError::invalid_input(
            "pin_id 'manager' 是保留字，不能使用",
        ));
    }
    // 白名单：只允许字母数字、下划线、连字符（pin_ 前缀格式 + 未来扩展）
    if !pin_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(CliError::invalid_input(format!(
            "非法 pin_id（只允许字母、数字、下划线和连字符）: {}",
            pin_id
        )));
    }
    Ok(())
}

/// 组装 PinDocument 并 POST /api/pins。
fn create_pin(client: &Client, doc: &PinDocument, output: OutputMode) -> Result<(), CliError> {
    // 提前校验，避免 HTTP 往返后才报错（复用 shared 校验逻辑，不复制业务规则）
    if let Err(e) = validate(doc) {
        return Err(CliError::new(
            pin_error_code(&e.code),
            format!("非法 PinDocument: {}", e.message),
        ));
    }
    let body = serde_json::to_string(doc).map_err(|e| {
        CliError::new(
            "SERIALIZE_FAILED",
            format!("序列化 PinDocument 失败: {}", e),
        )
    })?;
    create_pin_raw(client, &body, output)
}

/// 直接 POST 原始 JSON body（push 命令用）。
fn create_pin_raw(client: &Client, body: &str, output: OutputMode) -> Result<(), CliError> {
    let resp = client.post("/api/pins", body)?;
    // 防御性校验：HTTP 2xx 但 body ok!=true 视为错误（与 cmd_show/cmd_hide_all 对齐）
    ensure_ok_response(&resp)?;
    if let Some(pin_id) = resp.get("pinId").and_then(|v| v.as_str()) {
        if output.json {
            print_json(&json!({"ok": true, "pinId": pin_id}));
        } else {
            println!("Pin 已创建: {}", pin_id);
        }
    } else if output.json {
        print_json(&json!({"ok": true}));
    } else {
        println!("Pin 已创建。");
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
/// 用 File + take(MAX_FILE_BYTES+1) 读取，避免 metadata 检查与实际读取之间的 TOCTOU
/// （文件可能在 metadata 后变大，导致读到超限内容到内存）。
/// 剥离 UTF-8 BOM（Windows PowerShell 默认带 BOM，会导致 JSON 解析失败）。
fn read_file(path: &str) -> Result<String, CliError> {
    let limit = MAX_FILE_BYTES as usize + 1;
    if path == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(limit as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| CliError::new("READ_FAILED", format!("读取 stdin 失败: {}", e)))?;
        if bytes.len() > MAX_FILE_BYTES as usize {
            return Err(CliError::invalid_input(format!(
                "stdin 过大（最大 {} 字节）",
                MAX_FILE_BYTES
            )));
        }
        let content = String::from_utf8(bytes)
            .map_err(|e| CliError::invalid_input(format!("stdin 不是有效 UTF-8: {}", e)))?;
        return Ok(content.trim_start_matches('\u{feff}').to_string());
    }

    let file = std::fs::File::open(path)
        .map_err(|e| CliError::new("READ_FAILED", format!("读取文件 '{}' 失败: {}", path, e)))?;
    let mut bytes = Vec::new();
    file.take(limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| CliError::new("READ_FAILED", format!("读取文件 '{}' 失败: {}", path, e)))?;
    if bytes.len() > MAX_FILE_BYTES as usize {
        return Err(CliError::invalid_input(format!(
            "文件 '{}' 过大（最大 {} 字节）",
            path, MAX_FILE_BYTES
        )));
    }
    let content = String::from_utf8(bytes)
        .map_err(|e| CliError::invalid_input(format!("文件 '{}' 不是有效 UTF-8: {}", path, e)))?;
    Ok(content.trim_start_matches('\u{feff}').to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn home_dir_required() -> Result<PathBuf, CliError> {
    home_dir().ok_or_else(|| {
        CliError::new(
            "HOME_NOT_FOUND",
            "无法确定用户 home 目录（USERPROFILE/HOME 均未设置），无法托管图片",
        )
    })
}

fn data_dir_for(home: &Path) -> PathBuf {
    home.join(".agent-pin")
}

fn images_dir_for(home: &Path) -> PathBuf {
    data_dir_for(home).join("images")
}

fn current_dir() -> Result<PathBuf, CliError> {
    std::env::current_dir()
        .map_err(|e| CliError::new("CWD_FAILED", format!("获取当前工作目录失败: {}", e)))
}

#[derive(Default)]
struct ImageCopyTracker {
    copied: Vec<PathBuf>,
}

impl ImageCopyTracker {
    fn copy_image(&mut self, path: &str, base_dir: &Path) -> Result<String, CliError> {
        let home = home_dir_required()?;
        self.copy_image_for(path, base_dir, &home)
    }

    fn copy_image_for(
        &mut self,
        path: &str,
        base_dir: &Path,
        home: &Path,
    ) -> Result<String, CliError> {
        let result = copy_image_to_store_for(path, base_dir, home)?;
        if let Some(copied_path) = result.copied_path {
            self.copied.push(copied_path);
        }
        Ok(result.path)
    }

    /// 清理已复制的图片。仅在 Pin 创建失败时调用——成功时图片已交付给 desktop，不能清理。
    fn cleanup(&mut self) {
        for path in self.copied.drain(..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Debug)]
struct ImageCopyResult {
    path: String,
    copied_path: Option<PathBuf>,
}

fn copy_image_to_store_for(
    path: &str,
    base_dir: &Path,
    home: &Path,
) -> Result<ImageCopyResult, CliError> {
    let abs = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base_dir.join(path)
    };

    let metadata = std::fs::metadata(&abs).map_err(|e| {
        CliError::new(
            "IMAGE_NOT_FOUND",
            format!("读取图片 '{}' 失败: {}", abs.display(), e),
        )
    })?;
    if !metadata.is_file() {
        return Err(CliError::new(
            "IMAGE_NOT_FOUND",
            format!("图片路径不是文件: {}", abs.display()),
        ));
    }

    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| {
            CliError::new(
                "IMAGE_UNSUPPORTED",
                "图片路径必须带扩展名：png、jpg、jpeg、webp 或 gif",
            )
        })?;
    if !agent_pin_shared::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(CliError::new(
            "IMAGE_UNSUPPORTED",
            format!(
                "不支持的图片格式 '{}'；支持格式为 {:?}",
                ext,
                agent_pin_shared::IMAGE_EXTENSIONS
            ),
        ));
    }

    let store_dir = images_dir_for(home);
    std::fs::create_dir_all(&store_dir).map_err(|e| {
        CliError::new(
            "WRITE_FAILED",
            format!("创建图片托管目录 '{}' 失败: {}", store_dir.display(), e),
        )
    })?;

    let canonical_src = std::fs::canonicalize(&abs).map_err(|e| {
        CliError::new(
            "IMAGE_NOT_FOUND",
            format!("解析图片路径 '{}' 失败: {}", abs.display(), e),
        )
    })?;
    let canonical_store = std::fs::canonicalize(&store_dir).map_err(|e| {
        CliError::new(
            "WRITE_FAILED",
            format!("解析图片托管目录 '{}' 失败: {}", store_dir.display(), e),
        )
    })?;

    if canonical_src.starts_with(&canonical_store) {
        // 用 store_dir + 相对文件名重新拼接，避免 Windows canonicalize 返回的
        // \\?\ 前缀（VerbatimDisk 路径）写入 PinDocument 后导致前端 convertFileSrc
        // 解析失败。next_image_store_path 分支用的是普通路径，这里保持一致。
        let rel = canonical_src
            .strip_prefix(&canonical_store)
            .map_err(|e| CliError::new("WRITE_FAILED", format!("路径拼接失败: {}", e)))?;
        let path = store_dir.join(rel);
        return Ok(ImageCopyResult {
            path: path_to_string(&path)?,
            copied_path: None,
        });
    }

    let dest = next_image_store_path(&store_dir, &ext);
    let dest_string = path_to_string(&dest)?;
    // copy 失败时可能留下部分写入的 dest 文件，立即清理避免残留。
    // 注意：此处不能依赖 ImageCopyTracker，因为 dest 还未加入 tracker。
    if let Err(e) = std::fs::copy(&canonical_src, &dest) {
        let _ = std::fs::remove_file(&dest);
        return Err(CliError::new(
            "WRITE_FAILED",
            format!(
                "复制图片 '{}' 到 '{}' 失败: {}",
                canonical_src.display(),
                dest.display(),
                e
            ),
        ));
    }
    Ok(ImageCopyResult {
        path: dest_string,
        copied_path: Some(dest),
    })
}

fn next_image_store_path(store_dir: &Path, ext: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let counter = IMAGE_COPY_COUNTER.fetch_add(1, Ordering::Relaxed);
    store_dir.join(format!(
        "image_{}_{}_{}.{}",
        millis,
        std::process::id(),
        counter,
        ext
    ))
}

fn path_to_string(path: &Path) -> Result<String, CliError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| CliError::invalid_input(format!("路径包含无效 UTF-8: {}", path.display())))
}

fn input_base_dir(file: &str) -> Result<PathBuf, CliError> {
    if file == "-" {
        return current_dir();
    }

    let input = Path::new(file);
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        current_dir()?.join(input)
    };

    Ok(absolute
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".")))
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string(value).expect("serializing static JSON output should not fail")
    );
}

fn pin_error_code(code: &agent_pin_shared::PinErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "INVALID_PIN_DOCUMENT".to_string())
}

fn ensure_ok_response(resp: &Value) -> Result<(), CliError> {
    if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(());
    }

    if let Some(error) = resp.get("error") {
        let code = error
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN");
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("响应 ok=false 但缺少 error.message");
        return Err(CliError::new(code, message));
    }

    Err(CliError::new(
        "INVALID_RESPONSE",
        format!("服务端响应格式异常: {}", resp),
    ))
}

fn json_flag_requested<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    for arg in args {
        if arg.as_ref().to_str() == Some("--") {
            return false;
        }
        if arg.as_ref().to_str() == Some("--json") {
            return true;
        }
    }
    false
}

fn emit_error(output: OutputMode, error: &CliError, health_command: bool) {
    if output.json {
        print_json(&json!({
            "ok": false,
            "error": {
                "code": error.code,
                "message": error.message
            }
        }));
        return;
    }

    if health_command && error.code == "NOT_RUNNING" {
        println!("Agent Pin 未运行。");
        println!("请先启动桌面应用。");
        return;
    }

    if error.code == "NOT_RUNNING" {
        eprintln!("Agent Pin 未运行。");
        eprintln!("请先启动桌面应用。");
    } else {
        eprintln!("错误: {}", error);
    }
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

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "agent-pin-test-dir-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    // ---------- clap / output contract ----------

    #[test]
    fn cli_rejects_markdown_without_input() {
        let err = match Cli::try_parse_from(["agent-pin", "--json", "markdown", "--title", "T"]) {
            Ok(_) => panic!("markdown requires --file or --text"),
            Err(err) => err,
        };
        assert_ne!(err.exit_code(), 0);
    }

    #[test]
    fn json_flag_requested_detects_global_json() {
        assert!(json_flag_requested(["agent-pin", "--json", "health"]));
        assert!(json_flag_requested([
            "agent-pin",
            "markdown",
            "--title",
            "T",
            "--json",
            "--text",
            "x"
        ]));
        assert!(!json_flag_requested(["agent-pin", "health"]));
    }

    #[test]
    fn ensure_ok_response_preserves_server_error() {
        let resp = json!({
            "ok": false,
            "error": {
                "code": "INTERNAL_ERROR",
                "message": "some pins could not be hidden: pin_1"
            }
        });
        let err = ensure_ok_response(&resp).unwrap_err();
        assert_eq!(err.code, "INTERNAL_ERROR");
        assert!(err.message.contains("pin_1"));
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

    #[test]
    fn validate_pin_id_rejects_too_long() {
        // 与 desktop storage::validate_pin_id 对齐：上限 128 字符
        let long_id = "a".repeat(129);
        assert!(validate_pin_id(&long_id).is_err());
        let at_limit = "a".repeat(128);
        assert!(validate_pin_id(&at_limit).is_ok());
    }

    #[test]
    fn validate_pin_id_rejects_manager_reserved() {
        // 与 desktop 对齐：防 Tauri window label 滥用
        assert!(validate_pin_id("manager").is_err());
    }

    // ---------- read_file ----------

    #[test]
    fn read_file_rejects_nonexistent() {
        let err = read_file("/this/path/does/not/exist/anywhere.json").unwrap_err();
        assert!(err.message.contains("读取文件"), "got: {}", err);
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
        // 用 File+take 读取，无 TOCTOU：即使文件读取中变大也不会读到超限内容
        let path = temp_file("large.txt");
        let over_limit = MAX_FILE_BYTES + 1;
        fs::write(&path, vec![b'X'; over_limit as usize]).unwrap();
        let err = read_file(path.to_str().unwrap()).unwrap_err();
        assert!(err.message.contains("过大"), "got: {}", err);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn input_base_dir_uses_json_file_directory() {
        let path = if cfg!(windows) {
            r"C:\project\pins\pin.json"
        } else {
            "/project/pins/pin.json"
        };
        let base = input_base_dir(path).unwrap();
        assert!(base.ends_with("pins"));
    }

    #[test]
    fn copy_image_to_store_copies_existing_image() {
        let base = temp_dir("copy-image-base");
        let home = temp_dir("copy-image-home");
        let source = base.join("render.png");
        fs::write(&source, b"fake png bytes").unwrap();

        let stored = copy_image_to_store_for("render.png", &base, &home).unwrap();
        let stored_path = PathBuf::from(stored.path);
        assert!(stored_path.exists());
        assert!(stored_path.starts_with(images_dir_for(&home)));
        assert_eq!(fs::read(&stored_path).unwrap(), b"fake png bytes");
        assert_eq!(stored.copied_path.as_deref(), Some(stored_path.as_path()));

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn image_copy_tracker_cleanup_removes_only_new_copies() {
        let base = temp_dir("tracker-base");
        let home = temp_dir("tracker-home");
        let source = base.join("render.png");
        fs::write(&source, b"fake png bytes").unwrap();

        let mut tracker = ImageCopyTracker::default();
        let stored = tracker.copy_image_for("render.png", &base, &home).unwrap();
        let stored_path = PathBuf::from(&stored);
        assert!(stored_path.exists());

        tracker.cleanup();
        assert!(!stored_path.exists());
        assert!(source.exists(), "cleanup must not remove source image");

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn copy_image_to_store_does_not_track_existing_store_file() {
        let home = temp_dir("existing-store-home");
        let store = images_dir_for(&home);
        fs::create_dir_all(&store).unwrap();
        let existing = store.join("already.png");
        fs::write(&existing, b"existing").unwrap();

        let mut tracker = ImageCopyTracker::default();
        let stored = tracker
            .copy_image_for(existing.to_str().unwrap(), Path::new("."), &home)
            .unwrap();
        // store 内文件返回普通路径（非 canonical），避免 Windows \\?\ 前缀导致前端加载失败
        assert_eq!(PathBuf::from(&stored), existing);
        tracker.cleanup();
        assert!(
            existing.exists(),
            "existing store image must not be cleaned"
        );

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn copy_image_to_store_rejects_missing_image() {
        let base = temp_dir("missing-image-base");
        let home = temp_dir("missing-image-home");
        let err = copy_image_to_store_for("missing.png", &base, &home).unwrap_err();
        assert_eq!(err.code, "IMAGE_NOT_FOUND");
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn copy_image_to_store_rejects_unsupported_extension() {
        let base = temp_dir("bad-image-base");
        let home = temp_dir("bad-image-home");
        let source = base.join("render.bmp");
        fs::write(&source, b"fake bmp bytes").unwrap();
        let err = copy_image_to_store_for("render.bmp", &base, &home).unwrap_err();
        assert_eq!(err.code, "IMAGE_UNSUPPORTED");
        assert!(!images_dir_for(&home).join("render.bmp").exists());
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&home);
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
