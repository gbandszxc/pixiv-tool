<div align="center">

<img src="src-tauri/icons/icon.png" width="128" alt="pixiv-tool 图标" />

# pixiv-tool

![Version](https://img.shields.io/badge/version-1.3.7-2f6fd0)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3-42B883?logo=vuedotjs&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.99.1-DEA584?logo=rust&logoColor=white)

</div>

---

pixiv-tool 是一款跨平台 PC 端第三方 Pixiv 客户端。支持插画、漫画、小说的浏览与批量下载，同时精心设计了小说与漫画模式的阅读器，并加入大模型小说翻译等高级功能。

## 项目来源

故事的开始是我设计了一套小说翻译的工作流 skill，为了验证效果于是写了个批量扒 pixiv 小说的脚本。由于嫌终端操作麻烦索性又让 AI 封装了个 GUI，但还是觉得查看和下载太脱节了。

于是，早期版本还有过直接 webview 嵌入整个 pixiv 的网页版的时候。但是视觉割裂 + 资源占用过高，始终难以让自己满意。国庆节放假便一步到位把常用页面都做了封装。并重新设计了漫画和小说的阅读器。

最后，想着都做到这个地步了，索性后台直接把 Agent 也做了，彻底实现了翻译阅读的一站式体验，闭环了最初的使用场景……

## ✨ 功能特性

### 🌐 浏览模式

以自有界面代理 pixiv 的只读浏览，覆盖日常刷站的主要场景：

- **首页推荐流**：混合推荐与探索画廊，触底无限滚动，支持插画 / 漫画 / 小说分类筛选
- **频道页**：插画 / 漫画 / 小说的新作、推荐、排行榜与热门标签，顶部 R-18 快捷筛选
- **关注动态**：关注作者的新作品（插画 / 小说 × 全年龄 / R-18）
- **搜索**：关键词搜索与多维排序；纯数字 ID 与 pixiv 链接直达对应作品、作者、系列；Google 风格搜索历史下拉
- **排行榜 / 收藏 / 浏览历史**：多周期排行榜；公开与私密收藏（含标签筛选）；本地浏览访问历史（可一键清空，支持 WebDAV 同步）
- **作品查看器**：多页渐进加载、全屏翻页 + 胶卷缩略图、相关推荐与评论面板、R-18 遮罩
- **小说阅读器**：章节 / 分页 / 注音（ruby）标记渲染、系列导航、相关推荐与评论
- **社交动作**：收藏与取消收藏、公开关注与取关、发表评论与回复（文本、官方表情与贴图）
- **以图识图**：SauceNAO 反搜——拖拽或粘贴图片 / URL，一键定位 pixiv 来源作品，支持搜索历史

### 📥 内容下载

| 来源 | 产物 |
|---|---|
| 单篇小说 / 用户全部小说 | TXT / Markdown / EPUB（EPUB 内嵌正文插图与封面） |
| 系列小说 | 整包下载：自动拼接为 TXT / Markdown / EPUB，Markdown 与 EPUB 含目录 |
| 单幅 / 多页插画、用户全部插画与漫画 | 原图直链下载（`img-original`） |
| ugoira 动图 | 帧序列原始 zip |

### 📖 整篇小说翻译

- **自带模型服务**：配置你自己的 API 即可使用，支持 OpenAI Chat Completions / Responses 与 Anthropic Messages 三种协议，Key 存系统凭据库
- **共享术语设定集**：翻译前先维护全篇共享的专名、别名与文风设定，多章译名保持一致
- **Agent 内核驱动**：多角色协作（协调 / 翻译 / 审查），审查分级（轻微问题不阻塞、严重问题按块返工），保证交付质量
- **体验完整**：原文 / 译文 / 双语对照随时切换，实时流式预览与思考过程、token 用量可视化，多本小说并行后台翻译，等待确认跨重启可恢复，并可导出译文文件

### 🔐 账号、数据与更新

- **多账号**：登录优先使用独立 Chrome / Edge 窗口提取 Cookie，未安装时回退内置登录窗；随时切换与退出
- **凭据安全**：Cookie 与 API Key 只进系统凭据存储（Windows Credential Manager / macOS Keychain / Linux Secret Service），绝不落库、绝不出现在界面与日志中
- **WebDAV 同步**：浏览历史、登录态与 SauceNAO Key 加密同步到你的网盘，手动触发、端到端加密
- **应用内更新**：检查更新、下载并引导安装，无需手动重装

## 📦 安装

前往 [Releases](../../releases) 下载对应平台的安装包：

| 平台 | 安装包 |
|---|---|
| Windows x64 / arm64 | `.msi` |
| macOS universal / aarch64 | `.dmg` |
| Linux x64 / arm64 | `.AppImage` / `.deb` / `.rpm` |

> Linux 说明：运行需预装 `webkit2gtk`（Ubuntu/Debian：`sudo apt install libwebkit2gtk-4.1-dev`）；登录功能目前在 Windows / macOS 上完整支持，Linux 未实机验证。

## 🔨 从源码构建

**前置条件**：Node.js 18+ 与 [pnpm](https://pnpm.io/)；cmake（全平台，用于编译 BoringSSL）；LLVM/libclang（Windows 构建必需）。Rust 版本由仓库 `rust-toolchain.toml` 固定，`rustup` 会自动就位。

```bash
# Windows（PowerShell）
.\dev.ps1 install      # 安装前端依赖
.\dev.ps1 dev start    # 启动开发服务：Vite + Rust 热重载 + 应用窗口

# macOS / Linux（Git Bash / Bash）
./dev.sh install
./dev.sh dev start
```

其余常用操作：

```bash
.\dev.ps1 test         # Rust 单测 + IPC 冒烟（全离线）
.\dev.ps1 check        # 前端类型检查 + cargo check
.\dev.ps1 build        # 打包 release 安装包
.\dev.ps1 logs dev -f  # 跟踪开发日志
```

完整脚本用法以 `.\dev.ps1 -h` 输出为准；打包细节见 [docs/PACKAGING.md](docs/PACKAGING.md)。

## 🏗️ 技术架构

Tauri 2 单进程：Rust 后端 + Vue 3 / TypeScript 前端经 IPC 直连，**无本地 HTTP 服务、无固定端口、无额外后端进程**；抓取链路使用 wreq 的浏览器 TLS 指纹伪装，小说翻译运行在 Rig 通用 Agent 内核上。

详细的架构设计、数据模型与 IPC 契约不在 README 重复展开，请直接阅读真相源 [docs/SPEC.md](docs/SPEC.md)。

## 📚 文档

| 文档 | 内容 |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | 项目规格唯一真相源：范围 / 架构 / 数据模型 / IPC / 风险登记 |
| [GLOSSARY.md](GLOSSARY.md) | 领域术语表 |
| [DESIGN.md](DESIGN.md) | 前端视觉与交互规范（Material 3 token 体系） |
| [docs/PIXIV-API.md](docs/PIXIV-API.md) | pixiv 接口契约事实源 |
| [docs/PACKAGING.md](docs/PACKAGING.md) | 打包 / 分发 / 构建环境 / 签名 |
| [docs/adr/](docs/adr/) | 架构决策记录（ADR 0001 – 0054） |
| [docs/research/](docs/research/) | 外部接口调研证据档案 |
| [AGENTS.md](AGENTS.md) | AI 协作入口（agent 改动前必读） |

## ⚠️ 免责声明

- 本工具仅供个人在本地整理**自己有权访问**的内容，请遵守 pixiv 用户协议与你所在地的法律法规；不得用于批量抓取、二次分发或商业用途
- 项目与 pixiv（pixiv Inc.）官方无任何关联
- 本地优先、隐私默认：不集成错误上报与遥测，登录凭据仅保存在本机系统凭据存储中


