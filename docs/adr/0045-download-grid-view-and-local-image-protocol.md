# ADR 0045：下载历史宫格模式与本地已下载图片协议

- 状态：已接受
- 日期：2026-10-30
- 关联 ADR：[ADR 0012](0012-browse-mode-own-ui.md)（pixiv-img 图片代理协议）、[ADR 0039](0039-ugoira-playback-and-page-download.md)（pixiv-ugoira 动图帧协议）
- 规格：[SPEC](../SPEC.md) §3.1（协议表）/ §6.1（下载工作区）/ §7（命令表）

## 背景

1. **下载-任务页（TasksView）卡片空间利用率低**：原先卡片分为标题行、整宽进度条、元信息行与独立的底部删除文字按钮，垂直留白过大（单卡片高度达 140px+），删除操作孤立放置，与下载历史的图标化操作列风格不统一。
2. **下载历史页（HistoryView）仅支持表格列表**：用户下载小说和插画后，仅能以密集文字表格查看标题、作者和时间，缺乏直观的视觉回显。
3. **本地已下载文件的回显诉求**：所有抓取内容已稳定落盘在本地输出目录中，回显不需要也不应该再发起外部 Pixiv 网络请求；插画多 P 作品应当显示第一张本地图片；小说需要具备书本立体质感的封面组件，并摘录本地正文前几行文字试读。

## 决策

### 1. 任务页卡片紧凑布局与红色删除图标

- 任务卡片压缩为两行紧凑结构：
  - 第 1 行：复选框、类别标签（`m3-label`）、来源与 ID，右侧对齐状态药丸（`m3-status`）；
  - 第 2 行：自适应收窄的进度条（`max-width: 620px`）与元数据信息（`done/total · 跳过 · 失败`），与复选框之间保持 24px 呼吸间距，右侧水平并排对齐操作组（暂停/继续/取消/重试），最右侧放置与下载历史完全统一的红色垃圾桶图标按钮（`md-icon-button`，颜色为 `var(--md-sys-color-error)`）。
- 卡片垂直内边距从 `var(--space-lg)` 调整为 `var(--space-md)`，卡片高度由 140px+ 压缩至约 64px，单屏可查阅任务条数翻倍。

### 2. 下载历史支持「列表 / 宫格」双模式

- 工具栏右侧操作区追加 Material 3 分段控制样式的视图切换器（`[ ≡ 列表 ] [ ⊞ 宫格 ]`），偏好通过 `localStorage["pixiv-tool-history-view-mode"]` 持久化，默认采用体验更佳的宫格模式。
- 宫格模式下采用方形自适应网格布局（`minmax(180px, 1fr)`），封面采用 1:1 正方形比例。
- 左上角复选框为纯透明方框设计，去除深灰色背景底衬；右上角在悬停（hover）时浮现胶囊操作条（查看详情 / 打开文件或目录 / 删除），未悬停时不占用空间；多 P 标牌（如 `7P`）置于右下角，避免与操作区重叠。
### 3. 本地已下载插画协议：`pixiv-download`

- 注册本地自定义协议 `pixiv-download`：
  - Windows：`http://pixiv-download.localhost/<path>`
  - macOS / Linux：`pixiv-download://localhost/<path>`
  - CSP：`img-src` 追加 `pixiv-download: http://pixiv-download.localhost`
- 安全约束：
  - URL 经 percent-decode 后规范化路径；
  - 严格限制图片扩展名白名单：`jpg` / `jpeg` / `png` / `webp` / `gif`，其余扩展名（如 `.zip`、可执行文件等）直接 403 拒绝；
  - 缺失文件返回 404；成功文件返回正确的 `Content-Type` 与 `Cache-Control: public, max-age=86400`。
- 多 P 插画取 `saved_paths[0]` 作为首图，前端 `localDownloadSrc(row.cover_path)` 直读，零网络开销并原生支持 `loading="lazy"`。

### 4. 小说专属 Material 3 书本封面（Book Cover）

- 整体封面容器与插画卡片封面区保持严整一致的正方形比例（`1:1`），确保网格行对齐。
- 立体书本拟物细节：
  - 左侧细腻的书脊立体阴影凹槽（`linear-gradient` + `inset box-shadow`）；
  - 右侧纸质层叠切面细线（`novel-pages-edge`）；
  - 底色从作品 ID 哈希映射 5 款精选书香低饱和度调色板（藏青、黛绿、绯木、玄黑、琥珀），在明暗主题下均保证舒适阅读对比度。
- 主次分明的内容排布：
  - 顶部微型「小说」印章与系列编号角标；
  - 视觉中心为两行截断的书名大标题（字重 700，带细致文字阴影）；
  - 作者署名行与微透明细分界线；
  - 底部内嵌纸质试读窗口（`novel-excerpt-box`），展示由本地 txt/md 提取的 3~4 行纯文本正文摘录，优雅展示试读内容。

### 5. `list_history` 查询字段扩展

- 联合查询在返回 `HistoryRow` 时扩展输出：
  - `cover_path: Option<String>`：从插画 `saved_paths` JSON 数组中提取的首个图片有效路径；
  - `preview_text: Option<String>`：从小说 `txt_path` 或 `md_path` 本地文件直接读取前 1KB 整理出的前 180 字符前几行纯文本。
- 字段采用 `#[serde(skip_serializing_if = "Option::is_none")]`，完全向下兼容既有调用方。
