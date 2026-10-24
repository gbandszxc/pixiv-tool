//! 整篇小说翻译（v2）：基于通用 Agent 内核的 Job 化流程。
//!
//! 旧版单页两轮实现保留在 `translation_legacy`，仅用于 v1 缓存只读兼容；
//! 命令路径通过本模块的再导出保持稳定。

pub use crate::translation_legacy::*;

pub mod artifacts;
pub mod chunking;
pub mod legacy;
pub mod publish;
pub mod service;
pub mod source;
pub mod tools;
pub mod validation;
