import { useEffect, useState, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PinMeta, PinState } from "./types";

export default function Manager() {
  const [pins, setPins] = useState<PinMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState("");
  // M7：用 Set 支持多 Pin 并发操作锁，避免单值锁被覆盖
  const [busyPinIds, setBusyPinIds] = useState<Set<string>>(new Set());
  // M8：隐藏全部操作锁
  const [hidingAll, setHidingAll] = useState(false);

  // silent=true 时不清 loading 状态，避免单 Pin 操作后刷新按钮闪烁（m11）
  const refresh = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    setError(null);
    try {
      const list = await invoke<PinMeta[]>("list_pins");
      setPins(list);
    } catch (e) {
      setError(String(e));
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // m3：监听后端 pin:show-failed 事件（AsyncCreate 模式下窗口创建失败时触发）
  // 后端 show_pin 用 AsyncCreate 立即返回 Ok，窗口异步创建失败时通过此事件通知
  useEffect(() => {
    const unlistenPromise = listen<{ pinId: string; message: string }>(
      "pin:show-failed",
      (event) => {
        setError(`显示失败 (${event.payload.pinId}): ${event.payload.message}`);
        refresh(true);
      }
    );
    return () => {
      unlistenPromise.then((fn) => fn()).catch(() => {});
    };
  }, [refresh]);

  const handleShow = async (pinId: string) => {
    setBusyPinIds((prev) => new Set(prev).add(pinId));
    try {
      await invoke("show_pin", { pinId });
      await refresh(true);
    } catch (e) {
      setError(`显示失败: ${e}`);
    } finally {
      setBusyPinIds((prev) => {
        const next = new Set(prev);
        next.delete(pinId);
        return next;
      });
    }
  };

  const handleHide = async (pinId: string) => {
    setBusyPinIds((prev) => new Set(prev).add(pinId));
    try {
      await invoke("hide_pin", { pinId });
      await refresh(true);
    } catch (e) {
      setError(`隐藏失败: ${e}`);
    } finally {
      setBusyPinIds((prev) => {
        const next = new Set(prev);
        next.delete(pinId);
        return next;
      });
    }
  };

  const handleHideAll = async () => {
    setHidingAll(true);
    try {
      await invoke("hide_all_pins");
      await refresh(true);
    } catch (e) {
      setError(`隐藏全部失败: ${e}`);
    } finally {
      setHidingAll(false);
    }
  };

  const handleDelete = async (pinId: string, title: string) => {
    if (!window.confirm(`确定删除 "${title}" 吗？\n此操作不可恢复。`)) return;
    setBusyPinIds((prev) => new Set(prev).add(pinId));
    try {
      await invoke("delete_pin", { pinId });
      await refresh(true);
    } catch (e) {
      setError(`删除失败: ${e}`);
    } finally {
      setBusyPinIds((prev) => {
        const next = new Set(prev);
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
            onClick={() => refresh()}
            disabled={loading}
            title="刷新列表"
          >
            {loading ? "刷新中…" : "刷新"}
          </button>
          <button
            className="manager-btn"
            onClick={handleHideAll}
            disabled={hidingAll || visibleCount === 0 || busyPinIds.size > 0}
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
                busy={busyPinIds.has(pin.pinId)}
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
  busy,
  onShow,
  onHide,
  onDelete,
}: {
  pin: PinMeta;
  busy: boolean;
  onShow: () => void;
  onHide: () => void;
  onDelete: () => void;
}) {
  const created = formatTime(pin.createdAt);
  const updated = formatTime(pin.updatedAt);
  const isFailed = pin.state === "failed";

  return (
    <li className={`pin-card pin-card-${pin.state} ${busy ? "pin-card-busy" : ""}`}>
      <div className="pin-card-main">
        <div className="pin-card-title-row">
          <span className={`pin-card-state pin-card-state-${pin.state}`} title={pin.state}>
            {stateIcon(pin.state)}
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

function stateIcon(state: PinState): string {
  switch (state) {
    case "visible":
      return "●";
    case "hidden":
      return "○";
    case "failed":
      return "✕";
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
