// 用生产编译和打包 CSP 运行真实组件验收，避免开发模式掩盖渲染异常。
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build, preview } from "vite";

const root = fileURLToPath(new URL("../", import.meta.url));
const temporary = await mkdtemp(path.join(tmpdir(), "pixiv-settings-"));
const outDir = path.join(temporary, "dist");
const chrome = process.env.CHROME_PATH || path.join(process.env.ProgramFiles || "C:/Program Files", "Google/Chrome/Application/chrome.exe");
let server;
try {
  await build({ root, build: { outDir, rollupOptions: { input: path.join(root, "tests/translation.html") } } });
  const config = JSON.parse(await readFile(path.join(root, "../src-tauri/tauri.conf.json"), "utf8"));
  const csp = Object.entries(config.app.security.csp).map(([key, value]) => `${key} ${value}`).join("; ");
  server = await preview({ root, build: { outDir }, preview: { host: "127.0.0.1", port: 0, headers: { "Content-Security-Policy": csp } } });
  const { port } = server.httpServer.address();
  const html = await new Promise((resolve, reject) => {
    const child = spawn(chrome, ["--headless", "--disable-gpu", "--no-first-run", `--user-data-dir=${path.join(temporary, "chrome")}`, "--dump-dom", "--timeout=15000", "--virtual-time-budget=5000", `http://127.0.0.1:${port}/tests/translation.html`], { timeout: 30000 });
    let output = "";
    child.stdout.on("data", data => { output += data; });
    child.stderr.resume();
    child.on("error", reject);
    child.on("close", code => code === 0 ? resolve(output) : reject(new Error(`Chrome 退出码 ${code}`)));
  });
  const result = html.match(/<output id="translation-result"[^>]*>([\s\S]*?)<\/output>/)?.[1];
  if (!result?.startsWith("PASS")) throw new Error(`生产设置验收失败：${result || "未获得结果"}`);
  console.log(result);
} finally {
  if (server) await new Promise(resolve => server.httpServer.close(resolve));
  if (path.dirname(temporary) !== path.resolve(tmpdir()) || !path.basename(temporary).startsWith("pixiv-settings-")) throw new Error("临时目录越界，拒绝清理");
  await rm(temporary, { recursive: true, force: true });
}
