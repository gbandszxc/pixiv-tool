import { defineStore } from "pinia";
import { ref } from "vue";
import api from "../api";

export type HistoryCategory = "all" | "novel" | "illustration";

export interface HistoryItem {
  id: number;
  category: "novel" | "illustration";
  title: string;
  author_name: string | null;
  pages: number | null;
  series_id: number | null;
  illust_type: number | null;
  captured_at: string;
}

export interface HistoryListResponse {
  items: HistoryItem[];
  total: number;
  page: number;
  page_size: number;
}

export const useHistoryStore = defineStore("history", () => {
  const items = ref<HistoryItem[]>([]);
  const total = ref(0);
  const page = ref(1);
  const pageSize = ref(50);

  async function fetchHistory(category: HistoryCategory, keyword?: string) {
    const resp = await api.get("/api/history", {
      params: { category, page: page.value, page_size: pageSize.value, keyword },
    });
    const data: HistoryListResponse = resp.data;
    items.value = data.items;
    total.value = data.total;
  }

  async function deleteNovel(novelId: number, deleteFile = false) {
    await api.delete(`/api/novels/${novelId}`, { params: { delete_file: deleteFile } });
  }

  async function deleteNovelsBatch(novelIds: number[], deleteFile = false) {
    await api.post("/api/novels/batch-delete", { novel_ids: novelIds, delete_file: deleteFile });
  }

  async function deleteAllNovels(deleteFile = false) {
    await api.delete("/api/novels", { params: { delete_file: deleteFile } });
  }

  async function deleteIllustration(artworkId: number, deleteFile = false) {
    await api.delete(`/api/illustrations/${artworkId}`, { params: { delete_file: deleteFile } });
  }

  async function deleteIllustrationsBatch(artworkIds: number[], deleteFile = false) {
    await api.post("/api/illustrations/batch-delete", { illustration_ids: artworkIds, delete_file: deleteFile });
  }

  async function deleteAllIllustrations(deleteFile = false) {
    await api.delete("/api/illustrations", { params: { delete_file: deleteFile } });
  }

  async function openNovelFile(novelId: number) {
    const resp = await api.post(`/api/novels/${novelId}/open`);
    return resp.data as { status?: string; error?: string };
  }

  async function openIllustrationFolder(artworkId: number) {
    const resp = await api.post(`/api/illustrations/${artworkId}/open`);
    return resp.data as { status?: string; error?: string };
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
