/**
 * 详情描述（作品 illustComment / 小说 description / 系列简介 / 作者简介）只展示文本：
 * 解码实体、保留换行与段落、去除脚本/样式等非正文，不执行/不挂载外部 HTML。
 *
 * 正文内链：输出 segments——<a href> 锚文本与裸 URL 经 utils/pixivLinks.ts 识别为
 * 链接段（pixiv 已封装页面站内跳转，其余外开浏览器），其余保持纯文本段；
 * descriptionText() 由同一管线派生，供翻译上下文等纯文本用途，保证两处排版一致。
 */
import { linkifyText, normalizePixivHref, parsePixivLinkTarget, type LinkifiedSegment } from "./pixivLinks";

/** 产生段落换行的块级标签（与旧 descriptionText 的换行规则一致）。 */
const BLOCK_TAGS = new Set(["p", "div", "li", "blockquote", "h1", "h2", "h3", "h4", "h5", "h6"]);

/** 描述 HTML → 段数组：<br>/块级转换行，<a href> 与裸 URL 成链接段，其余纯文本。 */
export function descriptionSegments(html?: string): LinkifiedSegment[] {
  if (!html) return [];
  const template = document.createElement("template");
  template.innerHTML = html;
  const content = template.content;
  content.querySelectorAll("script, style, noscript, iframe, object").forEach((node) => node.remove());

  const segments: LinkifiedSegment[] = [];

  /** 追加原文：纯文本运行 linkify（裸 URL 成链接段）。 */
  const pushText = (text: string): void => {
    if (text) segments.push(...linkifyText(text));
  };
  /** 追加换行：并入末尾文本段（无则新建），保持 textContent 时代的排版。 */
  const pushNewline = (): void => {
    const last = segments[segments.length - 1];
    if (last && last.kind === "text") last.text += "\n";
    else segments.push({ kind: "text", text: "\n" });
  };

  const walk = (node: Node): void => {
    if (node.nodeType === Node.TEXT_NODE) {
      pushText(node.textContent ?? "");
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const el = node as Element;
    const tag = el.tagName.toLowerCase();
    if (tag === "br") {
      pushNewline();
      return;
    }
    if (tag === "a") {
      // 锚点 → 链接段：href 归一后识别站内目标；label 取锚内纯文本（现网锚内无嵌套结构）
      const href = el.getAttribute("href") ?? "";
      const label = (el.textContent ?? "").replace(/\s+/g, " ").trim();
      if (href && label) {
        const url = normalizePixivHref(href);
        segments.push({ kind: "link", text: label, url, target: parsePixivLinkTarget(url) });
        return;
      }
      // 无 href / 空 label 的锚点：退化为子节点文本
    }
    const isBlock = BLOCK_TAGS.has(tag);
    if (isBlock) pushNewline();
    el.childNodes.forEach(walk);
    if (isBlock) pushNewline();
  };

  content.childNodes.forEach(walk);

  // 排版收尾（与旧 descriptionText 一致）：压缩 3+ 连续换行、去首尾空白
  for (const seg of segments) {
    if (seg.kind === "text") seg.text = seg.text.replace(/\n{3,}/g, "\n\n");
  }
  if (segments.length) {
    const first = segments[0];
    if (first.kind === "text") first.text = first.text.replace(/^\s+/, "");
    const last = segments[segments.length - 1];
    if (last.kind === "text") last.text = last.text.replace(/\s+$/, "");
  }
  return segments.filter((seg) => seg.kind !== "text" || seg.text.length > 0);
}

/** 详情描述 → 纯文本（翻译上下文等用途）；与 descriptionSegments 同一排版规则。 */
export function descriptionText(html?: string): string {
  return descriptionSegments(html)
    .map((seg) => seg.text)
    .join("");
}
