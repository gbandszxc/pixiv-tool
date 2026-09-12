export function notify(message: string): void {
  window.dispatchEvent(new CustomEvent<string>("pixiv-tool:notify", { detail: message }));
}
