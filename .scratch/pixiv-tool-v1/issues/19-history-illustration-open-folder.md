# 19 — 历史页兼容插画 + 打开所在文件夹按钮

**Type:** task

**Status:** done

## What to build

1. 历史页只能看到小说，插画抓取结果只存在于任务页。需要在历史页增加插画分类
   支持：页签切换小说/插画，各自分页/搜索/批量删除/清空。
2. 历史页每行增加"打开所在文件夹"按钮（SVG 文件夹图标），点击按平台调用系统
   文件管理器打开当前下载项所在目录。

## 实现

- 后端 `src/pixiv_tool/platform.py`：新增 `reveal_in_file_manager(path)`——
  Windows `explorer /select,`、macOS `open -R`、Linux `xdg-open` 所在目录；
  文件不存在时回退打开父目录。novels.py 的内联实现改为复用此函数。
- 后端 `src/pixiv_tool/api/illustrations.py`：与 novels.py 平行的插画历史 API
  （列表 / open / 删除 / 批量删除 / 清空）。
- 后端 `db.py`：`list_illustrations` 增加 keyword 过滤，新增
  `delete_all_illustrations`。
- 前端 `HistoryView.vue`：小说/插画页签切换（复用 TasksView 的 radio-button
  模式），插画列含页数；操作列改为 SVG 文件夹图标按钮（带 tooltip + aria-label）。
- 前端 `stores/history.ts`：新增 Illustration 类型与插画 fetch/delete/open 操作。
- i18n：zh-CN / en-US 各补 history.category / pagesColumn / openFolder 等文案。

## Acceptance

- [x] 历史页可切换到插画分类并看到插画记录（列：标题/作者/页数/抓取时间/操作）
- [x] 插画记录可搜索、批量删除、一键清空
- [x] 每行"打开所在文件夹"按钮按平台调用文件管理器（Windows/macOS 定位文件，
      Linux 打开目录；文件丢失时回退父目录）
- [x] 后端测试 147 全过（新增 12 个插画 API / 平台分发测试）
- [x] 前端 `pnpm build` 通过；浏览器实测历史页两分类、空态、错误提示、删除流
