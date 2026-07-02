import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { PinDocument, StatusLevel } from "./types";

// ---------- 交互模式 ----------
//
// Pin 窗口有两种交互模式（通过 .pin-root 上的 class 切换）：
//
// view-mode（默认）：
//   - 整个窗口可拖动（onMouseDown → startDragging）
//   - user-select: none（不能选中文字）
//   - 双击 → 切换到 select-mode
//   - 右击 → 弹出上下文菜单（只读模式 / 隐藏 Pin）
//   - 链接点击用系统浏览器打开（invoke open_external_url）
//   - ESC → 关闭窗口（hidden，可恢复）
//
// select-mode（只读选择模式，不修改内容）：
//   - user-select: text（可滑动选择文本复制）
//   - 不触发 startDragging（用户可滑动选择）
//   - 图片可右键复制
//   - 顶部显示提示条"只读模式 · 按 ESC 退出"
//   - ESC → 退回 view-mode（优先于关闭窗口）
//
// ⚠️ select-mode 永不支持修改 Pin 内容。
// 命名用 select 而非 edit，避免误导未来开发者加入"用户编辑保存"逻辑。
// 用户编辑保存会破坏 MVP "Agent→App 纯展示" 不变量，见 docs/roadmap.md。
//
// 拖动实现：Tauri 2 在 Windows 上 data-tauri-drag-region 不可靠，
// 改用 onMouseDown + startDragging() 手动触发，最稳定。
// startDragging 是 async，必须 catch，否则失败时静默无反馈（违反 AGENTS.md "不要静默吞错"）。
//
// 双击与拖动共存：双击时 mousedown 会触发两次 startDragging，但因用户未移动鼠标，
// OS 拖动循环会立即退出，dblclick 仍能正常触发。此行为依赖 Windows 拖动 API，
// 若实测发现抖动可改用延迟启动方案（见 docs/feature/window-adaptive-and-titlebar-redesign.md）。
//
// ---------- 窗口尺寸记忆与自动适配 ----------
//
// 两种尺寸变更来源：
// 1. 程序自动适配（measureAndFit → invoke fit_pin_window_height）：
//    内容变化（doc/image 加载）时后端 set_size 到 scrollHeight。
// 2. 用户手动 resize（拖动窗口边缘）：
//    用户选择的尺寸，需持久化到 PinMeta.window_size，re-open 时恢复。
//
// 区分机制（两个 ref 标志）：
// - programResizing：measureAndFit 调 invoke 前置 true，invoke 返回后延迟 150ms 重置 false。
//   150ms 窗口覆盖 set_size → resize 事件的异步传播，让 onResize 能区分"自己触发的 resize"。
//   若 invoke 返回 false（后端跳过，如已有记忆尺寸），立即重置（无 set_size 不会产生 resize 事件）。
// - userResized：用户手动 resize 后置 true，此后 measureAndFit 不再调 invoke（只更新 overflow 状态）。
//   生命周期：当前窗口会话内有效，关闭后重建窗口时重置为 false（新 DOM 上下文）。
//   后端 fit_pin_window_height 也守卫：PinMeta.window_size 存在时返回 Ok(false)，双重保护。
//
// 关闭路径：前端 invoke close_pin（而非 getCurrentWindow().close()），后端同步 set_state(Hidden)
// + emit pins:changed → 管理页立即刷新，不依赖 Destroyed 事件时序。
type PinMode = "view-mode" | "select-mode";

// 防抖延迟：避免频繁 set_size 抖动（Windows WebView2 重绘延迟）
const FIT_DEBOUNCE_MS = 50;
// 用户手动 resize 后，延迟持久化窗口尺寸（避免拖动过程中频繁写 state.json）
const REMEMBER_DEBOUNCE_MS = 300;
// 程序 resize 后，等待 resize 事件传播的窗口期（覆盖 set_size → event 的异步延迟）
const PROGRAM_RESIZE_RESET_MS = 150;

// ---------- 类型守卫 ----------
// 后端已校验 level，但防御性白名单校验避免持久化数据被篡改时注入任意 className。
// 同时让 TS 收窄 string → StatusLevel，避免后续 statusIcon(level) 类型错误。
function isStatusLevel(v: unknown): v is StatusLevel {
  return v === "info" || v === "success" || v === "warning" || v === "error";
}

export default function Pin() {
  const [doc, setDoc] = useState<PinDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<PinMode>("view-mode");
  // isOverflow: 内容超出可视区（用于决定是否显示底部渐变暗示）
  const [isOverflow, setIsOverflow] = useState(false);
  // isAtBottom: 用户已滚动到底部（到底后移除渐变，让最后一行内容清晰可见）
  const [isAtBottom, setIsAtBottom] = useState(true);
  // 右键上下文菜单位置（null=关闭）。view-mode 右击弹出菜单，select-mode 保留原生菜单。
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  // .pin-body 引用：用于测量内容高度和检测溢出
  const bodyRef = useRef<HTMLDivElement | null>(null);
  // 防抖定时器引用
  const fitTimer = useRef<number | null>(null);
  // pinId 引用：fit_pin_window_height 需要传 pinId
  const pinIdRef = useRef<string | null>(null);
  // 程序正在 set_size（fit_pin_window_height）的标志。
  // measureAndFit 调 invoke 前置 true，invoke 完成后延迟重置 false。
  // onResize 检查此标志区分程序 resize 和用户 resize。
  const programResizing = useRef(false);
  // 用户已手动 resize 的标志。一旦 true，measureAndFit 不再调 invoke（只更新 overflow 状态）。
  // 后端 fit_pin_window_height 也守卫（window_size 存在时跳过），双重保护。
  const userResized = useRef(false);
  // remember_pin_size 的防抖定时器
  const rememberTimer = useRef<number | null>(null);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const pinId = params.get("pinId");
    if (!pinId) {
      setError("missing pinId in URL");
      return;
    }
    pinIdRef.current = pinId;
    // m12：用 ref 标志位防止窗口快速关闭后 setState 无效
    let cancelled = false;
    const timer = window.setTimeout(() => {
      invoke<PinDocument | null>("get_pin_document", { pinId })
        .then((d) => {
          if (cancelled) return;
          if (!d) {
            setError("pin document not found: " + pinId);
          } else {
            setDoc(d);
          }
        })
        .catch((e) => {
          if (cancelled) return;
          setError(String(e));
        });
    }, 0);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      // 清理防抖定时器
      if (fitTimer.current !== null) {
        window.clearTimeout(fitTimer.current);
        fitTimer.current = null;
      }
      if (rememberTimer.current !== null) {
        window.clearTimeout(rememberTimer.current);
        rememberTimer.current = null;
      }
    };
  }, []);

  // ---------- 溢出状态更新 ----------
  // 从 .pin-body 的 scrollHeight/clientHeight/scrollTop 推导 isOverflow 和 isAtBottom。
  // 独立于 measureAndFit，供 resize 事件handler 调用（只需更新状态，不需 fit）。
  const updateOverflowState = useCallback(() => {
    const body = bodyRef.current;
    if (!body) return;
    const overflow = body.scrollHeight > body.clientHeight + 1;
    setIsOverflow(overflow);
    // 不溢出时视为"已在底部"（无需渐变暗示）；溢出时按滚动位置判断
    const atBottom = !overflow || body.scrollTop + body.clientHeight >= body.scrollHeight - 2;
    setIsAtBottom(atBottom);
  }, []);

  // ---------- 自适应高度测量 ----------
  // 测量 .pin-body 的 scrollHeight（包含 padding），通过 invoke 通知后端调整窗口高度。
  // 触发时机：doc 变化、mode 变化、图片加载完成。
  // 防抖 FIT_DEBOUNCE_MS 避免频繁 set_size 抖动。
  //
  // userResized 守卫：用户手动 resize 后不再自动适配（尊重用户选择的尺寸），只更新 overflow 状态。
  // 后端 fit_pin_window_height 也守卫（window_size 存在时跳过），双重保护。
  const measureAndFit = useCallback(() => {
    if (!pinIdRef.current) return;
    const body = bodyRef.current;
    if (!body) return;

    // 先更新 overflow 状态（无论是否 fit 都需要）
    updateOverflowState();

    // 用户已手动 resize：不再自动适配，只保留 overflow 状态更新
    if (userResized.current) return;

    // 清理上一次防抖
    if (fitTimer.current !== null) {
      window.clearTimeout(fitTimer.current);
    }
    // 双重延迟：requestAnimationFrame 等布局稳定，setTimeout 防抖聚合
    fitTimer.current = window.setTimeout(() => {
      fitTimer.current = null;
      const bodyEl = bodyRef.current;
      if (!bodyEl || !pinIdRef.current) return;

      // scrollHeight 包含 padding，是内容的完整高度
      const contentHeight = bodyEl.scrollHeight;
      if (!contentHeight || contentHeight <= 0) return;

      // 标记程序 resize，让 onResize 能区分（set_size 产生的 resize 事件不触发 remember）
      programResizing.current = true;
      invoke<boolean>("fit_pin_window_height", {
        pinId: pinIdRef.current,
        contentHeight,
      })
        .then((adjusted) => {
          if (adjusted) {
            // 后端实际 set_size 了：延迟重置，等 resize 事件传播完毕
            window.setTimeout(() => {
              programResizing.current = false;
            }, PROGRAM_RESIZE_RESET_MS);
          } else {
            // 后端跳过（有记忆尺寸）：无 set_size，无 resize 事件，立即重置
            programResizing.current = false;
          }
        })
        .catch((err) => {
          programResizing.current = false;
          console.error("[agent-pin] fit_pin_window_height failed:", err);
        });
    }, FIT_DEBOUNCE_MS);
  }, [updateOverflowState]);

  // ---------- scroll 监听：更新 isAtBottom ----------
  // 用户滚动到底部时移除渐变暗示，让最后一行内容清晰可见。
  // 用闭包变量避免每次 scroll 都 setState（只在状态变化时更新）。
  const handleScroll = useCallback(() => {
    const body = bodyRef.current;
    if (!body) return;
    // 容差 2px：避免浮点精度导致到底判断失败
    const atBottom = body.scrollTop + body.clientHeight >= body.scrollHeight - 2;
    setIsAtBottom((prev) => (prev !== atBottom ? atBottom : prev));
  }, []);

  // doc 变化或 mode 变化后触发测量。
  // mode 变化时 .pin-mode-hint 出现/消失会挤压 .pin-body 可视高度，需重新检测溢出。
  useEffect(() => {
    if (doc) {
      // 等渲染完成
      requestAnimationFrame(measureAndFit);
    }
  }, [doc, mode, measureAndFit]);

  // ---------- 窗口 resize 监听 ----------
  // 核心设计：区分程序 resize 和用户 resize。
  // - 程序 resize（measureAndFit → fit_pin_window_height → set_size）：
  //   programResizing=true，只更新 overflow 状态，不调 remember。
  // - 用户 resize（拖动窗口边缘）：
  //   programResizing=false，置 userResized=true，调 remember_pin_size（防抖），更新 overflow 状态。
  //
  // 不调 measureAndFit：旧代码在 resize 时调 measureAndFit → fit_pin_window_height → set_size，
  // 覆盖了用户的手动 resize（问题5根因）。现在 resize 事件只更新状态 + 记忆，不反向 fit。
  useEffect(() => {
    const onResize = () => {
      if (programResizing.current) {
        // 程序自己触发的 resize：只更新 overflow 状态
        updateOverflowState();
        return;
      }
      // 用户手动 resize：标记 + 记忆 + 更新状态
      userResized.current = true;
      updateOverflowState();
      // 防抖记忆尺寸（拖动过程中频繁触发，只记最后一次）
      if (rememberTimer.current !== null) {
        window.clearTimeout(rememberTimer.current);
      }
      rememberTimer.current = window.setTimeout(() => {
        rememberTimer.current = null;
        const pinId = pinIdRef.current;
        if (!pinId) return;
        const win = getCurrentWindow();
        Promise.all([win.innerSize(), win.scaleFactor()])
          .then(([physical, scale]) => {
            const logical = physical.toLogical(scale);
            return invoke("remember_pin_size", {
              pinId,
              width: logical.width,
              height: logical.height,
            });
          })
          .catch((err) => {
            console.error("[agent-pin] remember_pin_size failed:", err);
          });
      }, REMEMBER_DEBOUNCE_MS);
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [updateOverflowState]);

  // ---------- 关闭窗口 ----------
  // 用 invoke close_pin 而非 getCurrentWindow().close()：
  // close_pin 同步 set_state(Hidden) + emit pins:changed → 管理页立即刷新，
  // 不依赖 Destroyed 事件时序（Tauri 2 Destroyed 触发时窗口可能未注销，时序不可靠）。
  // useCallback 保证引用稳定，避免 ESC useEffect 频繁重建监听
  const handleClose = useCallback(() => {
    const pinId = pinIdRef.current;
    if (!pinId) {
      // pinId 尚未初始化（极端情况）：fallback 到直接关闭
      getCurrentWindow()
        .close()
        .catch((err) => console.error("[agent-pin] close fallback failed:", err));
      return;
    }
    invoke("close_pin", { pinId }).catch((err) =>
      console.error("[agent-pin] close_pin failed:", err)
    );
  }, []);

  // ---------- 切换模式 ----------
  const enterSelectMode = useCallback(() => setMode("select-mode"), []);
  const exitSelectMode = useCallback(() => setMode("view-mode"), []);

  // ---------- 右上角 × 按钮：根据 mode 分发 ----------
  // select-mode：退出 select-mode（模式嵌套原则：从哪进从哪出，不跳级关闭窗口）
  // view-mode：关闭窗口（hidden，可恢复）
  const handleCloseButton = useCallback(() => {
    if (mode === "select-mode") {
      exitSelectMode();
    } else {
      handleClose();
    }
  }, [mode, exitSelectMode, handleClose]);

  // ---------- ESC 键 ----------
  // select-mode：ESC 退出 select-mode（优先）
  // view-mode：ESC 关闭窗口（hidden，可恢复）
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (mode === "select-mode") {
          e.preventDefault();
          exitSelectMode();
        } else {
          e.preventDefault();
          handleClose();
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [mode, exitSelectMode, handleClose]);

  // ---------- 拖动 ----------
  // view-mode 下：mousedown 左键 → startDragging（排除关闭按钮、链接、上下文菜单）
  // select-mode 下：不拖动（允许文本选择）
  const handleMouseDown = (e: React.MouseEvent) => {
    if (mode !== "view-mode") return;
    if (e.button !== 0) return; // 只响应左键
    const target = e.target as HTMLElement;
    // 点在关闭按钮上不拖动
    if (target.closest(".pin-close-floating")) return;
    // 点在链接上不拖动（允许链接点击）
    if (target.closest("a")) return;
    // 点在上下文菜单上不拖动（否则 startDragging 接管鼠标，菜单项 click 不触发）
    if (target.closest(".pin-context-menu")) return;
    getCurrentWindow()
      .startDragging()
      .catch((err) => {
        console.error("[agent-pin] startDragging failed:", err);
      });
  };

  // ---------- 双击进入 select-mode ----------
  // dblclick 在 click 之后触发，此时 startDragging 已因鼠标未移动而结束。
  // 见文件顶部"双击与拖动共存"注释。
  const handleDoubleClick = (e: React.MouseEvent) => {
    // 双击在关闭按钮上不切换模式
    if ((e.target as HTMLElement).closest(".pin-close-floating")) return;
    enterSelectMode();
  };

  // ---------- 右键上下文菜单 ----------
  // view-mode 下右击：preventDefault 抑制原生网页菜单 + 弹出自定义上下文菜单。
  //   菜单项：只读模式、隐藏 Pin。
  // select-mode 下右击：不 preventDefault，保留原生菜单（复制文本/图片等），
  //   像操作只读文档一样（docs/01_product_spec.md §9.1 承诺"图片可右键复制"）。
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    if (mode === "select-mode") {
      return;
    }
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, [mode]);

  // 菜单项：进入只读模式
  const handleMenuEnterSelect = useCallback(() => {
    enterSelectMode();
    setContextMenu(null);
  }, [enterSelectMode]);

  // 菜单项：隐藏 Pin（关闭窗口，可恢复）
  const handleMenuHide = useCallback(() => {
    handleClose();
    setContextMenu(null);
  }, [handleClose]);

  // 点击菜单外关闭菜单。
  // 依赖 contextMenu：菜单打开时注册监听，关闭时移除。
  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [contextMenu]);

  // ---------- 错误态 ----------
  if (error) {
    return (
      <div
        className="pin-root view-mode"
        onMouseDown={handleMouseDown}
        onContextMenu={handleContextMenu}
      >
        <button
          className="pin-close-floating"
          onClick={handleClose}
          title="关闭"
          aria-label="关闭"
        >
          ×
        </button>
        <div className="pin-body" ref={bodyRef}>
          <div className="pin-title-inline">错误</div>
          <div className="pin-error">{error}</div>
        </div>
      </div>
    );
  }

  // ---------- 加载态 ----------
  if (!doc) {
    return (
      <div
        className="pin-root view-mode"
        onMouseDown={handleMouseDown}
        onContextMenu={handleContextMenu}
      >
        <button
          className="pin-close-floating"
          onClick={handleClose}
          title="关闭"
          aria-label="关闭"
        >
          ×
        </button>
        <div className="pin-body" ref={bodyRef}>
          <div className="pin-title-inline">加载中…</div>
        </div>
      </div>
    );
  }

  // ---------- 正常态 ----------
  // showFade: 内容溢出且未滚动到底部时，显示底部渐变透明暗示
  const showFade = isOverflow && !isAtBottom;
  return (
    <div
      className={`pin-root ${mode} ${showFade ? "show-fade" : ""}`}
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
      onContextMenu={handleContextMenu}
    >
      {/* 右上角 × 按钮：view-mode=关闭窗口，select-mode=退出只读 */}
      <button
        className="pin-close-floating"
        onClick={handleCloseButton}
        title={mode === "select-mode" ? "退出只读模式" : "关闭"}
        aria-label={mode === "select-mode" ? "退出只读模式" : "关闭"}
      >
        ×
      </button>

      {/* select-mode 提示条 */}
      {mode === "select-mode" && (
        <div className="pin-mode-hint">只读模式 · 按 ESC 退出</div>
      )}

      <div className="pin-body" ref={bodyRef} onScroll={handleScroll}>
        {/* title 融入内容顶部 */}
        <div className="pin-title-inline" title={doc.title}>
          {doc.title}
        </div>
        {doc.blocks.map((block, i) => {
          if (block.type === "markdown") {
            return (
              <div className="pin-block pin-markdown" key={i}>
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  components={{
                    // 外链点击用系统浏览器打开。
                    // Tauri 2 WebView 中 <a target="_blank"> 不会自动打开系统浏览器，
                    // 必须拦截 click → invoke open_external_url → opener plugin 打开。
                    // 非 http(s) 链接（如页内锚点 #xxx）不拦截，走浏览器原生行为。
                    a: ({ node: _node, href, ...props }) => (
                      <a
                        {...props}
                        href={href}
                        target="_blank"
                        rel="noopener noreferrer"
                        onClick={(e) => {
                          if (
                            href &&
                            (href.startsWith("http://") ||
                              href.startsWith("https://") ||
                              href.startsWith("mailto:"))
                          ) {
                            e.preventDefault();
                            invoke("open_external_url", { url: href }).catch(
                              (err) => {
                                console.error(
                                  "[agent-pin] open external url failed:",
                                  err
                                );
                              }
                            );
                          }
                        }}
                      />
                    ),
                  }}
                >
                  {block.content}
                </ReactMarkdown>
              </div>
            );
          }
          if (block.type === "image") {
            return (
              <ImageBlock
                key={i}
                path={block.path}
                caption={block.caption}
                onLoaded={measureAndFit}
              />
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

      {/* 右键上下文菜单（view-mode 下右击弹出） */}
      {contextMenu && (
        <div
          className="pin-context-menu"
          style={{
            left: Math.min(contextMenu.x, window.innerWidth - 160),
            top: Math.min(contextMenu.y, window.innerHeight - 90),
          }}
        >
          <button
            className="pin-context-menu-item"
            onClick={handleMenuEnterSelect}
          >
            只读模式
          </button>
          <div className="pin-context-menu-divider" />
          <button className="pin-context-menu-item" onClick={handleMenuHide}>
            隐藏 Pin
          </button>
        </div>
      )}
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
// 图片不存在/加载失败时显示错误块，不崩溃（docs/01_product_spec.md §9）。
// onLoaded：图片加载完成或失败时回调，触发窗口高度重新测量。
function ImageBlock({
  path,
  caption,
  onLoaded,
}: {
  path: string;
  caption?: string;
  onLoaded?: () => void;
}) {
  const [failed, setFailed] = useState(false);
  const src = convertFileSrc(path);

  if (failed) {
    // 加载失败也要触发测量（错误块有固定高度）
    return (
      <div
        className="pin-block pin-image-error"
        ref={(el) => {
          if (el && onLoaded) {
            // 错误块渲染后下一帧触发测量
            requestAnimationFrame(onLoaded);
          }
        }}
      >
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
        onError={() => {
          setFailed(true);
          if (onLoaded) onLoaded();
        }}
        onLoad={() => {
          if (onLoaded) onLoaded();
        }}
        loading="eager"
      />
      {caption && <div className="pin-image-caption">{caption}</div>}
    </div>
  );
}
