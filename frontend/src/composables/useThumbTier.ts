/**
 * 缩略图档位读取：设置字符串 → 合法 ThumbTier。
 *
 * settings.json 可被手工改动，后端虽做白名单校验，前端仍按「未知值 → 该界面默认档」
 * 兜底，避免脏值透传成 /c/undefined/ 让整页封面 404。档位语义见 utils/thumb.ts。
 * 组件不直接读 settings 里的档位字符串，一律经本 composable 取值。
 */
import { computed, type ComputedRef } from "vue";
import { useSettingsStore } from "../stores/settings";
import type { ThumbTier } from "../utils/thumb";

/** 三个档位设置键（与后端 Settings 字段名一致）。 */
type ThumbSettingKey =
  | "thumb_quality_grid"
  | "thumb_quality_detail"
  | "thumb_quality_fullscreen";

/** 各键的合法取值（与后端校验一致）。 */
const ALLOWED: Record<ThumbSettingKey, ThumbTier[]> = {
  thumb_quality_grid: ["small", "medium", "large"],
  thumb_quality_detail: ["medium", "large", "original"],
  thumb_quality_fullscreen: ["large", "original"],
};

/** 各键的兜底档位（与后端 Settings::default 一致）。 */
const FALLBACK: Record<ThumbSettingKey, ThumbTier> = {
  thumb_quality_grid: "medium",
  thumb_quality_detail: "medium",
  thumb_quality_fullscreen: "large",
};

/** 响应式读取档位：设置变化即时生效（已加载的图片除外，浏览器自身缓存不回刷）。 */
export function useThumbTier(key: ThumbSettingKey): ComputedRef<ThumbTier> {
  const settings = useSettingsStore();
  return computed(() => {
    const raw = settings.settings[key] as string;
    const tier = raw as ThumbTier;
    return ALLOWED[key].includes(tier) ? tier : FALLBACK[key];
  });
}
