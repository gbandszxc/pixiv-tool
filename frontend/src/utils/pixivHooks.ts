/**
 * 浏览页 Pixiv 钩子：打开原页（系统默认浏览器）+ 就地打开下载表单。
 * 打开动作走官方 opener 插件（capability `opener:default`）；
 * 返填由会话级下载面板承载，不改变当前 URL。
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { useDownloadPanelStore } from "../stores/downloadPanel";
import { isTauri } from "../api/tauri";
import type { BrowseWorkItem } from "../api/browse";

export interface DownloadTarget {
  form: "novel" | "illustration";
  sourceType: "single" | "series" | "user";
  sourceId: number | string;
}

/** 用系统默认浏览器打开 pixiv 页面；非 Tauri（浏览器 mock 走查）退回 window.open。失败向上抛，由调用方提示。 */
export async function openInBrowser(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  await openUrl(url);
}

/** 在当前页面旁打开下载面板，不改变路由历史。 */
export function fillDownloadForm(target: DownloadTarget): void {
  useDownloadPanelStore().open(target);
}

/** 列表卡片按「单篇」语义返填：小说 → 小说表单，插画/漫画/动图 → 插画表单。 */
export function workDownloadTarget(item: BrowseWorkItem): DownloadTarget {
  return {
    form: item.kind === "novel" ? "novel" : "illustration",
    sourceType: "single",
    sourceId: item.id,
  };
}
