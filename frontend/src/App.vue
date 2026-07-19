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
          <div v-if="authStore.isLoggedIn" class="user-info" :title="`user_id: ${authStore.userId}`">
            <span class="user-name">{{ authStore.name || authStore.pixivId }}</span>
          </div>
          <n-button v-else size="small" block @click="handleLogin">未登录，点此登录</n-button>
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
import {
  NConfigProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NButton,
} from "naive-ui";
import type { MenuOption } from "naive-ui";
import { useAuthStore } from "./stores/auth";

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();

const theme = computed(() => null); // 浅色，ticket 15 实现完整主题

const menuOptions: MenuOption[] = [
  { label: "抓取", key: "/" },
  { label: "任务", key: "/tasks" },
  { label: "历史", key: "/history" },
  { label: "设置", key: "/settings" },
];

function navigateTo(key: string) {
  router.push(key);
}

async function handleLogin() {
  await authStore.login();
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

.user-info {
  text-align: center;
  font-size: 13px;
  color: #666;
  cursor: default;
}

.user-name {
  font-weight: 500;
}
</style>
