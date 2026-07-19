import { createApp } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import router from "./router";
import App from "./App.vue";
import zhCN from "./locales/zh-CN";
import enUS from "./locales/en-US";

// 从 localStorage 读取上次选择（前端独立持久化，不依赖后端 settings）
const savedLang = localStorage.getItem("pixiv-tool-lang") || "zh-CN";

const i18n = createI18n({
  legacy: false, // Composition API 模式
  locale: savedLang,
  fallbackLocale: "zh-CN",
  messages: {
    "zh-CN": zhCN,
    "en-US": enUS,
  },
});

const app = createApp(App);
app.use(createPinia());
app.use(router);
app.use(i18n);
app.mount("#app");
