/**
 * 设置分组清单：SettingsDialog 的左栏顺序与 SettingsPanel 的 section 取值共用此处，
 * 两侧不再各写一份。label 由 `settings.groups.<id>` 文案键提供。
 * 分组依次为：通用 / 外观 / 下载 / 浏览与图片 / 小说翻译 / 维护。
 */
export const SETTINGS_SECTIONS = ["general", "appearance", "download", "images", "translation", "maintenance"] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];
