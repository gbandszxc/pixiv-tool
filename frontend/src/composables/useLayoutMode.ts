/**
 * 竖屏旋转显示器「最大化」布局判定（P0 判定地基）。
 *
 * 判定对象始终是「逻辑像素」与 OS 最大化状态：
 *  - 进入：窗口最大化 && 逻辑宽高比 < 0.8 && 逻辑高度 ≥ 900
 *  - 退出（滞回，避免边界抖动）：比例 ≥ 0.88 或逻辑高度 < 820；非最大化作为硬门直接退出
 *    （回归要求：横屏 / 非最大化窗口必须保持现有观感）。
 *
 * 非 Tauri（浏览器 / dev 预览）用 window.innerWidth/innerHeight 作逻辑像素降级，并把
 * 「最大化」视为成立（浏览器无 OS 最大化概念），使按形状判定仍可用。import.meta.env.DEV
 * 下额外支持 window.__FORCE_LAYOUT__ 与 URL ?layout= 强制覆盖，供浏览器验收；Vite 生产
 * 构建把 DEV 视为常量 false，该覆盖分支被整体 tree-shake，不含于产物。
 *
 * 本 composable 只导出判定 ref；其余组件不 import 它，统一靠 .app-shell.is-portrait
 * 祖先 class 在自己的 scoped CSS 内响应（共享契约）。
 */
import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "../api/tauri";

const RATIO_ENTER = 0.8;
const RATIO_EXIT = 0.88;
const HEIGHT_ENTER = 900;
const HEIGHT_EXIT = 820;

type ForceLayout = "portrait" | "landscape";

declare global {
  interface Window {
    /** dev 验收钩子：强制竖屏 / 横屏；生产构建不含读取分支。 */
    __FORCE_LAYOUT__?: ForceLayout;
  }
}

interface Metrics {
  width: number;
  height: number;
  maximized: boolean;
}

/** dev 强制覆盖：window 变量优先，其次 URL ?layout=（兼容 hash 路由 #/...?layout=portrait）。 */
function readOverride(): ForceLayout | null {
  const forced = window.__FORCE_LAYOUT__;
  if (forced === "portrait" || forced === "landscape") return forced;
  for (const src of [window.location.search, window.location.hash]) {
    const q = src.indexOf("?");
    if (q < 0) continue;
    const value = new URLSearchParams(src.slice(q + 1)).get("layout");
    if (value === "portrait" || value === "landscape") return value;
  }
  return null;
}

export function useLayoutMode(): { isPortrait: Ref<boolean> } {
  const isPortrait = ref(false);
  let evalSeq = 0; // 单调递增：丢弃过期的异步判定结果
  let frame = 0;
  const unlisteners: UnlistenFn[] = [];
  let devCleanup: (() => void) | undefined;
  let disposed = false;

  /** 读取当前逻辑尺寸与最大化状态。 */
  async function readMetrics(): Promise<Metrics> {
    if (isTauri()) {
      try {
        const win = getCurrentWindow();
        const [maximized, size, factor] = await Promise.all([
          win.isMaximized(),
          win.innerSize(),
          win.scaleFactor(),
        ]);
        const logical = size.toLogical(factor);
        return { width: logical.width, height: logical.height, maximized };
      } catch {
        // IPC 不可用时退回 webview 逻辑像素；最大化未知视为否，避免误判。
        return { width: window.innerWidth, height: window.innerHeight, maximized: false };
      }
    }
    // 浏览器 / dev：webview 即逻辑像素；无 OS 最大化概念，视作目标窗口以便按形状判定。
    return { width: window.innerWidth, height: window.innerHeight, maximized: true };
  }

  /** 带滞回的状态机；非最大化是硬门。 */
  function applyHysteresis(m: Metrics): void {
    if (!m.maximized) {
      isPortrait.value = false;
      return;
    }
    const ratio = m.height > 0 ? m.width / m.height : Number.POSITIVE_INFINITY;
    if (!isPortrait.value) {
      if (ratio < RATIO_ENTER && m.height >= HEIGHT_ENTER) isPortrait.value = true;
    } else if (ratio >= RATIO_EXIT || m.height < HEIGHT_EXIT) {
      isPortrait.value = false;
    }
  }

  async function evaluate(): Promise<void> {
    const seq = ++evalSeq;
    const m = await readMetrics();
    if (seq !== evalSeq) return; // 有更新的判定发起，丢弃本次结果
    if (import.meta.env.DEV && readOverride() !== null) return; // dev 强制覆盖优先，不覆盖
    applyHysteresis(m);
  }

  function recompute(): void {
    if (import.meta.env.DEV) {
      const override = readOverride();
      if (override !== null) {
        isPortrait.value = override === "portrait";
        return;
      }
    }
    void evaluate();
  }

  /** resize / move / scale 事件密集，用 rAF 合帧，每帧只判定一次。 */
  function schedule(): void {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      recompute();
    });
  }

  /**
   * dev 验收钩子：控制台运行期改 window.__FORCE_LAYOUT__ 或 hash 里 ?layout= 时，
   * 普通赋值不触发事件，故轮询检测变化并重新判定；恒在 import.meta.env.DEV 分支内，
   * 生产构建整体消除。
   */
  function installDevOverride(onChange: () => void): () => void {
    let cached = readOverride();
    const poll = (): void => {
      const current = readOverride();
      if (current !== cached) {
        cached = current;
        onChange();
      }
    };
    window.addEventListener("hashchange", poll);
    const timer = window.setInterval(poll, 200);
    return () => {
      window.removeEventListener("hashchange", poll);
      clearInterval(timer);
    };
  }

  onMounted(() => {
    recompute();
    window.addEventListener("resize", schedule);
    if (import.meta.env.DEV) devCleanup = installDevOverride(recompute);
    if (isTauri()) {
      // tauri://resize（= onResized）/ 窗口移动 / DPI 变化：跟随最大化与跨屏 DPI 变更。
      const win = getCurrentWindow();
      void Promise.all([
        win.onResized(schedule),
        win.onMoved(schedule),
        win.onScaleChanged(schedule),
      ])
        .then((fns) => {
          if (disposed) {
            for (const f of fns) f();
            return;
          }
          unlisteners.push(...fns);
        })
        .catch(() => {}); // 订阅失败仍有 window.resize 兜底
    }
  });

  onBeforeUnmount(() => {
    disposed = true;
    evalSeq++; // 作废全部在途异步判定
    if (frame) cancelAnimationFrame(frame);
    window.removeEventListener("resize", schedule);
    devCleanup?.();
    for (const u of unlisteners) u();
    unlisteners.length = 0;
  });

  return { isPortrait };
}
