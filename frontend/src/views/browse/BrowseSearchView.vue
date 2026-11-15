<script setup lang="ts">
import HelpTooltip from "../../components/common/HelpTooltip.vue";
import { scrollContentToTop } from "../../utils/scroll";
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·搜索页（F2）。
 *
 * - URL 查询参数 `?word=&kind=&order=&mode=&s_mode=&users_iri=` 回显：挂载/前进后退时恢复
 *   状态并自动搜索；执行搜索或切换过滤时 `router.replace` 写回（单一真相源是
 *   路由查询参数，本地 ref 仅作受控回显）。
 * - ID 直达：输入先经 `parseBrowseInput` 解析，命中链接/纯数字直接跳站内路由，
 *   不发起搜索；纯数字按当前类型 tab（插画/漫画/小说）落到对应详情，链接形态
 *   自带类型不受 tab 影响；未命中按关键词搜索。
 * - 端点映射：kind=illust|manga → artworks 检索（type=illust|manga）；
 *   kind=novel → novels 检索（不传 type）。
 * - 结果 WorkGrid + 「约 N 件」total；页码分页（AppPagination）：每页条数按端点
 *   对齐（实测见 docs/PIXIV-API.md §搜索）——插画/漫画 artworks 60 条切 3 个显示页
 *   （20/页），小说 novels 30 条即 1 个显示页（30/页，一页恰一次请求）；
 *   接口页缓存在内存供跨页复用。
 * - users入り 档位过滤：收藏数下限档位（100/500/1000/5000/10000）经 IPC `users_iri`
 *   透传，后端按档位把 `{N}users入り` 拼进搜索词（搜索词过滤而非排序）；不过滤时
 *   不传参。与 s_tag_full（标签完全一致）互斥——组合词在该匹配下必然失效，双向收口
 *   回落 s_tag（见 normalizeSMode）。
 * - 搜索历史下拉：输入框聚焦/输入时展开（Google 风格，空历史不渲染）。后端在
 *   browse_search 成功且第 1 页时权威记录（重复搜索刷新时间，最新在前 ≤200 条），
 *   本页只读 + 单条删除（不确认，失败 notify），每次展开时重拉。条目 = 关键词 +
 *   类型徽标 + 相对时间（utils/relativeTime 共用实现）；点击条目 = 按六元组直接
 *   回写 URL 查询参数（复用下方 replace 写参与自动搜索机制，空缺过滤器按本页
 *   默认值回落，不经 parseBrowseInput 的 ID 直达解析）；Esc / 外部点击 / 选中后
 *   关闭，↑↓ 移动高亮 + Enter 选中，loading 期禁点。
 * - 本页排序：维度（点赞/收藏/浏览）下拉 + 升降方向切换（SortDirectionToggle），
 *   仅对当前显示页本地排序；
 *   优先复用列表已有计数（小说自带收藏数），仅所选维度缺失时经 browse_work_counts
 *   整页一次性补取（全局限速），补齐后只重排一次，缺失项垫底。
 */
import { computed, nextTick, onActivated, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { useRoute, useRouter, type LocationQuery } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  browseSearch,
  browseWorkCounts,
  deleteBrowseSearchHistory,
  errorMessage,
  listBrowseSearchHistory,
  type BrowseSearchHistoryItem,
  type BrowseWorkItem,
  type WorkCounts,
} from "../../api/browse";
import AppPagination from "../../components/common/AppPagination.vue";
import SortDirectionToggle from "../../components/common/SortDirectionToggle.vue";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { notify } from "../../ui/notify";
import { parseBrowseInput, type ParsedBrowseInput } from "../../utils/parseInput";
import { browseTargetPath } from "../../utils/pixivLinks";
import { relativeTime as formatRelativeTime } from "../../utils/relativeTime";

const { t, locale } = useI18n();
const route = useRoute();
const router = useRouter();

// ===== 参数取值域 =====

type SearchKind = "illust" | "manga" | "novel";
const ORDERS = ["date_d", "date", "popular_d"] as const;
const MODES = ["all", "safe", "r18"] as const;
const S_MODES = ["s_tag", "s_tag_full", "s_tc"] as const;
/** users入り 收藏数档位（搜索词过滤，非排序）；空串 = 不过滤（不向后端传参）。 */
const USERS_IRI_LEVELS = ["100", "500", "1000", "5000", "10000"] as const;
type Order = (typeof ORDERS)[number];
type Mode = (typeof MODES)[number];
type SMode = (typeof S_MODES)[number];
type UsersIri = "" | (typeof USERS_IRI_LEVELS)[number];

/** 值域校验：非法值回落默认（手工改 URL 也不会把页面带崩）。 */
function oneOf<T extends string>(allowed: readonly T[], value: string, fallback: T): T {
  return (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
}

/** users_iri 解析：非法值（含空串/缺参）回落不过滤。 */
function usersIriOf(value: string): UsersIri {
  return (USERS_IRI_LEVELS as readonly string[]).includes(value) ? (value as UsersIri) : "";
}

/**
 * s_mode 冲突规则：users入り 档位过滤靠 `{词} {N}users入り` 组合词命中标签，
 * 在 s_tag_full（标签完全一致）下组合词必然失效——启用档位时统一回落 s_tag。
 */
function normalizeSMode(usersIri: UsersIri, sMode: SMode): SMode {
  return usersIri && sMode === "s_tag_full" ? "s_tag" : sMode;
}

function queryStr(q: LocationQuery, key: string): string {
  const v = q[key];
  const raw = Array.isArray(v) ? v[0] : v;
  return typeof raw === "string" ? raw : "";
}

interface SearchParams {
  word: string;
  kind: SearchKind;
  order: Order;
  mode: Mode;
  sMode: SMode;
  usersIri?: UsersIri;
}

function paramsFromQuery(q: LocationQuery): SearchParams {
  const usersIri = usersIriOf(queryStr(q, "users_iri"));
  const sMode = oneOf(S_MODES, queryStr(q, "s_mode"), "s_tag");
  return {
    word: queryStr(q, "word").trim(),
    kind: oneOf(["illust", "manga", "novel"] as const, queryStr(q, "kind"), "illust"),
    order: oneOf(ORDERS, queryStr(q, "order"), "date_d"),
    mode: oneOf(MODES, queryStr(q, "mode"), "all"),
    // 手工改 URL 带入冲突组合（s_tag_full + users_iri）时同批规范化
    sMode: normalizeSMode(usersIri, sMode),
    usersIri,
  };
}

// ===== 受控状态（初始值即默认参数，挂载时由 applyQuery 覆盖） =====

const word = ref("");
const kind = ref<SearchKind>("illust");
const order = ref<Order>("date_d");
const mode = ref<Mode>("all");
const sMode = ref<SMode>("s_tag");
const usersIri = ref<UsersIri>("");

function currentParams(): SearchParams {
  return {
    word: word.value.trim(),
    kind: kind.value,
    order: order.value,
    mode: mode.value,
    sMode: sMode.value,
    usersIri: usersIri.value,
  };
}

function sigOf(p: SearchParams): string {
  return [p.word, p.kind, p.order, p.mode, p.sMode, p.usersIri].join("\u0000");
}

// ===== 结果列表（页码分页：接口每页条数按端点对齐，前端切显示页）=====

/** 显示页条数（AppPagination 每页容量）：artworks 接口页 60 条切 3 页（20/页）；
 * novels 接口页 30 条整页显示（30/页，一接口页 = 一显示页，一页恰一次请求）。 */
const PAGE_SIZE = computed(() => (kind.value === "novel" ? 30 : 20));
/** 一个接口页切成几个显示页。 */
const SEGMENTS_PER_API_PAGE = computed(() => (kind.value === "novel" ? 1 : 3));
/** 接口页缓存上限（超出淘汰最旧；被淘汰页再访问时重新请求）。 */
const MAX_API_PAGES = 30;

const page = ref(1);
const total = ref<number | null>(null);
const lastPage = ref<number | null>(null);
const items = shallowRef<BrowseWorkItem[]>([]);
const loading = ref(false);
const error = ref("");

/** 接口页缓存（apiPage → 接口页条目）；搜索参数变化时清空，跨显示页复用避免重复请求。 */
const apiPages = new Map<number, BrowseWorkItem[]>();
/** 请求序号守卫：快速翻页 / 切条件时丢弃过期响应。 */
let seq = 0;

/** 显示页数：total÷显示页条数 与 lastPage×切分数 取小（lastPage 是 pixiv 端硬上限）。 */
const pageCount = computed<number | null>(() => {
  const byTotal = total.value != null ? Math.ceil(total.value / PAGE_SIZE.value) : null;
  const byLast = lastPage.value != null ? lastPage.value * SEGMENTS_PER_API_PAGE.value : null;
  if (byTotal == null) return byLast;
  if (byLast == null) return byTotal;
  return Math.min(byTotal, byLast);
});

/** 加载当前显示页：命中接口页缓存直接切片，否则请求该接口页并缓存。 */
async function load(): Promise<void> {
  const keyword = word.value.trim();
  if (!keyword) return;
  const token = ++seq;
  loading.value = true;
  error.value = "";
  const apiPage = Math.ceil(page.value / SEGMENTS_PER_API_PAGE.value);
  const offset = ((page.value - 1) % SEGMENTS_PER_API_PAGE.value) * PAGE_SIZE.value;
  const cached = apiPages.get(apiPage);
  if (cached) {
    items.value = cached.slice(offset, offset + PAGE_SIZE.value);
    loading.value = false;
    afterLoad();
    return;
  }
  try {
    const data = await browseSearch(kind.value, keyword, {
      order: order.value,
      mode: mode.value,
      s_mode: sMode.value,
      // users入り 档位过滤：空串 = 不过滤，不向后端传参
      users_iri: usersIri.value || undefined,
      // novel 走 novels 端点不带 type；illust/manga 走 artworks 端点并带 type
      type: kind.value === "novel" ? undefined : kind.value,
      page: apiPage,
    });
    if (token !== seq) return;
    apiPages.set(apiPage, data.items);
    if (apiPages.size > MAX_API_PAGES) {
      apiPages.delete(apiPages.keys().next().value!);
    }
    total.value = data.total ?? null;
    lastPage.value = data.last_page ?? null;
    items.value = data.items.slice(offset, offset + PAGE_SIZE.value);
  } catch (err) {
    if (token !== seq) return;
    items.value = [];
    total.value = null;
    lastPage.value = null;
    error.value = errorMessage(err);
  } finally {
    if (token === seq) loading.value = false;
  }
  if (token === seq) afterLoad();
}

/** 整页替换后的收尾：回页首；处于计数排序时为新页补取计数。 */
function afterLoad(): void {
  scrollContentToTop();
  if (sortKey.value) void ensureCounts();
}

// ===== 本页排序（点赞 / 收藏 / 浏览 × 升/降；仅当前显示页本地排序）=====

type SortKey = "like" | "bookmark" | "view";
/** 本页排序维度：空串 = 默认顺序（接口顺序）；方向独立选择、跨维度保留。 */
const sortKey = ref<"" | SortKey>("");
const sortDir = ref<"asc" | "desc">("desc");

/** 三项计数缓存（`kind:id` → counts）；会话级，跨搜索 / 翻页复用。 */
const countsCache = new Map<string, WorkCounts>();
/** 计数缓存上限（超出淘汰最旧；被淘汰条目重选排序时重新补取）。 */
const MAX_COUNT_ENTRIES = 5000;
/** 缓存版本：普通 Map 非响应式，写入后自增以驱动 sortedItems 重算。 */
const countsVersion = ref(0);
const countsLoading = ref(false);
const countsTotal = ref(0);
/** 已完成计数补取的页面代次（seq）。未完成前保持接口顺序，避免计数分批到达引起多次重排。 */
const countsResolvedSeq = ref(-1);
/** 补取期间又来了新页请求：本轮结束后补跑一次。 */
let countsPending = false;

function countKey(item: BrowseWorkItem): string {
  return `${item.kind}:${item.id}`;
}

/** 当前显示页中所选排序维度仍缺计数的条目（零值是已知计数）。 */
function missingCountItems(): BrowseWorkItem[] {
  const key = sortKey.value;
  return key ? items.value.filter((it) => metricOf(it, key) === null) : [];
}

/**
 * 补取当前显示页缺失计数：整页一次 IPC（后端逐项请求详情，全局限速排队）。
 * 只有整页计数返回后才重排一次，避免分批到达导致顺序多次变化。
 * 整批失败提示并保留缺失；缺失项在重排时垫底。
 */
async function ensureCounts(): Promise<void> {
  if (countsLoading.value) {
    countsPending = true;
    return;
  }
  const missing = missingCountItems();
  if (!missing.length) return;
  const requestKind = kind.value;
  const requestSeq = seq;
  countsLoading.value = true;
  countsTotal.value = missing.length;
  try {
    const data = await browseWorkCounts(requestKind, missing.map((it) => it.id));
    for (const it of missing) {
      const counts = data.counts[String(it.id)];
      if (!counts) continue;
      countsCache.set(countKey(it), counts);
      if (countsCache.size > MAX_COUNT_ENTRIES) {
        countsCache.delete(countsCache.keys().next().value!);
      }
    }
  } catch (err) {
    notify(errorMessage(err) || t("browse.search.countsFailed"));
  } finally {
    // 无论成功与否都解除等待：失败时按已知计数重排一次，缺失项垫底。
    countsVersion.value += 1;
    countsResolvedSeq.value = requestSeq;
    countsLoading.value = false;
    if (countsPending) {
      countsPending = false;
      void ensureCounts();
    }
  }
}

function metricOf(item: BrowseWorkItem, key: SortKey): number | null {
  const counts = countsCache.get(countKey(item));
  const field = key === "like" ? "like_count" : key === "bookmark" ? "bookmark_count" : "view_count";
  return counts?.[field] ?? item[field] ?? null;
}

/**
 * 展示条目：默认 = 接口顺序；选中所选维度后，等整页计数补齐再一次性重排
 * （缺失项恒垫底）。计数未就绪期间保持接口顺序，避免中途多次跳序。
 */
const sortedItems = computed<BrowseWorkItem[]>(() => {
  void countsVersion.value; // 建立响应式依赖（countsCache 为普通 Map）
  const key = sortKey.value;
  const raw = items.value;
  if (!key) return raw;
  const awaiting =
    countsResolvedSeq.value !== seq && raw.some((it) => metricOf(it, key) === null);
  if (awaiting) return raw;
  const sign = sortDir.value === "desc" ? -1 : 1;
  return [...raw].sort((a, b) => {
    const av = metricOf(a, key);
    const bv = metricOf(b, key);
    if (av === null && bv === null) return 0;
    if (av === null) return 1;
    if (bv === null) return -1;
    return sign * (av - bv);
  });
});

function onSortKeyChange(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  sortKey.value = value === "like" || value === "bookmark" || value === "view" ? value : "";
  if (sortKey.value) void ensureCounts();
}

/** 方向切换：计数已就绪时由 sortedItems 立即重排（未选维度时控件禁用）。 */
function onSortDirChange(dir: "asc" | "desc"): void {
  sortDir.value = dir;
}

/** 翻页：整页替换（页码制）；回页首由 afterLoad 处理。 */
function onPagerChange({ page: next }: { page: number; pageSize: number | undefined }): void {
  if (next === page.value || loading.value) return;
  page.value = next;
  void load();
}

/** 最近一次已发起搜索的参数签名：URL 回显到达时据此去重，避免 replace 触发二次请求。 */
let lastFetchedSig = "";

function runSearch(sig: string): void {
  lastFetchedSig = sig;
  page.value = 1;
  total.value = null;
  lastPage.value = null;
  apiPages.clear();
  items.value = [];
  void load();
}

/**
 * 头部刷新（ListRefreshButton）：与 runSearch 不同，保留当前页码与筛选状态
 * （DESIGN.md：页码列表刷新保留当前页），仅使结果集缓存失效后按当前显示页重拉，
 * total / lastPage 由响应重算。
 */
function refreshCurrent(): void {
  if (!word.value.trim()) return;
  // 接口页缓存整体失效：本次重拉之外，后续翻页也重新请求
  apiPages.clear();
  // 本页排序的计数缓存一并失效：刷新应重新拉取，防止旧计数遮蔽新数据；
  // 版本号自增驱动 sortedItems 立即按新数据重算
  if (sortKey.value) {
    for (const it of items.value) countsCache.delete(countKey(it));
    countsVersion.value += 1;
  }
  void load();
}

// ===== URL ⇄ 状态 =====

function applyQuery(): void {
  // 离开本页时全局 route.query 也会变化，守卫避免误触发
  if (!route.path.startsWith("/browse/search")) return;
  const params = paramsFromQuery(route.query);
  word.value = params.word;
  kind.value = params.kind;
  order.value = params.order;
  mode.value = params.mode;
  sMode.value = params.sMode;
  usersIri.value = params.usersIri ?? "";
  if (!params.word) {
    // 无关键词（如从侧栏重新进入）：回到初始空态
    lastFetchedSig = "";
    total.value = null;
    lastPage.value = null;
    apiPages.clear();
    if (items.value.length || loading.value || error.value) {
      seq += 1; // 丢弃在途响应
      items.value = [];
      loading.value = false;
      error.value = "";
    }
    return;
  }
  const sig = sigOf(params);
  if (sig !== lastFetchedSig) runSearch(sig);
}

watch(() => route.query, applyQuery, { immediate: true });

function buildQuery(): Record<string, string> {
  const p = currentParams();
  const query: Record<string, string> = {
    kind: p.kind,
    order: p.order,
    mode: p.mode,
    s_mode: p.sMode,
  };
  if (p.word) query.word = p.word;
  // 不过滤时不写入，分享链接保持干净（回显侧空/缺参/非法值都回落不过滤）
  if (p.usersIri) query.users_iri = p.usersIri;
  return query;
}

// ===== 交互 =====

/** 过滤变化后提交：有关键词且参数有变则重新搜索，并始终把状态写回 URL。 */
function commit(): void {
  if (word.value.trim()) {
    const sig = sigOf(currentParams());
    if (sig !== lastFetchedSig) runSearch(sig);
  }
  void router.replace({ query: buildQuery() });
}

/** 回车 / 搜索按钮：先试 ID 直达（纯数字按当前类型 tab 归属），未命中按关键词搜索（同参数重按视为重试）。 */
function doSearch(): void {
  closeHistory();
  const raw = word.value.trim();
  const target = parseBrowseInput(raw, kind.value);
  if (target) {
    pushTarget(target);
    return;
  }
  if (!raw) return;
  word.value = raw;
  const sig = sigOf(currentParams());
  if (sig !== lastFetchedSig || error.value) runSearch(sig);
  void router.replace({ query: buildQuery() });
}

function onKindChange(value: string): void {
  kind.value = oneOf(["illust", "manga", "novel"] as const, value, "illust");
  commit();
}

function onOrderChange(event: Event): void {
  order.value = oneOf(ORDERS, (event.target as HTMLSelectElement).value, "date_d");
  commit();
}

function onModeChange(event: Event): void {
  mode.value = oneOf(MODES, (event.target as HTMLSelectElement).value, "all");
  commit();
}

function onSModeChange(event: Event): void {
  sMode.value = oneOf(S_MODES, (event.target as HTMLSelectElement).value, "s_tag");
  // 冲突双向收口：显式改匹配方式为 s_tag_full 时，最新操作优先——放弃档位过滤，
  // 保证「档位过滤 + 标签完全一致」组合永不产生（详见 normalizeSMode）
  if (sMode.value === "s_tag_full" && usersIri.value) {
    usersIri.value = "";
  }
  commit();
}

function onUsersIriChange(event: Event): void {
  // "none" 是「不过滤」选项哨兵值（md-select-option 不用空串 value）
  const raw = (event.target as HTMLSelectElement).value;
  usersIri.value = raw === "none" ? "" : usersIriOf(raw);
  // 启用档位过滤时 s_tag_full 组合必然失效，同步回落 s_tag（URL 与请求同批生效）
  if (usersIri.value && sMode.value === "s_tag_full") {
    sMode.value = "s_tag";
  }
  commit();
}

// ===== ID 直达与卡片跳转 =====

/** 链接识别结果 → 应用自绘页面；支持类型与扩展步骤见 utils/parseInput.ts 文件头。 */
function pushTarget(target: ParsedBrowseInput): void {
  void router.push(browseTargetPath(target));
}

/** 结果卡片跳转：novel → 阅读器，其余走插画/漫画查看器（ugoira V1 显示封面帧）。 */
function goWork(item: BrowseWorkItem): void {
  if (item.kind === "novel") void router.push(`/browse/work/novel/${item.id}`);
  else if (item.kind === "manga") void router.push(`/browse/work/manga/${item.id}`);
  else void router.push(`/browse/work/illust/${item.id}`);
}

// ===== 选项文案（computed：语言切换即时生效） =====

const typeTabs = computed(() => [
  { value: "illust", label: t("browse.search.typeIllust") },
  { value: "manga", label: t("browse.search.typeManga") },
  { value: "novel", label: t("browse.search.typeNovel") },
]);

// ===== 搜索历史下拉（后端在 browse_search 成功且第 1 页时权威记录，本页只读 + 删）=====

const historyItems = ref<BrowseSearchHistoryItem[]>([]);
const historyOpen = ref(false);
/** 键盘导航高亮下标（-1 = 无高亮，仍在输入框）。 */
const historyActive = ref(-1);
/** 输入框 + 下拉的 relative 容器：下拉定位锚点与外部点击判定范围。 */
const searchWrapEl = ref<HTMLElement | null>(null);
/** md-outlined-text-field 宿主：focusin 判定焦点是否落在搜索框内。 */
const searchFieldEl = ref<HTMLElement | null>(null);
/** 下拉面板：键盘导航时把高亮行滚入视野。 */
const historyPanelEl = ref<HTMLElement | null>(null);

/** 类型徽标文案复用类型 tab 的既有键。 */
const HISTORY_KIND_LABEL_KEYS: Record<string, string> = {
  illust: "browse.search.typeIllust",
  manga: "browse.search.typeManga",
  novel: "browse.search.typeNovel",
};

async function refreshHistory(): Promise<void> {
  try {
    historyItems.value = await listBrowseSearchHistory();
  } catch {
    // 历史是辅助信息，读取失败静默保留旧列表，不打断搜索主流程
  }
}

/**
 * 展开下拉：每次展开都重拉一次（后端权威记录，「每次成功搜索后」的刷新时机
 * 由展开时重拉覆盖——搜索完成后再聚焦必经展开），无需在各搜索路径补挂刷新。
 */
async function openHistory(): Promise<void> {
  historyActive.value = -1;
  historyOpen.value = true;
  void refreshHistory();
}

function closeHistory(): void {
  historyOpen.value = false;
  historyActive.value = -1;
}

/** 焦点进入搜索框时展开（focusin 可穿透 Material 组件的 shadow DOM 冒泡到宿主）。 */
function onSearchFocusIn(event: FocusEvent): void {
  const field = searchFieldEl.value;
  if (field && event.target instanceof Node && field.contains(event.target)) void openHistory();
}

/** 输入时也展开（「聚焦 / 输入时出历史下拉」）；已展开时不重复重拉，避免逐键 IPC。 */
function onSearchInput(event: Event): void {
  word.value = (event.target as HTMLInputElement).value;
  if (!historyOpen.value) void openHistory();
}

/** 外部点击关闭：容器（输入框 + 下拉）之外按下指针即收起。 */
function onDocPointerDown(event: PointerEvent): void {
  const root = searchWrapEl.value;
  if (!root || !(event.target instanceof Node) || root.contains(event.target)) return;
  closeHistory();
}

/** 条目元信息：相对时间（分钟/小时/天，超 30 天回落短日期；共用 utils/relativeTime）。 */
function historyTime(item: BrowseSearchHistoryItem): string {
  return formatRelativeTime(item.searched_at, locale.value, {
    justNow: t("browse.searchHistory.justNow"),
    minutesAgo: (n) => t("browse.searchHistory.minutesAgo", { n }),
    hoursAgo: (n) => t("browse.searchHistory.hoursAgo", { n }),
    daysAgo: (n) => t("browse.searchHistory.daysAgo", { n }),
  });
}

function historyKindLabel(kind: string): string {
  return t(HISTORY_KIND_LABEL_KEYS[kind] ?? "browse.search.typeIllust");
}

/** 键盘导航后把高亮行滚入视野（block: nearest 不扰动面板滚动位置）。 */
async function scrollActiveIntoView(): Promise<void> {
  await nextTick();
  const panel = historyPanelEl.value;
  if (!panel || historyActive.value < 0) return;
  panel
    .querySelector<HTMLElement>(`[data-history-idx="${historyActive.value}"]`)
    ?.scrollIntoView({ block: "nearest" });
}

/**
 * 搜索框键盘：↑↓ 移动高亮、Enter 选中高亮项（无高亮回落既有搜索语义）、
 * Esc 关闭（下拉关闭时 Esc 不拦截，交回默认行为）。仅在消费按键时 preventDefault。
 */
function onSearchKeydown(event: KeyboardEvent): void {
  if (event.key === "Enter") {
    if (historyOpen.value && historyActive.value >= 0) {
      const item = historyItems.value[historyActive.value];
      // 高亮项存在 → 选中该历史并返回（不再走普通搜索）；异常缺项回落普通搜索
      if (item) {
        event.preventDefault();
        void selectHistory(item);
        return;
      }
    }
    doSearch();
    return;
  }
  if (!historyOpen.value || !historyItems.value.length) return;
  if (event.key === "ArrowDown") {
    event.preventDefault();
    historyActive.value = Math.min(historyActive.value + 1, historyItems.value.length - 1);
    void scrollActiveIntoView();
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    historyActive.value = Math.max(historyActive.value - 1, -1);
    void scrollActiveIntoView();
  } else if (event.key === "Escape") {
    event.preventDefault();
    closeHistory();
  }
}

/**
 * 点击历史条目：按六元组直接构造查询参数经 router.replace 回写本页 URL，
 * 复用既有的 query → applyQuery → runSearch 自动搜索机制；空缺 / 非法过滤器
 * 按本页默认值回落（s_mode 与 users_iri 冲突同批规范化，与 paramsFromQuery 同口径）。
 * 历史词都是关键词，不经 parseBrowseInput 的 ID 直达解析。
 */
function selectHistory(item: BrowseSearchHistoryItem): void {
  if (loading.value) return;
  closeHistory();
  const usersIri = usersIriOf(item.users_iri);
  const query: Record<string, string> = {
    word: item.word,
    kind: oneOf(["illust", "manga", "novel"] as const, item.kind, "illust"),
    order: oneOf(ORDERS, item.order, "date_d"),
    mode: oneOf(MODES, item.mode, "all"),
    s_mode: normalizeSMode(usersIri, oneOf(S_MODES, item.s_mode, "s_tag")),
  };
  if (usersIri) query.users_iri = usersIri;
  void router.replace({ query });
}

/** 单条删除：不确认（对齐以图识图历史），失败 notify；删除后清键盘高亮。 */
async function deleteHistory(item: BrowseSearchHistoryItem): Promise<void> {
  if (loading.value) return;
  try {
    await deleteBrowseSearchHistory(item.id);
    historyItems.value = historyItems.value.filter((entry) => entry.id !== item.id);
    historyActive.value = -1;
  } catch (err) {
    notify(errorMessage(err));
  }
}

onMounted(() => {
  void refreshHistory();
  document.addEventListener("pointerdown", onDocPointerDown);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocPointerDown);
});

// 页面被 KeepAlive 缓存：回到本页时收起下拉（焦点已丢失）并重拉一次历史
onActivated(() => {
  closeHistory();
  void refreshHistory();
});
</script>

<template>
  <div class="page-view">
    <div class="app-page-head">
      <div class="browse-list-header">
        <div class="page-heading"><PageBackButton /><h1 class="page-title">{{ t("nav.browseSearch") }}</h1></div>
        <md-text-button @click="router.push('/saucenao')">{{ t('nav.saucenao') }}</md-text-button>
        <ListRefreshButton :busy="loading" :disabled="!word.trim()" @refresh="refreshCurrent" />
      </div>

      <!-- 搜索工具行：大而醒目的输入框 + 搜索按钮；输入容器 relative 承载历史下拉 -->
      <div class="tool-row">
        <div ref="searchWrapEl" class="search-wrap" @focusin="onSearchFocusIn">
          <md-outlined-text-field
            ref="searchFieldEl"
            class="search-field"
            :value="word"
            :placeholder="t('browse.search.placeholder')"
            :aria-label="t('nav.browseSearch')"
            @input="onSearchInput"
            @keydown="onSearchKeydown"
          />
          <!-- 历史下拉：贴输入框下沿、与其同宽；空历史不渲染。等价按钮组
             （原生 button 各带可读名称），容器 role=group + aria-label 命名 -->
          <div
            v-if="historyOpen && historyItems.length"
            ref="historyPanelEl"
            class="history-drop"
            role="group"
            :aria-label="t('browse.searchHistory.label')"
          >
            <div
              v-for="(item, i) in historyItems"
              :key="item.id"
              class="history-drop-row"
              :class="{ 'is-active': i === historyActive }"
              :data-history-idx="i"
            >
              <button
                type="button"
                class="history-main"
                :disabled="loading"
                :title="item.word"
                @click="selectHistory(item)"
              >
                <span class="history-word">{{ item.word }}</span>
                <span class="history-kind" aria-hidden="true">{{ historyKindLabel(item.kind) }}</span>
                <span class="history-time">{{ historyTime(item) }}</span>
              </button>
              <button
                type="button"
                class="history-remove"
                :aria-label="t('browse.searchHistory.delete')"
                :title="t('browse.searchHistory.delete')"
                :disabled="loading"
                @click.stop="deleteHistory(item)"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <!-- lucide x -->
                  <path d="M18 6 6 18" />
                  <path d="m6 6 12 12" />
                </svg>
              </button>
            </div>
          </div>
        </div>
        <md-filled-button class="search-btn" @click="doSearch">
          {{ t("common.search") }}
        </md-filled-button>
        <HelpTooltip :label="t('common.search')" :text="t('browse.search.help')" />
      </div>

      <!-- 过滤行：类型 tabs + 排序 / 对象 / 匹配 / users入り 档位下拉 -->
      <div class="filter-row">
        <div class="type-tabs-wrap" role="group" :aria-label="t('browse.search.typeLabel')">
          <SectionTabs class="type-tabs" :tabs="typeTabs" :value="kind" @change="onKindChange" />
        </div>
        <label class="select-item">
          <span>{{ t("browse.search.orderLabel") }}</span>
          <md-outlined-select :value="order" :aria-label="t('browse.search.orderLabel')" @change="onOrderChange">
            <md-select-option value="date_d">{{ t("browse.search.orderDateDesc") }}</md-select-option>
            <md-select-option value="date">{{ t("browse.search.orderDateAsc") }}</md-select-option>
            <md-select-option value="popular_d">{{ t("browse.search.orderPopular") }}</md-select-option>
          </md-outlined-select>
        </label>
        <label class="select-item">
          <span>{{ t("browse.search.modeLabel") }}</span>
          <md-outlined-select :value="mode" :aria-label="t('browse.search.modeLabel')" @change="onModeChange">
            <md-select-option value="all">{{ t("browse.search.modeAll") }}</md-select-option>
            <md-select-option value="safe">{{ t("browse.search.modeSafe") }}</md-select-option>
            <md-select-option value="r18">{{ t("browse.search.modeR18") }}</md-select-option>
          </md-outlined-select>
        </label>
        <label class="select-item">
          <span>{{ t("browse.search.sModeLabel") }}</span>
          <md-outlined-select :value="sMode" :aria-label="t('browse.search.sModeLabel')" @change="onSModeChange">
            <md-select-option value="s_tag">{{ t("browse.search.sModeTag") }}</md-select-option>
            <md-select-option value="s_tag_full">{{ t("browse.search.sModeTagFull") }}</md-select-option>
            <md-select-option value="s_tc">{{ t("browse.search.sModeTc") }}</md-select-option>
          </md-outlined-select>
        </label>
        <!-- users入り 收藏数档位过滤（搜索词过滤非排序，选项「N+」表达下限语义） -->
        <label class="select-item">
          <span>{{ t("browse.search.usersIriLabel") }}</span>
          <md-outlined-select
            :value="usersIri || 'none'"
            :aria-label="t('browse.search.usersIriLabel')"
            @change="onUsersIriChange"
          >
            <md-select-option value="none">{{ t("browse.search.usersIriNone") }}</md-select-option>
            <md-select-option v-for="lv in USERS_IRI_LEVELS" :key="lv" :value="lv">{{ lv }}+</md-select-option>
          </md-outlined-select>
        </label>
        <!-- 本页排序：维度下拉 + 升降方向切换；仅对当前显示页本地排序（计数按需补取） -->
        <div class="local-sort">
          <span class="local-sort-label" aria-hidden="true">{{ t("browse.search.localSortLabel") }}</span>
          <md-outlined-select
            :value="sortKey || 'default'"
            :disabled="!items.length"
            :aria-label="t('browse.search.localSortLabel')"
            @change="onSortKeyChange"
          >
            <md-select-option value="default">{{ t("browse.search.localSortDefault") }}</md-select-option>
            <md-select-option value="like">{{ t("browse.search.localSortLike") }}</md-select-option>
            <md-select-option value="bookmark">{{ t("browse.search.localSortBookmark") }}</md-select-option>
            <md-select-option value="view">{{ t("browse.search.localSortView") }}</md-select-option>
          </md-outlined-select>
          <SortDirectionToggle
            :value="sortDir"
            :disabled="!sortKey || !items.length"
            @change="onSortDirChange"
          />
        </div>
      </div>
    </div>

    <div class="result-bar">
      <p v-if="total !== null" class="total-line" role="status">
        {{ t("browse.search.total", { count: total }) }}
      </p>
      <span v-if="countsLoading" class="counts-progress" role="status">
        {{ t("browse.search.countsLoading", { total: countsTotal }) }}
      </span>
    </div>

    <!-- 查询容器：供 WorkGrid 的分页列数档位（2/4/5/10 列）按实际内容宽度查询 -->
    <div class="grid-host">
      <WorkGrid
        :items="sortedItems"
        :loading="loading"
        :error="error"
        :page-size="PAGE_SIZE"
        paginated
        @retry="load()"
        @select="goWork"
      />
    </div>

    <!-- 页码分页（插画/漫画 20 条/页 = 接口页 60 条切 3 页；小说 30 条/页 = 接口页整页） -->
    <AppPagination
      v-if="pageCount !== null && pageCount > 1"
      class="pager"
      :current-page="page"
      :total-pages="pageCount"
      :disabled="loading"
      @change="onPagerChange"
    />
  </div>
</template>

<style scoped>
.browse-list-header {
  margin-bottom: var(--space-md);
}

.tool-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
  margin-bottom: var(--space-md);
}

/* 输入容器（relative）：尺寸职责从输入框上移（窄窗换行由 tool-row 承接），
   并作为历史下拉的定位锚点与外部点击判定范围 */
.search-wrap {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 240px;
}

.search-field {
  flex: 1;
  min-width: 0;
}

.search-btn {
  flex: none;
}

/* ===== 搜索历史下拉：贴输入框下沿、与其同宽（left/right 归零，窄窗口不横向溢出）=====
   容器配方与账号菜单 / 任务选择菜单 popover 同源：surface-container 底、control 圆角、
   唯一合法轻阴影；面板在吸顶页头 stacking context（.app-page-head：sticky + z=5 +
   isolation）内。z=2 是压住过滤行的最小值：下方各 md 组件内部描边 / 指示条层为
   z=1（@material/web outlined-styles 与 tabs 的既定值，md 宿主自身不建 stacking
   context），z=1 会与之打平、按 DOM 序被后绘的过滤行盖住；z=2 压回全部过滤层，
   且不越过 md-menu 弹层的 z=20。≤200 条超出 max-height 后内部滚动。 */
.history-drop {
  position: absolute;
  top: calc(100% + var(--space-xs));
  left: 0;
  right: 0;
  z-index: 2;
  max-height: min(60vh, 320px);
  padding: var(--space-xs);
  overflow-y: auto;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
}

/* 条目行：hover 与键盘高亮同为 8% primary 状态层（菜单行惯例），control 圆角随行 */
.history-drop-row {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  min-height: 40px;
  padding: 0 var(--space-xs) 0 var(--space-md);
  border-radius: var(--radius-control);
  transition: background-color 0.15s ease;
}

.history-drop-row:hover,
.history-drop-row.is-active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

/* 行主体（关键词 + 类型徽标 + 相对时间）是可点按钮；删除 X 为兄弟节点，不嵌套 */
.history-main {
  display: flex;
  flex: 1;
  align-items: center;
  gap: var(--space-sm);
  min-width: 0;
  min-height: 40px;
  padding: 0;
  font: inherit;
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: var(--radius-control);
  cursor: pointer;
}

.history-main:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.history-main:disabled {
  cursor: default;
}

/* 关键词加粗（票面要求），单行省略保留完整值于行 title */
.history-word {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--ink);
}

/* 类型徽标：中性描边胶囊（与凭据状态胶囊同配方，非选中语义不用容器色） */
.history-kind {
  display: inline-flex;
  flex: none;
  align-items: center;
  min-height: 20px;
  padding: 0 var(--space-sm);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent);
  border-radius: 999px;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
}

/* 相对时间：12px ink-muted 辅助层级，等宽数字防抖动 */
.history-time {
  flex: none;
  font-size: 12px;
  color: var(--ink-muted);
  font-variant-numeric: tabular-nums;
}

/* 单条删除：28px 快捷动作配方（同以图识图历史 / 卡片快捷动作），hover / focus-within 显现 */
.history-remove {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  cursor: pointer;
  opacity: 0;
  transition: background-color 0.15s ease, opacity 0.15s ease;
}

.history-drop-row:hover .history-remove,
.history-drop-row:focus-within .history-remove {
  opacity: 1;
}

/* 触摸设备没有 hover：删除按钮常驻（同全站 hover 显现控件惯例） */
@media (hover: none) {
  .history-remove {
    opacity: 1;
  }
}

.history-remove:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.history-remove:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, var(--md-sys-color-surface-container));
}

.history-remove:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.history-remove:disabled {
  cursor: default;
}

.history-remove svg {
  width: 16px;
  height: 16px;
}

/* 过滤行：tabs + 排序/对象/匹配/users入り + 行尾本页排序。与全站筛选行统一 wrap
   配方：常规宽度自然单行（本页排序组 margin-left: auto 靠右），宽度不足时整项
   换行而非压缩——禁用收缩（flex: none）是因为 Material 下拉被压缩后文字会溢出
   边框与相邻标签重叠；断点随实际内容宽度生效，不依赖视口媒体查询（内容可用
   宽度受侧栏展开/收起与超宽窗口封顶影响，视口宽度判断不了）。 */
.filter-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm) var(--space-md);
  margin-top: 0;
  margin-bottom: var(--space-md);
}

.type-tabs-wrap {
  flex: none;
}

.select-item {
  display: flex;
  flex: none;
  align-items: center;
  gap: var(--space-xs);
}

/* 标签 12px 小字；下拉贴合选中项文本宽度（不设 flex: 1，避免等分撑满整行） */
.select-item > span,
.local-sort-label {
  flex: none;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

/* 下拉按选中项文本定宽、不收缩：宽度不足时 Material 内部标签会换行增高（64px） */
.select-item md-outlined-select {
  flex: none;
  width: auto;
  min-width: 0;
}

/* 分页网格的查询容器：WorkGrid.paginated 按内容宽度切换列数档位（每页条数的因数：
   插画/漫画 20 条 → 2/4/5/10 列，小说 30 条 → 2/3/5/6/10 列） */
.grid-host {
  container-type: inline-size;
}

/* 结果行：「约 N 件」+ 计数补取进度（本页排序控件与筛选控件同行，见 .local-sort） */
.result-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs) var(--space-md);
  margin-top: var(--space-lg);
}

.total-line {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

.counts-progress {
  font-size: 12px;
  color: var(--ink-subtle);
}

.local-sort {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: var(--space-xs);
  margin-left: auto;
}

.local-sort md-outlined-select {
  flex: none;
  width: auto;
  min-width: 0;
}

/* 分页行：仅负责与网格的间距（行内布局由 AppPagination 承担） */
.pager {
  margin-top: var(--space-lg);
}

/* 视口 ≤ 640px：纵向堆叠（更窄窗口由过滤行自身的 wrap 逐级承接） */
@media (max-width: 640px) {
  /* 工具行换行：输入框与按钮纵向堆叠，输入框占满整行 */
  .tool-row {
    flex-direction: column;
    align-items: stretch;
  }

  .filter-row {
    flex-direction: column;
    align-items: stretch;
  }

  /* 堆叠布局下下拉撑满整行 */
  .select-item md-outlined-select,
  .local-sort md-outlined-select {
    flex: 1;
    min-width: 0;
  }
}
</style>
