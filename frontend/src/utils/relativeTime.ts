/**
 * 相对时间轻量实现：刚刚 / N 分钟前 / N 小时前 / N 天前，超过 30 天回落
 * Intl 短日期（按界面语言排版）。供搜索历史条目（以图识图 / 浏览搜索）
 * 共用；标签文案由调用方经各自的 i18n 键解析后传入（渲染期求值，语言
 * 切换即时生效），本工具只保留时间分桶这一份实现。
 */

/** 相对时间标签集：分钟 / 小时 / 天按 {n} 插值，由调用方解析。 */
export interface RelativeTimeLabels {
  justNow: string;
  minutesAgo: (n: number) => string;
  hoursAgo: (n: number) => string;
  daysAgo: (n: number) => string;
}

export function relativeTime(time: string, locale: string, labels: RelativeTimeLabels): string {
  const at = new Date(time).getTime();
  if (Number.isNaN(at)) return "";
  const minutes = Math.floor(Math.max(0, Date.now() - at) / 60_000);
  if (minutes < 1) return labels.justNow;
  if (minutes < 60) return labels.minutesAgo(minutes);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return labels.hoursAgo(hours);
  const days = Math.floor(hours / 24);
  if (days < 30) return labels.daysAgo(days);
  return new Intl.DateTimeFormat(locale, { dateStyle: "short" }).format(at);
}
