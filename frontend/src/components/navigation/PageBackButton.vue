<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { goBack } from "../../router";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const visible = computed(() => route.path !== "/browse/home" || !!router.options.history.state.back);
const label = computed(() => t(route.path !== "/browse/home" && !router.options.history.state.back ? "nav.backHome" : "nav.back"));
</script>

<template>
  <md-icon-button v-if="visible" class="page-back" :aria-label="label" :title="label" @click="goBack">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
  </md-icon-button>
</template>

<style scoped>
.page-back { flex: none; color: var(--md-sys-color-on-surface-variant); }
svg { width: 20px; height: 20px; stroke-width: 2; }
</style>
