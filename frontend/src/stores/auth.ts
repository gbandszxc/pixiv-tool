import { defineStore } from "pinia";
import { ref } from "vue";
import api from "../api";

export interface AuthState {
  isLoggedIn: boolean;
  userId: string;
  pixivId: string;
  name: string;
  profileImg: string;
}

export const useAuthStore = defineStore("auth", () => {
  const isLoggedIn = ref(false);
  const userId = ref("");
  const pixivId = ref("");
  const name = ref("");
  const profileImg = ref("");
  const isLoggingIn = ref(false);

  function applyStatus(data: AuthState) {
    isLoggedIn.value = data.isLoggedIn;
    userId.value = data.userId || "";
    pixivId.value = data.pixivId || "";
    name.value = data.name || "";
    profileImg.value = data.profileImg || "";
  }

  async function checkStatus() {
    try {
      const resp = await api.get("/api/auth/status");
      const data = resp.data;
      applyStatus({
        isLoggedIn: data.is_logged_in,
        userId: data.user_id || "",
        pixivId: data.pixiv_id || "",
        name: data.name || "",
        profileImg: data.profile_img || "",
      });
    } catch {
      isLoggedIn.value = false;
    }
  }

  async function login() {
    if (isLoggingIn.value) {
      return;
    }

    isLoggingIn.value = true;
    try {
      const response = await api.post("/api/auth/login");
      const user = response.data.user;
      if (response.data.status === "success" && user) {
        applyStatus({
          isLoggedIn: true,
          userId: user.user_id || "",
          pixivId: user.pixiv_id || "",
          name: user.name || "",
          profileImg: user.profile_img || "",
        });
      } else {
        await checkStatus();
      }
    } finally {
      isLoggingIn.value = false;
    }
  }

  /**
   * 手动导入 PHPSESSID 登录（绕开 pywebview 登录窗的验证码循环）。
   * 用真实浏览器登录后把 PHPSESSID 复制进来即可。
   * csrf_token 留空时后端自动用 PHPSESSID 抓 pixiv 首页补全,用户无需手填。
   * 抛错时由调用方(组件)展示错误信息,err.response.data.detail 含后端原因。
   */
  async function loginWithCookie(phpsessid: string, csrfToken = "") {
    // 用 form data 而非 query string,避免 PHPSESSID 出现在 URL/日志里。
    const form = new URLSearchParams();
    form.append("PHPSESSID", phpsessid.trim());
    if (csrfToken.trim()) {
      form.append("csrf_token", csrfToken.trim());
    }
    await api.post("/api/auth/login/manual", form, {
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      // 后端要请求 pixiv 首页补 csrf,给足时间。
      timeout: 30000,
    });
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
    profileImg.value = "";
  }

  return {
    isLoggedIn,
    userId,
    pixivId,
    name,
    profileImg,
    isLoggingIn,
    checkStatus,
    login,
    loginWithCookie,
    logout,
    clearAuth,
  };
});
