<template>
  <n-config-provider :theme="theme" :theme-overrides="themeOverrides">
    <n-message-provider>
    <n-layout has-sider class="app-shell">
      <n-layout-sider
        bordered
        v-model:collapsed="siderCollapsed"
        :collapsed-width="64"
        :width="180"
        show-trigger
        collapse-mode="width"
        @transitionend="handleSiderTransitionEnd"
      >
        <div class="sider-title">
          <img
            class="sider-logo"
            :class="{ 'is-hidden': titleTextVisible }"
            src="./assets/icon.png"
            alt=""
            aria-hidden="true"
          />
          <span
            class="sider-text"
            :class="{ 'is-visible': titleTextVisible }"
          >pixiv-tool</span>
        </div>
        <n-menu
          :collapsed-width="64"
          :collapsed-icon-size="22"
          :options="menuOptions"
          :value="route.path"
          :default-expanded-keys="['crawl']"
          @update:value="navigateTo"
        />
        <div class="sider-footer">
          <n-dropdown
            v-if="authStore.isLoggedIn"
            trigger="click"
            :options="accountMenuOptions"
            @select="handleAccountMenuSelect"
          >
            <button
              class="account-trigger"
              :class="{ 'is-collapsed': siderCollapsed }"
              type="button"
              :title="`user_id: ${authStore.userId}`"
              :aria-label="t('auth.accountMenu', { id: authStore.pixivId || authStore.name })"
            >
              <!-- NAvatar 语义：default slot 存在则不渲染 img，故有 URL 时
                   只给 #fallback（img 加载失败兜底），无 URL 时给 default 显示首字母。
                   条件写在两个 n-avatar 外层：条件 slot（<template #fallback v-if> + v-else）
                   会让 Vue 3.5.40 模板编译器崩溃（Codegen node is missing） -->
              <n-avatar
                v-if="authStore.avatarUrl"
                round
                :size="32"
                :src="authStore.avatarUrl"
              >
                <template #fallback>{{ accountInitial }}</template>
              </n-avatar>
              <n-avatar v-else round :size="32">{{ accountInitial }}</n-avatar>
              <span class="account-id">{{ authStore.pixivId || authStore.name }}</span>
            </button>
          </n-dropdown>
          <n-button v-else size="small" block @click="showLoginDialog = true">{{ t('auth.login') }}</n-button>
        </div>
      </n-layout-sider>
      <n-layout-content
        class="app-content"
        :class="{ 'is-pixiv-route': route.path === '/pixiv' }"
        :native-scrollbar="route.path !== '/pixiv'"
      >
        <router-view />
      </n-layout-content>
    </n-layout>

    <!-- 退出确认：Rust 拦截窗口关闭/Cmd+Q 后发事件，此处统一 Naive UI 确认。 -->
    <n-modal
      v-model:show="showExitConfirm"
      preset="dialog"
      :title="t('app.exitConfirmTitle')"
      :positive-text="t('app.exit')"
      :negative-text="t('common.cancel')"
      @positive-click="invoke('app_exit').catch(() => {})"
    />
    <!-- 登录弹窗：真实浏览器主路径 + 手动 Session 兜底。 -->
    <LoginDialog v-model:show="showLoginDialog" />
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NConfigProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NButton,
  NAvatar,
  NDropdown,
  NMessageProvider,
  NModal,
} from "naive-ui";
import type { GlobalThemeOverrides, MenuOption } from "naive-ui";
import { darkTheme } from "naive-ui";
import LoginDialog from "./components/auth/LoginDialog.vue";
import { useAuthStore } from "./stores/auth";
import { useSettingsStore } from "./stores/settings";
import { invoke, setWindowTheme } from "./api/tauri";
import SidebarIcon, { type SidebarIconName } from "./components/navigation/SidebarIcon.vue";
import { listen } from "@tauri-apps/api/event";

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const { t } = useI18n();
// 登录弹窗显隐：点“登录”打开，在浏览器登录 / 手动 Session 间选择。
const showLoginDialog = ref(false);

// 退出确认：红叉/Cmd+W/Cmd+Q 均被 Rust 拦下并 emit，此处弹框；
// 已打开时重复 emit 无副作用（v-model 幂等置 true）
const showExitConfirm = ref(false);
let unlistenExit: (() => void) | undefined;

// 侧栏折叠状态：折叠时顶部标题切换为项目图标。
const siderCollapsed = ref(false);

// 标题文字可见性：展开时等 max-width 过渡结束后再交叉淡化到文字，
// 避免宽度动画中“pixiv-tool”折行重排；收起时立即切回图标。
const titleTextVisible = ref(true);
let titleSwapTimer: ReturnType<typeof setTimeout> | undefined;

function handleSiderTransitionEnd(e: TransitionEvent) {
  if (e.propertyName !== "max-width" || siderCollapsed.value) return;
  titleTextVisible.value = true;
}

watch(siderCollapsed, (collapsed) => {
  clearTimeout(titleSwapTimer);
  if (collapsed) {
    titleTextVisible.value = false;
  } else {
    // transitionend 优先；超时兜底（过渡被禁用或中断时也能切回文字）
    titleSwapTimer = setTimeout(() => {
      if (!siderCollapsed.value) titleTextVisible.value = true;
    }, 350);
  }
});
watch(showLoginDialog, (visible) => {
  if (visible) {
    invoke("browse_hide").catch(() => {});
  } else if (route.path === "/pixiv") {
    invoke("browse_show").catch(() => {});
  }
});

const settingsStore = useSettingsStore();

// 系统深色偏好：auto 模式的数据源，监听系统实时切换
const systemDark = ref(window.matchMedia("(prefers-color-scheme: dark)").matches);
const media = window.matchMedia("(prefers-color-scheme: dark)");
const onMediaChange = (e: MediaQueryListEvent) => {
  systemDark.value = e.matches;
};
media.addEventListener("change", onMediaChange);
onBeforeUnmount(() => media.removeEventListener("change", onMediaChange));

const isDark = computed(
  () =>
    settingsStore.settings.theme === "dark" ||
    (settingsStore.settings.theme === "auto" && systemDark.value)
);

// 深色时挂 html.dark 驱动 CSS 变量切换（main.css）；
// 同时同步窗口原生主题——内嵌 pixiv webview 的 prefers-color-scheme
// 跟随窗口外观，使内嵌页与应用主题一致（而非跟随系统）。
// 但 settings 就绪前不动原生窗口：初始 isDark 基于 store 默认值（auto），
// 与持久化主题不符的错误 setTheme 会影响子 webview 首次加载的外观；
// 窗口初始外观已由 Rust setup 按持久化主题预设。
const windowThemeReady = ref(false);
watch(
  isDark,
  (dark) => {
    document.documentElement.classList.toggle("dark", dark);
    if (windowThemeReady.value) {
      setWindowTheme(dark ? "dark" : "light").catch(() => {});
    }
  },
  { immediate: true }
);

// NaiveUI 主题：dark 时用 darkTheme（themeOverrides 中主色等仍叠加生效）
const theme = computed(() => (isDark.value ? darkTheme : null));

const themeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#0096FA",
    primaryColorHover: "#0077D1",
    primaryColorPressed: "#005A9E",
    primaryColorSuppl: "#0096FA",
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
    borderRadius: "6px",
  },
};

const accountInitial = computed(() => (authStore.name || authStore.pixivId || "P").charAt(0).toUpperCase());

const accountMenuOptions = computed<MenuOption[]>(() => [
  { label: t("auth.logout"), key: "logout" },
]);

function renderNavigationIcon(name: SidebarIconName) {
  return () => h(SidebarIcon, { name });
}

// computed 让菜单文案随 locale 切换自动更新
const menuOptions = computed<MenuOption[]>(() => [
  { label: t("nav.pixiv"), key: "/pixiv", icon: renderNavigationIcon("pixiv") },
  {
    label: t("nav.crawl"),
    key: "crawl",
    icon: renderNavigationIcon("crawl"),
    children: [
      { label: t("nav.crawlNovel"), key: "/" },
      { label: t("nav.crawlIllustration"), key: "/illustration" },
    ],
  },
  { label: t("nav.tasks"), key: "/tasks", icon: renderNavigationIcon("tasks") },
  { label: t("nav.history"), key: "/history", icon: renderNavigationIcon("history") },
  { label: t("nav.settings"), key: "/settings", icon: renderNavigationIcon("settings") },
]);

function navigateTo(key: string) {
  router.push(key);
}

async function handleAccountMenuSelect(key: string) {
  if (key === "logout") {
    await authStore.logout();
  }
}

onMounted(async () => {
  // 启动即取设置：主题（light/dark/auto）需要立即生效。
  // 失败时保持 windowThemeReady=false：窗口维持 Rust setup 按
  // 持久化设置预设的外观，不用 store 默认值覆盖
  settingsStore
    .fetchSettings()
    .then(() => {
      windowThemeReady.value = true;
    })
    .catch(() => {});
  authStore.checkStatus();
  // 退出确认事件：Rust 拦截窗口关闭/Cmd+Q 后发来
  listen("app://confirm-exit", () => {
    showExitConfirm.value = true;
  }).then((fn) => {
    unlistenExit = fn;
  });
});

onBeforeUnmount(() => {
  unlistenExit?.();
});
</script>

<style scoped>
.app-shell {
  height: 100vh;
}

.sider-title {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 56px;
  padding: 0 16px;
  overflow: hidden;
  font-weight: bold;
  font-size: 16px;
  border-bottom: 1px solid var(--divider);
}

/* 折叠图标与标题文字绝对居中叠加：展开结束后交叉淡化（180ms，DESIGN.md 动效区间）。 */
.sider-logo,
.sider-text {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  transition: opacity 180ms ease-out;
}

.sider-logo {
  width: 22px;
  height: 22px;
}

.sider-logo.is-hidden {
  opacity: 0;
}

.sider-text {
  white-space: nowrap;
  opacity: 0;
}

.sider-text.is-visible {
  opacity: 1;
}

.sider-footer {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 12px;
  border-top: 1px solid var(--divider);
}

.account-trigger {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  gap: 8px;
  padding: 6px;
  color: var(--ink);
  font: inherit;
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: var(--radius-control);
  cursor: pointer;
  transition: background-color 180ms ease-out;
}

/* 头像不参与收缩，避免侧栏收窄时被压成椭圆 */
.account-trigger :deep(.n-avatar) {
  flex-shrink: 0;
}

/* 折叠态：隐藏 ID 文字、头像居中，触发器不再被横向压扁 */
.account-trigger.is-collapsed {
  justify-content: center;
  gap: 0;
}

.account-trigger.is-collapsed .account-id {
  display: none;
}

.account-trigger:hover {
  background: #e5f5ff;
}

.account-trigger:focus-visible {
  outline: 2px solid var(--pixiv-blue);
  outline-offset: 2px;
}

.account-id {
  overflow: hidden;
  flex: 1;
  color: var(--ink-strong);
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Pixiv 内嵌全屏浏览路由：零内边距、充满高度、禁用外层滚动 */
.app-content.is-pixiv-route {
  padding: 0 !important;
  height: 100%;
  overflow: hidden;
}

.app-content.is-pixiv-route :deep(.n-layout-scroll-container) {
  padding: 0 !important;
  height: 100% !important;
  overflow: hidden !important;
}

</style>
