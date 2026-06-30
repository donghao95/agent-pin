import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 期望前端 dev server 跑在 1420 端口
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1", // 强制 IPv4，避免 Windows 上 IPv6 ::1 权限问题（EACCES）
    port: 1800, // 1420 在 Windows Hyper-V 保留端口范围，换 1800
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
