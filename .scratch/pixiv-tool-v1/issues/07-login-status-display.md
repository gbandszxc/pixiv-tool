# 07 — 登录态展示 + 失效检测

**What to build:**

从用户视角：App 启动后，主界面侧边栏底部显示当前登录状态——如果是已登录态显示头像 + 用户名（如 "gbandszxc"），鼠标 hover 显示 user_id；如果是未登录态显示"未登录，点此登录"按钮。如果运行过程中检测到 cookie 失效（如 pixiv 返回 401），自动清空本地 cookie 并切换到未登录态，提示用户重新登录。

**Blocked by:** 06（pywebview 登录窗）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `GET /api/auth/status` 返回 `{is_logged_in: bool, user_id: str, pixiv_id: str, name: str}` 或 `{is_logged_in: false}`
- [ ] 后端实现：启动时从 CookieStore 加载 cookie，调用 `/ajax/user/self?lang=zh`（不是 `/ajax/user/self/status`，后者 404）验证有效性
- [ ] 返回字段映射：`userData.id` → user_id、`userData.pixivId` → pixiv_id、`userData.name` → name
- [ ] 验证失败（401 或网络错误）时清空本地 cookie，返回 `is_logged_in: false`
- [ ] Vue 侧边栏底部组件显示登录状态，已登录显示用户名 + 头像（如果有 profileImg），未登录显示登录按钮
- [ ] Vue Pinia store `useAuthStore` 管理登录状态
- [ ] App 启动时主动调一次 `/api/auth/status` 更新 store
- [ ] 登录成功（ticket 06 流程）后前端主动刷新状态
- [ ] 失效检测：每次 API 调用收到 401 时，触发 store 状态清理 + Toast 提示
- [ ] `POST /api/auth/logout` 主动清空本地 cookie，前端切换到未登录态
- [ ] 单元测试：mock CookieStore 和 pixiv API 验证状态判断逻辑
