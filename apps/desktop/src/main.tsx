import React from "react";
import ReactDOM from "react-dom/client";
import Pin from "./Pin";
import Manager from "./Manager";
// 两个 CSS 同时加载：
// - pin.css 用 .pin-* 类名（Pin 窗口）
// - manager.css 用 .manager-* 和 .pin-card-* 类名（管理界面）
// 类名互不重叠，互不影响。Vite 会把两个 CSS 打到同一个 bundle，
// 各窗口只渲染对应组件，未使用的 CSS 规则不会被匹配，无副作用。
import "./pin.css";
import "./manager.css";

// 前端入口路由：根据 URL 参数分发到不同组件。
// - ?manager=1  → Manager（管理界面窗口，tray.rs open_manager_window 使用此 URL）
// - ?pinId=xxx  → Pin（Pin 窗口，window.rs create_pin_window 使用此 URL）
// - 其他        → 兜底渲染 Pin（开发态直接访问 index.html 时会因缺 pinId 报错，
//                 这是预期行为，避免误以为入口正常）
function selectApp() {
  const params = new URLSearchParams(window.location.search);
  if (params.get("manager") === "1") {
    return <Manager />;
  }
  return <Pin />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{selectApp()}</React.StrictMode>
);
