// 版本号同步脚本
//
// 单一事实来源：根目录 package.json 的 version 字段
// 同步目标：
//   - apps/desktop/package.json
//   - apps/desktop/src-tauri/tauri.conf.json
//   - apps/desktop/src-tauri/Cargo.toml
//   - packages/cli/Cargo.toml
//   - packages/shared/Cargo.toml
//
// 用法：
//   node scripts/sync-version.mjs            # 读取 root，写入其余 4 处
//   node scripts/sync-version.mjs --check   # 仅校验是否一致，不写文件（CI 用，不一致退出码 1）
//
// 设计决策：
// - 不引入第三方依赖（不用 commander / semver），仅用 Node 内置模块，避免给 release 流程加 npm 依赖
// - 直接正则替换 Cargo.toml 的 version 行，避免 TOML 解析器依赖
// - 保留 JSON 文件原有缩进与字段顺序（通过 read + 结构化解析 + stringify）

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');

const args = process.argv.slice(2);
const checkOnly = args.includes('--check');

// 读 root version
const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const version = rootPkg.version;
if (!version || !/^\d+\.\d+\.\d+/.test(version)) {
  console.error(`[sync-version] root version invalid: ${version}`);
  process.exit(1);
}

/** @type {{file: string, ok: boolean, before?: string, after?: string}[]} */
const results = [];

function syncJson(file) {
  const path = join(root, file);
  const raw = readFileSync(path, 'utf8');
  const obj = JSON.parse(raw);
  const before = obj.version;
  if (before === version) {
    results.push({ file, ok: true });
    return;
  }
  if (checkOnly) {
    results.push({ file, ok: false, before });
    return;
  }
  obj.version = version;
  // 保留 2 空格缩进
  writeFileSync(path, JSON.stringify(obj, null, 2) + '\n', 'utf8');
  results.push({ file, ok: true, before, after: version });
}

function syncCargoToml(file) {
  const path = join(root, file);
  const raw = readFileSync(path, 'utf8');
  // 用三个捕获组提取 version 值，避免 slice 假设固定字符数。
  // group1 = 前缀（"version = \""，含可能的前导空白），group2 = 旧版本号，group3 = 后缀（"\""）。
  // m 锚点 + \s* 兼容 "version=..." 与 "version = ..." 等不同空白格式。
  const re = /^(\s*version\s*=\s*")([^"]+)(")/m;
  const m = raw.match(re);
  if (!m) {
    console.error(`[sync-version] cannot find version field in ${file}`);
    process.exit(1);
  }
  const before = m[2];
  if (before === version) {
    results.push({ file, ok: true });
    return;
  }
  if (checkOnly) {
    results.push({ file, ok: false, before });
    return;
  }
  // 替换时保留原始前缀/后缀格式（引号与空白），仅替换捕获组 2 的值
  const next = raw.replace(re, `$1${version}$3`);
  writeFileSync(path, next, 'utf8');
  results.push({ file, ok: true, before, after: version });
}

// 同步目标清单（顺序无关）
syncJson('apps/desktop/package.json');
syncJson('apps/desktop/src-tauri/tauri.conf.json');
syncCargoToml('apps/desktop/src-tauri/Cargo.toml');
syncCargoToml('packages/cli/Cargo.toml');
syncCargoToml('packages/shared/Cargo.toml');

// 输出报告
let dirty = false;
for (const r of results) {
  if (r.ok && r.before === undefined) {
    console.log(`  OK   ${r.file}`);
  } else if (r.ok) {
    console.log(`  SYNC ${r.file}: ${r.before} -> ${r.after}`);
  } else {
    dirty = true;
    console.log(`  DIFF ${r.file}: root=${version}, file=${r.before}`);
  }
}

if (dirty) {
  console.error(`[sync-version] version mismatch (root=${version}). Run: node scripts/sync-version.mjs`);
  process.exit(1);
}
console.log(`[sync-version] all targets at version ${version}`);
