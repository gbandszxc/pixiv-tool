import { defineStore } from "pinia";
import { ref } from "vue";
import api from "../api";

export interface AuthState {
  isLoggedIn: boolean;
  userId: string;
  pixivId: string;
  name: string;
}

export const useAuthStore = defineStore("auth", () => {
  const isLoggedIn = ref(false);
  const userId = ref("");
  const pixivId = ref("");
  const name = ref("");

  async function checkStatus() {
    try {
      const resp = await api.get("/api/auth/status");
      const data = resp.data;
      isLoggedIn.value = data.is_logged_in;
      userId.value = data.user_id || "";
      pixivId.value = data.pixiv_id || "";
      name.value = data.name || "";
    } catch {
      isLoggedIn.value = false;
    }
  }

  async function login() {
    await api.post("/api/auth/login");
    await checkStatus();
  }

  async function logout() {
    await api.post("/api/auth/logout");
    clearAuth();
  }

  function clearAuth() {
    isLoggedIn.value = false;
    userId.value = "";
    pixivId.value = "";
    name.value = "";
  }

  return { isLoggedIn, userId, pixivId, name, checkStatus, login, logout, clearAuth };
});
