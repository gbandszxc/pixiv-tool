# 浮层配方收敛：共享样式层与 useAnchorPopover 实施计划

- 日期：2026-10-30
- 状态：规划（未实施）
- 来源：$impeccable audit 全局审查遗留条目 3（浮层配方在 6+ 组件各自复制）与条目 4（HelpTooltip / CopyPathButton popover 定位逻辑复制粘贴）。
- 规格入口：[`../SPEC.md`](../SPEC.md) 视觉规范以 [`../../DESIGN.md`](../../DESIGN.md) 为准；本文不改任何运行代码，实施时按阶段同步更新 DESIGN.md。
- 本轮范围：方案设计与文档落盘，不修改运行代码。

## 1. 目标与边界

把散落在各组件的「浮层三件套」（surface-container 底 + control 圆角 + 唯一合法轻阴影）收敛为 main.css 单一共享层，把 HelpTooltip 与 CopyPathButton 逐字重复的 popover 定位/关闭编排收敛为一个 composable。

| 项目 | 决定 |
|---|---|
| 样式收敛载体 | main.css 新增 `.m3-popover` 类，组件浮层节点挂类并删除各自重复声明 |
| 行为收敛载体 | `frontend/src/composables/useAnchorPopover.ts`（新建） |
| 首批行为迁移 | HelpTooltip、CopyPathButton（双胞胎，收益确定） |
| 菜单类组件 | AccountMenu / NovelBgPicker / CommentEmojiPicker / DownloadStatusBar / BookmarkButton：样式类必迁；行为编排视阶段 2 落地后的实际收益决定，不强行统一 |
| 明确不迁移 | NovelTranslationMenu 面板定位（侧栏右缘钳制为特有逻辑）只吃样式类，不进 composable |
| 不改动 | snackbar（main.css 全局层自持、inverse 底色独立配方，仅阴影同源）；ImageViewer 近黑浮层（DESIGN.md 授权的深色例外） |
| 视觉结果 | 迁移前后像素级一致；本计划是纯重构，不引入任何新视觉 |

## 2. 现状证据（实施前应再次核对行号）

- 阴影字面值 `0 4px 12px rgb(0 0 0 / 18%)` 出现于 9 个文件：`AccountMenu.vue`、`NovelBgPicker.vue`、`BookmarkButton.vue`、`CommentEmojiPicker.vue`、`NovelTranslationMenu.vue`、`DownloadStatusBar.vue`、`HelpTooltip.vue`、`CopyPathButton.vue` 与 `styles/main.css`（snackbar）。各处还各自重复 `background: var(--md-sys-color-surface-container)` 与 `border-radius: var(--radius-control)`。
- `HelpTooltip.vue:19-68` 与 `CopyPathButton.vue:22-76` 约 50 行逐字重复：`close` / `show` / `scheduleClose` / `onOutside` / `onScroll` / `onKeydown` 六个函数、相同的定位算术（`--space-sm` gap、`gap * 2` 视口边距、下方放不下翻上方）、相同的四组窗口监听编排（keydown capture / pointerdown capture / resize / scroll capture）与 `onBeforeUnmount` 清理。
- 两者真实差异仅三处：HelpTooltip 的点击固定（`pinned` / `toggle`，:52-55）；CopyPathButton 的 `disabled` 守卫与 copy 动作；tooltip 内宽（`width: max-content` 的具体 max-width 组合一致，内距不同：`--space-md` vs `--space-sm --space-md`）。
- 菜单类组件的「透明遮罩点击外部关闭 + Esc + 焦点回路」语义相同但实现各自为政（遮罩有无、v-if 结构、焦点去向细节不一）；第一轮审查修复后均已达标，属「行为正确、实现重复」状态。
- 原生 popover top layer（`popover="manual"` + `showPopover()`）是现行的防裁切手段（HelpTooltip.vue:33-34 注释明确），composable 必须沿用，不得改回普通绝对定位。

## 3. 方案设计

### 3.1 阶段 1：`.m3-popover` 共享样式层（对应条目 3）

main.css 新增（与「全局滚动条」「控件密度层」同级的独立小节）：

```css
/* 浮层共享层：唯一的合法轻阴影只在此处与 snackbar 出现 */
.m3-popover {
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  border-radius: var(--radius-control);
  box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
}
```

- 八个组件的浮层根节点挂 `class="m3-popover"`（scoped 样式里的三件套声明删除，定位 / 尺寸 / 内距 / 专用子样式保留）。
- scoped 选择器特异性高于全局类，迁移时须确认组件内没有对这三项的**有意覆盖**（当前盘点无）。
- snackbar 保持现状，DESIGN.md 注明「snackbar 为独立 inverse 配方、阴影与浮层同源」。

### 3.2 阶段 2：`useAnchorPopover()` composable（对应条目 4）

```ts
export function useAnchorPopover(options: {
  trigger: Ref<HTMLElement | null>;
  popover: Ref<HTMLElement | null>;            // popover="manual" 元素
  root?: Ref<HTMLElement | null>;              // 外部点击判定容器，缺省 popover 最近祖先
  placement?: "bottom" | "auto";               // 默认 bottom；auto = 下方放不下翻上方（现行语义）
  disabled?: () => boolean;                    // CopyPathButton 的禁用守卫
  hoverGraceMs?: number;                       // >0 启用 pointerenter/leave + 宽限收起（tooltip 形态，传 150）
}): {
  open: Ref<boolean>;
  pinned: Ref<boolean>;                        // 点击固定态（HelpTooltip click-to-pin 用）
  show(): void; close(): void; toggle(): void;
  scheduleClose(): void; cancelClose(): void;
};
```

- 内涵：`showPopover()/hidePopover()` 编排、定位算术、Esc（capture + stopPropagation）/外部 pointerdown（capture）/resize/scroll（capture、浮层内滚动豁免）四组监听的挂载与卸载、`onBeforeUnmount` 清理。
- 外留组件自持：焦点回路（`@focus="show"` / `@blur="close"`）、业务动作（copy / 固定切换）、`aria-describedby` 关联——各组件语义不同，不进 composable。
- 定位若日后需要侧栏钳制（翻译浮窗语义），以可选 `clampLeft?: () => number` 扩展，本期不做。

### 3.3 阶段 3：菜单类组件行为迁移（可选，收益驱动）

AccountMenu / NovelBgPicker / CommentEmojiPicker / DownloadStatusBar / BookmarkButton 五个菜单的「透明遮罩 + Esc + 焦点回路」编排同样重复。阶段 2 落地后评估：若 composable 增加「点击开关 + 焦点进出」形态的成本低于逐组件保留，则扩展 `mode: "hover" | "click"` 一并迁移；否则停在样式类收敛。**不以此为验收门槛。**

## 4. 实施拆解

| 阶段 | 内容 | 交付 | 预估 |
|---|---|---|---|
| 1 | `.m3-popover` 落 main.css；8 组件挂类删重复；grep 断言阴影字面值仅存 main.css | 一个提交 + DESIGN.md Elevation/HelpTooltip 条目同步 | 0.5 天 |
| 2 | `useAnchorPopover.ts` + HelpTooltip / CopyPathButton 迁移（预期净删 ~90 行重复） | 一个提交 | 1 天 |
| 3 | 回归验收（见 §5）+ 阶段 3 决策记录 | 决策写入本文状态行 | 0.5 天 |

提交信息沿用 `<类型>(<范围>): <中文说明>`：`refactor(ui): 浮层三件套收敛为 .m3-popover 共享层`、`refactor(ui): 抽 useAnchorPopover 收敛 tooltip 定位编排`。

## 5. 验收标准

- grep 断言：`0 4px 12px` 在 `frontend/src` 仅出现于 main.css；`surface-container` + `radius-control` + 阴影三件套在组件层零重复。
- 逐组件回归清单（迁移前后行为一致）：浅 / 深主题与五套色板；640px 窄窗不溢出；设置弹窗滚动区内不被裁切（top layer）；Esc / 点击外部 / 滚动 / resize 关闭；HelpTooltip 的悬停展开、150ms 跨隙宽限、点击固定 / 再点收起、`aria-describedby`；CopyPathButton 的 disabled、复制成功图标切换与 1.6s 复原；焦点回路（关闭回触发器）。
- `pnpm --dir frontend build` 通过；`prefers-reduced-motion` 下无新增动效（本重构不引入动效）。

## 6. 风险

- scoped 样式与全局类的特异性叠加（阶段 1 迁移时逐个目检渲染结果）。
- composable 过度抽象：菜单类形态若与 tooltip 形态差异过大，宁可停在样式类收敛，不为统一而统一（阶段 3 明确可不做）。
- 事件监听挂载时机变化引起的关闭竞态（迁移时保留「先 show 后挂监听」的现行顺序）。
