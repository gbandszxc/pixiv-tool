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
