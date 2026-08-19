//! 业务核心：来源解析（sources）、爬虫（crawler / illust_crawler）、
//! 导出（exporter）、任务状态机（task_manager）。

pub mod crawler;
pub mod exporter;
pub mod illust_crawler;
pub mod sources;
pub mod task_manager;

pub use crawler::{EventSink, IllustCrawlArgs, NovelCrawlArgs};
pub use task_manager::{TaskControls, TaskManager};
