/**
 * 时间展示：pixiv 的 createDate 为带时区的 ISO 时间（如 `2026-10-17T23:00:00+09:00`）。
 * 网页端统一以 JST 展示发布时间，这里同样固定 `Asia/Tokyo`，避免跨时区换算后与
 * 网页端差一天或差一小时；只把年月日与时分按界面语言排版。
 *
 * 仅日期（mock / 部分列表字段为 `2026-09-15`）没有时区语义，按 UTC 原样格式化，
 * 避免被换算成前一天。
 */
const PIXIV_TIME_ZONE = "Asia/Tokyo";

export function formatDateTime(value: string | undefined, locale: string): string {
  if (!value) return "";
  const dateOnly = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  const date = dateOnly
    ? new Date(Date.UTC(Number(dateOnly[1]), Number(dateOnly[2]) - 1, Number(dateOnly[3])))
    : new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const options: Intl.DateTimeFormatOptions = dateOnly
    ? { year: "numeric", month: "long", day: "numeric", timeZone: "UTC" }
    : {
        year: "numeric",
        month: "long",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        timeZone: PIXIV_TIME_ZONE,
      };
  return new Intl.DateTimeFormat(locale, options).format(date);
}
