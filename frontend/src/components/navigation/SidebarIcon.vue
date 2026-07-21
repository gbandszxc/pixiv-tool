<script setup lang="ts">
export type SidebarIconName = "crawl" | "tasks" | "history" | "settings";

defineProps<{
  name: SidebarIconName;
}>();
</script>

<template>
  <svg
    class="sidebar-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <template v-if="name === 'crawl'">
      <path d="M4 5.5c2.4-.9 4.9-.5 7 1.1v12c-2.1-1.6-4.6-2-7-1.1z" />
      <path d="M20 5.5c-2.4-.9-4.9-.5-7 1.1v12c2.1-1.6 4.6-2 7-1.1z" />
      <path d="M11 6.6h2" />
    </template>
    <template v-else-if="name === 'tasks'">
      <path d="M9 5h6" />
      <path d="M9 3h6v4H9z" />
      <path d="M7 5H5v16h14V5h-2" />
      <path d="m8 13 1.5 1.5L12 11" />
      <path d="M14 13h3" />
      <path d="m8 18 1.5 1.5L12 16" />
      <path d="M14 18h3" />
    </template>
    <template v-else-if="name === 'history'">
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7v5l3.5 2" />
      <path d="M6.4 5.6 4.8 4" />
    </template>
    <template v-else>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.1 2.1-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5v.2h-3v-.2a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1-2.1-2.1.1-.1A1.7 1.7 0 0 0 7 15a1.7 1.7 0 0 0-1.5-1H5.3v-3h.2A1.7 1.7 0 0 0 7 10a1.7 1.7 0 0 0-.3-1.9l-.1-.1 2.1-2.1.1.1a1.7 1.7 0 0 0 1.9.3 1.7 1.7 0 0 0 1-1.5v-.2h3v.2a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1 2.1 2.1-.1.1A1.7 1.7 0 0 0 19 10a1.7 1.7 0 0 0 1.5 1h.2v3h-.2a1.7 1.7 0 0 0-1.1 1z" />
    </template>
  </svg>
</template>

<style scoped>
.sidebar-icon {
  display: block;
  width: 1em;
  height: 1em;
  stroke-width: 1.8;
}
</style>
