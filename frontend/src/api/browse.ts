/**
 * 浏览模式 IPC 层 —— IPC 契约 v2 见 `.subdriver/browse-ui-v1.plan.md`（唯一权威，
 * 2026-10-01 依 docs/research/pixiv-browse-api.md 实测修订）。
 *
 * 组成：
 * - 契约类型（snake_case，与 Rust browse 命令返回体一致）
 * - 22 个命令的 invoke 封装（v2 的 12 个 + v2.1 评论补充的 2 个 + v3.1 收藏 4 个
 *   + 追更列表 1 个 + 浏览访问历史 3 个（browse-history-ui-v1）；
 *   未登录错误 → 派发 `pixiv-tool:open-login` 事件，App.vue 负责弹登录窗）
 * - pxSrc()：pximg 封面 URL → `pixiv-img://` 代理协议（Tauri 环境）
 * - thumbSrc()：按档位改写尺寸段后再走 pxSrc（组件层唯一的缩略图出口）
 * - mock 层：`!isTauri()`（普通浏览器直接打开 dev 页）时返回样例数据，
 *   仅供浏览器内视觉验收使用；Tauri 生产环境完全不走 mock。
 *   除 browse_discover（每次调用换种子，配合「重复调用 + 按 id 去重」）外均为确定性数据。
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { errorMessage, invoke, isTauri } from "./tauri";
import { thumbUrl, type ThumbTier } from "../utils/thumb";

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
/**
 * 收藏命令（browse_bookmark_*）的 kind：pixiv 端点只有 illusts / novels 两族，
 * 插画、漫画、动图共用 illust 族。
 */
export type BookmarkKind = "illust" | "novel";
/** browse_bookmark_list 的 rest：show = 公开收藏、hide = 私密收藏（自己；他人只能 show）。 */
export type BookmarkRest = "show" | "hide";
/** browse_watchlist 的 kind（官方追更列表只有漫画 / 小说两个子 tab）。 */
export type WatchKind = "manga" | "novel";
/** 收藏可见范围（pixiv restrict 语义）：0 = 公开、1 = 非公开。 */
export type BookmarkRestrict = 0 | 1;
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
export interface BrowseWorkItem extends WorkCounts {
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
  /**
   * 收藏列表专用：当前查看者对该作品的收藏记录 id（取消收藏直接用它，无需再查详情）。
   * 他人公开收藏列表中该字段是「查看者本人」的收藏态，UI 不使用。
   */
  bookmarkId?: string;
  /** 收藏列表专用：0 = 公开收藏、1 = 私密收藏 */
  bookmarkRestrict?: BookmarkRestrict;
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
  /** search 专用：最后一页页码（接口每页 60，上限 1000）；页码分页换算显示页数用 */
  last_page?: number | null;
}

/** 列表已有计数或 browse_work_counts 的 counts 值；缺失字段需按排序维度补取。 */
export interface WorkCounts {
  like_count?: number;
  bookmark_count?: number;
  view_count?: number;
}

/** browse_work_counts 返回体：id（字符串化）→ 三项计数；缺失 = 该次请求失败。 */
export interface WorkCountsMap {
  counts: Record<string, WorkCounts>;
}

/** 频道页「按标签推荐」板块（#tag 的推荐作品；实测仅插画频道返回，其余为空数组）。 */
export interface BrowseChannelSection {
  tag: string;
  items: BrowseWorkItem[];
}

/** browse_channel 返回体：频道页一次性快照（四个板块 + 按标签推荐 + 热门标签）。 */
export interface BrowseChannel {
  follow: BrowseList;
  recommend: BrowseList;
  ranking: BrowseList;
  new_post: BrowseList;
  /** 插画频道特有：官方每板块给 24 个 id、页面渲染 12 条 */
  tag_sections: BrowseChannelSection[];
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
  is_followed?: boolean;
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
  /** 查看者的收藏态（未收藏 null；B1 契约扩展字段，后端未上线时缺省 = 未知 → 视为未收藏） */
  bookmarkState?: WorkBookmarkState | null;
}

/** browse_work_detail（novel）返回体；content 为全文（保留 [newpage]/[chapter:]/[rb:]/[uploadedimage:] 原始标记，前端切分）。 */
export interface BrowseNovelDetail {
  detail_kind: "novel";
  item: BrowseWorkItem & {
    description?: string;
    like_count?: number;
    view_count?: number;
    bookmark_count?: number;
    reading_time?: number;
  };
  content: string;
  /**
   * 正文内嵌插画：`[uploadedimage:id]` 的 id → pximg URL（显示经 pxSrc）。
   * 后端取自同一详情响应的 textEmbeddedImages，无内嵌图时为空对象。
   */
  embedded_images?: Record<string, string>;
  /** seriesNavData：系列导航（order 为本书在系列中的序号） */
  series?: { id: number; title: string; order: number; next_id?: number | null } | null;
  /** 查看者的收藏态（未收藏 null；B1 契约扩展字段，后端未上线时缺省 = 未知 → 视为未收藏） */
  bookmarkState?: WorkBookmarkState | null;
}

export type BrowseWorkDetail = BrowseIllustDetail | BrowseNovelDetail;

/**
 * 详情响应中的收藏态（pixiv 详情体 bookmarkData 三态归一；id 统一 String 化）：
 * 未收藏为 null；匿名访问恒为 null（个人态字段，登录后才反映真实收藏态）。
 */
export interface WorkBookmarkState {
  bookmarkId: string;
  restrict: BookmarkRestrict;
}

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

/**
 * browse_illust_series 返回体：插画/漫画系列目录。
 * pixiv 端点 GET /ajax/series/{id}?p={page}&lang=zh —— 页码制：每页恒 12 条、
 * 恒按 series_order 降序；无游标/排序参数（limit、last_order、order* 均被服务端忽略）。
 * 证据：docs/research/pixiv-browse-api.md §13。
 */
export interface BrowseIllustSeriesDetail {
  id: number;
  title: string;
  user_id: number;
  user_name: string;
  /** users[] 按 userId 映射，imageBig 优先回退 image */
  user_avatar?: string;
  caption?: string;
  /** illustSeries[0].url；空串 = 未设自定义封面（isSetCover=false 恒 null），前端回退 contents[0].cover */
  cover?: string;
  /** page.total（系列总话数） */
  total: number;
  /** pixiv 无漫画/插画系列完结标记（illustSeries[0] 无 isConcluded 字段），恒 false */
  is_concluded: false;
  is_watched?: boolean;
  /** illustSeries[0].updateDate（ISO 含时区） */
  update_date?: string;
  /** 本页分集，series_order 降序（pixiv page.series[] 与 thumbnails.illust 同序按位映射） */
  contents: {
    id: number;
    /** 恒 "illust"：illustType 0/1/2 均入此 kind，进详情页后再分 */
    kind: "illust";
    title: string;
    /** pixiv urls.360x360 优先，回退顶层 url（官方卡片档） */
    cover: string;
    page_count?: number;
    /** 1=R-18；全局过滤 filterByR18 用条目级口径（系列头无 xRestrict） */
    x_restrict?: number;
    update_date?: string;
    /** 话数 1..total（pixiv page.series[].order；UI #N 徽标同源） */
    series_order: number;
    ai_type?: number;
  }[];
  /** 当前页码回显（从 1 起） */
  page: number;
  /** ceil(total/12) */
  total_pages: number;
  /** page < total_pages ? page+1 : null（pixiv 超页返回空数组不报错，等价到底） */
  next_page: number | null;
}

// ===== 评论契约（v2.1，2026-10-01 实机实测）=====

/** 浏览评论条目（roots 与 replies 同构）。 */
export interface BrowseComment {
  id: string;
  user_id: number;
  user_name: string;
  /** 后端补全 https:// 前缀（pixiv 返回无协议）；前端经 pxSrc() 代理显示 */
  profile_img?: string;
  /** 纯文本原样；表情（stamp）评论为空 */
  content?: string;
  /** stampId → 生成图 URL；有值时正文以表情图渲染（content 为空） */
  stamp_url?: string;
  /** commentDate 原样（"2026-10-01 08:15"） */
  date?: string;
  has_replies?: boolean;
  /** 仅回复列表（replyToUserName）：被回复者昵称 */
  reply_to_user_name?: string;
}

/**
 * browse_work_comments / browse_comment_replies 返回体。
 * 接口无 total；next 为续拉游标（roots：下一批 offset；replies：下一页 page），null = 到底。
 * disabled=true = 评论区被作者关闭（roots 端点恒 400 的后端映射，仅 roots 出现），
 * 此时 comments 恒为空数组；省略 = 正常评论区。
 */
export interface BrowseComments {
  comments: BrowseComment[];
  next?: number | null;
  disabled?: boolean;
}

// ===== 收藏契约（bookmark-ui-v1 v3.1）=====

/** browse_bookmark_list 返回体（offset 游标分页；next = null 到底）。 */
export interface BrowseBookmarkList {
  /** 插画/漫画/动图混排或小说；条目带 bookmarkId / bookmarkRestrict */
  items: BrowseWorkItem[];
  total: number | null;
  /** 下一批 offset；null = 到底 */
  next: number | null;
}

/** 收藏标签条目（name 为空串 = 未分类，前端 i18n 显示）。 */
export interface BrowseBookmarkTag {
  name: string;
  count: number;
}

/** browse_bookmark_tags 返回体（端点无 rest 参数，一次返回公开/私密两组）。 */
export interface BrowseBookmarkTags {
  public: BrowseBookmarkTag[];
  private: BrowseBookmarkTag[];
}

// ===== 追更列表契约（watchlist-ui-v1）=====

/** browse_watchlist 条目：用户订阅的系列（追更）。 */
export interface BrowseWatchlistItem {
  /** 系列 id（novel 可直达 /browse/series/:id） */
  id: number;
  kind: WatchKind;
  title: string;
  user_id: number;
  user_name: string;
  user_avatar?: string;
  /** 竖版封面（240x480 档；漫画为最新话封面，小说为系列封面） */
  cover?: string;
  /** 最新话的 R-18 标记（0 无 | 1 R-18 | 2 R-18G） */
  x_restrict?: number;
  /** 已发布话数 */
  total: number;
  /** ISO 时间戳；前端只展示日期部分 */
  update_date?: string;
  /** 最新话作品 id（「读最新话」直达） */
  latest_work_id?: number;
}

/** browse_watchlist 返回体：后端已按 max_page 聚合（≤20 页），前端无需翻页。 */
export interface BrowseWatchlist {
  kind: WatchKind;
  total: number;
  max_page: number;
  items: BrowseWatchlistItem[];
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

/**
 * 按档位改写尺寸段后的图片地址（组件层唯一出口）。
 * 档位一律经 composables/useThumbTier 读取；thumbUrl 不直接对组件开放，
 * 以免组件自行拼尺寸段。mock 的 data URI 与头像等非 pximg URL 由 thumbUrl 原样透传。
 */
export function thumbSrc(url: string | undefined | null, tier: ThumbTier): string {
  return pxSrc(thumbUrl(url, tier));
}

export type { ThumbTier };

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
export async function browseChannel(kind: ChannelKind, mode: "all" | "r18" = "all"): Promise<BrowseChannel> {
  if (!isTauri()) return mockChannel(kind, mode);
  return invokeBrowse<BrowseChannel>("browse_channel", { kind, mode });
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

/** browse_work_counts：作品三项计数批量（搜索页本页排序用；ids 上限 60）。 */
export async function browseWorkCounts(
  kind: ListWorkKind,
  ids: number[]
): Promise<WorkCountsMap> {
  if (!isTauri()) return mockWorkCounts(kind, ids);
  return invokeBrowse<WorkCountsMap>("browse_work_counts", { kind, ids });
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

const mockUserFollows = new Map<number, boolean>();

/** 公开关注或取消关注；生产环境由后端验证 Pixiv 成功响应。 */
export async function browseUserFollow(id: number, followed: boolean): Promise<{ is_followed: boolean }> {
  if (!isTauri()) {
    await mockDelay();
    mockUserFollows.set(id, followed);
    return { is_followed: followed };
  }
  return invokeBrowse("browse_user_follow", { id, followed });
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

/**
 * browse_novel_series：小说系列目录（游标 last_order 分页，每批 30；mock 同参）。
 * 页码分集页的映射：last_order = (page-1)*30（contentOrder 从 1 起的排他下界），
 * total_pages = ceil(total/30)。证据：docs/research/pixiv-browse-api.md §13.4。
 */
export async function browseNovelSeries(id: number, lastOrder = 0): Promise<BrowseSeriesDetail> {
  if (!isTauri()) return mockSeriesDetail(id, lastOrder);
  return invokeBrowse<BrowseSeriesDetail>("browse_novel_series", { id, lastOrder });
}

/** browse_illust_series：插画/漫画系列目录（官方页码制，每页 12 条、话数降序；mock 同参）。 */
export async function browseIllustSeries(id: number, page = 1): Promise<BrowseIllustSeriesDetail> {
  if (!isTauri()) return mockIllustSeries(id, page);
  return invokeBrowse<BrowseIllustSeriesDetail>("browse_illust_series", { id, page });
}

/** browse_work_comments：评论 roots（offset 游标，limit=10；接口无 total，next=null 到底）。 */
export async function browseWorkComments(params: {
  kind: ListWorkKind;
  id: number;
  offset?: number;
}): Promise<BrowseComments> {
  const { kind, id, offset = 0 } = params;
  if (!isTauri()) return mockComments(kind, id, offset);
  return invokeBrowse<BrowseComments>("browse_work_comments", { kind, id, offset });
}

/** browse_comment_replies：评论回复（page 从 1 起；next=null 到底）。 */
export async function browseCommentReplies(params: {
  kind: ListWorkKind;
  commentId: string;
  page?: number;
}): Promise<BrowseComments> {
  const { kind, commentId, page = 1 } = params;
  if (!isTauri()) return mockReplies(kind, commentId, page);
  return invokeBrowse<BrowseComments>("browse_comment_replies", { kind, commentId, page });
}

// ===== 收藏命令封装（bookmark-ui-v1；!isTauri() → mock）=====

/** 每页条数与官方一致：插画·漫画 48 / 页、小说 30 / 页。 */
export const BOOKMARK_PAGE_SIZE: Record<BookmarkKind, number> = { illust: 48, novel: 30 };

/**
 * browse_bookmark_list：收藏列表（自己）。tag 语义：null = 全部、"" = 未分类、其余为标签名。
 * 他人公开收藏（作者页收藏 tab）必须显式 rest="show"。
 */
export async function browseBookmarkList(
  kind: BookmarkKind,
  rest: BookmarkRest,
  tag: string | null,
  offset = 0,
  limit = BOOKMARK_PAGE_SIZE[kind]
): Promise<BrowseBookmarkList> {
  if (!isTauri()) return mockBookmarkList(kind, rest, tag, offset, limit);
  return invokeBrowse<BrowseBookmarkList>("browse_bookmark_list", { kind, rest, tag, offset, limit });
}

/** browse_bookmark_tags：收藏标签（一次返回 public/private 两组，供公开/私密筛选分别取组）。 */
export async function browseBookmarkTags(kind: BookmarkKind): Promise<BrowseBookmarkTags> {
  if (!isTauri()) return mockBookmarkTags(kind);
  return invokeBrowse<BrowseBookmarkTags>("browse_bookmark_tags", { kind });
}

/** browse_bookmark_add：添加收藏（restrict 0=公开 1=私密；tags 缺省为空 = 未分类）。 */
export async function browseBookmarkAdd(
  kind: BookmarkKind,
  id: number,
  restrict: BookmarkRestrict,
  tags?: string[]
): Promise<{ bookmarkId: string }> {
  if (!isTauri()) return mockBookmarkAdd(kind, id, restrict);
  return invokeBrowse<{ bookmarkId: string }>("browse_bookmark_add", { kind, id, restrict, tags });
}

/** browse_bookmark_remove：取消收藏（bookmarkId 来自列表项 bookmarkId 或详情 bookmarkState）。 */
export async function browseBookmarkRemove(
  kind: BookmarkKind,
  id: number,
  bookmarkId: string
): Promise<void> {
  if (!isTauri()) return mockBookmarkRemove(kind, id, bookmarkId);
  await invokeBrowse<void>("browse_bookmark_remove", { kind, id, bookmarkId });
}

// ===== 追更列表封装（watchlist-ui-v1；!isTauri() → mock）=====

/** browse_watchlist：追更列表（订阅的漫画/小说系列，一次全部返回）。 */
export async function browseWatchlist(kind: WatchKind): Promise<BrowseWatchlist> {
  if (!isTauri()) return mockWatchlist(kind);
  return invokeBrowse<BrowseWatchlist>("browse_watchlist", { kind });
}

// ===== 浏览访问历史契约与封装（browse-history-ui-v1；!isTauri() → mock）=====

/** 浏览历史条目：来自历史行（visited_at 倒序由后端保证，前端不再排序）。 */
export interface BrowseHistoryItem {
  work_id: number;
  kind: WorkKind;
  title: string;
  author_id: number;
  author_name: string;
  cover: string;
  page_count: number;
  x_restrict: number;
  visited_at: string;
}

/** browse_history_list 返回体（页码制分页）。 */
export interface BrowseHistoryList {
  items: BrowseHistoryItem[];
  total: number;
  page: number;
  page_size: number;
}

/** browse_history_record 入参：埋点上报的一次访问（同作品去重置顶由后端处理）。 */
export interface BrowseHistoryRecordInput {
  workId: number;
  kind: WorkKind;
  title: string;
  authorId: number;
  authorName: string;
  cover: string;
  pageCount: number;
  xRestrict: number;
}

/** browse_history_record：上报一次作品访问（去重置顶）。 */
export async function browseHistoryRecord(input: BrowseHistoryRecordInput): Promise<void> {
  if (!isTauri()) return mockHistoryRecord(input);
  await invokeBrowse<void>("browse_history_record", {
    workId: input.workId,
    kind: input.kind,
    title: input.title,
    authorId: input.authorId,
    authorName: input.authorName,
    cover: input.cover,
    pageCount: input.pageCount,
    xRestrict: input.xRestrict,
  });
}

/** browse_history_list：浏览历史分页列表（page 从 1 起；kind 省略 = 全部）。 */
export async function browseHistoryList(
  page: number,
  pageSize: number,
  kind?: WorkKind
): Promise<BrowseHistoryList> {
  if (!isTauri()) return mockHistoryList(page, pageSize, kind);
  // kind 为 undefined 时 JSON 序列化丢弃该键 → Rust Option::None（不过滤）。
  return invokeBrowse<BrowseHistoryList>("browse_history_list", { page, pageSize, kind });
}

/** browse_history_clear：一键清空浏览历史。 */
export async function browseHistoryClear(): Promise<{ status: string; deleted: number }> {
  if (!isTauri()) return mockHistoryClear();
  return invokeBrowse<{ status: string; deleted: number }>("browse_history_clear");
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
    last_page: opts?.withTotal ? MOCK_MAX_PAGES : undefined,
  };
}

/** 计数批量 mock：按 id 派生确定性三项计数（浏览器视觉验收用）。 */
async function mockWorkCounts(kind: ListWorkKind, ids: number[]): Promise<WorkCountsMap> {
  await mockDelay();
  const counts: WorkCountsMap["counts"] = {};
  for (const id of ids) {
    const rand = mulberry32(seedFrom(`counts:${kind}:${id}`));
    counts[String(id)] = {
      like_count: Math.floor(rand() * 20000),
      bookmark_count: Math.floor(rand() * 30000),
      view_count: Math.floor(rand() * 200000),
    };
  }
  return { counts };
}

async function mockChannel(kind: ChannelKind, mode: "all" | "r18"): Promise<BrowseChannel> {
  await mockDelay();
  const workKind = channelKindToWork(kind);
  const rand = mulberry32(seedFrom(`channel:${kind}:${mode}`));
  const restrictItems = (items: BrowseWorkItem[]) => items.map((item) => ({ ...item, x_restrict: mode === "r18" ? 1 : item.x_restrict }));
  const section = (name: string, withRank = false): BrowseList => ({
    items: restrictItems(mockItems(`channel:${kind}:${mode}:${name}`, 1, {
      kinds: [workKind],
      count: 10 + Math.floor(rand() * 9), // 10-18 条
      rankFrom: withRank ? 1 : undefined,
    })),
    total: null,
    next_page: null,
  });
  const trendingTags = pick(rand, MOCK_TRENDING_TAGS, 8).map((tag) => ({
    name: tag.name,
    translated_name: tag.translated_name,
    count: 100 + Math.floor(rand() * 9900),
  }));
  // #标签推荐板块：实测仅插画频道返回（官方每板块 24 个 id、渲染 12 条；mock 给 14 条供切片验收）
  const tagSections =
    kind === "illustration"
      ? pick(rand, MOCK_TRENDING_TAGS, 3).map((tag, i) => ({
          tag: tag.name,
          items: restrictItems(mockItems(`channel:${kind}:${mode}:tag:${i}`, 1, {
            kinds: [workKind],
            count: 14,
          })),
        }))
      : [];
  return {
    follow: section("follow"),
    recommend: section("recommend"),
    ranking: section("ranking", true),
    new_post: section("new"),
    tag_sections: tagSections,
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
    // mock 收藏态：优先取本会话 add/remove 后的存储，否则按 id 确定性预置（覆盖三态视觉验收）
    bookmarkState: mockBookmarkStore.get(`illust:${id}`) ?? mockDefaultBookmarkState("illust", id),
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
      like_count: 10 + Math.floor(rand() * 1000),
      view_count: 1000 + Math.floor(rand() * 90000),
      reading_time: 3 + Math.floor(rand() * 25),
    },
    content:
      "[chapter:序]\n这是 mock 正文第一段，用于小说阅读器的视觉验收。正文正文正文正文正文正文正文正文正文正文。\n\n" +
      "第二段：[rb:注音/よみ] 与 [uploadedimage:9000010] 内嵌图标记，图片由 embedded_images 解析显示。\n" +
      "[uploadedimage:9000011]\n" +
      "[pixivimage:9000012]\n" +
      "[newpage]\n[chapter:第一章]\n分页后的第一章正文。正文正文正文正文正文正文正文正文正文正文正文正文正文正文。\n\n" +
      "结尾段落。系列导航见 series。",
    // mock 内嵌图：data URI 在浏览器内可直接显示；9000012 故意不给 URL，
    // 用于视觉验收「无 URL 的内嵌图（pixivimage）走占位块」分支。
    embedded_images: {
      "9000010": mockCover((id * 17) % 360, "landscape"),
      "9000011": mockCover((id * 23) % 360, "landscape"),
    },
    series:
      item.series_id != null && item.series_title
        ? { id: item.series_id, title: item.series_title, order: 1 + (id % 8), next_id: id + 1 }
        : null,
    // mock 收藏态：优先取本会话 add/remove 后的存储，否则按 id 确定性预置（覆盖三态视觉验收）
    bookmarkState: mockBookmarkStore.get(`novel:${id}`) ?? mockDefaultBookmarkState("novel", id),
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
    is_followed: mockUserFollows.get(id) ?? false,
  };
}

/**
 * 小说系列目录：每批 30 条（与真实后端一致），页码映射 last_order=(page-1)*30 精确切片；
 * total=91 → 4 页（末页 1 条，覆盖页边界）。is_concluded 按 id 确定性（同系列各页恒定）。
 */
async function mockSeriesDetail(id: number, lastOrder = 0): Promise<BrowseSeriesDetail> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`series#${id}`));
  const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
  const total = 91;
  const start = Math.floor(lastOrder / 30) * 30;
  const count = Math.max(0, Math.min(30, total - start));
  const next = start + 30 < total ? start + 30 : null;
  return {
    id,
    title: `示例小说系列 #${id}`,
    user_id: 100001 + authorIndex,
    user_name: MOCK_AUTHORS[authorIndex],
    caption: "这是 mock 系列简介，用于视觉验收。",
    cover: mockCover((id * 31) % 360, "portrait"),
    total,
    is_concluded: id % 2 === 0,
    contents: Array.from({ length: count }, (_, i) => {
      const order = start + i + 1;
      return {
        id: 9500000 + order,
        title:
          order % 5 === 1
            ? `第 ${order} 话 示例章节标题——刻意拉长到超出两行，用于验收标题截断与省略号表现`
            : `第 ${order} 话 示例章节`,
        series_order: order,
        text_length: 1500 + Math.floor(rand() * 6000),
        update_date: `2026-09-${pad2(1 + (order % 28))}`,
        x_restrict: rand() < 0.15 ? 1 : 0,
      };
    }),
    next_last_order: next,
  };
}

/**
 * 插画/漫画系列目录：官方页码制复刻——每页恒 12 条、话数降序、超页空数组不报错；
 * total=31 → 3 页（p1: 31..20，p2: 19..8，p3: 7..1）。条目含长标题（两行截断）、
 * 部分多页作品与 R-18；封面 portrait/square 混合。系列头 cover 按 id 奇偶给「有值 /
 * 空串」两形态（空串 → 前端回退 contents[0].cover，验回退分支）。
 */
async function mockIllustSeries(id: number, page = 1): Promise<BrowseIllustSeriesDetail> {
  await mockDelay();
  const total = 31;
  const totalPages = Math.ceil(total / 12);
  const headRand = mulberry32(seedFrom(`illust-series#${id}`));
  const authorIndex = Math.floor(headRand() * MOCK_AUTHORS.length);
  const rand = mulberry32(seedFrom(`illust-series#${id}#${page}`));
  const startOrder = total - (page - 1) * 12; // 本页最大话数（降序切片页首）
  const count = Math.max(0, Math.min(12, startOrder)); // 末页剩余条数；超页 → 0（空数组）
  const contents = Array.from({ length: count }, (_, i) => {
    const order = startOrder - i;
    const restrictRoll = rand();
    return {
      id: 9600000 + order,
      kind: "illust" as const,
      title:
        order % 4 === 1
          ? `第 ${order} 话 · 用于验收两行截断的超长示例标题——这一话的标题被刻意拉长到两行以上，检查省略号与布局稳定`
          : `第 ${order} 话 示例分镜`,
      cover: mockCover((order * 53 + id * 17) % 360, order % 3 === 0 ? "portrait" : "square"),
      page_count: rand() < 0.35 ? 2 + Math.floor(rand() * 18) : 1,
      x_restrict: restrictRoll < 0.2 ? 1 : 0,
      update_date: `2026-09-${pad2(1 + (order % 28))}`,
      series_order: order,
      ai_type: 0,
    };
  });
  return {
    id,
    title: `示例插画系列 #${id}`,
    user_id: 100001 + authorIndex,
    user_name: MOCK_AUTHORS[authorIndex],
    user_avatar: mockCover((id * 41 + 7) % 360, "square"),
    caption: "这是 mock 插画/漫画系列简介，用于视觉验收。",
    cover: id % 2 === 0 ? mockCover((id * 31) % 360, "portrait") : "",
    total,
    is_concluded: false,
    is_watched: headRand() < 0.5,
    update_date: `2026-09-${pad2(1 + Math.floor(headRand() * 28))}T12:00:00+09:00`,
    contents,
    page,
    total_pages: totalPages,
    next_page: page < totalPages ? page + 1 : null,
  };
}

// ===== 追更列表 mock =====

/** 追更列表：5-8 个系列条目（竖版封面 + 作者头像），确定性。 */
async function mockWatchlist(kind: WatchKind): Promise<BrowseWatchlist> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`watchlist:${kind}`));
  const count = 5 + Math.floor(rand() * 4);
  const items: BrowseWatchlistItem[] = Array.from({ length: count }, (_, i) => {
    const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
    const restrictRoll = rand();
    return {
      id: (kind === "novel" ? 850000 : 800000) + i,
      kind,
      title: `示例追更系列 ${kind === "novel" ? "· 小说 " : ""}#${i + 1}`,
      user_id: 100001 + authorIndex,
      user_name: MOCK_AUTHORS[authorIndex],
      user_avatar: mockCover((i * 67 + 13) % 360, "square"),
      cover: mockCover((i * 97 + 29) % 360, "portrait"),
      x_restrict: restrictRoll < 0.15 ? 1 : 0,
      total: 6 + Math.floor(rand() * 30),
      update_date: `2026-09-${pad2(1 + Math.floor(rand() * 28))}`,
      latest_work_id: 9200000 + i,
    };
  });
  return { kind, total: items.length, max_page: 1, items };
}

// ===== 评论 mock =====

/** roots 每页条数（契约 limit=10）。 */
const MOCK_COMMENT_PAGE = 10;

const MOCK_COMMENT_TEXTS = [
  "神回！构图和配色都太舒服了",
  "收藏了，期待更多作品～",
  "这光影处理绝了，请问用什么笔刷？",
  "每天来看一眼已经成为习惯了",
  "太强了，膜拜大佬",
  "氛围感拉满，颜色好温柔\n已转发给朋友安利",
  "第一次评论：每次更新都会第一时间点开",
  "这个角度的光很难画吧，控制得真好",
  "角色表情好生动，仿佛能听到声音",
  "水面的反光细节太讲究了",
  "昨晚蹲到更新，果然没让我失望！",
  "构图参考价值很高，学习了",
];

const MOCK_COMMENT_URL_TEXTS = [
  "参考了这张的构图 https://www.pixiv.net/artworks/9000012 受益匪浅",
  "做成手机壁纸了，出处 https://www.pixiv.net/artworks/9000015",
  "系列前三话也超好看：\nhttps://www.pixiv.net/novel/series/700012",
];

/** 表情评论占位 stamp：内联 SVG data-URI（圆形笑脸，色相区分）。 */
function mockStamp(hue: number): string {
  const s = 96;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}" viewBox="0 0 ${s} ${s}">` +
    `<rect width="${s}" height="${s}" rx="20" fill="hsl(${hue} 75% 86%)"/>` +
    `<circle cx="${s * 0.5}" cy="${s * 0.5}" r="${s * 0.3}" fill="hsl(${hue} 78% 60%)"/>` +
    `<circle cx="${s * 0.42}" cy="${s * 0.44}" r="${s * 0.045}" fill="#1c1b22"/>` +
    `<circle cx="${s * 0.6}" cy="${s * 0.44}" r="${s * 0.045}" fill="#1c1b22"/>` +
    `<path d="M${s * 0.38} ${s * 0.58} Q ${s * 0.51} ${s * 0.7} ${s * 0.64} ${s * 0.58}" stroke="#1c1b22" stroke-width="4" fill="none" stroke-linecap="round"/>` +
    `</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** 确定性 roots 全集：10-25 条混合（纯文本 / URL / 表情 / 2-3 条带 has_replies）。 */
function mockCommentRoots(kind: ListWorkKind, id: number): BrowseComment[] {
  const rand = mulberry32(seedFrom(`comments:${kind}:${id}`));
  const total = 10 + Math.floor(rand() * 16); // 10-25
  return Array.from({ length: total }, (_, i) => {
    const authorIndex = Math.floor(rand() * MOCK_AUTHORS.length);
    const isStamp = rand() < 0.15;
    const isUrl = !isStamp && rand() < 0.18;
    const isMultiline = !isStamp && !isUrl && rand() < 0.2;
    const content = isStamp
      ? ""
      : isUrl
        ? MOCK_COMMENT_URL_TEXTS[Math.floor(rand() * MOCK_COMMENT_URL_TEXTS.length)]
        : isMultiline
          ? `${MOCK_COMMENT_TEXTS[Math.floor(rand() * MOCK_COMMENT_TEXTS.length)]}\n（补充：第二行文本用于验收 pre-wrap 换行）`
          : MOCK_COMMENT_TEXTS[Math.floor(rand() * MOCK_COMMENT_TEXTS.length)];
    // has_replies 固定命中 2-3 条（total≥10 保证前两处必中；total>12 追加第三处）
    const hasReplies = i === 1 || i === 4 || (i === 9 && total > 12);
    const comment: BrowseComment = {
      id: String(Number(id) * 100 + i),
      user_id: 100001 + authorIndex,
      user_name: MOCK_AUTHORS[authorIndex],
      profile_img: mockCover((id * 7 + i * 41) % 360, "square"),
      date: `2026-09-${pad2(1 + Math.floor(rand() * 28))} ${pad2(Math.floor(rand() * 24))}:${pad2(Math.floor(rand() * 60))}`,
    };
    if (isStamp) comment.stamp_url = mockStamp(Math.floor(rand() * 360));
    else comment.content = content;
    if (hasReplies) comment.has_replies = true;
    return comment;
  });
}

/** 评论 roots：offset 游标切片，next = hasNext ? offset+len : null。 */
async function mockComments(kind: ListWorkKind, id: number, offset = 0): Promise<BrowseComments> {
  await mockDelay();
  const roots = mockCommentRoots(kind, id);
  const slice = roots.slice(offset, offset + MOCK_COMMENT_PAGE);
  const hasNext = offset + slice.length < roots.length;
  return { comments: slice, next: hasNext ? offset + slice.length : null };
}

/** 评论回复：每组确定性 1-3 条，单页装下（≤10/页），next 恒为 null；第 2 条起带 reply_to。 */
async function mockReplies(kind: ListWorkKind, commentId: string, page = 1): Promise<BrowseComments> {
  await mockDelay();
  const rand = mulberry32(seedFrom(`replies:${kind}:${commentId}`));
  const count = 1 + Math.floor(rand() * 3); // 1-3 条
  const firstIndex = Math.floor(rand() * MOCK_AUTHORS.length);
  const replies: BrowseComment[] = Array.from({ length: count }, (_, i) => {
    const authorIndex = i === 0 ? firstIndex : Math.floor(rand() * MOCK_AUTHORS.length);
    const reply: BrowseComment = {
      id: `${commentId}-r${i + 1}`,
      user_id: 100001 + authorIndex,
      user_name: MOCK_AUTHORS[authorIndex],
      profile_img: mockCover((commentId.length * 13 + i * 57) % 360, "square"),
      date: `2026-09-${pad2(2 + Math.floor(rand() * 27))} ${pad2(Math.floor(rand() * 24))}:${pad2(Math.floor(rand() * 60))}`,
      content: MOCK_COMMENT_TEXTS[Math.floor(rand() * MOCK_COMMENT_TEXTS.length)],
    };
    if (i > 0) reply.reply_to_user_name = MOCK_AUTHORS[firstIndex];
    return reply;
  });
  // page 从 1 起；每组 ≤3 条单页装下，第 1 页即到底
  return { comments: page <= 1 ? replies : [], next: null };
}

// ===== 收藏 mock =====

/** 收藏标签池（mock；空串 = 未分类）。 */
const MOCK_BOOKMARK_TAGS: Record<BookmarkKind, string[]> = {
  illust: ["風景", "女の子", "オリジナル", "漫画", "水彩"],
  novel: ["ファンタジー", "恋愛", "短編", "連載"],
};

/** 各 kind × rest 的收藏池规模（确定性）。 */
const MOCK_BOOKMARK_TOTALS: Record<BookmarkKind, Record<BookmarkRest, number>> = {
  illust: { show: 132, hide: 26 },
  novel: { show: 64, hide: 11 },
};

/** mock 收藏池缓存：同一 kind + rest 恒为同一组条目（列表与标签计数共用，保证自洽）。 */
const bookmarkPoolCache = new Map<string, BrowseWorkItem[]>();

function mockBookmarkPool(kind: BookmarkKind, rest: BookmarkRest): BrowseWorkItem[] {
  const key = `${kind}:${rest}`;
  const cached = bookmarkPoolCache.get(key);
  if (cached) return cached;
  const kinds: WorkKind[] = kind === "novel" ? ["novel"] : ["illust", "manga", "ugoira"];
  const items = mockItems(`bookmark:${key}`, 1, {
    kinds,
    count: MOCK_BOOKMARK_TOTALS[kind][rest],
    idBase: rest === "show" ? 300000 : 400000,
  }).map((item) => ({
    ...item,
    bookmarkId: String(77000000000 + (rest === "hide" ? 100000 : 0) + item.id),
    bookmarkRestrict: (rest === "hide" ? 1 : 0) as BookmarkRestrict,
  }));
  bookmarkPoolCache.set(key, items);
  return items;
}

/** 第 i 项的收藏标签：约 1/4 未分类（空串），其余按组内标签轮转（与池条目确定性对应）。 */
function mockBookmarkTagOf(kind: BookmarkKind, index: number): string {
  const tags = MOCK_BOOKMARK_TAGS[kind];
  return index % 4 === 3 ? "" : tags[index % tags.length];
}

/** 收藏列表：池按 tag（null 全部 / "" 未分类 / 标签名）过滤后 offset 切片，next 为续拉游标。 */
async function mockBookmarkList(
  kind: BookmarkKind,
  rest: BookmarkRest,
  tag: string | null,
  offset: number,
  limit: number
): Promise<BrowseBookmarkList> {
  await mockDelay();
  const pool = mockBookmarkPool(kind, rest);
  const filtered =
    tag === null ? pool : pool.filter((_, i) => mockBookmarkTagOf(kind, i) === tag);
  const slice = filtered.slice(offset, offset + limit);
  const next = offset + slice.length < filtered.length ? offset + slice.length : null;
  return { items: slice, total: filtered.length, next };
}

/** 收藏标签：由池实时统计（含未分类），计数与列表 total 自洽。 */
async function mockBookmarkTags(kind: BookmarkKind): Promise<BrowseBookmarkTags> {
  await mockDelay();
  const group = (rest: BookmarkRest): BrowseBookmarkTag[] => {
    const counts = new Map<string, number>();
    mockBookmarkPool(kind, rest).forEach((_, i) => {
      const name = mockBookmarkTagOf(kind, i);
      counts.set(name, (counts.get(name) ?? 0) + 1);
    });
    return [...counts.entries()]
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([name, count]) => ({ name, count }));
  };
  return { public: group("show"), private: group("hide") };
}

/** mock 收藏态存储：详情页 bookmarkState 与 add/remove 共享（仅浏览器 dev mock 生效）。 */
const mockBookmarkStore = new Map<string, WorkBookmarkState>();

/** mock 详情的默认收藏态：按 id 确定性预置（≈2/5 已收藏，覆盖公开/私密/未收藏三态视觉验收）。 */
function mockDefaultBookmarkState(kind: BookmarkKind, id: number): WorkBookmarkState | null {
  const roll = id % 5;
  if (roll === 0) return { bookmarkId: `7800000${id}`, restrict: 0 };
  if (roll === 1) return { bookmarkId: `7800001${id}`, restrict: 1 };
  return null;
}

async function mockBookmarkAdd(
  kind: BookmarkKind,
  id: number,
  restrict: BookmarkRestrict
): Promise<{ bookmarkId: string }> {
  await mockDelay();
  const state = { bookmarkId: `790000${restrict}${id}`, restrict };
  mockBookmarkStore.set(`${kind}:${id}`, state);
  return { bookmarkId: state.bookmarkId };
}

async function mockBookmarkRemove(kind: BookmarkKind, id: number, bookmarkId: string): Promise<void> {
  await mockDelay();
  mockBookmarkStore.delete(`${kind}:${id}`);
  void bookmarkId;
}

// ===== 浏览历史 mock =====

/** 浏览历史 mock 内存数组：模块级可变，record 去重置顶 / clear 清空（仅浏览器 dev mock 生效）。 */
let mockHistoryItems: BrowseHistoryItem[] | null = null;

/** 首次访问惰性生成 47 条混合历史（illust/manga/novel），visited_at 按小时递减（最新在前）。 */
function mockHistorySeed(): BrowseHistoryItem[] {
  const rand = mulberry32(seedFrom("browse_history"));
  const kinds: WorkKind[] = ["illust", "manga", "novel"];
  const now = Date.now();
  return Array.from({ length: 47 }, (_, i) => {
    const kind = kinds[Math.floor(rand() * kinds.length)] ?? "illust";
    const work = makeMockItem(rand, 8000000 + i, kind);
    return {
      work_id: work.id,
      kind,
      title: work.title,
      author_id: work.author_id,
      author_name: work.author_name,
      cover: work.cover ?? "",
      page_count: work.page_count,
      x_restrict: work.x_restrict ?? 0,
      visited_at: new Date(now - i * 3_600_000).toISOString(),
    };
  });
}

function mockHistoryEnsure(): BrowseHistoryItem[] {
  if (!mockHistoryItems) mockHistoryItems = mockHistorySeed();
  return mockHistoryItems;
}

/** record：同 work_id + kind 去重后置顶，visited_at 取当前时刻。 */
async function mockHistoryRecord(input: BrowseHistoryRecordInput): Promise<void> {
  await mockDelay();
  const arr = mockHistoryEnsure();
  const index = arr.findIndex((it) => it.work_id === input.workId && it.kind === input.kind);
  if (index >= 0) arr.splice(index, 1);
  arr.unshift({
    work_id: input.workId,
    kind: input.kind,
    title: input.title,
    author_id: input.authorId,
    author_name: input.authorName,
    cover: input.cover,
    page_count: input.pageCount,
    x_restrict: input.xRestrict,
    visited_at: new Date().toISOString(),
  });
}

/** list：按 kind（省略 = 全部）过滤后页码切片（内存已按 visited_at 倒序维护）。 */
async function mockHistoryList(
  page: number,
  pageSize: number,
  kind?: WorkKind
): Promise<BrowseHistoryList> {
  await mockDelay();
  const arr = kind ? mockHistoryEnsure().filter((it) => it.kind === kind) : mockHistoryEnsure();
  const start = (Math.max(1, page) - 1) * pageSize;
  return { items: arr.slice(start, start + pageSize), total: arr.length, page, page_size: pageSize };
}

async function mockHistoryClear(): Promise<{ status: string; deleted: number }> {
  await mockDelay();
  const deleted = mockHistoryEnsure().length;
  mockHistoryItems = [];
  return { status: "success", deleted };
}
