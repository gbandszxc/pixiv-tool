//! v1 单页两轮翻译的只读兼容层。
//!
//! 旧缓存（`data/translations/<novel_id>/<hash>.json`）只作为只读回退：
//! - 不参与 v2 的审查 / 发布判定，绝不冒充「新流程已审结果」；
//! - 统一投影为 [`PublishStatus::Partial`]，由阅读器自行标注「未审查」；
//! - 由现有 `clear_cache("translation")` 一起清除，v2 不引用其中任何文件。
//!
//! 旧实现的真实逻辑仍在 `crate::translation_legacy`，本模块只做路径发现、
//! 投影与错误归一，不复制其两轮流程。

use std::{collections::BTreeMap, path::Path};

use crate::translation::artifacts::{PublishStatus, TranslatedLine, TranslationResult};

pub use crate::translation_legacy::{
    NovelInput, StoryBible, Term, TranslationBook, TranslatedLine as LegacyTranslatedLine,
};

/// 扫描某本小说的 v1 缓存目录，返回文件路径（按修改时间倒序，最新在前）。
///
/// 旧版文件名是 `NovelInput` 的 SHA-256，只有命令层同时拿到原文才能精确命中，
/// 因此这里只做目录扫描，不解析文件名语义。
pub fn book_files(data_dir: &Path, novel_id: i64) -> Vec<std::path::PathBuf> {
    let dir = data_dir.join("translations").join(novel_id.to_string());
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, path))
        })
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files.into_iter().map(|(_, path)| path).collect()
}

/// 读取最新的 v1 缓存；目录不存在或没有可读记录时返回 `Ok(None)`。
pub fn read_latest_book(data_dir: &Path, novel_id: i64) -> Result<Option<TranslationBook>, String> {
    for path in book_files(data_dir, novel_id) {
        match std::fs::read(&path) {
            Ok(raw) => match serde_json::from_slice::<TranslationBook>(&raw) {
                Ok(book) => return Ok(Some(book)),
                Err(_) => continue,
            },
            Err(_) => continue,
        }
    }
    Ok(None)
}

/// 把 v1 缓存投影为阅读器结果；空记录返回 `None`。
pub fn to_partial_result(
    novel_id: i64,
    target_language: &str,
    book: &TranslationBook,
) -> Option<TranslationResult> {
    if book.pages.is_empty() {
        return None;
    }
    let pages: BTreeMap<u32, Vec<TranslatedLine>> = book
        .pages
        .iter()
        .filter_map(|(page, lines)| {
            let page = u32::try_from(*page).ok()?;
            let lines: Vec<TranslatedLine> = lines
                .iter()
                .filter_map(|line| {
                    Some(TranslatedLine {
                        line: u32::try_from(line.line).ok()?,
                        text: line.text.clone(),
                    })
                })
                .collect();
            Some((page, lines))
        })
        .collect();
    Some(TranslationResult {
        novel_id,
        job_id: String::new(),
        target_language: target_language.to_string(),
        pages,
        status: PublishStatus::Partial,
        unresolved: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_book() -> TranslationBook {
        TranslationBook {
            bible: StoryBible::default(),
            pages: BTreeMap::from([(
                1,
                vec![LegacyTranslatedLine {
                    line: 2,
                    text: "旧译文".into(),
                }],
            )]),
        }
    }

    #[test]
    fn projection_is_partial_and_maps_line_numbers() {
        let result = to_partial_result(7, "zh-CN", &fixture_book()).expect("有内容");
        assert_eq!(result.status, PublishStatus::Partial, "v1 结果不得冒充已完成");
        assert_eq!(result.target_language, "zh-CN");
        assert_eq!(result.pages[&1][0].line, 2);
        assert_eq!(result.pages[&1][0].text, "旧译文");
        assert!(to_partial_result(7, "zh-CN", &TranslationBook::default()).is_none());
    }

    #[test]
    fn latest_book_is_read_from_disk_and_ignores_broken_files() {
        let dir = std::env::temp_dir().join(format!("pixiv-legacy-{}", uuid::Uuid::new_v4()));
        let novel_dir = dir.join("translations").join("42");
        std::fs::create_dir_all(&novel_dir).unwrap();
        std::fs::write(novel_dir.join("broken.json"), b"not json").unwrap();
        let book = fixture_book();
        std::fs::write(
            novel_dir.join("good.json"),
            serde_json::to_vec(&book).unwrap(),
        )
        .unwrap();
        let loaded = read_latest_book(&dir, 42).unwrap().expect("能读到旧缓存");
        assert_eq!(loaded.pages[&1][0].text, "旧译文");
        assert!(read_latest_book(&dir, 99).unwrap().is_none(), "没有目录时返回 None");
        let _ = std::fs::remove_dir_all(dir);
    }
}
