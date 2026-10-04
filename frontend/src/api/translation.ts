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
 * 单页翻译。后端仍经 progress 上报内部阶段（queued/prepare/translate），
 * 但设定集整理属于实现细节，界面只呈现统一的「翻译中」。
 * 原文已是目标语言时后端直接返回 already_target_language，不请求模型。
 */
export function translateNovelPage(novel: NovelTranslationInput, page: number, force: boolean): Promise<PageTranslation> {
  if (!isTauri()) return Promise.resolve({ status: "translated", lines: [], target_language: "zh-CN" });
  return invoke("novel_translate_page", { novel, page, force, progress: new Channel<string>() });
}
