import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "../api/tauri";
import type {
  AccountEntry,
  AuthAccountsResponse,
  AuthLoginResponse,
  AuthLoginManualResponse,
  AuthStatusResponse,
} from "../api/tauri";

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
  /** 已保存的多账号列表（后端账号索引快照）。 */
  const accounts = ref<AccountEntry[]>([]);
  /** 当前激活账号的 user_id（未登录时可能为空）。 */
  const activeAccountId = ref("");
  /** 账号切换进行中（触发器转圈，防重复点击）。 */
  const isSwitching = ref(false);

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
        await fetchAccounts();
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
    await fetchAccounts();
  }

  /** 拉取已保存账号列表（登录 / 登出 / 切换后调用以刷新下拉）。 */
  async function fetchAccounts() {
    try {
      const data = await invoke<AuthAccountsResponse>("auth_accounts_list");
      accounts.value = data.accounts ?? [];
      activeAccountId.value = data.active ?? "";
    } catch {
      // 索引读取失败不阻塞主流程：保留旧列表，下次操作再试
    }
  }

  /**
   * 切换当前账号。后端会归档当前登录态、激活目标账号并同步内嵌 webview；
   * 成功后本地刷新登录态与账号列表。失败抛 reject string（中文文案）。
   */
  async function switchAccount(targetUserId: string) {
    if (isSwitching.value) {
      return;
    }
    isSwitching.value = true;
    try {
      await invoke("auth_account_switch", { userId: targetUserId });
      await checkStatus();
      await fetchAccounts();
    } finally {
      isSwitching.value = false;
    }
  }

  async function logout() {
    await invoke("auth_logout");
    clearAuth();
    // 当前账号已从后端索引移除，刷新列表（其余账号保留）
    await fetchAccounts();
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
    accounts,
    activeAccountId,
    isSwitching,
    checkStatus,
    login,
    loginWithCookie,
    fetchAccounts,
    switchAccount,
    logout,
    clearAuth,
  };
});
