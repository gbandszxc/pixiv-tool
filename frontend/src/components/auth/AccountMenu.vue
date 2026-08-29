<template>
  <n-dropdown
    :show="showMenu"
    trigger="click"
    placement="top-start"
    :options="menuOptions"
    @update:show="onUpdateShow"
    @select="handleSelect"
  >
    <button
      class="account-trigger"
      :class="{ 'is-collapsed': collapsed }"
      type="button"
      :title="triggerTitle"
      :aria-label="triggerAria"
    >
      <!-- 切换进行中：头像位转圈，避免重复点击 -->
      <n-spin v-if="authStore.isSwitching" :size="26" class="account-spin" />
      <!-- NAvatar 语义：default slot 存在则不渲染 img，故有 URL 时
           只给 #fallback（img 加载失败兜底），无 URL 时给 default 显示首字母。
           条件写在两个 n-avatar 外层：条件 slot 会让 Vue 3.5.40 模板编译器崩溃 -->
      <n-avatar v-else-if="triggerAvatarUrl" round :size="32" :src="triggerAvatarUrl">
        <template #fallback>{{ triggerInitial }}</template>
      </n-avatar>
      <n-avatar v-else round :size="32">{{ triggerInitial }}</n-avatar>
      <span class="account-id">{{ triggerText }}</span>
    </button>
  </n-dropdown>
</template>

<script setup lang="ts">
import { computed, h, ref } from "vue";
import { useI18n } from "vue-i18n";
import { NAvatar, NDropdown, NSpin, useMessage, type MenuOption } from "naive-ui";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, type AccountEntry } from "../../api/tauri";

defineProps<{ collapsed: boolean }>();

const emit = defineEmits<{
  /** 下拉开合：打开时 App.vue 需先藏内嵌子 webview（原生层浮于 DOM 之上） */
  (e: "visible", v: boolean): void;
  (e: "add-account"): void;
}>();

const { t } = useI18n();
const message = useMessage();
const authStore = useAuthStore();
const showMenu = ref(false);

function onUpdateShow(v: boolean) {
  showMenu.value = v;
  emit("visible", v);
}

// ===== 触发器（当前账号 / 未登录但存在已存账号的占位） =====

const triggerAvatarUrl = computed(() =>
  authStore.isLoggedIn ? authStore.avatarUrl : ""
);

const triggerInitial = computed(() =>
  authStore.isLoggedIn
    ? (authStore.name || authStore.pixivId || "P").charAt(0).toUpperCase()
    : "P"
);

const triggerText = computed(() =>
  authStore.isLoggedIn
    ? authStore.pixivId || authStore.name
    : t("auth.accounts")
);

const triggerTitle = computed(() =>
  authStore.isLoggedIn ? `user_id: ${authStore.userId}` : t("auth.switchAccount")
);

const triggerAria = computed(() =>
  authStore.isLoggedIn
    ? t("auth.accountMenu", { id: authStore.pixivId || authStore.name })
    : t("auth.switchAccount")
);

// ===== 下拉选项：账号列表 + 添加账号 + 退出登录 =====

function displayNameOf(a: AccountEntry): string {
  return a.pixiv_id || a.name || a.user_id;
}

function renderCheckIcon() {
  return h(
    "svg",
    {
      class: "paccount-check",
      viewBox: "0 0 24 24",
      fill: "none",
      stroke: "currentColor",
      "stroke-width": 2.4,
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
      "aria-hidden": "true",
    },
    [h("polyline", { points: "20 6 9 17 4 12" })]
  );
}

function renderAccountRow(a: AccountEntry) {
  const isActive = authStore.isLoggedIn && a.user_id === authStore.activeAccountId;
  const initial = (a.name || a.pixiv_id || "P").charAt(0).toUpperCase();
  // 同 NAvatar 语义：有 URL 给 fallback 兜底，无 URL 用 default 画首字母
  const avatar = a.avatar_url
    ? h(NAvatar, { round: true, size: 28, src: a.avatar_url }, { fallback: () => initial })
    : h(NAvatar, { round: true, size: 28 }, { default: () => initial });
  return h("div", { class: `paccount-row${isActive ? " is-active" : ""}` }, [
    avatar,
    h("div", { class: "paccount-meta" }, [
      h("div", { class: "paccount-name" }, displayNameOf(a)),
      h("div", { class: "paccount-sub" }, `ID: ${a.user_id}`),
    ]),
    isActive ? renderCheckIcon() : null,
  ]);
}

const menuOptions = computed<MenuOption[]>(() => {
  const options: MenuOption[] = authStore.accounts.map((a) => ({
    key: `account:${a.user_id}`,
    label: () => renderAccountRow(a),
  }));
  if (options.length > 0) {
    options.push({ type: "divider", key: "paccount-divider" });
  }
  options.push({ label: t("auth.addAccount"), key: "add-account" });
  options.push({
    label: t("auth.logout"),
    key: "logout",
    disabled: !authStore.isLoggedIn,
  });
  return options;
});

async function handleSelect(key: string) {
  if (key.startsWith("account:")) {
    const target = key.slice("account:".length);
    if (authStore.isLoggedIn && target === authStore.activeAccountId) {
      return;
    }
    try {
      await authStore.switchAccount(target);
      const account = authStore.accounts.find((a) => a.user_id === target);
      message.success(
        t("auth.switchSuccess", { name: account ? displayNameOf(account) : target })
      );
    } catch (err: unknown) {
      message.error(errorMessage(err) || t("auth.switchFailed"));
    }
  } else if (key === "add-account") {
    showMenu.value = false;
    emit("add-account");
  } else if (key === "logout") {
    showMenu.value = false;
    await authStore.logout();
  }
}
</script>

<style scoped>
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

/* 主色同色系的悬停反馈：浅色固定浅蓝底，深色按主色透明度叠色 */
.account-trigger:hover {
  background: #e5f5ff;
}

:global(html.dark) .account-trigger:hover {
  background: rgba(0, 150, 250, 0.18);
}

.account-trigger:focus-visible {
  outline: 2px solid var(--pixiv-blue);
  outline-offset: 2px;
}

/* 头像与转圈不参与收缩，避免侧栏收窄时被压成椭圆 */
.account-trigger :deep(.n-avatar),
.account-spin {
  flex-shrink: 0;
}

/* 折叠态：隐藏文字、头像居中，触发器不再被横向压扁 */
.account-trigger.is-collapsed {
  justify-content: center;
  gap: 0;
}

.account-trigger.is-collapsed .account-id {
  display: none;
}

.account-id {
  overflow: hidden;
  flex: 1;
  color: var(--ink-strong);
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>

<!--
  下拉选项行样式：n-dropdown 菜单 teleport 到 body，scoped 属性覆盖不到
  render 函数产出的行内容，故用全局样式并以 paccount- 前缀防冲突。
  颜色走 main.css 的中性变量（html.dark 自动反转），回退值同 DESIGN.md。
-->
<style>
.paccount-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 2px 0;
}

.paccount-meta {
  min-width: 0;
  flex: 1;
}

.paccount-name {
  overflow: hidden;
  color: var(--ink-strong, #444444);
  font-size: 14px;
  font-weight: 500;
  line-height: 1.4;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.paccount-sub {
  overflow: hidden;
  color: var(--ink-muted, #777777);
  font-size: 12px;
  line-height: 1.4;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.paccount-check {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  color: var(--pixiv-blue, #0096fa);
}
</style>
