<script setup lang="ts">
/**
 * 我的主页（/browse/me，「我的」分区的第三个一级入口）：
 * - uid 由 authStore.userId 响应式解析（登录 / 切号后自动生效，失效响应由 KeepAlive 的
 *   browseSession key 整树重挂保证）；
 * - 动作行「编辑资料 / 投稿插画作品」经官方 opener 打开 pixiv 网页（写操作仍在网页端完成，
 *   本应用维持只读 + 跳转的边界）；
 * - 作品 / 收藏 tab、排序、刷新、返回一律复用作者页组件（props 契约 `{ id: number }`），
 *   本页不自带返回键与标题，避免一页两个返回入口。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import BrowseNavigation from "../../components/navigation/BrowseNavigation.vue";
import PageBackButton from "../../components/navigation/PageBackButton.vue";
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
    <!-- 未登录：返回入口 + 标题 + 引导（对齐「空态保留返回入口」的既有规则） -->
    <div v-if="meId === 0" class="browse-list-header">
      <div class="page-heading"><PageBackButton /><h1 class="page-title">{{ t("nav.browseMe") }}</h1></div>
    </div>
    <BrowseNavigation />
    <div v-if="meId === 0" class="me-state" role="status">
      <p class="state-text">{{ t("browseMe.loginRequired") }}</p>
      <md-filled-button @click="openLogin">{{ t("auth.login") }}</md-filled-button>
    </div>
    <template v-else>
      <div class="me-actions">
        <md-outlined-button :title="t('browseMe.editProfile')" @click="openExternal(pixivProfileEditUrl())">{{ t("browseMe.editProfile") }}</md-outlined-button>
        <md-outlined-button :title="t('browseMe.submitWork')" @click="openExternal(pixivUploadUrl())">{{ t("browseMe.submitWork") }}</md-outlined-button>
      </div>
      <!-- 复用作者页：资料卡 / 四类 tab / 排序 / 刷新全部继承；本页不再叠返回键与标题 -->
      <BrowseAuthorView :id="meId" />
    </template>
  </div>
</template>

<style scoped>
.me-actions { display: flex; flex-wrap: wrap; gap: var(--space-sm); margin-bottom: var(--space-lg); }
.me-state { display: flex; flex-direction: column; align-items: center; gap: var(--space-md); padding: var(--space-xl) 0; }
.state-text { margin: 0; color: var(--ink-muted); font-size: 14px; line-height: 1.5; }
</style>
