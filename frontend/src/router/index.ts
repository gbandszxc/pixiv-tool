import { createRouter, createWebHashHistory } from "vue-router";
import type { RouteLocation, RouteRecordRaw } from "vue-router";

/** /browse/channel 系（插画/漫画/小说）三路由复用 BrowseChannelView，props 区分 kind。 */
const browseChannelRoutes: RouteRecordRaw[] = (
  ["illustration", "manga", "novel"] as const
).map((kind) => ({
  path: `/browse/${kind}`,
  name: `browse-${kind}`,
  component: () => import("../views/browse/BrowseChannelView.vue"),
  props: { kind },
}));

/**
 * /browse/work/:kind/:id 作品详情：kind 用正则约束，按 kind 落到不同 View
 * （illust/manga → BrowseWorkView，novel → BrowseNovelView 阅读器）。配置数组生成，避免重复。
 */
const browseWorkRoutes: RouteRecordRaw[] = [
  { kinds: ["illust", "manga"], component: () => import("../views/browse/BrowseWorkView.vue") },
  { kinds: ["novel"], component: () => import("../views/browse/BrowseNovelView.vue") },
].map(({ kinds, component }) => ({
  path: `/browse/work/:kind(${kinds.join("|")})/:id`,
  name: `browse-work-${kinds.join("-")}`,
  component,
  props: (route: RouteLocation) => ({
    kind: route.params.kind as "illust" | "manga" | "novel",
    id: Number(route.params.id),
  }),
}));

/** 旧路径 → /tools/* 的函数式 redirect：保留 query 与 hash（浏览页「返填表单」跳 / 或 /illustration 并带 query 预填，依赖此处透传）。 */
const legacyToolRedirect = (path: string) => (to: RouteLocation) => ({ path, query: to.query, hash: to.hash });

const router = createRouter({
  // Tauri 静态资源（tauri:// 或 asset 协议）下 hash 路由无需服务端回退，最稳。
  history: createWebHashHistory(),
  routes: [
    // ===== 工具页（页签壳 ToolsView，子路由 = 顶部页签）=====
    {
      path: "/tools",
      component: () => import("../views/ToolsView.vue"),
      children: [
        { path: "", redirect: "/tools/novel" },
        { path: "novel", name: "tools-novel", component: () => import("../views/CrawlView.vue") },
        { path: "illustration", name: "tools-illustration", component: () => import("../views/IllustrationView.vue") },
        { path: "tasks", name: "tools-tasks", component: () => import("../views/TasksView.vue") },
        { path: "history", name: "tools-history", component: () => import("../views/HistoryView.vue") },
      ],
    },
    // 旧抓取页路径保留为重定向；/settings 已移除（设置在账号抽屉的「设置」入口 → 模态设置弹窗）。
    { path: "/", redirect: legacyToolRedirect("/tools/novel") },
    { path: "/illustration", redirect: legacyToolRedirect("/tools/illustration") },
    { path: "/tasks", redirect: legacyToolRedirect("/tools/tasks") },
    { path: "/history", redirect: legacyToolRedirect("/tools/history") },
    // ===== 浏览模式（browse-ui-v1，F1-F5 替换占位实现）=====
    {
      path: "/browse/home",
      name: "browse-home",
      component: () => import("../views/browse/BrowseHomeView.vue"),
    },
    ...browseChannelRoutes,
    {
      path: "/browse/discover",
      name: "browse-discover",
      component: () => import("../views/browse/BrowseDiscoverView.vue"),
    },
    {
      path: "/browse/feed",
      name: "browse-feed",
      component: () => import("../views/browse/BrowseFeedView.vue"),
    },
    {
      path: "/browse/search",
      name: "browse-search",
      component: () => import("../views/browse/BrowseSearchView.vue"),
    },
    {
      path: "/browse/ranking",
      name: "browse-ranking",
      component: () => import("../views/browse/BrowseRankingView.vue"),
    },
    {
      path: "/browse/bookmark",
      name: "browse-bookmark",
      component: () => import("../views/browse/BrowseBookmarkView.vue"),
    },
    ...browseWorkRoutes,
    {
      path: "/browse/series/:id",
      name: "browse-series",
      component: () => import("../views/browse/BrowseSeriesView.vue"),
      props: (route: RouteLocation) => ({ id: Number(route.params.id) }),
    },
    {
      path: "/browse/user/:id",
      name: "browse-author",
      component: () => import("../views/browse/BrowseAuthorView.vue"),
      props: (route: RouteLocation) => ({ id: Number(route.params.id) }),
    },
  ],
});

export default router;
