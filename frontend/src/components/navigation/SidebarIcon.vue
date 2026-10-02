<script setup lang="ts">
/**
 * 侧栏图标：全部使用 lucide.dev 官方 path 数据（内联，零依赖，不引外部图标库）。
 */
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
  | "watchlist"
  | "search"
  | "ranking"
  | "bookmark"
  | "saucenao";

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
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <template v-if="name === 'crawl'">
      <!-- lucide book-open -->
      <path d="M12 5v16" />
      <path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z" />
    </template>
    <template v-else-if="name === 'tasks'">
      <!-- lucide clipboard-check -->
      <rect width="8" height="4" x="8" y="2" rx="1" ry="1" />
      <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
      <path d="m9 14 2 2 4-4" />
    </template>
    <template v-else-if="name === 'history'">
      <!-- lucide history -->
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
      <path d="M12 7v5l4 2" />
    </template>
    <template v-else-if="name === 'home'">
      <!-- lucide house -->
      <path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" />
      <path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </template>
    <template v-else-if="name === 'image'">
      <!-- lucide image -->
      <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
      <circle cx="9" cy="9" r="2" />
      <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
    </template>
    <template v-else-if="name === 'manga'">
      <!-- lucide columns-2 -->
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M12 3v18" />
    </template>
    <template v-else-if="name === 'novel'">
      <!-- lucide book -->
      <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20" />
    </template>
    <template v-else-if="name === 'discover'">
      <!-- lucide compass -->
      <circle cx="12" cy="12" r="10" />
      <path d="m16.24 7.76-1.804 5.411a2 2 0 0 1-1.265 1.265L7.76 16.24l1.804-5.411a2 2 0 0 1 1.265-1.265z" />
    </template>
    <template v-else-if="name === 'feed'">
      <!-- lucide rss -->
      <path d="M4 11a9 9 0 0 1 9 9" />
      <path d="M4 4a16 16 0 0 1 16 16" />
      <circle cx="5" cy="19" r="1" />
    </template>
    <template v-else-if="name === 'watchlist'">
      <!-- lucide bell -->
      <path d="M10.268 21a2 2 0 0 0 3.464 0" />
      <path d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326" />
    </template>
    <template v-else-if="name === 'search'">
      <!-- lucide search -->
      <path d="m21 21-4.34-4.34" />
      <circle cx="11" cy="11" r="8" />
    </template>
    <template v-else-if="name === 'saucenao'">
      <!-- lucide scan-search：以图识图（反搜图片找原图） -->
      <path d="M3 7V5a2 2 0 0 1 2-2h2" />
      <path d="M17 3h2a2 2 0 0 1 2 2v2" />
      <path d="M21 17v2a2 2 0 0 1-2 2h-2" />
      <path d="M7 21H5a2 2 0 0 1-2-2v-2" />
      <circle cx="12" cy="12" r="3" />
      <path d="m16 16-1.9-1.9" />
    </template>
    <template v-else-if="name === 'ranking'">
      <!-- lucide chart-no-axes-column -->
      <path d="M5 21v-6" />
      <path d="M12 21V3" />
      <path d="M19 21V9" />
    </template>
    <template v-else-if="name === 'bookmark'">
      <!-- lucide heart -->
      <path d="M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5" />
    </template>
    <template v-else>
      <!-- lucide settings（fallback） -->
      <path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" />
      <circle cx="12" cy="12" r="3" />
    </template>
  </svg>
</template>

<style scoped>
.sidebar-icon {
  display: block;
  width: 1em;
  height: 1em;
  stroke-width: 2;
}
</style>
