<template>
  <div class="history-view">
    <h1>历史</h1>

    <n-space style="margin-bottom: 16px">
      <n-input
        v-model:value="keyword"
        placeholder="搜索标题..."
        clearable
        style="width: 240px"
        @keyup.enter="handleSearch"
      />
      <n-button @click="handleSearch">搜索</n-button>
    </n-space>

    <n-data-table
      :columns="columns"
      :data="historyStore.novels"
      :loading="loading"
      :pagination="pagination"
      :row-key="(row: { novel_id: number }) => row.novel_id"
    />

    <n-empty v-if="!loading && !historyStore.novels.length" description="还没有抓取记录，去抓取页试试" />
  </div>
</template>

<script setup lang="ts">
import { ref, h, onMounted } from "vue";
import { NDataTable, NInput, NButton, NSpace, NEmpty, NPopconfirm } from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import { useHistoryStore } from "../stores/history";
import type { Novel } from "../stores/history";

const historyStore = useHistoryStore();
const loading = ref(false);
const keyword = ref("");

const columns: DataTableColumns<Novel> = [
  { title: "标题", key: "title", ellipsis: { tooltip: true } },
  { title: "作者", key: "author_name" },
  { title: "系列", key: "series_id", render: (row) => row.series_id ? String(row.series_id) : "-" },
  { title: "抓取时间", key: "captured_at", width: 180 },
  {
    title: "操作",
    key: "actions",
    width: 160,
    render(row) {
      return h(NSpace, { size: 4 }, () => [
        h(NButton, { size: "tiny", onClick: () => historyStore.openNovelFile(row.novel_id) }, () => "打开"),
        h(NPopconfirm, {
          onPositiveClick: () => historyStore.deleteNovel(row.novel_id, true),
        }, {
          trigger: () => h(NButton, { size: "tiny", type: "error" }, () => "删除"),
          default: () => "确定删除此记录？",
        }),
      ]);
    },
  },
];

const pagination = ref({
  page: 1,
  pageSize: 50,
  pageCount: 1,
  showSizePicker: false,
  onChange: (page: number) => {
    pagination.value.page = page;
    historyStore.page = page;
    loadData();
  },
});

async function loadData() {
  loading.value = true;
  await historyStore.fetchNovels(keyword.value ? { keyword: keyword.value } : undefined);
  pagination.value.pageCount = Math.ceil(historyStore.total / historyStore.pageSize);
  loading.value = false;
}

function handleSearch() {
  pagination.value.page = 1;
  historyStore.page = 1;
  loadData();
}

onMounted(() => loadData());
</script>
