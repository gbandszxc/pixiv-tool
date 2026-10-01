/**
 * 缩略图尺寸档位改写（纯函数）。
 *
 * 背景：列表封面由 pixiv 接口给出 `/c/<尺寸段>/...` 形式，详情页 pages 的 medium 则是
 * `/img-master/img/...` 原样（不带 /c/ 前缀）。本模块只在原 URL 上替换 / 插入尺寸段，
 * 绝不构造日期路径或文件名（拼错即 404），也不碰查询串与锚点。
 *
 * 约束：零 import、纯函数、只用可擦除类型语法（enum / namespace 会让 node 的类型
 * 剥离失效），保证 `node --experimental-strip-types` 可直接 import 断言。
 */

export type ThumbTier = "small" | "medium" | "large" | "original";

const TIER_SIZE: Record<Exclude<ThumbTier, "original">, string> = {
  small: "250x250_80_a2",
  medium: "540x540_70",
  large: "600x1200_90",
};

/** https:// 之后 host 段结束的位置（无 path/query/hash 时为串尾）。 */
function hostEnd(url: string): number {
  const rest = url.slice("https://".length);
  const idx = rest.search(/[/?#]/);
  return idx < 0 ? url.length : "https://".length + idx;
}

/**
 * 按档位改写 pximg 图片 URL 的尺寸段：
 * - 空 URL → ""；tier === "original" → 原样；
 * - 非 https:// 或 host 不以 .pximg.net 结尾 → 原样（含 data: URI 与头像等其它路径）；
 * - path 以 /c/<尺寸段>/ 开头 → 替换该尺寸段；以 /img-master/img/ 开头 → host 后插入 /c/<尺寸段>/；
 * - 其余（/img-original/、/user-profile/ 等）→ 原样。
 */
export function thumbUrl(url: string | undefined | null, tier: ThumbTier): string {
  if (!url) return "";
  if (tier === "original") return url;
  if (!/^https:\/\//i.test(url)) return url;
  const end = hostEnd(url);
  const authority = url.slice("https://".length, end);
  const hostPort = authority.includes("@")
    ? authority.slice(authority.lastIndexOf("@") + 1)
    : authority;
  const host = (hostPort.split(":")[0] ?? "").toLowerCase();
  if (!host.endsWith(".pximg.net")) return url;

  const path = url.slice(end);
  const size = TIER_SIZE[tier] ?? TIER_SIZE.medium; // 运行时脏值兜底：settings.json 手改可能绕过写路径校验
  if (path.startsWith("/c/")) {
    const next = path.indexOf("/", "/c/".length);
    if (next > "/c/".length) {
      return `${url.slice(0, end)}/c/${size}${path.slice(next)}`;
    }
    return url;
  }
  if (path.startsWith("/img-master/img/")) {
    return `${url.slice(0, end)}/c/${size}${path}`;
  }
  return url;
}
