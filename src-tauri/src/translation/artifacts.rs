//! 翻译产物 schema 与幂等读写。
//!
//! 本模块只依赖一个极小的 [`ArtifactIo`] 抽象，不关心 Tauri、路径根或文件系统；
//! 生产环境由 U2/U4 用 Job 存储实现，离线测试用 [`MemoryIo`]。
//!
//! 所有产物都是带 `schema: 2` 与 `kind` 的 JSON 信封：
//! - schema 不匹配、kind 不匹配、字段缺失或 JSON 损坏都会返回可识别错误；
//! - 写入全部经 `write_atomic`，不会留下部分文件；
//! - 同一版本覆盖写入是幂等的；draft 拒绝版本回退。

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// 便于 U4 从 `translation::artifacts` 一次性取到全部 schema 类型。
pub use crate::translation::chunking::Chunk;
pub use crate::translation::source::{
    SegmentKind, SourceDoc, SourceLine, SourcePage, SourceSegment,
};

/// 当前产物 schema 版本。
pub const SCHEMA_VERSION: u32 = 2;

const ERR_SCHEMA: &str = "artifact_schema_mismatch";
const ERR_KIND: &str = "artifact_kind_mismatch";
const ERR_INVALID: &str = "artifact_invalid";
const ERR_INVALID_ID: &str = "artifact_invalid_id";
const ERR_VERSION: &str = "draft_version_regression";

const KIND_SOURCE: &str = "source";
const KIND_CHUNK: &str = "chunk";
const KIND_SCAN: &str = "scan";
const KIND_CONSTRAINTS: &str = "constraints";
const KIND_DRAFT: &str = "draft";
const KIND_REVIEW: &str = "review";
const KIND_MERGE_REPORT: &str = "merge-report";
const KIND_RESULT: &str = "result";
const KIND_GLOBAL_REVIEW: &str = "global-review";

/// 绝对路径规则由宿主负责；这里只处理 Job 相对路径。
pub trait ArtifactIo {
    /// 读取文件；不存在返回 `Ok(None)`。
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String>;
    /// 原子写入：要么完整替换，要么保持旧内容。
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> Result<(), String>;
    fn exists(&self, path: &str) -> bool;
    /// 列出目录下直接子文件名（不含目录前缀），顺序不限。
    fn list(&self, dir: &str) -> Result<Vec<String>, String>;
}

// ----------------------------------------------------------------------
// 共享 schema 类型
// ----------------------------------------------------------------------

/// token 用量（v2 字段为 input / output / total）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.total = self.input.saturating_add(self.output);
    }
}

/// 扫描置信度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// v2 术语：字段与模型工具 schema 一致。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTerm {
    pub source_term: String,
    pub locked_translation: String,
    pub term_type: String,
    pub aliases: Vec<String>,
    pub notes: String,
}

/// 单块扫描记录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanNote {
    pub chunk_id: String,
    pub terms: Vec<AgentTerm>,
    pub style: String,
    pub notes: String,
    pub confidence: Confidence,
}

/// 术语冲突记录；不静默丢弃。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub source_term: String,
    pub existing_translation: String,
    pub candidate_translation: String,
    pub chunk_id: String,
    pub notes: String,
}

/// 冻结后的约束版本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraints {
    pub version: u64,
    pub terms: Vec<AgentTerm>,
    pub style: String,
    pub conflicts: Vec<Conflict>,
    pub unresolved: Vec<String>,
    pub frozen: bool,
}

/// 单片段译文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedSegment {
    pub id: String,
    pub text: String,
}

/// 单块译文草稿。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub chunk_id: String,
    pub version: u64,
    pub segments: Vec<TranslatedSegment>,
    pub usage: TokenUsage,
    pub reviewed: bool,
    pub needs_repair: bool,
}

/// 审查结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    NeedsFix,
}

/// 问题严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Major,
    Minor,
}

/// 单条审查问题。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub segment_id: String,
    pub kind: String,
    pub severity: Severity,
    pub detail: String,
}

impl Issue {
    /// 是否为阻断发布的严重问题。
    pub fn is_blocking(&self) -> bool {
        matches!(self.severity, Severity::Critical | Severity::Major)
    }
}

/// 单块审查记录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub chunk_id: String,
    pub draft_version: u64,
    pub verdict: ReviewVerdict,
    pub issues: Vec<Issue>,
    pub notes: String,
}

/// 阅读器投影的一行译文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedLine {
    pub line: u32,
    pub text: String,
}

/// 发布状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishStatus {
    Complete,
    Partial,
    NeedsAttention,
}

/// 合并校验报告。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeReport {
    pub coverage_ok: bool,
    pub order_ok: bool,
    pub duplicate_ids: Vec<String>,
    pub missing_ids: Vec<String>,
    pub markup_issues: Vec<Issue>,
}

/// 整体复核记录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalReview {
    pub passed: bool,
    pub sample_chunk_ids: Vec<String>,
    pub notes: String,
}

/// 正式阅读器结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationResult {
    pub novel_id: i64,
    pub job_id: String,
    pub target_language: String,
    pub pages: BTreeMap<u32, Vec<TranslatedLine>>,
    pub status: PublishStatus,
    pub unresolved: u32,
}

// ----------------------------------------------------------------------
// 固定路径
// ----------------------------------------------------------------------

pub const SOURCE_PATH: &str = "source/source.json";
pub const CONSTRAINTS_PATH: &str = "preparation/constraints.json";
pub const MERGE_REPORT_PATH: &str = "reviews/merge-report.json";
pub const RESULT_PATH: &str = "output/result.json";
pub const GLOBAL_REVIEW_PATH: &str = "output/global-review.json";

pub fn chunk_source_path(chunk_id: &str) -> String {
    format!("chunks/{chunk_id}.src.json")
}

pub fn scan_path(chunk_id: &str) -> String {
    format!("preparation/scan-{chunk_id}.json")
}

pub fn draft_path(chunk_id: &str) -> String {
    format!("chunks/{chunk_id}.draft.json")
}

pub fn review_path(chunk_id: &str) -> String {
    format!("reviews/{chunk_id}.json")
}

/// 只接受 `c` + 至少四位数字，杜绝路径穿越与误用。
pub fn validate_chunk_id(chunk_id: &str) -> Result<(), String> {
    let digits = chunk_id
        .strip_prefix('c')
        .ok_or_else(|| format!("{ERR_INVALID_ID}: {chunk_id}"))?;
    if digits.len() < 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{ERR_INVALID_ID}: {chunk_id}"));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// 内存实现（测试 / 离线运行）
// ----------------------------------------------------------------------

/// 内存文件系统：BTreeMap 保证遍历顺序稳定，写入即整体替换。
#[derive(Default)]
pub struct MemoryIo {
    files: RwLock<BTreeMap<String, Vec<u8>>>,
}

impl MemoryIo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn paths(&self) -> Vec<String> {
        match self.files.read() {
            Ok(files) => files.keys().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        match self.files.read() {
            Ok(files) => files.clone(),
            Err(_) => BTreeMap::new(),
        }
    }

    pub fn clear(&self) {
        if let Ok(mut files) = self.files.write() {
            files.clear();
        }
    }
}

impl ArtifactIo for MemoryIo {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let files = self
            .files
            .read()
            .map_err(|_| "memory_io_poisoned".to_string())?;
        Ok(files.get(path).cloned())
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let mut files = self
            .files
            .write()
            .map_err(|_| "memory_io_poisoned".to_string())?;
        files.insert(path.to_string(), bytes.to_vec());
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        self.files
            .read()
            .map(|files| files.contains_key(path))
            .unwrap_or(false)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, String> {
        let prefix = format!("{dir}/");
        let files = self
            .files
            .read()
            .map_err(|_| "memory_io_poisoned".to_string())?;
        let mut names: Vec<String> = files
            .keys()
            .filter_map(|path| {
                let rest = path.strip_prefix(&prefix)?;
                if rest.contains('/') {
                    None
                } else {
                    Some(rest.to_string())
                }
            })
            .collect();
        names.sort();
        Ok(names)
    }
}

// ----------------------------------------------------------------------
// Job 产物读写
// ----------------------------------------------------------------------

/// 绑定到某个 Job 根目录的产物访问器。
pub struct JobArtifacts<'a> {
    io: &'a dyn ArtifactIo,
}

impl<'a> JobArtifacts<'a> {
    pub fn new(io: &'a dyn ArtifactIo) -> Self {
        Self { io }
    }

    pub fn io(&self) -> &dyn ArtifactIo {
        self.io
    }

    // ---- source ----

    pub fn write_source(&self, doc: &SourceDoc) -> Result<(), String> {
        self.write_json(SOURCE_PATH, KIND_SOURCE, doc)
    }

    pub fn read_source(&self) -> Result<Option<SourceDoc>, String> {
        self.read_json(SOURCE_PATH, KIND_SOURCE)
    }

    // ---- chunks ----

    pub fn write_chunk(&self, chunk: &Chunk) -> Result<(), String> {
        validate_chunk_id(&chunk.id)?;
        self.write_json(&chunk_source_path(&chunk.id), KIND_CHUNK, chunk)
    }

    pub fn read_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, String> {
        validate_chunk_id(chunk_id)?;
        self.read_json(&chunk_source_path(chunk_id), KIND_CHUNK)
    }

    /// 已落盘的块 ID，按数字顺序排列。
    pub fn list_chunks(&self) -> Result<Vec<String>, String> {
        let mut ids: Vec<String> = self
            .io
            .list("chunks")?
            .into_iter()
            .filter_map(|name| {
                let id = name.strip_suffix(".src.json")?;
                validate_chunk_id(id).ok()?;
                Some(id.to_string())
            })
            .collect();
        ids.sort_by_key(|id| {
            id.strip_prefix('c')
                .and_then(|digits| digits.parse::<u32>().ok())
                .unwrap_or(u32::MAX)
        });
        Ok(ids)
    }

    // ---- scan / constraints ----

    pub fn write_scan(&self, note: &ScanNote) -> Result<(), String> {
        validate_chunk_id(&note.chunk_id)?;
        self.write_json(&scan_path(&note.chunk_id), KIND_SCAN, note)
    }

    pub fn read_scan(&self, chunk_id: &str) -> Result<Option<ScanNote>, String> {
        validate_chunk_id(chunk_id)?;
        self.read_json(&scan_path(chunk_id), KIND_SCAN)
    }

    pub fn write_constraints(&self, constraints: &Constraints) -> Result<(), String> {
        self.write_json(CONSTRAINTS_PATH, KIND_CONSTRAINTS, constraints)
    }

    pub fn read_constraints(&self) -> Result<Option<Constraints>, String> {
        self.read_json(CONSTRAINTS_PATH, KIND_CONSTRAINTS)
    }

    // ---- drafts ----

    /// 写入草稿：同版本幂等覆盖，拒绝版本回退。
    pub fn write_draft(&self, draft: &Draft) -> Result<(), String> {
        validate_chunk_id(&draft.chunk_id)?;
        if let Some(existing) = self.read_draft(&draft.chunk_id)? {
            if draft.version < existing.version {
                return Err(format!(
                    "{ERR_VERSION}: {}: {} < {}",
                    draft.chunk_id, draft.version, existing.version
                ));
            }
        }
        self.write_json(&draft_path(&draft.chunk_id), KIND_DRAFT, draft)
    }

    pub fn read_draft(&self, chunk_id: &str) -> Result<Option<Draft>, String> {
        validate_chunk_id(chunk_id)?;
        self.read_json(&draft_path(chunk_id), KIND_DRAFT)
    }

    // ---- reviews ----

    pub fn write_review(&self, review: &Review) -> Result<(), String> {
        validate_chunk_id(&review.chunk_id)?;
        self.write_json(&review_path(&review.chunk_id), KIND_REVIEW, review)
    }

    pub fn read_review(&self, chunk_id: &str) -> Result<Option<Review>, String> {
        validate_chunk_id(chunk_id)?;
        self.read_json(&review_path(chunk_id), KIND_REVIEW)
    }

    pub fn write_merge_report(&self, report: &MergeReport) -> Result<(), String> {
        self.write_json(MERGE_REPORT_PATH, KIND_MERGE_REPORT, report)
    }

    pub fn read_merge_report(&self) -> Result<Option<MergeReport>, String> {
        self.read_json(MERGE_REPORT_PATH, KIND_MERGE_REPORT)
    }

    // ---- output ----

    pub fn write_result(&self, result: &TranslationResult) -> Result<(), String> {
        self.write_json(RESULT_PATH, KIND_RESULT, result)
    }

    pub fn read_result(&self) -> Result<Option<TranslationResult>, String> {
        self.read_json(RESULT_PATH, KIND_RESULT)
    }

    pub fn write_global_review(&self, review: &GlobalReview) -> Result<(), String> {
        self.write_json(GLOBAL_REVIEW_PATH, KIND_GLOBAL_REVIEW, review)
    }

    pub fn read_global_review(&self) -> Result<Option<GlobalReview>, String> {
        self.read_json(GLOBAL_REVIEW_PATH, KIND_GLOBAL_REVIEW)
    }

    // ---- JSON 信封 ----

    fn write_json<T: Serialize>(&self, path: &str, kind: &str, data: &T) -> Result<(), String> {
        #[derive(Serialize)]
        struct Envelope<'a, T> {
            schema: u32,
            kind: &'a str,
            data: &'a T,
        }

        let raw = serde_json::to_vec(&Envelope {
            schema: SCHEMA_VERSION,
            kind,
            data,
        })
        .map_err(|_| format!("{ERR_INVALID}: {path}"))?;
        self.io.write_atomic(path, &raw)
    }

    fn read_json<T: DeserializeOwned>(
        &self,
        path: &str,
        expected_kind: &str,
    ) -> Result<Option<T>, String> {
        let Some(raw) = self.io.read(path)? else {
            return Ok(None);
        };
        let value: Value =
            serde_json::from_slice(&raw).map_err(|_| format!("{ERR_INVALID}: {path}"))?;
        let schema = value
            .get("schema")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{ERR_INVALID}: {path}"))?;
        if schema != u64::from(SCHEMA_VERSION) {
            return Err(format!(
                "{ERR_SCHEMA}: {path}: found {schema}, expected {SCHEMA_VERSION}"
            ));
        }
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{ERR_INVALID}: {path}"))?;
        if kind != expected_kind {
            return Err(format!("{ERR_KIND}: {path}"));
        }
        let data = value
            .get("data")
            .ok_or_else(|| format!("{ERR_INVALID}: {path}"))?;
        serde_json::from_value(data.clone())
            .map(Some)
            .map_err(|_| format!("{ERR_INVALID}: {path}"))
    }
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::chunking::Chunk;
    use crate::translation::source::{
        SegmentKind, SourceLine, SourcePage, SourceSegment, segment_id,
    };

    fn sample_doc() -> SourceDoc {
        SourceDoc {
            pages: vec![SourcePage {
                page: 1,
                text: "[chapter:序]\n正文".into(),
                lines: vec![
                    SourceLine {
                        line: 0,
                        text: "[chapter:序]".into(),
                        segments: vec![SourceSegment {
                            id: segment_id(1, 0, 0),
                            text: "[chapter:序]".into(),
                            kind: SegmentKind::Chapter,
                        }],
                    },
                    SourceLine {
                        line: 1,
                        text: "正文".into(),
                        segments: vec![SourceSegment {
                            id: segment_id(1, 1, 0),
                            text: "正文".into(),
                            kind: SegmentKind::Text,
                        }],
                    },
                ],
            }],
            input_hash: "hash".into(),
            target_language: "zh-CN".into(),
        }
    }

    fn sample_chunk() -> Chunk {
        Chunk {
            id: "c0001".into(),
            order: 1,
            segments: sample_doc().pages[0].lines[1].segments.clone(),
            prev_context: String::new(),
            next_context: String::new(),
            chars: 2,
        }
    }

    fn sample_draft() -> Draft {
        Draft {
            chunk_id: "c0001".into(),
            version: 1,
            segments: vec![TranslatedSegment {
                id: "p001-l001-s0".into(),
                text: "译文".into(),
            }],
            usage: TokenUsage {
                input: 10,
                output: 5,
                total: 15,
            },
            reviewed: false,
            needs_repair: false,
        }
    }

    fn sample_review() -> Review {
        Review {
            chunk_id: "c0001".into(),
            draft_version: 1,
            verdict: ReviewVerdict::Pass,
            issues: vec![],
            notes: String::new(),
        }
    }

    fn sample_result() -> TranslationResult {
        TranslationResult {
            novel_id: 1,
            job_id: "20260101000000-abcdef".into(),
            target_language: "zh-CN".into(),
            pages: BTreeMap::from([(
                1,
                vec![TranslatedLine {
                    line: 1,
                    text: "译文".into(),
                }],
            )]),
            status: PublishStatus::Partial,
            unresolved: 0,
        }
    }

    #[test]
    fn source_roundtrip() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);
        let doc = sample_doc();
        artifacts.write_source(&doc).unwrap();
        assert_eq!(artifacts.read_source().unwrap(), Some(doc));
    }

    #[test]
    fn every_artifact_type_roundtrips() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);

        let scan = ScanNote {
            chunk_id: "c0001".into(),
            terms: vec![AgentTerm {
                source_term: "勇者".into(),
                locked_translation: "勇者".into(),
                term_type: "character".into(),
                aliases: vec!["勇気".into()],
                notes: String::new(),
            }],
            style: "简洁".into(),
            notes: "备注".into(),
            confidence: Confidence::Medium,
        };
        artifacts.write_scan(&scan).unwrap();
        assert_eq!(artifacts.read_scan("c0001").unwrap(), Some(scan));

        let constraints = Constraints {
            version: 2,
            terms: vec![AgentTerm {
                source_term: "勇者".into(),
                locked_translation: "勇者".into(),
                term_type: "character".into(),
                aliases: vec![],
                notes: String::new(),
            }],
            style: "简洁".into(),
            conflicts: vec![Conflict {
                source_term: "勇者".into(),
                existing_translation: "勇者".into(),
                candidate_translation: "英雄".into(),
                chunk_id: "c0002".into(),
                notes: "保留既有译名".into(),
            }],
            unresolved: vec![],
            frozen: true,
        };
        artifacts.write_constraints(&constraints).unwrap();
        assert_eq!(artifacts.read_constraints().unwrap(), Some(constraints));

        let review = sample_review();
        artifacts.write_review(&review).unwrap();
        assert_eq!(artifacts.read_review("c0001").unwrap(), Some(review));

        let report = MergeReport {
            coverage_ok: true,
            order_ok: true,
            duplicate_ids: vec![],
            missing_ids: vec![],
            markup_issues: vec![],
        };
        artifacts.write_merge_report(&report).unwrap();
        assert_eq!(artifacts.read_merge_report().unwrap(), Some(report));

        let result = sample_result();
        artifacts.write_result(&result).unwrap();
        assert_eq!(artifacts.read_result().unwrap(), Some(result));

        let global = GlobalReview {
            passed: true,
            sample_chunk_ids: vec!["c0001".into()],
            notes: "抽样通过".into(),
        };
        artifacts.write_global_review(&global).unwrap();
        assert_eq!(artifacts.read_global_review().unwrap(), Some(global));
    }

    #[test]
    fn all_paths_are_fixed() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);
        artifacts.write_source(&sample_doc()).unwrap();
        artifacts.write_chunk(&sample_chunk()).unwrap();
        artifacts
            .write_scan(&ScanNote {
                chunk_id: "c0001".into(),
                terms: vec![],
                style: "简洁".into(),
                notes: String::new(),
                confidence: Confidence::High,
            })
            .unwrap();
        artifacts
            .write_constraints(&Constraints {
                version: 1,
                terms: vec![],
                style: "简洁".into(),
                conflicts: vec![],
                unresolved: vec![],
                frozen: true,
            })
            .unwrap();
        artifacts.write_draft(&sample_draft()).unwrap();
        artifacts.write_review(&sample_review()).unwrap();
        artifacts
            .write_merge_report(&MergeReport {
                coverage_ok: true,
                order_ok: true,
                ..MergeReport::default()
            })
            .unwrap();
        artifacts.write_result(&sample_result()).unwrap();
        artifacts
            .write_global_review(&GlobalReview {
                passed: true,
                sample_chunk_ids: vec!["c0001".into()],
                notes: String::new(),
            })
            .unwrap();

        let expected = vec![
            "chunks/c0001.draft.json",
            "chunks/c0001.src.json",
            "output/global-review.json",
            "output/result.json",
            "preparation/constraints.json",
            "preparation/scan-c0001.json",
            "reviews/c0001.json",
            "reviews/merge-report.json",
            "source/source.json",
        ];
        assert_eq!(io.paths(), expected);
    }

    #[test]
    fn chunk_draft_roundtrip_and_listing() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);
        artifacts.write_chunk(&sample_chunk()).unwrap();
        let mut second = sample_chunk();
        second.id = "c0002".into();
        second.order = 2;
        artifacts.write_chunk(&second).unwrap();

        assert_eq!(artifacts.list_chunks().unwrap(), vec!["c0001", "c0002"]);
        assert_eq!(artifacts.read_chunk("c0002").unwrap(), Some(second));

        let draft = sample_draft();
        artifacts.write_draft(&draft).unwrap();
        assert_eq!(artifacts.read_draft("c0001").unwrap(), Some(draft));
    }

    #[test]
    fn schema_mismatch_is_recognizable() {
        let io = MemoryIo::new();
        io.write_atomic(
            SOURCE_PATH,
            br#"{"schema":1,"kind":"source","data":{"pages":[],"input_hash":"x","target_language":"zh-CN"}}"#,
        )
        .unwrap();
        let artifacts = JobArtifacts::new(&io);
        let error = artifacts.read_source().unwrap_err();
        assert!(error.contains(ERR_SCHEMA), "错误应可识别：{error}");
    }

    #[test]
    fn kind_mismatch_and_missing_fields_are_recognizable() {
        let io = MemoryIo::new();
        io.write_atomic(
            SOURCE_PATH,
            br#"{"schema":2,"kind":"chunk","data":{"id":"c0001"}}"#,
        )
        .unwrap();
        let artifacts = JobArtifacts::new(&io);
        let error = artifacts.read_source().unwrap_err();
        assert!(error.contains(ERR_KIND), "错误应可识别：{error}");

        io.write_atomic(
            SOURCE_PATH,
            br#"{"schema":2,"kind":"source","data":{"pages":[]}}"#,
        )
        .unwrap();
        let error = artifacts.read_source().unwrap_err();
        assert!(error.contains(ERR_INVALID), "字段缺失应可识别：{error}");
    }

    #[test]
    fn same_version_write_is_idempotent_and_regression_is_rejected() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);

        let mut draft = sample_draft();
        artifacts.write_draft(&draft).unwrap();
        draft.segments[0].text = "第二版".into();
        artifacts.write_draft(&draft).unwrap();
        assert_eq!(
            artifacts.read_draft("c0001").unwrap().unwrap().segments[0].text,
            "第二版"
        );

        let mut older = sample_draft();
        older.version = 0;
        let error = artifacts.write_draft(&older).unwrap_err();
        assert!(error.contains(ERR_VERSION), "错误应可识别：{error}");
        assert_eq!(
            artifacts.read_draft("c0001").unwrap().unwrap().version,
            1,
            "版本回退不得覆盖现有草稿"
        );
    }

    #[test]
    fn invalid_chunk_id_is_rejected_before_io() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);
        let error = artifacts.read_chunk("../source").unwrap_err();
        assert!(error.contains(ERR_INVALID_ID));
        assert!(io.paths().is_empty(), "非法 ID 不得产生写操作");
    }

    #[test]
    fn missing_file_reads_none() {
        let io = MemoryIo::new();
        let artifacts = JobArtifacts::new(&io);
        assert!(artifacts.read_source().unwrap().is_none());
        assert!(artifacts.read_constraints().unwrap().is_none());
        assert!(artifacts.read_result().unwrap().is_none());
    }

    struct FailingIo<'a>(&'a MemoryIo);

    impl ArtifactIo for FailingIo<'_> {
        fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
            self.0.read(path)
        }

        fn write_atomic(&self, _path: &str, _bytes: &[u8]) -> Result<(), String> {
            Err("simulated_write_failure".into())
        }

        fn exists(&self, path: &str) -> bool {
            self.0.exists(path)
        }

        fn list(&self, dir: &str) -> Result<Vec<String>, String> {
            self.0.list(dir)
        }
    }

    #[test]
    fn failed_atomic_write_keeps_old_content() {
        let io = MemoryIo::new();
        let good = JobArtifacts::new(&io);
        good.write_source(&sample_doc()).unwrap();
        let before = io.snapshot();

        let failing = FailingIo(&io);
        let bad = JobArtifacts::new(&failing);
        let mut changed = sample_doc();
        changed.target_language = "en".into();
        assert!(bad.write_source(&changed).is_err());
        assert_eq!(io.snapshot(), before, "失败写入不得产生部分文件");
    }

    #[test]
    fn memory_io_list_returns_direct_children_only() {
        let io = MemoryIo::new();
        io.write_atomic("chunks/c0001.src.json", b"{}").unwrap();
        io.write_atomic("chunks/nested/c0002.src.json", b"{}")
            .unwrap();
        assert_eq!(io.list("chunks").unwrap(), vec!["c0001.src.json"]);
    }
}
