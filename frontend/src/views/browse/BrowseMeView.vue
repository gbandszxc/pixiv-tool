<script setup lang="ts">
/**
 * 主页（/browse/me，「我的」分区的第三个一级入口）：
 * - uid 由 authStore.userId 响应式解析（登录 / 切号后自动生效，失效响应由 KeepAlive 的
 *   browseSession key 整树重挂保证）；
 * - 动作行「编辑资料 / 投稿插画作品」经官方 opener 打开 pixiv 网页（写操作仍在网页端完成，
 *   本应用维持只读 + 跳转的边界）；
 * - 作品 / 收藏 tab、排序、刷新、返回一律复用作者页组件（props 契约 `{ id: number }`），
 *   本页不自带返回键与标题，避免一页两个返回入口。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import BrowseNavigation from "../../components/navigation/BrowseNavigation.vue";
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
import BrowseAuthorView from "./BrowseAuthorView.vue";
import { OPEN_LOGIN_EVENT } from "../../api/browse";
import { useAuthStore } from "../../stores/auth";
import { notify } from "../../ui/notify";
import { openInBrowser } from "../../utils/pixivHooks";
import { pixivProfileEditUrl, pixivUploadUrl } from "../../utils/pixivUrl";

const { t } = useI18n();
const authStore = useAuthStore();
/** 当前账号 uid；未登录 / 非法值 → 0（作者页 id 必须为正整数，故 0 不渲染作者页）。 */
const meId = computed(() => Number(authStore.userId) || 0);
const authorRef = ref<{ refresh: () => void; loading: boolean } | null>(null);
const refreshing = computed(() => Boolean(authorRef.value?.loading));

function onRefresh(): void {
  authorRef.value?.refresh();
}
/** 未登录：复用全局登录弹窗（App.vue 监听 OPEN_LOGIN_EVENT）。 */
function openLogin(): void {
  window.dispatchEvent(new CustomEvent(OPEN_LOGIN_EVENT));
}

function openExternal(url: string): void {
  void openInBrowser(url).catch(() => notify(t("browse.hooks.openFailed")));
}
</script>

<template>
  <div class="page-view me-view">
    <div class="app-page-head">
      <!-- 无论登录与否，均保持全「我的」分区统一的大标题与基准线，切换绝无任何跳动 -->
      <div class="browse-list-header">
        <div class="page-heading">
          <PageBackButton />
          <h1 class="page-title">{{ t("workspace.library") }}</h1>
        </div>
        <ListRefreshButton v-if="meId !== 0" :busy="refreshing" @refresh="onRefresh" />
      </div>
      <BrowseNavigation />
    </div>
    <div v-if="meId === 0" class="me-state" role="status">
      <p class="state-text">{{ t("browseMe.loginRequired") }}</p>
      <md-filled-button @click="openLogin">{{ t("auth.login") }}</md-filled-button>
    </div>
    <template v-else>
      <BrowseAuthorView ref="authorRef" :id="meId" is-me>
        <template #me-actions>
          <md-outlined-button :title="t('browseMe.editProfile')" @click="openExternal(pixivProfileEditUrl())">
            <svg slot="icon" viewBox="0 0 24 24" aria-hidden="true" class="btn-icon">
              <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
              <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
            </svg>
            {{ t("browseMe.editProfile") }}
          </md-outlined-button>
          <md-outlined-button :title="t('browseMe.submitWork')" @click="openExternal(pixivUploadUrl())">
            <svg slot="icon" viewBox="0 0 24 24" aria-hidden="true" class="btn-icon">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="17 8 12 3 7 8" />
              <line x1="12" y1="3" x2="12" y2="15" />
            </svg>
            {{ t("browseMe.submitWork") }}
          </md-outlined-button>
        </template>
      </BrowseAuthorView>
    </template>
  </div>
</template>
<style scoped>
.browse-list-header {
  margin-bottom: var(--space-md);
}

.btn-icon {
  width: 16px;
  height: 16px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.me-state { display: flex; flex-direction: column; align-items: center; gap: var(--space-md); padding: var(--space-xl) 0; }
.state-text { margin: 0; color: var(--ink-muted); font-size: 14px; line-height: 1.5; }
</style>
