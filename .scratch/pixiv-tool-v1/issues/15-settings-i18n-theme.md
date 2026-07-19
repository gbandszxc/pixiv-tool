# 15 — 设置页 + i18n + 主题切换

**What to build:**

从用户视角：点侧边栏"设置"进入设置页，看到所有可配置项：输出目录（带浏览按钮）、输出格式（txt/markdown 复选框）、语言（简体中文/English 下拉）、主题（浅色/深色/跟随系统）。改完点保存立即生效。切换语言后整个 UI 立即翻译，切换主题后整个 UI 配色立即变化。还有"清除日志"和"清除登录"两个按钮。

**Blocked by:** 04（Settings 配置）、10（单篇任务，需要 App 主体跑起来）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `GET /api/settings` 返回当前配置（配合 ticket 04 的 Settings 单例）
- [ ] `PUT /api/settings` 更新配置并持久化
- [ ] `POST /api/settings/clear-logs` 清空 `data/logs/app.log`
- [ ] Vue 设置页（路由 `/settings`）：
  - [ ] 输出目录输入框 + 浏览按钮（调原生目录选择对话框，通过 pywebview 或 fastapi 暴露）
  - [ ] 输出格式复选框
  - [ ] 语言下拉（简体中文 / English）
  - [ ] 主题下拉（浅色 / 深色 / 跟随系统）
  - [ ] 保存按钮（调 PUT /api/settings）
  - [ ] 清除日志按钮（带确认）
  - [ ] 清除登录按钮（调 POST /api/auth/logout，带确认）
- [ ] i18n 实现：用 vue-i18n
  - [ ] `frontend/src/locales/zh-CN.ts` 和 `en-US.ts` 两个语言包
  - [ ] 所有 UI 文案集中管理（按钮、标签、提示、Toast）
  - [ ] 切换语言后整个 UI 立即翻译（vue-i18n 响应式）
  - [ ] 语言选择持久化到 settings.json
- [ ] 主题实现：
  - [ ] CSS Variables：根 `--bg` `--fg` `--accent` 等
  - [ ] Naive UI 用 `n-config-provider` 注入 `darkTheme`
  - [ ] "跟随系统"监听 `prefers-color-scheme` media query
  - [ ] 主题选择持久化到 settings.json
- [ ] 设置变更后通知其他模块（如 output_dir 改了，Crawler 后续任务用新路径）
- [ ] 单元测试：settings 读写、i18n 切换、主题切换
