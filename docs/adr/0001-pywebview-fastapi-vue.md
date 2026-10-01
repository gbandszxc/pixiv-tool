# ADR 0001 · 技术栈选型：pywebview + FastAPI + Vue3

**状态**：已被 ADR 0008 取代（仅 Vue3 前端选型沿用） · **日期**：2026-07-19 · **关联 SPEC**：§2.1

> 状态补充（2026-08-20）：本文桌面壳与后端选型已被 [ADR 0008](0008-tauri-rewrite.md) 全量取代（Tauri 2 + Rust）；前端框架选型（Vue3）仍有效。
> 状态补充（2026-10-01）：前端 UI 层已由 Naive UI 换为 `@material/web`（Material 3），视觉规范见根目录 `DESIGN.md`。本文正文的 pywebview / FastAPI 描述均为历史。

## 背景

需要为 Pixiv 小说抓取工具选择桌面客户端技术栈。候选：

- A. Electron + Node/TS
- B. Tauri + Rust + 前端框架
- C. Python + pywebview + Vue
- D. Python FastAPI + Vue 纯 Web 应用

约束：
- 作者熟 Python、已装 fastapi/vue 相关 skill
- 需嵌入式登录窗（取 HttpOnly cookie）
- 目标跨平台、解压即用
- 体积尽可能小

## 决策

选 **C（pywebview + Vue3 + FastAPI）**。

理由：
1. Python 生态下 httpx、sqlite3、异步、cookie 处理顺手，与旧 PowerShell 脚本同源。
2. pywebview 在 Windows 调用 Edge WebView2，既能渲染 Vue 前端、又能弹真登录窗取 cookie，一箭双雕。
3. 比 Electron 小、比 Tauri 简单（无 Rust 学习成本）。
4. 纯 Web 方案 D 不符合"客户端工具"定位。

## 后果

**正面**
- 学习成本低，复用作者已有技能。
- 单一代码库覆盖三平台。
- 登录窗与主窗统一技术栈。

**负面**
- pywebview `get_cookies()` 能否读 HttpOnly 需 spike 验证（R1）。
- Linux 需用户预装 webkit2gtk（R4）。
- Python 打包体积 ~50MB，比 Tauri 大。
