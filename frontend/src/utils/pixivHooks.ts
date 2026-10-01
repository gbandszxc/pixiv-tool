/**
 * 浏览页 Pixiv 钩子：打开原页（系统默认浏览器）+ 返填到下载表单。
 * 打开动作走官方 opener 插件（capability `opener:default`）；
 * 返填复用抓取页既有 query 协议（sourceType/sourceId，挂载后自清 query）。
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import router from "../router";
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

/** 返填到下载表单：跳对应抓取页并带 sourceType/sourceId（CrawlView / IllustrationView 既有 query 协议）。 */
export function fillDownloadForm(target: DownloadTarget): void {
  void router.push({
    path: target.form === "novel" ? "/" : "/illustration",
    query: { sourceType: target.sourceType, sourceId: String(target.sourceId) },
  });
}

/** 列表卡片按「单篇」语义返填：小说 → 小说抓取页，插画/漫画/动图 → 插画抓取页。 */
export function workDownloadTarget(item: BrowseWorkItem): DownloadTarget {
  return {
    form: item.kind === "novel" ? "novel" : "illustration",
    sourceType: "single",
    sourceId: item.id,
  };
}
