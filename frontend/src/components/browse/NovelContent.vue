<script setup lang="ts">
/**
 * 小说正文渲染器（browse-ui-v1 / F4 核心）。
 *
 * 把 pixiv 原始标记文本渲染为只读正文（Assumption 3：内嵌图 V1 显示占位块）：
 * - 先按 [newpage] 切页，只渲染当前页（props.page，1 起），页数经 pages-change 通知父级；
 * - 整行 [chapter:X] → 章节标题；整行 [pixivimage:]/[uploadedimage:] → 占位块；
 * - 整行 [jump:]/[jumpurl:] → 忽略；段落内残留的 jump/内嵌图标记按纯文本剔除（URL 不渲染为链接）；
 * - 行内 [rb:A>B]（pixiv 注音，容忍旧格式 / 分隔）→ <ruby>A<rt>B</rt></ruby>。
 *
 * 安全约定：用户内容全部经 segments 数组按元素渲染，不使用 v-html。
 */
import { computed, watch } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  /** 小说全文（保留原始标记） */
  content: string;
  /** 当前页码（1 起） */
  page: number;
}>();

const emit = defineEmits<{ (e: "pages-change", total: number): void }>();

const { t } = useI18n();

// ===== 标记解析 =====

/** 行内注音：[rb:显示文字>ルビ]；pixiv 用 > 分隔，容忍 mock/旧格式的 /。 */
const RB_INLINE_RE = /\[rb:([^>\]/]+)[>/]([^\]]+)\]/g;
/** 整行章节标题：[chapter:X]。 */
const CHAPTER_LINE_RE = /^\s*\[chapter:(.*?)\]\s*$/;
/** 整行内嵌图：[pixivimage:12345] / [pixivimage:12345-2] / [uploadedimage:xxx]。 */
const IMAGE_LINE_RE = /^\s*\[(?:pixivimage|uploadedimage):[^\]]*\]\s*$/;
/** 整行跳转标记：[jump:N] / [jumpurl:...]（渲染时忽略）。 */
const JUMP_LINE_RE = /^\s*\[(?:jump|jumpurl):[^\]]*\]\s*$/;
/** 段落内剔除：跳转与内嵌图标记不产生任何可见内容。 */
const STRIP_INLINE_RE = /\[(?:jump|jumpurl|pixivimage|uploadedimage):[^\]]*\]/g;

type Segment = { kind: "text"; text: string } | { kind: "ruby"; base: string; ruby: string };

type Block =
  | { kind: "chapter"; text: string }
  | { kind: "image" }
  | { kind: "para"; segments: Segment[] };

/** 行内拆 segments：普通文本与注音交替，供模板按元素渲染（替代 v-html）。 */
function parseInline(line: string): Segment[] {
  const segments: Segment[] = [];
  let last = 0;
  for (const match of line.matchAll(RB_INLINE_RE)) {
    const start = match.index ?? 0;
    if (start > last) segments.push({ kind: "text", text: line.slice(last, start) });
    segments.push({ kind: "ruby", base: match[1], ruby: match[2] });
    last = start + match[0].length;
  }
  if (last < line.length) segments.push({ kind: "text", text: line.slice(last) });
  return segments;
}

/** 单页文本 → 块列表。空行只产生段间距（CSS），不产生空段落。 */
function parsePage(pageText: string): Block[] {
  const blocks: Block[] = [];
  for (const rawLine of pageText.split("\n")) {
    const line = rawLine.trim();
    if (!line) continue;
    const chapter = line.match(CHAPTER_LINE_RE);
    if (chapter) {
      const title = chapter[1].trim();
      if (title) blocks.push({ kind: "chapter", text: title });
      continue;
    }
    if (IMAGE_LINE_RE.test(line)) {
      blocks.push({ kind: "image" });
      continue;
    }
    if (JUMP_LINE_RE.test(line)) continue;
    const cleaned = line.replace(STRIP_INLINE_RE, "").trim();
    if (!cleaned) continue;
    blocks.push({ kind: "para", segments: parseInline(cleaned) });
  }
  return blocks;
}

/** 全文按 [newpage] 切页（至少 1 页）；\r 统一为 \n。 */
const pages = computed<string[]>(() =>
  props.content
    .replace(/\r\n?/g, "\n")
    .split(/\[newpage\]/)
    .map((pageText) => pageText.trim())
);

watch(
  () => pages.value.length,
  (total) => emit("pages-change", total),
  { immediate: true }
);

const blocks = computed<Block[]>(() => parsePage(pages.value[props.page - 1] ?? ""));
/** 非末页时，页尾显示分页线提示（当前页之后还有内容）。 */
const hasMorePages = computed(() => props.page < pages.value.length);
</script>

<template>
  <div class="novel-content">
    <template v-for="(block, i) in blocks" :key="i">
      <h3 v-if="block.kind === 'chapter'" class="chapter">{{ block.text }}</h3>
      <div
        v-else-if="block.kind === 'image'"
        class="image-placeholder"
        role="img"
        :aria-label="t('browse.novel.imagePlaceholder')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="3.5" y="5" width="17" height="14" rx="2" />
          <circle cx="9" cy="10" r="1.5" />
          <path d="m6 16.5 3.5-3.5 3 3 2.5-2.5 3 3" />
        </svg>
        <span>{{ t("browse.novel.imagePlaceholder") }}</span>
      </div>
      <p v-else class="para">
        <template v-for="(seg, j) in block.segments" :key="j">
          <ruby v-if="seg.kind === 'ruby'">{{ seg.base }}<rt>{{ seg.ruby }}</rt></ruby>
          <template v-else>{{ seg.text }}</template>
        </template>
      </p>
    </template>
    <div v-if="hasMorePages" class="page-divider" aria-hidden="true">· · ·</div>
  </div>
</template>

<style scoped>
.novel-content {
  color: var(--ink);
  font-size: 14px;
  line-height: 1.8;
  overflow-wrap: anywhere;
}

.para {
  margin: 0 0 1em;
}

.chapter {
  margin: 1.6em 0 0.8em;
  color: var(--ink);
  font-size: 16px;
  font-weight: 700;
  line-height: 1.5;
}

.chapter:first-child {
  margin-top: 0;
}

.image-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-xs);
  min-height: 96px;
  margin: 0 0 1em;
  padding: var(--space-lg);
  border: 1.5px dashed color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  border-radius: var(--radius-control);
  color: var(--ink-muted);
  font-size: 12px;
  text-align: center;
}

.image-placeholder svg {
  width: 24px;
  height: 24px;
  stroke-width: 1.8;
}

rt {
  color: var(--ink-muted);
  font-size: 0.55em;
}

.page-divider {
  margin: 1.5em 0 0.5em;
  color: var(--ink-subtle);
  text-align: center;
  user-select: none;
}
</style>
