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
import { isTerminalJobState, type TranslationJobSnapshot } from "../../api/translation";
import { errorMessage } from "../../api/tauri";
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
/** 页码是可选信息：整篇任务尚未定位到页面、或处于静止态时都省略，不显示「未知」占位。 */
function pageText(job: TranslationJobSnapshot): string {
  if (job.state !== "running" && job.state !== "pausing" && job.state !== "awaiting_confirmation") {
    return "";
  }
  const page = job.current?.page;
  return page != null ? t("translation.job.page", { page }) : "";
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

/** 快速取消：底栏 / 浮窗直达，不进小说页；同一时间只放行一个在途取消。 */
const pendingCancel = ref("");
const cancelError = ref("");
async function quickCancel(job: TranslationJobSnapshot) {
  if (pendingCancel.value) return;
  pendingCancel.value = job.job_id;
  cancelError.value = "";
  try {
    await translation.cancel(job.job_id);
  } catch (error) {
    // 取消失败行不消失（任务仍是活跃态），错误留在浮层 / 底栏供重试或进页面处理。
    cancelError.value = errorMessage(error);
  } finally {
    pendingCancel.value = "";
  }
}
/** 取消入口的可达名：任务名进可达名，标题只留动作避免与徽标文案重复。 */
function cancelLabel(job: TranslationJobSnapshot): string {
  return `${t("translation.statusBarQuickCancel")}：${jobTitle(job)}`;
}

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
    <!-- 单任务：整条文案即返回入口（标题 · 页码 · 状态），右侧直达取消 -->
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
    <button
      v-if="jobs.length === 1 && !isTerminalJobState(jobs[0].state)"
      class="status-cancel"
      :aria-label="cancelLabel(jobs[0])"
      :title="t('translation.statusBarQuickCancel')"
      :disabled="pendingCancel === jobs[0].job_id"
      @click="quickCancel(jobs[0])"
    >
      <svg class="cancel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18" /><path d="m6 6 12 12" /></svg>
    </button>
    <span v-if="cancelError" role="status" class="bar-cancel-error" :title="cancelError">{{ cancelError }}</span>
    <!-- 多任务：计数入口 + 上方任务选择菜单（同一浮层配方） -->
    <div v-else-if="jobs.length > 1" class="translation-menu">
      <button ref="trigger" class="status-translation status-translation-count" :aria-label="countLabel" :title="countLabel" aria-haspopup="menu" :aria-expanded="pickerOpen" @click="togglePicker">
        <span class="translation-full">{{ countLabel }}</span>
        <span class="translation-short">{{ runningCount === jobs.length ? t('translation.statusBarTranslatingCountShort', { count: jobs.length }) : t('translation.job.statusBarCountIdleShort', { count: jobs.length }) }}</span>
      </button>
      <div v-if="pickerOpen" class="translation-backdrop" @click="closePicker"></div>
      <div v-if="pickerOpen" ref="picker" class="translation-picker" role="menu" :aria-label="t('translation.statusBarTasks')" tabindex="-1" @keydown.esc="closePicker">
        <span class="picker-title">{{ t('translation.statusBarTasks') }}</span>
        <!-- 行 = 导航按钮 + 快速取消；button 不能嵌套，导航与取消拆成兄弟控件 -->
        <div v-for="job in jobs" :key="job.job_id" class="picker-item">
          <button role="menuitem" class="picker-item-main" @click="goToNovel(job.novel_id)">
            <span class="picker-dot" :class="`is-${job.state}`" aria-hidden="true"></span>
            <span class="picker-body">
              <span class="picker-name">{{ jobTitle(job) }}</span>
              <span class="picker-meta">{{ pickerMeta(job) }}</span>
            </span>
          </button>
          <button
            v-if="!isTerminalJobState(job.state)"
            class="picker-cancel"
            :aria-label="cancelLabel(job)"
            :title="t('translation.statusBarQuickCancel')"
            :disabled="pendingCancel === job.job_id"
            @click="quickCancel(job)"
          >
            <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18" /><path d="m6 6 12 12" /></svg>
          </button>
        </div>
        <span v-if="cancelError" role="status" class="picker-cancel-error">{{ cancelError }}</span>
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
/* 行容器只做布局；导航与取消是兄弟 button（button 不能嵌套），悬停底色跟导航区走 */
.picker-item { display:flex; align-items:stretch; width:100%; min-height:48px; border-radius:var(--radius-control); }
.picker-item-main { flex:1; min-width:0; display:flex; align-items:flex-start; gap:var(--space-sm); padding:var(--space-sm); color:var(--ink); text-align:left; }
.picker-item-main:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
.picker-item-main:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:-2px; }
/* 快速取消：32px 命中区，悬停用 error 语义色提示破坏性动作，不新增颜色字面值 */
.picker-cancel { flex:none; align-self:center; width:calc(var(--space-lg) * 2); height:calc(var(--space-lg) * 2); margin:0 var(--space-xxs) 0 0; display:flex; align-items:center; justify-content:center; color:var(--ink-muted); }
.picker-cancel:hover { background:color-mix(in srgb,var(--md-sys-color-error) 10%,transparent); color:var(--md-sys-color-error); }
.picker-cancel:focus-visible { outline:2px solid var(--md-sys-color-error); outline-offset:-2px; }
.picker-cancel:disabled { opacity:0.5; cursor:default; }
.picker-cancel:disabled:hover { background:transparent; color:var(--ink-muted); }
.picker-cancel-error { padding:0 var(--space-sm) var(--space-xxs); color:var(--md-sys-color-error); font-size:12px; line-height:1.4; overflow:hidden; text-overflow:ellipsis; }
/* 单任务底栏的直达取消：与浮窗同语义，图标 16px 居中 */
.status-cancel { flex:none; width:calc(var(--space-lg) * 2); display:flex; align-items:center; justify-content:center; color:var(--ink-muted); }
.status-cancel:hover { background:color-mix(in srgb,var(--md-sys-color-error) 10%,transparent); color:var(--md-sys-color-error); }
.status-cancel:focus-visible { outline:2px solid var(--md-sys-color-error); outline-offset:-2px; }
.status-cancel:disabled { opacity:0.5; cursor:default; }
.bar-cancel-error { flex:none; max-width:30%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--md-sys-color-error); }
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
