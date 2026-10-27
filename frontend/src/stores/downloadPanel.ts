import { defineStore } from "pinia";
import { reactive, ref } from "vue";
import type { IllustPage } from "../api/browse";
import type { DownloadTarget } from "../utils/pixivHooks";

export type DownloadFormKind = DownloadTarget["form"];
function emptyDraft() { return { sourceType: "single", sourceId: "", formats: ["txt", "markdown"], edited: false }; }

/** 单篇插画的分页缓存：同一作品命中缓存不重复请求详情。 */
export interface PageCache {
  id: string;
  pages: IllustPage[];
  saved: number[];
  ugoira: boolean;
}

/** 非模态下载工作区：草稿只留在当前会话，不进入路由或磁盘。 */
export const useDownloadPanelStore = defineStore("downloadPanel", () => {
  const visible = ref(false);
  const kind = ref<DownloadFormKind>("novel");
  const drafts = reactive({ novel: emptyDraft(), illustration: emptyDraft() });
  /** 按页选择（0 起页码）；按表单分开保存，切 tab 不互相污染 */
  const pageSelection = reactive<{ novel: number[]; illustration: number[] }>({ novel: [], illustration: [] });
  /** 详情页预置的 pages/saved 缓存；列表返填时由面板自己拉取 */
  const pageCache = reactive<{ illustration: PageCache | null }>({ illustration: null });
  const pendingTarget = ref<DownloadTarget | null>(null);
  const submitting = ref(false);
  const error = ref("");
  const session = ref(0);
  let trigger: HTMLElement | null = null;

  function applyTarget(target: DownloadTarget) {
    kind.value = target.form;
    Object.assign(drafts[target.form], { sourceType: target.sourceType, sourceId: String(target.sourceId), edited: false });
    // 换来源即换作品：旧的页码选择不再适用
    pageSelection[target.form] = [];
    // 详情页带了已加载的 pages：直接预置缓存，面板据此免去一次详情请求；
    // 未带 pages 时（列表/作者页返填）清掉旧作品的缓存，交给面板自己拉取
    if (target.form === "illustration") {
      const id = String(target.sourceId);
      if (target.pages) pageCache.illustration = { id, pages: target.pages, saved: [], ugoira: false };
      else if (pageCache.illustration?.id !== id) pageCache.illustration = null;
    }
    error.value = "";
  }
  /** 父级（详情页）已加载的 pages 直接预置，省掉面板侧的一次详情请求 */
  function setPageCache(cache: PageCache) {
    pageCache.illustration = cache;
  }
  function resetPageCache() {
    pageCache.illustration = null;
  }
  function setPageSelection(form: DownloadFormKind, pages: number[]) {
    pageSelection[form] = [...pages].sort((a, b) => a - b);
  }
  function open(target?: DownloadTarget) {
    trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    visible.value = true;
    if (!target || submitting.value) return;
    const draft = drafts[target.form];
    if (draft.edited && draft.sourceId.trim() && (draft.sourceId !== String(target.sourceId) || draft.sourceType !== target.sourceType)) {
      pendingTarget.value = target;
    } else applyTarget(target);
  }
  function confirmTarget() {
    if (pendingTarget.value) applyTarget(pendingTarget.value);
    pendingTarget.value = null;
  }
  function close(restoreFocus = true) {
    visible.value = false;
    pendingTarget.value = null;
    if (restoreFocus) {
      queueMicrotask(() => {
        const fallback = document.querySelector<HTMLElement>(".app-content h1, .app-content .page-heading, .app-content");
        const target = trigger?.isConnected && !trigger.closest(".download-panel") ? trigger : fallback;
        if (target && !target.hasAttribute("tabindex") && target === fallback) target.tabIndex = -1;
        target?.focus({ preventScroll: true });
      });
    }
  }
  function reset() {
    session.value++;
    close(false);
    Object.assign(drafts.novel, emptyDraft());
    Object.assign(drafts.illustration, emptyDraft());
    pageSelection.novel = [];
    pageSelection.illustration = [];
    pageCache.illustration = null;
    error.value = "";
  }
  return {
    visible,
    kind,
    drafts,
    pageSelection,
    pageCache,
    pendingTarget,
    submitting,
    error,
    session,
    open,
    close,
    confirmTarget,
    reset,
    setPageSelection,
    setPageCache,
    resetPageCache,
  };
});
