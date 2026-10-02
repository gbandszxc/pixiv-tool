<script setup lang="ts">
/**
 * 小说正文渲染器（browse-ui-v1 / F4 核心）。
 *
 * 把 pixiv 原始标记文本渲染为只读正文：
 * - 先按 [newpage] 切页，只渲染当前页（props.page，1 起），页数经 pages-change 通知父级；
 * - 整行 [chapter:X] → 章节标题；整行 [uploadedimage:id] → 图片（id 在 props.images 内时）；
 * - 段落内残留的 jump/内嵌图标记按纯文本剔除（URL 不渲染为链接）；
 * - [jumpuri:文字 > URL] → 只保留可见文字（不开外链）；
 * - 行内 [rb:A>B]（pixiv 注音，容忍旧格式 / 分隔）→ <ruby>A<rt>B</rt></ruby>。
 *
 * 内嵌图：`[uploadedimage:id]` 的 URL 由父级经详情响应的 embedded_images 传入
 * （后端取自同响应的 textEmbeddedImages，无额外请求）。取不到 URL 的标记
 * （未收录的 id / `[pixivimage:illustId]` 插图引用）渲染为占位块，不静默丢图。
 *
 * 安全约定：用户内容全部经 segments 数组按元素渲染，不使用 v-html。
 */
import { computed, watch } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc } from "../../api/browse";

const props = defineProps<{
  /** 小说全文（保留原始标记） */
  content: string;
  /** 当前页码（1 起） */
  page: number;
  /** 内嵌图 id → pximg URL（详情响应 embedded_images）；缺省视为全部无图 */
  images?: Record<string, string>;
}>();

const emit = defineEmits<{ (e: "pages-change", total: number): void }>();

const { t } = useI18n();

// ===== 标记解析 =====

/**
 * 行内标记一次扫描（顺序即优先级）：
 * 1. 注音 [rb:显示>ルビ]（pixiv 用 > 分隔，容忍 mock/旧格式的 /）；
 * 2. 内嵌图 [uploadedimage:id] / [pixivimage:illustId(-p)]；
 * 3. 站内跳转链接 [[jumpuri:文字 > URL]]（pixiv 实测是双层方括号）→ 只取「文字」；
 * 4. 其余跳转 [jump:N] / [jumpurl:...] → 整段丢弃。
 */
const INLINE_RE =
  /\[rb:([^>\]/]+)[>/]([^\]]+)\]|\[(?:pixivimage|uploadedimage):([^\]]*)\]|\[\[?jumpuri:([^>\]]*?)\s*>\s*[^\]]*\]\]?|\[(?:jump|jumpurl):[^\]]*\]/g;
/** 整行章节标题：[chapter:X]。 */
const CHAPTER_LINE_RE = /^\s*\[chapter:(.*?)\]\s*$/;
/** 整行跳转标记：[jump:N] / [jumpurl:...]（渲染时忽略）。 */
const JUMP_LINE_RE = /^\s*\[(?:jump|jumpurl):[^\]]*\]\s*$/;

type Segment =
  | { kind: "text"; text: string }
  | { kind: "ruby"; base: string; ruby: string }
  /** url 为空串 = 取不到图片地址（整行时由调用方渲染占位块）。 */
  | { kind: "image"; id: string; url: string };

type Block =
  | { kind: "chapter"; text: string }
  | { kind: "image"; id: string; url: string }
  | { kind: "para"; segments: Segment[] };

/** 内嵌图 id → 展示 URL；父级未给 images 时恒为空。 */
function imageUrl(id: string): string {
  return props.images?.[id] ?? "";
}

/** 行内拆 segments（普通文本 / 注音 / 内嵌图交替），供模板按元素渲染（替代 v-html）。 */
function parseInline(line: string): Segment[] {
  const segments: Segment[] = [];
  let last = 0;
  for (const match of line.matchAll(INLINE_RE)) {
    const start = match.index ?? 0;
    if (start > last) segments.push({ kind: "text", text: line.slice(last, start) });
    if (match[1] !== undefined) {
      segments.push({ kind: "ruby", base: match[1], ruby: match[2] });
    } else if (match[3] !== undefined) {
      // pixivimage 的 id 形如 `12345-2`（第 2 页），表键只认 uploadedimage 的裸 id
      segments.push({ kind: "image", id: match[3], url: imageUrl(match[3]) });
    } else if (match[4] !== undefined) {
      const label = match[4].trim();
      if (label) segments.push({ kind: "text", text: label });
    }
    last = start + match[0].length;
  }
  if (last < line.length) segments.push({ kind: "text", text: line.slice(last) });
  return segments.filter((seg) => seg.kind !== "text" || seg.text.length > 0);
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
    if (JUMP_LINE_RE.test(line)) continue;
    const segments = parseInline(line);
    if (!segments.length) continue;
    // 整行只有内嵌图 → 逐图成块（块级居中，与正文段落区隔）
    if (segments.every((seg) => seg.kind === "image")) {
      for (const seg of segments) {
        if (seg.kind === "image") blocks.push({ kind: "image", id: seg.id, url: seg.url });
      }
      continue;
    }
    // 混排段落：取不到 URL 的内嵌图不占位（避免打断行文）
    const kept = segments.filter((seg) => seg.kind !== "image" || seg.url);
    if (!kept.length) continue;
    blocks.push({ kind: "para", segments: kept });
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
      <!-- 内嵌图：有 URL 出图，无 URL（pixivimage / 未收录 id）出占位块 -->
      <figure v-else-if="block.kind === 'image' && block.url" class="image-figure">
        <img
          class="novel-image"
          :src="pxSrc(block.url)"
          :alt="t('browse.novel.imageAlt')"
          loading="lazy"
          decoding="async"
        />
      </figure>
      <div
        v-else-if="block.kind === 'image'"
        class="image-placeholder"
        role="img"
        :aria-label="t('browse.novel.imageUnsupported')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
          <circle cx="9" cy="9" r="2" />
          <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
        </svg>
        <span>{{ t("browse.novel.imageUnsupported") }}</span>
      </div>
      <p v-else class="para">
        <template v-for="(seg, j) in block.segments" :key="j">
          <ruby v-if="seg.kind === 'ruby'">{{ seg.base }}<rt>{{ seg.ruby }}</rt></ruby>
          <img
            v-else-if="seg.kind === 'image'"
            class="novel-image novel-image-inline"
            :src="pxSrc(seg.url)"
            :alt="t('browse.novel.imageAlt')"
            loading="lazy"
            decoding="async"
          />
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
  /* 基准 16px；阅读器在根节点注入 --novel-scale（缺省 1），字号缩放随设置生效 */
  font-size: calc(16px * var(--novel-scale, 1));
  line-height: 1.8;
  overflow-wrap: anywhere;
}

.para {
  margin: 0 0 1em;
}

.chapter {
  margin: 1.6em 0 0.8em;
  color: var(--ink);
  /* 1.15em ≈ 原 16px/14px 比例，随正文字号等比缩放 */
  font-size: 1.15em;
  font-weight: 700;
  line-height: 1.5;
}

.chapter:first-child {
  margin-top: 0;
}

/* 内嵌图：整行成块，居中、限宽于正文列，圆角与卡片同 recipe */
.image-figure {
  margin: 1.5em 0;
}

.novel-image {
  display: block;
  max-width: 100%;
  height: auto;
  border-radius: var(--radius-control);
}

.image-figure .novel-image {
  margin: 0 auto;
}

/* 混排在段落里的图：与文字同基线随行，不撑破行高 */
.novel-image-inline {
  display: inline-block;
  max-height: 1.6em;
  margin: 0 0.25em;
  vertical-align: middle;
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
  stroke-width: 2;
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
