import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "../api/tauri";
import type { AuthLoginResponse, AuthLoginManualResponse, AuthStatusResponse } from "../api/tauri";

export interface AuthState {
  isLoggedIn: boolean;
  userId: string;
  pixivId: string;
  name: string;
  profileImg: string;
  /** pixiv-avatar:// 本地缓存 URL；空则头像回退首字母 */
  avatarUrl?: string;
}

/**
 * auth_login 返回非 success 时抛出：携带后端 status 与原始 message。
 * message 可能为空，展示侧需自行回退到本地化通用文案。
 */
export class LoginError extends Error {
  status: AuthLoginResponse["status"];

  constructor(status: AuthLoginResponse["status"], message?: string) {
    super(message);
    this.name = "LoginError";
    this.status = status;
  }
}

export const useAuthStore = defineStore("auth", () => {
  const isLoggedIn = ref(false);
  const userId = ref("");
  const pixivId = ref("");
  const name = ref("");
  const profileImg = ref("");
  const avatarUrl = ref("");
  const isLoggingIn = ref(false);

  function applyStatus(data: AuthState) {
    isLoggedIn.value = data.isLoggedIn;
    userId.value = data.userId || "";
    pixivId.value = data.pixivId || "";
    name.value = data.name || "";
    profileImg.value = data.profileImg || "";
    avatarUrl.value = data.avatarUrl || "";
  }

  async function checkStatus() {
    try {
      const data = await invoke<AuthStatusResponse>("auth_status");
      applyStatus({
        isLoggedIn: data.is_logged_in,
        userId: data.user_id || "",
        pixivId: data.pixiv_id || "",
        name: data.name || "",
        profileImg: data.profile_img || "",
        avatarUrl: data.avatar_url || "",
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
      // invoke 不设超时：真实浏览器登录是长阻塞，保持 loading 态直到返回。
      const response = await invoke<AuthLoginResponse>("auth_login");
      const user = response.user;
      if (response.status === "success" && user) {
        applyStatus({
          isLoggedIn: true,
          userId: user.user_id || "",
          pixivId: user.pixiv_id || "",
          name: user.name || "",
          profileImg: user.profile_img || "",
        });
        // auth_login 返回体不含头像缓存，补拉一次 auth_status（含代下与协议 URL）
        await checkStatus();
      } else {
        // 非 success（cancelled/timeout/error）：仍刷新一次本地登录态，
        // 再抛出携带后端信息的错误，由视图层展示具体原因。
        await checkStatus();
        throw new LoginError(response.status, response.message);
      }
    } finally {
      isLoggingIn.value = false;
    }
  }

  /** 手动导入 PHPSESSID，后端会验证会话并自动获取 csrf token。 */
  async function loginWithCookie(phpsessid: string) {
    // 失败时后端 reject string（校验类错误），由视图层用 errorMessage() 展示。
    const response = await invoke<AuthLoginManualResponse>("auth_login_manual", {
      phpsessid: phpsessid.trim(),
    });
    const user = response.user;
    applyStatus({
      isLoggedIn: true,
      userId: user?.user_id || "",
      pixivId: user?.pixiv_id || "",
      name: user?.name || "",
      profileImg: user?.profile_img || "",
    });
    // 同 login：补拉含 avatar_url 的完整状态
    await checkStatus();
  }

  async function logout() {
    await invoke("auth_logout");
    clearAuth();
  }

  function clearAuth() {
    isLoggedIn.value = false;
    userId.value = "";
    pixivId.value = "";
    name.value = "";
    profileImg.value = "";
    avatarUrl.value = "";
  }

  return {
    isLoggedIn,
    userId,
    pixivId,
    name,
    profileImg,
    avatarUrl,
    isLoggingIn,
    checkStatus,
    login,
    loginWithCookie,
    logout,
    clearAuth,
  };
});
