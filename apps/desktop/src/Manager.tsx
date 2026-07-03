import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { PinMeta, PinState, ManagerSnapshot, TransientState } from "./types";

export default function Manager() {
  const [pins, setPins] = useState<PinMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState("");
  // M7：用 Map<pinId, TransientState> 支持多 Pin 并发操作锁 + 临时态
  // opening：用户点了"显示"，窗口异步创建中（后端 show_pin AsyncCreate）
  // closing：用户点了"隐藏"
  // deleting：用户点了"删除"
  const [busyByPinId, setBusyByPinId] = useState<Map<string, TransientState>>(
    new Map()
  );
  // M8：隐藏全部操作锁
  const [hidingAll, setHidingAll] = useState(false);
  // request sequence：防止旧 invoke 响应覆盖新响应（并发竞态）
  const requestSeqRef = useRef(0);

  // ---------- applySnapshot ----------
  // 收到后端推来的 snapshot 直接 setPins。
  // 边界（ChatGPT 最终方案）：manager:snapshot 只负责同步 pins 数据，
  // 不负责清 opening 临时态。opening 只由 pin:show-ready / pin:show-failed 结束。
  // 原因：AsyncCreate 期间可能因 focus/mount/别的 Pin 变化收到 snapshot，
  // 此时 registry 可能已是 visible（pre-set），但窗口还没创建完。
  // 若此时清 opening，会回到"列表显示 visible 但窗口没出现"的撒谎问题。
  const applySnapshot = useCallback((snapshot: ManagerSnapshot) => {
    setPins(snapshot.pins);
  }, []);

  // ---------- refreshSnapshot ----------
  // 主动拉取 snapshot（mount/focus/错误恢复时）。
  // 用 requestSeqRef 丢弃过时响应：若发起 A 后又发起 B，A 的响应到达时 seq 已变，丢弃。
  const refreshSnapshot = useCallback(
    async (silent = false) => {
      const seq = ++requestSeqRef.current;
      if (!silent) setLoading(true);
      setError(null);
      try {
        const snapshot = await invoke<ManagerSnapshot>("get_manager_snapshot");
        if (seq !== requestSeqRef.current) return; // 过时响应，丢弃
        applySnapshot(snapshot);
      } catch (e) {
        if (seq === requestSeqRef.current) {
          setError(String(e));
        }
      } finally {
        if (seq === requestSeqRef.current && !silent) {
          setLoading(false);
        }
      }
    },
    [applySnapshot]
  );

  // mount 时拉取一次
  useEffect(() => {
    refreshSnapshot();
  }, [refreshSnapshot]);

  // ---------- 监听 manager:snapshot（推模型，主同步路径）----------
  // 后端状态变化后 emit_to("manager", "manager:snapshot", snapshot)，
  // 前端收到后直接 setPins，不再二次 invoke。
  // 不清 opening：opening 只由 pin:show-ready / pin:show-failed 结束。
  useEffect(() => {
    const unlistenPromise = listen<ManagerSnapshot>("manager:snapshot", (event) => {
      applySnapshot(event.payload);
    });
    return () => {
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, [applySnapshot]);

  // ---------- 监听 pin:show-ready ----------
  // AsyncCreate 模式下窗口创建成功时后端通知，清 opening 临时态。
  // 后端在 emit manager:snapshot 之前 emit pin:show-ready，确保 opening 先清再 setPins。
  useEffect(() => {
    const unlistenPromise = listen<string>("pin:show-ready", (event) => {
      setBusyByPinId((prev) => {
        if (!prev.has(event.payload)) return prev;
        const next = new Map(prev);
        next.delete(event.payload);
        return next;
      });
    });
    return () => {
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // ---------- 监听 pin:show-failed ----------
  // AsyncCreate 模式下窗口创建失败时后端通知，清 opening 临时态 + 显示错误
  useEffect(() => {
    const unlistenPromise = listen<{ pinId: string; message: string }>(
      "pin:show-failed",
      (event) => {
        setError(`显示失败 (${event.payload.pinId}): ${event.payload.message}`);
        // 清 opening 临时态（后端已推 manager:snapshot，state 已是 hidden）
        setBusyByPinId((prev) => {
          const next = new Map(prev);
          next.delete(event.payload.pinId);
          return next;
        });
      }
    );
    return () => {
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // ---------- focus 兜底刷新 ----------
  // Manager 窗口 hide→show 时不 remount，focus 事件作为兜底拉取一次最新 snapshot。
  // 与 open_manager_window 后端推 snapshot 双保险。
  // m2：加 50ms debounce，防止 focus 事件快速连续触发导致多次 invoke。
  useEffect(() => {
    const win = getCurrentWindow();
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unlistenPromise = win.listen("tauri://focus", () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        refreshSnapshot(true);
        timer = null;
      }, 50);
    });
    return () => {
      if (timer) clearTimeout(timer);
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, [refreshSnapshot]);

  // ---------- 操作 handlers ----------

  // show：特殊处理。AsyncCreate 命令只返回 ack，不返回 snapshot。
  // 前端进入 opening 临时态，等后端创建成功后推 manager:snapshot（state=visible）退出 opening。
  // 失败由 pin:show-failed 事件处理。
  const handleShow = async (pinId: string) => {
    setBusyByPinId((prev) => new Map(prev).set(pinId, "opening"));
    try {
      await invoke("show_pin", { pinId });
      // 不 applySnapshot，不清 busy：等后端推 manager:snapshot / pin:show-failed
    } catch (e) {
      setError(`显示失败: ${e}`);
      setBusyByPinId((prev) => {
        const next = new Map(prev);
        next.delete(pinId);
        return next;
      });
    }
  };

  // hide：命令返回 snapshot，直接 apply + 清 busy
  const handleHide = async (pinId: string) => {
    setBusyByPinId((prev) => new Map(prev).set(pinId, "closing"));
    try {
      const snapshot = await invoke<ManagerSnapshot>("hide_pin", { pinId });
      applySnapshot(snapshot);
    } catch (e) {
      setError(`隐藏失败: ${e}`);
      await refreshSnapshot(true);
    } finally {
      setBusyByPinId((prev) => {
        const next = new Map(prev);
        next.delete(pinId);
        return next;
      });
    }
  };

  const handleHideAll = async () => {
    setHidingAll(true);
    try {
      const snapshot = await invoke<ManagerSnapshot>("hide_all_pins");
      applySnapshot(snapshot);
    } catch (e) {
      setError(`隐藏全部失败: ${e}`);
      await refreshSnapshot(true);
    } finally {
      setHidingAll(false);
    }
  };

  const handleDelete = async (pinId: string, title: string) => {
    if (!window.confirm(`确定删除 "${title}" 吗？\n此操作不可恢复。`)) return;
    setBusyByPinId((prev) => new Map(prev).set(pinId, "deleting"));
    try {
      const snapshot = await invoke<ManagerSnapshot>("delete_pin", { pinId });
      applySnapshot(snapshot);
    } catch (e) {
      setError(`删除失败: ${e}`);
      await refreshSnapshot(true);
    } finally {
      setBusyByPinId((prev) => {
        const next = new Map(prev);
        next.delete(pinId);
        return next;
      });
    }
  };

  const handleOpenDataDir = async () => {
    try {
      await invoke("open_data_dir");
    } catch (e) {
      setError(`打开数据目录失败: ${e}`);
    }
  };

  // ---------- 计算展示状态 ----------
  // displayState 优先级：临时态 > registry.state
  // opening 期间即使收到 snapshot（state 可能是 visible），仍显示 opening
  const displayStateOf = useCallback(
    (pin: PinMeta): PinState | TransientState => {
      const transient = busyByPinId.get(pin.pinId);
      if (transient === "opening") return "opening";
      if (transient === "closing") return "closing";
      if (transient === "deleting") return "deleting";
      return pin.state;
    },
    [busyByPinId]
  );

  // 前端过滤：按 title 或 source.agent 匹配关键词
  const filtered = useMemo(() => {
    if (!keyword.trim()) return pins;
    const kw = keyword.trim().toLowerCase();
    return pins.filter((p) => {
      const inTitle = p.title.toLowerCase().includes(kw);
      const inAgent = p.source?.agent?.toLowerCase().includes(kw) ?? false;
      const inWorkspace =
        p.source?.workspace?.toLowerCase().includes(kw) ?? false;
      return inTitle || inAgent || inWorkspace;
    });
  }, [pins, keyword]);

  const { visibleCount, hiddenCount, failedCount } = useMemo(() => {
    let v = 0, h = 0, f = 0;
    for (const p of pins) {
      if (p.state === "visible") v++;
      else if (p.state === "hidden") h++;
      else if (p.state === "failed") f++;
    }
    return { visibleCount: v, hiddenCount: h, failedCount: f };
  }, [pins]);

  const hasBusy = busyByPinId.size > 0;

  return (
    <div className="manager-root">
      <header className="manager-header">
        <div className="manager-title-row">
          <h1 className="manager-title">Agent Pin 管理</h1>
          <div className="manager-stats">
            <span className="manager-stat">共 {pins.length}</span>
            <span className="manager-stat manager-stat-visible">
              可见 {visibleCount}
            </span>
            <span className="manager-stat manager-stat-hidden">
              隐藏 {hiddenCount}
            </span>
            {failedCount > 0 && (
              <span className="manager-stat manager-stat-failed">
                异常 {failedCount}
              </span>
            )}
          </div>
        </div>
        <div className="manager-toolbar">
          <input
            className="manager-search"
            type="search"
            placeholder="搜索标题 / agent / workspace…"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
          <button
            className="manager-btn"
            onClick={() => refreshSnapshot()}
            disabled={loading}
            title="刷新列表"
          >
            {loading ? "刷新中…" : "刷新"}
          </button>
          <button
            className="manager-btn"
            onClick={handleHideAll}
            disabled={hidingAll || visibleCount === 0 || hasBusy}
            title="隐藏所有可见 Pin"
          >
            {hidingAll ? "隐藏中…" : "隐藏全部"}
          </button>
          <button
            className="manager-btn manager-btn-secondary"
            onClick={handleOpenDataDir}
            title="打开 ~/.agent-pin/ 数据目录"
          >
            打开数据目录
          </button>
        </div>
        {error && (
          <div
            className="manager-error"
            onClick={() => setError(null)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === "Escape") setError(null);
            }}
          >
            {error} ×
          </div>
        )}
      </header>

      <main className="manager-body">
        {loading && pins.length === 0 ? (
          <div className="manager-empty">加载中…</div>
        ) : filtered.length === 0 ? (
          <div className="manager-empty">
            {pins.length === 0
              ? "还没有 Pin。\n通过 HTTP POST /api/pins 创建第一个 Pin。"
              : `没有匹配 "${keyword}" 的 Pin`}
          </div>
        ) : (
          <ul className="manager-list">
            {filtered.map((pin) => (
              <PinCard
                key={pin.pinId}
                pin={pin}
                displayState={displayStateOf(pin)}
                busy={busyByPinId.has(pin.pinId) || hidingAll}
                onShow={() => handleShow(pin.pinId)}
                onHide={() => handleHide(pin.pinId)}
                onDelete={() => handleDelete(pin.pinId, pin.title)}
              />
            ))}
          </ul>
        )}
      </main>
    </div>
  );
}

// ---------- PinCard ----------

function PinCard({
  pin,
  displayState,
  busy,
  onShow,
  onHide,
  onDelete,
}: {
  pin: PinMeta;
  displayState: PinState | TransientState;
  busy: boolean;
  onShow: () => void;
  onHide: () => void;
  onDelete: () => void;
}) {
  const created = formatTime(pin.createdAt);
  const updated = formatTime(pin.updatedAt);
  const isFailed = pin.state === "failed";
  // 临时态文案
  const transientLabel: Partial<Record<TransientState, string>> = {
    opening: "显示中…",
    closing: "隐藏中…",
    deleting: "删除中…",
  };
  const transient = ["opening", "closing", "deleting"].includes(displayState)
    ? (displayState as TransientState)
    : undefined;

  return (
    <li className={`pin-card pin-card-${displayState} ${busy ? "pin-card-busy" : ""}`}>
      <div className="pin-card-main">
        <div className="pin-card-title-row">
          <span className={`pin-card-state pin-card-state-${displayState}`} title={displayState}>
            {stateIcon(displayState)}
          </span>
          <span className="pin-card-title" title={pin.title}>
            {pin.title}
          </span>
        </div>
        <div className="pin-card-meta">
          <span className="pin-card-time" title={`创建: ${created}\n更新: ${updated}`}>
            {created}
          </span>
          {pin.source?.agent && (
            <span className="pin-card-tag" title="agent">
              {pin.source.agent}
            </span>
          )}
          {pin.source?.workspace && (
            <span className="pin-card-tag" title="workspace">
              {pin.source.workspace}
            </span>
          )}
          {isFailed && (
            <span className="pin-card-tag pin-card-tag-warn" title="Pin 文件损坏或丢失">
              数据异常
            </span>
          )}
          {transient && transientLabel[transient] && (
            <span className="pin-card-tag pin-card-tag-transient">{transientLabel[transient]}</span>
          )}
        </div>
      </div>
      <div className="pin-card-actions">
        {pin.state === "visible" ? (
          <button
            className="pin-card-btn"
            onClick={onHide}
            disabled={busy}
            title="隐藏窗口（保留记录，可恢复）"
          >
            隐藏
          </button>
        ) : (
          <button
            className="pin-card-btn pin-card-btn-primary"
            onClick={onShow}
            disabled={busy}
            title="显示窗口"
          >
            显示
          </button>
        )}
        <button
          className="pin-card-btn pin-card-btn-danger"
          onClick={onDelete}
          disabled={busy}
          title="删除此 Pin（不可恢复）"
        >
          删除
        </button>
      </div>
    </li>
  );
}

// ---------- 辅助 ----------

function stateIcon(state: PinState | TransientState): string {
  switch (state) {
    case "visible":
      return "●";
    case "hidden":
      return "○";
    case "failed":
      return "✕";
    case "opening":
      return "◐";
    case "closing":
      return "◑";
    case "deleting":
      return "⌫";
  }
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    return d.toLocaleString("zh-CN", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}
