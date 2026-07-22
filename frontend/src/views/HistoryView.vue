<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('history.title') }}</h1>

    <n-space class="history-toolbar" justify="space-between" align="center">
      <n-space>
        <n-input
          v-model:value="keyword"
          :placeholder="t('history.searchPlaceholder')"
          clearable
          class="history-search"
          @keyup.enter="handleSearch"
        />
        <n-button @click="handleSearch">{{ t('common.search') }}</n-button>
      </n-space>
      <n-space>
        <n-popconfirm @positive-click="handleBatchDelete">
          <template #trigger>
            <n-button type="warning" :disabled="!checkedRowKeys.length" :loading="batchDeleting">
              {{ t('history.batchDelete') }}<span v-if="checkedRowKeys.length"> ({{ checkedRowKeys.length }})</span>
            </n-button>
          </template>
          {{ t('history.batchDeleteConfirm', { count: checkedRowKeys.length }) }}
        </n-popconfirm>
        <n-popconfirm @positive-click="handleDeleteAll">
          <template #trigger>
            <n-button type="error" :disabled="!historyStore.novels.length || loading" :loading="clearing">
              {{ t('history.deleteAll') }}
            </n-button>
          </template>
          {{ t('history.deleteAllConfirm') }}
        </n-popconfirm>
      </n-space>
    </n-space>

    <n-data-table
      :columns="columns"
      :data="historyStore.novels"
      :loading="loading"
      :pagination="pagination"
      :row-key="rowKey"
      :checked-row-keys="checkedRowKeys"
      @update:checked-row-keys="handleCheck"
    />

    <n-empty v-if="!loading && !historyStore.novels.length" :description="t('history.empty')" />
  </div>
</template>

<script setup lang="ts">
import { ref, h, onMounted, computed } from "vue";
import { NDataTable, NInput, NButton, NSpace, NEmpty, NPopconfirm, useMessage } from "naive-ui";
import type { DataTableColumns, DataTableRowKey } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useHistoryStore } from "../stores/history";
import type { Novel } from "../stores/history";

const { t } = useI18n();
const message = useMessage();
const historyStore = useHistoryStore();
const loading = ref(false);
const keyword = ref("");
const checkedRowKeys = ref<number[]>([]);
const batchDeleting = ref(false);
const clearing = ref(false);

const rowKey = (row: { novel_id: number }) => row.novel_id;

const handleCheck = (keys: DataTableRowKey[]) => {
  checkedRowKeys.value = keys as number[];
};

// computed 让列标题随 locale 切换自动更新
const columns = computed<DataTableColumns<Novel>>(() => [
  { type: "selection" },
  { title: t("history.titleColumn"), key: "title", ellipsis: { tooltip: true } },
  { title: t("history.authorColumn"), key: "author_name" },
  { title: t("history.seriesColumn"), key: "series_id", render: (row) => row.series_id ? String(row.series_id) : "-" },
  { title: t("history.capturedAtColumn"), key: "captured_at", width: 180 },
  {
    title: t("history.actions"),
    key: "actions",
    width: 160,
    render(row) {
      return h(NSpace, { size: 4 }, () => [
        h(NButton, { size: "tiny", onClick: () => historyStore.openNovelFile(row.novel_id) }, () => t("history.openFile")),
        h(NPopconfirm, {
          onPositiveClick: () => historyStore.deleteNovel(row.novel_id, true),
        }, {
          trigger: () => h(NButton, { size: "tiny", type: "error" }, () => t("common.delete")),
          default: () => t("history.deleteConfirm"),
        }),
      ]);
    },
  },
]);

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

async function handleBatchDelete() {
  const ids = [...checkedRowKeys.value];
  batchDeleting.value = true;
  try {
    await historyStore.deleteNovelsBatch(ids, false);
    checkedRowKeys.value = [];
    message.success(t("history.batchDeleted", { count: ids.length }));
    await loadData();
  } catch {
    message.error(t("common.deleteFailed"));
  } finally {
    batchDeleting.value = false;
  }
}

async function handleDeleteAll() {
  clearing.value = true;
  try {
    await historyStore.deleteAllNovels(false);
    checkedRowKeys.value = [];
    message.success(t("history.cleared"));
    pagination.value.page = 1;
    historyStore.page = 1;
    await loadData();
  } catch {
    message.error(t("common.deleteFailed"));
  } finally {
    clearing.value = false;
  }
}

onMounted(() => loadData());
</script>
