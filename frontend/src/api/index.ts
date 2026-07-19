import axios from "axios";

const api = axios.create({
  baseURL: "",  // dev 模式走 Vite proxy，prod 模式同源
  timeout: 15000,
});

// 响应拦截：401 触发登出
api.interceptors.response.use(
  (resp) => resp,
  (err) => {
    if (err.response?.status === 401) {
      // 触发 store 清理
      import("../stores/auth").then(({ useAuthStore }) => {
        useAuthStore().clearAuth();
      });
    }
    return Promise.reject(err);
  }
);

export default api;
