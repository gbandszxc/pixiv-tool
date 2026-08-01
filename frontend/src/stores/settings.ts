import { defineStore } from "pinia";
import { ref } from "vue";
import api from "../api";

export interface Settings {
  output_dir: string;
  output_formats: string[];
  language: string;
  theme: string;
  backend_port: number | null;
}

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings>({
    output_dir: "downloads",
    output_formats: ["txt", "markdown"],
    language: "zh-CN",
    theme: "auto",
    backend_port: null,
  });

  async function fetchSettings() {
    const resp = await api.get("/api/settings");
    settings.value = resp.data;
  }

  async function saveSettings(newSettings: Partial<Settings>) {
    await api.put("/api/settings", newSettings);
    Object.assign(settings.value, newSettings);
  }

  async function clearLogs() {
    await api.post("/api/settings/clear-logs");
  }

  async function selectDirectory(): Promise<string | null> {
    const resp = await api.post("/api/settings/select-directory");
    return resp.data?.path ?? null;
  }

  return { settings, fetchSettings, saveSettings, clearLogs, selectDirectory };
});
