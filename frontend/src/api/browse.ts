/**
 * 浏览模式 IPC 层 —— IPC 契约见 `.subdriver/browse-ui-v1.plan.md`（唯一权威）。
 *
 * 组成：
 * - 契约类型（snake_case，与 Rust browse 命令返回体一致）
 * - 11 个命令的 invoke 封装（未登录错误 → 派发 `pixiv-tool:open-login` 事件，App.vue 负责弹登录窗）
 * - pxSrc()：pximg 封面 URL → `pixiv-img://` 代理协议（Tauri 环境）
 * - mock 层：`!isTauri()`（普通浏览器直接打开 dev 页）时返回**确定性**样例数据，
 *   仅供浏览器内视觉验收使用；Tauri 生产环境完全不走 mock。
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { errorMessage, invoke, isTauri } from "./tauri";

// 复用 tauri.ts 的基础封装，下游浏览相关组件可只 import 本文件。
export { errorMessage, invoke, isTauri };

// ===== 契约类型（snake_case）=====

export type WorkKind = "illust" | "manga" | "ugoira" | "novel";

/** browse_channel 的 kind 取值（illustration 是 pixiv 端命名，区别于 WorkKind.illust）。 */
export type ChannelKind = "illustration" | "manga" | "novel";
/** browse_discover 的 kind 取值。 */
export type DiscoverKind = "all" | "illust" | "manga" | "novel";
/** browse_follow_latest 的 kind / mode 取值。 */
export type FeedKind = "illust" | "novel";
export type FeedMode = "all" | "r18";
/** browse_search / browse_ranking / browse_user_works / browse_related 的 kind 取值。 */
export type ListWorkKind = "illust" | "manga" | "novel";
/** browse_ranking 的 mode 取值（日/周/月榜在不同 kind 下命名不同）。 */
export type RankingMode =
  | "daily"
  | "weekly"
  | "monthly"
  | "day"
  | "week"
  | "month"
  | "rookie"
  | "original"
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
  /** pximg 缩略图 URL（前端经 pxSrc() → pixiv-img 代理显示） */
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

/** 浏览列表通用返回体。next_page 为 null/缺省 表示没有更多。 */
export interface BrowseList {
  items: BrowseWorkItem[];
  total?: number | null;
  next_page?: number | null;
}

/** browse_ranking 返回体（无分页，date 为榜单日期）。 */
export interface BrowseRanking {
  items: BrowseWorkItem[];
  date: string;
}

/** browse_user_profile 返回体。 */
export interface BrowseUserProfile {
  id: number;
  name: string;
  pixiv_id: string;
  profile_img: string;
  comment_html?: string;
  total_follow_users?: number;
  total_mypixiv_users?: number;
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
    bookmarked?: boolean;
  };
  /** 单页作品也返回 1 项 */
  pages: { url_small?: string; url_medium?: string; url_original: string }[];
  /** ugoira 动图帧信息；V1 只显示封面帧 */
  ugoira?: { src: string; frames: { file: string; delay: number }[] } | null;
}

/** browse_work_detail（novel）返回体；content 保留 [newpage]/[chapter:]/[rb:]/[pixivimage:] 原始标记。 */
export interface BrowseNovelDetail {
  detail_kind: "novel";
  item: BrowseWorkItem;
  content: string;
  series_nav?: { prev_id?: number | null; next_id?: number | null };
}

export type BrowseWorkDetail = BrowseIllustDetail | BrowseNovelDetail;

/** browse_novel_series 返回体。 */
export interface BrowseSeries {
  id: number;
  title: string;
  user_id: number;
  user_name: string;
  total: number;
  contents: {
    id: number;
    title: string;
    series_order: number;
    text_length?: number;
    update_date?: string;
  }[];
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
  return convertFileSrc(encodeURIComponent(url), "pixiv-img");
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
  const text = errorMessage(err);
  return LOGIN_ERROR_RE.test(text);
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

/** browse_home_feed：首页推荐。 */
export async function browseHomeFeed(): Promise<BrowseList> {
  if (!isTauri()) return mockList("home", 1, ["illust", "manga", "novel", "ugoira"]);
  return invokeBrowse<BrowseList>("browse_home_feed");
}

/** browse_channel：频道页（插画 / 漫画 / 小说）。 */
export async function browseChannel(kind: ChannelKind, page = 1): Promise<BrowseList> {
  if (!isTauri()) return mockList(`channel:${kind}`, page, [channelKindToWork(kind)]);
  return invokeBrowse<BrowseList>("browse_channel", { kind, page });
}

/** browse_discover：发现页。 */
export async function browseDiscover(kind: DiscoverKind, page = 1): Promise<BrowseList> {
  if (!isTauri()) {
    return mockList(
      `discover:${kind}`,
      page,
      kind === "all" ? ["illust", "manga", "novel"] : [kind]
    );
  }
  return invokeBrowse<BrowseList>("browse_discover", { kind, page });
}

/** browse_follow_latest：关注动态。 */
export async function browseFollowLatest(
  kind: FeedKind,
  mode: FeedMode,
  page: number
): Promise<BrowseList> {
  if (!isTauri()) return mockList(`feed:${kind}:${mode}`, page, [kind]);
  return invokeBrowse<BrowseList>("browse_follow_latest", { kind, mode, page });
}

/** browse_search：搜索。 */
export async function browseSearch(
  kind: ListWorkKind,
  word: string,
  options?: { order?: string; mode?: string; s_mode?: string; page?: number }
): Promise<BrowseList> {
  const page = options?.page ?? 1;
  if (!isTauri()) return mockList(`search:${kind}:${word}`, page, [kind]);
  return invokeBrowse<BrowseList>("browse_search", {
    kind,
    word,
    order: options?.order,
    mode: options?.mode,
    sMode: options?.s_mode,
    page,
  });
}

/** browse_ranking：排行榜（返回体带榜单日期，无 next_page）。 */
export async function browseRanking(
  kind: ListWorkKind,
  mode: RankingMode,
  page: number
): Promise<BrowseRanking> {
  if (!isTauri()) return mockRanking(kind, mode, page);
  return invokeBrowse<BrowseRanking>("browse_ranking", { kind, mode, page });
}

/** browse_work_detail：作品详情（illust/manga → BrowseIllustDetail，novel → BrowseNovelDetail）。 */
export async function browseWorkDetail(
  kind: ListWorkKind,
  id: number
): Promise<BrowseWorkDetail> {
  if (!isTauri()) return kind === "novel" ? mockNovelDetail(id) : mockIllustDetail(kind, id);
  return invokeBrowse<BrowseWorkDetail>("browse_work_detail", { kind, id });
}

/** browse_related：相关推荐。 */
export async function browseRelated(
  kind: ListWorkKind,
  id: number,
  page: number
): Promise<BrowseList> {
  if (!isTauri()) return mockList(`related:${kind}:${id}`, page, [kind === "novel" ? "novel" : "illust"]);
  return invokeBrowse<BrowseList>("browse_related", { kind, id, page });
}

/** browse_user_profile：作者信息。 */
export async function browseUserProfile(id: number): Promise<BrowseUserProfile> {
  if (!isTauri()) return mockUserProfile(id);
  return invokeBrowse<BrowseUserProfile>("browse_user_profile", { id });
}

/** browse_user_works：作者作品列表。 */
export async function browseUserWorks(
  id: number,
  kind: ListWorkKind,
  page = 1
): Promise<BrowseList> {
  if (!isTauri()) return mockList(`user:${id}:${kind}`, page, [kind]);
  return invokeBrowse<BrowseList>("browse_user_works", { id, kind, page });
}

/** browse_novel_series：小说系列目录。 */
export async function browseNovelSeries(id: number): Promise<BrowseSeries> {
  if (!isTauri()) return mockSeries(id);
  return invokeBrowse<BrowseSeries>("browse_novel_series", { id });
}

function channelKindToWork(kind: ChannelKind): WorkKind {
  return kind === "illustration" ? "illust" : kind;
}

// ===== mock 层（仅非 Tauri 环境生效）=====
//
// 用途：普通浏览器直接打开 dev 页（pnpm dev 后 localhost:9961）时做视觉验收，
// 不依赖 Rust 后端。数据由种子伪随机生成，同一命令 + 页码永远返回相同内容；
// 封面为内联 SVG data URI。生产 Tauri 环境不会进入任何 mock 分支。

const MOCK_DELAY_MS = 300;
/** 第 MOCK_MAX_PAGES 页后 next_page = null，用于验收无限滚动收尾。 */
const MOCK_MAX_PAGES = 4;

const MOCK_AUTHORS = ["星空ゆめ", "青野原千鶴", "雾岛静", "白露川音", "toshi", "篠原こま", "南云时雨", "栗山もか"];
const MOCK_TAGS = ["オリジナル", "風景", "女の子", "漫画", "水彩", "日常", "幻想", "創作", "背景", "眼鏡"];

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

/** FNV-1a：命令名 + 页码 → 稳定种子。 */
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
    create_date: `2026-09-${String(1 + Math.floor(rand() * 28)).padStart(2, "0")}`,
  };
  if (kind === "novel") item.text_length = 1200 + Math.floor(rand() * 7200);
  if (rand() < 0.3) {
    item.series_id = 700000 + Math.floor(rand() * 40);
    item.series_title = `示例系列 ${item.series_id - 700000}`;
  }
  return item;
}

async function mockList(seedKey: string, page: number, kinds: WorkKind[]): Promise<BrowseList> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`${seedKey}#${page}`));
  const count = 25 + Math.floor(rand() * 16); // 25-40 条/页
  const base = (page - 1) * 32;
  const items = Array.from({ length: count }, (_, i) =>
    makeMockItem(rand, base + i + 1, kinds[Math.floor(rand() * kinds.length)] ?? "illust")
  );
  return { items, total: null, next_page: page < MOCK_MAX_PAGES ? page + 1 : null };
}

async function mockRanking(kind: ListWorkKind, mode: RankingMode, page: number): Promise<BrowseRanking> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`ranking:${kind}:${mode}#${page}`));
  const count = 30;
  const base = (page - 1) * 30;
  const items = Array.from({ length: count }, (_, i) => {
    const item = makeMockItem(rand, base + i + 1, kind === "novel" ? "novel" : kind);
    item.rank = base + i + 1;
    return item;
  });
  const today = new Date();
  const pad = (v: number) => String(v).padStart(2, "0");
  return { items, date: `${today.getFullYear()}-${pad(today.getMonth() + 1)}-${pad(today.getDate())}` };
}

async function mockIllustDetail(kind: "illust" | "manga", id: number): Promise<BrowseIllustDetail> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`detail:${kind}#${id}`));
  const n = id % 1000;
  const item = makeMockItem(rand, n, kind);
  item.id = id;
  const pages = Array.from({ length: Math.max(1, item.page_count) }, (_, i) => ({
    url_original: mockCover((n * 47 + i * 29) % 360, "landscape"),
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
      bookmarked: false,
    },
    pages,
    ugoira: null,
  };
}

async function mockNovelDetail(id: number): Promise<BrowseNovelDetail> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`detail:novel#${id}`));
  const item = makeMockItem(rand, id % 1000, "novel");
  item.id = id;
  return {
    detail_kind: "novel",
    item,
    content:
      "[chapter:序]\n这是 mock 正文第一段，用于小说阅读器的视觉验收。正文正文正文正文正文正文正文正文正文正文。\n\n" +
      "第二段：[rb:注音/よみ] 与 [pixivimage:9000010] 内嵌图占位标记保留原样。\n" +
      "[newpage]\n[chapter:第一章]\n分页后的第一章正文。正文正文正文正文正文正文正文正文正文正文正文正文正文正文。\n\n" +
      "结尾段落。系列导航见 series_nav。",
    series_nav: { prev_id: rand() < 0.5 ? id - 1 : null, next_id: id + 1 },
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
    total_follow_users: 40 + Math.floor(rand() * 800),
    total_mypixiv_users: 5 + Math.floor(rand() * 60),
  };
}

async function mockSeries(id: number): Promise<BrowseSeries> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`series#${id}`));
  const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
  const total = 6 + Math.floor(rand() * 5);
  return {
    id,
    title: `示例小说系列 #${id}`,
    user_id: 100001 + authorIndex,
    user_name: MOCK_AUTHORS[authorIndex],
    total,
    contents: Array.from({ length: total }, (_, i) => ({
      id: 9500000 + i + 1,
      title: `第 ${i + 1} 话 示例章节`,
      series_order: i + 1,
      text_length: 1500 + Math.floor(rand() * 6000),
      update_date: `2026-09-${String(1 + (i % 28)).padStart(2, "0")}`,
    })),
  };
}
