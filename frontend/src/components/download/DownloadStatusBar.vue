<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { taskFailedCount, taskProgress, useTaskStore } from "../../stores/tasks";
import { useDownloadPanelStore } from "../../stores/downloadPanel";
import { useTranslationStore, type TranslationActive } from "../../stores/translation";
const tasks = useTaskStore(); const panel = useDownloadPanelStore(); const translation = useTranslationStore();
const router = useRouter(); const route = useRoute(); const { t } = useI18n();
function openTasks() { if (route.path !== '/tools/tasks') void router.push('/tools/tasks'); }

/**
 * 翻译后台指示（ADR 0031）：单个任务保持原样——整条文案直接回到该小说；
 * 多个任务时显示「后台 N 个翻译任务正在进行」，点击在底栏上方浮出任务选择菜单。
 */
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
/** 阶段文案与浮窗进度区同源；选择菜单里只作一行辅助信息。 */
function phaseText(task: TranslationActive): string {
  if (task.phase === "queued") return t("translation.progressQueued");
  if (task.phase === "prepare") return t("translation.progressPrepare");
  return t("translation.progressTranslate");
}
/** 已用时间：分′秒″（1′42″），秒补零；时钟回拨不出现负值（与浮窗进度区同格式）。 */
function elapsedText(task: TranslationActive): string {
  const total = Math.max(0, Math.floor(task.elapsed / 1000));
  return `${Math.floor(total / 60)}′${String(total % 60).padStart(2, "0")}″`;
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
watch(() => translation.active.length, (count) => {
  if (count === 0) pickerOpen.value = false;
});
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
    <!-- 单任务：与既有样式完全一致，整条文案即返回入口 -->
    <button v-if="translation.active.length === 1" class="status-translation" :aria-label="t('translation.statusBarBackTo') + '：' + t('translation.statusBarTranslating', { title: translation.active[0].title, page: translation.active[0].page })" :title="t('translation.statusBarTranslating', { title: translation.active[0].title, page: translation.active[0].page })" @click="goToNovel(translation.active[0].novelId)">
      <span class="translation-full">{{ t('translation.statusBarTranslating', { title: translation.active[0].title, page: translation.active[0].page }) }}</span>
      <span class="translation-short">{{ t('translation.statusBarTranslatingShort') }}</span>
    </button>
    <!-- 多任务：计数入口 + 上方任务选择菜单（同一浮层配方：surface-container / control 圆角 / 唯一合法轻阴影） -->
    <div v-else-if="translation.active.length > 1" class="translation-menu">
      <button ref="trigger" class="status-translation status-translation-count" :aria-label="t('translation.statusBarTranslatingCount', { count: translation.active.length })" :title="t('translation.statusBarTranslatingCount', { count: translation.active.length })" aria-haspopup="menu" :aria-expanded="pickerOpen" @click="togglePicker">
        <span class="translation-full">{{ t('translation.statusBarTranslatingCount', { count: translation.active.length }) }}</span>
        <span class="translation-short">{{ t('translation.statusBarTranslatingCountShort', { count: translation.active.length }) }}</span>
      </button>
      <div v-if="pickerOpen" class="translation-backdrop" @click="closePicker"></div>
      <div v-if="pickerOpen" ref="picker" class="translation-picker" role="menu" :aria-label="t('translation.statusBarTasks')" tabindex="-1" @keydown.esc="closePicker">
        <span class="picker-title">{{ t('translation.statusBarTasks') }}</span>
        <button v-for="task in translation.active" :key="task.id" role="menuitem" class="picker-item" @click="goToNovel(task.novelId)">
          <span class="picker-dot" aria-hidden="true"></span>
          <span class="picker-body">
            <span class="picker-name">{{ task.title }}</span>
            <span class="picker-meta">{{ t('translation.statusBarTaskMeta', { page: task.page, phase: phaseText(task), time: elapsedText(task) }) }}</span>
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
.status-link { flex:1; min-width:0; display:flex; align-items:center; gap:var(--space-md); text-align:left; }
.status-translation { flex:0 1 auto; min-width:0; display:flex; align-items:center; color:var(--md-sys-color-primary); padding:0 var(--space-sm); }
/* 计数文案是固定短句，不参与收缩：flex:none 保证整句完整显示，由左侧下载区让位
   （百分比 max-width 在宽度由内容决定的 flex 容器上会互相牵制，把文案压成省略号） */
.translation-menu { position:relative; display:flex; flex:none; height:100%; }
.status-translation-count { flex:none; }
.translation-full { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.translation-short { display:none; white-space:nowrap; }
/* 透明遮罩只负责「点击外部关闭」，不做变暗 scrim（AccountMenu / 翻译浮窗同配方） */
.translation-backdrop { position:fixed; inset:0; z-index:890; }
/* 任务选择菜单：锚定计数入口上方、右对齐向左展开，与既有浮层同宽同配方 */
.translation-picker { position:absolute; right:0; bottom:calc(100% + var(--space-sm)); z-index:900; display:flex; flex-direction:column; gap:var(--space-xxs); box-sizing:border-box; width:calc(var(--space-lg) * 24); max-width:calc(100vw - var(--space-lg) * 2); max-height:min(60vh,320px); overflow-y:auto; padding:var(--space-sm); border-radius:var(--radius-control); background:var(--md-sys-color-surface-container); color:var(--ink); box-shadow:0 4px 12px rgb(0 0 0 / 18%); outline:none; transform-origin:bottom right; }
.picker-title { padding:0 var(--space-sm); color:var(--ink-muted); font-size:12px; font-weight:600; line-height:32px; }
.picker-item { display:flex; align-items:flex-start; gap:var(--space-sm); width:100%; min-height:48px; padding:var(--space-sm); color:var(--ink); font:inherit; text-align:left; background:transparent; border:0; border-radius:var(--radius-control); cursor:pointer; }
.picker-item:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
.picker-item:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:-2px; }
/* 进行中状态点：与翻译浮窗入口点同色（primary），不新增语义色 */
.picker-dot { flex:none; width:8px; height:8px; margin-top:6px; border-radius:50%; background:var(--md-sys-color-primary); }
.picker-body { display:flex; flex-direction:column; gap:2px; min-width:0; }
.picker-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:13px; line-height:1.4; }
.picker-meta { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--ink-muted); font-size:12px; line-height:1.4; }
.mini-progress { width:calc(var(--space-xl) * 5); flex:none; --md-linear-progress-track-height:4px; }
.task-summary { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.new-download { flex:none; color:var(--md-sys-color-primary); padding:0 var(--space-sm); }
@media (max-width:640px) { .task-summary { display:none; } .mini-progress { width:calc(var(--space-xl) * 2); } .download-status,.status-link { gap:var(--space-sm); } .translation-full { display:none; } .translation-short { display:inline; } }
</style>
