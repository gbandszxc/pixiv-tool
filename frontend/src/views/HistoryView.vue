<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('history.title') }}</h1>

    <n-space class="history-toolbar" justify="space-between" align="center">
      <n-space>
        <n-radio-group v-model:value="category" size="small" @update:value="handleCategoryChange">
          <n-radio-button value="novel">{{ t('history.category.novel') }}</n-radio-button>
          <n-radio-button value="illustration">{{ t('history.category.illustration') }}</n-radio-button>
        </n-radio-group>
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
            <n-button type="error" :disabled="!rows.length || loading" :loading="clearing">
              {{ t('history.deleteAll') }}
            </n-button>
          </template>
          {{ t('history.deleteAllConfirm') }}
        </n-popconfirm>
      </n-space>
    </n-space>

    <n-data-table
      :columns="columns"
      :data="rows"
      :loading="loading"
      :pagination="pagination"
      :row-key="rowKey"
      :checked-row-keys="checkedRowKeys"
      @update:checked-row-keys="handleCheck"
    />

    <n-empty v-if="!loading && !rows.length" :description="t('history.empty')" />
  </div>
</template>

<script setup lang="ts">
import { ref, h, onMounted, computed } from "vue";
import { NDataTable, NInput, NButton, NSpace, NEmpty, NPopconfirm, NTooltip, NRadioGroup, NRadioButton, useMessage } from "naive-ui";
import type { DataTableColumns, DataTableRowKey } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useHistoryStore } from "../stores/history";
import type { Novel, Illustration } from "../stores/history";

const { t } = useI18n();
const message = useMessage();
const historyStore = useHistoryStore();
const loading = ref(false);
const keyword = ref("");
const checkedRowKeys = ref<number[]>([]);
const batchDeleting = ref(false);
const clearing = ref(false);
const category = ref<"novel" | "illustration">("novel");

type HistoryRow = Novel | Illustration;

const rowKey = (row: HistoryRow) => "novel_id" in row ? row.novel_id : row.artwork_id;

const rows = computed(() =>
  category.value === "novel" ? historyStore.novels : historyStore.illustrations
);

const handleCheck = (keys: DataTableRowKey[]) => {
  checkedRowKeys.value = keys as number[];
};

// 文件夹图标（lucide folder 轮廓，随 currentColor 着色）
const FolderOpenIcon = () =>
  h(
    "svg",
    {
      viewBox: "0 0 24 24",
      width: "16",
      height: "16",
      fill: "none",
      stroke: "currentColor",
      "stroke-width": "2",
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
      "aria-hidden": "true",
    },
    h("path", {
      d: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
    })
  );

async function handleOpenFolder(row: HistoryRow) {
  try {
    const data = "novel_id" in row
      ? await historyStore.openNovelFile(row.novel_id)
      : await historyStore.openIllustrationFolder(row.artwork_id);
    if (data.error) {
      message.error(t("history.openFolderFailed"));
    }
  } catch {
    message.error(t("history.openFolderFailed"));
  }
}

function actionsRender(row: HistoryRow) {
  return h(NSpace, { size: 4 }, () => [
    h(
      NTooltip,
      { placement: "top" },
      {
        trigger: () =>
          h(
            NButton,
            {
              size: "tiny",
              quaternary: true,
              "aria-label": t("history.openFolder"),
              onClick: () => handleOpenFolder(row),
            },
            { icon: FolderOpenIcon }
          ),
        default: () => t("history.openFolder"),
      }
    ),
    h(
      NPopconfirm,
      {
        onPositiveClick: () => {
          if ("novel_id" in row) {
            return historyStore.deleteNovel(row.novel_id, true);
          }
          return historyStore.deleteIllustration(row.artwork_id, true);
        },
      },
      {
        trigger: () => h(NButton, { size: "tiny", type: "error" }, () => t("common.delete")),
        default: () => t("history.deleteConfirm"),
      }
    ),
  ]);
}

// computed 让列标题随 locale 切换自动更新
const columns = computed<DataTableColumns<HistoryRow>>(() =>
  category.value === "novel"
    ? [
        { type: "selection" },
        { title: t("history.titleColumn"), key: "title", ellipsis: { tooltip: true } },
        { title: t("history.authorColumn"), key: "author_name" },
        { title: t("history.seriesColumn"), key: "series_id", render: (row) => (row as Novel).series_id ? String((row as Novel).series_id) : "-" },
        { title: t("history.capturedAtColumn"), key: "captured_at", width: 180 },
        { title: t("history.actions"), key: "actions", width: 120, render: actionsRender },
      ]
    : [
        { type: "selection" },
        { title: t("history.titleColumn"), key: "title", ellipsis: { tooltip: true } },
        { title: t("history.authorColumn"), key: "author_name" },
        { title: t("history.pagesColumn"), key: "page_count", width: 80, render: (row) => String((row as Illustration).page_count) },
        { title: t("history.capturedAtColumn"), key: "captured_at", width: 180 },
        { title: t("history.actions"), key: "actions", width: 120, render: actionsRender },
      ]
);

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
  const params = keyword.value ? { keyword: keyword.value } : undefined;
  if (category.value === "novel") {
    await historyStore.fetchNovels(params);
  } else {
    await historyStore.fetchIllustrations(params);
  }
  pagination.value.pageCount = Math.ceil(historyStore.total / historyStore.pageSize);
  loading.value = false;
}

function handleSearch() {
  pagination.value.page = 1;
  historyStore.page = 1;
  loadData();
}

function handleCategoryChange() {
  checkedRowKeys.value = [];
  pagination.value.page = 1;
  historyStore.page = 1;
  loadData();
}

async function handleBatchDelete() {
  const ids = [...checkedRowKeys.value];
  batchDeleting.value = true;
  try {
    if (category.value === "novel") {
      await historyStore.deleteNovelsBatch(ids, false);
    } else {
      await historyStore.deleteIllustrationsBatch(ids, false);
    }
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
    if (category.value === "novel") {
      await historyStore.deleteAllNovels(false);
    } else {
      await historyStore.deleteAllIllustrations(false);
    }
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
