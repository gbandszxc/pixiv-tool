<script setup lang="ts">
/**
 * 底部状态栏（下载 + 翻译后台任务）。
 *
 * 翻译侧以 Job 快照为准（ADR 0034）：每本小说最多一条活跃任务；单个任务整条文案即返回入口，
 * 多个任务显示计数并向上浮出选择菜单。暂停 / 等待确认 / 需要处理仍计入后台，但文案不谎称
 * 「正在进行」；已完成 / 已取消 / 已中断等终态不再占位。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { taskFailedCount, taskProgress, useTaskStore } from "../../stores/tasks";
import { useDownloadPanelStore } from "../../stores/downloadPanel";
import { useTranslationStore } from "../../stores/translation";
import type { TranslationJobSnapshot } from "../../api/translation";
const tasks = useTaskStore(); const panel = useDownloadPanelStore(); const translation = useTranslationStore();
const router = useRouter(); const route = useRoute(); const { t } = useI18n();
function openTasks() { if (route.path !== '/tools/tasks') void router.push('/tools/tasks'); }

/** 活跃翻译任务（每本小说一条；暂停 / 待确认 / 待处理也在此列，但不冒充进行中）。 */
const jobs = computed(() => translation.activeJobs);
/** 真正在推进（排队 / 运行 / 收尾）的任务数：决定计数文案用「正在进行」还是中性说法。 */
const runningCount = computed(() => jobs.value.filter((job) => ["queued", "running", "pausing", "cancelling"].includes(job.state)).length);

const pickerOpen = ref(false);
const picker = ref<HTMLElement | null>(null);
const trigger = ref<{ focus(): void } | null>(null);

function novelPath(id: number) { return `/browse/work/novel/${id}`; }
function goToNovel(id: number) {
  closePicker();
  if (route.path !== novelPath(id)) void router.push(novelPath(id));
}
function closePicker() { pickerOpen.value = false; }
function togglePicker() { pickerOpen.value = !pickerOpen.value; }

function jobTitle(job: TranslationJobSnapshot): string {
  return translation.titleFor(job.novel_id) || t("translation.job.untitled", { id: job.novel_id });
}
function stateText(job: TranslationJobSnapshot): string { return t(`translation.job.state.${job.state}`); }
function phaseText(job: TranslationJobSnapshot): string { return t(`translation.job.phase.${job.phase}`); }
/** 页码只在任务推进中有意义；暂停 / 失败等静止态不显示「页码未知」。 */
function pageText(job: TranslationJobSnapshot): string {
  if (job.state !== "running" && job.state !== "pausing" && job.state !== "awaiting_confirmation") {
    return "";
  }
  const page = job.current?.page;
  return page != null ? t("translation.job.page", { page }) : t("translation.job.pageUnknown");
}
/** 底栏单任务徽标：标题 + 可选页码 + 状态；页码为空时不留空段。 */
function badgeText(job: TranslationJobSnapshot): string {
  const page = pageText(job);
  const head = page ? `${jobTitle(job)} · ${page}` : jobTitle(job);
  return `${head} · ${stateText(job)}`;
}
/** 任务选择菜单行：状态 · 阶段 · 已用；页码为空时省略该段。 */
function pickerMeta(job: TranslationJobSnapshot): string {
  const page = pageText(job);
  return page
    ? t("translation.job.statusBarTaskMeta", { page, state: stateText(job), phase: phaseText(job), time: elapsedText(job) })
    : `${stateText(job)} · ${phaseText(job)} · ${t("translation.job.statusBarTaskTime", { time: elapsedText(job) })}`;
}
/** 已用时间：分′秒″；活跃任务在 store 的 1s ticker 上自走。 */
function elapsedText(job: TranslationJobSnapshot): string {
  const total = Math.max(0, Math.floor(translation.elapsedFor(job) / 1000));
  return `${Math.floor(total / 60)}′${String(total % 60).padStart(2, "0")}″`;
}
/** 计数入口：全部在推进时说「正在进行」，有暂停 / 待确认时不冒充。 */
const countLabel = computed(() =>
  runningCount.value === jobs.value.length
    ? t("translation.statusBarTranslatingCount", { count: jobs.value.length })
    : t("translation.job.statusBarCountIdle", { count: jobs.value.length })
);

async function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && !document.querySelector("dialog[open]")) closePicker();
}
watch(pickerOpen, (open) => {
  if (open) {
    window.addEventListener("keydown", onKeydown);
    void nextTick(() => picker.value?.focus());
  } else {
    window.removeEventListener("keydown", onKeydown);
    trigger.value?.focus();
  }
});
// 任务全部结束时菜单自动收起；降到一条时保留菜单（用户可能正在挑选）。
watch(() => jobs.value.length, (count) => {
  if (count === 0) pickerOpen.value = false;
});
onMounted(() => { void translation.ensureListening(); });
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <footer class="download-status" :aria-label="t('workspace.downloads')">
    <button class="status-link" @click="openTasks">
      <span>{{ t('workspace.downloads') }}</span>
      <span v-if="tasks.syncFailed" role="status">{{ t('workspace.syncFailed') }}</span>
      <template v-else-if="tasks.representative">
        <span>{{ t('workspace.activeCount', { count: tasks.activeTasks.length }) }}</span>
        <md-linear-progress class="mini-progress" :value="taskProgress(tasks.representative)" :indeterminate="tasks.representative.total <= 0 && tasks.representative.status !== 'paused'" :aria-label="t('tasks.progressDetail', { done: tasks.representative.done, total: tasks.representative.total, skipped: tasks.representative.skipped, failed: taskFailedCount(tasks.representative) })" />
        <span class="task-summary">{{ t(`tasks.status.${tasks.representative.status}`) }} · {{ tasks.representative.source_id }}<template v-if="tasks.representative.total > 0"> · {{ Math.round(taskProgress(tasks.representative) * 100) }}%</template></span>
      </template>
      <span v-else>{{ t('workspace.idle') }}</span>
    </button>
    <!-- 单任务：整条文案即返回入口（标题 · 页码 · 状态） -->
    <button
      v-if="jobs.length === 1"
      class="status-translation"
      :aria-label="t('translation.statusBarBackTo') + '：' + badgeText(jobs[0])"
      :title="badgeText(jobs[0])"
      @click="goToNovel(jobs[0].novel_id)"
    >
      <span class="translation-full">{{ badgeText(jobs[0]) }}</span>
      <span class="translation-short">{{ t('translation.job.statusBarShort') }}</span>
    </button>
    <!-- 多任务：计数入口 + 上方任务选择菜单（同一浮层配方） -->
    <div v-else-if="jobs.length > 1" class="translation-menu">
      <button ref="trigger" class="status-translation status-translation-count" :aria-label="countLabel" :title="countLabel" aria-haspopup="menu" :aria-expanded="pickerOpen" @click="togglePicker">
        <span class="translation-full">{{ countLabel }}</span>
        <span class="translation-short">{{ runningCount === jobs.length ? t('translation.statusBarTranslatingCountShort', { count: jobs.length }) : t('translation.job.statusBarCountIdleShort', { count: jobs.length }) }}</span>
      </button>
      <div v-if="pickerOpen" class="translation-backdrop" @click="closePicker"></div>
      <div v-if="pickerOpen" ref="picker" class="translation-picker" role="menu" :aria-label="t('translation.statusBarTasks')" tabindex="-1" @keydown.esc="closePicker">
        <span class="picker-title">{{ t('translation.statusBarTasks') }}</span>
        <button v-for="job in jobs" :key="job.job_id" role="menuitem" class="picker-item" @click="goToNovel(job.novel_id)">
          <span class="picker-dot" :class="`is-${job.state}`" aria-hidden="true"></span>
          <span class="picker-body">
            <span class="picker-name">{{ jobTitle(job) }}</span>
            <span class="picker-meta">{{ pickerMeta(job) }}</span>
          </span>
        </button>
      </div>
    </div>
    <button class="new-download" @click="panel.open()">{{ t('workspace.newDownload') }}</button>
  </footer>
</template>

<style scoped>
.download-status { display:flex; align-items:center; gap:var(--space-md); height:calc(var(--space-lg) * 2); padding:0 var(--space-lg); background:var(--md-sys-color-surface-container); border-top:1px solid color-mix(in srgb,var(--md-sys-color-outline) 35%,transparent); font-size:12px; }
button { color:var(--ink-muted); font:inherit; border:0; background:transparent; cursor:pointer; border-radius:var(--radius-control); height:100%; }
button:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
button:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:-2px; }
.status-link { flex:1; min-width:0; display:flex; align-items:center; gap:var(--space-md); text-align:left; white-space:nowrap; }
.status-link > span:not(.task-summary) { flex:none; }
.status-translation { flex:0 1 auto; min-width:0; display:flex; align-items:center; color:var(--md-sys-color-primary); padding:0 var(--space-sm); }
/* 单任务标题上限按底栏宽度计：长标题在 55% 处省略，剩余空间留给下载进度区 */
.download-status > .status-translation { max-width:55%; }
.translation-menu { position:relative; display:flex; flex:none; height:100%; }
.status-translation-count { flex:none; }
.translation-full { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.translation-short { display:none; white-space:nowrap; }
/* 透明遮罩只负责「点击外部关闭」，不做变暗 scrim（AccountMenu / 翻译浮窗同配方） */
.translation-backdrop { position:fixed; inset:0; z-index:890; }
.translation-picker { position:absolute; right:0; bottom:calc(100% + var(--space-sm)); z-index:900; display:flex; flex-direction:column; gap:var(--space-xxs); box-sizing:border-box; width:calc(var(--space-lg) * 24); max-width:calc(100vw - var(--space-lg) * 2); max-height:min(60vh,320px); overflow-y:auto; padding:var(--space-sm); border-radius:var(--radius-control); background:var(--md-sys-color-surface-container); color:var(--ink); box-shadow:0 4px 12px rgb(0 0 0 / 18%); outline:none; transform-origin:bottom right; }
.picker-title { padding:0 var(--space-sm); color:var(--ink-muted); font-size:12px; font-weight:600; line-height:32px; }
.picker-item { display:flex; align-items:flex-start; gap:var(--space-sm); width:100%; min-height:48px; padding:var(--space-sm); color:var(--ink); font:inherit; text-align:left; background:transparent; border:0; border-radius:var(--radius-control); cursor:pointer; }
.picker-item:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
.picker-item:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:-2px; }
/* 状态点：进行中 primary、完成绿、需处理 error、暂停 / 待确认中性（不新增语义色） */
.picker-dot { flex:none; width:8px; height:8px; margin-top:6px; border-radius:50%; background:var(--ink-subtle); }
.picker-dot.is-running, .picker-dot.is-queued, .picker-dot.is-pausing, .picker-dot.is-cancelling { background:var(--md-sys-color-primary); }
.picker-dot.is-needs_attention { background:var(--md-sys-color-error); }
.picker-dot.is-completed { background:var(--state-success-ink); }
.picker-body { display:flex; flex-direction:column; gap:2px; min-width:0; }
.picker-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:13px; line-height:1.4; }
.picker-meta { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--ink-muted); font-size:12px; line-height:1.4; }
.mini-progress { width:calc(var(--space-xl) * 5); flex:none; --md-linear-progress-track-height:4px; }
.task-summary { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.new-download { flex:none; color:var(--md-sys-color-primary); padding:0 var(--space-sm); }
@media (max-width:640px) { .task-summary { display:none; } .mini-progress { width:calc(var(--space-xl) * 2); } .download-status,.status-link { gap:var(--space-sm); } .translation-full { display:none; } .translation-short { display:inline; } }
</style>
