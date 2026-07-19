# 08 — PixivClient（httpx + 限速 + 重试）

**What to build:**

从用户视角：底层有一个统一的 Pixiv HTTP 客户端，自动携带登录 cookie + x-csrf-token，所有请求都经过限速（并发 2 + 0.4s 间隔）和容错处理（超时、重试、429 暂停）。其他模块（如 Source、Crawler）调用 `client.get_novel(id)` 就能拿到小说数据，无需关心限速和重试细节。

**Blocked by:** 06（pywebview 登录窗，需要 cookie + csrf）、02（SQLite，存储元数据）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `backend/core/pixiv_client.py` 实现 `PixivClient` 类，封装 httpx.AsyncClient
- [ ] 请求头自动注入：cookie（PHPSESSID 等）+ `x-csrf-token` + 必要的 User-Agent
- [ ] 接口方法（V1 至少）：`get_novel(novel_id)`、`get_series_content(series_id)`、`get_user_novels(user_id)`、`get_user_self()`（验证登录态）
- [ ] 限速：`asyncio.Semaphore(2)` + 每请求后 `await asyncio.sleep(0.4)`
- [ ] 超时：单请求 15s（httpx.Timeout）
- [ ] 重试：失败重试 3 次，指数退避 1s → 2s → 4s
- [ ] 429 处理：检测到 429 状态码立即让全队列暂停 60s（通过共享 asyncio.Event），记日志，恢复后继续
- [ ] 错误分类：网络错误、超时、4xx（401/403/404）、5xx、429 分别有清晰的异常类型
- [ ] 401 时触发 cookie 失效信号（配合 ticket 07）
- [ ] 日志：每个请求记录 URL + 状态码 + 耗时，不记录 cookie 完整值
- [ ] 配置参数写死为常量（V1 不暴露给用户）：concurrency=2、interval=0.4、timeout=15、retry=3、pause_on_429=60
- [ ] 单元测试：mock httpx 验证限速、重试、429 暂停逻辑
- [ ] 集成测试：用真实 cookie 调 `get_novel(<已知 id>)` 返回正确数据（需登录态）
