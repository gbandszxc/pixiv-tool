import { defineStore } from "pinia";
import { reactive, ref } from "vue";
import type { DownloadTarget } from "../utils/pixivHooks";

export type DownloadFormKind = DownloadTarget["form"];
function emptyDraft() { return { sourceType: "single", sourceId: "", formats: ["txt", "markdown"], edited: false }; }

/** 非模态下载工作区：草稿只留在当前会话，不进入路由或磁盘。 */
export const useDownloadPanelStore = defineStore("downloadPanel", () => {
  const visible = ref(false);
  const kind = ref<DownloadFormKind>("novel");
  const drafts = reactive({ novel: emptyDraft(), illustration: emptyDraft() });
  const pendingTarget = ref<DownloadTarget | null>(null);
  const submitting = ref(false);
  const error = ref("");
  const session = ref(0);
  let trigger: HTMLElement | null = null;

  function applyTarget(target: DownloadTarget) {
    kind.value = target.form;
    Object.assign(drafts[target.form], { sourceType: target.sourceType, sourceId: String(target.sourceId), edited: false });
    error.value = "";
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
    error.value = "";
  }
  return { visible, kind, drafts, pendingTarget, submitting, error, session, open, close, confirmTarget, reset };
});
