---
name: Pixiv Tool
description: Material 3 Web styled local workspace for Pixiv novel and illustration tasks.
colors:
  primary: "#006EAF"
  on-primary: "#FFFFFF"
  primary-container: "#CFE5FF"
  on-primary-container: "#001D36"
  secondary-container: "#D7E3F8"
  on-secondary-container: "#101C2B"
  surface: "#F8F9FF"
  surface-container: "#EDF1F9"
  on-surface: "#191C20"
  on-surface-variant: "#42474E"
  outline: "#72777F"
  error: "#BA1A1A"
  error-container: "#FFDAD6"
  on-error-container: "#410002"
  success-container: "#C8F7D0"
  success-ink: "#0D3B18"
  warning-container: "#FFDDB2"
  on-warning-container: "#2A1700"
typography:
  title:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Noto Sans CJK SC', 'Noto Sans SC', sans-serif"
    fontSize: "20px"
    fontWeight: 700
    lineHeight: 1.4
  body:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Noto Sans CJK SC', 'Noto Sans SC', sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', 'Noto Sans CJK SC', 'Noto Sans SC', sans-serif"
    fontSize: "12px"
    fontWeight: 600
    lineHeight: 1.4
rounded:
  control: "12px"
  card: "16px"
  navigation: "24px"
  dialog: "28px"
spacing:
  xxs: "4px"
  xs: "6px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
components:
  button-filled:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
  button-outlined:
    textColor: "{colors.primary}"
    rounded: "{rounded.control}"
  card:
    backgroundColor: "{colors.surface-container}"
    rounded: "{rounded.card}"
    padding: "{spacing.xl}"
  navigation-item-active:
    backgroundColor: "{colors.primary-container}"
    textColor: "{colors.on-primary-container}"
    rounded: "{rounded.navigation}"
---

# Design System: Pixiv Tool

## Overview

Pixiv Tool 是一款以任务完成为中心的本地桌面工具。当前界面采用 Material 3 Web 的组件与颜色角色：浅色页面使用冷调 surface，surface container 用于侧栏、卡片和表格；primary 只强调可执行操作、选中导航与相关标签。它不使用营销式视觉或装饰性渐变。

**The Material Role Rule.** 页面优先使用 `--md-sys-color-*` 角色和其 `--surface`、`--ink` 别名；不要重新引入旧的 Pixiv 蓝色 token 或把颜色字面值散落为新的视觉系统。

## Colors

前言中的 token 是浅色主题的规范值。`html.dark` 覆盖这些角色为：primary `#9DCBFF`、on-primary `#003354`、primary-container `#004B77`、on-primary-container `#CFE5FF`、surface `#101418`、surface-container `#1C2025`、on-surface `#E1E2E8`、on-surface-variant `#C2C7CF`、outline `#8C9199`。主题由设置项和系统 `prefers-color-scheme` 共同决定。

### Color palettes

主题模式与色板独立：模式选择浅色、深色或跟随系统；色板选择 Pixiv 蓝（默认）、靛蓝、玉石绿、紫罗兰或琥珀。色板在 `html[data-palette]` 覆盖 primary / secondary 及它们的 on/container 角色，`html.dark[data-palette]` 提供配对的深色值。设置会持久化为 `theme_color`；每个色板都必须同时定义浅深两组角色，不能只替换主按钮颜色。

- Primary / on-primary：Material Web 的 filled button，以及立即执行的主操作。
- Primary container / on-primary-container：当前左侧导航、头像底色和分类 label。
- Secondary container / on-secondary-container：Material Web select 选项的当前选择态；深色主题下使用低亮度容器与浅色文字，不能回退到组件默认紫色。
- Surface / surface container：应用底色与低强调层级；侧栏、表单卡、列表、表格及对话框使用 container。
- On-surface / on-surface-variant / outline：正文、辅助文本和分隔线。分隔线以 outline 的半透明 `color-mix()` 呈现。
- Error 与 warning container：当前自定义 alert、任务状态使用已定义的浅色危险和警告组合；文字和颜色必须一起表达状态。完成/成功状态共用一组绿色（`--state-success-container` / `--state-success-ink`，任务完成徽标与翻译状态一致，深色主题换深底浅字）；错误文字与状态指示点用 M3 标准 error 角色（`--md-sys-color-error`，浅色 #BA1A1A / 深色 #FFB4AB）。颜色只作补充，状态本身始终有可读文字。

**The Semantic State Rule.** primary 不代表成功、失败或警告；错误、警告、进度与任务状态始终要有可读文字。

## Typography

全局字体为 `system-ui, -apple-system, "Segoe UI", Roboto, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", "Noto Sans CJK SC", "Noto Sans SC", sans-serif`（`--font-app`）：`system-ui` 取各 OS 的 UI 字体（Linux 即桌面环境默认），中文按平台显式降级——macOS 苹方 → Windows 微软雅黑 → Linux Noto，杜绝回落宋体；@material/web 组件经 `--md-ref-typeface-brand` / `--md-ref-typeface-plain` 注入同一栈。正文为 14px / 400 / 1.5。页面标题为 20px / 700 / 1.4；表头、状态 pill、提示和任务元信息使用 12px，常见权重为 600。字段标签为 14px / 500，浏览器地址文本和识别 badge 为 13px。

**The Truncation Rule.** 历史表格标题、作者和浏览器地址通过省略号处理；其余长文本不得以更小字号换取空间。表格中的标题和作者提供 `title` 属性保留完整值。

## Layout

应用外壳是全高双列 grid，恒为视口高（行高钉 100vh、`overflow: hidden`）：默认侧栏 256px，折叠后 72px，主内容占余宽且是主要滚动容器（下载面板独立滚动）（`overflow-y: auto`），长内容不会把侧栏拉长。侧栏头部行高 72px，含 logo、标题与收起按钮（折叠态仅留居中的展开按钮）；导航项最小高 48px，圆角胶囊宽度扣除左右各 12px，窗口高度不足时导航区自身滚动。普通主内容的内边距为 24px；内容列（`.page-view`）在窗口宽高比 ≤16:9 时不设上限、铺满可用宽度（列表与网格自适应向右补满，不设每行条数上限），只有比 16:9 更宽的超宽窗口才按「视口高 × 16/9」封顶并居中；下载表单由非模态布局面板承载，设置表单承载于模态设置弹窗（宽 `min(840px, 94vw)`，左分组栏 + 右字段区，字段直排、不用表单卡容器）。

历史与任务列表底部统一为公共分页 AppPagination（default 变体）：行首总数回显、行尾为每页 20、50、100 条的选择器；切换容量或分类回到第 1 页。历史页把容量传至数据库查询，任务页只对当前筛选结果分页，选择与批量操作仍保留跨页状态。

空间只使用 4、6、8、12、16、24px 刻度。工具栏与控制行可以换行；任务标题在 640px 以下变为纵向排列，历史搜索框与设置路径行扩展为全宽/纵向，Pixiv 浏览工具栏允许地址栏换至下一行。表格保持最小宽度 760px 并允许横向滚动。侧栏不会因媒体查询自动隐藏，用户通过头部行的折叠按钮切换（展开态位于标题行右端，折叠态独占头部行）。

**The Compact Task Rule.** 对齐、留白和换行优先服务于任务、历史和设置的扫读，而不是制造大面积空白。

## Elevation & Depth

日常层级通过 surface 与 surface container 的色调差、outline 分隔线和圆角表达；普通卡片、任务列表和表格没有阴影。snackbar 与详情页收藏操作的 details 弹出菜单使用 `0 4px 12px rgb(0 0 0 / 18%)` 的轻阴影（**唯一合法值**，不要再写 `12px/14px` 这类非法或变体写法；snackbar 历史上的 14px blur 已于 2026-10-01 收敛到 12px），原生对话框的 backdrop 为 `rgb(0 0 0 / 35%)`。

**The Overlay-Only Shadow Rule.** 阴影仅标示临时浮层，不应用于普通内容容器。

## Shapes

Material Web 控件继承库的 M3 外观。应用自定义的 control 圆角为 12px，卡片、表格和任务列表为 16px；左侧导航当前项为 24px；登录、确认等原生 `dialog` 为 28px。状态与类别标签使用完整胶囊形状（999px），头像为圆形。

## Components

- **帮助说明 HelpTooltip**：字段标签、分组名或相关操作旁使用 32px 原生帮助按钮 + 16px Lucide circle-question-mark SVG，颜色 ink-muted，hover 为 8% on-surface，focus-visible 为 primary 2px 外环；`.field-label` 用既有 4px 间距与居中对齐，保持标签字号/字重。冗长的静态帮助默认收起，悬停、键盘聚焦或点击后显示 `role=tooltip`，按钮通过 `aria-describedby` 关联完整说明，焦点留在按钮。原生 popover top layer 防止被 dialog/滚动容器裁切，优先向下、空间不足向上，视口内保留 16px 余量；浮层为 surface-container / ink、13px/1.6、12px 内边距、control 圆角与唯一合法轻阴影，宽度至多 360px（15 × space-xl），长文本按段落换行，不新增颜色或动效。指针可移入说明继续阅读（跨间隙 150ms 关闭宽限），点击可固定/再次关闭，Esc 先关说明、外部点击/失焦/页面滚动/resize 关闭。设置六组、登录指引、搜索链接/ID 帮助、以图识图介绍统一使用此入口；错误、校验、进度、空态、未配置引导、确认操作后果与维护操作立即生效的短提示仍直接显示。
- 小说翻译：翻译入口为原分页底栏中、阅读背景按钮右侧的 20px Lucide languages SVG + md-icon-button；点击在按钮上方弹出 256px（16 × space-lg）面板，右边缘对齐按钮，surface-container 底、既有 control 圆角与唯一合法轻阴影。入口与面板共用 8px 状态指示点：翻译中 primary、本页已译 `--state-success-ink`、失败 error（入口点带 2px surface 描边与图标按钮区隔）；面板内指示点与 12px ink-muted 状态文字同排，失败原因经指示点/入口 title 悬停可读，状态文字本身也可读。设定集整理与两轮精翻属内部实现，界面只回显统一的「翻译中…」与最终成败，不展示阶段进度。面板依次显示标题、当前页状态、三种模式、md-outlined-button 翻译/重译；透明遮罩点击外部或 Esc 关闭，打开焦点进入面板，关闭回到图标；翻译期间仍可打开查看状态。三种模式沿用原生文本切换按钮 + aria-pressed，当前项 secondary-container/on-secondary-container、既有 control 圆角。默认双语，译文紧随对应原文段落或章节，沿用字号/行距、primary 色（纸色模式为 primary 20% + ink 80% 混色以保持深浅主题对比度）、lang=zh-CN、纯文本；原文用 ink，图片不重复。无译文明确提示，仅译文不冒充原文；错误用 error 与可重试文字，旧译文保留。既有 4/8/16px spacing；≤800px 按实际剩余空间收缩阅读进度滑杆，≤600px 同一底栏内控件分两行以免重叠，无新增动效，翻译按钮沿用 Material 默认 hover/focus/disabled；模式按钮沿用既有 8% hover 和 2px primary focus-visible 外环。
- 设置「小说翻译」组沿用直排 Material 字段：API URL、password Key（已保存只显示状态、空白保持、清除后保存）、模型 ID、五行高级 JSON 文本框。模型 ID 默认是文本框，「获取模型」成功后原位变为 md-outlined-select（选项含获取结果与当前值，另有「手动输入模型 ID」切回文本）；「检测可用」结果以 `--state-success-ink` 绿或 `--md-sys-color-error` 红回显，探测期间显示进行中文案。JSON 格式错误字段内提示；简体中文目标/两轮调用/发送范围与 URL 规则合入 API URL 帮助，凭据保存规则与高级 JSON 示例分别合入对应字段帮助；沿用常驻保存/取消与脏检查。

- `md-filled-button`：主提交、恢复和确认操作；`md-outlined-button`：浏览、同步、批量删除等次要操作；`md-text-button`：取消、删除等低强调操作；`md-icon-button`：工具栏和行级图标动作。
- `md-outlined-text-field`、`md-outlined-select`、`md-radio` 与 `md-checkbox`：所有可编辑字段和选择；字段以标签、12px–16px 间距和至少 40px 的 choice 行组织。
- `md-tabs` / `md-primary-tab`：登录方式切换；`md-secondary-tab`（SectionTabs 封装）：关注 / 我的分区与下载页顶部页签（下载页页签即子路由导航）；`md-linear-progress`：任务进度。Material Web 负责它们的默认交互状态。
- 原生 `dialog`：登录、退出/删除确认与模态设置弹窗；容器与内部结构收敛于 `main.css` 的「原生 dialog 通用层」（`.m3-dialog`：surface-container、28px 圆角、24px 内边距、35% scrim、标题 18px/700；紧随标题的说明文字为 on-surface-variant，`.dialog-actions` 右对齐操作行、上间距 24px）。退出确认弹窗为「标题 + 说明」的 360px 窄弹窗；历史/任务的删除确认是无标题的单行正文式，正文保持主文案层级。自定义 `.m3-snackbar` 固定在右上角，通知在 3.2 秒后消失。页面不应另建成功提示条。
- 左侧导航：四个核心入口「发现 / 关注 / 我的 / 下载」、无分组标题；图标加文字，hover 使用 8% primary 的状态层，active 使用 primary-container。账号区位于侧栏底部，为头像 chip（头像 + 账号名 + 向上箭头，hover 8% primary 状态层；折叠态居中只显示头像），点击在 chip 上方弹出账号菜单。
- 账号菜单：锚定头像 chip 上方的轻量 popover——宽 264px、surface-container 底、12px 圆角、既定轻阴影 `0 4px 12px rgb(0 0 0 / 18%)`，无深色 scrim，透明遮罩仅负责点击外部关闭（Esc 同效）；内容为账号列表（当前账号 ✓，点击切换）/ 添加账号 + 分隔线 + 「设置」入口（图标 + 文案 + 右侧箭头）与退出登录（danger 色文案，未登录置灰，经原生 confirm 弹窗确认后执行），菜单行高 40px、hover 8% primary 状态层；「设置」与「退出登录」之间以一条分隔线隔出 meta 行——左对齐版本回显（12px muted 文案，`getVersion()` 动态读取真实 app 版本，读不到显示 `--`）+ 右对齐 GitHub 主页图标入口（16px GitHub mark、28px 圆形按钮，hover 8% primary 状态层，点击关菜单并经系统默认浏览器打开）。弹出过渡复用既有 0.15s ease、transform-origin 左下（reduced-motion 由全局兜底），层级低于 snackbar。
- 设置弹窗：账号菜单「设置」入口打开的模态 `dialog`（菜单先自行关闭，同一时刻只留一层浮层）——宽 `min(840px, 94vw)`、高 `min(680px, 88vh)`（**定高，不用 min-/max-height**：各分组字段数不同，由内容撑高会让弹窗随分组跳动；超出高度的分组在右栏内部滚动，头部与底部保存栏不动）、surface-container 底、28px 圆角与既定 scrim。三行结构：头部为标题 + 关闭按钮；主区为 `176px | 1fr` 两列 grid（行高 `minmax(0, 1fr)`，否则列内 `overflow-y` 不生效）——**左分组栏**按 `components/settings/sections.ts` 的顺序列出分组（通用 / 外观 / 图片与内容 / 小说翻译 / 高级 / 维护，文案键 `settings.groups.*`），条目沿用应用侧栏的导航配方（primary-container 选中、8% primary hover、focus-visible primary 2px 外环）按弹窗密度压到 40px 高 / 20px 胶囊、宽不扣边距，与右栏以 outline 35% 的竖直 divider 分隔；两栏各自滚动，右栏承载 SettingsPanel 的当前分组字段；**底部为常驻保存栏**（divider + 右对齐「取消」「保存」），不随表单滚动，保存为整表一次写入（分组建模不设分组级保存）。Esc、点 backdrop 或标题栏 ✕ 关闭前先做脏检查：有未保存改动时弹第二层 `360px` 窄确认弹窗（标题「未保存的修改」+「继续编辑」/「放弃修改」，与登录/退出确认同配方），确认放弃即回滚到已保存值并提示；无改动直接关。640px 以下左分组栏改为横排可滚的胶囊行、表单落下一行。层级低于 snackbar。
- alert、状态 pill、表格和表单卡是小型本地样式，复用 M3 颜色角色和上述 shape/spacing，不另建组件库。
- 公共分页 AppPagination：跨域分页控件（`components/common/`），能力对齐「总数回显 / 每页容量下拉 / prev-next / 页码窗口 + 省略号 / 快捷跳页（对齐 antd showQuickJumper）/ 响应式收缩」，受控无状态（`update:currentPage` / `update:pageSize` / `change({page, pageSize})`，父级可 v-model；容量切换回第 1 页由父级处理，组件只 emit）。数据模式：`total` 与 `totalPages` 二选一（内部换算 pageCount，同时给时 totalPages 优先），两者皆缺省为「未知总页数」简单模式——无页码窗口、恒显「第 N 页」、next 禁用跟随 `hasNext`（默认 true）。默认形态一行四段：左侧总数「共 {total} 项」（14px/500 on-surface-variant，`#start` 插槽可整体覆盖，供排行榜名次区间）→ 中部/右侧 prev/next + 页码窗口 → 行尾跳页输入组（`margin-left: auto` 推至行尾）→ 最右容量下拉（紧贴跳页输入组右侧，两者成对占据行尾）。行内控件统一 32px 控制高度（以 `--space-lg × 2` 表达，行容器 flex `align-items: center` 垂直居中；640px 以下收缩态同规）。给 `pageSizeOptions` 才渲染容量下拉——原生小号 `<select>`（32px 控制高度、13px 字、surface-container 底、outline 45% 描边、`--radius-control` 圆角，风格与 reader 变体跳页 select 统一，选项「{n} 条/页」）；跳页输入组仅在 default 变体且已知总页数 >1 时渲染（单页与「未知总页数」模式不显示）——`<label>` 内联「跳转至」+ `<input type="text" inputmode="numeric">` + 「页」三段，输入框宽 48px（`--space-xl × 2`）、高 32px、13px 字居中、surface-container 底、outline 45% 描边、`--radius-control` 圆角（与容量下拉同 recipe），focus-visible 为 primary 2px 外环（offset 2px）；Enter 或 blur 时解析输入——`Number.parseInt` 落在 `1..pageCount` 则按既有 `update:currentPage` + `change` 事件链跳页并清空输入框，越界 / 非数字只清空不导航；组件整体禁用时输入框一并禁用、组内文案降 outline 色；prev/next 为自绘 32px 胶囊 icon button + 20px chevron（stroke 2，hover 8% primary 状态层，禁用 `‹ ›` 文本字形）；页码为高 32px、`min-width` 32px 的自绘胶囊按钮（999px 圆角、14px/500），选中 secondary-container 底 / on-secondary-container 字并以 `aria-current="page"` 标记，未选中 hover 为 8% primary 状态层，禁用降为 outline 色文字；pageCount ≤ 7 全显，否则首尾恒显 + current±2 窗口 + 纯文本「…」（「…」本身不可点）。响应式：640px（compact 断点）以下隐藏页码窗口、显示「{current} / {total}」（未知模式仍为「第 N 页」），prev/next、跳页输入组与容量下拉保留（随 flex-wrap 自然换行）。`variant="reader"`：吸底居中紧凑形态，复用小说阅读器翻页器 recipe（sticky 底部、surface 底 + 上缘 divider、prev/next md-icon-button + 跳页 select「第 X / Y 页」、surface-container 底 `--radius-control` 圆角），不显示页码窗口与总数区。reader 变体为三列 grid（`1fr auto 1fr`），翻页组显式 `grid-column: 2` 恒居中，第 1 列为可选 `#leading` 插槽（小说阅读器在此外挂字号缩放控件 `[−] [百分比] [+] [重置]`），第 3 列为可选 `#trailing` 插槽（`justify-self: end` + `min-width: 0`，小说阅读器在此外挂阅读进度条，见下条），两插槽皆缺省时与纯居中视觉一致。过渡仅 0.15s ease、无阴影；自绘控件 focus-visible 为 primary 2px 外环（offset 2px）。

**控件密度.** Material Web 的 `md-outlined-text-field` 与 `md-outlined-select` 统一为 40px 高、14px 输入文字（组件默认 56px / 16px），与全局 14px 正文和 40px 的 choice 行对齐。实现只做尺寸 token 覆盖（`frontend/src/styles/main.css` 的「控件密度层」）：上下内距 8px（文本框 `--md-outlined-text-field-top/bottom-space`；选择器无容器高度 token，经内嵌 field 继承 `--md-outlined-field-top/bottom-space`）、输入字号 14px（选择器走 `--md-outlined-select-text-field-input-text-size`）、输入行高钉 24px（8 + 24 + 8 = 40px）。按钮维持 Material 默认的 40px 高 / 14px 标签字，不做覆盖。密度层不改颜色、圆角、描边、浮标等组件外观（Native-First Rule）。

### 浏览模式组件（ADR 0012）

浏览（browse）页面在既有 M3 体系上新增以下本地组件，全部复用现有颜色角色与间距刻度，不引入新 token：

- **作品卡 WorkCard**：封面圆角 `--radius-control`（12px）、无阴影；标题 14px/600 最多两行省略，作者 12px on-surface-variant；左上角完整胶囊徽标（999px、12px/600）：页数（surface-container/ink，>1 时显示「12P」）与 R-18/R-18G（ink 底/surface 字，同时出现时 R 系优先）；小说封面右下角 12px 小书角标。hover 为 8% primary 状态层，focus-visible 环保留。整卡可点击；封面右上角快捷动作（见下条）只在浏览频道页卡片出现，收藏页卡片可渲染同规格的「取消收藏」心形动作（仅条目带 bookmarkId 时），其它网格不渲染。
- **卡片快捷动作**：28×28、圆角 999px、`surface-container` 底 / `ink` 图标、16px lucide 官方路径线性图标（stroke 2）；hover 8% / active 12% primary 混合（与卡片状态层同源），`focus-visible` primary 2px 外环；默认 `opacity: 0`，hover 或 `focus-within` 显现，过渡 0.15s，`prefers-reduced-motion` 下取消；按钮必须有 `aria-label` 与 `title`；动作按钮与整卡可点击元素为兄弟节点，不得嵌套在 `role="button"` 内。
- **作品网格 WorkGrid**：`repeat(auto-fill, minmax(160px,1fr))`，gap 12/16px（紧凑相关推荐变体 120px）；骨架为纯 surface-container 色块（**不做闪烁动画**）；空态带插画占位与引导文案；错误态给可读文案 + 重试；「没有更多」收尾。
- **分区与 Tab**：频道页分区标题 16px/600 on-surface-variant；「按标签推荐」板块（仅插画频道返回）标题中 `#标签名` 升为 on-surface（`--ink`）强调、后缀「的推荐插画作品」沿用辅助色，板块位于每日排行之后、最新投稿之前，每板块 ≤12 条并纳入 R-18 渲染期过滤（整板块被滤空则整体隐藏，隐藏数并入合计）；类型/周期切换用 md-tabs（secondary），排行前三名徽标用 primary-container 突出。
- **频道 R-18 快捷筛选**：频道页顶部一行「内容筛选」+ 三枚胶囊选项（全部 / 一般向 / R-18）；胶囊与发现页过滤 chips 同一 recipe——透明底、`outline` 60% 的 1px 描边、`--ink` 文字、13px/600，选中态 `secondary-container` / `on-secondary-container` 且描边透明，hover 为 8% `on-surface` 状态层，间距取既有 4–12px 刻度，不新增 token。选中态以 `aria-pressed` 表达，键盘可达，`focus-visible` primary 2px 外环。默认跟随全局 `show_r18`，手动选择只覆盖当前频道页（插画 / 漫画 / 小说三档各自独立、不串档），不改写设置。切入/切出 R-18 时加载对应服务端频道快照（R-18 使用 `mode=r18`，全部/一般向复用普通快照），加载中清除旧档卡片并展示既有骨架，失败显示既有错误与重试；只接收最新请求结果。作品仍按当前频道档在渲染期过滤，WorkGrid 不再叠加全局过滤；标签推荐使用同档响应（不硬编码标签），完整榜单与标签搜索继承内容档位。隐藏计数沿用辅助行层级（12px、`--ink-subtle`（outline 角色）），不升为标题层级、不占用卡片徽标；整页被滤空不改变分页判定。
- **追更列表**：官方 /following/watchlist 同构——「类型」标签行 + 漫画/小说 md-secondary-tab（SectionTabs）；行式系列卡 `repeat(auto-fill, minmax(360px,1fr))` 网格，卡内 2:3 封面 112px（`--radius-control`）+「系列作品」overline（12px on-surface-variant）+ 标题 16px/600 两行截断 + 作者行（20px 圆头像 + 12px 名字）+「N 话 · 日期」元信息（12px）；主行动「读最新话」为 filled 按钮（34px 高）且与整卡点击同动作（缺失最新话时禁用），动作行与可点击区为兄弟节点、缩进对齐信息列（640px 以下取消缩进）；两类卡均附「系列目录」（应用内系列分集页：novel 卡 → novel 段、manga 卡为追更语义映射 illust 段）；hover 8% primary 状态层、无阴影、无新增动效；追更为用户主动订阅，页面不做 R-18 渲染期过滤；骨架为纯 surface-container 色块，空/错误态沿用居中文案 + 重试层级。
- **全屏翻页输入**：图片浮层支持滚轮下 / PgDn 前进、滚轮上 / PgUp 后退，单页一步一张，双页一步一组，不随左右阅读方向反转；首尾不循环。滚轮翻页以 250ms 间隔限速，胶卷区域仍可原生滚动，并由统一规则响应 PgUp/PgDn；Ctrl/Meta 滚轮与水平滚动不翻页；输入框与带 Ctrl/Meta/Alt/Shift 的 PgUp/PgDn 不触发翻页。沿用现有页码 aria-live、按钮 disabled 与退出焦点恢复，不增视觉控件或动效。
- **查看器舞台**：整页路由，舞台**无自有底色**（`background: transparent`，即页面底色——浅色主题为白、深色主题跟随 surface），图片 object-contain 居中；图片满幅（滚动区上/左/下 padding 为 0、页框直角），整体圆角由舞台 16px 圆角 + `overflow: hidden` 承担。多页作品为纵向滚动舞台（单页与 R-18 遮罩态整幅在舞台内垂直居中）：逐页排列、滚动到视口附近才加载，未加载页为与该页纵横比一致（缺省 2:3）的 `surface-container` 色块、先在占位块上铺低清层再换详情档；点击任意页进入全屏浮层，**页框本身即全屏入口**——可聚焦（`tabindex=0` + `role="button"`，Enter / Space 打开浮层并定位到该页，`focus-visible` 为 primary 2px 内环），R-18 遮罩态不可聚焦（`tabindex=-1`），右下角为「第 N / M 页」深色胶囊徽标（`rgb(0 0 0 / 0.78)` 底 + 白字、12px/600，多页时出现，浮于图片之上的阅读器惯例），舞台可聚焦、↑/↓ 原生滚动，`prefers-reduced-motion` 下取消平滑滚动。舞台滚动条与右侧信息列滚动条同配方：6px 常显、thumb 为 `color-mix(outline 40%)`（hover 60%）、track 透明；滚动区右侧留 6px 内边距（与信息列同配方），图片与滚动条不贴靠，左侧满幅不缩进。R-18 遮罩为 `blur(24px)` + `rgb(0 0 0 / 0.4)` 洗层 + 中央白字 + filled「显示」按钮，确认后本会话记忆——**仅在设置关闭 `show_r18` 时出现**（作为深链直访兜底），开关开启（默认）时详情页直接展示、不显示确认按钮；遮罩期间只加载首页模糊图、不可进入全屏浮层。深色背板仅存在于全屏浮层（图片浏览器唯一保留的深色例外，不得扩散到普通内容容器）；查看器舞台与页面同底色。
- **全屏浮层（查看器）**：全屏近黑浮层内页框按「可用高度（视口高 − 浮层上下内边距）× 该页纵横比」定尺寸——纵向页撑满高度、横向页撑满宽度，小图同样放大到该尺寸（不受原始像素限制）；单页一张、双图模式并排两张（8px 间距）。底部控制条（`‹ ›` + 页码 + 双图开关 + 阅读方向开关）与右上角 ✕ **默认隐藏**（opacity 0 且不响应指针），指针进入底部整条 / 右上角热区或键盘聚焦时以 0.15s 过渡显现，触摸设备常驻、`prefers-reduced-motion` 下取消过渡，Esc 或点击图片外关闭；控制条底为近黑胶囊、`‹ ›` 与页码居中、模式按钮走 md-icon-button 的 toggle/selected。双图模式下跨页按 1-2 / 3-4 对齐、翻页步进 2 页、页码显示区间、胶卷标出整组跨页，阅读方向可切「从右往左」（默认）/「从左往右」（会话内记忆、不持久化）；**从右往左时翻页组整组镜像**——前进按钮落到左侧、`‹ ›` 箭头同步改朝左，键盘 ← 为前进，右侧「双图 / 阅读方向」开关不是方向性控件、不参与镜像。右侧为胶卷缩略图列（固定 80px 宽、缩略图 64px 方形 `--radius-control` 圆角——列内出现纵向滚动条时按剩余宽度收窄、始终方形且不出横向滚动条、`thumb_quality_grid` 档、列内 8px 间距、自身可滚动，滚动条为 6px 常显样式、不随闲置隐藏），屏幕上的页以白色 2px 描边标记（近黑底上的既定白，不用 primary）、`aria-current` 表达主选页，点击直达该页；翻页时当前缩略图自动滚入视野；胶卷仅多页时出现，窄窗不隐藏。
- **小说阅读器**：整页按主工作区可用高度（100cqh，排除状态栏 / 底部面板）三行 flex——顶栏（非 sticky，天然贴窗口上边）/ 中间唯一滚动层 `.reader-scroll`（`flex:1; min-height:0; overflow-y:auto`，6px 细滚动条，与查看器/信息列同配方）/ 底栏 AppPagination `variant="reader"`（flex 尾行贴窗口下边，组件 recipe 不变：surface 底 + 上缘 divider、prev/next + 页码下拉直选）；顶底栏均贴窗口上下边，只有中间层滚动，键盘 ←/→ 翻页由视图层自理。正文列默认**不限宽**（铺满中间区），仅 `@media (min-width: 1921px), (min-height: 1081px)`（即大于 16:9 1080p 的屏幕）限 1280px 保持行宽可读——1920×1080 及以下满宽，更大屏左右留白不再过宽（列内边距 `--space-lg` 16px，100% 字号下约 78 个全角字/行）；正文 16px 基准 × `--novel-scale`（`calc(16px * var(--novel-scale, 1))`，on-surface），`[chapter:]` 渲染为 1.15em/700 章节标题。**正文内嵌图**：`[uploadedimage:id]` 由详情响应 `embedded_images`（`id → pximg URL`）出图，整行成块、居中、`max-width:100%`、`--radius-control` 圆角、上下 `1.5em` 间距、`loading="lazy"`，经 `pixiv-img` 协议代理显示（与封面同缓存）；混排段落内的内嵌图降为行内小图（`max-height:1.6em`、`vertical-align:middle`、左右 `0.25em`），不打断行高；取不到 URL 的标记（未收录 id / `[pixivimage:illustId]`）保留既有虚线占位块（`--radius-control`、outline 45% 虚线边、`--ink-muted` 12px 图标 + 文案 `browse.novel.imageUnsupported`），不静默丢图。图片 alt 为 `browse.novel.imageAlt`。`--novel-scale` 来自设置键 `novel_font_scale`（默认 1.0、区间 0.75~2.0，越界/非数字由后端拒绝、加载期回落 1.0）；底栏左侧经 AppPagination reader 变体的 `#leading` 插槽挂字号缩放控件 `[−] [百分比] [+] [重置]`（步进 0.1、到界禁用，重置回到 100%（100% 时禁用）；百分比 `aria-live="polite"`，i18n `browse.novel.fontSmaller` / `fontLarger` / `fontReset`）。底栏右侧经 `#trailing` 插槽挂**阅读进度条**：`md-slider`（min 0 / max 100 / step 1，宽 200px、640px 以下收窄 140px）+ 滑杆右侧百分比回显（`--ink-muted` 13px、44px 定宽防跳变抖动，百分比本体对读屏隐藏、值由滑杆的 aria-label `browse.novel.readProgress` + valuenow 播报）。进度语义是**当前页内滚动位置**——`scrollTop / (scrollHeight - clientHeight)` 四舍五入为百分比，随滚动被动监听实时回显，`ResizeObserver` 观察滚动层与正文列以跟踪高度变化（内嵌图懒加载 / 字号缩放 / 面板切换 / 窗口伸缩），加载与切页在 DOM 就绪后重挂观察并回算；滚动层无余量（短内容）恒显 100%。拉条拖动即时定位：input 事件按百分比 `scrollTo` 正文位置并**乐观同步回显**（`progressPercent` 随 input 直接更新），scroll 事件回声仅作确认——回声值与乐观值恒等（step 1 整数对齐）、无反馈环，程序化 `scrollTo` 不派发 scroll 事件的环境（窗口不可见 / 渲染节流的 webview）显示也不漂移；切页 / 重载回正文顶部时进度同步归零（回正文顶部）。滑杆视觉只落既有角色：handle（含 hover / focus / pressed）与 active track 用 primary，inactive track 用 `color-mix(outline 30%, transparent)`（与底栏上缘 divider、滚动条 thumb 同配方），handle 阴影色置透明——包内默认引用的 surface-container-highest / shadow 角色本应用未定义，不映射会漏出浅色兜底；高度经 `--md-slider-state-layer-size` 收敛到底栏 32px 控件约定。键盘 ←/→ 在滑杆聚焦时归滑杆调值：滑杆焦点在其 shadow `<input type="range">`，window 侧事件 target 已重定向为宿主，视图层翻页守卫以 `composedPath()[0].closest("input, select, [role=slider]")` 放行。正文之后为相关推荐 / 评论面板（见上条），顶栏动作为返回 + 标题 / 作者 + 收藏 / 评论 / 返填表单 / 在浏览器中打开。
- **详情信息与标签**：插画/漫画沿用右侧信息列，小说在信息头元信息行回显点赞/收藏/浏览数（12px 辅助层级，零值保留、千位格式化），简介放在元信息之后、正文之前；简介使用既有正文颜色与字号，纯文本保留换行与段落（`white-space: pre-wrap`、`overflow-wrap: anywhere`），不渲染外部 HTML。标签沿用既有胶囊配方，元素为原生搜索链接（无下划线、键盘与新标签打开可用），携带标签原文、作品类型与精确标签匹配；hover 沿用角色状态层，focus-visible primary 2px 外环（offset 2px），长标签换行。计数行可换行，空简介与标签不生成空分区；无新增颜色、图标或动效。
- **小说阅读背景色板**：小说阅读器底栏「文字大小」控件右侧的**阅读背景色块按钮**（`NovelBgPicker`）所用纸色配对，设置键 `novel_bg_color`（语义键，空串 = 跟随主题）。触发器为 `md-icon-button` 内嵌 **18px 圆形色块**回显当前色（跟随主题时 = surface 底 + outline 1px 描边）；点击在按钮上方**水平居中**弹出一排气泡——弹出层与账号菜单 popover 同 recipe：surface-container 底、`--radius-control` 圆角、唯一合法轻阴影 `0 4px 12px rgb(0 0 0 / 18%)`、透明遮罩（无 scrim）点击外部关闭 + Esc 同效、0.15s ease 上浮过渡（transform-origin 底部中心，reduced-motion 由全局兜底）、焦点移入弹出层（`tabindex="-1"`）、关闭后回触发器。气泡为 **28px 圆形色块一行**（`role="radio"` + `aria-checked`），底色即纸面色本身（默认态 surface 底 + outline 描边），选中态 primary 2px 外环（offset 2px），点击立即应用并经 `settings_save` 持久化。五档「纸面 bg + 正文 ink + 次级 muted」配对：

  | 键 | 纸面 bg | 正文 ink | 次级 muted |
  |---|---|---|---|
  | `green` 护眼绿 | `#c8e6ce` | `#1f3a2e` | `#52685c` |
  | `kraft` 牛皮纸 | `#e6d7b8` | `#433722` | `#75684d` |
  | `warm` 暖杏 | `#f3e4d0` | `#463526` | `#7c6753` |
  | `mist` 雾蓝 | `#e1ebf2` | `#263844` | `#5c7180` |
  | `blush` 藕粉 | `#f5e7e5` | `#46302f` | `#7f6462` |

  每档配对自含、**不引入新颜色 token**：选中后整页根节点（`.novel-view.read-bg-*`）落 `background` 纸面色 + 正文 `color` 墨色，并在该子树重映射既有角色——`--md-sys-color-surface` 与 `--surface` = 纸面色，`--md-sys-color-on-surface` 与 `--ink` = ink，`--md-sys-color-on-surface-variant` 与 `--ink-muted` = muted，`--md-sys-color-surface-container` = `color-mix(in srgb, ink 6%, 纸面色)`，`--md-sys-color-outline` 与 `--ink-subtle` = `color-mix(in srgb, ink 45%, 纸面色)`——卡片底、分隔线、滚动条 thumb 等派生色自动跟随；作用域为阅读器整页根节点，顶栏 + 正文 + 底栏一起落纸色。暗色主题下同样以浅纸面呈现——这是阅读模式的刻意例外，配对自含、恒可读，不得把纸色扩散到阅读器以外的页面。
- **系列分集页**：路由 `/browse/series/:kind(novel|illust)/:id`（旧 `/browse/series/:id` 函数式 redirect 到 novel 段；watchlist 的 manga 是追更语义、入口映射为 illust），小说与插画/漫画系列共用一套展示。头部：120px 方形封面（`--radius-control`；illust 系列封面空串时回退第一话封面）+ 标题（单行省略、640px 下换行）+ 作者链接 + 简介剥 HTML + 「共 N 话 · 状态」统计行（12px/600 on-surface-variant；illust 端点无完结标记恒显示连载中）+ 动作行（小说「返填表单」+「在 pixiv 打开」，插画/漫画仅「在 pixiv 打开」）。头部之下为工具行：左侧「系列目录」标签（12px/600 on-surface-variant），右侧宫格/列表切换复用公共 `ViewModeToggle` 组件（`browse/ViewModeToggle.vue`，lucide `layout-grid` / `list` 官方路径、stroke 2，30px 胶囊按钮 + 18px 图标；选中 `secondary-container` 底 / `on-secondary-container`，未选中透明底 + `ink`、hover 8% on-surface；`aria-pressed` + `aria-label`、focus-visible primary 2px 外环；会话内记忆，默认 illust → 宫格、novel → 列表）。宫格 `repeat(auto-fill, minmax(160px,1fr))`（与 WorkGrid 同参）：illust 封面卡 = 1:1 封面 + 左上角 `#N` 序号胶囊（999px、12px/600、surface-container/ink，与作品卡页数徽标同 recipe）+ R-18/R-18G 右上角徽标（ink 底 surface 字）+ 标题两行截断 + 页数/日期副行；novel 目录瓦片卡 = surface-container 底、20px/600 大号 `#N` + 标题两行截断 + 字数元信息，hover 8% primary 混合。列表 = 48px+ 行式（surface-container 圆角容器 + 行间 divider + hover 8% primary；illust 行加 48px 小封面与 `#N` 序号，novel 行保持两位序号/标题/元信息右对齐，640px 下标题换行、元信息落行尾）。翻页器为 AppPagination `variant="reader"`（与小说阅读器同一吸底形态：sticky 底部、prev/next + 页码下拉直选、边界禁用），novel 30 条/页（`last_order=(page-1)*30`）、illust 12 条/页（官方页码制）；切页保留旧内容降透明局部过渡、禁止闪烁，首屏骨架为纯色块无动画。R-18 全局过滤只作用于分集条目（宫格/列表同口径），头部不受影响。入口约定：追更列表卡、作品/小说详情系列导航、搜索 ID 直达（`novel/series/{id}` → novel 段、`user/{uid}/series/{sid}` → illust 段）一律跳本页。
- **资源列表头部与刷新**：首页、插画/漫画/小说频道、发现、动态、追更、搜索、排行榜、收藏、浏览历史、作者作品与系列目录提供「刷新」；标题行左标题、右动作，允许窄窗换行，无标题的作者/系列刷新行右对齐。辅助刷新统一 `ListRefreshButton`：`md-text-button` + 18px 线性刷新图标，既有间距、无新 token；请求中显示「刷新中…」、禁用并标记 `aria-busy`，tooltip 提示 Ctrl+R / Command+R，`aria-keyshortcuts` 同步声明。快捷键只作用于激活列表，模态弹窗打开时不刷新背后列表；刷新保留筛选并回到顶部，无限列表从首批重载、页码列表保留当前页。首页「换一批」保留为右侧独立追加动作。
- **返回导航**：非作品页面共用 `PageBackButton`：40×40 `md-icon-button`、20px lucide `chevron-left` 官方路径（stroke 2）、`on-surface-variant` 颜色，不显示常驻文字、不单独占一行。普通列表与以图识图放在标题左侧（`.page-heading` flex、居中对齐、`--space-sm` 间距、标题 min-width:0 允许长文换行）；作者页放在头像资料区左侧，居中对齐 72px 头像（16px 上间距，窄窗保持在资料列左侧）；系列页并入系列标题行；工具页放在唯一分区标题左侧。作品查看器与小说阅读器保留现有顶栏返回位置。所有入口共享原生路由历史，逐层回到实际来路；相关推荐同样压栈。tooltip 与 aria-label 为「返回上一页」，无应用内历史时为「返回所属分区」或「返回首页」，以 replace 返回分区核心入口（核心入口回首页）避免循环，首页无历史时隐藏入口。返回不因加载、空态或错误态消失，骨架的 aria-hidden 不包含返回按钮；沿用 Material 的 hover、focus 与键盘操作，无新增 token 或动效。
- **返回恢复**：浏览列表、作者与系列目录在会话内最多缓存 20 页（LRU）；进入详情再返回恢复内容、筛选、已加载分页与主内容滚动位置，不闪首屏骨架、不自动重载。账号/登录状态变化清空会话缓存；缓存页面停用时暂停无限加载与刷新快捷键。作品查看器与小说阅读器本身不缓存。首页首次挂载可恢复同账号 24h 内的最多 120 张卡片快照，并后台刷新；刷新中保留卡片、失败保留旧数据并显示错误，未确认账号不展示快照。
- **列表图片加载**：WorkCard 先显示 small 封面，小图完成后升级到所选网格档位，升级期间保持小图、高清失败保留小图并尝试接口原始 URL；small 档及相同 URL 不重复下载。WorkGrid 前 12 张小图 eager/high，其余 lazy，高清升级 low 优先级；复用现有封面宽高比、底色与圆角，不增加模糊滤镜或动画，避免占位尺寸变化。
- **登录守卫联动**：浏览命令未登录返回固定文案并自动打开既有登录弹窗；浏览页面本身不重复实现登录 UI。
- **收藏页布局**：类型 md-tabs 按内容定宽，公开/私密胶囊置于控制行右侧，窄窗自然换行。标签为全宽、随内容占位的横向换行工具带，不再保留 180px sticky 侧栏；36px 标签胶囊以 surface-container 为底，选中 primary-container/on-primary-container，名称与 12px/600 计数并列，空名显示「未分类」。超过三行（132px）时内部滚动，标签名称最多 192px 省略并保留 title；按钮有 aria-pressed 与 2px primary 焦点环。下方总数行 + 全宽 WorkGrid。取消收藏与作者公开收藏语义不变。
- **搜索页筛选行与本页排序**：工具行之下为筛选行——类型 md-tabs + 三个 `md-outlined-select`（排序 / 对象 / 匹配）+ 行尾「本页排序」组，每个下拉配 12px/600 `--ink-muted` 外部标签；下拉按选中项文本定宽且**不收缩**（flex: none——Material 内部标签在过窄宽度下会换行、把下拉顶高到 64px），正常窗口恒为单行，窄窗（≤1040px）先把本页排序组换到第二行、≤640px 纵向堆叠。结果行仅保留左侧「约 N 件」+ 计数补取进度（本页排序不再单占一行）。**本页排序组**：维度 `md-outlined-select`（`default` / 点赞数 / 收藏数 / 浏览数，默认 `default`——value 必须非空，空串选项 Material 不识别会渲染成空白下拉）+ **SortDirectionToggle**（`components/common/`，可复用）——升/降两枚 30px icon toggle（lucide `arrow-up-narrow-wide` / `arrow-down-wide-narrow` 内联路径、18px、stroke 2），选中态 `secondary-container` / `on-secondary-container`、未选中透明底 + `--ink`，hover 8% `on-surface` 状态层，`aria-pressed` + `aria-label`（升序 / 降序）、组级「排序方向」标签、`focus-visible` primary 2px 外环、`prefers-reduced-motion` 去过渡；未选择排序维度时整体禁用。排序只作用于当前显示页 20 条：三项计数经 `browse_work_counts` 分批补取（进度「正在获取本页计数 n/m…」，缺失项垫底），维度 / 方向切换复用已缓存计数、不重复请求。
- **搜索页分页网格列数**：页码分页模式的 WorkGrid 每页条数固定（搜索页 20 条），列数只取该页数的因数——**2 / 4 / 5 / 10 列**，保证任何响应宽度下每行都排满、末行不残缺。档位按外层查询容器（`.grid-host`，`container-type: inline-size`）宽度切换，阈值 608 / 764 / 1544px（= 列数 × 140px 最小卡宽 + (列数−1) × 16px 列间距）；无容器祖先时退回 2 列。纯 CSS `@container`，不监听 resize。列数变化不影响每页条数与翻页（接口仍每页 60 条切 3 个显示页）。
- **浏览历史页**：「我的 / 浏览历史」入口的宫格页——顶行返回 + 标题 + 右侧「刷新」与 danger「清空历史」（无历史时禁用）；宫格 `repeat(auto-fill, minmax(160px,1fr))` + WorkCard（自绘网格，页码分页，不用无限滚动 WorkGrid）；分页走公共 AppPagination 的 default 变体（总数回显 + 每页容量 20/50/100，默认 20，切换容量回第 1 页）；「清空」经原生 `dialog` 单行正文式二次确认（与历史/任务删除确认同配方）后上报清空、回第 1 页并重载；骨架为纯 surface-container 色块（无动画）、空态带插画占位与引导文案、错误态给可读文案 + 重试；全局 R-18 开关关闭时按 `x_restrict` 在**渲染期**过滤（不改数据、不改分页 `total`，与本项目其它列表同口径），整页被滤空时在空态下补隐藏计数辅助行（12px、`--ink-muted`）；顶栏下另有类别筛选胶囊行（全部 / 插画 / 漫画 / 小说，默认全部，同频道页「内容筛选」recipe），切换回第 1 页并重新加载，分页 `total` 用过滤后的值；页面被 KeepAlive 缓存，每次激活（进入页面）`onActivated` 自动重新加载，筛选与页码状态保留。
- **详情页收藏按钮**：顶栏触发器对齐 md-outlined-button 规格（40px 胶囊、outline 45%、primary 文字、18px 心形）：未收藏 = 空心 + 「收藏」；已收藏 = 实心 + 「已收藏」，私密收藏追加「私密」角标（surface-container 底 / ink-muted 12px/600 胶囊）；点击弹原生 `details` 弹出菜单（surface-container、`--radius-control` 圆角、唯一合法轻阴影）：未收藏 → 公开收藏 / 私密收藏，已收藏 → 取消收藏；请求进行中禁用（`aria-busy` + 降透明度）。点击菜单外或 Escape 关闭。
- **详情页顶栏动作（查看器 / 阅读器共用）**：带文案的动作保持 40px 胶囊（收藏，见上条），图标动作一律 `md-icon-button`（40×40、无描边、`on-surface-variant` 图标、选中态用 Material 的 toggle/selected），图标统一为 20px lucide 官方路径线性图标（stroke 2）：顺序为「收藏 / 评论 / 返填表单 / 在浏览器中打开」。**禁止把只有图标的 `md-outlined-button` 当图标按钮用**（其左右各 24px 内距会把单个图标撑成约 66px 宽的宽胶囊），也禁止用 `‹ › ✕` 文本字形充当图标；页码翻页器统一为公共 AppPagination（自绘控件，chevron 同为 20px lucide 官方路径 stroke 2），排行榜日期切换用 `md-icon-button` + 20px chevron，同规，`md-text-button` 只留给带文案的低强调动作。
- **详情页面板（相关推荐 / 评论）**：查看器右列下段与阅读器正文列下段是同一个可切换面板——默认「相关推荐」，顶栏评论按钮（`md-icon-button` 的 toggle + selected：选中为实心气泡 + primary，标签在「查看评论」与「返回相关推荐」间切换）切到「评论」，再点切回。评论面板**挂载时才发起请求**（打开即拉第一页），roots 走「加载更多评论」分页、回复展开后按页续拉；回复、表情贴图与纯文本渲染沿用评论区既有规格。切换面板时把面板滚入视野（阅读器 `block: start` + 72px 吸顶余量，查看器 `block: nearest`）；作品切换（kind / id 变化）时面板回到「相关推荐」。

**The Native-First Rule.** 已由 Material Web 覆盖的按钮、输入、选择、复选、单选、标签页和进度条不重写外观；原生 `dialog`、`details` 和表格只补充当前实现所需的容器样式。


### 核心导航与下载工作区（ADR 0015）

- **作者关注（ADR 0016）**：作者资料行在昵称/统计与网页按钮之间提供关注按钮：未关注为 md-filled-button「关注」，已关注为 md-outlined-button「已关注」（tooltip/aria-label 为取消关注），40px 既有控件密度。公开关注，点击已关注直接取消；成功后回写 is_followed 并 Snackbar 提示，失败保持原状态。提交中禁用、防重并 aria-busy；未知状态禁用并提示刷新，本人资料不显示关注自己。按钮与网页入口为 8px 间距动作组，窄窗随资料区落行。账号切换或作者对象变化后忽略旧请求回显，刷新不与关注操作并发，不重载作品列表。

- **页面层级与间距**：列表页按「返回与分区标题 + 右侧操作 → 分区导航 → 本页筛选 → 内容」组织。首页/发现、关注、我的分别以核心分区为标题，避免重复当前导航名称；频道保留具体标题。二级导航位于各列表页内，沿内容左边缘对齐，用原生路由链接胶囊标明当前页，发现的频道入口以细分隔线分组；详情不额外堆叠分区导航，侧栏仍继承来路。导航组间 16px，导航到下一区域 24px；页内筛选采用按内容定宽的 Material tabs，禁止均分整页宽度。关注类型与范围并排、窄窗自然换行。下载由父页面提供唯一标题和新建入口，下方为按内容定宽的任务/历史页签；任务类型筛选与批量管理左右分组，窄窗换行，清理完成任务并入管理组。

- 一级导航为发现 / 关注 / 我的 / 下载；侧栏仍为 256px / 折叠 72px、48px 胶囊项与账号底栏。发现默认首页推荐，页内推荐 / 为你发现、插画 / 漫画 / 小说频道和排行入口；关注提供新作品 / 系列追更；我的提供收藏 / 浏览历史；下载默认任务，页内任务 / 下载历史和新建下载。浏览原路由保留，详情导航高亮继承实际来路，直达默认发现。以图识图入口位于搜索页。
- 主工作区为内容 + 非模态下载面板 + 32px 底部状态栏。工作区容器宽度 ≥1120px 时面板右侧 480px（20 × space-xl）；否则底栏高 min(45%,320px)，内部滚动。surface-container 底、outline 35% 的 1px 分隔线，无阴影、无遮罩、不限制焦点。头部标题 18px、关闭为 40px md-icon-button + 20px lucide x；小说 / 插画页签用 --md-secondary-tab-container-color 继承面板 surface-container 底色，浅深色都不生成独立白色条；字段与来源选择沿用 Material Web，40px 密度。布局切换保持可见作品卡锚点或原滚动位置。
- 小说 / 插画共用面板，来源与格式沿用既有抓取语义。所有返填打开面板、URL 与历史不变；关闭 / 跳页保留会话草稿，账号变化清空。覆盖手动修改且非空的来源前原生 dialog 确认；格式保留。失败保留输入和错误，提交中禁用字段和重复提交；成功关闭面板、更新任务并用 Snackbar 提示，浏览页不跳转。
- 底部状态栏为独立布局行：32px（2 × space-lg）、12px 辅助文字、surface-container 底 / ink-muted；左侧进度区域按钮进入完整下载页，右侧新建下载打开表单。空闲也保留，显示无进行中任务；活跃任务计数包含 running / pending / paused，代表任务优先级同序、同状态按创建时间升序。小进度条 120px（5 × space-xl），640px 以下 48px 并隐藏来源摘要；已知总量按 done/total，未知总量不定进度、暂停停止动画。不汇总不同任务百分比。同步失败保留快照并回显重试状态。全屏看图隐藏底栏与全局搜索。
- 搜索为主内容右下角固定圆形图标按钮，md-filled-tonal-icon-button、56px 圆形容器（2 × space-xl + space-sm）与 lucide search 24px，无常驻文字，保留搜索 aria-label 与快捷键 tooltip；right space-xl、bottom 3 × space-xl。显示按钮时，主内容内所有 AppPagination 在右侧预留 72px（3 × space-xl，即 56px 按钮 + 16px 间距），分页控件自然换行，任意滚动位置都不进入按钮区域；搜索页隐藏，阅读器放顶栏。Ctrl+K / Command+K 进入搜索并聚焦输入，模态窗或全屏看图时不穿透；以图识图由搜索页文字按钮进入。
- 所有页面沿真实路由历史回退，无历史以 replace 返回所属核心入口、核心入口再回推荐首页，首次首页无返回。手动刷新保留；面板字段聚焦和模态窗打开时不响应底层刷新快捷键。面板关闭 / Escape 恢复触发器焦点，触发器已离开则聚焦当前页标题，不设焦点陷阱。默认 / hover / focus / disabled / loading / error、长文本、浅深色、中英文与 reduced-motion 沿既有角色和组件规则。

- **按屏滚动**：普通可纵向滚动的数据区（图片舞台 / 小说正文 / 列表 / 信息与评论 / 下载面板 / 侧栏 / 设置弹窗）统一支持 PgUp/PgDn 按屏滚动：鼠标所在位置的最内层实际滚动容器优先，无鼠标目标时跟随焦点，最后兜底主内容；按 Chromium/Windows 规则每次滚动容器可见高度的 87.5%，保留阅读重叠。首尾不循环、不串动相邻或父滚动区；模态窗口内不滚动背景。输入、下拉、滑杆、可编辑内容、已消费的按键及 Ctrl/Meta/Alt/Shift 组合交给原生控件或系统。全屏图片区仍按作品页 / 双页组切换，鼠标或焦点落在可滚动胶卷时 PgUp/PgDn 只滚胶卷。无新增视觉控件、颜色或动效。

- 返回控件与图片详情 Escape 使用页面注入的实际路由实例读取历史和执行回退，不依赖路由模块单例；完整路由变化后重新判定历史、按钮可见性和提示，热更新后仍按真实来源逐层返回。

设置弹窗中的主题、配色与语言可即时预览，但只在点「保存」后写入设置；「取消」或确认放弃修改时回滚到已保存值（含主题预览）并显示全局 Snackbar。设置分组：通用（应用打开默认进入 / 输出目录 / 输出格式 / 语言）、外观（主题 / 配色）、图片与内容（「图片与缩略图」列表·网格 / 详情页主图 / 大图·全屏三档，默认 medium / medium / large；「内容显示」R-18 开关，默认开启 `true`）、高级（SauceNAO API Key / 最大等待时间）、维护（清除日志 / 清除登录，行式「标题 + 提示 + 右侧动作」，立即执行、不受保存影响）。启动页使用同密度 md-outlined-select，提供发现（默认，推荐首页）/ 关注 / 我的 / 下载，提示「保存后下次打开应用生效」；保存或取消遵循整表语义。字段沿用既有 form / `fieldset` 结构与 40px 控件密度，不新增 token。

交互动画主要来自 Material Web 组件；项目全局在 `prefers-reduced-motion: reduce` 下将过渡和动画缩短至 0.01ms、禁止平滑滚动。焦点表现依赖 Material Web 的控件实现；自定义图标按钮提供 `aria-label`，任务筛选组、任务列表、设置表单、通知及告警已有对应的语义标签或角色。新自定义可操作控件必须保留等效的键盘可达性和名称。

## Do's and Don'ts

应用内更新使用 `.m3-dialog` 与 Material Web 按钮/进度条：确认、下载、启动安装、失败、取消、安装引导
在同一弹窗切换。宽 `min(480px, 90vw)`、最大高 `88vh`，文件名与路径允许任意处换行，统计行可换行、
数字等宽；进度、大小与速度文字同时展示。下载阶段 Esc 取消，启动安装期间不关闭；取消按钮请求中 disabled。
失败显示错误并提供重试/发布页，完成保留目录入口与三平台安装步骤，不把系统打开请求等同安装成功。
继续使用既有颜色、间距、圆角和减少动态效果规则，无新视觉 token。

**Do:** 使用 M3 颜色角色、Material Web 组件和既有 4–24px 间距；将失败、警告、进度与登录状态以文字和视觉共同表达；保留 640px 下的换行、纵向布局和表格滚动；尊重减少动态效果偏好。

**Don't:** 不要以旧版蓝白“书库”规范、Naive UI 主题或玻璃拟态覆盖 M3 系统；不要为普通卡片添加阴影；不要将 primary 用作语义状态颜色；不要删除可访问名称、原生控件语义或缩减动态效果规则。
