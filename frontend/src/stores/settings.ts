import { defineStore } from "pinia";
import { ref } from "vue";
import { isTauri, invoke } from "../api/tauri";
import type { Settings } from "../api/tauri";
import { mockSettingsGet, mockSettingsSave } from "../api/devMock";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export type { Settings } from "../api/tauri";

// ===== 小说翻译运行参数（二级高级设置）=====
// 键名、默认值与区间是 src-tauri/src/settings.rs 的 TRANSLATION_AGENT_INT_RULES /
// TRANSLATION_AGENT_BOOL_KEYS 的前端镜像；改后端时必须同批改这里，避免「保存成功但显示越界」。

/** 高级设置的分类（顺序即二级入口顺序）；extra / diagnostics 不参与整数校验。 */
export type TranslationAgentCategory =
  | "model"
  | "concurrency"
  | "retry"
  | "chunk"
  | "extra"
  | "diagnostics";

/** 参与整数校验的运行参数键。 */
export type TranslationAgentIntKey =
  | "translation_agent_global_concurrency"
  | "translation_agent_novel_concurrency"
  | "translation_agent_active_novels"
  | "translation_agent_retry_limit"
  | "translation_agent_backoff_base_ms"
  | "translation_agent_backoff_max_seconds"
  | "translation_agent_rework_rounds"
  | "translation_agent_split_depth"
  | "translation_agent_tool_corrections"
  | "translation_agent_max_turns"
  | "translation_agent_stall_limit"
  | "translation_agent_context_window"
  | "translation_agent_output_cap"
  | "translation_agent_compact_percent"
  | "translation_agent_recent_history_tokens"
  | "translation_agent_summary_tokens"
  | "translation_agent_chunk_target_chars"
  | "translation_agent_chunk_max_chars"
  | "translation_agent_context_chars"
  | "translation_call_budget"
  | "translation_timeout_seconds";

/** 二级页面分类顺序（与计划 §8.2 一致；extra / diagnostics 不参与整数校验）。 */
export const TRANSLATION_AGENT_CATEGORIES: readonly TranslationAgentCategory[] = [
  "model",
  "concurrency",
  "retry",
  "chunk",
  "extra",
  "diagnostics",
];

/** 运行参数的单位（i18n 键 `translation.agent.units.<unit>`）。 */
export type TranslationAgentUnit =
  | "tokens"
  | "chars"
  | "milliseconds"
  | "seconds"
  | "percent"
  | "count"
  | "times"
  | "rounds"
  | "turns"
  | "levels";

/** 单条运行参数的区间与默认值（labelKey / hintKey 指向 i18n；step 仅用于数字输入框）。 */
export interface TranslationAgentIntRule {
  key: TranslationAgentIntKey;
  default: number;
  min: number;
  max: number;
  step: number;
  unit: TranslationAgentUnit;
  category: Exclude<TranslationAgentCategory, "extra" | "diagnostics">;
  labelKey: string;
  hintKey: string;
}

const fields = (key: TranslationAgentIntKey) => ({
  labelKey: `translation.agent.fields.${key}.label`,
  hintKey: `translation.agent.fields.${key}.hint`,
});

/** 与后端 TRANSLATION_AGENT_INT_RULES 逐项一致（顺序即分类内字段顺序）。 */
export const TRANSLATION_AGENT_INT_RULES: readonly TranslationAgentIntRule[] = [
  { key: "translation_agent_context_window", default: 131_072, min: 16_384, max: 2_097_152, step: 1_024, unit: "tokens", category: "model", ...fields("translation_agent_context_window") },
  { key: "translation_agent_output_cap", default: 16_384, min: 1_024, max: 262_144, step: 1_024, unit: "tokens", category: "model", ...fields("translation_agent_output_cap") },
  { key: "translation_timeout_seconds", default: 600, min: 30, max: 3_600, step: 30, unit: "seconds", category: "model", ...fields("translation_timeout_seconds") },
  { key: "translation_agent_compact_percent", default: 75, min: 50, max: 90, step: 1, unit: "percent", category: "model", ...fields("translation_agent_compact_percent") },
  { key: "translation_agent_recent_history_tokens", default: 4_096, min: 512, max: 131_072, step: 256, unit: "tokens", category: "model", ...fields("translation_agent_recent_history_tokens") },
  { key: "translation_agent_summary_tokens", default: 2_048, min: 256, max: 65_536, step: 256, unit: "tokens", category: "model", ...fields("translation_agent_summary_tokens") },

  { key: "translation_agent_global_concurrency", default: 4, min: 1, max: 16, step: 1, unit: "count", category: "concurrency", ...fields("translation_agent_global_concurrency") },
  { key: "translation_agent_novel_concurrency", default: 4, min: 1, max: 16, step: 1, unit: "count", category: "concurrency", ...fields("translation_agent_novel_concurrency") },
  { key: "translation_agent_active_novels", default: 2, min: 1, max: 8, step: 1, unit: "count", category: "concurrency", ...fields("translation_agent_active_novels") },
  { key: "translation_agent_max_turns", default: 24, min: 4, max: 128, step: 1, unit: "turns", category: "concurrency", ...fields("translation_agent_max_turns") },
  { key: "translation_agent_stall_limit", default: 8, min: 3, max: 32, step: 1, unit: "rounds", category: "concurrency", ...fields("translation_agent_stall_limit") },
  { key: "translation_call_budget", default: 0, min: 0, max: 1_000_000, step: 1, unit: "times", category: "concurrency", ...fields("translation_call_budget") },

  { key: "translation_agent_retry_limit", default: 2, min: 0, max: 5, step: 1, unit: "times", category: "retry", ...fields("translation_agent_retry_limit") },
  { key: "translation_agent_backoff_base_ms", default: 1_000, min: 500, max: 30_000, step: 100, unit: "milliseconds", category: "retry", ...fields("translation_agent_backoff_base_ms") },
  { key: "translation_agent_backoff_max_seconds", default: 120, min: 10, max: 600, step: 10, unit: "seconds", category: "retry", ...fields("translation_agent_backoff_max_seconds") },
  { key: "translation_agent_rework_rounds", default: 2, min: 0, max: 5, step: 1, unit: "rounds", category: "retry", ...fields("translation_agent_rework_rounds") },
  { key: "translation_agent_split_depth", default: 3, min: 0, max: 5, step: 1, unit: "levels", category: "retry", ...fields("translation_agent_split_depth") },
  { key: "translation_agent_tool_corrections", default: 2, min: 0, max: 3, step: 1, unit: "times", category: "retry", ...fields("translation_agent_tool_corrections") },

  { key: "translation_agent_chunk_target_chars", default: 3_000, min: 500, max: 100_000, step: 100, unit: "chars", category: "chunk", ...fields("translation_agent_chunk_target_chars") },
  { key: "translation_agent_chunk_max_chars", default: 6_000, min: 1_000, max: 200_000, step: 100, unit: "chars", category: "chunk", ...fields("translation_agent_chunk_max_chars") },
  { key: "translation_agent_context_chars", default: 600, min: 0, max: 20_000, step: 100, unit: "chars", category: "chunk", ...fields("translation_agent_context_chars") },
];

const RULES_BY_KEY = new Map(TRANSLATION_AGENT_INT_RULES.map((rule) => [rule.key, rule]));

/** 按键取区间定义；键来自上面的封闭联合类型，运行时不会缺项。 */
export function translationAgentIntRule(key: TranslationAgentIntKey): TranslationAgentIntRule {
  const rule = RULES_BY_KEY.get(key);
  if (!rule) throw new Error(`未知运行参数：${key}`);
  return rule;
}

/** 读取运行参数当前值：老配置 / mock 缺键或脏值时按后端默认值回显。 */
export function agentIntValue(settings: Settings, key: TranslationAgentIntKey): number {
  const value = settings[key];
  return typeof value === "number" && Number.isFinite(value) ? value : translationAgentIntRule(key).default;
}

/** 运行参数控件的 DOM 键：整数参数与「未知结果重放」开关。 */
export type TranslationAgentFieldKey = TranslationAgentIntKey | "translation_agent_replay_unknown";

/** 运行参数控件的 DOM id（label for、保存失败定位与验收共用）。 */
export function agentFieldId(key: TranslationAgentFieldKey): string {
  return `translation-agent-${key.replace(/^translation_(agent_)?/, "").replace(/_/g, "-")}`;
}

/** 校验结果码；文案键为 `translation.agent.errors.<code>`。 */
export type TranslationAgentIssueCode =
  | "range"
  | "chunkTargetOverMax"
  | "outputCapOverWindow"
  | "historyPlusSummaryOverWindow";

/** 单条违规：key 指向要标红的字段（交叉违规挂在下游字段上）。 */
export interface TranslationAgentIssue {
  key: TranslationAgentIntKey;
  code: TranslationAgentIssueCode;
}

/** 二级页面的保存失败定位目标：分类必填，字段为空时聚焦该分类的首个控件。 */
export interface TranslationAgentLocate {
  category: TranslationAgentCategory;
  key: TranslationAgentIntKey | null;
}

/** 计数与校验的取值口径：缺键视为推荐值，NaN / 非数字按「已修改且非法」处理。 */
function agentRawValue(settings: Settings, key: TranslationAgentIntKey): number {
  const value = settings[key];
  return value === undefined ? translationAgentIntRule(key).default : value;
}

/**
 * 保存前校验（与后端 `validate_translation_agent_settings` 同规则）：
 * 先逐项检查整数与区间，再检查输出 / 摘要 / 分块三处交叉约束。
 * 交叉项只在其相关单项都合法时才报告，避免一处错误连带标红多个字段。
 */
export function validateTranslationAgent(settings: Settings): TranslationAgentIssue[] {
  const issues: TranslationAgentIssue[] = [];
  const invalid = new Set<TranslationAgentIntKey>();
  for (const rule of TRANSLATION_AGENT_INT_RULES) {
    const value = agentRawValue(settings, rule.key);
    if (!Number.isInteger(value) || value < rule.min || value > rule.max) {
      issues.push({ key: rule.key, code: "range" });
      invalid.add(rule.key);
    }
  }
  const value = (key: TranslationAgentIntKey) => agentRawValue(settings, key);
  const bothValid = (a: TranslationAgentIntKey, b: TranslationAgentIntKey) => !invalid.has(a) && !invalid.has(b);
  if (bothValid("translation_agent_chunk_target_chars", "translation_agent_chunk_max_chars")
    && value("translation_agent_chunk_target_chars") > value("translation_agent_chunk_max_chars")) {
    issues.push({ key: "translation_agent_chunk_target_chars", code: "chunkTargetOverMax" });
  }
  if (bothValid("translation_agent_output_cap", "translation_agent_context_window")
    && value("translation_agent_output_cap") > value("translation_agent_context_window")) {
    issues.push({ key: "translation_agent_output_cap", code: "outputCapOverWindow" });
  }
  if (bothValid("translation_agent_recent_history_tokens", "translation_agent_summary_tokens")
    && value("translation_agent_recent_history_tokens") + value("translation_agent_summary_tokens")
      > value("translation_agent_context_window")) {
    issues.push({ key: "translation_agent_recent_history_tokens", code: "historyPlusSummaryOverWindow" });
  }
  return issues;
}

/** 恢复指定分类的推荐值（就地改写草稿；高级 JSON 由调用方写回空对象）。 */
export function resetTranslationAgentCategory(settings: Settings, category: TranslationAgentCategory): void {
  for (const rule of TRANSLATION_AGENT_INT_RULES) {
    if (rule.category === category) settings[rule.key] = rule.default;
  }
  if (category === "retry") settings.translation_agent_replay_unknown = false;
}

/** 恢复全部运行参数推荐值（不触碰一级字段与诊断会话开关）。 */
export function resetTranslationAgentSettings(settings: Settings): void {
  for (const rule of TRANSLATION_AGENT_INT_RULES) settings[rule.key] = rule.default;
  settings.translation_agent_replay_unknown = false;
}

/** JSON「空对象视为推荐值」：null / 数组 / 标量都算已修改或非法。 */
export function isDefaultTranslationExtra(value: unknown): boolean {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    && Object.keys(value as Record<string, unknown>).length === 0;
}

/** 补齐缺失的运行参数键（浏览器 mock 与旧响应），让草稿与「已修改 N 项」使用后端默认值。 */
export function normalizeTranslationAgentSettings(settings: Settings): Settings {
  const next = { ...settings };
  for (const rule of TRANSLATION_AGENT_INT_RULES) {
    if (typeof next[rule.key] !== "number") next[rule.key] = rule.default;
  }
  if (typeof next.translation_agent_replay_unknown !== "boolean") next.translation_agent_replay_unknown = false;
  if (typeof next.translation_confirm_before_start !== "boolean") next.translation_confirm_before_start = false;
  return next;
}

/**
 * 按分类统计与推荐值的差异项数（整数、布尔重放开关与高级 JSON）。
 * 高级 JSON 传入草稿解析结果；解析失败时调用方传 undefined，计为已修改并同时落校验错误。
 * 清空输入框得到的 NaN 也算已修改（它是用户显式改动，保存时会被校验拦下）。
 */
export function countModifiedTranslationAgent(
  settings: Settings,
  extra: unknown,
): Record<TranslationAgentCategory, number> {
  const counts: Record<TranslationAgentCategory, number> = { model: 0, concurrency: 0, retry: 0, chunk: 0, extra: 0, diagnostics: 0 };
  for (const rule of TRANSLATION_AGENT_INT_RULES) {
    if (agentRawValue(settings, rule.key) !== rule.default) counts[rule.category] += 1;
  }
  if (settings.translation_agent_replay_unknown === true) counts.retry += 1;
  if (!isDefaultTranslationExtra(extra)) counts.extra += 1;
  return counts;
}

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings>({
    output_dir: "",  // 实际值由后端 settings_get 提供（系统下载目录/pixiv-tool）
    output_formats: ["txt", "markdown"],
    // 下载文件名模板（占位符语法），默认值与后端 Settings::default 一致；校验见 utils/filenameTemplate.ts
    illust_filename_template: "{title}_{pid}_p{page}",
    novel_filename_template: "{title}_{nid}",
    novel_series_filename_template: "{order}_{title}",
    language: "zh-CN",
    theme: "auto",
    theme_color: "pixiv",
    startup_page: "/browse/home",
    backend_port: null,
    max_wait_seconds: 180,
    // 以下四项的取值约束见 useThumbTier；默认值与后端 Settings::default 一致
    show_r18: true,
    thumb_quality_grid: "medium",
    thumb_quality_detail: "medium",
    thumb_quality_fullscreen: "large",
    // 图片磁盘缓存上限（MiB）：区间 256~2048，默认 512
    image_cache_max_mib: 512,
    // 详情页图片宽度占比（区间 0.5~1.0），默认 1.0 = 撑满舞台
    detail_image_scale: 1,
    // 小说正文字号缩放（区间 0.75~2.0），默认 1.0
    novel_font_scale: 1.0,
    // 阅读背景色语义键（空串=跟随主题），默认未设置
    novel_bg_color: "",
    // SauceNAO API Key（以图识图必需），默认未配置
    saucenao_api_key: "",
    translation_api_url: "",
    // 接口协议（默认 Chat Completions），取值见 api/translation.ts 的 API_FORMATS
    translation_api_format: "chat_completions",
    translation_model: "",
    // 翻译目标语言（空串=跟随界面语言），取值见 api/translation.ts 的 TARGET_LANGUAGE_CODES
    translation_target_language: "",
    // 单请求超时（秒）：默认 600（10 分钟），合法区间 30~3600
    translation_timeout_seconds: 600,
    translation_extra: {},
    // 运行参数默认值与后端 Settings::default 一致；二级高级设置读写
    translation_agent_global_concurrency: 4,
    translation_agent_novel_concurrency: 4,
    translation_agent_active_novels: 2,
    translation_agent_retry_limit: 2,
    translation_agent_backoff_base_ms: 1000,
    translation_agent_backoff_max_seconds: 120,
    translation_agent_rework_rounds: 2,
    translation_agent_split_depth: 3,
    translation_agent_tool_corrections: 2,
    translation_agent_max_turns: 24,
    translation_agent_stall_limit: 8,
    translation_agent_context_window: 131072,
    translation_agent_output_cap: 16384,
    translation_agent_compact_percent: 75,
    translation_agent_recent_history_tokens: 4096,
    translation_agent_summary_tokens: 2048,
    translation_agent_chunk_target_chars: 3000,
    translation_agent_chunk_max_chars: 6000,
    translation_agent_context_chars: 600,
    translation_call_budget: 0,
    translation_agent_replay_unknown: false,
    translation_confirm_before_start: false,
    translation_key_configured: false,
    translation_key_error: "",
  });

  async function fetchSettings() {
    // 非 Tauri（浏览器视觉验收）：内存版样例配置，见 api/devMock.ts。
    if (!isTauri()) {
      settings.value = mockSettingsGet();
      return;
    }
    settings.value = await invoke<Settings>("settings_get");
  }

  async function saveSettings(newSettings: Partial<Settings> & { translation_api_key?: string }) {
    const { translation_api_key: apiKey, ...publicSettings } = newSettings;
    if (!isTauri()) {
      if (apiKey !== undefined) publicSettings.translation_key_configured = Boolean(apiKey.trim());
      mockSettingsSave(publicSettings);
      Object.assign(settings.value, publicSettings);
      return;
    }
    // 校验失败时后端 reject string，由视图层用 errorMessage() 展示。
    await invoke("settings_save", { settings: newSettings });
    Object.assign(settings.value, publicSettings);
    if (apiKey !== undefined) {
      settings.value.translation_key_configured = Boolean(apiKey.trim());
      settings.value.translation_key_error = "";
    }
  }

  async function clearLogs() {
    await invoke("clear_logs");
  }

  async function selectDirectory(): Promise<string | null> {
    // 目录选择走系统对话框插件，不经过后端命令；取消时返回 null。
    const selected = await openDialog({ directory: true });
    return typeof selected === "string" ? selected : null;
  }

  return { settings, fetchSettings, saveSettings, clearLogs, selectDirectory };
});
