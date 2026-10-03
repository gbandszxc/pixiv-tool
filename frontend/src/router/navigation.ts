import type { Router } from "vue-router";

export type NavigationGroup = "discover" | "following" | "library" | "downloads";
export const groupRoots: Record<NavigationGroup, string> = {
  discover: "/browse/home", following: "/browse/feed", library: "/browse/bookmark", downloads: "/tools/tasks",
};

/** 详情保留来路归属；无来路直达时归入发现。 */
export function navigationGroup(path: string): NavigationGroup | undefined {
  if (path.startsWith("/tools")) return "downloads";
  if (["/browse/feed", "/browse/watchlist"].includes(path)) return "following";
  if (["/browse/bookmark", "/browse/history"].includes(path)) return "library";
  if (/^\/browse\/(work|series|user)\//.test(path)) return undefined;
  return "discover";
}
export function fallbackPage(path: string): string {
  const root = groupRoots[navigationGroup(path) ?? "discover"];
  return path === root ? groupRoots.discover : root;
}

/** 必须使用页面注入的实例，避免热更新后模块实例与应用历史分离。 */
export function canGoBack(router: Router): boolean {
  const back = router.options.history.state.back;
  return typeof back === "string" && back.startsWith("/") && router.resolve(back).matched.length > 0;
}

export function goBack(router: Router): void {
  if (canGoBack(router)) router.back();
  else void router.replace(fallbackPage(router.currentRoute.value.path));
}
