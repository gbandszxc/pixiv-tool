/**
 * 正文区域 pixiv 内链：识别 → 路由映射 → 点击动作。
 *
 * 评论、小说正文、作品描述、简介等非按钮区域出现的 pixiv 链接，凡对应
 * 应用内已封装页面（作品 illust/manga、小说、用户、系列 novel/illust），
 * 点击经 router.push 跳站内自有路由（返回堆栈由 hash history + goBack()
 * 自然承接，SPEC §6.1）；未封装的链接（pixiv 标签页、jump.php 外链、
 * 其他站点 http(s)）点击走系统默认浏览器（ADR 0013，复用 openInBrowser
 * 的 http(s)/无 userinfo 校验）。
 *
 * 页面类型解析与搜索框「ID 直达」（utils/parseInput.ts）共用同一张
 * 「pixiv URL 形态 → ParsedBrowseInput」映射表；本文件的规则按特异度
 * 从高到低排列，并额外覆盖旧版别名形态与 jump.php 中转。新增自绘页面时
 * 的扩展步骤见 parseInput.ts 文件头。
 *
 * 渲染约定：正文一律按 segments 数组按元素渲染（不引入 v-html），
 * link 段为原生 button.text-link（配方见 DESIGN.md「正文内联链接」）。
 */
import type { Router } from "vue-router";
import type { ParsedBrowseInput } from "./parseInput";
import { openInBrowser } from "./pixivHooks";

/** 已封装页面类型 → 站内路由（与搜索框 ID 直达同一张映射；穷尽检查卡漏接）。 */
export function browseTargetPath(target: ParsedBrowseInput): string {
  switch (target.type) {
    case "illust-work":
      return `/browse/work/illust/${target.id}`;
    case "manga-work":
      return `/browse/work/manga/${target.id}`;
    case "novel-work":
      return `/browse/work/novel/${target.id}`;
    case "user":
      return `/browse/user/${target.id}`;
    case "novel-series":
      return `/browse/series/novel/${target.id}`;
    case "illust-series":
      return `/browse/series/illust/${target.id}`;
    default: {
      // 新增 ParsedBrowseInput 类型后必须接入目标页面，遗漏分支会导致类型检查失败。
      const unhandled: never = target;
      throw new Error(`未处理的浏览跳转类型：${unhandled}`);
    }
  }
}

/** 可选的 pixiv 语言前缀：`en/`、`zh-cn/` 等（与 parseInput.ts 保持一致）。 */
const LOCALE_PREFIX = "(?:[a-z]{2}(?:-[a-z]{2})?/)?";

/** 小说系列：/novel/series/{id}（先于小说单篇，规则互斥但保持顺序清晰）。 */
const RE_NOVEL_SERIES = new RegExp(`${LOCALE_PREFIX}novel/series/(\\d+)`, "i");
/** 插画/漫画系列：users?/{uid}/series/{sid}（sid 生效；先于用户主页，防 uid 被吞）。 */
const RE_ILLUST_SERIES = new RegExp(`${LOCALE_PREFIX}users?/(\\d+)/series/(\\d+)`, "i");
/** 小说单篇（旧链接形态）：novel/show.php?id={id}（id 取自查询串）。 */
const RE_NOVEL_SHOW = new RegExp(`${LOCALE_PREFIX}novel/show\\.php\\?(?:[^#]*&)?id=(\\d+)`, "i");
/** 作品（旧版别名）：illust.php / member_illust.php 的 illust_id 参数。 */
const RE_LEGACY_ILLUST = new RegExp(`${LOCALE_PREFIX}(?:member_)?illust\\.php\\?(?:[^#]*&)?illust_id=(\\d+)`, "i");
/** 作品：/artworks/{id}。 */
const RE_ARTWORK = new RegExp(`${LOCALE_PREFIX}artworks/(\\d+)`, "i");
/** 小说单篇（新链接）：/novel/{id}。 */
const RE_NOVEL = new RegExp(`${LOCALE_PREFIX}novel/(\\d+)`, "i");
/** 用户主页（旧版别名）：member_illust.php?id= 为作者插画列表，同属用户页。 */
const RE_LEGACY_USER = new RegExp(`${LOCALE_PREFIX}(?:member_illust|member|user)\\.php\\?(?:[^#]*&)?id=(\\d+)`, "i");
/** 用户主页：users?/{id}（最宽泛，放最后）。 */
const RE_USER = new RegExp(`${LOCALE_PREFIX}users?/(\\d+)`, "i");
/** jump.php 中转：路径含 /jump.php? 的整段查询。 */
const RE_JUMP = /jump\.php\?(.+)$/i;
/** 合法外链仅接受 http(s)（解码 jump.php 后同样适用）。 */
const RE_HTTP = /^https?:\/\//i;
/** 链接候选：http(s) 任意站点，或无协议但显式以 pixiv.net/ 开头的站内串。 */
const URL_RE = /https?:\/\/[^\s<>"']+|(?:www\.)?pixiv\.net\/[^\s<>"']+/gi;
/** 粘在链接尾部的句读标点（全/半角）：pixiv URL 不以这些字符结尾。 */
const TRAILING_PUNCT_RE = /[)\]}>.,;:!?、。！？：；…～"'”’）」』】〉》]+$/;

/** 字符串 id → 安全正整数；越界/非法返回 null（与 parseInput.toId 同口径）。 */
function toId(raw: string): number | null {
  const id = Number(raw);
  return Number.isSafeInteger(id) && id > 0 ? id : null;
}

/** 链接候选必须是 pixiv.net 站内 URL：带协议看主机，无协议要求显式域名开头。 */
function isPixivSiteUrl(url: string): boolean {
  return /^(?:https?:\/\/(?:www\.)?)?pixiv\.net(?:[/?#]|$)/i.test(url);
}

/**
 * pixiv 路径 → 已封装页面类型；非 pixiv 站 URL / 未封装路径返回 null
 * （调用方按外链打开）。规则与 parseInput.ts 映射表同源，另含旧版别名。
 */
function parsePixivPathTarget(url: string): ParsedBrowseInput | null {
  if (!isPixivSiteUrl(url)) return null;

  const novelSeries = url.match(RE_NOVEL_SERIES);
  if (novelSeries) {
    const id = toId(novelSeries[1]);
    if (id) return { type: "novel-series", id };
  }

  const illustSeries = url.match(RE_ILLUST_SERIES);
  if (illustSeries) {
    const id = toId(illustSeries[2]);
    if (id) return { type: "illust-series", id };
  }

  const show = url.match(RE_NOVEL_SHOW);
  if (show) {
    const id = toId(show[1]);
    if (id) return { type: "novel-work", id };
  }

  const legacyIllust = url.match(RE_LEGACY_ILLUST);
  if (legacyIllust) {
    const id = toId(legacyIllust[1]);
    if (id) return { type: "illust-work", id };
  }

  const artwork = url.match(RE_ARTWORK);
  if (artwork) {
    const id = toId(artwork[1]);
    if (id) return { type: "illust-work", id };
  }

  const novel = url.match(RE_NOVEL);
  if (novel) {
    const id = toId(novel[1]);
    if (id) return { type: "novel-work", id };
  }

  const legacyUser = url.match(RE_LEGACY_USER);
  if (legacyUser) {
    const id = toId(legacyUser[1]);
    if (id) return { type: "user", id };
  }

  const user = url.match(RE_USER);
  if (user) {
    const id = toId(user[1]);
    if (id) return { type: "user", id };
  }

  return null;
}

/**
 * 链接 URL → 已封装页面类型。jump.php 中转解码一次后按目标重判：
 * 解码出 pixiv 目标 → 内链；解码出 http(s) → 外开解码结果（跳过统计跳转）；
 * 解码失败 → 原样外开。其余未封装路径返回 null（外开原 URL）。
 */
export function parsePixivLinkTarget(url: string): ParsedBrowseInput | null {
  const jump = url.match(RE_JUMP);
  if (!jump) return parsePixivPathTarget(url);
  try {
    const decoded = decodeURIComponent(jump[1]);
    if (RE_HTTP.test(decoded)) {
      const inner = parsePixivPathTarget(decoded);
      if (inner) return inner;
    }
  } catch {
    // 解码失败按外链原样打开
  }
  return null;
}

/** jump.php 链接外开时优先用解码后的真实地址；非 http(s) / 解码失败返回 null。 */
function decodeJumpUrl(url: string): string | null {
  const jump = url.match(RE_JUMP);
  if (!jump) return null;
  try {
    const decoded = decodeURIComponent(jump[1]);
    return RE_HTTP.test(decoded) ? decoded : null;
  } catch {
    return null;
  }
}

/** 正文 linkify 后的段：纯文本或链接（target 非空 = 已封装页面，可站内跳转）。 */
export type LinkifiedSegment =
  | { kind: "text"; text: string }
  | { kind: "link"; text: string; url: string; target: ParsedBrowseInput | null };

/**
 * 正文标记/锚点里的链接地址归一：以 pixiv 根为基解析相对路径
 * （/jump.php、../users/…、jump.php?…），无协议候选补 https
 * （openInBrowser 只接受完整 http(s) URL）；其他 scheme（pixiv:// 等）
 * 原样返回（由调用方按外链安全校验兜底）。
 */
export function normalizePixivHref(href: string): string {
  if (RE_HTTP.test(href)) return href;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(href)) return href;
  try {
    return new URL(href, "https://www.pixiv.net/").toString();
  } catch {
    return href;
  }
}

/** 纯文本 → 段数组：URL（含未封装 pixiv 链接与外站链接）成 link 段，其余保持文本。 */
export function linkifyText(text: string): LinkifiedSegment[] {
  const segments: LinkifiedSegment[] = [];
  let last = 0;
  for (const match of text.matchAll(URL_RE)) {
    const start = match.index ?? 0;
    const raw = match[0];
    // 剥掉粘在尾部的句读（如「…/artworks/1。」），被剥部分归还正文
    const url = raw.replace(TRAILING_PUNCT_RE, "");
    if (!url) continue;
    if (start > last) segments.push({ kind: "text", text: text.slice(last, start) });
    segments.push({ kind: "link", text: url, url: normalizePixivHref(url), target: parsePixivLinkTarget(url) });
    last = start + url.length;
    if (url.length < raw.length) segments.push({ kind: "text", text: raw.slice(url.length) });
  }
  if (last < text.length) segments.push({ kind: "text", text: text.slice(last) });
  return segments;
}

/** 正文链接点击：已封装页面站内跳转（返回堆栈随 hash history 正常逐层返回）；其余外开。 */
export function openTextLink(router: Router, url: string, target: ParsedBrowseInput | null): void {
  if (target) {
    void router.push(browseTargetPath(target));
    return;
  }
  void openInBrowser(decodeJumpUrl(url) ?? url).catch(() => {
    // 正文内链打开失败不打断阅读，静默即可
  });
}
