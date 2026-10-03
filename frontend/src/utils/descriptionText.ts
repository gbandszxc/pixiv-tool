/** 详情描述只展示文本；保留换行/段落，既不执行 HTML，也不挂载外部资源。 */
export function descriptionText(html?: string): string {
  if (!html) return "";
  const template = document.createElement("template");
  template.innerHTML = html;
  const content = template.content;
  content.querySelectorAll("script, style, noscript, iframe, object").forEach((node) => node.remove());
  content.querySelectorAll("br").forEach((node) => node.replaceWith("\n"));
  content.querySelectorAll("p, div, li, blockquote, h1, h2, h3, h4, h5, h6").forEach((node) => {
    node.before("\n");
    node.after("\n");
  });
  return (content.textContent ?? "").replace(/\n{3,}/g, "\n\n").trim();
}
