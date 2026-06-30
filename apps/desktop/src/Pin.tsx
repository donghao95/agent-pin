import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
          // Phase 1 只支持 markdown；image/status 由 Phase 2 补齐
          return (
            <div className="pin-block pin-unsupported" key={i}>
              unsupported block type: {block.type}
            </div>
          );
        })}
      </div>
    </div>
  );
}
