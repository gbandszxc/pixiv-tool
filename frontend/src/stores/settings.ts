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

  return { settings, fetchSettings, saveSettings, clearLogs };
});
