/**
 * 浏览列表通用无限滚动状态机。
 *
 * 供 browse 系列表格页（F1-F5）使用：页码从 1 开始，`hasMore` 由后端返回的
 * `next_page` 判定（null 即无更多）；错误经 api 层 errorMessage() 归一为可直接展示的文本。
 */
import { shallowRef, ref } from "vue";
import { errorMessage } from "../api/browse";

/** 与 IPC 契约的 BrowseList 同构（items/next_page），T 为条目类型。 */
export interface PagedList<T> {
  items: T[];
  total?: number | null;
  next_page?: number | null;
}

export function useInfiniteList<T>(fetcher: (page: number) => Promise<PagedList<T>>) {
  // shallowRef：列表整体替换/拼接，无需深层响应式（也规避泛型 T 的 UnwrapRef 问题）
  const items = shallowRef<T[]>([]);
  /** 首屏加载中（items 为空时的 loadMore） */
  const loading = ref(false);
  /** 追加下一页中（已有内容时的 loadMore） */
  const loadingMore = ref(false);
  /** 归一化后的错误文案；非空表示当前页加载失败，loadMore 会暂停直到 retry */
  const error = ref("");
  /** 下一次要请求的页码（从 1 开始） */
  const page = ref(1);
  /** 由 next_page 判定；首屏前默认 true，让 grid 发起首次加载 */
  const hasMore = ref(true);

  /** 请求下一页（首次调用即请求第 1 页）。并发/终态/错误态下为空操作。 */
  async function loadMore(): Promise<void> {
    if (loading.value || loadingMore.value || error.value || !hasMore.value) return;
    const initial = items.value.length === 0;
    if (initial) loading.value = true;
    else loadingMore.value = true;
    try {
      const data = await fetcher(page.value);
      items.value = initial ? data.items : items.value.concat(data.items);
      hasMore.value = data.next_page != null;
      if (data.next_page != null) page.value = data.next_page;
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      loading.value = false;
      loadingMore.value = false;
    }
  }

  /** 失败页重试（清除错误后重新请求当前页）。 */
  function retry(): void {
    if (!error.value) return;
    error.value = "";
    void loadMore();
  }

  /** 清空状态回到第 1 页（不发起请求）。 */
  function reset(): void {
    items.value = [];
    loading.value = false;
    loadingMore.value = false;
    error.value = "";
    page.value = 1;
    hasMore.value = true;
  }

  /** 重置并立即重新加载第 1 页（tab/筛选切换时用）。 */
  function reload(): void {
    reset();
    void loadMore();
  }

  return { items, loading, loadingMore, error, page, hasMore, loadMore, retry, reset, reload };
}
