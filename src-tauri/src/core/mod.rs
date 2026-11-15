//! 业务核心：来源解析（sources）、爬虫（crawler / illust_crawler）、
//! 导出（exporter）、EPUB 引擎（epub：标记渲染 / EPUB3 组装 / 占位封面）、
//! 下载文件名模板（filename_template）、任务状态机（task_manager）。

pub mod crawler;
pub mod epub;
pub mod exporter;
pub mod filename_template;
pub mod illust_crawler;
pub mod sources;
pub mod task_manager;

pub use crawler::{EventSink, IllustCrawlArgs, NovelCrawlArgs};
pub use task_manager::{TaskControls, TaskManager};
