<script setup lang="ts">
/**
 * 评论区（browse-ui-v1 / F6）：展示作品评论 roots + 内联回复，并支持发表。
 *
 * - 分页自持：roots 用 offset 游标（browse_work_comments，接口无 total，不显示总数）；
 *   has_replies 条目展开后按 page 游标续拉回复（browse_comment_replies）。
 * - 发表：根评论与「回复某条根评论」（browse_comment_add，parent_id = 该评论 id）。
 *   成功后重拉首页 / 该评论回复首页，展示服务端权威数据（头像、时间、回复数）。
 *   回复入口只挂在根评论上（回复某条回复需嵌套 parent 语义，未验证，不做）。
 * - 正文纯文本渲染（white-space: pre-wrap，绝不 v-html，URL 保持纯文本）；
 *   表情评论（content 空 + stamp_url）渲染 stamp 图。
 * - 未登录错误由 api 层统一联动登录弹窗，此处只展示归一文案 + 重试。
 * - 评论区被作者关闭（roots 恒 400 → 后端 disabled 信封）为终态提示，非错误、无重试。
 */
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  COMMENT_MAX_CHARS,
  browseCommentAdd,
  browseCommentReplies,
  browseWorkComments,
  errorMessage,
  pxSrc,
  type BrowseComment,
  type ListWorkKind,
} from "../../api/browse";

const props = defineProps<{
  kind: ListWorkKind;
  id: number;
  /** 作品作者的用户 id（发表评论必传；省略/0 时隐藏发表入口） */
  authorId?: number;
}>();

const { t } = useI18n();

// ===== roots：评论列表（offset 游标）=====

const comments = ref<BrowseComment[]>([]);
/** 待拉取的 offset 游标；null = 已到底 */
const nextOffset = ref<number | null>(0);
const loading = ref(false);
const loadingMore = ref(false);
const error = ref("");
/** 评论区被作者关闭（disabled 信封）：终态，非错误 */
const closed = ref(false);
/** 首屏是否成功加载过：发表后重拉首页时不让发表区随 loading 卸载重建（避免闪烁与草稿区闪失） */
const loaded = ref(false);

/** 请求序号：快速切换作品时丢弃过期响应（回复请求沿用同一序号快照） */
let reqSeq = 0;

async function fetchPage(offset: number): Promise<void> {
  const seq = ++reqSeq;
  const first = offset === 0;
  if (first) {
    loading.value = true;
    error.value = "";
    comments.value = [];
  } else {
    loadingMore.value = true;
  }
  try {
    const data = await browseWorkComments({ kind: props.kind, id: props.id, offset });
    if (seq !== reqSeq) return;
    if (first) closed.value = data.disabled === true;
    loaded.value = true;
    comments.value = first ? data.comments : [...comments.value, ...data.comments];
    // 预建展开态，避免渲染期再写 reactive map
    ensureRepliesStates(comments.value);
    nextOffset.value = data.next ?? null;
  } catch (err) {
    if (seq !== reqSeq) return;
    error.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    if (seq === reqSeq) {
      loading.value = false;
      loadingMore.value = false;
    }
  }
}

/** 首屏失败重试（offset 0）或追加失败就地重试（nextOffset 未被失败推进）。 */
function retry(): void {
  void fetchPage(comments.value.length ? (nextOffset.value ?? 0) : 0);
}

function loadMoreRoots(): void {
  if (nextOffset.value == null || loading.value || loadingMore.value || error.value) return;
  void fetchPage(nextOffset.value);
}

// ===== replies：展开回复（page 游标，按评论 id 一份状态）=====

interface RepliesState {
  items: BrowseComment[];
  /** 下一页 page 游标；null = 到底 */
  next: number | null;
  loading: boolean;
  loadingMore: boolean;
  error: string;
}

const repliesMap = reactive<Record<string, RepliesState>>({});
/** 已展开的评论 id（V1 展开后不提供收起） */
const expandedIds = reactive(new Set<string>());

function state(commentId: string): RepliesState {
  return repliesMap[commentId] ?? (repliesMap[commentId] = { items: [], next: 1, loading: false, loadingMore: false, error: "" });
}

function ensureRepliesStates(list: BrowseComment[]): void {
  for (const c of list) {
    if (c.has_replies && !repliesMap[c.id]) state(c.id);
  }
}

async function loadRepliesPage(comment: BrowseComment, page: number): Promise<void> {
  const seq = reqSeq;
  const st = state(comment.id);
  if (page <= 1) st.loading = true;
  else st.loadingMore = true;
  st.error = "";
  try {
    const data = await browseCommentReplies({ kind: props.kind, commentId: comment.id, page });
    if (seq !== reqSeq) return;
    st.items = page <= 1 ? data.comments : [...st.items, ...data.comments];
    st.next = data.next ?? null;
  } catch (err) {
    if (seq !== reqSeq) return;
    st.error = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    if (seq === reqSeq) {
      st.loading = false;
      st.loadingMore = false;
    }
  }
}

function toggleReplies(comment: BrowseComment): void {
  if (expandedIds.has(comment.id)) return;
  expandedIds.add(comment.id);
  void loadRepliesPage(comment, 1);
}

/** 回复加载失败重试：首屏失败重拉第 1 页；追加失败重拉 next 指向的失败页。 */
function retryReplies(comment: BrowseComment): void {
  const st = repliesMap[comment.id];
  const page = st && st.items.length ? (st.next ?? 1) : 1;
  void loadRepliesPage(comment, page);
}

// ===== 头像加载失败占位 =====

const brokenAvatars = reactive(new Set<string>());

function onAvatarError(comment: BrowseComment): void {
  brokenAvatars.add(comment.id);
}

// ===== 发表 / 回复 =====

/** 根评论草稿与提交状态 */
const draft = ref("");
const posting = ref(false);
const postError = ref("");

/** 当前打开回复框的根评论 id（同时只开一个）；null = 未打开 */
const replyTarget = ref<string | null>(null);
const replyDraft = ref("");
const replyPosting = ref(false);
const replyError = ref("");

/** 作品作者 id（后端要求随请求回传；未知 = 0） */
const authorId = computed(() => props.authorId ?? 0);

/**
 * 显示发表入口：首屏已成功加载、无错误态、评论区未关闭、作者 id 已知。
 * 未登录不预判——发表时由 api 层弹登录窗（与浏览命令一致）。
 */
const showComposer = computed(
  () => loaded.value && !error.value && !closed.value && authorId.value > 0
);

const canSubmitRoot = computed(() => draft.value.trim().length > 0);
const canSubmitReply = computed(() => replyDraft.value.trim().length > 0);

/** 发表根评论：成功后重拉首页（评论按时间倒序，新评论出现在顶部）。 */
async function submitRoot(): Promise<void> {
  if (!canSubmitRoot.value || posting.value) return;
  posting.value = true;
  postError.value = "";
  try {
    await browseCommentAdd({
      kind: props.kind,
      id: props.id,
      authorId: authorId.value,
      comment: draft.value,
    });
    draft.value = "";
    await fetchPage(0);
  } catch (err) {
    postError.value = errorMessage(err) || t("browse.comments.postFailed");
  } finally {
    posting.value = false;
  }
}

function startReply(comment: BrowseComment): void {
  replyTarget.value = comment.id;
  replyDraft.value = "";
  replyError.value = "";
}

function cancelReply(): void {
  replyTarget.value = null;
  replyDraft.value = "";
  replyError.value = "";
}

/** 回复根评论：成功后展开该评论的回复并重拉第 1 页（新回复可见）。 */
async function submitReply(comment: BrowseComment): Promise<void> {
  if (!canSubmitReply.value || replyPosting.value) return;
  replyPosting.value = true;
  replyError.value = "";
  try {
    await browseCommentAdd({
      kind: props.kind,
      id: props.id,
      authorId: authorId.value,
      comment: replyDraft.value,
      parentId: comment.id,
    });
    cancelReply();
    comment.has_replies = true;
    expandedIds.add(comment.id);
    await loadRepliesPage(comment, 1);
  } catch (err) {
    replyError.value = errorMessage(err) || t("browse.comments.postFailed");
  } finally {
    replyPosting.value = false;
  }
}

// ===== kind / id 变化：整体重置并重拉首屏 =====

function resetAll(): void {
  reqSeq += 1; // 在途请求全部过期
  comments.value = [];
  nextOffset.value = 0;
  loading.value = false;
  loadingMore.value = false;
  error.value = "";
  closed.value = false;
  loaded.value = false;
  draft.value = "";
  postError.value = "";
  posting.value = false;
  replyPosting.value = false;
  cancelReply();
  for (const key of Object.keys(repliesMap)) delete repliesMap[key];
  expandedIds.clear();
  brokenAvatars.clear();
  void fetchPage(0);
}

watch(() => [props.kind, props.id] as const, resetAll, { immediate: true });
</script>

<template>
  <section class="comments">
    <h2 class="comments-title">{{ t("browse.comments.title") }}</h2>

    <!-- 发表根评论（评论区关闭 / 无作者 id / 首屏错误时不显示） -->
    <div v-if="showComposer" class="composer">
      <md-outlined-text-field
        class="composer-field"
        type="textarea"
        rows="2"
        :value="draft"
        :maxlength="COMMENT_MAX_CHARS"
        :placeholder="t('browse.comments.placeholder')"
        :aria-label="t('browse.comments.placeholder')"
        :disabled="posting"
        :error="Boolean(postError)"
        :error-text="postError"
        @input="draft = ($event.target as HTMLTextAreaElement).value"
      />
      <div class="composer-bar">
        <md-filled-button :disabled="!canSubmitRoot || posting" @click="submitRoot">
          {{ posting ? t("browse.comments.posting") : t("browse.comments.submit") }}
        </md-filled-button>
      </div>
    </div>

    <!-- 首屏骨架：3 条占位（纯色块，无动画） -->
    <div v-if="loading && !comments.length" class="comments-skeleton" aria-hidden="true">
      <div v-for="n in 3" :key="n" class="sk-row">
        <div class="sk-avatar"></div>
        <div class="sk-main">
          <div class="sk-line w30"></div>
          <div class="sk-line" :class="n % 2 === 0 ? 'w60' : 'w90'"></div>
        </div>
      </div>
    </div>

    <!-- 首屏失败：文案 + 重试 -->
    <div v-else-if="error && !comments.length" class="comments-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <!-- 评论区被作者关闭：终态提示（非错误，无重试） -->
    <p v-else-if="closed" class="comments-state empty">{{ t("browse.comments.closed") }}</p>

    <!-- 空态 -->
    <p v-else-if="!comments.length" class="comments-state empty">{{ t("browse.comments.empty") }}</p>

    <template v-else>
      <ul class="comment-list">
        <li v-for="c in comments" :key="c.id" class="comment">
          <div class="comment-row">
            <span class="avatar">
              <img
                v-if="c.profile_img && !brokenAvatars.has(c.id)"
                :src="pxSrc(c.profile_img)"
                alt=""
                loading="lazy"
                @error="onAvatarError(c)"
              />
              <span v-else class="avatar-fallback" aria-hidden="true">{{ (c.user_name || "?").slice(0, 1) }}</span>
            </span>
            <div class="comment-body">
              <div class="who">
                <span class="user-name">{{ c.user_name }}</span>
                <span v-if="c.date" class="date">{{ c.date }}</span>
              </div>
              <!-- 正文：纯文本（契约保证无 HTML）；表情评论渲染 stamp 图 -->
              <img v-if="c.stamp_url" class="stamp" :src="pxSrc(c.stamp_url)" :alt="t('browse.comments.stampAlt')" />
              <p v-else-if="c.content" class="content">{{ c.content }}</p>
            </div>
          </div>

          <!-- 发表回复：入口挂根评论（同时只开一个回复框） -->
          <div v-if="showComposer" class="comment-actions">
            <button type="button" class="reply-link" @click="startReply(c)">{{ t("browse.comments.reply") }}</button>
          </div>
          <div v-if="replyTarget === c.id" class="composer reply-composer">
            <md-outlined-text-field
              class="composer-field"
              type="textarea"
              rows="2"
              :value="replyDraft"
              :maxlength="COMMENT_MAX_CHARS"
              :placeholder="t('browse.comments.replyPlaceholder', { name: c.user_name })"
              :aria-label="t('browse.comments.replyPlaceholder', { name: c.user_name })"
              :disabled="replyPosting"
              :error="Boolean(replyError)"
              :error-text="replyError"
              @input="replyDraft = ($event.target as HTMLTextAreaElement).value"
            />
            <div class="composer-bar">
              <md-text-button :disabled="replyPosting" @click="cancelReply">{{ t("common.cancel") }}</md-text-button>
              <md-filled-button :disabled="!canSubmitReply || replyPosting" @click="submitReply(c)">
                {{ replyPosting ? t("browse.comments.posting") : t("browse.comments.replySubmit") }}
              </md-filled-button>
            </div>
          </div>

          <!-- 回复：展开内联（缩进 + 左侧 divider 竖线），page 游标续拉 -->
          <template v-if="c.has_replies">
            <md-text-button v-if="!expandedIds.has(c.id)" class="replies-toggle" @click="toggleReplies(c)">
              {{ t("browse.comments.viewReplies") }}
            </md-text-button>
            <div v-else class="replies">
              <div v-if="state(c.id).loading && !state(c.id).items.length" class="reply-skeleton" aria-hidden="true">
                <div v-for="n in 2" :key="n" class="sk-row">
                  <div class="sk-avatar small"></div>
                  <div class="sk-main">
                    <div class="sk-line w40"></div>
                    <div class="sk-line w80"></div>
                  </div>
                </div>
              </div>

              <div v-else-if="state(c.id).error && !state(c.id).items.length" class="reply-state" role="alert">
                <span class="state-text">{{ state(c.id).error }}</span>
                <md-text-button @click="retryReplies(c)">{{ t("common.retry") }}</md-text-button>
              </div>

              <template v-else>
                <ul class="reply-list">
                  <li v-for="r in state(c.id).items" :key="r.id" class="reply">
                    <div class="comment-row">
                      <span class="avatar small">
                        <img
                          v-if="r.profile_img && !brokenAvatars.has(r.id)"
                          :src="pxSrc(r.profile_img)"
                          alt=""
                          loading="lazy"
                          @error="onAvatarError(r)"
                        />
                        <span v-else class="avatar-fallback" aria-hidden="true">{{ (r.user_name || "?").slice(0, 1) }}</span>
                      </span>
                      <div class="comment-body">
                        <div class="who">
                          <span class="user-name small">{{ r.user_name }}</span>
                          <span v-if="r.reply_to_user_name" class="reply-to">
                            {{ t("browse.comments.replyTo", { name: r.reply_to_user_name }) }}
                          </span>
                          <span v-if="r.date" class="date">{{ r.date }}</span>
                        </div>
                        <img v-if="r.stamp_url" class="stamp" :src="pxSrc(r.stamp_url)" :alt="t('browse.comments.stampAlt')" />
                        <p v-else-if="r.content" class="content">{{ r.content }}</p>
                      </div>
                    </div>
                  </li>
                </ul>
                <div v-if="state(c.id).next != null" class="more-replies">
                  <md-text-button :disabled="state(c.id).loadingMore" @click="loadRepliesPage(c, state(c.id).next ?? 1)">
                    {{ t("browse.comments.moreReplies") }}
                  </md-text-button>
                </div>
              </template>
            </div>
          </template>
        </li>
      </ul>

      <!-- 追加页失败（已有内容）：就地提示 + 重试 -->
      <div v-if="error && comments.length" class="append-error" role="alert">
        <span class="append-error-text">{{ error }}</span>
        <md-text-button @click="retry">{{ t("common.retry") }}</md-text-button>
      </div>

      <!-- 底部：加载更多 / 到底收尾 -->
      <div v-if="!error" class="comments-foot">
        <md-outlined-button v-if="nextOffset != null" :disabled="loadingMore" @click="loadMoreRoots">
          {{ t("browse.comments.loadMore") }}
        </md-outlined-button>
        <p v-else class="comments-end">{{ t("browse.comments.noMore") }}</p>
      </div>
    </template>
  </section>
</template>

<style scoped>
.comments {
  min-width: 0;
}

.comments-title {
  margin: 0 0 var(--space-md);
  color: var(--ink);
  font-size: 16px;
  font-weight: 700;
  line-height: 1.4;
}

/* ===== 发表评论 / 回复 ===== */

.composer {
  display: flex;
  flex-direction: column;
  gap: var(--space-xs);
  min-width: 0;
  margin-bottom: var(--space-lg);
}

.composer-field {
  width: 100%;
}

.composer-bar {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-sm);
}

.comment-actions {
  display: flex;
  gap: var(--space-sm);
  margin-top: var(--space-xxs);
}

.reply-link {
  padding: 0;
  border: 0;
  background: none;
  color: var(--md-sys-color-primary);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  cursor: pointer;
}

.reply-link:hover {
  text-decoration: underline;
}

.reply-link:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.reply-composer {
  margin-top: var(--space-sm);
  margin-bottom: 0;
}

/* ===== 评论行 ===== */

.comment-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-lg);
  margin: 0;
  padding: 0;
  list-style: none;
}

.comment-row {
  display: flex;
  gap: var(--space-sm);
  min-width: 0;
}

.comment-body {
  flex: 1;
  min-width: 0;
}

.avatar {
  display: inline-flex;
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  border-radius: 999px;
  overflow: hidden;
  /* 主色弱色底占位（与作者卡头像同源派生） */
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface));
}

.avatar.small {
  width: 24px;
  height: 24px;
}

.avatar img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.avatar-fallback {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 14px;
  font-weight: 600;
}

.avatar.small .avatar-fallback {
  font-size: 11px;
}

.who {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: var(--space-xxs) var(--space-xs);
  min-width: 0;
}

.user-name {
  color: var(--ink);
  font-size: 13px;
  font-weight: 600;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.user-name.small {
  font-size: 12px;
}

.date {
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
}

.reply-to {
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
}

.content {
  margin: var(--space-xxs) 0 0;
  color: var(--ink);
  font-size: 14px;
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.stamp {
  display: block;
  max-width: 96px;
  max-height: 96px;
  margin-top: var(--space-xs);
  padding: var(--space-xxs);
  border-radius: 8px;
  background: var(--md-sys-color-surface-container);
}

/* ===== 回复区：缩进 24px + 左侧 2px divider 竖线 ===== */

.replies {
  display: flex;
  flex-direction: column;
  gap: var(--space-md);
  margin-top: var(--space-sm);
  margin-left: var(--space-xl);
  padding-left: var(--space-md);
  border-left: 2px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.reply-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-md);
  margin: 0;
  padding: 0;
  list-style: none;
}

.replies-toggle {
  margin-top: var(--space-xxs);
  /* 正文列起点 = 头像 36px + 间距 8px；抵消 md-text-button 水平内边距 → 标签与正文左对齐（同作者卡 bio-toggle 手法） */
  margin-left: 36px;
}

.more-replies {
  display: flex;
}

/* ===== 骨架 / 状态 ===== */

.comments-skeleton,
.reply-skeleton {
  display: flex;
  flex-direction: column;
  gap: var(--space-lg);
}

.sk-row {
  display: flex;
  gap: var(--space-sm);
}

.sk-avatar {
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.sk-avatar.small {
  width: 24px;
  height: 24px;
}

.sk-main {
  flex: 1;
  min-width: 0;
}

.sk-line {
  height: 12px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.sk-line + .sk-line {
  margin-top: var(--space-sm);
}

.sk-line.w30 { width: 30%; }
.sk-line.w40 { width: 40%; }
.sk-line.w60 { width: 60%; }
.sk-line.w80 { width: 80%; }
.sk-line.w90 { width: 90%; }

.comments-state {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--space-sm);
  padding: var(--space-md) 0;
}

.comments-state.empty {
  padding: var(--space-xl) 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  text-align: center;
}

.state-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.reply-state {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
}

.reply-state .state-text {
  font-size: 12px;
}

/* ===== 追加失败 / 底部收尾 ===== */

.append-error {
  display: flex;
  align-items: center;
  gap: var(--space-xs);
  margin-top: var(--space-md);
}

.append-error-text {
  color: var(--ink-muted);
  font-size: 13px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.comments-foot {
  display: flex;
  justify-content: center;
  margin-top: var(--space-lg);
}

.comments-end {
  margin: 0;
  color: var(--ink-subtle);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}
</style>
