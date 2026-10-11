/**
 * 下载文件名模板校验（纯函数，供设置弹窗保存前预校验使用）。
 *
 * 模板 = 自由字面文本 + `{占位符}` 语法，校验规则与后端 settings_save 一致：
 * - 非空、长度 ≤ 200；
 * - 禁含 `< > : " / \ | ? *` 与 ASCII 控制字符（即后端 sanitize_filename 会替换的字符，
 *   模板层面直接拦下，避免生成含义不明的文件名）；
 * - `{` / `}` 必须成对（不嵌套、不允许孤立单向括号）；
 * - 占位符名必须在对应白名单内（图片 / 小说两套，系列沿用小说名单）；
 * - 图片模板必须含 `{page}` 或 `{page1}`（多页作品分页），系列小说模板必须含 `{order}`。
 *
 * 返回 i18n 键 + 命名插值参数，由视图层用 `t()` 格式化展示
 * （文案见 locales 的 settings.filenameTemplate*）。
 */
export type FilenameTemplateKind = "illust" | "novel" | "novelSeries";

/** 模板长度上限（字符数，与后端一致）。 */
const TEMPLATE_MAX_LENGTH = 200;

/** 占位符白名单：图片 / 小说（系列小说沿用小说名单，另要求必含 {order}）。 */
export const ILLUST_TEMPLATE_PLACEHOLDERS = ["user", "uid", "title", "pid", "page", "page1"] as const;
export const NOVEL_TEMPLATE_PLACEHOLDERS = ["user", "uid", "title", "nid", "order", "sid"] as const;

/** 禁用字符（与后端 sanitize_filename 的替换集合一致，另含 ASCII 控制字符）。 */
const FORBIDDEN_CHARS = /[<>:"/\\|?*\x00-\x1f]/;
/** 禁用字符的展示文案（仅供错误消息插值，保持与 FORBIDDEN_CHARS 一致）。 */
const FORBIDDEN_CHARS_TEXT = '< > : " / \\ | ? *';

/** 校验失败的首个错误：i18n 键（settings.filenameTemplate*）+ 命名插值参数。 */
export interface FilenameTemplateIssue {
  key: string;
  params: Record<string, string>;
}

function placeholdersOf(kind: FilenameTemplateKind): readonly string[] {
  return kind === "illust" ? ILLUST_TEMPLATE_PLACEHOLDERS : NOVEL_TEMPLATE_PLACEHOLDERS;
}

/** 大括号必须成对：不嵌套、不出现孤立的单向括号。 */
function hasBalancedBraces(template: string): boolean {
  let depth = 0;
  for (const ch of template) {
    if (ch === "{") {
      if (++depth > 1) return false;
    } else if (ch === "}" && --depth < 0) {
      return false;
    }
  }
  return depth === 0;
}

/** 校验模板：合法返回 null，非法返回首个命中的错误（按严重顺序短路）。 */
export function validateFilenameTemplate(kind: FilenameTemplateKind, template: string): FilenameTemplateIssue | null {
  if (!template.trim()) return { key: "settings.filenameTemplateEmpty", params: {} };
  if (template.length > TEMPLATE_MAX_LENGTH) return { key: "settings.filenameTemplateTooLong", params: {} };
  if (FORBIDDEN_CHARS.test(template)) {
    return { key: "settings.filenameTemplateChars", params: { chars: FORBIDDEN_CHARS_TEXT } };
  }
  if (!hasBalancedBraces(template)) return { key: "settings.filenameTemplateBraces", params: {} };
  const whitelist = placeholdersOf(kind);
  for (const match of template.matchAll(/\{([^{}]*)\}/g)) {
    if (!(whitelist as readonly string[]).includes(match[1])) {
      return { key: "settings.filenameTemplateUnknown", params: { name: `{${match[1]}}` } };
    }
  }
  if (kind === "illust" && !/\{page(?:1)?\}/.test(template)) {
    return { key: "settings.filenameTemplateIllustPage", params: { page: "{page}", page1: "{page1}" } };
  }
  if (kind === "novelSeries" && !template.includes("{order}")) {
    return { key: "settings.filenameTemplateOrder", params: { order: "{order}" } };
  }
  return null;
}

/** 白名单 → 支持文本里的占位符清单（`{user} {uid} …`），与校验白名单同源、不会漂移。 */
export function formatTemplatePlaceholders(names: readonly string[]): string {
  return names.map((name) => `{${name}}`).join(" ");
}
