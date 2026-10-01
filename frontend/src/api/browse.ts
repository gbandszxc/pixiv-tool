/**
 * 浏览模式 IPC 层 —— IPC 契约 v2 见 `.subdriver/browse-ui-v1.plan.md`（唯一权威，
 * 2026-10-01 依 docs/research/pixiv-browse-api.md 实测修订）。
 *
 * 组成：
 * - 契约类型（snake_case，与 Rust browse 命令返回体一致）
 * - 11 个命令的 invoke 封装（未登录错误 → 派发 `pixiv-tool:open-login` 事件，App.vue 负责弹登录窗）
 * - pxSrc()：pximg 封面 URL → `pixiv-img://` 代理协议（Tauri 环境）
 * - mock 层：`!isTauri()`（普通浏览器直接打开 dev 页）时返回样例数据，
 *   仅供浏览器内视觉验收使用；Tauri 生产环境完全不走 mock。
 *   除 browse_discover（每次调用换种子，配合「重复调用 + 按 id 去重」）外均为确定性数据。
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { errorMessage, invoke, isTauri } from "./tauri";

// 复用 tauri.ts 的基础封装，下游浏览相关组件可只 import 本文件。
export { errorMessage, invoke, isTauri };

// ===== 契约类型（snake_case）=====

export type WorkKind = "illust" | "manga" | "ugoira" | "novel";

/** browse_channel 的 kind 取值（illustration 是 pixiv 端命名，区别于 WorkKind.illust）。 */
export type ChannelKind = "illustration" | "manga" | "novel";
/** browse_follow_latest 的 kind / mode 取值。 */
export type FeedKind = "illust" | "novel";
export type FeedMode = "all" | "r18";
/** browse_search / browse_related / browse_user_works 的 kind 取值。 */
export type ListWorkKind = "illust" | "manga" | "novel";
/** browse_ranking 的 kind（ranking 端点含 ugoira 榜）。 */
export type RankingKind = "illust" | "manga" | "ugoira" | "novel";
/**
 * browse_ranking 的 mode。合法组合依 kind 而定（v2 契约）：
 * - illust / manga / ugoira：daily | weekly | monthly | rookie | daily_r18 | weekly_r18
 * - novel：daily | weekly | monthly | male | female | daily_r18
 */
export type RankingMode =
  | "daily"
  | "weekly"
  | "monthly"
  | "rookie"
  | "daily_r18"
  | "weekly_r18"
  | "male"
  | "female";

/** 浏览列表通用作品条目。 */
export interface BrowseWorkItem {
  id: number;
  kind: WorkKind;
  title: string;
  author_id: number;
  author_name: string;
  profile_img?: string;
  /** pximg 缩略图 URL 原样返回（前端经 pxSrc() → pixiv-img 代理显示） */
  cover?: string;
  page_count: number;
  /** 0 无 | 1 R-18 | 2 R-18G */
  x_restrict?: number;
  tags?: string[];
  create_date?: string;
  /** novel 专用：字数 */
  text_length?: number;
  series_id?: number | null;
  series_title?: string | null;
  /** ranking 专用：名次 */
  rank?: number;
}

/**
 * 浏览列表通用返回体。分页语义按命令而定：
 * - 有翻页（follow_latest/search/user_works/ranking）：next_page = null 即无更多；
 * - 无翻页（home/discover/related/channel 板块）：next_page 恒为 null，
 *   前端「换一批」重复调用并按 id 去重。
 */
export interface BrowseList {
  items: BrowseWorkItem[];
  total?: number | null;
  next_page?: number | null;
  /** follow_latest 等命令的后端终页标记 */
  is_last_page?: boolean;
}

/** browse_channel 返回体：频道页一次性快照（四个板块 + 热门标签）。 */
export interface BrowseChannel {
  follow: BrowseList;
  recommend: BrowseList;
  ranking: BrowseList;
  new_post: BrowseList;
  trending_tags: { name: string; translated_name?: string; count?: number }[];
  ranking_date?: string | null;
}

/** browse_ranking 返回体（date 为榜单日期 YYYYMMDD；prev/next 日期供前后日切换）。 */
export interface BrowseRanking {
  items: BrowseWorkItem[];
  date: string;
  prev_date?: string | null;
  next_date?: string | null;
  next_page?: number | null;
}

/** browse_user_profile 返回体。 */
export interface BrowseUserProfile {
  id: number;
  name: string;
  pixiv_id: string;
  profile_img: string;
  comment_html?: string;
  background?: string | null;
  following_count?: number;
  mypixiv_count?: number;
}

/** browse_work_detail（illust/manga）返回体；detail_kind 用于运行时区分联合类型。 */
export interface BrowseIllustDetail {
  detail_kind: "illust";
  item: BrowseWorkItem & {
    description?: string;
    width?: number;
    height?: number;
    view_count?: number;
    like_count?: number;
    bookmark_count?: number;
    bookmarked?: boolean;
  };
  /** 单页作品也返回 1 项 */
  pages: { small?: string; medium?: string; original: string; width?: number; height?: number }[];
  /** ugoira 动图帧信息；V1 只显示封面帧 */
  ugoira?: { src: string; frames: { file: string; delay: number }[] } | null;
  /** 所属系列（页面顶部入口） */
  series?: { id: number; title: string; order: number } | null;
}

/** browse_work_detail（novel）返回体；content 为全文（保留 [newpage]/[chapter:]/[rb:]/[pixivimage:] 原始标记，前端切分）。 */
export interface BrowseNovelDetail {
  detail_kind: "novel";
  item: BrowseWorkItem & {
    description?: string;
    bookmark_count?: number;
    reading_time?: number;
  };
  content: string;
  /** seriesNavData：系列导航（order 为本书在系列中的序号） */
  series?: { id: number; title: string; order: number; next_id?: number | null } | null;
}

export type BrowseWorkDetail = BrowseIllustDetail | BrowseNovelDetail;

/** browse_novel_series 返回体：系列目录，游标 last_order 分页。 */
export interface BrowseSeriesDetail {
  id: number;
  title: string;
  user_id: number;
  user_name: string;
  caption?: string;
  cover?: string;
  total: number;
  is_concluded?: boolean;
  contents: {
    id: number;
    title: string;
    series_order: number;
    text_length?: number;
    update_date?: string;
    x_restrict?: number;
  }[];
  /** 游标：下一批请求带回；null = 到底 */
  next_last_order?: number | null;
}

// ===== 图片代理 helper =====

/**
 * pximg 封面/图片 URL → 可直接用于 <img :src> 的地址。
 * - Tauri：走后端 `pixiv-img` 自定义协议（URL encode 后交给协议解析，带磁盘缓存）。
 * - 非 Tauri：原样返回（mock 封面是 data URI，浏览器内可直接显示）。
 */
export function pxSrc(url?: string): string {
  if (!url) return "";
  if (!isTauri()) return url;
  // 传原始 URL：convertFileSrc 在 Windows 侧会做一次 encodeURIComponent，
  // 后端 image_proxy 恰好按「单次编码」解码；这里再预编码会导致双重编码、
  // 后端白名单解析失败（pixiv-img 403）。非 Windows 平台不编码，由后端兼容。
  return convertFileSrc(url, "pixiv-img");
}

// ===== 未登录错误联动 =====

/** 打开登录弹窗的自定义事件名（App.vue 监听并复用现有 LoginDialog）。 */
export const OPEN_LOGIN_EVENT = "pixiv-tool:open-login";

/**
 * 未登录错误特征：按 plan 约定，后端 browse 命令的未登录 detail 文案含「登录」
 * （如「未登录」「请先登录」「登录已过期」），或显式英文标记。
 */
const LOGIN_ERROR_RE = /登录|not\s*logged\s*in|login\s*required/i;

function looksLikeLoginError(err: unknown): boolean {
  return LOGIN_ERROR_RE.test(errorMessage(err));
}

/** invoke 封装：未登录类业务错误时派发 OPEN_LOGIN_EVENT，随后原样抛出。 */
async function invokeBrowse<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    if (looksLikeLoginError(err)) {
      window.dispatchEvent(new CustomEvent(OPEN_LOGIN_EVENT));
    }
    throw err;
  }
}

// ===== 命令封装（!isTauri() → mock）=====

/** browse_home_feed：首页推荐（无翻页；前端「换一批」重复调用并去重）。 */
export async function browseHomeFeed(): Promise<BrowseList> {
  if (!isTauri()) return mockOneShot("home", ["illust", "manga", "novel", "ugoira"], { countRange: [25, 40] });
  return invokeBrowse<BrowseList>("browse_home_feed");
}

/** browse_channel：频道页快照（插画 / 漫画 / 小说），一次返回四板块 + 热门标签。 */
export async function browseChannel(kind: ChannelKind): Promise<BrowseChannel> {
  if (!isTauri()) return mockChannel(kind);
  return invokeBrowse<BrowseChannel>("browse_channel", { kind });
}

/** browse_discover：发现（仅作品无小说；无翻页参数，前端重复调用按 id 去重追加）。 */
export async function browseDiscover(): Promise<BrowseList> {
  if (!isTauri()) return mockDiscover();
  return invokeBrowse<BrowseList>("browse_discover");
}

/** browse_follow_latest：关注动态（next_page = isLastPage ? null : p+1）。 */
export async function browseFollowLatest(
  kind: FeedKind,
  mode: FeedMode,
  page: number
): Promise<BrowseList> {
  if (!isTauri()) return mockPaginated(`feed:${kind}:${mode}`, page, [kind]);
  return invokeBrowse<BrowseList>("browse_follow_latest", { kind, mode, page });
}

/** browse_search：搜索（含 total；type 过滤 illust/manga/ugoira）。 */
export async function browseSearch(
  kind: ListWorkKind,
  word: string,
  options?: { order?: string; mode?: string; s_mode?: string; type?: string; page?: number }
): Promise<BrowseList> {
  const page = options?.page ?? 1;
  if (!isTauri()) return mockPaginated(`search:${kind}:${word}`, page, [kind], { withTotal: true });
  return invokeBrowse<BrowseList>("browse_search", {
    kind,
    word,
    order: options?.order,
    mode: options?.mode,
    sMode: options?.s_mode,
    type: options?.type,
    page,
  });
}

/** browse_ranking：排行榜（p 每页 50；date 供历史榜单切换）。 */
export async function browseRanking(
  kind: RankingKind,
  mode: RankingMode,
  page = 1,
  date?: string
): Promise<BrowseRanking> {
  if (!isTauri()) return mockRanking(kind, mode, page, date);
  return invokeBrowse<BrowseRanking>("browse_ranking", { kind, mode, page, date });
}

/** browse_work_detail：作品详情（illust/manga → BrowseIllustDetail，novel → BrowseNovelDetail）。 */
export async function browseWorkDetail(
  kind: ListWorkKind,
  id: number
): Promise<BrowseWorkDetail> {
  if (!isTauri()) return kind === "novel" ? mockNovelDetail(id) : mockIllustDetail(kind, id);
  return invokeBrowse<BrowseWorkDetail>("browse_work_detail", { kind, id });
}

/** browse_related：相关推荐（一次性池，limit 默认 30，page 无效）。 */
export async function browseRelated(
  kind: ListWorkKind,
  id: number,
  limit = 30
): Promise<BrowseList> {
  if (!isTauri()) return mockOneShot(`related:${kind}:${id}`, [kind], { count: limit });
  return invokeBrowse<BrowseList>("browse_related", { kind, id, limit });
}

/** browse_user_profile：作者信息。 */
export async function browseUserProfile(id: number): Promise<BrowseUserProfile> {
  if (!isTauri()) return mockUserProfile(id);
  return invokeBrowse<BrowseUserProfile>("browse_user_profile", { id });
}

/** browse_user_works：作者作品（后端按 id 全集降序切片 60/批）。 */
export async function browseUserWorks(
  id: number,
  kind: ListWorkKind,
  page = 1
): Promise<BrowseList> {
  if (!isTauri()) return mockUserWorks(id, kind, page);
  return invokeBrowse<BrowseList>("browse_user_works", { id, kind, page });
}

/** browse_novel_series：小说系列目录（游标 last_order 分页，每批 30；mock 12/批）。 */
export async function browseNovelSeries(id: number, lastOrder = 0): Promise<BrowseSeriesDetail> {
  if (!isTauri()) return mockSeriesDetail(id, lastOrder);
  return invokeBrowse<BrowseSeriesDetail>("browse_novel_series", { id, lastOrder });
}

function channelKindToWork(kind: ChannelKind): WorkKind {
  return kind === "illustration" ? "illust" : kind;
}

// ===== mock 层（仅非 Tauri 环境生效）=====
//
// 用途：普通浏览器直接打开 dev 页（pnpm dev 后 localhost:9961）时做视觉验收，
// 不依赖 Rust 后端。数据由种子伪随机生成：除 mockDiscover（模块内递增计数参与种子，
// 每次调用不同，配合前端「按 id 去重」的无限滚动）外，同一命令 + 参数永远返回相同内容；
// 封面为内联 SVG data URI。生产 Tauri 环境不会进入任何 mock 分支。

const MOCK_DELAY_MS = 300;
/** 分页类命令的最大页数（之后 next_page = null），用于验收无限滚动收尾。 */
const MOCK_MAX_PAGES = 4;

const MOCK_AUTHORS = ["星空ゆめ", "青野原千鶴", "雾岛静", "白露川音", "toshi", "篠原こま", "南云时雨", "栗山もか"];
const MOCK_TAGS = ["オリジナル", "風景", "女の子", "漫画", "水彩", "日常", "幻想", "創作", "背景", "眼鏡"];
const MOCK_TRENDING_TAGS: { name: string; translated_name: string }[] = [
  { name: "オリジナル", translated_name: "原创" },
  { name: "風景", translated_name: "风景" },
  { name: "女の子", translated_name: "女孩" },
  { name: "漫画", translated_name: "漫画" },
  { name: "水彩", translated_name: "水彩" },
  { name: "日常", translated_name: "日常" },
  { name: "幻想", translated_name: "幻想" },
  { name: "創作", translated_name: "创作" },
  { name: "眼鏡", translated_name: "眼镜" },
  { name: "背景", translated_name: "背景" },
];

/** mulberry32 种子伪随机：确定性、无依赖。 */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** FNV-1a：命令名 + 参数 → 稳定种子。 */
function seedFrom(key: string): number {
  let h = 2166136261;
  for (const ch of key) {
    h ^= ch.codePointAt(0) ?? 0;
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function mockDelay(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, MOCK_DELAY_MS));
}

function pad2(v: number): string {
  return String(v).padStart(2, "0");
}

/** 今日日期 YYYYMMDD（ranking mock 的榜单日期基底）。 */
function todayYmd(): string {
  const d = new Date();
  return `${d.getFullYear()}${pad2(d.getMonth() + 1)}${pad2(d.getDate())}`;
}

function ymdShift(ymd: string, deltaDays: number): string {
  const dt = new Date(
    Number(ymd.slice(0, 4)),
    Number(ymd.slice(4, 6)) - 1,
    Number(ymd.slice(6, 8)) + deltaDays
  );
  return `${dt.getFullYear()}${pad2(dt.getMonth() + 1)}${pad2(dt.getDate())}`;
}

/** browse_discover 的「换一批」计数：参与种子，保证每次调用数据不同。 */
let mockDiscoverCallCount = 0;

/** 生成内联 SVG data-URI 封面：不同色相 + 三种横竖比例（1:1.4 / 1:1 / 4:3）。 */
function mockCover(hue: number, ratio: "portrait" | "square" | "landscape"): string {
  const size = { portrait: [320, 448], square: [320, 320], landscape: [320, 240] }[ratio];
  const [w, h] = size;
  const h2 = (hue + 42) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="hsl(${hue} 52% 74%)"/><stop offset="1" stop-color="hsl(${h2} 46% 52%)"/>` +
    `</linearGradient></defs>` +
    `<rect width="${w}" height="${h}" fill="url(#g)"/>` +
    `<circle cx="${w * 0.72}" cy="${h * 0.26}" r="${Math.min(w, h) * 0.13}" fill="hsl(${hue} 70% 90%)"/>` +
    `<path d="M0 ${h * 0.74} Q ${w * 0.28} ${h * 0.56} ${w * 0.52} ${h * 0.72} T ${w} ${h * 0.66} V ${h} H 0 Z" fill="hsl(${(hue + 200) % 360} 38% 34%)" opacity="0.6"/>` +
    `<path d="M0 ${h * 0.86} Q ${w * 0.34} ${h * 0.72} ${w * 0.64} ${h * 0.86} T ${w} ${h * 0.82} V ${h} H 0 Z" fill="hsl(${(hue + 220) % 360} 34% 24%)" opacity="0.55"/>` +
    `</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

function pick<T>(rand: () => number, arr: readonly T[], count: number): T[] {
  const pool = [...arr];
  const out: T[] = [];
  for (let i = 0; i < count && pool.length; i += 1) {
    out.push(pool.splice(Math.floor(rand() * pool.length), 1)[0]);
  }
  return out;
}

function makeMockItem(rand: () => number, n: number, kind: WorkKind): BrowseWorkItem {
  const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
  const portrait = kind === "novel";
  const coverRatio = portrait ? "portrait" : rand() < 0.35 ? "landscape" : "square";
  const pageCount =
    kind === "novel" ? 1 : rand() < (kind === "manga" ? 0.55 : 0.25) ? 2 + Math.floor(rand() * 18) : 1;
  const restrictRoll = rand();
  const xRestrict = restrictRoll < 0.1 ? 1 : restrictRoll < 0.13 ? 2 : 0;
  const item: BrowseWorkItem = {
    id: 9000000 + n,
    kind,
    title: `作品示例 #${n}`,
    author_id: 100001 + authorIndex,
    author_name: MOCK_AUTHORS[authorIndex],
    cover: mockCover((n * 47 + authorIndex * 83) % 360, coverRatio),
    page_count: pageCount,
    x_restrict: xRestrict,
    tags: pick(rand, MOCK_TAGS, 2 + Math.floor(rand() * 3)),
    create_date: `2026-09-${pad2(1 + Math.floor(rand() * 28))}`,
  };
  if (kind === "novel") item.text_length = 1200 + Math.floor(rand() * 7200);
  if (rand() < 0.3) {
    item.series_id = 700000 + Math.floor(rand() * 40);
    item.series_title = `示例系列 ${item.series_id - 700000}`;
  }
  return item;
}

interface MockItemsOptions {
  kinds: WorkKind[];
  /** 固定条数（缺省 25-40 随机） */
  count?: number;
  countRange?: [number, number];
  /** 条目带 rank（ranking / 频道 ranking 板块），从该值起编号 */
  rankFrom?: number;
  /** id 编号基底（缺省 (page-1)*60；discover 的「换一批」用调用计数偏移避免 id 撞车） */
  idBase?: number;
}

function mockItems(seedKey: string, page: number, opts: MockItemsOptions): BrowseWorkItem[] {
  const rand = mulberry32(seedFrom(`${seedKey}#${page}`));
  const count = opts.count ?? (opts.countRange
    ? opts.countRange[0] + Math.floor(rand() * (opts.countRange[1] - opts.countRange[0] + 1))
    : 25 + Math.floor(rand() * 16));
  const idBase = opts.idBase ?? (page - 1) * 60;
  return Array.from({ length: count }, (_, i) => {
    const item = makeMockItem(rand, idBase + i + 1, opts.kinds[Math.floor(rand() * opts.kinds.length)] ?? "illust");
    if (opts.rankFrom != null) item.rank = opts.rankFrom + i;
    return item;
  });
}

/** 无翻页的一次性列表（home/related 等）：next_page 恒为 null。 */
function mockOneShot(
  seedKey: string,
  kinds: WorkKind[],
  opts?: { count?: number; countRange?: [number, number] }
): BrowseList {
  const items = mockItems(seedKey, 1, { kinds, count: opts?.count, countRange: opts?.countRange });
  return { items, total: null, next_page: null };
}

/** 有翻页的列表（follow_latest/search/user_works）：next_page 链 + is_last_page。 */
function mockPaginated(
  seedKey: string,
  page: number,
  kinds: WorkKind[],
  opts?: { withTotal?: boolean }
): BrowseList {
  const items = mockItems(seedKey, page, { kinds });
  const next = page < MOCK_MAX_PAGES ? page + 1 : null;
  return {
    items,
    total: opts?.withTotal ? MOCK_MAX_PAGES * items.length : null,
    next_page: next,
    is_last_page: next === null,
  };
}

async function mockChannel(kind: ChannelKind): Promise<BrowseChannel> {
  await mockDelay();
  const workKind = channelKindToWork(kind);
  const rand = mulberry32(seedFrom(`channel:${kind}`));
  const section = (name: string, withRank = false): BrowseList => ({
    items: mockItems(`channel:${kind}:${name}`, 1, {
      kinds: [workKind],
      count: 10 + Math.floor(rand() * 9), // 10-18 条
      rankFrom: withRank ? 1 : undefined,
    }),
    total: null,
    next_page: null,
  });
  const trendingTags = pick(rand, MOCK_TRENDING_TAGS, 8).map((tag) => ({
    name: tag.name,
    translated_name: tag.translated_name,
    count: 100 + Math.floor(rand() * 9900),
  }));
  return {
    follow: section("follow"),
    recommend: section("recommend"),
    ranking: section("ranking", true),
    new_post: section("new"),
    trending_tags: trendingTags,
    ranking_date: todayYmd(),
  };
}

/** discover：每次调用换种子（模块内递增计数），60 条、无翻页；前端重复调用按 id 去重。 */
async function mockDiscover(): Promise<BrowseList> {
  await mockDelay();
  mockDiscoverCallCount += 1;
  return {
    items: mockItems(`discover:${mockDiscoverCallCount}`, 1, {
      kinds: ["illust", "manga", "ugoira"],
      count: 60,
      idBase: mockDiscoverCallCount * 100,
    }),
    total: null,
    next_page: null,
  };
}

async function mockRanking(
  kind: RankingKind,
  mode: RankingMode,
  page = 1,
  date?: string
): Promise<BrowseRanking> {
  await mockDelay();
  const ymd = date || todayYmd();
  const items = mockItems(`ranking:${kind}:${mode}:${ymd}`, page, {
    kinds: [kind],
    count: 50, // v2：p 每页 50
    rankFrom: (page - 1) * 50 + 1,
    idBase: (page - 1) * 50,
  });
  const next = page < 3 ? page + 1 : null;
  return { items, date: ymd, prev_date: ymdShift(ymd, -1), next_date: ymdShift(ymd, 1), next_page: next };
}

/** 作者作品：60/批，3 批后到底（模拟「id 全集降序切片」语义）。 */
async function mockUserWorks(id: number, kind: ListWorkKind, page = 1): Promise<BrowseList> {
  await mockDelay();
  const items = mockItems(`user:${id}:${kind}`, page, { kinds: [kind], count: 60 });
  const next = page < 3 ? page + 1 : null;
  return { items, total: null, next_page: next, is_last_page: next === null };
}

async function mockIllustDetail(kind: "illust" | "manga", id: number): Promise<BrowseIllustDetail> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`detail:${kind}#${id}`));
  const n = id % 1000;
  const item = makeMockItem(rand, n, kind);
  item.id = id;
  const pages = Array.from({ length: Math.max(1, item.page_count) }, (_, i) => ({
    medium: mockCover((n * 47 + i * 29) % 360, "landscape"),
    original: mockCover((n * 47 + i * 29) % 360, "landscape"),
    width: 1200,
    height: 900,
  }));
  return {
    detail_kind: "illust",
    item: {
      ...item,
      description: "这是 mock 详情描述，用于视觉验收。",
      width: 1200,
      height: 900,
      view_count: 1000 + Math.floor(rand() * 90000),
      like_count: 50 + Math.floor(rand() * 4000),
      bookmark_count: 30 + Math.floor(rand() * 3000),
      bookmarked: false,
    },
    pages,
    ugoira: null,
    series:
      item.series_id != null && item.series_title
        ? { id: item.series_id, title: item.series_title, order: 1 + (id % 5) }
        : null,
  };
}

async function mockNovelDetail(id: number): Promise<BrowseNovelDetail> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`detail:novel#${id}`));
  const item = makeMockItem(rand, id % 1000, "novel");
  item.id = id;
  return {
    detail_kind: "novel",
    item: {
      ...item,
      description: "这是 mock 详情描述，用于视觉验收。",
      bookmark_count: 20 + Math.floor(rand() * 2000),
      reading_time: 3 + Math.floor(rand() * 25),
    },
    content:
      "[chapter:序]\n这是 mock 正文第一段，用于小说阅读器的视觉验收。正文正文正文正文正文正文正文正文正文正文。\n\n" +
      "第二段：[rb:注音/よみ] 与 [pixivimage:9000010] 内嵌图占位标记保留原样。\n" +
      "[newpage]\n[chapter:第一章]\n分页后的第一章正文。正文正文正文正文正文正文正文正文正文正文正文正文正文正文。\n\n" +
      "结尾段落。系列导航见 series。",
    series:
      item.series_id != null && item.series_title
        ? { id: item.series_id, title: item.series_title, order: 1 + (id % 8), next_id: id + 1 }
        : null,
  };
}

async function mockUserProfile(id: number): Promise<BrowseUserProfile> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`profile#${id}`));
  const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
  return {
    id,
    name: MOCK_AUTHORS[authorIndex],
    pixiv_id: `sample_user_${authorIndex + 1}`,
    profile_img: mockCover((id * 31) % 360, "square"),
    comment_html: "<p>这是 mock 作者自我介绍。</p>",
    background: null,
    following_count: 40 + Math.floor(rand() * 800),
    mypixiv_count: 5 + Math.floor(rand() * 60),
  };
}

/** 系列目录：游标 last_order 分页，每批 12 条，第 3 批后 next_last_order=null。 */
async function mockSeriesDetail(id: number, lastOrder = 0): Promise<BrowseSeriesDetail> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`series#${id}`));
  const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
  const total = 36; // 3 批 × 12
  const start = Math.floor(lastOrder / 12) * 12;
  const count = Math.max(0, Math.min(12, total - start));
  const next = start + 12 < total ? start + 12 : null;
  return {
    id,
    title: `示例小说系列 #${id}`,
    user_id: 100001 + authorIndex,
    user_name: MOCK_AUTHORS[authorIndex],
    caption: "这是 mock 系列简介，用于视觉验收。",
    cover: mockCover((id * 31) % 360, "portrait"),
    total,
    is_concluded: next === null,
    contents: Array.from({ length: count }, (_, i) => {
      const order = start + i + 1;
      return {
        id: 9500000 + order,
        title: `第 ${order} 话 示例章节`,
        series_order: order,
        text_length: 1500 + Math.floor(rand() * 6000),
        update_date: `2026-09-${pad2(1 + (order % 28))}`,
        x_restrict: rand() < 0.15 ? 1 : undefined,
      };
    }),
    next_last_order: next,
  };
}
