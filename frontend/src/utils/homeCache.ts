import type { BrowseWorkItem } from "../api/browse";

const PREFIX = "pixiv-tool-home-v1:";
const MAX_ITEMS = 120;
const MAX_AGE_MS = 24 * 60 * 60 * 1000;

/** 只保存卡片展示字段，不保存凭据；按已确认的账号隔离。 */
export function saveHomeCache(userId: string, items: BrowseWorkItem[]): void {
  if (!userId) return;
  try {
    const cards = items.slice(0, MAX_ITEMS).map(({ id, kind, title, author_id, author_name, cover, page_count, x_restrict }) =>
      ({ id, kind, title, author_id, author_name, cover, page_count, x_restrict })
    );
    localStorage.setItem(PREFIX + userId, JSON.stringify({ savedAt: Date.now(), items: cards }));
  } catch {
    // 存储禁用或容量不足时仍正常显示在线数据。
  }
}

export function readHomeCache(userId: string): BrowseWorkItem[] {
  if (!userId) return [];
  try {
    const cached = JSON.parse(localStorage.getItem(PREFIX + userId) || "null");
    if (!cached || !Number.isFinite(cached.savedAt) || cached.savedAt > Date.now() ||
        Date.now() - cached.savedAt > MAX_AGE_MS || !Array.isArray(cached.items) || cached.items.length > MAX_ITEMS) return [];
    // 持久化数据属于输入边界：损坏或旧格式不进入组件。
    const valid = cached.items.every((item: BrowseWorkItem) => item &&
      Number.isSafeInteger(item.id) && item.id > 0 && ["illust", "manga", "ugoira", "novel"].includes(item.kind) &&
      typeof item.title === "string" && Number.isSafeInteger(item.author_id) && typeof item.author_name === "string" &&
      (item.cover === undefined || typeof item.cover === "string") && Number.isSafeInteger(item.page_count) && item.page_count >= 0 &&
      (item.x_restrict === undefined || [0, 1, 2].includes(item.x_restrict)));
    return valid ? cached.items : [];
  } catch {
    return [];
  }
}
