import { defineStore } from "pinia";
import { ref } from "vue";
import { isTauri, invoke } from "../api/tauri";
import type { Settings } from "../api/tauri";
import { mockSettingsGet, mockSettingsSave } from "../api/devMock";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export type { Settings } from "../api/tauri";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings>({
    output_dir: "",  // 实际值由后端 settings_get 提供（系统下载目录/pixiv-tool）
    output_formats: ["txt", "markdown"],
    language: "zh-CN",
    theme: "auto",
    theme_color: "pixiv",
    startup_page: "/browse/home",
    backend_port: null,
    max_wait_seconds: 180,
    // 以下四项的取值约束见 useThumbTier；默认值与后端 Settings::default 一致
    show_r18: true,
    thumb_quality_grid: "medium",
    thumb_quality_detail: "medium",
    thumb_quality_fullscreen: "large",
    // 小说正文字号缩放（区间 0.75~2.0），默认 1.0
    novel_font_scale: 1.0,
    // 阅读背景色语义键（空串=跟随主题），默认未设置
    novel_bg_color: "",
    // SauceNAO API Key（以图识图必需），默认未配置
    saucenao_api_key: "",
  });

  async function fetchSettings() {
    // 非 Tauri（浏览器视觉验收）：内存版样例配置，见 api/devMock.ts。
    if (!isTauri()) {
      settings.value = mockSettingsGet();
      return;
    }
    settings.value = await invoke<Settings>("settings_get");
  }

  async function saveSettings(newSettings: Partial<Settings>) {
    if (!isTauri()) {
      mockSettingsSave(newSettings);
      Object.assign(settings.value, newSettings);
      return;
    }
    // 校验失败时后端 reject string，由视图层用 errorMessage() 展示。
    await invoke("settings_save", { settings: newSettings });
    Object.assign(settings.value, newSettings);
  }

  async function clearLogs() {
    await invoke("clear_logs");
  }

  async function selectDirectory(): Promise<string | null> {
    // 目录选择走系统对话框插件，不经过后端命令；取消时返回 null。
    const selected = await openDialog({ directory: true });
    return typeof selected === "string" ? selected : null;
  }

  return { settings, fetchSettings, saveSettings, clearLogs, selectDirectory };
});
