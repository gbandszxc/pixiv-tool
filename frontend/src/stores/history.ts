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

export interface Illustration {
  artwork_id: number;
  title: string;
  author_id: number;
  author_name: string | null;
  illust_type: number;
  page_count: number;
  saved_paths: string;
  captured_at: string;
  status: string;
}

export interface NovelListResponse {
  items: Novel[];
  total: number;
  page: number;
  page_size: number;
}

export interface IllustrationListResponse {
  items: Illustration[];
  total: number;
  page: number;
  page_size: number;
}

export const useHistoryStore = defineStore("history", () => {
  const novels = ref<Novel[]>([]);
  const illustrations = ref<Illustration[]>([]);
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

  async function fetchIllustrations(params?: { author_id?: number; keyword?: string }) {
    const resp = await api.get("/api/illustrations", {
      params: { page: page.value, page_size: pageSize.value, ...params },
    });
    const data: IllustrationListResponse = resp.data;
    illustrations.value = data.items;
    total.value = data.total;
  }

  async function deleteNovel(novelId: number, deleteFile = false) {
    await api.delete(`/api/novels/${novelId}`, { params: { delete_file: deleteFile } });
    await fetchNovels();
  }

  async function deleteNovelsBatch(novelIds: number[], deleteFile = false) {
    await api.post("/api/novels/batch-delete", { novel_ids: novelIds, delete_file: deleteFile });
    await fetchNovels();
  }

  async function deleteAllNovels(deleteFile = false) {
    await api.delete("/api/novels", { params: { delete_file: deleteFile } });
    await fetchNovels();
  }

  async function deleteIllustration(artworkId: number, deleteFile = false) {
    await api.delete(`/api/illustrations/${artworkId}`, { params: { delete_file: deleteFile } });
    await fetchIllustrations();
  }

  async function deleteIllustrationsBatch(artworkIds: number[], deleteFile = false) {
    await api.post("/api/illustrations/batch-delete", { illustration_ids: artworkIds, delete_file: deleteFile });
    await fetchIllustrations();
  }

  async function deleteAllIllustrations(deleteFile = false) {
    await api.delete("/api/illustrations", { params: { delete_file: deleteFile } });
    await fetchIllustrations();
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
    novels,
    illustrations,
    total,
    page,
    pageSize,
    fetchNovels,
    fetchIllustrations,
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
