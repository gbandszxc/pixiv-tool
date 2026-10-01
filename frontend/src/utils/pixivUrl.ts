import type { WorkKind } from "../api/browse";

export type PixivPageKind = "novel-single" | "novel-series" | "illustration" | "user";

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

/**
 * 解析 Pixiv 页面 URL，识别小说单篇、小说系列、插画作品或用户主页
 */
export function parsePixivUrl(url: string): ParsedPixivUrl | null {
  if (!url) return null;

  // 1. 小说系列：/(?:[a-z]{2}/)?novel/series/(\d+)
  const seriesMatch = url.match(/(?:[a-z]{2}\/)?novel\/series\/(\d+)/i);
  if (seriesMatch) {
    return { kind: "novel-series", id: seriesMatch[1] };
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
