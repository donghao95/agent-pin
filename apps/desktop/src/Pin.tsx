import { useEffect, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// 标题栏拖动：Tauri 2 在 Windows 上 data-tauri-drag-region 属性不可靠，
// 改用 onMouseDown + startDragging() 手动触发，最稳定。
// 注意：startDragging 是 async，必须 catch，否则失败时静默无反馈（违反 AGENTS.md "不要静默吞错"）。
// 未来如需"双击标题栏最大化"，需在此处加双击检测，避免与 startDragging 冲突。
const handleTitlebarMouseDown = (e: React.MouseEvent) => {
  // 只响应左键
  if (e.button !== 0) return;
  // 点在关闭按钮上不拖动
  if ((e.target as HTMLElement).closest(".pin-close")) return;
  getCurrentWindow()
    .startDragging()
    .catch((err) => {
      console.error("[agent-pin] startDragging failed:", err);
    });
};

// PinDocument 类型，与后端 pin.rs 对齐
type PinDocument = {
  version: number;
  title: string;
  blocks: Array<
    | { type: "markdown"; content: string }
    | { type: "image"; path: string; caption?: string }
    | { type: "status"; level?: string; text: string }
  >;
};

// status block 的 level 类型
type StatusLevel = "info" | "success" | "warning" | "error";

// 类型守卫：后端已校验 level，但防御性白名单校验避免持久化数据被篡改时注入任意 className。
// 同时让 TS 收窄 string → StatusLevel，避免后续 statusIcon(level) 类型错误。
function isStatusLevel(v: unknown): v is StatusLevel {
  return v === "info" || v === "success" || v === "warning" || v === "error";
}

export default function Pin() {
  const [doc, setDoc] = useState<PinDocument | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const pinId = params.get("pinId");
    if (!pinId) {
      setError("missing pinId in URL");
      return;
    }
    invoke<PinDocument | null>("get_pin_document", { pinId })
      .then((d) => {
        if (!d) {
          setError("pin document not found: " + pinId);
        } else {
          setDoc(d);
        }
      })
      .catch((e) => setError(String(e)));
  }, []);

  const handleClose = () => {
    getCurrentWindow()
      .close()
      .catch((err) => console.error("[agent-pin] close failed:", err));
  };

  // 错误态
  if (error) {
    return (
      <div className="pin-root">
        <div className="pin-titlebar" onMouseDown={handleTitlebarMouseDown}>
          <span className="pin-title">错误</span>
          <button
            className="pin-close"
            onClick={handleClose}
            title="关闭"
          >
            ×
          </button>
        </div>
        <div className="pin-body">
          <div className="pin-error">{error}</div>
        </div>
      </div>
    );
  }

  // 加载态
  if (!doc) {
    return (
      <div className="pin-root">
        <div className="pin-titlebar" onMouseDown={handleTitlebarMouseDown}>
          <span className="pin-title">加载中…</span>
          <button
            className="pin-close"
            onClick={handleClose}
            title="关闭"
          >
            ×
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="pin-root">
      <div className="pin-titlebar" onMouseDown={handleTitlebarMouseDown}>
        <span className="pin-title" title={doc.title}>
          {doc.title}
        </span>
        <button
          className="pin-close"
          onClick={handleClose}
          title="关闭"
        >
          ×
        </button>
      </div>
      <div className="pin-body">
        {doc.blocks.map((block, i) => {
          if (block.type === "markdown") {
            return (
              <div className="pin-block pin-markdown" key={i}>
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {block.content}
                </ReactMarkdown>
              </div>
            );
          }
          if (block.type === "image") {
            return (
              <ImageBlock key={i} path={block.path} caption={block.caption} />
            );
          }
          if (block.type === "status") {
            const level = isStatusLevel(block.level) ? block.level : "info";
            return (
              <div className={`pin-block pin-status pin-status-${level}`} key={i}>
                <span className="pin-status-icon">{statusIcon(level)}</span>
                <span className="pin-status-text">{block.text}</span>
              </div>
            );
          }
          // 未知 block 类型兜底，不静默吞错。
          // 经前面三个 if 收窄后 TS 认为 block 是 never，但运行时可能传来未知 type 的对象，
          // 用 as 转换访问 type 字段，避免 TS 报错且保留运行时兜底能力。
          const unknown = block as { type: string };
          return (
            <div className="pin-block pin-unsupported" key={i}>
              unsupported block type: {unknown.type}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// status level 对应的图标（用文字符号，避免引入图标库）
function statusIcon(level: StatusLevel): string {
  switch (level) {
    case "success":
      return "✓";
    case "warning":
      return "!";
    case "error":
      return "✕";
    case "info":
    default:
      return "i";
  }
}

// ImageBlock：用 convertFileSrc 把绝对路径转 asset:// URL，由 Tauri asset protocol 加载。
// 图片不存在/加载失败时显示错误块，不崩溃（docs/mvp-spec.md §9）。
function ImageBlock({ path, caption }: { path: string; caption?: string }) {
  const [failed, setFailed] = useState(false);
  const src = convertFileSrc(path);

  if (failed) {
    return (
      <div className="pin-block pin-image-error">
        <div className="pin-image-error-title">图片加载失败</div>
        <div className="pin-image-error-path" title={path}>{path}</div>
      </div>
    );
  }

  return (
    <div className="pin-block pin-image">
      <img
        src={src}
        alt={caption ?? ""}
        onError={() => setFailed(true)}
        loading="lazy"
      />
      {caption && <div className="pin-image-caption">{caption}</div>}
    </div>
  );
}
