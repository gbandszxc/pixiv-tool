<template>
  <n-config-provider :theme="theme">
    <n-layout has-sider style="height: 100vh">
      <n-layout-sider
        bordered
        :collapsed-width="64"
        :width="180"
        show-trigger
        collapse-mode="width"
      >
        <div class="sider-title">pixiv-tool</div>
        <n-menu
          :collapsed-width="64"
          :collapsed-icon-size="22"
          :options="menuOptions"
          :value="route.path"
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
              type="button"
              :title="`user_id: ${authStore.userId}`"
              :aria-label="t('auth.accountMenu', { id: authStore.pixivId || authStore.name })"
            >
              <n-avatar
                round
                :size="32"
                :src="authStore.profileImg || undefined"
                :fallback-src="undefined"
              >
                {{ accountInitial }}
              </n-avatar>
              <span class="account-id">{{ authStore.pixivId || authStore.name }}</span>
              <span class="account-chevron" aria-hidden="true">⌄</span>
            </button>
          </n-dropdown>
          <n-button v-else size="small" block @click="handleLogin">{{ t('auth.notLoggedIn') }}</n-button>
        </div>
      </n-layout-sider>
      <n-layout-content style="padding: 24px">
        <router-view />
      </n-layout-content>
    </n-layout>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
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
} from "naive-ui";
import type { MenuOption } from "naive-ui";
import { useAuthStore } from "./stores/auth";

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const { t } = useI18n();

const theme = computed(() => null); // 浅色，ticket 15 实现完整主题

const accountInitial = computed(() => (authStore.name || authStore.pixivId || "P").charAt(0).toUpperCase());

const accountMenuOptions = computed<MenuOption[]>(() => [
  { label: t("auth.logout"), key: "logout" },
]);

// computed 让菜单文案随 locale 切换自动更新
const menuOptions = computed<MenuOption[]>(() => [
  { label: t("nav.crawl"), key: "/" },
  { label: t("nav.tasks"), key: "/tasks" },
  { label: t("nav.history"), key: "/history" },
  { label: t("nav.settings"), key: "/settings" },
]);

function navigateTo(key: string) {
  router.push(key);
}

async function handleLogin() {
  await authStore.login();
}

async function handleAccountMenuSelect(key: string) {
  if (key === "logout") {
    await authStore.logout();
  }
}

onMounted(() => {
  authStore.checkStatus();
});
</script>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}

.sider-title {
  padding: 16px;
  font-weight: bold;
  font-size: 16px;
  text-align: center;
  border-bottom: 1px solid var(--n-border-color, #e0e0e6);
}

.sider-footer {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 12px;
  border-top: 1px solid var(--n-border-color, #e0e0e6);
}

.account-trigger {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  gap: 8px;
  padding: 6px;
  color: #333;
  font: inherit;
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 180ms ease-out;
}

.account-trigger:hover {
  background: rgba(24, 160, 88, 0.1);
}

.account-trigger:focus-visible {
  outline: 2px solid #18a058;
  outline-offset: 2px;
}

.account-id {
  overflow: hidden;
  flex: 1;
  color: #444;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.account-chevron {
  color: #777;
  font-size: 16px;
}

@media (prefers-reduced-motion: reduce) {
  .account-trigger {
    transition: none;
  }
}
</style>
