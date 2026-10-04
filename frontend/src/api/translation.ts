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

export interface TranslationProbe {
  api_url: string;
  api_key?: string;
  model: string;
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
 */
export function translateNovelPage(novel: NovelTranslationInput, page: number, force: boolean): Promise<TranslatedLine[]> {
  return invoke("novel_translate_page", { novel, page, force, progress: new Channel<string>() });
}
