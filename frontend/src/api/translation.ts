import { Channel } from "@tauri-apps/api/core";
import { invoke, isTauri } from "./tauri";

export interface NovelTranslationInput {
  novel_id: number;
  title: string;
  tags: string[];
  description: string;
  content: string;
}
export interface TranslatedLine { line: number; text: string }
export type TranslationMode = "original" | "translated" | "bilingual";

/** 翻译内部阶段（novel_translate_page progress 事件流的 phase 值）。 */
export type TranslationPhase = "queued" | "prepare" | "translate";

/**
 * 翻译实时进度事件：novel_translate_page 的 progress Channel 每行推送一个 JSON 对象。
 * - phase：阶段切换（queued=命令入口 / prepare=Pass 1 / translate=Pass 2），不节流；
 * - progress：距上次 ≥300ms 且累计新增 ≥2048B 时推送，received 为译文累计字节数，
 *   summary 为最近一行译文摘要（Pass 1 恒空串，提取不到亦为空串）。
 */
export type TranslationProgressEvent =
  | { type: "phase"; phase: TranslationPhase }
  | { type: "progress"; phase: "prepare" | "translate"; received: number; summary: string };

export interface TranslationBook {
  bible: { style: string; terms: { source: string; translation: string; kind: string; aliases: string[]; notes: string }[] };
  pages: Record<string, TranslatedLine[]>;
}

/** 目标语言白名单，与后端 translation::TARGET_LANGUAGES 一致（顺序即选择项顺序）。 */
export const TARGET_LANGUAGE_CODES = ["zh-CN", "zh-TW", "en", "ja", "ko", "es", "fr", "de", "ru"] as const;

/** 接口协议白名单，与后端 translation::TRANSLATION_API_FORMATS 一致；默认 Chat Completions。 */
export const API_FORMATS = ["chat_completions", "responses", "anthropic"] as const;
export type ApiFormat = (typeof API_FORMATS)[number];

/** 协议回显：手改 settings.json 写入的未知值按默认 Chat Completions 处理（与后端加载回落一致）。 */
export function canonicalApiFormat(value: string): ApiFormat {
  return (API_FORMATS as readonly string[]).includes(value) ? (value as ApiFormat) : "chat_completions";
}

/** 归一化语言标签（与后端 normalize_language_code 一致），仅用于回显选中项。 */
export function canonicalLanguage(value: string): string {
  const text = value.trim().replace("_", "-").toLowerCase();
  const base = text.split("-")[0];
  if (base === "zh") return text.includes("tw") || text.includes("hk") || text.includes("hant") ? "zh-TW" : "zh-CN";
  return base;
}

/** 单页翻译结果：already_target_language = 原文已是目标语言，未调用模型。 */
export interface PageTranslation {
  status: "translated" | "already_target_language";
  lines: TranslatedLine[];
  target_language: string;
}

export interface TranslationProbe {
  api_url: string;
  api_key?: string;
  model: string;
  /** 接口协议（API_FORMATS 之一），空串由后端按默认 Chat Completions 处理。 */
  format: string;
  /** 单请求超时（秒）：「检测可用」按此值计时；获取模型列表固定 30 秒。 */
  timeout_seconds?: number;
  extra: Record<string, unknown>;
}

export async function fetchTranslationModels(probe: TranslationProbe): Promise<string[]> {
  if (!isTauri()) return ["demo-model", "demo-model-fast"];
  return invoke("translation_models", { probe });
}

export async function testTranslationService(probe: TranslationProbe): Promise<void> {
  if (!isTauri()) return;
  return invoke("translation_test", { probe });
}

export function getNovelTranslation(novel: NovelTranslationInput): Promise<TranslationBook> {
  if (!isTauri()) return Promise.resolve({ bible: { style: "", terms: [] }, pages: {} });
  return invoke("novel_translation_get", { novel });
}

/**
 * progress Channel 单行解析：非法 JSON 或形状不符 → null（该行忽略，不影响其他行）。
 */
function parseTranslationProgressEvent(raw: string): TranslationProgressEvent | null {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null) return null;
  const evt = value as Record<string, unknown>;
  if (evt.type === "phase") {
    return evt.phase === "queued" || evt.phase === "prepare" || evt.phase === "translate"
      ? { type: "phase", phase: evt.phase }
      : null;
  }
  if (evt.type === "progress") {
    const phase = evt.phase;
    if ((phase !== "prepare" && phase !== "translate") || typeof evt.received !== "number" || typeof evt.summary !== "string") return null;
    return { type: "progress", phase, received: evt.received, summary: evt.summary };
  }
  return null;
}

/** devMock 事件流步间隔（ms）：确定性循环，全部落在契约要求的 300~800ms 区间，便于反复验收。 */
const MOCK_TRANSLATION_STEP_MS = [400, 600, 300, 700, 500, 800, 450, 550, 650, 350];

/**
 * devMock 模拟事件流：与后端协议同构的确定性序列（queued → prepare → prepare 进度 →
 * translate → translate 进度），received 单调递增，Pass 1 summary 恒空串。
 */
const MOCK_TRANSLATION_EVENTS: TranslationProgressEvent[] = [
  { type: "phase", phase: "queued" },
  { type: "phase", phase: "prepare" },
  { type: "progress", phase: "prepare", received: 2048, summary: "" },
  { type: "progress", phase: "prepare", received: 5120, summary: "" },
  { type: "progress", phase: "prepare", received: 9216, summary: "" },
  { type: "phase", phase: "translate" },
  { type: "progress", phase: "translate", received: 14336, summary: "星光洒落在回家的路上，她的脚步很轻。" },
  { type: "progress", phase: "translate", received: 22016, summary: "于是他低下头，避开那道追过来的视线…" },
  { type: "progress", phase: "translate", received: 31232, summary: "「我们还会再见吗？」她对着夜空轻声问道。" },
  { type: "progress", phase: "translate", received: 40960, summary: "风穿过树梢，把未说完的话带向了远方。" },
  { type: "progress", phase: "translate", received: 49664, summary: "黎明之前，所有的等待都有了名字。" },
];

/** devMock：有界序列（11 步 × 300~800ms，约 5s 内必然 resolve），不会无限挂起。 */
async function mockTranslateNovelPage(onEvent?: (evt: TranslationProgressEvent) => void): Promise<PageTranslation> {
  for (const [index, evt] of MOCK_TRANSLATION_EVENTS.entries()) {
    await new Promise<void>((resolve) => {
      setTimeout(resolve, MOCK_TRANSLATION_STEP_MS[index % MOCK_TRANSLATION_STEP_MS.length]);
    });
    onEvent?.(evt);
  }
  return {
    status: "translated",
    // 与 mock 正文首页（browse.ts mockNovelDetail）行号对应的非空译文，让「完成即刷新」链路
    // 可在浏览器验收：行 0 = [chapter:序]，行 1 / 行 3 = 两段正文（行 2 为空行）。
    lines: [
      { line: 0, text: "序章" },
      { line: 1, text: "这是 mock 正文第一段的译文，用于视觉验收。" },
      { line: 3, text: "第二段的译文：注音与内嵌图标记同样走译文渲染。" },
    ],
    target_language: "zh-CN",
  };
}

/**
 * 单页翻译。progress Channel 逐行上报 JSON 事件（见 TranslationProgressEvent），
 * onEvent 可订阅实时进度；原文已是目标语言时后端直接返回 already_target_language，不请求模型。
 */
export function translateNovelPage(
  novel: NovelTranslationInput,
  page: number,
  force: boolean,
  onEvent?: (evt: TranslationProgressEvent) => void
): Promise<PageTranslation> {
  if (!isTauri()) return mockTranslateNovelPage(onEvent);
  const progress = new Channel<string>();
  progress.onmessage = (raw: string) => {
    const evt = parseTranslationProgressEvent(raw);
    if (evt) onEvent?.(evt);
  };
  return invoke("novel_translate_page", { novel, page, force, progress });
}
