// ESLint 9 flat config
// 仅检查 apps/desktop/src 下的前端 TS/TSX 代码
// 忽略 dist 产物和 src-tauri（Rust 后端，不在 ESLint 范围）
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-unused-vars": ["warn", { argsIgnorePattern: "^_" }],
      "@typescript-eslint/no-explicit-any": "off",
    },
    ignores: ["dist/", "src-tauri/"],
  },
);
