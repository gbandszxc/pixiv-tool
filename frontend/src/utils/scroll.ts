/**
 * 主内容滚动复位。
 *
 * 应用外壳恒为视口高、`.app-content` 是唯一滚动容器（见 DESIGN.md「Layout」），
 * 因此 `window.scrollTo` 在主内容上不生效——列表换筛选条件、系列页切页、
 * 详情页换作品时都必须复位这个容器。
 */
export function scrollContentToTop(behavior: ScrollBehavior = "instant"): void {
  document.querySelector(".app-content")?.scrollTo({ top: 0, behavior });
}
