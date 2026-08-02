<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('history.title') }}</h1>

    <n-space class="history-toolbar" justify="space-between" align="center">
      <n-space>
        <n-radio-group v-model:value="category" size="small" @update:value="handleCategoryChange">
          <n-radio-button value="all">{{ t('history.category.all') }}</n-radio-button>
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
import {
  NDataTable, NInput, NButton, NSpace, NEmpty, NPopconfirm, NTooltip,
  NRadioGroup, NRadioButton, NTag, useMessage,
} from "naive-ui";
import type { DataTableColumns, DataTableRowKey } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useHistoryStore } from "../stores/history";
import type { HistoryItem, HistoryCategory } from "../stores/history";

const { t } = useI18n();
const message = useMessage();
const historyStore = useHistoryStore();
const loading = ref(false);
const keyword = ref("");
const checkedRowKeys = ref<string[]>([]);
const batchDeleting = ref(false);
const clearing = ref(false);
const category = ref<HistoryCategory>("all");

const rows = computed(() => historyStore.items);

// novel_id 与 artwork_id 可跨表重号，行键用 "category:id" 保证唯一
const rowKey = (row: HistoryItem) => `${row.category}:${row.id}`;

const handleCheck = (keys: DataTableRowKey[]) => {
  checkedRowKeys.value = keys as string[];
};

// 统一按东八区（UTC+8，固定偏移，不随机器时区）显示 yyyy-MM-dd HH:mm:ss
function formatCapturedAt(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const s = new Date(d.getTime() + 8 * 3600 * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${s.getUTCFullYear()}-${pad(s.getUTCMonth() + 1)}-${pad(s.getUTCDate())} ${pad(s.getUTCHours())}:${pad(s.getUTCMinutes())}:${pad(s.getUTCSeconds())}`;
}

// 文件夹 / 垃圾桶图标（lucide 轮廓，随 currentColor 着色），与按钮尺寸一致
const FolderOpenIcon = () =>
  h(
    "svg",
    {
      viewBox: "0 0 24 24", width: "16", height: "16", fill: "none",
      stroke: "currentColor", "stroke-width": "2", "stroke-linecap": "round",
      "stroke-linejoin": "round", "aria-hidden": "true",
    },
    h("path", {
      d: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
    })
  );

const TrashIcon = () =>
  h(
    "svg",
    {
      viewBox: "0 0 24 24", width: "16", height: "16", fill: "none",
      stroke: "currentColor", "stroke-width": "2", "stroke-linecap": "round",
      "stroke-linejoin": "round", "aria-hidden": "true",
    },
    [
      h("path", { d: "M3 6h18" }),
      h("path", { d: "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" }),
      h("path", { d: "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" }),
      h("path", { d: "M10 11v6" }),
      h("path", { d: "M14 11v6" }),
    ]
  );

async function handleOpenFolder(row: HistoryItem) {
  try {
    const data = row.category === "novel"
      ? await historyStore.openNovelFile(row.id)
      : await historyStore.openIllustrationFolder(row.id);
    if (data.error) {
      message.error(t("history.openFolderFailed"));
    }
  } catch {
    message.error(t("history.openFolderFailed"));
  }
}

function actionsRender(row: HistoryItem) {
  return h(NSpace, { size: 8 }, () => [
    h(NTooltip, { placement: "top" }, {
      trigger: () =>
        h(NButton, {
          size: "tiny",
          quaternary: true,
          "aria-label": t("history.openFolder"),
          onClick: () => handleOpenFolder(row),
        }, { icon: FolderOpenIcon }),
      default: () => t("history.openFolder"),
    }),
    h(NPopconfirm, {
      onPositiveClick: async () => {
        if (row.category === "novel") {
          await historyStore.deleteNovel(row.id, true);
        } else {
          await historyStore.deleteIllustration(row.id, true);
        }
        await loadData();
      },
    }, {
      trigger: () =>
        h(NTooltip, { placement: "top" }, {
          trigger: () =>
            h(NButton, {
              size: "tiny",
              quaternary: true,
              type: "error",
              "aria-label": t("common.delete"),
            }, { icon: TrashIcon }),
          default: () => t("common.delete"),
        }),
      default: () => t("history.deleteConfirm"),
    }),
  ]);
}

// computed 让列标题随 locale 切换自动更新
const columns = computed<DataTableColumns<HistoryItem>>(() => {
  const base = [
    { type: "selection" as const },
    ...(category.value === "all"
      ? [{
          title: t("history.typeColumn"), key: "category", width: 72,
          render: (row: HistoryItem) =>
            h(NTag, { size: "small", type: "info" }, () =>
              row.category === "novel"
                ? t("history.category.novel")
                : t("history.category.illustration")),
        }]
      : []),
    { title: t("history.titleColumn"), key: "title", width: 220, ellipsis: { tooltip: true } },
    { title: t("history.authorColumn"), key: "author_name", width: 140, ellipsis: { tooltip: true } },
    ...(category.value === "novel"
      ? [{
          title: t("history.seriesColumn"), key: "series_id", width: 90,
          render: (row: HistoryItem) => (row.series_id != null ? String(row.series_id) : "-"),
        }]
      : []),
    ...(category.value !== "novel"
      ? [{ title: t("history.pagesColumn"), key: "pages", width: 72, render: (row: HistoryItem) => String(row.pages ?? "-") }]
      : []),
    {
      title: t("history.capturedAtColumn"), key: "captured_at", width: 170,
      render: (row: HistoryItem) => formatCapturedAt(row.captured_at),
    },
    { title: t("history.actions"), key: "actions", width: 88, render: actionsRender },
  ];
  return base as DataTableColumns<HistoryItem>;
});

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
  await historyStore.fetchHistory(category.value, keyword.value || undefined);
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
  const keys = [...checkedRowKeys.value];
  const novelIds = keys.filter((k) => k.startsWith("novel:")).map((k) => Number(k.slice(6)));
  const illustIds = keys.filter((k) => k.startsWith("illustration:")).map((k) => Number(k.slice(14)));
  batchDeleting.value = true;
  try {
    if (novelIds.length) await historyStore.deleteNovelsBatch(novelIds, false);
    if (illustIds.length) await historyStore.deleteIllustrationsBatch(illustIds, false);
    checkedRowKeys.value = [];
    message.success(t("history.batchDeleted", { count: keys.length }));
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
    if (category.value !== "illustration") await historyStore.deleteAllNovels(false);
    if (category.value !== "novel") await historyStore.deleteAllIllustrations(false);
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
