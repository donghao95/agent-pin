import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

// 与后端 storage.rs 对齐
type PinState = "visible" | "hidden" | "failed";

type PinSource = {
  agent?: string;
  workspace?: string;
  task?: string;
  conversationId?: string;
};

type PinMeta = {
  pinId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  state: PinState;
  source?: PinSource;
};

export default function Manager() {
  const [pins, setPins] = useState<PinMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState("");
  const [busyPinId, setBusyPinId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<PinMeta[]>("list_pins");
      setPins(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const handleShow = async (pinId: string) => {
    setBusyPinId(pinId);
    try {
      await invoke("show_pin", { pinId });
      await refresh();
    } catch (e) {
      setError(`显示失败: ${e}`);
    } finally {
      setBusyPinId(null);
    }
  };

  const handleHide = async (pinId: string) => {
    setBusyPinId(pinId);
    try {
      await invoke("hide_pin", { pinId });
      await refresh();
    } catch (e) {
      setError(`隐藏失败: ${e}`);
    } finally {
      setBusyPinId(null);
    }
  };

  const handleHideAll = async () => {
    try {
      await invoke("hide_all_pins");
      await refresh();
    } catch (e) {
      setError(`隐藏全部失败: ${e}`);
    }
  };

  const handleDelete = async (pinId: string, title: string) => {
    if (!window.confirm(`确定删除 "${title}" 吗？\n此操作不可恢复。`)) return;
    setBusyPinId(pinId);
    try {
      await invoke("delete_pin", { pinId });
      await refresh();
    } catch (e) {
      setError(`删除失败: ${e}`);
    } finally {
      setBusyPinId(null);
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
  const filtered = pins.filter((p) => {
    if (!keyword.trim()) return true;
    const kw = keyword.trim().toLowerCase();
    const inTitle = p.title.toLowerCase().includes(kw);
    const inAgent = p.source?.agent?.toLowerCase().includes(kw) ?? false;
    const inWorkspace =
      p.source?.workspace?.toLowerCase().includes(kw) ?? false;
    return inTitle || inAgent || inWorkspace;
  });

  const visibleCount = pins.filter((p) => p.state === "visible").length;
  const hiddenCount = pins.filter((p) => p.state === "hidden").length;
  const failedCount = pins.filter((p) => p.state === "failed").length;

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
            onClick={refresh}
            disabled={loading}
            title="刷新列表"
          >
            {loading ? "刷新中…" : "刷新"}
          </button>
          <button
            className="manager-btn"
            onClick={handleHideAll}
            disabled={visibleCount === 0}
            title="隐藏所有可见 Pin"
          >
            隐藏全部
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
          <div className="manager-error" onClick={() => setError(null)}>
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
                busy={busyPinId === pin.pinId}
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
