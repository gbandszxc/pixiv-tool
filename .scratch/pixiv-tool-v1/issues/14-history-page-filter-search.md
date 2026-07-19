# 14 — 历史页（筛选/搜索/打开文件）

**What to build:**

从用户视角：点侧边栏"历史"进入历史页，看到所有已抓小说的列表（表格形式），列含标题、作者、系列、抓取时间。顶部有筛选器（按系列/作者下拉）和搜索框（按标题关键词）。每行有"打开文件位置"按钮，点击在系统资源管理器中定位到该文件。

**Blocked by:** 10（单篇任务，需要 novels 表有数据）

**Status:** done

**Acceptance criteria:**

- [x] `GET /api/novels` 分页查询，支持参数：page、page_size、series_id、author_id、keyword
- [x] 返回格式：`{items: [...], total, page, page_size}`
- [x] `GET /api/novels/{id}/file` 返回文件路径（不直接返回文件内容，太大）
- [x] `POST /api/novels/{id}/open` 在系统资源管理器中打开文件所在目录（Windows: `explorer /select,<path>`，跨平台预留）
- [x] `DELETE /api/novels/{id}` 删除数据库记录（可选删文件，query param `delete_file=true`）
- [x] Vue 历史页（路由 `/history`）：
  - [x] 表格显示：标题、作者、系列（可空）、抓取时间、状态
  - [x] 顶部筛选：系列下拉（来自 novels 表去重 series_id）、作者下拉
  - [x] 搜索框：按标题模糊匹配
  - [x] 分页控件（默认每页 50 条）
  - [x] 每行操作：打开文件位置、删除
  - [x] 删除前确认对话框
- [x] 筛选和搜索可组合（如"作者 A + 关键词 风"）
- [x] 空状态友好提示（"还没有抓取记录，去抓取页试试"）
- [x] 性能：分页查询用 SQL LIMIT/OFFSET，不全量加载
- [x] 单元测试：查询接口的筛选/搜索/分页
