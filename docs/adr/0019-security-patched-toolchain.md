# ADR 0019 · 安全补丁驱动的构建工具链升级

**状态**：已接受 · **日期**：2026-10-04 · **关联**：SPEC §2.1、§8、PACKAGING §2、§3、ADR 0008

## 背景

2026-10-04 使用 npm 官方 audit API 对锁文件审计，发现 Vite 5.4.21 存在
Windows 文件拒绝列表绕过与 UNC 路径触发 NTLM 凭据泄露的上游公告。
Vite 5 没有相应修复版本；继续固定旧主版本无法用补丁升级解决。
同时，Rust 传递依赖的 `time` 0.3.45、`serde_with` 3.17 包含已公布的
拒绝服务问题，其安全补丁要求 Rust 1.88。

## 决策

1. Vite 升级至兼容项目现有配置的最低安全版本线 **6.4.3+**，保持
   `@vitejs/plugin-vue` **5.2.4** 与现有 Vue 3、Material Web、Tauri IPC 架构。
   不增加运行时框架，不改开发端口 9961、strictPort 与构建入口。
2. 通过锁文件更新修复 `esbuild`、`brace-expansion`、`postcss`、`nanoid`
   等传递依赖，使用官方 registry 的 audit API 验证；不额外添加同名
   顶层依赖来掩盖旧传递版本。
3. Rust 最低工具链统一为 **1.88**（edition 2024），允许升级 `time` 至
   0.3.47、`serde_with` 至 3.21。wreq / wreq-util 的既有指纹锁定保持有效。
   `plist` 同步更新至 1.10.1，使用 `quick-xml` 0.42.0 修复 XML 拒绝服务问题。
4. 对无法兼容升级的 GTK/WebKit 等传递依赖，保留审计结论与路径可达性
   说明；未维护公告与可达漏洞分别描述，不能宣称所有依赖风险已消除。

## 后果

开发与 CI 继续使用既有 pnpm / cargo 入口；本地 Rust 低于 1.88 的设备
需要升级工具链。升级依赖必须通过前端生产构建、现有组件回归和 Rust
离线测试，业务 API、输出文件布局及用户功能保持一致。

## 证据

- [Vite Windows 路径绕过公告（修复 6.4.3）](https://github.com/vitejs/vite/security/advisories/GHSA-fx2h-pf6j-xcff)
- [launch-editor UNC/NTLM 泄露公告（Vite 修复 6.4.3）](https://github.com/vitejs/launch-editor/security/advisories/GHSA-v6wh-96g9-6wx3)
- [time 栈耗尽公告（修复 0.3.47）](https://rustsec.org/advisories/RUSTSEC-2026-0009.html)
- [serde_with KeyValueMap 公告（修复 3.21）](https://github.com/jonasbb/serde_with/security/advisories/GHSA-7gcf-g7xr-8hxj)
