<script setup lang="ts">
/**
 * 详情页元信息行·作品 pid（browse-work-pages-v1）。
 *
 * - 一行内联：12px/600 的「pid」标签 + 可点击的 id 数字（13px、tabular-nums）；
 * - 点击复制 pid：成功 notify 并把图标瞬时切为 lucide check（约 1.6s 复原），失败 notify；
 * - 用原生 <button>（非 md-icon-button）：40px 的组件高度会把紧凑信息行撑高；
 * - 图标为 lucide copy / check 官方路径（stroke 2），aria-label 与 title 同文案。
 */
import { onBeforeUnmount, ref } from "vue";
import { useI18n } from "vue-i18n";
import { notify } from "../../ui/notify";

const props = defineProps<{ id: number }>();

const { t } = useI18n();

/** 复制成功后短暂显示对勾；定时器在组件卸载时清掉。 */
const copied = ref(false);
let copiedTimer: ReturnType<typeof setTimeout> | undefined;

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(String(props.id));
    notify(t("browse.work.pidCopied", { id: props.id }));
    copied.value = true;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => { copied.value = false; }, 1600);
  } catch {
    notify(t("browse.work.copyFailed"));
  }
}

onBeforeUnmount(() => clearTimeout(copiedTimer));
</script>

<template>
  <span class="work-pid">
    <span class="pid-label">{{ t("browse.work.pidLabel") }}</span>
    <button
      type="button"
      class="pid-btn"
      :aria-label="t('browse.work.copyPid', { id })"
      :title="t('browse.work.copyPid', { id })"
      @click="copy"
    >
      <span class="pid-value">{{ id }}</span>
      <!-- lucide check -->
      <svg v-if="copied" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M20 6 9 17l-5-5" />
      </svg>
      <!-- lucide copy -->
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
        <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
      </svg>
    </button>
  </span>
</template>

<style scoped>
.work-pid {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xxs);
  min-width: 0;
}

.pid-label {
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink-muted);
}

/* 原生按钮：高度跟随文字行，不用 40px 的 md-icon-button 撑高信息行 */
.pid-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xxs);
  min-height: 20px;
  padding: 0 var(--space-xxs);
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--ink);
  font-size: 13px;
  line-height: 1.4;
  font-variant-numeric: tabular-nums;
  text-decoration: none;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.pid-btn:hover {
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.pid-btn:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.pid-btn svg {
  width: 14px;
  height: 14px;
  flex: none;
  color: var(--ink-muted);
}

@media (prefers-reduced-motion: reduce) {
  .pid-btn {
    transition: none;
  }
}
</style>
