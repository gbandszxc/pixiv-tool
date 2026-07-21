import axios from "axios";
import { useAuthStore } from "../stores/auth";

const api = axios.create({
  baseURL: "",  // dev 模式走 Vite proxy，prod 模式同源
  timeout: 15000,
});

// 响应拦截：401 触发登出
// 注意：本文件与 stores/auth.ts 存在循环 import（auth.ts 也 import 本文件的 api）。
// 用静态 import 是安全的：useAuthStore 只在拦截器回调（运行时）里调用，
// 而非模块顶层；此时两个模块都已完成求值，ESM live binding 保证拿到正确引用。
// 之所以放弃动态 import()：auth store 必须在主 bundle（每个页面都要看登录态），
// 动态拆分无意义，且会触发 Vite "dynamically imported but also statically imported" 警告。
// 详见 ticket V2-02。
api.interceptors.response.use(
  (resp) => resp,
  (err) => {
    if (err.response?.status === 401) {
      // 触发 store 清理
      useAuthStore().clearAuth();
    }
    return Promise.reject(err);
  }
);

export default api;
