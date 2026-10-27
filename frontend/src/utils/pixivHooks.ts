/**
 * 浏览页 Pixiv 钩子：打开原页（系统默认浏览器）+ 就地打开下载表单。
 * 打开动作走官方 opener 插件（capability `opener:default`）；
 * 返填由会话级下载面板承载，不改变当前 URL。
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { useDownloadPanelStore } from "../stores/downloadPanel";
import { isTauri } from "../api/tauri";
import type { BrowseWorkItem, IllustPage } from "../api/browse";

export interface DownloadTarget {
  form: "novel" | "illustration";
  sourceType: "single" | "series" | "user";
  sourceId: number | string;
  /**
   * 详情页已加载的页面（插画单篇）；面板据此直接渲染按页选择，省掉一次详情请求。
   * 省略 = 面板自己拉取详情。
   */
  pages?: IllustPage[];
}

/** 用系统默认浏览器打开 pixiv 页面；非 Tauri（浏览器 mock 走查）退回 window.open。失败向上抛，由调用方提示。 */
export async function openInBrowser(url: string): Promise<void> {
  const target = new URL(url);
  if (!["https:", "http:"].includes(target.protocol) || target.username || target.password) {
    throw new Error("不支持的外部网页地址");
  }
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  await openUrl(url);
}

/** 在当前页面旁打开下载面板，不改变路由历史。target.pages 一并透传：面板据此免去一次详情请求。 */
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
