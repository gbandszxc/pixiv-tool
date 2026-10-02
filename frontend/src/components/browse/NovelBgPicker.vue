<template>
  <div class="bg-picker">
    <!-- 触发器：圆形色块回显当前选中纸色（默认 = surface 底 + outline 描边） -->
    <md-icon-button
      ref="trigger"
      :aria-label="t('browse.novel.bgLabel')"
      :title="t('browse.novel.bgLabel')"
      aria-haspopup="true"
      :aria-expanded="open"
      @click="toggle()"
    >
      <span
        class="bg-dot"
        :class="{ plain: !currentBg }"
        :style="currentBg ? { background: currentBg } : undefined"
        aria-hidden="true"
      ></span>
    </md-icon-button>
    <!-- 透明遮罩只负责「点击外部关闭」，不做变暗 scrim（与账号菜单同 recipe） -->
    <div v-if="open" class="bg-backdrop" @click="close()"></div>
    <!-- 外层锚点恒挂载、只做「按钮上方水平居中」定位（空置时无内容无背景，不影响布局）；
         内层面板承担过渡 transform，两者互不冲突，Transition 直接作用于面板本身 -->
    <div class="bg-anchor">
      <Transition name="bg-pop">
        <div
          v-if="open"
          ref="panel"
          class="bg-popover"
          role="radiogroup"
          :aria-label="t('browse.novel.bgLabel')"
          tabindex="-1"
        >
          <button
            v-for="paper in PAPERS"
            :key="paper.key || 'default'"
            type="button"
            role="radio"
            class="bg-swatch"
            :class="{ plain: !paper.bg, selected: selectedKey === paper.key }"
            :style="paper.bg ? { background: paper.bg } : undefined"
            :aria-checked="selectedKey === paper.key"
            :aria-label="t(paper.labelKey)"
            :title="t(paper.labelKey)"
            @click="choose(paper.key)"
          ></button>
        </div>
      </Transition>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 小说阅读背景色选择器（阅读器底栏「文字大小」控件右侧）：
 * 触发器为 md-icon-button 内嵌 18px 圆形色块（回显当前纸色），点击在其上方弹出
 * 水平居中的一排气泡（6 枚 28px 圆形色块：默认 / 护眼绿 / 牛皮纸 / 暖杏 / 雾蓝 / 藕粉），
 * 点击色块立即 emit 更新（持久化由视图层经 settings_save 写入）并关闭；
 * 透明遮罩点击外部关闭、Esc 同效（原生 dialog 打开时让位）、焦点移入弹出层、
 * 关闭后回到触发器，弹出过渡 0.15s ease（reduced-motion 由全局兜底）。
 * 纸色配对与派生规则见 DESIGN.md「小说阅读背景色板」。
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ (e: "update:modelValue", value: string): void }>();
const { t } = useI18n();

/** 候选纸色（语义键 → 纸面 bg；空串 = 跟随主题，色块走 surface 底 + outline 描边）。 */
interface Paper {
  key: string;
  labelKey: string;
  bg: string;
}

/** 可聚焦宿主元素的最小接口（md-icon-button 自定义元素，仅用于关闭后回焦）。 */
interface Focusable {
  focus(): void;
}

const PAPERS: Paper[] = [
  { key: "", labelKey: "browse.novel.bgDefault", bg: "" },
  { key: "green", labelKey: "browse.novel.bgGreen", bg: "#c8e6ce" },
  { key: "kraft", labelKey: "browse.novel.bgKraft", bg: "#e6d7b8" },
  { key: "warm", labelKey: "browse.novel.bgWarm", bg: "#f3e4d0" },
  { key: "mist", labelKey: "browse.novel.bgMist", bg: "#e1ebf2" },
  { key: "blush", labelKey: "browse.novel.bgBlush", bg: "#f5e7e5" },
];

const open = ref(false);
const panel = ref<HTMLElement | null>(null);
// 注意：不要把函数签名内联进 ref 的泛型实参（如 ref<{ focus(): void } | null>）——
// 本仓 vue-tsc（jsx: preserve）会把泛型实参误读成值上下文对象字面量而报 TS1005，
// 函数签名一律提为顶层 interface（PAPERS 色板同文件顶部）。
const trigger = ref<Focusable | null>(null);

/** 当前值归一到语义键（容错未知值按默认处理），回显其纸面色。 */
const selectedKey = computed(() => {
  const key = props.modelValue || "";
  return PAPERS.some((p) => p.key === key) ? key : "";
});
const currentBg = computed(() => PAPERS.find((p) => p.key === selectedKey.value)?.bg ?? "");

function toggle(): void {
  open.value = !open.value;
}
function close(): void {
  open.value = false;
}
/** 选择即应用：立刻 emit + 关闭（保存失败由视图层 notify，不打断阅读）。 */
function choose(key: string): void {
  emit("update:modelValue", key);
  close();
}

/** Esc 关弹出层；登录/退出确认等原生 dialog 打开时让位（Esc 优先关闭最上层 dialog）。 */
function onWindowKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape" && !document.querySelector("dialog[open]")) close();
}

watch(open, (isOpen) => {
  if (isOpen) {
    window.addEventListener("keydown", onWindowKeydown);
    void nextTick(() => panel.value?.focus());
  } else {
    window.removeEventListener("keydown", onWindowKeydown);
    trigger.value?.focus();
  }
});
onBeforeUnmount(() => window.removeEventListener("keydown", onWindowKeydown));
</script>

<style scoped>
.bg-picker {
  position: relative;
  display: inline-flex;
}

/* 触发器内 18px 圆形色块；默认态（跟随主题）用 surface 底 + outline 描边保证在底栏上可见 */
.bg-dot {
  width: 18px;
  height: 18px;
  border-radius: 50%;
}

.bg-dot.plain {
  border: 1px solid var(--md-sys-color-outline);
}

/* 透明遮罩：fixed 铺满、无 scrim，仅承接「点击外部关闭」；层级在底栏（z 10）之上 */
.bg-backdrop {
  position: fixed;
  inset: 0;
  z-index: 890;
}

/* 外层锚点只做居中定位（left 50% + translateX(-50%)），自身无背景无阴影 */
.bg-anchor {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 50%;
  transform: translateX(-50%);
  z-index: 900;
}

/* 气泡面板：一排色块、surface-container 底、既定轻阴影（唯一合法值） */
.bg-popover {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-sm);
  background: var(--md-sys-color-surface-container);
  border-radius: var(--radius-control);
  box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
  outline: none;
  transform-origin: bottom center;
}

/* 色块：28px 圆形、纸面色即底色；默认态 surface 底 + outline 描边；选中 primary 2px 外环 */
.bg-swatch {
  flex: none;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  cursor: pointer;
}

.bg-swatch.plain {
  background: var(--md-sys-color-surface);
  border: 1px solid var(--md-sys-color-outline);
}

.bg-swatch.selected {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.bg-swatch:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

/* 上浮过渡：时长复用既有 0.15s ease（账号菜单同源）；Transition 直接作用于面板，
 * 面板自身的 translateY 与外层锚点的居中 translateX 分离、互不覆盖；
 * reduced-motion 由全局兜底压到 0.01ms */
.bg-pop-enter-active,
.bg-pop-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.bg-pop-enter-from,
.bg-pop-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>
