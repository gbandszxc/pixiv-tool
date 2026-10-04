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

export function getNovelTranslation(novel: NovelTranslationInput): Promise<TranslationBook> {
  if (!isTauri()) return Promise.resolve({ bible: { style: "", terms: [] }, pages: {} });
  return invoke("novel_translation_get", { novel });
}

export function translateNovelPage(novel: NovelTranslationInput, page: number, force: boolean, onProgress: (stage: string) => void): Promise<TranslatedLine[]> {
  const progress = new Channel<string>();
  progress.onmessage = onProgress;
  return invoke("novel_translate_page", { novel, page, force, progress });
}
