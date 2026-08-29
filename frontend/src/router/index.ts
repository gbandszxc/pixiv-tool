import { createRouter, createWebHashHistory } from "vue-router";

const router = createRouter({
  // Tauri 静态资源（tauri:// 或 asset 协议）下 hash 路由无需服务端回退，最稳。
  history: createWebHashHistory(),
  routes: [
    {
      path: "/pixiv",
      name: "pixiv",
      component: () => import("../views/PixivView.vue"),
    },
    {
      path: "/",
      name: "crawl",
      component: () => import("../views/CrawlView.vue"),
    },
    {
      path: "/illustration",
      name: "illustration",
      component: () => import("../views/IllustrationView.vue"),
    },
    {
      path: "/tasks",
      name: "tasks",
      component: () => import("../views/TasksView.vue"),
    },
    {
      path: "/history",
      name: "history",
      component: () => import("../views/HistoryView.vue"),
    },
    {
      path: "/settings",
      name: "settings",
      component: () => import("../views/SettingsView.vue"),
    },
  ],
});

export default router;
