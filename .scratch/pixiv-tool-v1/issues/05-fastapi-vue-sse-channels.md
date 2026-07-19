# 05 — FastAPI 路由 + Vue axios + SSE 通道

**What to build:**

从用户视角：前端 Vue 页面有一个"测试连接"按钮，点击后调用后端 `/api/ping` 接口，把后端返回的时间戳显示在页面上。同时前端有一个"测试 SSE"按钮，点击后建立到 `/api/test/events` 的 EventSource 连接，后端每秒推送一条测试事件，前端实时显示 3 条事件。前后端通过这套基础通道完成所有后续功能的通信基础。

**Blocked by:** 01（项目骨架 + dev.ps1）

**Status:** done

**Acceptance criteria:**

- [x] `backend/api/__init__.py` 注册 FastAPI router，挂载到主 app
- [x] `GET /api/ping` 返回 `{"pong": <ISO8601 timestamp>}`
- [x] `GET /api/test/events` 返回 `text/event-stream`，连续推 3 条 SSE 事件（间隔 1s），格式符合 SPEC §7.6
- [x] SSE 事件类型示例：`event: progress\ndata: {"n": 1}\n\n`
- [x] `frontend/src/api/` 封装 axios 实例，baseURL 在 dev 模式为空（走 Vite proxy），prod 模式为当前 origin
- [x] `frontend/src/api/sse.ts` 封装 EventSource 工厂，提供类型安全的事件订阅接口
- [x] Vue 测试页面有"测试连接"和"测试 SSE"两个按钮，分别调用上述接口并显示结果
- [x] CORS：dev 模式 FastAPI 允许 `localhost:9961`，prod 模式同源不需要
- [x] 错误处理：网络失败时前端显示明确错误，不静默
- [x] TypeScript 类型覆盖：所有 API 请求和响应都有 interface
- [x] 单元测试（后端）：ping 返回正确格式；SSE 推送 3 条事件
