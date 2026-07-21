---
name: Pixiv Tool
description: A refined, reliable local workspace for organizing Pixiv novels.
colors:
  primary: "#0096FA"
  primary-hover: "#0077D1"
  surface: "#FFFFFF"
  text: "#333333"
  text-strong: "#444444"
  text-muted: "#777777"
  text-subtle: "#999999"
  border: "#E0E0E6"
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "16px"
    fontWeight: 700
    lineHeight: 1.4
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.4
rounded:
  control: "6px"
spacing:
  xxs: "4px"
  xs: "6px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "#FFFFFF"
    rounded: "{rounded.control}"
    padding: "0 14px"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  account-trigger:
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "6px"
---

# Design System: Pixiv Tool

## 1. Overview

**Creative North Star: "静谧书库"**

Pixiv Tool 是个人内容整理工具，不是社交产品或营销页。界面应有书库般安静、有序的背景感，同时保持桌面工具应有的响应速度和信息密度；用户在浏览任务、确认账号或查看历史时，应能迅速找到下一步。

以 Naive UI 的默认组件语义作为共同语言，所有页面维持一致的控件高度、反馈方式与信息节奏。Pixiv 蓝只承担主要操作、当前选择和键盘焦点等需要行动的语义，不作为大面积装饰。现有代码中的绿色字面值属于待迁移实现，不构成设计规范。

**Key Characteristics:**

- 精致而紧凑：留白服务于分组与扫描，不追求空泛的“呼吸感”。
- 可靠而清楚：账号、任务、错误和成功结果均使用稳定的组件语义。
- 高效而低干扰：动效只解释菜单、悬停和状态变化，时长短且可降级。

## 2. Colors

以干净的白色内容面配合 Pixiv 蓝，建立明确的行动层级；中性色承担文本、边界和辅助信息，避免引入第二个竞争性强调色。

### Primary

- **Pixiv Blue** (`#0096FA`): 主按钮、当前导航、可交互焦点与需要立即处理的正向操作。单个页面中仅用于行动语义，不作为卡片或背景的大面积铺色。
- **Pixiv Blue Deep** (`#0077D1`): 主按钮悬停与按下反馈，保持与主色同一色相。

### Neutral

- **Library Surface** (`#FFFFFF`): 主内容与卡片表面。
- **Ink** (`#333333`): 常规正文、按钮和账号名称。
- **Strong Ink** (`#444444`): 次级但仍需清晰阅读的文本。
- **Quiet Ink** (`#777777`): 图标与非关键辅助信息。
- **Faint Ink** (`#999999`): 低优先级元信息，不能单独传达关键状态。
- **Divider** (`#E0E0E6`): 侧栏、容器与列表的分隔线。

**The One Accent Rule.** Pixiv Blue 是唯一的产品强调色；成功、警告和错误继续使用 Naive UI 语义色，不能用蓝色替代或伪装状态。

## 3. Typography

**Display Font:** 不使用独立展示字体。
**Body Font:** `-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif`
**Label/Mono Font:** 与正文一致；ID、路径和数字在系统等宽替代可用时自然回退。

**Character:** 系统无衬线字体在 Windows WebView2 中稳定、清楚，适合中英文混排和密集任务数据。字号对比克制，依靠字重、间距与组件分组而非夸张标题制造层级。

### Hierarchy

- **Title** (700, 16px, 1.4): 侧栏产品名、页面小标题和区块标题。
- **Body** (400, 14px, 1.5): 表单、列表、说明和按钮的默认文本。
- **Label** (500, 12px, 1.4): 任务进度、表格元数据和紧凑控件标签。
- **Muted metadata** (400, 12px, 1.4): 仅用于补充信息；关键错误与操作不可使用此层级。

**The Readable Metadata Rule.** 超长小说标题、Pixiv ID 和文件路径必须通过省略号配合悬浮提示或可展开区域保留完整信息；不得靠缩小字号来硬塞。

## 4. Elevation

默认采用 Naive UI 的平面组件层次：白色表面、细分隔线与清晰的内容分组优先。弹出菜单、确认框和下拉层使用组件库默认轻阴影来表明临时上下文；普通卡片、列表与表单不额外叠加阴影。

**The Flat-by-Default Rule.** 阴影只属于浮层和瞬时交互层，不能用多层卡片、玻璃模糊或持续悬浮感替代信息架构。

## 5. Components

### Buttons

- **Shape:** 统一使用 Naive UI 默认控件形状；自定义控件圆角为 6px。
- **Primary:** Pixiv Blue 背景、白色文字，用于提交抓取、保存设置和恢复任务等明确的主要操作。
- **Hover / Focus:** 悬停切换至 `#0077D1`；键盘焦点使用 2px Pixiv Blue 外轮廓，并保持足够偏移量。
- **Danger / Secondary:** 沿用 Naive UI `error` 与默认按钮语义，分别用于不可逆操作和次要动作。

### Inputs / Fields

- **Style:** 使用 Naive UI 默认输入框、选择器、单选和复选组件，页面内不自行发明字段外观。
- **Focus:** 统一交给 Pixiv Blue 主题色；校验失败仍使用组件库 error 语义。
- **Density:** 常用表单宽度以 600px 内容列为上限，标签使用左对齐布局，避免阅读时左右跳动。

### Cards / Containers

- **Corner Style:** 遵循 Naive UI 默认卡片圆角；不要在卡片内部再嵌套装饰性卡片。
- **Background:** `#FFFFFF` 内容表面，细边界或自然页面留白用于分组。
- **Internal Padding:** 默认以 16px 为基准；紧凑列表和账号触发器可用 6–8px。

### Navigation

- **Style:** 左侧固定导航，页面标题与内容在主区以 24px 内边距对齐。
- **State:** 当前路由由 Pixiv Blue 标识；侧栏底部账号区使用头像、Pixiv ID 和菜单箭头构成一个整体触发器。
- **Menu:** 菜单通过 Naive UI 浮层呈现，浮层不得被侧栏滚动或裁切。

### Status and Progress

- **Style:** 任务状态由 Naive UI 标签、进度条与警告/错误语义色共同表达；文字必须同时说明状态，不能只靠颜色。
- **Motion:** 菜单与按钮仅允许约 180–200ms 的背景或透明度变化；在减少动态效果环境中禁用非必要过渡。

## 6. Do's and Don'ts

### Do:

- **Do** 以 `#0096FA` 作为唯一主强调色，并通过 Naive UI 主题配置统一注入，而不是在页面中散落颜色字面值。
- **Do** 使用 4px、6px、8px、12px、16px、24px 间距刻度组织表单、列表和侧栏。
- **Do** 让账号、任务进度、失败原因和完成结果在首屏可扫读。
- **Do** 使用短暂、服务状态的 180–200ms 交互反馈，并为减少动态效果提供降级。

### Don't:

- **Don't** 使用营销页式大视觉、社交信息流布局或“炫技型下载器”式装饰。
- **Don't** 使用渐变文字、彩色侧边条、默认玻璃拟态或多层嵌套卡片。
- **Don't** 以 Pixiv Blue 伪装成功、警告或错误状态，也不要引入竞争性的第二主色。
- **Don't** 为了塞入长标题、账号或路径而缩小关键文字；应使用省略、提示或展开。
- **Don't** 让动效拖慢任务，或以动画遮掩加载、登录失效和抓取失败。
