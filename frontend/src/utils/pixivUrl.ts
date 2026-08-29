export type PixivPageKind = "novel-single" | "novel-series" | "illustration" | "user";

export interface ParsedPixivUrl {
  kind: PixivPageKind;
  id: string;
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
