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

应用外壳是全高双列 grid：默认侧栏 256px，折叠后 72px，主内容占余宽。侧栏标题高 72px；导航项最小高 48px，圆角胶囊宽度扣除左右各 12px。普通主内容的内边距为 24px，内容列最大宽度为 1120px；抓取与设置表单卡最大宽度为 600px。

历史与任务列表在底部右对齐分页区提供每页 20、50、100 条的选择器；切换容量或分类回到第 1 页。历史页把容量传至数据库查询，任务页只对当前筛选结果分页，选择与批量操作仍保留跨页状态。

空间只使用 4、6、8、12、16、24px 刻度。工具栏与控制行可以换行；任务标题在 640px 以下变为纵向排列，历史搜索框与设置路径行扩展为全宽/纵向，Pixiv 浏览工具栏允许地址栏换至下一行。表格保持最小宽度 760px 并允许横向滚动。侧栏不会因媒体查询自动隐藏，用户通过折叠按钮切换。

**The Compact Task Rule.** 对齐、留白和换行优先服务于任务、历史和设置的扫读，而不是制造大面积空白。

## Elevation & Depth

日常层级通过 surface 与 surface container 的色调差、outline 分隔线和圆角表达；普通卡片、任务列表和表格没有阴影。账号菜单与 snackbar 使用 `0 4px 12px rgb(0 0 0 / 18%)` 的轻阴影（**唯一合法值**，不要再写 `12px/14px` 这类非法或变体写法；snackbar 历史上的 14px blur 已于 2026-10-01 收敛到 12px），原生对话框的 backdrop 为 `rgb(0 0 0 / 35%)`。

**The Overlay-Only Shadow Rule.** 阴影仅标示临时浮层，不应用于普通内容容器。

## Shapes

Material Web 控件继承库的 M3 外观。应用自定义的 control 圆角为 12px，卡片、表格和任务列表为 16px；左侧导航当前项为 24px；登录、确认等原生 `dialog` 为 28px。状态与类别标签使用完整胶囊形状（999px），头像为圆形。

## Components

- `md-filled-button`：主提交、恢复和确认操作；`md-outlined-button`：浏览、同步、批量删除等次要操作；`md-text-button`：取消、删除等低强调操作；`md-icon-button`：工具栏和行级图标动作。
- `md-outlined-text-field`、`md-outlined-select`、`md-radio` 与 `md-checkbox`：所有可编辑字段和选择；字段以标签、12px–16px 间距和至少 40px 的 choice 行组织。
- `md-tabs` / `md-primary-tab`：登录方式切换；`md-linear-progress`：任务进度。Material Web 负责它们的默认交互状态。
- 原生 `dialog`：登录和危险操作确认，采用 surface-container、28px 圆角及右对齐按钮行；自定义 `.m3-snackbar` 固定在右上角，通知在 3.2 秒后消失。页面不应另建成功提示条。
- 左侧导航：图标加文字，hover 使用 8% primary 的状态层，active 使用 primary-container。账号区位于侧栏底部，原生 `details` 菜单向上弹出。
- alert、状态 pill、表格和表单卡是小型本地样式，复用 M3 颜色角色和上述 shape/spacing，不另建组件库。

**控件密度.** Material Web 的 `md-outlined-text-field` 与 `md-outlined-select` 统一为 40px 高、14px 输入文字（组件默认 56px / 16px），与全局 14px 正文和 40px 的 choice 行对齐。实现只做尺寸 token 覆盖（`frontend/src/styles/main.css` 的「控件密度层」）：上下内距 8px（文本框 `--md-outlined-text-field-top/bottom-space`；选择器无容器高度 token，经内嵌 field 继承 `--md-outlined-field-top/bottom-space`）、输入字号 14px（选择器走 `--md-outlined-select-text-field-input-text-size`）、输入行高钉 24px（8 + 24 + 8 = 40px）。按钮维持 Material 默认的 40px 高 / 14px 标签字，不做覆盖。密度层不改颜色、圆角、描边、浮标等组件外观（Native-First Rule）。

### 浏览模式组件（ADR 0012）

浏览（browse）页面在既有 M3 体系上新增以下本地组件，全部复用现有颜色角色与间距刻度，不引入新 token：

- **作品卡 WorkCard**：封面圆角 `--radius-control`（12px）、无阴影；标题 14px/600 最多两行省略，作者 12px on-surface-variant；左上角完整胶囊徽标（999px、12px/600）：页数（surface-container/ink，>1 时显示「12P」）与 R-18/R-18G（ink 底/surface 字，同时出现时 R 系优先）；小说封面右下角 12px 小书角标。hover 为 8% primary 状态层，focus-visible 环保留。整卡可点击；封面右上角快捷动作（见下条）只在浏览频道页卡片出现，其它网格不渲染。
- **卡片快捷动作**：28×28、圆角 999px、`surface-container` 底 / `ink` 图标、16px 线性图标（stroke 1.8）；hover 8% / active 12% primary 混合（与卡片状态层同源），`focus-visible` primary 2px 外环；默认 `opacity: 0`，hover 或 `focus-within` 显现，过渡 0.15s，`prefers-reduced-motion` 下取消；按钮必须有 `aria-label` 与 `title`；动作按钮与整卡可点击元素为兄弟节点，不得嵌套在 `role="button"` 内。
- **作品网格 WorkGrid**：`repeat(auto-fill, minmax(160px,1fr))`，gap 12/16px（紧凑相关推荐变体 120px）；骨架为纯 surface-container 色块（**不做闪烁动画**）；空态带插画占位与引导文案；错误态给可读文案 + 重试；「没有更多」收尾。
- **分区与 Tab**：频道页分区标题 16px/600 on-surface-variant；类型/周期切换用 md-tabs（secondary），排行前三名徽标用 primary-container 突出。
- **查看器舞台**：整页路由，图片区以中性近黑 `rgb(0 0 0 / 0.78)` 为底（深浅色一致），图片 object-contain 居中；R-18 遮罩为 `blur(24px)` + 中央文案 + filled「显示」按钮，确认后本会话记忆。近黑底是查看器的既定例外，不得扩散到普通内容容器。
- **小说阅读器**：正文列 max 720px、14px/1.8 on-surface；`[chapter:]` 渲染为 16px/700 章节标题；翻页器 sticky 底部（surface 底 + 上缘 divider），页码可下拉直选；系列目录为 48px+ 行式列表（序号/标题/元信息右对齐），非卡片网格。
- **登录守卫联动**：浏览命令未登录返回固定文案并自动打开既有登录弹窗；浏览页面本身不重复实现登录 UI。

**The Native-First Rule.** 已由 Material Web 覆盖的按钮、输入、选择、复选、单选、标签页和进度条不重写外观；原生 `dialog`、`details` 和表格只补充当前实现所需的容器样式。

设置页的主题与配色可即时预览，但只在保存后写入设置；带有未保存预览时离开页面，恢复已保存主题并显示全局 Snackbar。

交互动画主要来自 Material Web 组件；项目全局在 `prefers-reduced-motion: reduce` 下将过渡和动画缩短至 0.01ms、禁止平滑滚动。焦点表现依赖 Material Web 的控件实现；自定义图标按钮提供 `aria-label`，任务筛选组、任务列表、设置表单、通知及告警已有对应的语义标签或角色。新自定义可操作控件必须保留等效的键盘可达性和名称。

## Do's and Don'ts

**Do:** 使用 M3 颜色角色、Material Web 组件和既有 4–24px 间距；将失败、警告、进度与登录状态以文字和视觉共同表达；保留 640px 下的换行、纵向布局和表格滚动；尊重减少动态效果偏好。

**Don't:** 不要以旧版蓝白“书库”规范、Naive UI 主题或玻璃拟态覆盖 M3 系统；不要为普通卡片添加阴影；不要将 primary 用作语义状态颜色；不要删除可访问名称、原生控件语义或缩减动态效果规则。
