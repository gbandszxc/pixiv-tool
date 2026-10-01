//! `tests/pixiv_api/` —— pixiv 接口测试的**唯一入口目录**（测试目标 `pixiv_api`）。
//!
//! 目录职责
//! --------
//! - `common.rs`：cookie 三级获取 + `PixivApi` 构造（进程内共用）+ 断言辅助；
//! - `offline_guard.rs`：命令层离线冒烟（由 `tests/browse_smoke.rs` 迁移，
//!   断言自迁移起逐字未改）；
//! - `live_read.rs`：只读端点在线实测（真实登录态 + 真实 pixiv）；
//! - `live_write.rs`：写端点 add→delete 往返（默认跳过，需 `PIXIV_LIVE_WRITE=1`）。
//!
//! 两条跑法
//! --------
//! - 离线：`./dev.ps1 test`（等价 `cargo test --locked`）——跑 lib 单测 +
//!   `offline_guard`；全部 live 用例带 `#[ignore]`，不会触网、不读凭据。
//! - 在线：`./dev.ps1 test-live`（等价
//!   `cargo test --locked --test pixiv_api -- --ignored --test-threads=1`）——
//!   串行执行 `live_*` 用例，共用同一客户端实例（并发 2 / 间隔 400ms / 重试 3 /
//!   429 暂停 60s），不要并发跑多个 test-live。
//!
//! 新增端点的步骤
//! --------------
//! 1. 在 `src-tauri/src/pixiv/api.rs` / `browse_api.rs` 增加 `PixivApi` 方法；
//! 2. 本目录加同名 `live_*` 用例（只读进 `live_read.rs`，写操作进 `live_write.rs`）；
//!    样例 id 一律在用例内动态取样（榜单/搜索首条），不硬编码易失效的旧 id；
//! 3. 更新 `docs/PIXIV-API.md` 的端点维护矩阵（与代码同一变更批次，勿留待下次）。
//!
//! 凭据纪律
//! --------
//! - cookie 来源按 `common.rs` 三级回退：环境变量 → `default` 条目 →
//!   `config/accounts.json` 的 active 对应 `u-<id>` 条目；全部为**只读**；
//!   **绝不允许**在测试里对任何条目调用 `save()` / `clear()`（会毁掉用户真实登录态）。
//! - 日志与断言消息不得出现 cookie 值、csrf token 值（只可出现字段名、长度、条数）。
//! - 写用例只在 `PIXIV_LIVE_WRITE=1` 时执行，且必须先 add（restrict=1 私密）
//!   再删除还原；无论断言成败都要尝试删除。

mod common;
mod live_read;
mod live_write;
mod offline_guard;
