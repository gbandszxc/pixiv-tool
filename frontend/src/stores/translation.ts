import { defineStore } from "pinia";
import { ref } from "vue";
import { errorMessage } from "../api/tauri";
import { translateNovelPage } from "../api/translation";
import type { NovelTranslationInput, PageTranslation, TranslatedLine, TranslationPhase, TranslationProgressEvent } from "../api/translation";

/** 进行中的翻译任务快照；phase/received/summary/reasoning 由后端 progress 事件驱动刷新。 */
export interface TranslationActive {
  /** 任务自增标识；同小说同页 join 时复用，不在界面上展示。 */
  id: number;
  novelId: number;
  title: string;
  page: number;
  phase: TranslationPhase;
  received: number;
  summary: string;
  reasoning: string;
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
 * 完成结果快照：任务成功时写入（新对象引用即变更信号）。
 * 修复「翻译完成后页面不刷新」：发起页卸载后，结果写入被旧实例守卫拦截，
 * 返回的新实例靠 watch 本快照把译文同步进页内状态。
 */
export interface TranslationDone {
  novelId: number;
  page: number;
  status: PageTranslation["status"];
  lines: TranslatedLine[];
  target_language: string;
  /** 本次两轮请求的 token 用量合计；未调用模型或服务未下发时缺省。 */
  usage?: PageTranslation["usage"];
}

/** 完成快照只保留最近若干条：多小说并行时互不覆盖，同时避免长期占用内存。 */
const MAX_DONE = 8;

/**
 * 全局翻译任务状态（ADR 0029、0031）：store 自身持有各任务的 invoke promise 与计时器，
 * 调用方组件卸载不取消任务。不同小说并行（后端按小说加锁），同一小说的多页请求在前端
 * 串行排队——它们共享设定集与页译文文件，并发只会让后一页在后端锁上干等。
 */
export const useTranslationStore = defineStore("translation", () => {
  /** 在途任务：不同小说各一条，同一小说最多一条（下一页排在该小说的队列里）。 */
  const active = ref<TranslationActive[]>([]);
  const errors = ref<TranslationFailure[]>([]);
  const dones = ref<TranslationDone[]>([]);

  let nextId = 1;
  /** `${novelId}:${page}` → 在途 promise，重复调用 join 同一次请求。 */
  const inFlight = new Map<string, Promise<PageTranslation>>();
  /** 小说 ID → 该小说请求链的尾 promise（同小说多页按调用顺序串行）。 */
  const chains = new Map<number, Promise<void>>();
  let ticker: number | null = null;

  /** 当前小说的在途任务；别的小说页不显示他人进度（浮窗与阅读器共享该判定）。 */
  function activeFor(novelId: number): TranslationActive | null {
    return active.value.find((task) => task.novelId === novelId) ?? null;
  }

  /** 该小说最近一次完成快照；没有则 null。 */
  function doneFor(novelId: number): TranslationDone | null {
    for (let index = dones.value.length - 1; index >= 0; index--) {
      const done = dones.value[index];
      if (done.novelId === novelId) return done;
    }
    return null;
  }

  function ensureTicker(): void {
    if (ticker !== null) return;
    ticker = setInterval(() => {
      const now = Date.now();
      for (const task of active.value) task.elapsed = now - task.startedAt;
    }, 1000);
  }

  function releaseTicker(): void {
    if (active.value.length > 0 || ticker === null) return;
    clearInterval(ticker);
    ticker = null;
  }

  /** progress 事件只改发起它的那份快照：多小说并行时不会串到别人的进度条上。 */
  function applyEvent(taskId: number, evt: TranslationProgressEvent): void {
    const task = active.value.find((item) => item.id === taskId);
    if (!task) return;
    if (evt.type === "phase") {
      task.phase = evt.phase;
      return;
    }
    task.phase = evt.phase;
    task.received = evt.received;
    task.summary = evt.summary;
    task.reasoning = evt.reasoning;
  }

  async function runTask(novel: NovelTranslationInput, title: string, page: number, force: boolean): Promise<PageTranslation> {
    const task: TranslationActive = {
      id: nextId++,
      novelId: novel.novel_id,
      title,
      page,
      phase: "queued",
      received: 0,
      summary: "",
      reasoning: "",
      startedAt: Date.now(),
      elapsed: 0,
    };
    active.value = [...active.value, task];
    ensureTicker();
    try {
      const result = await translateNovelPage(novel, page, force, (evt) => applyEvent(task.id, evt));
      dones.value = [
        ...dones.value,
        {
          novelId: novel.novel_id,
          page,
          status: result.status,
          lines: result.lines,
          target_language: result.target_language,
          usage: result.usage,
        },
      ].slice(-MAX_DONE);
      return result;
    } catch (err) {
      errors.value = [...errors.value, { novelId: novel.novel_id, page, message: errorMessage(err) }];
      throw err;
    } finally {
      active.value = active.value.filter((item) => item.id !== task.id);
      releaseTicker();
    }
  }

  /**
   * 发起单页翻译。同一小说同一页重复调用返回同一 promise（join，不报错、不起第二个 invoke）；
   * 同一小说换页时排到该小说队尾顺序执行，不同小说立即并行。
   * force 由调用方决定（BrowseNovelView：已译/无需翻译的页再次点击即强制）。
   */
  function begin(novel: NovelTranslationInput, title: string, page: number, force = false): Promise<PageTranslation> {
    const key = `${novel.novel_id}:${page}`;
    const existing = inFlight.get(key);
    if (existing) return existing;

    const previous = chains.get(novel.novel_id) ?? Promise.resolve();
    const run = previous.then(() => runTask(novel, title, page, force));
    const tail = run.then(
      () => {},
      () => {}
    );
    chains.set(novel.novel_id, tail);
    inFlight.set(key, run);
    const settle = (): void => {
      if (inFlight.get(key) === run) inFlight.delete(key);
      if (chains.get(novel.novel_id) === tail) chains.delete(novel.novel_id);
    };
    void run.then(settle, settle);
    return run;
  }

  /** 读取并清除匹配（novelId+page）的失败记录；不匹配返回 null，保留现场。 */
  function consumeError(novelId: number, page: number): string | null {
    const index = errors.value.findIndex((failure) => failure.novelId === novelId && failure.page === page);
    if (index < 0) return null;
    const [failure] = errors.value.splice(index, 1);
    return failure.message;
  }

  return { active, errors, dones, activeFor, doneFor, begin, consumeError };
});
