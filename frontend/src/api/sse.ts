/**
 * SSE EventSource 封装 — 类型安全的事件订阅。
 */

export interface SSEEvent {
  type: string;
  data: Record<string, unknown>;
}

type EventHandler = (data: Record<string, unknown>) => void;

export function createSSEClient(url: string): {
  subscribe: (eventType: string, handler: EventHandler) => () => void;
  close: () => void;
} {
  const source = new EventSource(url);
  const handlers = new Map<string, Set<EventHandler>>();

  source.onmessage = (event) => {
    const handlers_set = handlers.get("*");
    if (handlers_set) {
      for (const h of handlers_set) {
        h({ raw: event.data });
      }
    }
  };

  return {
    subscribe(eventType: string, handler: EventHandler) {
      if (!handlers.has(eventType)) {
        handlers.set(eventType, new Set());
        source.addEventListener(eventType, (e: MessageEvent) => {
          try {
            const data = JSON.parse(e.data);
            for (const h of handlers.get(eventType)!) {
              h(data);
            }
          } catch {
            // ignore parse errors
          }
        });
      }
      handlers.get(eventType)!.add(handler);

      // 返回取消订阅函数
      return () => {
        handlers.get(eventType)?.delete(handler);
      };
    },

    close() {
      source.close();
      handlers.clear();
    },
  };
}
