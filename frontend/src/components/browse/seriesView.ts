/**
 * 系列分集页共享状态与视图模型（BrowseSeriesView 及其子组件共用）。
 *
 * - SeriesKind：路由 kind 段取值（novel | illust；watchlist 的 manga 语义在入口映射为 illust）。
 * - 展示模式会话内记忆：模块级 ref（SPA 生命周期，参照 r18Filter 的频道档模式），
 *   默认 illust → 宫格、novel → 列表（series-episode-ui 主会话决策固定项）。
 */
import { computed, ref, type ComputedRef } from "vue";

export type SeriesKind = "novel" | "illust";
export type SeriesViewMode = "grid" | "list";

/** 各 kind 的展示模式（模块级 = 本次会话内记忆）。 */
const modeByKind = ref<Record<SeriesKind, SeriesViewMode>>({ novel: "list", illust: "grid" });

/**
 * 读取/设置当前 kind 的展示模式。kind 传 getter：
 * novel/illust 两条路由复用同一组件实例，props.kind 变化无需重新调用本函数。
 */
export function useSeriesViewMode(
  kind: () => SeriesKind
): { mode: ComputedRef<SeriesViewMode>; setMode: (mode: SeriesViewMode) => void } {
  return {
    mode: computed(() => modeByKind.value[kind()]),
    setMode: (mode) => {
      modeByKind.value[kind()] = mode;
    },
  };
}

/**
 * 归一化分集条目：两类型共用视图模型（字段沿用契约 snake_case，与 BrowseWorkItem
 * 直通组件的惯例一致；novel 无封面/页数，illust 无字数）。
 * x_restrict 沿用契约命名，可直接进入 filterByR18 条目级口径。
 */
export interface SeriesEpisodeView {
  id: number;
  /** 话数 1..total（#N 序号同源） */
  series_order: number;
  title: string;
  /** illust：封面 URL（原样，展示层经 thumbSrc 改写） */
  cover?: string;
  /** illust：页数 */
  page_count?: number;
  /** 0 无 | 1 R-18 | 2 R-18G（R-18 过滤与徽标同源） */
  x_restrict?: number;
  /** novel：字数 */
  text_length?: number | null;
  /** YYYY-MM-DD（update_date 截取日期段） */
  update_date?: string;
}
