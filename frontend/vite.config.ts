import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],

  // Tauri 约定：不吞掉 Rust 侧日志输出；允许读取 TAURI_ 前缀的环境变量。
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],

  server: {
    // 固定端口：Tauri 窗口的 devUrl 指向这里，被占用时直接失败而非换端口。
    port: 9961,
    strictPort: true,
  },
});
