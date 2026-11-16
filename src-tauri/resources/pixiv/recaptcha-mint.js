// pixiv 关注人机验证 token 铸造脚本（ADR 0055）。
// 由 Rust 侧 initialization_script 注入 recaptcha-mint 窗口，在 www.pixiv.net
// 页面上下文执行：加载 recaptcha.net 的 reCAPTCHA Enterprise 脚本（render=site key，
// 由 Rust 侧从登录态主页 __NEXT_DATA__ 解析后 replace 注入），经
// grecaptcha.enterprise.execute 铸出关注用 score token，再导航到自定义 scheme
// 由 Rust 的 on_navigation 拦截回收。整体为无感验证，无需用户交互。
//
// 形态对齐 ppixiv 的实现：ready 后先按 pixiv 自身行为发一次 www/pageload，
// 再以 www/follow_user action 铸关注 token。
//
// 安全纪律：本脚本是编译期常量（include_str!），不含任何用户输入；
// token 只经导航 URL 一次性回传，Rust 侧不落日志。
//
// 回收通道：pixivtool-recaptcha://done?t={token} / pixivtool-recaptcha://error?m={message}
(() => {
  // initialization_script 在窗口内每次页面加载都会注入，防重复触发
  if (window.__pixivtoolMintRan) return;
  window.__pixivtoolMintRan = true;

  // 编译期哨兵值，Rust 构建窗口前以真实 site key 替换；未替换即在此拦下
  const SITE_KEY = "__PIXIVTOOL_RECAPTCHA_SITE_KEY__";

  const report = (scheme, key, value) => {
    try {
      location.href =
        "pixivtool-recaptcha://" + scheme + "?" + key + "=" + encodeURIComponent(value);
    } catch (_) {
      // 窗口可能已在销毁中，忽略
    }
  };
  const fail = (message) => report("error", "m", String(message).slice(0, 200));

  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  // initialization_script 在 document start 注入，head 可能尚未就绪
  async function waitForHead(timeoutMs) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (document.head) return true;
      await wait(50);
    }
    return false;
  }

  function loadEnterpriseJs(siteKey) {
    return new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src =
        "https://www.recaptcha.net/recaptcha/enterprise.js?render=" +
        encodeURIComponent(siteKey);
      script.onload = () => resolve();
      script.onerror = () => reject(new Error("recaptcha enterprise.js 加载失败"));
      document.head.appendChild(script);
    });
  }

  function waitForGrecaptcha(timeoutMs) {
    const deadline = Date.now() + timeoutMs;
    return (async () => {
      while (Date.now() < deadline) {
        if (window.grecaptcha && window.grecaptcha.enterprise) return true;
        await wait(150);
      }
      return false;
    })();
  }

  (async () => {
    try {
      if (!SITE_KEY || SITE_KEY.startsWith("__")) {
        fail("site key 未注入");
        return;
      }
      if (!(await waitForHead(10000))) {
        fail("页面 head 未就绪");
        return;
      }
      await loadEnterpriseJs(SITE_KEY);
      if (!(await waitForGrecaptcha(10000))) {
        fail("grecaptcha 未初始化");
        return;
      }
      // ready 后按 pixiv 自身行为发一次 pageload（对齐 ppixiv；token 不回收）
      await new Promise((resolve) => window.grecaptcha.enterprise.ready(resolve));
      window.grecaptcha.enterprise.execute(SITE_KEY, { action: "www/pageload" });
      await wait(300);
      const token = await window.grecaptcha.enterprise.execute(SITE_KEY, {
        action: "www/follow_user",
      });
      if (!token) {
        fail("recaptcha execute 返回空 token");
        return;
      }
      report("done", "t", token);
    } catch (err) {
      fail(err && err.message ? err.message : err);
    }
  })();
})();
