import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "../api/tauri";
import type { Settings } from "../api/tauri";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export type { Settings } from "../api/tauri";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings>({
    output_dir: "",  // 实际值由后端 settings_get 提供（系统下载目录/pixiv-tool）
    output_formats: ["txt", "markdown"],
    language: "zh-CN",
    theme: "auto",
    backend_port: null,
    max_wait_seconds: 180,
  });

  async function fetchSettings() {
    settings.value = await invoke<Settings>("settings_get");
  }

  async function saveSettings(newSettings: Partial<Settings>) {
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
