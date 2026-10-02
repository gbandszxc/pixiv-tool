/**
 * SauceNAO 以图识图 IPC 层 —— 契约见 `.subdriver/saucenao-image-search.plan.md`
 * 「契约」节（唯一权威），后端实现在 `src-tauri/src/saucenao.rs`（snake_case 返回体）。
 *
 * 与 api/browse.ts 的差异：
 * - 本命令与 pixiv 登录态无关，**不做** OPEN_LOGIN_EVENT 联动，错误文案原样向上抛由视图展示；
 * - 缩略图为 saucenao 签名临时 URL，原样直连（不走 pixiv-img 代理，见视图层 referrerpolicy）。
 *
 * mock 层：`!isTauri()`（普通浏览器直接打开 dev 页）时返回确定性样例数据，
 * 仅供浏览器内视觉验收；Tauri 生产环境完全不走 mock。样例刻意按乱序排列，
 * 用于验收视图层的「pixiv 恒最前 + similarity 降序」排序；第 4 条缩略图用失效
 * URL，用于验收加载失败占位块。
 */
import { errorMessage, invoke, isTauri } from "./tauri";

// 复用 tauri.ts 的基础封装，视图层可只 import 本文件。
export { errorMessage, invoke, isTauri };

/** 打开设置弹窗的自定义事件名（本页「key 未配置」引导条派发，App.vue 监听）。 */
export const OPEN_SETTINGS_EVENT = "pixiv-tool:open-settings";

// ===== 契约类型（snake_case，与 Rust SaucenaoSearchResponse 一致）=====

/** 单条匹配结果（后端已完成 similarity 数值化与 pixiv_id/member_id 回退链提取）。 */
export interface SaucenaoResult {
  /** 命中库编号（pixiv=5 / pixivhistorical=6）。 */
  index_id: number;
  /** 库名原样（形如 `Index #5: Pixiv Images - xxx.jpg`），展示时前端剥离前缀。 */
  index_name: string;
  /** 相似度（0-100 数值，后端 parseFloat）。 */
  similarity: number;
  /** saucenao 签名临时 URL，会过期；原样直连 + 失败占位块。 */
  thumbnail: string;
  /** index_id ∈ {5, 6}。 */
  is_pixiv: boolean;
  /** pixiv 作品 id（回退链仍拿不到为 null：只展示相似度，不给「站内打开」）。 */
  pixiv_id: number | null;
  member_id: number | null;
  member_name: string | null;
  title: string | null;
  /** 外链（ext_urls[0]）。 */
  ext_url: string | null;
}

/** 双窗口配额（short = 每 30 秒 / long = 每 24 小时；剩余量缺省 null）。 */
export interface SaucenaoQuota {
  short_limit: string;
  long_limit: string;
  short_remaining: number | null;
  long_remaining: number | null;
}

/** saucenao_search 返回体；无顶层 header 时 quota 为 null。 */
export interface SaucenaoSearchResponse {
  results: SaucenaoResult[];
  quota: SaucenaoQuota | null;
}

// ===== 命令封装（!isTauri() → mock）=====

/**
 * saucenao_search：以图识图搜索。
 * - sourceType="file"：source 为本地图片绝对路径（后端读文件 POST multipart）；
 * - sourceType="url"：source 为公网图片 URL（后端 GET `url=`）。
 * numres 不传（后端默认 16）；api_key 由后端读 settings，前端不经手。
 * 注意：调用方须先确认 settings.saucenao_api_key 非空（key 为空时前端显示
 * 引导态、不打请求；后端也会防御性 Err）。
 */
export async function searchSaucenao(
  sourceType: "file" | "url",
  source: string
): Promise<SaucenaoSearchResponse> {
  if (!isTauri()) return mockSearch();
  return invoke<SaucenaoSearchResponse>("saucenao_search", { sourceType, source });
}

// ===== mock 层（仅非 Tauri 环境生效）=====

const MOCK_DELAY_MS = 400;

function mockDelay(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, MOCK_DELAY_MS));
}

/** mock 缩略图：内联 SVG data URI（确定性，浏览器内可直接显示）。 */
function mockThumb(hue: number): string {
  const s = 128;
  const h2 = (hue + 42) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}" viewBox="0 0 ${s} ${s}">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="hsl(${hue} 52% 74%)"/><stop offset="1" stop-color="hsl(${h2} 46% 52%)"/>` +
    `</linearGradient></defs>` +
    `<rect width="${s}" height="${s}" fill="url(#g)"/>` +
    `<path d="M0 ${s * 0.72} Q ${s * 0.3} ${s * 0.52} ${s * 0.55} ${s * 0.7} T ${s} ${s * 0.64} V ${s} H 0 Z" fill="hsl(${(hue + 200) % 360} 38% 34%)" opacity="0.6"/>` +
    `<circle cx="${s * 0.72}" cy="${s * 0.26}" r="${s * 0.12}" fill="hsl(${hue} 70% 90%)"/>` +
    `</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/**
 * 确定性样例（4 条）：乱序排列供排序验收——
 * 1) index 21 anime（72.5）；2) index 5 pixiv（94.32，data 字段全命中）；
 * 3) index 34 deviantArt（61.03，缩略图用失效 URL 验收占位块）；
 * 4) index 6 pixiv 历史库（87.1，仅 ext_urls 回退 → pixiv_id 仍可提取、无 member/title）。
 */
async function mockSearch(): Promise<SaucenaoSearchResponse> {
  await mockDelay();
  const results: SaucenaoResult[] = [
    {
      index_id: 21,
      index_name: "Index #21: Anime - ep12_1043.jpg",
      similarity: 72.5,
      thumbnail: mockThumb(210),
      is_pixiv: false,
      pixiv_id: null,
      member_id: null,
      member_name: null,
      title: null,
      ext_url: "https://anidb.net/anime/12345",
    },
    {
      index_id: 5,
      index_name: "Index #5: Pixiv Images - 89737421_p0.jpg",
      similarity: 94.32,
      thumbnail: mockThumb(330),
      is_pixiv: true,
      pixiv_id: 89737421,
      member_id: 17473379,
      member_name: "星空ゆめ",
      title: "東方Project 例大祭新刊封面插图——这条标题刻意拉长，用于验收单行截断省略号与布局稳定性",
      ext_url: "https://www.pixiv.net/artworks/89737421",
    },
    {
      index_id: 34,
      index_name: "Index #34: deviantArt - 515715132.jpg",
      similarity: 61.03,
      // 故意失效的签名 URL（auth/exp 均过期形态）→ 视图层 @error 占位块验收
      thumbnail: "https://img1.saucenao.com/res/deviantart/515/515715132_s.jpg?auth=EXPIRED&exp=0",
      is_pixiv: false,
      pixiv_id: null,
      member_id: null,
      member_name: null,
      title: "Koshitantan + video link+stagedl",
      ext_url: "https://deviantart.com/view/515715132",
    },
    {
      index_id: 6,
      index_name: "Index #6: Pixiv Historical - 3836606_s.jpg",
      similarity: 87.1,
      thumbnail: mockThumb(90),
      is_pixiv: true,
      // data.pixiv_id 缺失、后端从 ext_urls 回退提取成功（member/title 无）
      pixiv_id: 3836606,
      member_id: null,
      member_name: null,
      title: null,
      ext_url: "https://www.pixiv.net/member_illust.php?mode=medium&illust_id=3836606",
    },
  ];
  return {
    results,
    quota: {
      short_limit: "4",
      long_limit: "150",
      short_remaining: 3,
      long_remaining: 148,
    },
  };
}
