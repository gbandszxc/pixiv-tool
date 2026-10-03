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
      ? [["/browse/bookmark", "nav.browseBookmark"], ["/browse/history", "workspace.browseHistory"]]
      : [["/browse/home", "workspace.recommended"], ["/browse/discover", "workspace.forYou"]];
  return entries.map(([value, label]) => ({ value, label: t(label) }));
});
const show = computed(() => group.value && group.value !== "downloads" && route.path.startsWith("/browse/") && route.path !== "/browse/search");
</script>

<template>
  <nav v-if="show" class="browse-navigation" :aria-label="t('workspace.sectionNavigation')">
    <div class="channel-links"><router-link v-for="tab in tabs" :key="tab.value" :to="tab.value" :aria-current="route.path === tab.value ? 'page' : undefined">{{ tab.label }}</router-link></div>
    <div v-if="group === 'discover'" class="channel-links channels">
      <router-link v-for="entry in [['illustration', 'nav.browseIllustration'], ['manga', 'nav.browseManga'], ['novel', 'nav.browseNovel'], ['ranking', 'nav.browseRanking']]" :key="entry[0]" :to="`/browse/${entry[0]}`" :aria-current="route.path === `/browse/${entry[0]}` ? 'page' : undefined">{{ t(entry[1]) }}</router-link>
    </div>
  </nav>
</template>

<style scoped>
.browse-navigation { display:flex; flex-wrap:wrap; align-items:center; gap:var(--space-sm) var(--space-lg); margin:0 0 var(--space-xl); }
.channels { border-left:1px solid color-mix(in srgb,var(--md-sys-color-outline) 30%,transparent); padding-left:var(--space-lg); }
.channel-links { display:flex; flex-wrap:wrap; gap:var(--space-sm); }
a { padding:var(--space-sm) var(--space-md); border-radius:var(--radius-navigation, 24px); color:var(--ink-muted); text-decoration:none; }
a:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
a[aria-current] { background:var(--md-sys-color-primary-container); color:var(--md-sys-color-on-primary-container); }
a:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:2px; }
</style>
