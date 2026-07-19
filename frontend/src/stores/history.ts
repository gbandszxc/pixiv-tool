import { defineStore } from "pinia";
import { ref } from "vue";
import api from "../api";

export interface Novel {
  novel_id: number;
  title: string;
  series_id: number | null;
  series_order: number | null;
  author_id: number;
  author_name: string | null;
  page_count: number | null;
  text_length: number | null;
  captured_at: string;
  modification_date: string | null;
  txt_path: string | null;
  md_path: string | null;
  status: string;
}

export interface NovelListResponse {
  items: Novel[];
  total: number;
  page: number;
  page_size: number;
}

export const useHistoryStore = defineStore("history", () => {
  const novels = ref<Novel[]>([]);
  const total = ref(0);
  const page = ref(1);
  const pageSize = ref(50);

  async function fetchNovels(params?: { series_id?: number; author_id?: number; keyword?: string }) {
    const resp = await api.get("/api/novels", {
      params: { page: page.value, page_size: pageSize.value, ...params },
    });
    const data: NovelListResponse = resp.data;
    novels.value = data.items;
    total.value = data.total;
  }

  async function deleteNovel(novelId: number, deleteFile = false) {
    await api.delete(`/api/novels/${novelId}`, { params: { delete_file: deleteFile } });
    await fetchNovels();
  }

  async function openNovelFile(novelId: number) {
    await api.post(`/api/novels/${novelId}/open`);
  }

  return { novels, total, page, pageSize, fetchNovels, deleteNovel, openNovelFile };
});
