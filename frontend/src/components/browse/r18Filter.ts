/**
 * R-18 过滤：全局档（settings.show_r18）与频道档（频道页筛选条）分离。
 *
 * - 全局档只读设置，供 WorkGrid / RelatedGrid / 排行榜 / 系列目录等共用列表使用；
 * - 频道档默认跟随全局，手动切换后以模块级状态为准，且只有频道页读写——
 *   避免「频道页点一次 R-18」把共用列表的过滤口径一起放开；手动档按频道 kind
 *   分别记录，插画 / 漫画 / 小说三条路由互不串档。
 * 过滤全程为渲染期 computed：切换不触发任何请求，也不改分页 / 去重状态。
 */
import { computed, ref, type ComputedRef } from "vue";
import { useSettingsStore } from "../../stores/settings";

export type R18Filter = "all" | "safe" | "r18";

/** 频道页的三种 kind（三条频道路由复用同一组件实例，仅 props.kind 变化）。 */
export type ChannelKind = "illustration" | "manga" | "novel";

/** 各频道档的手动覆盖；null = 跟随全局档。模块级即「本次会话」（SPA 生命周期）。 */
const channelManual = ref<Record<ChannelKind, R18Filter | null>>({
  illustration: null,
  manga: null,
  novel: null,
});

/**
 * x_restrict：0 无 | 1 R-18 | 2 R-18G；缺失视为「状态未知」。
 *
 * 未知条目只在「全部」档可见：safe（一般向）与 r18（只看 R-18）都不收——
 * 排行榜等接口存在 `illust_content_type` 缺失 / 为空数组的条目（ugoira 榜实测
 * 有之），把未知当 0 会在关闭 R-18 时把它们放进列表，宁可少显示不可放行。
 */
export function matchesR18(xRestrict: number | undefined, filter: R18Filter): boolean {
  if (filter === "all") return true;
  if (xRestrict === undefined) return false;
  if (filter === "safe") return xRestrict === 0;
  return xRestrict >= 1;
}

export function filterByR18<T extends { x_restrict?: number }>(items: T[], filter: R18Filter): T[] {
  return items.filter((item) => matchesR18(item.x_restrict, filter));
}

/** 全局档：只读 settings.show_r18，不读任何手动覆盖 —— WorkGrid / RelatedGrid / Ranking / Series 用。 */
export function useGlobalR18Filter(): ComputedRef<R18Filter> {
  const settings = useSettingsStore();
  return computed<R18Filter>(() => (settings.settings.show_r18 ? "all" : "safe"));
}

/**
 * 频道档：默认跟随全局；手动切换后本次会话（模块级）以手动为准，按 kind 分档 ——
 * 只有 BrowseChannelView 读写。kind 传 getter：三条路由复用同一组件实例，
 * props.kind 变化不需要重新调用本函数。
 */
export function useChannelR18Filter(
  kind: () => ChannelKind
): { filter: ComputedRef<R18Filter>; setFilter: (v: R18Filter) => void } {
  const settings = useSettingsStore();
  const filter = computed<R18Filter>(
    () => channelManual.value[kind()] ?? (settings.settings.show_r18 ? "all" : "safe")
  );
  return {
    filter,
    setFilter: (v) => {
      channelManual.value[kind()] = v;
    },
  };
}
