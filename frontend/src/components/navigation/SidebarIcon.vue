<script setup lang="ts">
export type SidebarIconName =
  | "crawl"
  | "tasks"
  | "history"
  | "settings"
  | "home"
  | "image"
  | "manga"
  | "novel"
  | "discover"
  | "feed"
  | "search"
  | "ranking"
  | "bookmark";

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
    <template v-else-if="name === 'home'">
      <path d="m3 10.2 9-7 9 7" />
      <path d="M5.5 8.5V20a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V8.5" />
      <path d="M10 21v-6h4v6" />
    </template>
    <template v-else-if="name === 'image'">
      <rect x="4" y="5" width="16" height="14" rx="2" />
      <circle cx="9" cy="10" r="1.5" />
      <path d="m6.5 16.5 4-4 3 3 2.5-2.5 3 3" />
    </template>
    <template v-else-if="name === 'manga'">
      <rect x="4" y="4.5" width="16" height="15" rx="2" />
      <path d="M12 4.5v15" />
      <path d="M4 12.5h8" />
      <path d="M15 8.5h2.5" />
      <path d="M15 15.5h2.5" />
    </template>
    <template v-else-if="name === 'novel'">
      <path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v18H6.5A1.5 1.5 0 0 1 5 19.5z" />
      <path d="M5 19.5c0-.83.67-1.5 1.5-1.5H19" />
      <path d="M9 7.5h6" />
      <path d="M9 11h6" />
    </template>
    <template v-else-if="name === 'discover'">
      <circle cx="12" cy="12" r="8.5" />
      <path d="m15.2 8.8-1.7 4.7-4.7 1.7 1.7-4.7z" />
    </template>
    <template v-else-if="name === 'feed'">
      <path d="M5 11.5a7.5 7.5 0 0 1 7.5 7.5" />
      <path d="M5 5.5A13.5 13.5 0 0 1 18.5 19" />
      <circle cx="6.2" cy="17.8" r="1.3" />
    </template>
    <template v-else-if="name === 'search'">
      <circle cx="11" cy="11" r="6.5" />
      <path d="m20 20-4.4-4.4" />
    </template>
    <template v-else-if="name === 'ranking'">
      <path d="M9 20v-9h6v9" />
      <path d="M4 20v-5h5" />
      <path d="M15 20v-5h5v5" />
      <path d="M3 20h18" />
    </template>
    <template v-else-if="name === 'bookmark'">
      <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
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
