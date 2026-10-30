<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { navigationGroup } from "../../router/navigation";

const route = useRoute(); const { t } = useI18n();
const group = computed(() => navigationGroup(route.path));
const tabs = computed(() => {
  const entries = group.value === "following"
    ? [["/browse/feed", "workspace.newWorks"], ["/browse/watchlist", "nav.browseWatchlist"]]
    : group.value === "library"
      ? [["/browse/bookmark", "nav.browseBookmark"], ["/browse/history", "workspace.browseHistory"], ["/browse/me", "nav.browseMe"]]
      : [
          ["/browse/home", "workspace.recommended"],
          ["/browse/illustration", "nav.browseIllustration"],
          ["/browse/manga", "nav.browseManga"],
          ["/browse/novel", "nav.browseNovel"],
          ["/browse/ranking", "nav.browseRanking"],
        ];
  return entries.map(([value, label]) => ({ value, label: t(label) }));
});
const show = computed(() => group.value && group.value !== "downloads" && route.path.startsWith("/browse/") && route.path !== "/browse/search");
</script>

<template>
  <nav v-if="show" class="browse-navigation" :aria-label="t('workspace.sectionNavigation')">
    <div class="channel-links">
      <router-link
        v-for="tab in tabs"
        :key="tab.value"
        :to="tab.value"
        :aria-current="route.path === tab.value ? 'page' : undefined"
      >
        {{ tab.label }}
      </router-link>
    </div>
  </nav>
</template>

<style scoped>
.browse-navigation { display:flex; flex-wrap:wrap; align-items:center; gap:var(--space-sm) var(--space-lg); margin:0 0 var(--space-xl); }
.channel-links { display:flex; flex-wrap:wrap; gap:var(--space-sm); }
a { padding:var(--space-sm) var(--space-md); border-radius:24px; color:var(--ink-muted); text-decoration:none; }
a:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
a[aria-current] { background:var(--md-sys-color-primary-container); color:var(--md-sys-color-on-primary-container); }
a:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:2px; }
</style>
