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
  error-container: "#FFDAD6"
  on-error-container: "#410002"
  warning-container: "#FFDDB2"
  on-warning-container: "#2A1700"
typography:
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "20px"
    fontWeight: 700
    lineHeight: 1.4
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
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
- Error 与 warning container：当前自定义 alert、任务状态使用已定义的浅色危险和警告组合；文字和颜色必须一起表达状态。完成状态目前为独立的绿色字面值，属于已有实现而非通用 primary 角色。

**The Semantic State Rule.** primary 不代表成功、失败或警告；错误、警告、进度与任务状态始终要有可读文字。

## Typography

全局字体为 `-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`，正文为 14px / 400 / 1.5。页面标题为 20px / 700 / 1.4；表头、状态 pill、提示和任务元信息使用 12px，常见权重为 600。字段标签为 14px / 500，浏览器地址文本和识别 badge 为 13px。

**The Truncation Rule.** 历史表格标题、作者和浏览器地址通过省略号处理；其余长文本不得以更小字号换取空间。表格中的标题和作者提供 `title` 属性保留完整值。

## Layout

应用外壳是全高双列 grid，恒为视口高（行高钉 100vh、`overflow: hidden`）：默认侧栏 256px，折叠后 72px，主内容占余宽且是唯一滚动容器（`overflow-y: auto`），长内容不会把侧栏拉长。侧栏头部行高 72px，含 logo、标题与收起按钮（折叠态仅留居中的展开按钮）；导航项最小高 48px，圆角胶囊宽度扣除左右各 12px，窗口高度不足时导航区自身滚动。普通主内容的内边距为 24px，内容列最大宽度为 1120px；抓取表单卡最大宽度为 600px，设置表单承载于模态设置弹窗（宽 `min(600px, 92vw)`，字段直排、不用表单卡容器）。

历史与任务列表在底部右对齐分页区提供每页 20、50、100 条的选择器；切换容量或分类回到第 1 页。历史页把容量传至数据库查询，任务页只对当前筛选结果分页，选择与批量操作仍保留跨页状态。

空间只使用 4、6、8、12、16、24px 刻度。工具栏与控制行可以换行；任务标题在 640px 以下变为纵向排列，历史搜索框与设置路径行扩展为全宽/纵向，Pixiv 浏览工具栏允许地址栏换至下一行。表格保持最小宽度 760px 并允许横向滚动。侧栏不会因媒体查询自动隐藏，用户通过头部行的折叠按钮切换（展开态位于标题行右端，折叠态独占头部行）。

**The Compact Task Rule.** 对齐、留白和换行优先服务于任务、历史和设置的扫读，而不是制造大面积空白。

## Elevation & Depth

日常层级通过 surface 与 surface container 的色调差、outline 分隔线和圆角表达；普通卡片、任务列表和表格没有阴影。snackbar 与详情页收藏操作的 details 弹出菜单使用 `0 4px 12px rgb(0 0 0 / 18%)` 的轻阴影（**唯一合法值**，不要再写 `12px/14px` 这类非法或变体写法；snackbar 历史上的 14px blur 已于 2026-10-01 收敛到 12px），原生对话框的 backdrop 为 `rgb(0 0 0 / 35%)`。

**The Overlay-Only Shadow Rule.** 阴影仅标示临时浮层，不应用于普通内容容器。

## Shapes

Material Web 控件继承库的 M3 外观。应用自定义的 control 圆角为 12px，卡片、表格和任务列表为 16px；左侧导航当前项为 24px；登录、确认等原生 `dialog` 为 28px。状态与类别标签使用完整胶囊形状（999px），头像为圆形。

## Components

- `md-filled-button`：主提交、恢复和确认操作；`md-outlined-button`：浏览、同步、批量删除等次要操作；`md-text-button`：取消、删除等低强调操作；`md-icon-button`：工具栏和行级图标动作。
- `md-outlined-text-field`、`md-outlined-select`、`md-radio` 与 `md-checkbox`：所有可编辑字段和选择；字段以标签、12px–16px 间距和至少 40px 的 choice 行组织。
- `md-tabs` / `md-primary-tab`：登录方式切换；`md-secondary-tab`（SectionTabs 封装）：浏览分区与工具页顶部页签（工具页页签即子路由导航）；`md-linear-progress`：任务进度。Material Web 负责它们的默认交互状态。
- 原生 `dialog`：登录、退出/删除确认与模态设置弹窗；容器与内部结构收敛于 `main.css` 的「原生 dialog 通用层」（`.m3-dialog`：surface-container、28px 圆角、24px 内边距、35% scrim、标题 18px/700；紧随标题的说明文字为 on-surface-variant，`.dialog-actions` 右对齐操作行、上间距 24px）。退出确认弹窗为「标题 + 说明」的 360px 窄弹窗；历史/任务的删除确认是无标题的单行正文式，正文保持主文案层级。自定义 `.m3-snackbar` 固定在右上角，通知在 3.2 秒后消失。页面不应另建成功提示条。
- 左侧导航：扁平菜单、无分组标题——浏览区在上、一条分隔线、最后是「工具」单项；图标加文字，hover 使用 8% primary 的状态层，active 使用 primary-container。账号区位于侧栏底部，为头像 chip（头像 + 账号名 + 向上箭头，hover 8% primary 状态层；折叠态居中只显示头像），点击在 chip 上方弹出账号菜单。
- 账号菜单：锚定头像 chip 上方的轻量 popover——宽 264px、surface-container 底、12px 圆角、既定轻阴影 `0 4px 12px rgb(0 0 0 / 18%)`，无深色 scrim，透明遮罩仅负责点击外部关闭（Esc 同效）；内容为账号列表（当前账号 ✓，点击切换）/ 添加账号 + 分隔线 + 「设置」入口（图标 + 文案 + 右侧箭头）与退出登录（danger 色文案，未登录置灰，经原生 confirm 弹窗确认后执行），菜单行高 40px、hover 8% primary 状态层。弹出过渡复用既有 0.15s ease、transform-origin 左下（reduced-motion 由全局兜底），层级低于 snackbar。
- 设置弹窗：账号菜单「设置」入口打开的模态 `dialog`（菜单先自行关闭，同一时刻只留一层浮层）——宽 `min(600px, 92vw)`、最大高 `min(84vh, 100%)`、surface-container 底、28px 圆角与既定 scrim；头部为标题 + 关闭按钮，SettingsPanel 表单区自身滚动。Esc、点 backdrop 或标题栏 ✕ 均可关闭，关闭前恢复未保存的主题预览；设置项增多时在弹窗内新增分组。层级低于 snackbar。
- alert、状态 pill、表格和表单卡是小型本地样式，复用 M3 颜色角色和上述 shape/spacing，不另建组件库。

**控件密度.** Material Web 的 `md-outlined-text-field` 与 `md-outlined-select` 统一为 40px 高、14px 输入文字（组件默认 56px / 16px），与全局 14px 正文和 40px 的 choice 行对齐。实现只做尺寸 token 覆盖（`frontend/src/styles/main.css` 的「控件密度层」）：上下内距 8px（文本框 `--md-outlined-text-field-top/bottom-space`；选择器无容器高度 token，经内嵌 field 继承 `--md-outlined-field-top/bottom-space`）、输入字号 14px（选择器走 `--md-outlined-select-text-field-input-text-size`）、输入行高钉 24px（8 + 24 + 8 = 40px）。按钮维持 Material 默认的 40px 高 / 14px 标签字，不做覆盖。密度层不改颜色、圆角、描边、浮标等组件外观（Native-First Rule）。

### 浏览模式组件（ADR 0012）

浏览（browse）页面在既有 M3 体系上新增以下本地组件，全部复用现有颜色角色与间距刻度，不引入新 token：

- **作品卡 WorkCard**：封面圆角 `--radius-control`（12px）、无阴影；标题 14px/600 最多两行省略，作者 12px on-surface-variant；左上角完整胶囊徽标（999px、12px/600）：页数（surface-container/ink，>1 时显示「12P」）与 R-18/R-18G（ink 底/surface 字，同时出现时 R 系优先）；小说封面右下角 12px 小书角标。hover 为 8% primary 状态层，focus-visible 环保留。整卡可点击；封面右上角快捷动作（见下条）只在浏览频道页卡片出现，收藏页卡片可渲染同规格的「取消收藏」心形动作（仅条目带 bookmarkId 时），其它网格不渲染。
- **卡片快捷动作**：28×28、圆角 999px、`surface-container` 底 / `ink` 图标、16px 线性图标（stroke 1.8）；hover 8% / active 12% primary 混合（与卡片状态层同源），`focus-visible` primary 2px 外环；默认 `opacity: 0`，hover 或 `focus-within` 显现，过渡 0.15s，`prefers-reduced-motion` 下取消；按钮必须有 `aria-label` 与 `title`；动作按钮与整卡可点击元素为兄弟节点，不得嵌套在 `role="button"` 内。
- **作品网格 WorkGrid**：`repeat(auto-fill, minmax(160px,1fr))`，gap 12/16px（紧凑相关推荐变体 120px）；骨架为纯 surface-container 色块（**不做闪烁动画**）；空态带插画占位与引导文案；错误态给可读文案 + 重试；「没有更多」收尾。
- **分区与 Tab**：频道页分区标题 16px/600 on-surface-variant；「按标签推荐」板块（仅插画频道返回）标题中 `#标签名` 升为 on-surface（`--ink`）强调、后缀「的推荐插画作品」沿用辅助色，板块位于每日排行之后、最新投稿之前，每板块 ≤12 条并纳入 R-18 渲染期过滤（整板块被滤空则整体隐藏，隐藏数并入合计）；类型/周期切换用 md-tabs（secondary），排行前三名徽标用 primary-container 突出。
- **频道 R-18 快捷筛选**：频道页顶部一行「内容筛选」+ 三枚胶囊选项（全部 / 一般向 / R-18）；胶囊与发现页过滤 chips 同一 recipe——透明底、`outline` 60% 的 1px 描边、`--ink` 文字、13px/600，选中态 `secondary-container` / `on-secondary-container` 且描边透明，hover 为 8% `on-surface` 状态层，间距取既有 4–12px 刻度，不新增 token。选中态以 `aria-pressed` 表达，键盘可达，`focus-visible` primary 2px 外环。默认跟随全局 `show_r18`，手动选择只覆盖当前频道页（插画 / 漫画 / 小说三档各自独立、不串档），不改写设置。被过滤的作品只在渲染期隐藏（不重新请求）；隐藏计数沿用辅助行层级（12px、`--ink-subtle`（outline 角色）），不升为标题层级、不占用卡片徽标；整页被滤空不改变分页判定。
- **追更列表**：官方 /following/watchlist 同构——「类型」标签行 + 漫画/小说 md-secondary-tab（SectionTabs）；行式系列卡 `repeat(auto-fill, minmax(360px,1fr))` 网格，卡内 2:3 封面 112px（`--radius-control`）+「系列作品」overline（12px on-surface-variant）+ 标题 16px/600 两行截断 + 作者行（20px 圆头像 + 12px 名字）+「N 话 · 日期」元信息（12px）；主行动「读最新话」为 filled 按钮（34px 高）且与整卡点击同动作（缺失最新话时禁用），动作行与可点击区为兄弟节点、缩进对齐信息列（640px 以下取消缩进）；两类卡均附「系列目录」（应用内系列分集页：novel 卡 → novel 段、manga 卡为追更语义映射 illust 段）；hover 8% primary 状态层、无阴影、无新增动效；追更为用户主动订阅，页面不做 R-18 渲染期过滤；骨架为纯 surface-container 色块，空/错误态沿用居中文案 + 重试层级。
- **查看器舞台**：整页路由，图片区以中性近黑 `rgb(0 0 0 / 0.78)` 为底（深浅色一致），图片 object-contain 居中。多页作品为纵向滚动舞台（单页与 R-18 遮罩态整幅在舞台内垂直居中）：逐页排列、滚动到视口附近才加载，未加载页为与该页纵横比一致（缺省 2:3）的纯色占位块、先在占位块上铺低清层再换详情档；点击任意页进入全屏浮层，舞台右上角保留全屏按钮（键盘入口，定位到视口内当前页），右下角为「第 N / M 页」小徽标（近黑底 + 白字、12px/600，多页时出现），舞台可聚焦、↑/↓ 原生滚动，`prefers-reduced-motion` 下取消平滑滚动。R-18 遮罩为 `blur(24px)` + 中央文案 + filled「显示」按钮，确认后本会话记忆——**仅在设置关闭 `show_r18` 时出现**（作为深链直访兜底），开关开启（默认）时详情页直接展示、不显示确认按钮；遮罩期间只加载首页模糊图、不可进入全屏浮层。近黑底是查看器的既定例外，不得扩散到普通内容容器；从舞台进入的全屏浮层复用同一近黑底，属同一例外、不另立视觉，同样不得扩散。
- **全屏浮层（查看器）**：全屏近黑浮层内 `‹ ›` + 「第 N / M 页」+ ✕（右上角，位于图片区而非胶卷列上方），Esc 或点击图片外关闭；右侧为胶卷缩略图列（固定 80px 宽、缩略图 64px 方形 `--radius-control` 圆角——列内出现纵向滚动条时按剩余宽度收窄、始终方形且不出横向滚动条、`thumb_quality_grid` 档、列内 8px 间距、自身可滚动），当前页以白色 2px 描边标记（近黑底上的既定白，不用 primary）、`aria-current` 表达选中，点击直达该页；翻页时当前缩略图自动滚入视野；胶卷仅多页时出现，窄窗不隐藏。
- **小说阅读器**：正文列 max 720px、14px/1.8 on-surface；`[chapter:]` 渲染为 16px/700 章节标题；翻页器 sticky 底部（surface 底 + 上缘 divider），页码可下拉直选。
- **系列分集页**：路由 `/browse/series/:kind(novel|illust)/:id`（旧 `/browse/series/:id` 函数式 redirect 到 novel 段；watchlist 的 manga 是追更语义、入口映射为 illust），小说与插画/漫画系列共用一套展示。头部：120px 方形封面（`--radius-control`；illust 系列封面空串时回退第一话封面）+ 标题（单行省略、640px 下换行）+ 作者链接 + 简介剥 HTML + 「共 N 话 · 状态」统计行（12px/600 on-surface-variant；illust 端点无完结标记恒显示连载中）+ 动作行（小说「返填表单」+「在 pixiv 打开」，插画/漫画仅「在 pixiv 打开」）。头部之下为工具行：左侧「系列目录」标签（12px/600 on-surface-variant），右侧宫格/列表两枚 30px 自绘 SVG icon toggle（grid_view / view_list、stroke 1.8；选中 `secondary-container` 底 / `on-secondary-container`，未选中透明底 + `ink`、hover 8% on-surface；`aria-pressed` + `aria-label`、focus-visible primary 2px 外环；会话内记忆，默认 illust → 宫格、novel → 列表）。宫格 `repeat(auto-fill, minmax(160px,1fr))`（与 WorkGrid 同参）：illust 封面卡 = 1:1 封面 + 左上角 `#N` 序号胶囊（999px、12px/600、surface-container/ink，与作品卡页数徽标同 recipe）+ R-18/R-18G 右上角徽标（ink 底 surface 字）+ 标题两行截断 + 页数/日期副行；novel 目录瓦片卡 = surface-container 底、20px/600 大号 `#N` + 标题两行截断 + 字数元信息，hover 8% primary 混合。列表 = 48px+ 行式（surface-container 圆角容器 + 行间 divider + hover 8% primary；illust 行加 48px 小封面与 `#N` 序号，novel 行保持两位序号/标题/元信息右对齐，640px 下标题换行、元信息落行尾）。翻页器复用小说阅读器 recipe（sticky 底部、prev/next + 页码下拉直选、边界禁用），novel 30 条/页（`last_order=(page-1)*30`）、illust 12 条/页（官方页码制）；切页保留旧内容降透明局部过渡、禁止闪烁，首屏骨架为纯色块无动画。R-18 全局过滤只作用于分集条目（宫格/列表同口径），头部不受影响。入口约定：追更列表卡、作品/小说详情系列导航、搜索 ID 直达（`novel/series/{id}` → novel 段、`user/{uid}/series/{sid}` → illust 段）一律跳本页。
- **首页头部**：页面标题与「换一批」同行两端对齐、垂直居中；辅助刷新操作固定为 text 按钮级（`md-text-button` + 18px 线性刷新图标），不升到 outlined / filled，保持标题行的主次层级。
- **登录守卫联动**：浏览命令未登录返回固定文案并自动打开既有登录弹窗；浏览页面本身不重复实现登录 UI。
- **收藏页布局**：顶行为类型 md-tabs（插画·漫画 / 小说）+ 公开/私密胶囊分段切换（surface-container 底，选中 primary-container/on-primary-container）；左侧 sticky 标签栏（180px，「全部」行 = 名称+合计计数，标签行 = 名称+计数徽标 12px/600，空名显示「未分类」，选中态 primary-container；640px 下转为横向换行 chips）；右侧为总数行（12px/600 on-surface-variant）+ WorkGrid。收藏页卡片 hover 显示「取消收藏」心形快捷动作；作者页收藏 tab（他人公开收藏）固定 rest=show，不渲染取消收藏动作。
- **详情页收藏按钮**：顶栏触发器对齐 md-outlined-button 规格（40px 胶囊、outline 45%、primary 文字、18px 心形）：未收藏 = 空心 + 「收藏」；已收藏 = 实心 + 「已收藏」，私密收藏追加「私密」角标（surface-container 底 / ink-muted 12px/600 胶囊）；点击弹原生 `details` 弹出菜单（surface-container、`--radius-control` 圆角、唯一合法轻阴影）：未收藏 → 公开收藏 / 私密收藏，已收藏 → 取消收藏；请求进行中禁用（`aria-busy` + 降透明度）。点击菜单外或 Escape 关闭。

**The Native-First Rule.** 已由 Material Web 覆盖的按钮、输入、选择、复选、单选、标签页和进度条不重写外观；原生 `dialog`、`details` 和表格只补充当前实现所需的容器样式。

设置弹窗中的主题与配色可即时预览，但只在保存后写入设置；带有未保存预览时关闭弹窗，恢复已保存主题并显示全局 Snackbar。设置区新增「图片与缩略图」（列表·网格 / 详情页主图 / 大图·全屏三档，默认 medium / medium / large）与「内容显示」（R-18 开关，默认开启 `true`）两组，沿用既有 form / `fieldset` 结构与 40px 控件密度，不新增 token。

交互动画主要来自 Material Web 组件；项目全局在 `prefers-reduced-motion: reduce` 下将过渡和动画缩短至 0.01ms、禁止平滑滚动。焦点表现依赖 Material Web 的控件实现；自定义图标按钮提供 `aria-label`，任务筛选组、任务列表、设置表单、通知及告警已有对应的语义标签或角色。新自定义可操作控件必须保留等效的键盘可达性和名称。

## Do's and Don'ts

**Do:** 使用 M3 颜色角色、Material Web 组件和既有 4–24px 间距；将失败、警告、进度与登录状态以文字和视觉共同表达；保留 640px 下的换行、纵向布局和表格滚动；尊重减少动态效果偏好。

**Don't:** 不要以旧版蓝白“书库”规范、Naive UI 主题或玻璃拟态覆盖 M3 系统；不要为普通卡片添加阴影；不要将 primary 用作语义状态颜色；不要删除可访问名称、原生控件语义或缩减动态效果规则。
