import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "../api/tauri";
import type { HistoryItem, HistoryListResponse } from "../api/tauri";

export type { HistoryItem } from "../api/tauri";

export type HistoryCategory = "all" | "novel" | "illustration";

export const useHistoryStore = defineStore("history", () => {
  const items = ref<HistoryItem[]>([]);
  const total = ref(0);
  const page = ref(1);
  const pageSize = ref(50);

  async function fetchHistory(category: HistoryCategory, keyword?: string) {
    const data = await invoke<HistoryListResponse>("history_list", {
      category,
      page: page.value,
      pageSize: pageSize.value,
      // undefined 键在序列化时被丢弃，对齐 Rust 端的 Option 参数
      keyword,
    });
    items.value = data.items;
    total.value = data.total;
  }

  async function deleteNovel(novelId: number, deleteFile = false) {
    await invoke("novel_delete", { novelId, deleteFile });
  }

  async function deleteNovelsBatch(novelIds: number[], deleteFile = false) {
    await invoke("novels_batch_delete", { novelIds, deleteFile });
  }

  async function deleteAllNovels(deleteFile = false) {
    await invoke("novels_delete_all", { deleteFile });
  }

  async function deleteIllustration(artworkId: number, deleteFile = false) {
    await invoke("illustration_delete", { artworkId, deleteFile });
  }

  async function deleteIllustrationsBatch(artworkIds: number[], deleteFile = false) {
    await invoke("illustrations_batch_delete", { artworkIds, deleteFile });
  }

  async function deleteAllIllustrations(deleteFile = false) {
    await invoke("illustrations_delete_all", { deleteFile });
  }

  async function openNovelFile(novelId: number) {
    // 失败可能以 { error } 返回（旧 200+error 风格），也可能 reject string
    return invoke<{ status?: string; error?: string }>("open_novel_file", { novelId });
  }

  async function openIllustrationFolder(artworkId: number) {
    return invoke<{ status?: string; error?: string }>("open_illustration_folder", { artworkId });
  }

  return {
    items,
    total,
    page,
    pageSize,
    fetchHistory,
    deleteNovel,
    deleteNovelsBatch,
    deleteAllNovels,
    deleteIllustration,
    deleteIllustrationsBatch,
    deleteAllIllustrations,
    openNovelFile,
    openIllustrationFolder,
  };
});
