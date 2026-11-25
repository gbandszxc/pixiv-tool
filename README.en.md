<div align="center">

<img src="src-tauri/icons/icon.png" width="128" alt="pixiv-tool icon" />

# pixiv-tool

[![Version](https://img.shields.io/github/v/release/gbandszxc/pixiv-tool?label=version)](../../releases)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3-42B883?logo=vuedotjs&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.99.1-DEA584?logo=rust&logoColor=white)

[简体中文](README.md) | **English** | [日本語](README.ja.md)

</div>

---

pixiv-tool is a cross-platform desktop client for Pixiv. It covers everyday browsing of illustrations, manga, and novels, batch-downloads them for offline reading, and rounds it out with purpose-built readers for novels and manga — plus AI-powered whole-novel translation.

## Why this exists

The project began as a set of translation-workflow skills I had designed. To try them out, I wrote a script that batch-downloads Pixiv novels — and since working in the terminal was clunky, I had AI wrap it in a GUI. Viewing and downloading still felt disconnected, though, so an early version simply embedded Pixiv's web app in a webview. The visual mismatch and the memory footprint never sat right with me, so over a week-long holiday I rebuilt the pages I actually use as native screens, and redesigned the manga and novel readers from scratch.

At that point I figured I might as well build the agent backend too — and that closed the loop on the original use case: a one-stop experience for translating and reading.

## ✨ Features

### 🌐 Browse mode

A native interface over Pixiv's read-only content, covering the everyday browsing flow:

- **Home feed**: mixed recommendations and an explore gallery with infinite scroll; filter by illustrations / manga / novels
- **Channel pages**: new, recommended, ranking, and trending tags for illustrations / manga / novels, with a quick R-18 toggle up top
- **Following feed**: new works from artists you follow (illustrations / novels × all-ages / R-18)
- **Search**: keyword search with multi-axis sorting; numeric IDs and Pixiv links jump straight to the matching work, artist, or series; Google-style search-history suggestions
- **Rankings / bookmarks / view history**: rankings by period; public and private bookmarks with tag filters; local view history (one-click clear, WebDAV sync supported)
- **Work viewer**: progressive multi-page loading, full-screen paging with filmstrip thumbnails, related works, comment panel, R-18 blur
- **Novel reader**: chapter and page navigation, ruby (furigana-style) annotation rendering, series navigation, related works and comments
- **Social actions**: bookmark / unbookmark, follow / unfollow, post comments and replies (text, official emoticons and stickers)
- **Reverse image search**: SauceNAO integration — drag-and-drop or paste an image / URL to pin down its Pixiv source, with search history

### 📥 Downloads

| Source | Output |
|---|---|
| Single novel / all novels by a user | TXT / Markdown / EPUB (EPUB embeds in-text images and the cover) |
| Novel series | Bundled download: stitched into TXT / Markdown / EPUB, with a table of contents in Markdown and EPUB |
| Single & multi-page illustrations, all illustrations and manga by a user | Original-quality downloads straight from Pixiv's image servers |
| Ugoira animations | Animated GIF at the original frame resolution |

### 📖 Whole-novel AI translation

- **Bring your own model**: plug in your own API — OpenAI Chat Completions / Responses and Anthropic Messages are supported; keys are stored in the OS credential vault
- **Shared glossary**: before translating, curate character names, aliases, and style notes shared across the whole work, so names stay consistent chapter after chapter
- **Agent-driven pipeline**: multiple roles collaborate (coordinator / translator / reviewer) with tiered review — minor issues don't block delivery, serious ones trigger block-level rework
- **A complete experience**: switch between original / translated / side-by-side at any time; streaming preview with visible reasoning and token usage; translate several novels in parallel in the background; confirmation prompts survive restarts; export the finished translation to a file

### 🔐 Accounts, data & updates

- **Multi-account**: login prefers a separate Chrome / Edge window to extract cookies, with a built-in login window as fallback; switch or sign out at any time
- **Credential safety**: cookies and API keys go only into the OS credential store (Windows Credential Manager / macOS Keychain / Linux Secret Service) — never into the database, never shown in the UI or logs
- **WebDAV sync**: encrypt and sync view history, login sessions, and your SauceNAO key to your own cloud drive — manual, end-to-end encrypted
- **In-app updates**: check for updates, download, and be walked through installing — no manual reinstalls

## 📦 Installation

Grab the installer for your platform from [Releases](../../releases):

| Platform | Installer |
|---|---|
| Windows x64 / arm64 | `.msi` |
| macOS universal / aarch64 | `.dmg` |
| Linux x64 / arm64 | `.AppImage` / `.deb` / `.rpm` |

> Note for Linux: `webkit2gtk` is required at runtime (Ubuntu/Debian: `sudo apt install libwebkit2gtk-4.1-dev`). Login currently works fully on Windows / macOS; Linux hasn't been verified on real hardware yet.

## 🔨 Building from source

**Prerequisites**: Node.js 18+ with [pnpm](https://pnpm.io/); cmake (all platforms, for building BoringSSL); LLVM/libclang (required on Windows). The Rust version is pinned by the repo's `rust-toolchain.toml`, so `rustup` sets it up automatically.

```bash
# Windows (PowerShell)
.\dev.ps1 install      # Install frontend dependencies
.\dev.ps1 dev start    # Start dev server: Vite + Rust hot-reload + app window

# macOS / Linux (Git Bash / Bash)
./dev.sh install
./dev.sh dev start
```

Other common tasks:

```bash
.\dev.ps1 test         # Rust unit tests + IPC smoke tests (fully offline)
.\dev.ps1 check        # Frontend type check + cargo check
.\dev.ps1 build        # Package a release installer
.\dev.ps1 logs dev -f  # Tail dev logs
```

For the full script reference, run `.\dev.ps1 -h`; packaging details live in [docs/PACKAGING.md](docs/PACKAGING.md).

## 🏗️ Architecture

A single-process Tauri 2 app: a Rust backend and a Vue 3 / TypeScript front end talk directly over IPC — **no local HTTP server, no fixed ports, no extra backend processes**. Scraping uses wreq's browser TLS fingerprint impersonation, and novel translation runs on the Rig general-purpose agent kernel.

Architecture details, the data model, and IPC contracts aren't repeated here — go straight to the source of truth, [docs/SPEC.md](docs/SPEC.md).

## 📚 Documentation

> The documents below are written in Chinese.

| Document | Contents |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | Single source of truth: scope / architecture / data model / IPC / risk register |
| [GLOSSARY.md](GLOSSARY.md) | Domain glossary |
| [DESIGN.md](DESIGN.md) | Frontend visual & interaction spec (Material 3 token system) |
| [docs/PIXIV-API.md](docs/PIXIV-API.md) | Pixiv endpoint contract reference |
| [docs/PACKAGING.md](docs/PACKAGING.md) | Packaging / distribution / build environment / signing |
| [docs/adr/](docs/adr/) | Architecture decision records (ADR 0001 – 0055) |
| [docs/research/](docs/research/) | Captured evidence from external API research |
| [AGENTS.md](AGENTS.md) | Entry point for AI collaboration (required reading before agent changes) |

## ⚠️ Disclaimer

- This tool is for personal, local organizing of content **you have the right to access**. Please follow Pixiv's Terms of Use and the laws of your jurisdiction; do not use it for bulk scraping, redistribution, or commercial purposes.
- This project is not affiliated with pixiv (pixiv Inc.).
- Local-first and private by default: no error reporting, no telemetry. Login credentials are stored only in your machine's OS credential store.
