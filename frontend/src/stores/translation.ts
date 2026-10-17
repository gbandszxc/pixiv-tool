import { defineStore } from "pinia";
import { ref } from "vue";
import { errorMessage } from "../api/tauri";
import { translateNovelPage } from "../api/translation";
import type { NovelTranslationInput, PageTranslation, TranslationPhase, TranslationProgressEvent } from "../api/translation";

/** 进行中的翻译任务快照；phase/received/summary 由后端 progress 事件驱动刷新。 */
export interface TranslationActive {
  novelId: number;
  title: string;
  page: number;
  phase: TranslationPhase;
  received: number;
  summary: string;
  startedAt: number;
  elapsed: number;
}

/** 失败记录：跨页面保留，直到 consumeError 被匹配消费（修复「退出页面后失败被吞」）。 */
export interface TranslationFailure {
  novelId: number;
  page: number;
  message: string;
}

/**
 * 全局翻译任务状态（C 轻量版）：store 自身持有 invoke promise 与计时器，
 * 调用方组件卸载不取消任务，active/elapsed 继续刷新，失败写入 error 后原样 rethrow。
 */
export const useTranslationStore = defineStore("translation", () => {
  const active = ref<TranslationActive | null>(null);
  const error = ref<TranslationFailure | null>(null);

  /** 串行 in-flight promise：重入时直接 join 同一任务，不重复调用后端（后端另有 translation_lock）。 */
  let running: Promise<PageTranslation> | null = null;
  let ticker: number | null = null;

  function applyEvent(evt: TranslationProgressEvent): void {
    const task = active.value;
    if (!task) return;
    if (evt.type === "phase") {
      task.phase = evt.phase;
      return;
    }
    task.phase = evt.phase;
    task.received = evt.received;
    task.summary = evt.summary;
  }

  /**
   * 发起单页翻译。force 由调用方决定（BrowseNovelView：已译/无需翻译的页再次点击即强制）。
   * 进行中重复调用返回同一 promise（join，不报错、不起第二个 invoke）。
   */
  async function begin(novel: NovelTranslationInput, title: string, page: number, force = false): Promise<PageTranslation> {
    if (running) return running;
    active.value = {
      novelId: novel.novel_id,
      title,
      page,
      phase: "queued",
      received: 0,
      summary: "",
      startedAt: Date.now(),
      elapsed: 0,
    };
    ticker = setInterval(() => {
      const task = active.value;
      if (task) task.elapsed = Date.now() - task.startedAt;
    }, 1000);
    running = (async () => {
      try {
        return await translateNovelPage(novel, page, force, applyEvent);
      } catch (err) {
        error.value = { novelId: novel.novel_id, page, message: errorMessage(err) };
        throw err;
      }
    })();
    try {
      return await running;
    } finally {
      running = null;
      active.value = null;
      if (ticker !== null) {
        clearInterval(ticker);
        ticker = null;
      }
    }
  }

  /** 读取并清除匹配（novelId+page）的失败记录；不匹配返回 null，保留现场。 */
  function consumeError(novelId: number, page: number): string | null {
    const failure = error.value;
    if (!failure || failure.novelId !== novelId || failure.page !== page) return null;
    error.value = null;
    return failure.message;
  }

  return { active, error, begin, applyEvent, consumeError };
});
