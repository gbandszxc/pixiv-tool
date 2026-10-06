import type { WorkKind } from "../api/browse";

export type PixivPageKind = "novel-single" | "novel-series" | "illust-series" | "illustration" | "user";

export interface ParsedPixivUrl {
  kind: PixivPageKind;
  id: string;
}

/** 作品页 URL：illust/manga/ugoira → /artworks/{id}；novel → /novel/show.php?id={id}。 */
export function pixivWorkUrl(kind: WorkKind, id: number | string): string {
  return kind === "novel"
    ? `https://www.pixiv.net/novel/show.php?id=${id}`
    : `https://www.pixiv.net/artworks/${id}`;
}

/** 作者主页 URL。 */
export function pixivUserUrl(id: number | string): string {
  return `https://www.pixiv.net/users/${id}`;
}

/** 小说系列页 URL。 */
export function pixivSeriesUrl(id: number | string): string {
  return `https://www.pixiv.net/novel/series/${id}`;
}

/** 插画/漫画系列页 URL（需作者 userId，追更列表「在 pixiv 打开」用）。 */
export function pixivIllustSeriesUrl(userId: number | string, seriesId: number | string): string {
  return `https://www.pixiv.net/user/${userId}/series/${seriesId}`;
}

/**
 * 个人资料编辑页（官方网页版，需登录）。
 * 证据（2026-10-06 本机 `curl -s -o /dev/null -w "%{http_code} %{redirect_url}"`）：
 *   /settings/profile    → 302 http://www.pixiv.net/?return_to=%2Fsettings%2Fprofile
 *   伪路径 /definitely-not-a-real-pixiv-path-xyz123 → 302 https://www.pixiv.net/definitely-not-a-real-pixiv-path-xyz123/
 *     （**注意**：伪路径不是 404，所以「未登录 302 ⇒ 路径存在」这条判别只在「重定向到 /?return_to=<原路径>」时成立，
 *      而不是「非 404 即存在」；本函数按前者成立采用）
 */
export function pixivProfileEditUrl(): string {
  return "https://www.pixiv.net/settings/profile";
}

/**
 * 作品投稿页（官方网页版，需登录）。
 * 同上探测：/illustration/create → 302 http://www.pixiv.net/?return_to=%2Fillustration%2Fcreate（判据成立）；
 * /novel/create → 302 https://www.pixiv.net/novel/create/（自带尾斜杠，与伪路径同形，**按此判据不成立/未证实**）
 * → 因此本次只提供「插画投稿」入口，按钮文案明确写「投稿插画作品」；
 * 小说投稿页的真实 URL 需在真实登录态下复核后再考虑补第二个入口。
 */
export function pixivUploadUrl(): string {
  return "https://www.pixiv.net/illustration/create";
}

/**
 * 解析 Pixiv 页面 URL，识别小说单篇、小说系列、插画/漫画系列、插画作品或用户主页
 */
export function parsePixivUrl(url: string): ParsedPixivUrl | null {
  if (!url) return null;

  // 1. 小说系列：/(?:[a-z]{2}/)?novel/series/(\d+)
  const seriesMatch = url.match(/(?:[a-z]{2}\/)?novel\/series\/(\d+)/i);
  if (seriesMatch) {
    return { kind: "novel-series", id: seriesMatch[1] };
  }

  // 1.5 插画/漫画系列：/(?:[a-z]{2}/)?user/{uid}/series/{sid}（官方两类系列共用；
  // sid 生效、uid 仅消歧。先于用户主页匹配，避免 user/{uid} 前缀被 4 吞掉）
  const illustSeriesMatch = url.match(/(?:[a-z]{2}\/)?users?\/(\d+)\/series\/(\d+)/i);
  if (illustSeriesMatch) {
    return { kind: "illust-series", id: illustSeriesMatch[2] };
  }

  // 2. 小说单篇：/(?:[a-z]{2}/)?novel/show\.php\?.*id=(\d+) 或 /(?:[a-z]{2}/)?novel/(\d+)
  const novelShowMatch = url.match(/(?:[a-z]{2}\/)?novel\/show\.php\?(?:.*&)?id=(\d+)/i);
  if (novelShowMatch) {
    return { kind: "novel-single", id: novelShowMatch[1] };
  }
  const novelMatch = url.match(/(?:[a-z]{2}\/)?novel\/(\d+)/i);
  if (novelMatch) {
    return { kind: "novel-single", id: novelMatch[1] };
  }

  // 3. 插画作品：/(?:[a-z]{2}/)?artworks/(\d+)
  const artworkMatch = url.match(/(?:[a-z]{2}\/)?artworks\/(\d+)/i);
  if (artworkMatch) {
    return { kind: "illustration", id: artworkMatch[1] };
  }

  // 4. 用户主页：/(?:[a-z]{2}/)?users?/(\d+)
  const userMatch = url.match(/(?:[a-z]{2}\/)?users?\/(\d+)/i);
  if (userMatch) {
    return { kind: "user", id: userMatch[1] };
  }

  return null;
}
