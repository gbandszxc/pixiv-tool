/**
 * 小说译文可用性预检（下载面板「下载内容」入口的置灰判据）。
 *
 * 只读现有 IPC：`novel_translation_jobs` 取 Job 清单、`browse_novel_series` 取系列目录，
 * 不新增后端命令。这里只回答「译文入口能不能点亮」，实际导出仍由后端严格校验
 * （源文件在盘、全系列每话 Completed、目标语言一致等）；预检通过不代表导出必然成功。
 */
import { browseNovelSeries, type BrowseSeriesDetail } from "../api/browse";
import { listTranslationJobs, type TranslationJobSnapshot } from "../api/translation";

export interface TranslationReadiness {
  available: boolean;
  /** 需要译文的篇数（单篇恒 1；系列为目录话数）。 */
  total: number;
  /** 已有可用译文的篇数。 */
  ready: number;
  /** 语言基准（单篇 = 该篇最新 completed Job；系列 = 首话最新 completed Job）。 */
  targetLanguage: string | null;
  /** 系列缺译文的话 order（升序）；单篇恒空。 */
  missingOrders: number[];
}

/** 系列目录聚合的翻页上限（游标每批 30 条；200 批 = 6000 话，防异常游标死循环）。 */
const SERIES_PAGE_LIMIT = 200;

/** 某小说最新的 completed Job：job_id 是时间戳前缀，字典序最大即最新。 */
function latestCompletedJob(jobs: TranslationJobSnapshot[], novelId: number): TranslationJobSnapshot | null {
  let latest: TranslationJobSnapshot | null = null;
  for (const job of jobs) {
    if (job.novel_id !== novelId || job.state !== "completed") continue;
    if (!latest || job.job_id > latest.job_id) latest = job;
  }
  return latest;
}

/** 单篇预检：存在该小说最新的 completed Job 即可用。 */
export async function checkNovelTranslationReadiness(novelId: number): Promise<TranslationReadiness> {
  const { jobs } = await listTranslationJobs();
  const latest = latestCompletedJob(jobs, novelId);
  return latest
    ? { available: true, total: 1, ready: 1, targetLanguage: latest.target_language, missingOrders: [] }
    : { available: false, total: 1, ready: 0, targetLanguage: null, missingOrders: [] };
}

/** 聚合系列完整目录：游标 last_order 从 0 起，按 id 去重；游标为空 / 非正 / 不前进即到底。 */
async function loadSeriesContents(seriesId: number): Promise<BrowseSeriesDetail["contents"]> {
  const byId = new Map<number, BrowseSeriesDetail["contents"][number]>();
  let cursor = 0;
  for (let page = 0; page < SERIES_PAGE_LIMIT; page += 1) {
    const batch = await browseNovelSeries(seriesId, cursor);
    for (const item of batch.contents) byId.set(item.id, item);
    const next = batch.next_last_order;
    if (next == null || next <= 0 || next === cursor) break;
    cursor = next;
  }
  return [...byId.values()];
}

/**
 * 系列预检：语言基准取首话（最小 order）最新 completed Job 的 target_language；
 * 每话都必须有同语言的 completed Job 才算可用。
 */
export async function checkSeriesTranslationReadiness(seriesId: number): Promise<TranslationReadiness> {
  const [{ jobs }, contents] = await Promise.all([listTranslationJobs(), loadSeriesContents(seriesId)]);
  const chapters = [...contents].sort((a, b) => a.series_order - b.series_order);
  const total = chapters.length;
  const targetLanguage = total ? latestCompletedJob(jobs, chapters[0].id)?.target_language ?? null : null;
  const missingOrders: number[] = [];
  for (const chapter of chapters) {
    const job = latestCompletedJob(jobs, chapter.id);
    if (!job || job.target_language !== targetLanguage) missingOrders.push(chapter.series_order);
  }
  const ready = total - missingOrders.length;
  return { available: targetLanguage !== null && ready === total, total, ready, targetLanguage, missingOrders };
}
