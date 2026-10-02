/**
 * 浏览·搜索框「ID 直达」解析。
 *
 * 搜索框输入的内容可能是普通关键词，也可能是 Pixiv 链接 / ID。视图层在执行搜索前
 * 先经 parseBrowseInput 识别，命中则直接 router.push 跳站内自有路由（不发起搜索）：
 *
 * | 输入形态                                | 识别为     | 跳转路由                      |
 * |----------------------------------------|-----------|-------------------------------|
 * | ① 纯数字                                | 按 numericKind 归属（见下）      |
 * | ② URL 含 /artworks/{id}                | 插画/漫画   | /browse/work/illust/{id}      |
 * | ③ URL 含 /users/{id}                   | 用户主页    | /browse/user/{id}             |
 * | ④ novel/show.php?id= 或 /novel/{id}    | 小说       | /browse/work/novel/{id}       |
 * | ⑤ /novel/series/{id}                   | 小说系列    | /browse/series/novel/{id}     |
 * | ⑥ /user/{uid}/series/{sid}             | 插画/漫画系列 | /browse/series/illust/{sid}   |
 *
 * ① 纯数字自身不带类型信息，按调用方传入的 numericKind（搜索页的类型 tab）归属：
 * illust → /browse/work/illust/{id}，manga → /browse/work/manga/{id}，
 * novel → /browse/work/novel/{id}；缺省 illust（保持 V1 约定）。
 *
 * 边界处理：
 * - 尾随斜杠：`/artworks/123/` 仍命中（正则不锚定结尾）；
 * - 语言前缀：`en`、`ja`、`zh-cn` 等前缀（`/en/artworks/123`）可命中；
 * - `?query` 与 `#fragment`：不干扰路径匹配；`novel/show.php` 从查询串取 id；
 * - 协议/主机可省略：`https://www.pixiv.net/...`、`pixiv.net/...`、`/artworks/1`、
 *   `//pixiv.net/...` 均可命中；
 * - 数字安全：id 为 0 / 负数 / 超出 Number.MAX_SAFE_INTEGER 时不命中，
 *   回落为普通关键词搜索。
 */
export type ParsedBrowseInput =
  | { type: "illust-work"; id: number }
  | { type: "manga-work"; id: number }
  | { type: "novel-work"; id: number }
  | { type: "user"; id: number }
  | { type: "novel-series"; id: number }
  | { type: "illust-series"; id: number };

/** 纯数字输入的归属类型（搜索页的类型 tab；决定纯数字落到哪类作品详情）。 */
export type NumericKind = "illust" | "manga" | "novel";

/** 可选的 pixiv 语言前缀：`en/`、`ja/`、`zh-cn/` 等（1-2 段小写字母 + 斜杠）。 */
const LOCALE_PREFIX = "(?:[a-z]{2}(?:-[a-z]{2})?/)?";

/** ⑤ 小说系列：/novel/series/{id}（先于小说单篇匹配，规则互斥但保持顺序清晰）。 */
const RE_NOVEL_SERIES = new RegExp(`${LOCALE_PREFIX}novel/series/(\\d+)`, "i");
/**
 * ⑥ 插画/漫画系列：/user/{uid}/series/{sid}（官方插画与漫画系列共用此形态；
 * sid 生效、uid 仅消歧；兼容单复数 users?。必须先于用户主页规则匹配，
 * 否则 user/{uid} 前缀会被 ③ 吞掉）。
 */
const RE_ILLUST_SERIES = new RegExp(`${LOCALE_PREFIX}users?/(\\d+)/series/(\\d+)`, "i");
/** ④ 小说单篇（旧链接）：novel/show.php?id={id}（id 取自查询串，忽略其余参数）。 */
const RE_NOVEL_SHOW = new RegExp(`${LOCALE_PREFIX}novel/show\\.php\\?(?:[^#]*&)?id=(\\d+)`, "i");
/** ② 插画/漫画作品：/artworks/{id}。 */
const RE_ARTWORK = new RegExp(`${LOCALE_PREFIX}artworks/(\\d+)`, "i");
/** ④ 小说单篇（新链接）：/novel/{id}。 */
const RE_NOVEL = new RegExp(`${LOCALE_PREFIX}novel/(\\d+)`, "i");
/** ③ 用户主页：/users/{id}（兼容旧的单数 user/，与 utils/pixivUrl.ts 保持一致）。 */
const RE_USER = new RegExp(`${LOCALE_PREFIX}users?/(\\d+)`, "i");

/** 字符串 id → 安全正整数；越界/非法返回 null（调用方回落为关键词搜索）。 */
function toId(raw: string): number | null {
  const id = Number(raw);
  return Number.isSafeInteger(id) && id > 0 ? id : null;
}

/**
 * 解析搜索框输入；命中返回 `{type, id}`，识别不出返回 null（按普通关键词搜索）。
 *
 * @param numericKind 纯数字输入的归属（默认 "illust"）；链接形态自带类型，不受影响。
 *
 * @example
 * parseBrowseInput("  123456 ")            // { type: "illust-work", id: 123456 }
 * parseBrowseInput("  123456 ", "novel")   // { type: "novel-work", id: 123456 }
 * parseBrowseInput("  123456 ", "manga")   // { type: "manga-work", id: 123456 }
 * parseBrowseInput("https://www.pixiv.net/en/artworks/123456/") // { type: "illust-work", id: 123456 }
 * parseBrowseInput("pixiv.net/users/11")   // { type: "user", id: 11 }
 * parseBrowseInput("www.pixiv.net/novel/show.php?id=22&w=1#c") // { type: "novel-work", id: 22 }
 * parseBrowseInput("/zh-cn/novel/33/")     // { type: "novel-work", id: 33 }
 * parseBrowseInput("novel/series/44")      // { type: "novel-series", id: 44 }
 * parseBrowseInput("user/11/series/22")    // { type: "illust-series", id: 22 }
 * parseBrowseInput("風景 插画")             // null（关键词）
 */
export function parseBrowseInput(input: string, numericKind: NumericKind = "illust"): ParsedBrowseInput | null {
  const text = input.trim();
  if (!text) return null;

  // ① 纯数字 → 按调用方选中的类型归属（搜索页类型 tab）；小说/用户链接请粘贴对应 URL
  if (/^\d+$/.test(text)) {
    const id = toId(text);
    if (!id) return null;
    if (numericKind === "novel") return { type: "novel-work", id };
    if (numericKind === "manga") return { type: "manga-work", id };
    return { type: "illust-work", id };
  }

  // 链接形态：按特异度从高到低依次匹配
  const series = text.match(RE_NOVEL_SERIES);
  if (series) {
    const id = toId(series[1]);
    if (id) return { type: "novel-series", id };
  }

  const illustSeries = text.match(RE_ILLUST_SERIES);
  if (illustSeries) {
    const id = toId(illustSeries[2]);
    if (id) return { type: "illust-series", id };
  }

  const show = text.match(RE_NOVEL_SHOW);
  if (show) {
    const id = toId(show[1]);
    if (id) return { type: "novel-work", id };
  }

  const artwork = text.match(RE_ARTWORK);
  if (artwork) {
    const id = toId(artwork[1]);
    if (id) return { type: "illust-work", id };
  }

  const novel = text.match(RE_NOVEL);
  if (novel) {
    const id = toId(novel[1]);
    if (id) return { type: "novel-work", id };
  }

  const user = text.match(RE_USER);
  if (user) {
    const id = toId(user[1]);
    if (id) return { type: "user", id };
  }

  return null;
}
