/**
 * 自定义右键菜单的条目契约（`ContextMenu.vue` 与其调用方共用）。
 *
 * `icon` 是内置 lucide 线性图标白名单（stroke 2）；未给时不占图标位。
 * `disabled` 为 true 的条目保留文案与图标但不可聚焦 / 点击（原生 button disabled）。
 */
export type ContextMenuIcon = "save" | "copy" | "fullscreen";

export interface ContextMenuEntry {
  id: string;
  label: string;
  icon?: ContextMenuIcon;
  disabled?: boolean;
}
