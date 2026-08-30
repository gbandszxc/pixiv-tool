# ADR 0011 · 登录窗必然以未登录态打开

**状态**：已接受 · **日期**：2026-08-30 · **关联**：SPEC §4.1、ADR 0006、ADR 0009、ADR 0010

## 背景

浏览器登录（ADR 0006）与内嵌 webview 回退登录（ADR 0009）都使用**持久化
profile**：`<config>/login-browser-profile` / `<config>/login-webview-profile`。
持久化的本意是积累设备状态（device_token、cf_clearance 等），降低登录风控
（ADR 0006：「独立浏览器 profile 可积累正常设备状态」）。

多账号功能（ADR 0010）上线后暴露缺口：profile 里已有有效会话时，
`accounts.pixiv.net/login` 会把已登录用户 **302 回 pixiv 主站**；登录轮询
循环看到主站 + 有效 PHPSESSID 即刻判 success 关窗。结果：「添加账号」永远
秒拿到已登录的旧账号，用户根本见不到登录表单。

## 决策

**登录窗语义定为「必然以未登录态打开」**：两条登录路径在登录页请求发出前
保证 profile 无 pixiv 会话。

1. **浏览器 CDP 路径——定向清会话，保留设备态**：
   - 浏览器改为 `--app=about:blank` 启动，登录页改由 CDP 在清理完成后
     `Target.createTarget` 打开。顺序是正确性的关键：登录页请求若先于
     清理发出，302 决策已由服务端根据请求头里的旧 cookie 做出，事后清理
     无法阻止跳转。
   - 清理方式：`Storage.getCookies` 找 pixiv 域 PHPSESSID（精确
     `(name, domain, path)` 身份），`Storage.setCookies` 以 `expires: 0`
     过期覆盖删除（浏览器级 CDP 即可，无需 page session；真机已验证）。
   - 只删 PHPSESSID，**保留 device_token / cf_clearance** 等——这是
     ADR 0006 持久化 profile 的全部意义，避免每次登录都被当全新设备。
2. **webview 回退路径——整目录重置**：打开前
   `remove_dir_all(login-webview-profile)`。webview 无 CDP 通道做定向清理，
   且「首导航前注入 cookie 不可靠」（browse 模块实测），而该目录以
   「删目录=完整复测」为既定语义（ADR 0009 第 4 条）；作为无 Chromium
   环境的低频回退路径，重置成本可接受。
3. 清理失败不阻塞登录（记 warn 降级为旧行为），登录流程整体不因前置
   清理失败而失败。

## 备选与弃选理由

- **整目录重置（浏览器路径）**：最简单，但每次登录都是全新设备指纹，
  登录风控（CF/验证码）概率显著上升，丢掉持久化初衷。
- **先导航 logout.php**：依赖 pixiv 登出跳转链路，登出瞬间落在主站会被
  轮询误判，竞态多。
- **已登录视为成功**：满足不了「添加另一个账号」的核心诉求。

## 后果

**正面**

- 「添加账号」必然出现登录表单；多账号语义闭环。
- 浏览器路径保留设备态，登录风控不升温。

**负面**

- 「重新登录同一账号」不再秒过（旧会话已被清），必须重输密码。多账号
  切换（ADR 0010）已覆盖会话过期的主场景，代价可接受。
- 浏览器启动序列多一次 CDP 往返（毫秒级）；启动页短暂显示 about:blank
  后跳转登录页。
- webview 回退路径每次登录重建 profile，首次加载略慢（回退路径低频，可接受）。
