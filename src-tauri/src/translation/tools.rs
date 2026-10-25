//! 三角色工具集：协调者、翻译子 Agent、审查子 Agent。
//!
//! 工具名、参数与返回严格对齐内置资源（`resources/agent/**`）：
//! - 协调者：`load_resource` / `prepare_source` / `read_chunk_summary` / `read_chunk` /
//!   `submit_scan` / `freeze_constraints` / `dispatch_translation` / `dispatch_review` /
//!   `read_review_summary` / `merge_drafts` / `record_global_review` / `publish` /
//!   `reset_phase` / `job_status`
//! - 翻译：`load_resource` / `read_chunk` / `submit_translation`
//! - 审查：`load_resource` / `read_chunk` / `read_translation` / `submit_review`
//!
//! 所有工具按 Job 隔离、按调用幂等；无效参数、未知分块、版本回退、未冻结约束都会
//! 在产生副作用之前返回结构化错误（JSON：`{"error":{"code","message"}}`）。
//!
//! 产物访问集中在同步辅助函数里（`JobArtifacts` 借用非 `Send` 的 IO 抽象，不能跨
//! `await` 持有）；派发工具用 [`runtime::run`] 启动独立子运行，每个子 Agent 有自己
//! 的模型句柄、工具集与会话，等待子任务时不持有模型许可。单个块失败不终止其他块。

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use chrono::Utc;
use rig_core::{
    message::ToolName,
    serde_json::{Value, json},
    tool::{DynamicTool, ToolExecutionError, ToolOutput},
};
use serde::{Deserialize, Serialize};

use crate::{
    agent::{
        CancelToken, FailureClass, Phase, Role, RunError, RunRequest, TokenUsage,
        budget::{TOOL_RESULT_TOKEN_LIMIT, estimate_tokens, truncate_to_tokens},
        events::{safe_text, warn_job},
        recovery::{RetryPolicy, backoff},
        resources::{load_resource, role_prompt, system_prompt},
        runtime,
    },
    translation::{
        artifacts::{
            self, AgentTerm, Chunk, Confidence, Conflict, Constraints, Draft, GlobalReview, Issue,
            MergeReport, PublishStatus, Review, ReviewVerdict, ScanNote, Severity,
            TranslatedSegment, TranslationResult,
        },
        chunking::chunk_source_with_warnings,
        publish::{publish_status_with_language, render_markdown},
        service::{JobContext, read_input},
        source::{SegmentKind, SourceSegment, parse_segment_id, parse_source},
        validation::{merge_pages, validate_draft_with_language, validate_merge_with_language},
    },
};

// ----------------------------------------------------------------------
// 工具契约摘要（拼进对应角色的系统提示）
// ----------------------------------------------------------------------

/// 协调者工具契约（与 SKILL.md §3.2 一致）。
pub const COORDINATOR_TOOL_CONTRACT: &str = "\
工具契约（参数名与返回字段固定，不得另造）：
- load_resource(resource_id) → 文本；
- prepare_source() → {pages, chunks, segments, budget}；
- read_chunk_summary(chunk_ids[]) → 每块摘要（不含全文）；
- read_chunk(chunk_id, include_context) → {segments, context}；
- submit_scan(chunk_id, terms[], style, notes, confidence) → 已提交或冲突提示；
- freeze_constraints() → {version, terms, style, conflicts, unresolved}；
- dispatch_translation(chunk_ids[]) → 每块 {chunk_id, status, usage, issue_count}；
- dispatch_review(chunk_ids[]) → 每块 {chunk_id, status, usage, issue_count}；
- read_review_summary(chunk_ids?) → 问题汇总与受影响块；
- merge_drafts() → {coverage_ok, order_ok, duplicate_ids, missing_ids, markup_issues}；
- record_global_review(sample_chunk_ids[], notes, passed)；
- publish() → {published, status, unresolved}；
- reset_phase(phase) → 使该阶段及下游产物失效；
- job_status() → 状态、阶段、计数、预算。
派发工具返回 status=ok 才计为完成；paused / awaiting_confirmation / fatal 时立即停止派发。";

/// 翻译子 Agent 工具契约。
pub const TRANSLATOR_TOOL_CONTRACT: &str = "\
工具契约：
- load_resource(resource_id)：仅 role:translator、skill:smart-translator、reference:domain-*；
- read_chunk(chunk_id, include_context) → {segments, context}；
- submit_translation(chunk_id, segments:[{id, text}]) → 提交结果。
必须先 read_chunk 再 submit_translation；每个输入 segment ID 恰好出现一次且顺序一致。";

/// 审查子 Agent 工具契约。
pub const REVIEWER_TOOL_CONTRACT: &str = "\
工具契约：
- load_resource(resource_id)：仅 role:reviewer、skill:smart-translator、reference:domain-*；
- read_chunk(chunk_id, include_context) → {segments, context}；
- read_translation(chunk_id) → {draft_version, segments, constraints_version, terms, style}；
- submit_review(chunk_id, verdict, issues:[{segment_id, kind, severity, detail}], notes)。
verdict=pass 时 issues 必须为空；任何已记录问题都使 verdict=needs_fix。";

// ----------------------------------------------------------------------
// 分块计划与阶段版本
// ----------------------------------------------------------------------

/// 分块计划：有效块顺序 + 细分谱系。合并 / 发布只认这里列出的块。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkPlan {
    /// 写入时的 `prepare_source` 阶段版本。
    pub version: u64,
    pub entries: Vec<PlanEntry>,
}

/// 计划项。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanEntry {
    pub id: String,
    pub order: u32,
    /// 细分来源块；顶层块为 `None`。
    pub parent: Option<String>,
    /// 细分深度（顶层为 0）。
    pub depth: u32,
}

/// 阶段版本：重置时把目标阶段及下游 +1，上游保持不变。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseVersions {
    pub prepare_source: u64,
    pub prepare_constraints: u64,
    pub translate: u64,
    pub review: u64,
    pub global_review: u64,
}

impl Default for PhaseVersions {
    fn default() -> Self {
        Self {
            prepare_source: 1,
            prepare_constraints: 1,
            translate: 1,
            review: 1,
            global_review: 1,
        }
    }
}

impl PhaseVersions {
    pub fn get(&self, phase: Phase) -> u64 {
        match phase {
            Phase::PrepareSource => self.prepare_source,
            Phase::PrepareConstraints => self.prepare_constraints,
            Phase::Translate | Phase::Repair => self.translate,
            Phase::Review => self.review,
            Phase::GlobalReview | Phase::Publish => self.global_review,
        }
    }

    /// 目标阶段与全部下游 +1（上游不变）。
    pub fn reset(&mut self, phase: Phase) {
        let start = match phase {
            Phase::PrepareSource => 0,
            Phase::PrepareConstraints => 1,
            Phase::Translate | Phase::Repair => 2,
            Phase::Review => 3,
            Phase::GlobalReview | Phase::Publish => 4,
        };
        let slots: [&mut u64; 5] = [
            &mut self.prepare_source,
            &mut self.prepare_constraints,
            &mut self.translate,
            &mut self.review,
            &mut self.global_review,
        ];
        for slot in slots.into_iter().skip(start) {
            *slot = slot.saturating_add(1);
        }
    }
}

/// 分块计划路径。
pub const PLAN_PATH: &str = "preparation/chunk-plan.json";
const PHASES_PATH: &str = "checkpoints/phase-versions.json";
const RESET_NOTES_PATH: &str = "checkpoints/reset-notes.json";

fn meta_path(kind: &str, id: &str) -> String {
    match kind {
        "scan" => format!("preparation/scan-{id}.meta.json"),
        "draft" => format!("chunks/{id}.draft.meta.json"),
        "review" => format!("reviews/{id}.meta.json"),
        "global_review" => "output/global-review.meta.json".to_string(),
        "result" => "output/result.meta.json".to_string(),
        _ => format!("checkpoints/{kind}-{id}.meta.json"),
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(ctx: &JobContext, path: &str) -> Option<T> {
    let bytes = ctx.store.read(path).ok()??;
    serde_json::from_slice(&bytes).ok()
}

fn write_json<T: Serialize>(ctx: &JobContext, path: &str, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "无法序列化任务数据".to_string())?;
    ctx.store
        .write_atomic(path, &bytes)
        .map_err(|error| error.to_string())
}

/// 读取阶段版本；文件缺失时按默认（全 1）并落盘。
pub fn phase_versions(ctx: &JobContext) -> PhaseVersions {
    if let Some(versions) = read_json::<PhaseVersions>(ctx, PHASES_PATH) {
        return versions;
    }
    let versions = PhaseVersions::default();
    let _ = write_json(ctx, PHASES_PATH, &versions);
    versions
}

fn reset_phase_versions(ctx: &JobContext, phase: Phase) -> PhaseVersions {
    let mut versions = read_json::<PhaseVersions>(ctx, PHASES_PATH).unwrap_or_default();
    versions.reset(phase);
    let _ = write_json(ctx, PHASES_PATH, &versions);
    versions
}

/// 产物戳：记录写入时的阶段版本；版本不匹配即视为失效（不删除文件）。
fn stamp(ctx: &JobContext, kind: &str, id: &str, phase_version: u64) {
    let _ = write_json(
        ctx,
        &meta_path(kind, id),
        &json!({"phase_version": phase_version, "at": Utc::now().to_rfc3339()}),
    );
}

fn stamped(ctx: &JobContext, kind: &str, id: &str, phase_version: u64) -> bool {
    read_json::<Value>(ctx, &meta_path(kind, id))
        .and_then(|value| value.get("phase_version").and_then(Value::as_u64))
        .is_some_and(|version| version == phase_version)
}

/// 读取分块计划。
pub fn read_plan(ctx: &JobContext) -> Option<ChunkPlan> {
    read_json::<ChunkPlan>(ctx, PLAN_PATH)
}

fn write_plan(ctx: &JobContext, plan: &ChunkPlan) -> Result<(), String> {
    write_json(ctx, PLAN_PATH, plan)
}

// ----------------------------------------------------------------------
// 产物读写（同步；不跨 await 持有 JobArtifacts）
// ----------------------------------------------------------------------

fn read_source(ctx: &JobContext) -> Result<Option<artifacts::SourceDoc>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_source())
}

fn read_chunk(ctx: &JobContext, chunk_id: &str) -> Result<Option<Chunk>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_chunk(chunk_id))
}

fn list_chunks(ctx: &JobContext) -> Result<Vec<String>, String> {
    ctx.with_artifacts(|artifacts| artifacts.list_chunks())
}

fn write_chunk(ctx: &JobContext, chunk: &Chunk) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_chunk(chunk))
}

fn read_scan(ctx: &JobContext, chunk_id: &str) -> Result<Option<ScanNote>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_scan(chunk_id))
}

fn write_scan(ctx: &JobContext, note: &ScanNote) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_scan(note))
}

fn read_constraints(ctx: &JobContext) -> Result<Option<Constraints>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_constraints())
}

fn write_constraints(ctx: &JobContext, constraints: &Constraints) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_constraints(constraints))
}

fn read_draft(ctx: &JobContext, chunk_id: &str) -> Result<Option<Draft>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_draft(chunk_id))
}

fn write_draft(ctx: &JobContext, draft: &Draft) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_draft(draft))
}

fn read_review(ctx: &JobContext, chunk_id: &str) -> Result<Option<Review>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_review(chunk_id))
}

fn write_review(ctx: &JobContext, review: &Review) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_review(review))
}

fn read_global_review(ctx: &JobContext) -> Result<Option<GlobalReview>, String> {
    ctx.with_artifacts(|artifacts| artifacts.read_global_review())
}

fn write_global_review(ctx: &JobContext, review: &GlobalReview) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_global_review(review))
}

fn write_merge_report(ctx: &JobContext, report: &MergeReport) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_merge_report(report))
}

fn write_result(ctx: &JobContext, result: &TranslationResult) -> Result<(), String> {
    ctx.with_artifacts(|artifacts| artifacts.write_result(result))
}

/// 计划中的有效块（按计划顺序）。
pub fn plan_chunks(ctx: &JobContext) -> Result<Vec<Chunk>, String> {
    let plan = read_plan(ctx).ok_or_else(|| "尚未准备原文（缺少分块计划）".to_string())?;
    let mut chunks = Vec::with_capacity(plan.entries.len());
    for entry in &plan.entries {
        let chunk = read_chunk(ctx, &entry.id)?.ok_or_else(|| format!("分块缺失：{}", entry.id))?;
        chunks.push(chunk);
    }
    Ok(chunks)
}

/// 计划中的有效块 ID。
pub fn plan_ids(ctx: &JobContext) -> Result<Vec<String>, String> {
    Ok(plan_chunks(ctx)?
        .into_iter()
        .map(|chunk| chunk.id)
        .collect())
}

// ----------------------------------------------------------------------
// 结构化错误与结果裁剪
// ----------------------------------------------------------------------

fn tool_error(code: &str, message: &str) -> ToolExecutionError {
    ToolExecutionError::invalid_args(
        json!({"error": {"code": code, "message": message}}).to_string(),
    )
}

fn tool_error_with(code: &str, message: &str, extra: Value) -> ToolExecutionError {
    let mut payload = json!({"error": {"code": code, "message": message}});
    if let (Some(object), Some(extra)) = (payload.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    ToolExecutionError::invalid_args(payload.to_string())
}

/// 工具结果统一出口：JSON 文本 + token 上限（超限给摘要与引用，不静默丢内容）。
fn tool_result(value: Value) -> Result<ToolOutput, ToolExecutionError> {
    let text = value.to_string();
    if estimate_tokens(&text) <= TOOL_RESULT_TOKEN_LIMIT {
        return Ok(ToolOutput::text(text));
    }
    let summary = json!({
        "truncated": true,
        "token_estimate": estimate_tokens(&text),
        "limit": TOOL_RESULT_TOKEN_LIMIT,
        "summary": truncate_to_tokens(&text, TOOL_RESULT_TOKEN_LIMIT / 2),
        "hint": "结果超过单次读取上限；请缩小 chunk_ids 范围或改用摘要工具",
    });
    Ok(ToolOutput::text(summary.to_string()))
}

fn arg_str(args: &Value, key: &str) -> Result<String, ToolExecutionError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| tool_error("invalid_args", &format!("缺少参数 {key}")))
}

fn arg_str_list(args: &Value, key: &str) -> Result<Vec<String>, ToolExecutionError> {
    let list = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| tool_error("invalid_args", &format!("缺少数组参数 {key}")))?;
    let mut values = Vec::with_capacity(list.len());
    for item in list {
        let Some(value) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Err(tool_error(
                "invalid_args",
                &format!("{key} 只能包含非空字符串"),
            ));
        };
        values.push(value.to_string());
    }
    if values.is_empty() {
        return Err(tool_error("invalid_args", &format!("{key} 不能为空数组")));
    }
    Ok(values)
}

fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::Major => "major",
        Severity::Minor => "minor",
    }
}

fn verdict_str(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Pass => "pass",
        ReviewVerdict::NeedsFix => "needs_fix",
    }
}

fn confidence_str(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "high",
        Confidence::Medium => "medium",
        Confidence::Low => "low",
    }
}

fn publish_status_str(status: PublishStatus) -> &'static str {
    match status {
        PublishStatus::Complete => "complete",
        PublishStatus::Partial => "partial",
        PublishStatus::NeedsAttention => "needs_attention",
    }
}

fn segment_kind_str(kind: SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Text => "text",
        SegmentKind::Chapter => "chapter",
        SegmentKind::Ruby => "ruby",
        SegmentKind::Markup => "markup",
    }
}

fn agent_usage(usage: artifacts::TokenUsage) -> TokenUsage {
    TokenUsage {
        input: usage.input,
        output: usage.output,
        total: usage.total,
    }
}

// ----------------------------------------------------------------------
// 工具构造
// ----------------------------------------------------------------------

fn tool_name(name: &str) -> ToolName {
    ToolName::new(name).expect("工具名非空且合法")
}

/// 协调者工具集。
pub fn coordinator_tools(ctx: &std::sync::Arc<JobContext>) -> Vec<DynamicTool> {
    vec![
        load_resource_tool(),
        prepare_source_tool(ctx),
        read_chunk_summary_tool(ctx),
        read_chunk_tool(ctx),
        submit_scan_tool(ctx),
        freeze_constraints_tool(ctx),
        dispatch_tool(ctx, DispatchKind::Translate),
        dispatch_tool(ctx, DispatchKind::Review),
        read_review_summary_tool(ctx),
        merge_drafts_tool(ctx),
        record_global_review_tool(ctx),
        publish_tool(ctx),
        reset_phase_tool(ctx),
        job_status_tool(ctx),
    ]
}

/// 翻译子 Agent 工具集。
pub fn translator_tools(ctx: &std::sync::Arc<JobContext>, chunk_id: &str) -> Vec<DynamicTool> {
    vec![
        load_resource_tool(),
        read_chunk_tool(ctx),
        submit_translation_tool(ctx, chunk_id),
    ]
}

/// 审查子 Agent 工具集。
pub fn reviewer_tools(ctx: &std::sync::Arc<JobContext>, chunk_id: &str) -> Vec<DynamicTool> {
    vec![
        load_resource_tool(),
        read_chunk_tool(ctx),
        read_translation_tool(ctx),
        submit_review_tool(ctx, chunk_id),
    ]
}

fn load_resource_tool() -> DynamicTool {
    DynamicTool::new(
        tool_name("load_resource"),
        "按 ID 加载应用内置资源（skill / role / reference 词库）。不接受路径。",
        json!({
            "type": "object",
            "properties": {
                "resource_id": {"type": "string", "description": "如 skill:smart-translator、role:coordinator、reference:domain-literature"}
            },
            "required": ["resource_id"]
        }),
        move |args: Value| {
            Box::pin(async move {
                let id = arg_str(&args, "resource_id")?;
                match load_resource(&id) {
                    Ok(content) => tool_result(json!({
                        "resource_id": id,
                        "content": truncate_to_tokens(content, TOOL_RESULT_TOKEN_LIMIT),
                        "truncated": estimate_tokens(content) > TOOL_RESULT_TOKEN_LIMIT,
                    })),
                    Err(error) => Err(tool_error("unknown_resource", &error.to_string())),
                }
            })
        },
    )
}

fn prepare_source_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("prepare_source"),
        "解析整篇原文、建立页/行/片段 ID、语义切块与邻接窗口，返回页数与块数。",
        json!({"type": "object", "properties": {}, "required": []}),
        move |_args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move { prepare_source_impl(&ctx).await })
        },
    )
}

async fn prepare_source_impl(
    ctx: &std::sync::Arc<JobContext>,
) -> Result<ToolOutput, ToolExecutionError> {
    let input = read_input(&ctx.store, ctx.scope.novel_id)
        .ok_or_else(|| tool_error("missing_input", "任务内没有可用的原文记录"))?;
    let source = parse_source(&input.content, &ctx.target_language)
        .map_err(|error| tool_error("invalid_source", &error))?;
    let (chunks, warnings) = chunk_source_with_warnings(&source, &ctx.chunk_config());
    for chunk in &chunks {
        write_chunk(ctx, chunk).map_err(|error| tool_error("storage", &error))?;
    }
    let versions = phase_versions(ctx);
    let existing = read_plan(ctx);
    // 计划是否仍然有效：阶段版本一致，且**顶层块**与最新切块一一对应。
    // 顶层块一致时保留已细分的派生块（重复调用 prepare_source 不应丢弃有效的
    // 缩块结果，否则已完成的分块会被要求重做）。
    let plan_is_current = existing.as_ref().is_some_and(|plan| {
        plan.version == versions.prepare_source
            && plan
                .entries
                .iter()
                .filter(|entry| entry.parent.is_none())
                .map(|entry| entry.id.clone())
                .eq(chunks.iter().map(|chunk| chunk.id.clone()))
    });
    if !plan_is_current {
        let plan = ChunkPlan {
            version: versions.prepare_source,
            entries: chunks
                .iter()
                .map(|chunk| PlanEntry {
                    id: chunk.id.clone(),
                    order: chunk.order,
                    parent: None,
                    depth: 0,
                })
                .collect(),
        };
        write_plan(ctx, &plan).map_err(|error| tool_error("storage", &error))?;
    }
    let budget = ctx.budget.snapshot();
    let segments: usize = chunks.iter().map(|chunk| chunk.segments.len()).sum();
    let _ = ctx.update_manifest(|manifest| {
        manifest.counts.pages = source.page_count();
        manifest.counts.chunks = chunks.len() as u32;
        manifest.input_hash = source.input_hash.clone();
    });
    let _ = ctx.touch_checkpoint(Phase::PrepareSource);
    let _ = ctx.refresh_counts();
    let config = ctx.budget_config.clone().normalized();
    tool_result(json!({
        "pages": source.page_count(),
        "chunks": chunks.len(),
        "segments": segments,
        "warnings": warnings.len(),
        "budget": {
            "call_limit": budget.limit,
            "call_used": budget.used,
            "call_remaining": budget.remaining,
            "context_window": config.context_window,
            "output_limit": config.output_limit,
        },
        "chunk_ids": chunks.iter().map(|chunk| chunk.id.clone()).collect::<Vec<_>>(),
    }))
}

fn chunk_summary(ctx: &JobContext, chunk: &Chunk) -> Value {
    let scan = read_scan(ctx, &chunk.id).ok().flatten();
    let draft = read_draft(ctx, &chunk.id).ok().flatten();
    let review = read_review(ctx, &chunk.id).ok().flatten();
    let pages: BTreeSet<u32> = chunk
        .segments
        .iter()
        .filter_map(|segment| parse_segment_id(&segment.id).map(|(page, _, _)| page))
        .collect();
    json!({
        "chunk_id": chunk.id,
        "order": chunk.order,
        "pages": pages.into_iter().collect::<Vec<_>>(),
        "segments": chunk.segments.len(),
        "translatable": chunk.segments.iter().filter(|segment| segment.kind != SegmentKind::Markup).count(),
        "chars": chunk.chars,
        "scanned": scan.is_some(),
        "terms": scan.as_ref().map(|scan| scan.terms.iter().map(|term| json!({
            "source_term": term.source_term,
            "locked_translation": term.locked_translation,
            "term_type": term.term_type,
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "style": scan.as_ref().map(|scan| safe_text(&scan.style, 240)).unwrap_or_default(),
        "confidence": scan.as_ref().map(|scan| confidence_str(scan.confidence)).unwrap_or(""),
        "draft_version": draft.as_ref().map(|draft| draft.version).unwrap_or(0),
        "reviewed": draft.as_ref().map(|draft| draft.reviewed).unwrap_or(false),
        "needs_repair": draft.as_ref().map(|draft| draft.needs_repair).unwrap_or(false),
        "review_verdict": review.as_ref().map(|review| verdict_str(review.verdict)).unwrap_or(""),
        "issues": review.as_ref().map(|review| review.issues.len()).unwrap_or(0),
    })
}

fn read_chunk_summary_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("read_chunk_summary"),
        "读取一个或多个块的摘要：页、片段数、术语、风格与问题计数，不含全文。",
        json!({
            "type": "object",
            "properties": {"chunk_ids": {"type": "array", "items": {"type": "string"}}},
            "required": ["chunk_ids"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let ids = arg_str_list(&args, "chunk_ids")?;
                let mut summaries = Vec::with_capacity(ids.len());
                for id in &ids {
                    let chunk = read_chunk(&ctx, id)
                        .map_err(|error| tool_error("invalid_chunk", &error))?
                        .ok_or_else(|| {
                            tool_error_with("unknown_chunk", "分块不存在", json!({"chunk_id": id}))
                        })?;
                    summaries.push(chunk_summary(&ctx, &chunk));
                }
                tool_result(json!({"chunks": summaries}))
            })
        },
    )
}

fn read_chunk_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("read_chunk"),
        "读取一个块的原文片段与可选邻接上下文；ID 由工具生成，不得自造。",
        json!({
            "type": "object",
            "properties": {
                "chunk_id": {"type": "string"},
                "include_context": {"type": "boolean"}
            },
            "required": ["chunk_id"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let id = arg_str(&args, "chunk_id")?;
                let include_context = arg_bool(&args, "include_context", false);
                let chunk = read_chunk(&ctx, &id)
                    .map_err(|error| tool_error("invalid_chunk", &error))?
                    .ok_or_else(|| {
                        tool_error_with("unknown_chunk", "分块不存在", json!({"chunk_id": id}))
                    })?;
                let segments: Vec<Value> = chunk
                    .segments
                    .iter()
                    .map(|segment| {
                        let (page, line, _) = parse_segment_id(&segment.id).unwrap_or((0, 0, 0));
                        json!({
                            "id": segment.id,
                            "page": page,
                            "line": line,
                            "text": segment.text,
                            "kind": segment_kind_str(segment.kind),
                        })
                    })
                    .collect();
                let context = if include_context {
                    json!({"prev": chunk.prev_context, "next": chunk.next_context})
                } else {
                    json!({"prev": "", "next": ""})
                };
                tool_result(json!({
                    "chunk_id": chunk.id,
                    "order": chunk.order,
                    "segments": segments,
                    "context": context,
                }))
            })
        },
    )
}

/// 协调者工具结果大小预估器：`read_chunk` 按块字符数（含邻接窗口）估算
/// 结果 token；其余工具结果都很小，返回 0 不参与单轮预算。
pub(crate) struct ChunkResultEstimator {
    ctx: std::sync::Arc<JobContext>,
}

impl ChunkResultEstimator {
    pub(crate) fn new(ctx: std::sync::Arc<JobContext>) -> Self {
        Self { ctx }
    }
}

impl crate::agent::ToolResultEstimator for ChunkResultEstimator {
    fn estimate(&self, tool: &str, args: &str) -> u32 {
        if tool != "read_chunk" {
            return 0;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(args) else {
            return 0;
        };
        let Some(chunk_id) = args.get("chunk_id").and_then(|value| value.as_str()) else {
            return 0;
        };
        let include_context = args
            .get("include_context")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let Ok(Some(chunk)) = read_chunk(&self.ctx, chunk_id) else {
            // 无效参数走工具本身的错误返回（小结果），不参与预算。
            return 0;
        };
        let context_chars = if include_context {
            chunk.prev_context.chars().count() + chunk.next_context.chars().count()
        } else {
            0
        };
        // CJK 保守按 1 token/字符 + 10% 余量 + JSON 结构外壳。
        ((chunk.chars as usize + context_chars) * 11 / 10) as u32 + 128
    }
}

#[derive(Deserialize)]
struct ScanTermArg {
    source_term: String,
    #[serde(default)]
    locked_translation: String,
    #[serde(default)]
    term_type: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    notes: String,
}

fn submit_scan_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("submit_scan"),
        "提交单块扫描结果（术语、风格、说明与置信度）；同块重复提交为幂等替换。",
        json!({
            "type": "object",
            "properties": {
                "chunk_id": {"type": "string"},
                "terms": {"type": "array", "items": {"type": "object"}},
                "style": {},
                "notes": {"type": "string"},
                "confidence": {"type": "string", "enum": ["high", "medium", "low"]}
            },
            "required": ["chunk_id", "terms", "style", "notes", "confidence"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let chunk_id = arg_str(&args, "chunk_id")?;
                let chunk = read_chunk(&ctx, &chunk_id)
                    .map_err(|error| tool_error("invalid_chunk", &error))?
                    .ok_or_else(|| {
                        tool_error_with(
                            "unknown_chunk",
                            "分块不存在",
                            json!({"chunk_id": chunk_id}),
                        )
                    })?;
                let terms_value = args
                    .get("terms")
                    .cloned()
                    .ok_or_else(|| tool_error("invalid_args", "缺少参数 terms"))?;
                let terms: Vec<ScanTermArg> =
                    serde_json::from_value(terms_value).map_err(|error| {
                        tool_error("invalid_args", &format!("terms 结构无效：{error}"))
                    })?;
                let style = match args.get("style") {
                    Some(Value::String(text)) => {
                        if text.trim().is_empty() {
                            return Err(tool_error("invalid_args", "style 不能为空"));
                        }
                        text.trim().to_string()
                    }
                    Some(Value::Object(_)) => args["style"].to_string(),
                    _ => return Err(tool_error("invalid_args", "缺少参数 style")),
                };
                if style.chars().count() > 4_000 {
                    return Err(tool_error("invalid_args", "style 超过 4000 字符"));
                }
                let notes = args
                    .get("notes")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let confidence = match args
                    .get("confidence")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "high" => Confidence::High,
                    "medium" => Confidence::Medium,
                    "low" => Confidence::Low,
                    other => {
                        return Err(tool_error(
                            "invalid_args",
                            &format!("confidence 必须是 high/medium/low，收到 {other}"),
                        ));
                    }
                };
                let mut prepared = Vec::with_capacity(terms.len());
                for term in terms {
                    let source = term.source_term.trim();
                    if source.is_empty() {
                        continue;
                    }
                    let locked = term.locked_translation.trim();
                    if locked.is_empty() {
                        return Err(tool_error_with(
                            "empty_translation",
                            "术语缺少锁定译名",
                            json!({"source_term": source}),
                        ));
                    }
                    if term.term_type.chars().count() > 64 || term.notes.chars().count() > 1_000 {
                        return Err(tool_error("invalid_args", "术语字段超长"));
                    }
                    prepared.push(AgentTerm {
                        source_term: source.to_string(),
                        locked_translation: locked.to_string(),
                        term_type: if term.term_type.trim().is_empty() {
                            "other".into()
                        } else {
                            term.term_type.trim().to_string()
                        },
                        aliases: term
                            .aliases
                            .into_iter()
                            .map(|alias| alias.trim().to_string())
                            .filter(|alias| !alias.is_empty())
                            .take(16)
                            .collect(),
                        notes: term.notes.trim().to_string(),
                    });
                }
                let note = ScanNote {
                    chunk_id: chunk.id.clone(),
                    terms: prepared,
                    style,
                    notes: notes.chars().take(4_000).collect(),
                    confidence,
                };
                write_scan(&ctx, &note).map_err(|error| tool_error("storage", &error))?;
                let versions = phase_versions(&ctx);
                stamp(&ctx, "scan", &chunk_id, versions.prepare_constraints);
                let _ = ctx.touch_checkpoint(Phase::PrepareConstraints);
                let _ = ctx.refresh_counts();
                tool_result(json!({
                    "chunk_id": chunk_id,
                    "terms": note.terms.len(),
                    "committed": true,
                }))
            })
        },
    )
}

fn freeze_constraints_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("freeze_constraints"),
        "汇总全部块的扫描结果并冻结术语与风格，生成新的约束版本。",
        json!({"type": "object", "properties": {}, "required": []}),
        move |_args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move { freeze_constraints_impl(&ctx) })
        },
    )
}

fn freeze_constraints_impl(
    ctx: &std::sync::Arc<JobContext>,
) -> Result<ToolOutput, ToolExecutionError> {
    let versions = phase_versions(ctx);
    let chunks = plan_chunks(ctx).map_err(|error| tool_error("not_prepared", &error))?;
    if chunks.is_empty() {
        return Err(tool_error("not_prepared", "没有可冻结的分块"));
    }
    let mut scans = Vec::with_capacity(chunks.len());
    let mut missing = Vec::new();
    for chunk in &chunks {
        let scan = read_scan(ctx, &chunk.id).map_err(|error| tool_error("storage", &error))?;
        match scan {
            Some(scan) if stamped(ctx, "scan", &chunk.id, versions.prepare_constraints) => {
                scans.push(scan)
            }
            _ => missing.push(chunk.id.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(tool_error_with(
            "scans_incomplete",
            "还有分块没有有效扫描记录，不能冻结约束",
            json!({"missing": missing}),
        ));
    }

    let previous = read_constraints(ctx).map_err(|error| tool_error("storage", &error))?;
    // 既有冻结值（含用户确认结果）优先：先整体放入，再按块顺序补充新术语。
    let mut terms: Vec<AgentTerm> = previous
        .as_ref()
        .filter(|previous| previous.frozen)
        .map(|previous| previous.terms.clone())
        .unwrap_or_default();
    let mut conflicts: Vec<Conflict> = Vec::new();
    let mut style = previous
        .as_ref()
        .map(|previous| previous.style.clone())
        .unwrap_or_default();
    for scan in &scans {
        if style.trim().is_empty() && !scan.style.trim().is_empty() {
            style = scan.style.clone();
        }
        for term in &scan.terms {
            match terms
                .iter()
                .find(|existing| existing.source_term == term.source_term)
            {
                None => terms.push(term.clone()),
                Some(existing) if existing.locked_translation == term.locked_translation => {}
                Some(existing) => conflicts.push(Conflict {
                    source_term: term.source_term.clone(),
                    existing_translation: existing.locked_translation.clone(),
                    candidate_translation: term.locked_translation.clone(),
                    chunk_id: scan.chunk_id.clone(),
                    notes: "保留既有锁定译名".into(),
                }),
            }
        }
    }
    let version = previous
        .as_ref()
        .map(|previous| previous.version)
        .unwrap_or(0)
        + 1;
    let constraints = Constraints {
        version,
        terms,
        style,
        conflicts,
        unresolved: Vec::new(),
        frozen: true,
    };
    write_constraints(ctx, &constraints).map_err(|error| tool_error("storage", &error))?;
    let _ = ctx.touch_checkpoint(Phase::PrepareConstraints);
    let _ = ctx.update_manifest(|manifest| manifest.constraint_version = version);
    let _ = ctx.refresh_counts();
    // 确认模式：冻结后停止派发，等待用户确认。
    if ctx.confirm_before_start && !ctx.control.confirmed() {
        ctx.control.set_awaiting_confirmation(true);
    }
    tool_result(json!({
        "version": constraints.version,
        "terms": constraints.terms.iter().map(|term| json!({
            "source_term": term.source_term,
            "locked_translation": term.locked_translation,
            "term_type": term.term_type,
        })).collect::<Vec<_>>(),
        "style": truncate_to_tokens(&constraints.style, 1_500),
        "conflicts": constraints.conflicts.len(),
        "unresolved": constraints.unresolved,
        "awaiting_confirmation": ctx.control.awaiting_confirmation(),
    }))
}

// ----------------------------------------------------------------------
// 派发（子 Agent 运行）
// ----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum DispatchKind {
    Translate,
    Review,
}

impl DispatchKind {
    fn role(self) -> Role {
        match self {
            Self::Translate => Role::Translator,
            Self::Review => Role::Reviewer,
        }
    }

    fn phase(self) -> Phase {
        match self {
            Self::Translate => Phase::Translate,
            Self::Review => Phase::Review,
        }
    }

    fn tool_name(self) -> &'static str {
        match self {
            Self::Translate => "dispatch_translation",
            Self::Review => "dispatch_review",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct ChunkOutcome {
    chunk_id: String,
    status: &'static str,
    usage: TokenUsage,
    issue_count: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    reused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    derived: Vec<String>,
}

impl ChunkOutcome {
    fn ok(chunk_id: &str, usage: TokenUsage, issue_count: u32, reused: bool) -> Self {
        Self {
            chunk_id: chunk_id.to_string(),
            status: "ok",
            usage,
            issue_count,
            reused,
            message: None,
            derived: Vec::new(),
        }
    }

    fn failed(chunk_id: &str, status: &'static str, message: impl Into<String>) -> Self {
        Self {
            chunk_id: chunk_id.to_string(),
            status,
            usage: TokenUsage::default(),
            issue_count: 0,
            reused: false,
            message: Some(safe_text(&message.into(), 200)),
            derived: Vec::new(),
        }
    }
}

fn dispatch_tool(ctx: &std::sync::Arc<JobContext>, kind: DispatchKind) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name(kind.tool_name()),
        match kind {
            DispatchKind::Translate => "派发翻译子 Agent 处理指定块，返回每块状态、用量与问题数。",
            DispatchKind::Review => "派发独立审查子 Agent 审查指定块，返回每块结论摘要。",
        },
        json!({
            "type": "object",
            "properties": {"chunk_ids": {"type": "array", "items": {"type": "string"}}},
            "required": ["chunk_ids"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let ids = arg_str_list(&args, "chunk_ids")?;
                dispatch_impl(&ctx, kind, ids).await
            })
        },
    )
}

async fn dispatch_impl(
    ctx: &std::sync::Arc<JobContext>,
    kind: DispatchKind,
    ids: Vec<String>,
) -> Result<ToolOutput, ToolExecutionError> {
    // 暂停 / 等待确认：不派发，也不产生副作用。
    if ctx.control.cancelled() {
        return tool_result(json!({"status": "cancelled", "chunks": Vec::<Value>::new()}));
    }
    if ctx.control.awaiting_confirmation() {
        return tool_result(json!({
            "status": "awaiting_confirmation",
            "chunks": Vec::<Value>::new(),
        }));
    }
    let plan = read_plan(ctx).ok_or_else(|| tool_error("not_prepared", "尚未准备原文"))?;
    let known: BTreeSet<&str> = plan.entries.iter().map(|entry| entry.id.as_str()).collect();
    let unknown: Vec<&String> = ids
        .iter()
        .filter(|id| !known.contains(id.as_str()))
        .collect();
    if !unknown.is_empty() {
        return Err(tool_error_with(
            "unknown_chunk",
            "分块不在当前计划中，未执行任何派发",
            json!({"unknown": unknown}),
        ));
    }
    let constraints = read_constraints(ctx).map_err(|error| tool_error("storage", &error))?;
    if kind == DispatchKind::Translate {
        match &constraints {
            Some(constraints) if constraints.frozen => {}
            _ => {
                return Err(tool_error(
                    "constraints_not_frozen",
                    "必须先调用 freeze_constraints 冻结术语与风格",
                ));
            }
        }
    }
    let versions = phase_versions(ctx);

    // 返工轮次：仅在存在待修块时计一轮，达到上限即拒绝继续重译。
    if kind == DispatchKind::Translate {
        let targets = rework_targets(ctx, &versions, &ids)?;
        if !targets.is_empty() {
            let limit = ctx.settings.translation_agent_rework_rounds.clamp(0, 5) as u32;
            if ctx.control.rework_rounds() >= limit {
                return Err(tool_error_with(
                    "rework_limit",
                    "返工轮次已用尽，请按未解决项上报需要处理",
                    json!({"affected": targets}),
                ));
            }
            ctx.control.bump_rework();
        }
    }

    // 逐块决定是否需要请求模型（已完成且有效的块直接复用）。
    let mut planned: Vec<String> = Vec::new();
    let mut outcomes: Vec<ChunkOutcome> = Vec::new();
    for id in ids {
        let chunk = match read_chunk(ctx, &id).map_err(|error| tool_error("storage", &error))? {
            Some(chunk) => chunk,
            None => {
                outcomes.push(ChunkOutcome::failed(&id, "failed", "分块缺失"));
                continue;
            }
        };
        let draft = read_draft(ctx, &id).map_err(|error| tool_error("storage", &error))?;
        let review = read_review(ctx, &id).map_err(|error| tool_error("storage", &error))?;
        let draft_valid = draft.as_ref().is_some_and(|draft| {
            stamped(ctx, "draft", &id, versions.translate)
                && validate_draft_with_language(&chunk, draft, Some(&ctx.target_language)).is_ok()
        });
        let review_valid = review.as_ref().is_some_and(|review| {
            stamped(ctx, "review", &id, versions.review)
                && draft
                    .as_ref()
                    .is_some_and(|draft| review.draft_version == draft.version)
        });
        match kind {
            DispatchKind::Translate => {
                let needs = !draft_valid
                    || draft.as_ref().is_some_and(|draft| draft.needs_repair)
                    || (review_valid
                        && review
                            .as_ref()
                            .is_some_and(|review| review.verdict == ReviewVerdict::NeedsFix));
                if needs {
                    planned.push(id);
                } else {
                    outcomes.push(ChunkOutcome::ok(
                        &id,
                        draft
                            .as_ref()
                            .map(|draft| agent_usage(draft.usage))
                            .unwrap_or_default(),
                        0,
                        true,
                    ));
                }
            }
            DispatchKind::Review => {
                if review_valid {
                    outcomes.push(ChunkOutcome::ok(&id, TokenUsage::default(), 0, true));
                } else {
                    planned.push(id);
                }
            }
        }
    }
    if !planned.is_empty() {
        let gate = ctx.dispatch_gate.clone();
        let futures: Vec<_> = planned
            .into_iter()
            .map(|id| {
                let ctx = ctx.clone();
                let gate = gate.clone();
                async move {
                    let _permit = gate.acquire_owned().await;
                    dispatch_one(&ctx, kind, &id).await
                }
            })
            .collect();
        for outcome in futures::future::join_all(futures).await {
            match outcome {
                Ok(mut results) => outcomes.append(&mut results),
                Err(error) => outcomes.push(error),
            }
        }
    }
    outcomes.sort_by(|left, right| left.chunk_id.cmp(&right.chunk_id));
    let fatal = outcomes.iter().any(|outcome| {
        matches!(
            outcome.status,
            "permanent" | "budget_exhausted" | "unknown_outcome"
        )
    });
    let paused = outcomes.iter().any(|outcome| outcome.status == "paused");
    let _ = ctx.refresh_counts();
    tool_result(json!({
        "status": if fatal { "fatal" } else if paused { "paused" } else { "ok" },
        "fatal": fatal,
        "chunks": outcomes,
    }))
}

/// 返工目标：草稿标了 needs_repair，或审查结论（同版本）为 needs_fix。
fn rework_targets(
    ctx: &JobContext,
    versions: &PhaseVersions,
    ids: &[String],
) -> Result<Vec<String>, ToolExecutionError> {
    let mut targets = Vec::new();
    for id in ids {
        let draft = read_draft(ctx, id).map_err(|error| tool_error("storage", &error))?;
        let review = read_review(ctx, id).map_err(|error| tool_error("storage", &error))?;
        let needs = draft.as_ref().is_some_and(|draft| {
            draft.needs_repair && stamped(ctx, "draft", id, versions.translate)
        }) || review.as_ref().is_some_and(|review| {
            stamped(ctx, "review", id, versions.review)
                && review.verdict == ReviewVerdict::NeedsFix
                && draft
                    .as_ref()
                    .is_some_and(|draft| review.draft_version == draft.version)
        });
        if needs {
            targets.push(id.clone());
        }
    }
    Ok(targets)
}

/// 处理单个块：重试、缩块、取消与暂停。
///
/// 递归（缩块后对派生块再次派发）经装箱边界，避免 async fn 无限尺寸。
fn dispatch_one<'a>(
    ctx: &'a std::sync::Arc<JobContext>,
    kind: DispatchKind,
    chunk_id: &'a str,
) -> futures::future::BoxFuture<'a, Result<Vec<ChunkOutcome>, ChunkOutcome>> {
    Box::pin(dispatch_one_inner(ctx, kind, chunk_id))
}

async fn dispatch_one_inner(
    ctx: &std::sync::Arc<JobContext>,
    kind: DispatchKind,
    chunk_id: &str,
) -> Result<Vec<ChunkOutcome>, ChunkOutcome> {
    let policy = retry_policy(ctx);
    let mut attempt = 0u32;
    let mut split_done = false;
    loop {
        if ctx.control.cancelled() {
            return Ok(vec![ChunkOutcome::failed(
                chunk_id,
                "cancelled",
                "任务已取消",
            )]);
        }
        if ctx.control.pause_requested() || ctx.control.awaiting_confirmation() {
            return Ok(vec![ChunkOutcome::failed(chunk_id, "paused", "已暂停派发")]);
        }
        let started = Instant::now();
        let chunk_chars = read_chunk(ctx, chunk_id)
            .ok()
            .flatten()
            .map(|chunk| chunk.chars)
            .unwrap_or(0);
        let result = match kind {
            DispatchKind::Translate => run_translator(ctx, chunk_id).await,
            DispatchKind::Review => run_reviewer(ctx, chunk_id).await,
        };
        match result {
            Ok(usage) => {
                ctx.append_session(&json!({
                    "kind": match kind {
                        DispatchKind::Translate => "subtask_translate_done",
                        DispatchKind::Review => "subtask_review_done",
                    },
                    "chunk": chunk_id,
                    "chars": chunk_chars,
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                    "usage": {
                        "input": usage.input,
                        "output": usage.output,
                        "total": usage.total,
                    },
                    "attempt": attempt,
                }));
                let _ = ctx.touch_checkpoint(kind.phase());
                let issue_count = read_review(ctx, chunk_id)
                    .ok()
                    .flatten()
                    .map(|review| review.issues.len() as u32)
                    .unwrap_or(0);
                return Ok(vec![ChunkOutcome::ok(chunk_id, usage, issue_count, false)]);
            }
            Err(error) => {
                let class = classify_run_error(&error);
                let code = error.code();
                warn_job(
                    &ctx.scope,
                    kind.role().as_str(),
                    kind.phase(),
                    Some(chunk_id),
                    "subtask_failed",
                    &[
                        ("code", code),
                        ("class", class.as_str()),
                        ("attempt", &attempt.to_string()),
                        ("elapsed_ms", &started.elapsed().as_millis().to_string()),
                    ],
                );
                let message = safe_text(&error.to_string(), 200);
                ctx.append_session(&json!({
                    "kind": "subtask_failed",
                    "chunk": chunk_id,
                    "chars": chunk_chars,
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                    "attempt": attempt,
                    "code": code,
                    "class": class.as_str(),
                }));
                let _ = ctx.update_manifest(|manifest| {
                    manifest.last_error = Some(crate::agent::SafeError {
                        code: code.to_string(),
                        message: message.clone(),
                        step: kind.phase().as_str().into(),
                    });
                });
                match class {
                    FailureClass::Permanent => {
                        return Ok(vec![ChunkOutcome::failed(
                            chunk_id,
                            "permanent",
                            format!("{code}：需要修正设置后重试"),
                        )]);
                    }
                    FailureClass::UnknownOutcome => {
                        if !ctx.settings.translation_agent_replay_unknown {
                            return Ok(vec![ChunkOutcome::failed(
                                chunk_id,
                                "unknown_outcome",
                                "请求结果未知，按设置不自动重放",
                            )]);
                        }
                        if !policy.allows(attempt) {
                            return Ok(vec![ChunkOutcome::failed(
                                chunk_id,
                                "unknown_outcome",
                                "请求结果未知且重试次数用尽",
                            )]);
                        }
                        attempt = attempt.saturating_add(1);
                        if backoff(&policy, attempt - 1, &ctx.control.cancel)
                            .await
                            .is_none()
                        {
                            return Ok(vec![ChunkOutcome::failed(
                                chunk_id,
                                terminal_status(ctx),
                                "重试等待已中止",
                            )]);
                        }
                        continue;
                    }
                    FailureClass::InputTooLarge | FailureClass::OutputTruncated => {
                        if !split_done {
                            split_done = true;
                            let _ = split_done;
                            match split_chunk(ctx, chunk_id) {
                                Ok(derived) => {
                                    let mut outcomes = Vec::with_capacity(derived.len());
                                    for id in &derived {
                                        match dispatch_one(ctx, kind, id).await {
                                            Ok(mut nested) => outcomes.append(&mut nested),
                                            Err(error) => outcomes.push(error),
                                        }
                                    }
                                    if outcomes.iter().all(|outcome| outcome.status == "ok")
                                        && let Some(first) = outcomes.first_mut()
                                    {
                                        first.derived = derived;
                                    }
                                    return Ok(outcomes);
                                }
                                Err(status) => {
                                    return Ok(vec![ChunkOutcome::failed(
                                        chunk_id,
                                        status,
                                        "输入或输出超限且无法继续细分",
                                    )]);
                                }
                            }
                        }
                        return Ok(vec![ChunkOutcome::failed(
                            chunk_id,
                            "too_large",
                            "输入或输出超限且无法继续细分",
                        )]);
                    }
                    FailureClass::Retryable | FailureClass::NotSent => {
                        if !policy.allows(attempt) {
                            return Ok(vec![ChunkOutcome::failed(
                                chunk_id,
                                "failed",
                                format!("{code}：重试次数用尽"),
                            )]);
                        }
                        attempt = attempt.saturating_add(1);
                        if backoff(&policy, attempt - 1, &ctx.control.cancel)
                            .await
                            .is_none()
                        {
                            return Ok(vec![ChunkOutcome::failed(
                                chunk_id,
                                terminal_status(ctx),
                                "重试等待已中止",
                            )]);
                        }
                        continue;
                    }
                    FailureClass::InvalidToolArgs | FailureClass::ReviewIssues => {
                        return Ok(vec![ChunkOutcome::failed(
                            chunk_id,
                            "failed",
                            format!("{code}：子任务未能提交有效产物"),
                        )]);
                    }
                }
            }
        }
    }
}

fn terminal_status(ctx: &JobContext) -> &'static str {
    if ctx.control.cancelled() {
        "cancelled"
    } else {
        "paused"
    }
}

fn retry_policy(ctx: &JobContext) -> RetryPolicy {
    RetryPolicy::new(
        ctx.settings.translation_agent_retry_limit.clamp(0, 5) as u32,
        ctx.settings.translation_agent_backoff_base_ms.max(1) as u64,
        (ctx.settings
            .translation_agent_backoff_max_seconds
            .clamp(10, 600) as u64)
            * 1_000,
    )
}

fn classify_run_error(error: &RunError) -> FailureClass {
    match error {
        RunError::Cancelled => FailureClass::Permanent,
        RunError::BudgetExceeded => FailureClass::Permanent,
        RunError::NoProgress | RunError::MaxTurns(_) => FailureClass::InvalidToolArgs,
        RunError::InvalidToolArgs => FailureClass::InvalidToolArgs,
        RunError::InputTooLarge => FailureClass::InputTooLarge,
        RunError::CompactionFailed(_) => FailureClass::Retryable,
        RunError::Provider { class, .. } => *class,
        RunError::Internal(_) => FailureClass::Permanent,
    }
}

async fn run_translator(
    ctx: &std::sync::Arc<JobContext>,
    chunk_id: &str,
) -> Result<TokenUsage, RunError> {
    let chunk = read_chunk(ctx, chunk_id)
        .ok()
        .flatten()
        .ok_or_else(|| RunError::Internal("分块不存在".into()))?;
    let constraints = read_constraints(ctx).ok().flatten();
    let draft = read_draft(ctx, chunk_id).ok().flatten();
    let review = read_review(ctx, chunk_id).ok().flatten();
    let system = format!(
        "{}\n\n{}\n\n{}",
        system_prompt(),
        role_prompt(Role::Translator),
        TRANSLATOR_TOOL_CONTRACT
    );
    let prompt = translator_prompt(
        ctx,
        &chunk,
        constraints.as_ref(),
        draft.as_ref(),
        review.as_ref(),
    );
    let (max_tokens, params) = ctx
        .model
        .request_params(chunk.chars as usize)
        .map_err(RunError::Internal)?;
    let request = RunRequest::new(
        Role::Translator,
        system,
        prompt,
        ctx.model.model().map_err(RunError::Internal)?,
        translator_tools(ctx, chunk_id),
        ctx.run_limits(),
        ctx.run_sink(),
        ctx.scope.clone(),
    )
    .with_phase(Phase::Translate)
    .with_budget(ctx.budget.clone())
    .with_budget_config(ctx.budget_config.clone())
    .with_compaction(
        crate::agent::CompactionConfig::new(ctx.budget_config.clone(), ctx.scope.clone())
            .with_scheduler(ctx.scheduler.clone())
            .with_tracker(ctx.budget.clone()),
    )
    .with_scheduler(ctx.scheduler.clone())
    .with_request_params(max_tokens, params);
    let outcome = runtime::run(request, ctx.control.cancel.child()).await?;
    accumulate_usage(ctx, outcome.usage);
    // 模型可能只结束而没有提交：以产物为准判断成败。
    let Some(saved) = read_draft(ctx, chunk_id).ok().flatten() else {
        return Err(RunError::Internal("翻译子任务没有提交草稿".into()));
    };
    if validate_draft_with_language(&chunk, &saved, Some(&ctx.target_language)).is_err() {
        return Err(RunError::Internal(
            "翻译子任务提交的草稿未通过结构校验".into(),
        ));
    }
    Ok(outcome.usage)
}

fn translator_prompt(
    ctx: &JobContext,
    chunk: &Chunk,
    constraints: Option<&Constraints>,
    draft: Option<&Draft>,
    review: Option<&Review>,
) -> String {
    let mut prompt = format!(
        "chunk_id: {}\n目标语言: {}（{}）\n约束版本: {}\n风格: {}\n相关术语: {}\n",
        chunk.id,
        ctx.target_language,
        ctx.target_language_name,
        constraints
            .map(|constraints| constraints.version)
            .unwrap_or(0),
        constraints
            .map(|constraints| safe_text(&constraints.style, 600))
            .unwrap_or_else(|| "（无）".into()),
        constraints
            .map(|constraints| {
                serde_json::to_string(&relevant_terms(chunk, constraints))
                    .unwrap_or_else(|_| "[]".into())
            })
            .unwrap_or_else(|| "[]".into()),
    );
    if let Some(draft) = draft {
        prompt.push_str(&format!(
            "当前草稿版本 {}（返工基线，可整体重写）：{}\n",
            draft.version,
            truncate_to_tokens(&draft_to_text(draft), 1_200)
        ));
    }
    if let Some(review) = review
        && review.verdict == ReviewVerdict::NeedsFix
    {
        let issues: Vec<Value> = review
            .issues
            .iter()
            .take(12)
            .map(|issue| {
                json!({
                    "segment_id": issue.segment_id,
                    "kind": issue.kind,
                    "severity": severity_str(issue.severity),
                    "detail": safe_text(&issue.detail, 240),
                })
            })
            .collect();
        prompt.push_str(&format!("本块未解决问题：{}\n", Value::Array(issues)));
    }
    prompt.push_str(
        "请先调用 read_chunk(chunk_id, true) 读取原文，然后调用 submit_translation 提交本块全部片段。\
         只翻主块，上下文不进入输出；不得新增、删除或重排片段。",
    );
    prompt
}

fn draft_to_text(draft: &Draft) -> String {
    draft
        .segments
        .iter()
        .map(|segment| format!("{}={}", segment.id, segment.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 与本块相关的术语：原文中出现的术语及其别名。
fn relevant_terms(chunk: &Chunk, constraints: &Constraints) -> Vec<Value> {
    let text = chunk
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    constraints
        .terms
        .iter()
        .filter(|term| {
            text.contains(&term.source_term)
                || term
                    .aliases
                    .iter()
                    .any(|alias| !alias.is_empty() && text.contains(alias))
        })
        .take(80)
        .map(|term| {
            json!({
                "source_term": term.source_term,
                "locked_translation": term.locked_translation,
                "term_type": term.term_type,
            })
        })
        .collect()
}

async fn run_reviewer(
    ctx: &std::sync::Arc<JobContext>,
    chunk_id: &str,
) -> Result<TokenUsage, RunError> {
    let chunk = read_chunk(ctx, chunk_id)
        .ok()
        .flatten()
        .ok_or_else(|| RunError::Internal("分块不存在".into()))?;
    let draft = read_draft(ctx, chunk_id)
        .ok()
        .flatten()
        .ok_or_else(|| RunError::Internal("本块还没有可审查的译文".into()))?;
    let constraints = read_constraints(ctx).ok().flatten();
    let system = format!(
        "{}\n\n{}\n\n{}",
        system_prompt(),
        role_prompt(Role::Reviewer),
        REVIEWER_TOOL_CONTRACT
    );
    let prompt = format!(
        "chunk_id: {}\n目标语言: {}（{}）\n草稿版本: {}\n约束版本: {}\n风格: {}\n相关术语: {}\n\
         请先 read_chunk(chunk_id, true) 与 read_translation(chunk_id)，逐片段对照后 submit_review。",
        chunk.id,
        ctx.target_language,
        ctx.target_language_name,
        draft.version,
        constraints.as_ref().map(|item| item.version).unwrap_or(0),
        constraints
            .as_ref()
            .map(|item| safe_text(&item.style, 600))
            .unwrap_or_else(|| "（无）".into()),
        constraints
            .as_ref()
            .map(|item| serde_json::to_string(&relevant_terms(&chunk, item)).unwrap_or_default())
            .unwrap_or_else(|| "[]".into()),
    );
    let (max_tokens, params) = ctx
        .model
        .request_params((chunk.chars as usize).saturating_mul(2))
        .map_err(RunError::Internal)?;
    let request = RunRequest::new(
        Role::Reviewer,
        system,
        prompt,
        ctx.model.model().map_err(RunError::Internal)?,
        reviewer_tools(ctx, chunk_id),
        ctx.run_limits(),
        ctx.run_sink(),
        ctx.scope.clone(),
    )
    .with_phase(Phase::Review)
    .with_budget(ctx.budget.clone())
    .with_budget_config(ctx.budget_config.clone())
    .with_compaction(
        crate::agent::CompactionConfig::new(ctx.budget_config.clone(), ctx.scope.clone())
            .with_scheduler(ctx.scheduler.clone())
            .with_tracker(ctx.budget.clone()),
    )
    .with_scheduler(ctx.scheduler.clone())
    .with_request_params(max_tokens, params);
    let outcome = runtime::run(request, ctx.control.cancel.child()).await?;
    accumulate_usage(ctx, outcome.usage);
    // 审查结论必须对应本次草稿版本，否则视为未完成（防止旧结论冒充）。
    let Some(saved) = read_review(ctx, chunk_id).ok().flatten() else {
        return Err(RunError::Internal("审查子任务没有提交结论".into()));
    };
    if saved.draft_version != draft.version {
        return Err(RunError::Internal("审查结论的草稿版本不匹配".into()));
    }
    Ok(outcome.usage)
}

fn accumulate_usage(ctx: &std::sync::Arc<JobContext>, usage: TokenUsage) {
    ctx.add_usage(usage);
    let snapshot = ctx.budget.snapshot();
    let _ = ctx.update_manifest(|manifest| {
        manifest.usage.add(usage);
        manifest.attempts.model_calls = snapshot.used;
        manifest.attempts.manual_extra = snapshot.manual_extra;
    });
}

/// 细分一个超限块：拆成两个派生块（不超过 `split_depth`）。
fn split_chunk(
    ctx: &std::sync::Arc<JobContext>,
    chunk_id: &str,
) -> Result<Vec<String>, &'static str> {
    let max_depth = ctx.settings.translation_agent_split_depth.clamp(0, 5) as u32;
    let mut plan = read_plan(ctx).ok_or("too_large")?;
    let Some(index) = plan.entries.iter().position(|entry| entry.id == chunk_id) else {
        return Err("too_large");
    };
    let depth = plan.entries[index].depth;
    if depth >= max_depth {
        return Err("too_large");
    }
    let chunk = read_chunk(ctx, chunk_id)
        .map_err(|_| "too_large")?
        .ok_or("too_large")?;
    let take = chunk.segments.len() / 2;
    if take == 0 || take >= chunk.segments.len() {
        return Err("too_large");
    }
    let halves: [Vec<SourceSegment>; 2] = [
        chunk.segments[..take].to_vec(),
        chunk.segments[take..].to_vec(),
    ];
    let mut max_number = plan
        .entries
        .iter()
        .filter_map(|entry| entry.id.strip_prefix('c')?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let context_chars = ctx.chunk_config().context_chars;
    let texts: Vec<String> = halves
        .iter()
        .map(|half| {
            half.iter()
                .map(|segment| segment.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    let mut derived = Vec::new();
    for (position, half) in halves.into_iter().enumerate() {
        max_number += 1;
        let id = format!("c{max_number:04}");
        let prev_context = if position == 0 {
            chunk.prev_context.clone()
        } else {
            tail_chars(&texts[0], context_chars)
        };
        let next_context = if position == 1 {
            chunk.next_context.clone()
        } else {
            head_chars(&texts[1], context_chars)
        };
        let chars: usize = half
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum();
        let derived_chunk = Chunk {
            id: id.clone(),
            order: chunk.order,
            segments: half,
            prev_context,
            next_context,
            chars: chars.min(u32::MAX as usize) as u32,
        };
        write_chunk(ctx, &derived_chunk).map_err(|_| "too_large")?;
        derived.push(id);
    }
    plan.entries[index] = PlanEntry {
        id: derived[0].clone(),
        order: chunk.order,
        parent: Some(chunk_id.to_string()),
        depth: depth + 1,
    };
    plan.entries.insert(
        index + 1,
        PlanEntry {
            id: derived[1].clone(),
            order: chunk.order,
            parent: Some(chunk_id.to_string()),
            depth: depth + 1,
        },
    );
    write_plan(ctx, &plan).map_err(|_| "too_large")?;
    warn_job(
        &ctx.scope,
        "coordinator",
        Phase::Translate,
        Some(chunk_id),
        "chunk_split",
        &[("depth", &(depth + 1).to_string())],
    );
    ctx.append_session(&json!({
        "kind": "chunk_split",
        "chunk": chunk_id,
        "depth": depth + 1,
        "derived": derived,
        "source_segments": chunk.segments.len(),
    }));
    Ok(derived)
}

fn head_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn tail_chars(text: &str, count: usize) -> String {
    let total = text.chars().count();
    if total <= count {
        text.to_string()
    } else {
        text.chars().skip(total - count).collect()
    }
}

// ----------------------------------------------------------------------
// 汇总、合并、复核、发布、重置、状态
// ----------------------------------------------------------------------

fn read_review_summary_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("read_review_summary"),
        "读取审查汇总：问题总数、严重度分布、受影响块与未解决项。",
        json!({
            "type": "object",
            "properties": {"chunk_ids": {"type": "array", "items": {"type": "string"}}},
            "required": []
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let ids = match args.get("chunk_ids") {
                    Some(Value::Array(items)) if !items.is_empty() => items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                    _ => plan_ids(&ctx).map_err(|error| tool_error("not_prepared", &error))?,
                };
                let (mut critical, mut major, mut minor) = (0u32, 0u32, 0u32);
                let mut affected = Vec::new();
                let mut per_chunk = Vec::new();
                for id in &ids {
                    let review =
                        read_review(&ctx, id).map_err(|error| tool_error("storage", &error))?;
                    match review {
                        None => per_chunk.push(json!({"chunk_id": id, "status": "missing"})),
                        Some(review) => {
                            for issue in &review.issues {
                                match issue.severity {
                                    Severity::Critical => critical += 1,
                                    Severity::Major => major += 1,
                                    Severity::Minor => minor += 1,
                                }
                            }
                            if review.verdict == ReviewVerdict::NeedsFix {
                                affected.push(id.clone());
                            }
                            per_chunk.push(json!({
                                "chunk_id": id,
                                "verdict": verdict_str(review.verdict),
                                "issues": review.issues.iter().map(|issue| json!({
                                    "segment_id": issue.segment_id,
                                    "kind": issue.kind,
                                    "severity": severity_str(issue.severity),
                                })).collect::<Vec<_>>(),
                                "notes": safe_text(&review.notes, 200),
                            }));
                        }
                    }
                }
                tool_result(json!({
                    "total": critical + major + minor,
                    "critical": critical,
                    "major": major,
                    "minor": minor,
                    "affected_chunk_ids": affected,
                    "chunks": per_chunk,
                }))
            })
        },
    )
}

struct MergeOutcome {
    report: MergeReport,
    status: PublishStatus,
    unresolved: u32,
    pages: BTreeMap<u32, Vec<artifacts::TranslatedLine>>,
}

fn merge_report_ok(report: &MergeReport) -> bool {
    report.coverage_ok
        && report.order_ok
        && report.duplicate_ids.is_empty()
        && report.missing_ids.is_empty()
        && report.markup_issues.is_empty()
}

fn compute_merge(ctx: &JobContext) -> Result<MergeOutcome, String> {
    let versions = phase_versions(ctx);
    let chunks = plan_chunks(ctx)?;
    if chunks.is_empty() {
        return Err("没有可合并的分块".into());
    }
    let mut drafts = Vec::new();
    for chunk in &chunks {
        if let Some(draft) = read_draft(ctx, &chunk.id)?
            && stamped(ctx, "draft", &chunk.id, versions.translate)
        {
            drafts.push(draft);
        }
    }
    let mut reviews = Vec::new();
    for chunk in &chunks {
        if let Some(review) = read_review(ctx, &chunk.id)?
            && stamped(ctx, "review", &chunk.id, versions.review)
        {
            reviews.push(review);
        }
    }
    let global = read_global_review(ctx)?
        .filter(|_| stamped(ctx, "global_review", "global", versions.global_review));
    let pages = merge_pages(&chunks, &drafts);
    let report = validate_merge_with_language(&chunks, &drafts, &pages, Some(&ctx.target_language));
    let status = publish_status_with_language(
        &chunks,
        &drafts,
        &reviews,
        global.as_ref(),
        Some(&ctx.target_language),
    );
    let unresolved = reviews
        .iter()
        .flat_map(|review| review.issues.iter())
        .filter(|issue| issue.is_blocking())
        .count() as u32
        + report.missing_ids.len() as u32
        + report.duplicate_ids.len() as u32
        + report.markup_issues.len() as u32;
    Ok(MergeOutcome {
        report,
        status,
        unresolved,
        pages,
    })
}

fn merge_drafts_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("merge_drafts"),
        "合并校验全部草稿：覆盖、顺序、重复、缺失与标记；写入合并报告与阅读器投影。",
        json!({"type": "object", "properties": {}, "required": []}),
        move |_args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move { merge_drafts_impl(&ctx) })
        },
    )
}

fn merge_drafts_impl(ctx: &std::sync::Arc<JobContext>) -> Result<ToolOutput, ToolExecutionError> {
    let outcome = compute_merge(ctx).map_err(|error| tool_error("merge_failed", &error))?;
    write_merge_report(ctx, &outcome.report).map_err(|error| tool_error("storage", &error))?;
    let versions = phase_versions(ctx);
    let result = TranslationResult {
        novel_id: ctx.scope.novel_id,
        job_id: ctx.scope.job_id.clone(),
        target_language: ctx.target_language.clone(),
        pages: outcome.pages.clone(),
        status: outcome.status,
        unresolved: outcome.unresolved,
    };
    write_result(ctx, &result).map_err(|error| tool_error("storage", &error))?;
    stamp(ctx, "result", "result", versions.global_review);
    let _ = ctx.touch_checkpoint(Phase::Publish);
    let _ = ctx.refresh_counts();
    tool_result(json!({
        "coverage_ok": outcome.report.coverage_ok,
        "order_ok": outcome.report.order_ok,
        "duplicate_ids": outcome.report.duplicate_ids,
        "missing_ids": outcome.report.missing_ids,
        "markup_issues": outcome.report.markup_issues,
        "all_ok": merge_report_ok(&outcome.report),
        "status": publish_status_str(outcome.status),
        "unresolved": outcome.unresolved,
    }))
}

fn record_global_review_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("record_global_review"),
        "记录整体复核结论；passed=true 仅当抽样块无未解决 critical / major。",
        json!({
            "type": "object",
            "properties": {
                "sample_chunk_ids": {"type": "array", "items": {"type": "string"}},
                "notes": {"type": "string"},
                "passed": {"type": "boolean"}
            },
            "required": ["sample_chunk_ids", "notes", "passed"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let sample = arg_str_list(&args, "sample_chunk_ids")?;
                let notes = args
                    .get("notes")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let passed = args
                    .get("passed")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| tool_error("invalid_args", "缺少参数 passed"))?;
                let known = plan_ids(&ctx).map_err(|error| tool_error("not_prepared", &error))?;
                if let Some(unknown) = sample.iter().find(|id| !known.contains(id)) {
                    return Err(tool_error_with(
                        "unknown_chunk",
                        "抽样块不在当前计划中",
                        json!({"chunk_id": unknown}),
                    ));
                }
                let review = GlobalReview {
                    passed,
                    sample_chunk_ids: sample,
                    notes: notes.chars().take(4_000).collect(),
                };
                let versions = phase_versions(&ctx);
                write_global_review(&ctx, &review)
                    .map_err(|error| tool_error("storage", &error))?;
                stamp(&ctx, "global_review", "global", versions.global_review);
                let _ = ctx.touch_checkpoint(Phase::GlobalReview);
                tool_result(json!({"recorded": true, "passed": passed}))
            })
        },
    )
}

fn publish_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("publish"),
        "发布整篇译文：仅在全部完成条件满足时写入合并稿与正式结果。",
        json!({"type": "object", "properties": {}, "required": []}),
        move |_args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move { publish_impl(&ctx) })
        },
    )
}

fn publish_impl(ctx: &std::sync::Arc<JobContext>) -> Result<ToolOutput, ToolExecutionError> {
    let outcome = compute_merge(ctx).map_err(|error| tool_error("publish_failed", &error))?;
    let versions = phase_versions(ctx);
    let complete = outcome.status == PublishStatus::Complete && merge_report_ok(&outcome.report);
    let status = if complete {
        PublishStatus::Complete
    } else {
        outcome.status
    };
    let unresolved = if complete {
        0
    } else {
        outcome.unresolved.max(1)
    };
    if complete {
        let source = read_source(ctx)
            .map_err(|error| tool_error("storage", &error))?
            .ok_or_else(|| tool_error("storage", "缺少原文映射"))?;
        let markdown = render_markdown(&source, &outcome.pages);
        ctx.store
            .write_atomic("output/merged.md", markdown.as_bytes())
            .map_err(|error| tool_error("storage", &error.to_string()))?;
    }
    let result = TranslationResult {
        novel_id: ctx.scope.novel_id,
        job_id: ctx.scope.job_id.clone(),
        target_language: ctx.target_language.clone(),
        pages: outcome.pages.clone(),
        status,
        unresolved,
    };
    write_result(ctx, &result).map_err(|error| tool_error("storage", &error))?;
    stamp(ctx, "result", "result", versions.global_review);
    if complete {
        let _ = ctx.touch_checkpoint(Phase::Publish);
        let _ = ctx.update_manifest(|manifest| {
            manifest.state = crate::agent::JobState::Completed;
            manifest.phase = Phase::Publish;
        });
    }
    let _ = ctx.refresh_counts();
    tool_result(json!({
        "published": complete,
        "status": publish_status_str(status),
        "unresolved": unresolved,
    }))
}

fn reset_phase_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("reset_phase"),
        "按依赖使目标阶段及下游产物失效；保留上游产物，不清预算与重试记录。",
        json!({
            "type": "object",
            "properties": {
                "phase": {"type": "string", "enum": ["prepare_source", "prepare_constraints", "translate", "review", "global_review"]},
                "reason": {"type": "string"}
            },
            "required": ["phase"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let phase = match args.get("phase").and_then(Value::as_str) {
                    Some("prepare_source") => Phase::PrepareSource,
                    Some("prepare_constraints") => Phase::PrepareConstraints,
                    Some("translate") | Some("repair") => Phase::Translate,
                    Some("review") => Phase::Review,
                    Some("global_review") | Some("publish") => Phase::GlobalReview,
                    other => {
                        return Err(tool_error(
                            "invalid_args",
                            &format!("phase 无效：{other:?}"),
                        ));
                    }
                };
                let reason = args
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let versions = reset_phase_versions(&ctx, phase);
                ctx.control.note_reset(phase, &reason);
                if phase == Phase::PrepareSource {
                    // 原文或切分策略变化：重新解析并按新阶段版本重建计划。
                    prepare_source_impl(&ctx).await?;
                }
                let mut notes: Vec<Value> = read_json(&ctx, RESET_NOTES_PATH).unwrap_or_default();
                notes.push(json!({
                    "phase": phase,
                    "reason": safe_text(&reason, 200),
                    "at": Utc::now().to_rfc3339(),
                }));
                if notes.len() > 50 {
                    let drop = notes.len() - 50;
                    notes.drain(..drop);
                }
                let _ = write_json(&ctx, RESET_NOTES_PATH, &notes);
                let _ = ctx.refresh_counts();
                tool_result(json!({
                    "reset": phase.as_str(),
                    "versions": {
                        "prepare_source": versions.prepare_source,
                        "prepare_constraints": versions.prepare_constraints,
                        "translate": versions.translate,
                        "review": versions.review,
                        "global_review": versions.global_review,
                    },
                    "budget_preserved": true,
                }))
            })
        },
    )
}

fn job_status_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("job_status"),
        "读取任务状态、阶段、计数、预算与待办块。",
        json!({"type": "object", "properties": {}, "required": []}),
        move |_args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let manifest = ctx
                    .read_manifest()
                    .map_err(|error| tool_error("storage", &error))?;
                let versions = phase_versions(&ctx);
                let chunks = plan_chunks(&ctx).unwrap_or_default();
                let mut pending_translate = Vec::new();
                let mut pending_review = Vec::new();
                for chunk in &chunks {
                    let draft = read_draft(&ctx, &chunk.id).ok().flatten();
                    let draft_valid = draft.as_ref().is_some_and(|draft| {
                        stamped(&ctx, "draft", &chunk.id, versions.translate)
                            && validate_draft_with_language(
                                chunk,
                                draft,
                                Some(&ctx.target_language),
                            )
                            .is_ok()
                            && !draft.needs_repair
                    });
                    if !draft_valid {
                        pending_translate.push(chunk.id.clone());
                    }
                    let review_valid =
                        read_review(&ctx, &chunk.id)
                            .ok()
                            .flatten()
                            .is_some_and(|review| {
                                stamped(&ctx, "review", &chunk.id, versions.review)
                                    && draft
                                        .as_ref()
                                        .is_some_and(|draft| review.draft_version == draft.version)
                                    && review.verdict == ReviewVerdict::Pass
                            });
                    if !review_valid {
                        pending_review.push(chunk.id.clone());
                    }
                }
                let budget = ctx.budget.snapshot();
                tool_result(json!({
                    "job_id": ctx.scope.job_id,
                    "state": manifest.state.as_str(),
                    "phase": manifest.phase.as_str(),
                    "target_language": ctx.target_language,
                    "counts": {
                        "pages": manifest.counts.pages,
                        "chunks": chunks.len(),
                        "translated": chunks.len().saturating_sub(pending_translate.len()),
                        "reviewed": chunks.len().saturating_sub(pending_review.len()),
                        "needs_repair": pending_translate.len(),
                        "terms": manifest.counts.terms,
                    },
                    "constraint_version": manifest.constraint_version,
                    "budget": {
                        "used": budget.used,
                        "limit": budget.limit,
                        "remaining": budget.remaining,
                        "manual_extra": budget.manual_extra,
                    },
                    "chunk_ids": chunks.iter().map(|chunk| chunk.id.clone()).collect::<Vec<_>>(),
                    "pending_translate": pending_translate,
                    "pending_review": pending_review,
                    "awaiting_confirmation": ctx.control.awaiting_confirmation(),
                    "rework_rounds": ctx.control.rework_rounds(),
                    "rework_limit": ctx.settings.translation_agent_rework_rounds,
                    "split_depth": ctx.settings.translation_agent_split_depth,
                }))
            })
        },
    )
}

// ----------------------------------------------------------------------
// 子角色写入工具
// ----------------------------------------------------------------------

fn submit_translation_tool(ctx: &std::sync::Arc<JobContext>, chunk_id: &str) -> DynamicTool {
    let ctx = ctx.clone();
    let bound = chunk_id.to_string();
    DynamicTool::new(
        tool_name("submit_translation"),
        "提交本块译文；每个输入片段恰好一次且顺序一致。",
        json!({
            "type": "object",
            "properties": {
                "chunk_id": {"type": "string"},
                "segments": {"type": "array", "items": {
                    "type": "object",
                    "properties": {"id": {"type": "string"}, "text": {"type": "string"}},
                    "required": ["id", "text"]
                }}
            },
            "required": ["chunk_id", "segments"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            let bound = bound.clone();
            Box::pin(async move {
                let chunk_id = arg_str(&args, "chunk_id")?;
                if chunk_id != bound {
                    return Err(tool_error_with(
                        "out_of_scope",
                        "只能提交本次派发的分块",
                        json!({"expected": bound, "actual": chunk_id}),
                    ));
                }
                let chunk = read_chunk(&ctx, &chunk_id)
                    .map_err(|error| tool_error("invalid_chunk", &error))?
                    .ok_or_else(|| {
                        tool_error_with(
                            "unknown_chunk",
                            "分块不存在",
                            json!({"chunk_id": chunk_id}),
                        )
                    })?;
                let raw = args
                    .get("segments")
                    .cloned()
                    .ok_or_else(|| tool_error("invalid_args", "缺少参数 segments"))?;
                let parsed: Vec<TranslatedSegment> =
                    serde_json::from_value(raw).map_err(|error| {
                        tool_error("invalid_args", &format!("segments 结构无效：{error}"))
                    })?;
                let existing =
                    read_draft(&ctx, &chunk_id).map_err(|error| tool_error("storage", &error))?;
                let version = existing.as_ref().map(|draft| draft.version).unwrap_or(0) + 1;
                let draft = Draft {
                    chunk_id: chunk_id.clone(),
                    version,
                    segments: parsed,
                    usage: artifacts::TokenUsage::default(),
                    reviewed: false,
                    needs_repair: false,
                };
                // 先校验再写：结构错误不产生任何副作用。
                if let Err(issues) =
                    validate_draft_with_language(&chunk, &draft, Some(&ctx.target_language))
                {
                    let summary: Vec<Value> = issues
                        .iter()
                        .take(12)
                        .map(|issue| {
                            json!({
                                "segment_id": issue.segment_id,
                                "kind": issue.kind,
                                "severity": severity_str(issue.severity),
                                "detail": issue.detail,
                            })
                        })
                        .collect();
                    return Err(tool_error_with(
                        "draft_invalid",
                        "译文未通过结构校验，未写入草稿",
                        json!({"issues": summary, "issue_count": issues.len()}),
                    ));
                }
                write_draft(&ctx, &draft).map_err(|error| tool_error("storage", &error))?;
                let versions = phase_versions(&ctx);
                stamp(&ctx, "draft", &chunk_id, versions.translate);
                let _ = ctx.touch_checkpoint(Phase::Translate);
                let _ = ctx.refresh_counts();
                tool_result(json!({"chunk_id": chunk_id, "version": version, "committed": true}))
            })
        },
    )
}

fn read_translation_tool(ctx: &std::sync::Arc<JobContext>) -> DynamicTool {
    let ctx = ctx.clone();
    DynamicTool::new(
        tool_name("read_translation"),
        "读取本块译文、草稿版本、约束版本、相关术语与风格。",
        json!({
            "type": "object",
            "properties": {"chunk_id": {"type": "string"}},
            "required": ["chunk_id"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let chunk_id = arg_str(&args, "chunk_id")?;
                let chunk = read_chunk(&ctx, &chunk_id)
                    .map_err(|error| tool_error("invalid_chunk", &error))?
                    .ok_or_else(|| {
                        tool_error_with(
                            "unknown_chunk",
                            "分块不存在",
                            json!({"chunk_id": chunk_id}),
                        )
                    })?;
                let draft = read_draft(&ctx, &chunk_id)
                    .map_err(|error| tool_error("storage", &error))?
                    .ok_or_else(|| tool_error("draft_missing", "本块还没有译文草稿"))?;
                let constraints = read_constraints(&ctx)
                    .map_err(|error| tool_error("storage", &error))?
                    .unwrap_or(Constraints {
                        version: 0,
                        terms: Vec::new(),
                        style: String::new(),
                        conflicts: Vec::new(),
                        unresolved: Vec::new(),
                        frozen: false,
                    });
                tool_result(json!({
                    "chunk_id": chunk_id,
                    "draft_version": draft.version,
                    "segments": draft.segments,
                    "constraints_version": constraints.version,
                    "terms": relevant_terms(&chunk, &constraints),
                    "style": truncate_to_tokens(&constraints.style, 1_200),
                }))
            })
        },
    )
}

#[derive(Deserialize)]
struct ReviewIssueArg {
    segment_id: String,
    kind: String,
    severity: String,
    detail: String,
}

fn submit_review_tool(ctx: &std::sync::Arc<JobContext>, chunk_id: &str) -> DynamicTool {
    let ctx = ctx.clone();
    let bound = chunk_id.to_string();
    DynamicTool::new(
        tool_name("submit_review"),
        "提交本块审查结论；verdict=pass 时 issues 必须为空。",
        json!({
            "type": "object",
            "properties": {
                "chunk_id": {"type": "string"},
                "verdict": {"type": "string", "enum": ["pass", "needs_fix"]},
                "issues": {"type": "array", "items": {"type": "object"}},
                "notes": {"type": "string"}
            },
            "required": ["chunk_id", "verdict", "issues", "notes"]
        }),
        move |args: Value| {
            let ctx = ctx.clone();
            let bound = bound.clone();
            Box::pin(async move {
                let chunk_id = arg_str(&args, "chunk_id")?;
                if chunk_id != bound {
                    return Err(tool_error_with(
                        "out_of_scope",
                        "只能提交本次派发的分块",
                        json!({"expected": bound, "actual": chunk_id}),
                    ));
                }
                let chunk = read_chunk(&ctx, &chunk_id)
                    .map_err(|error| tool_error("invalid_chunk", &error))?
                    .ok_or_else(|| {
                        tool_error_with(
                            "unknown_chunk",
                            "分块不存在",
                            json!({"chunk_id": chunk_id}),
                        )
                    })?;
                let draft = read_draft(&ctx, &chunk_id)
                    .map_err(|error| tool_error("storage", &error))?
                    .ok_or_else(|| tool_error("draft_missing", "本块还没有译文草稿"))?;
                let verdict = match args.get("verdict").and_then(Value::as_str) {
                    Some("pass") => ReviewVerdict::Pass,
                    Some("needs_fix") => ReviewVerdict::NeedsFix,
                    other => {
                        return Err(tool_error(
                            "invalid_args",
                            &format!("verdict 无效：{other:?}"),
                        ));
                    }
                };
                let raw_issues = args.get("issues").cloned().unwrap_or_else(|| json!([]));
                let parsed: Vec<ReviewIssueArg> =
                    serde_json::from_value(raw_issues).map_err(|error| {
                        tool_error("invalid_args", &format!("issues 结构无效：{error}"))
                    })?;
                let known: BTreeSet<&str> = chunk
                    .segments
                    .iter()
                    .map(|segment| segment.id.as_str())
                    .collect();
                let mut issues = Vec::with_capacity(parsed.len());
                for issue in parsed {
                    if !known.contains(issue.segment_id.as_str()) {
                        return Err(tool_error_with(
                            "unknown_segment",
                            "问题引用了本块之外的片段",
                            json!({"segment_id": issue.segment_id}),
                        ));
                    }
                    let severity = match issue.severity.to_ascii_lowercase().as_str() {
                        "critical" => Severity::Critical,
                        "major" => Severity::Major,
                        "minor" => Severity::Minor,
                        other => {
                            return Err(tool_error(
                                "invalid_args",
                                &format!("severity 无效：{other}"),
                            ));
                        }
                    };
                    if issue.detail.trim().is_empty() {
                        return Err(tool_error("invalid_args", "问题 detail 不能为空"));
                    }
                    if issue.kind.trim().is_empty() {
                        return Err(tool_error("invalid_args", "问题 kind 不能为空"));
                    }
                    issues.push(Issue {
                        segment_id: issue.segment_id,
                        kind: issue.kind.chars().take(64).collect(),
                        severity,
                        detail: issue.detail.chars().take(1_000).collect(),
                    });
                }
                if verdict == ReviewVerdict::Pass && !issues.is_empty() {
                    return Err(tool_error(
                        "verdict_conflict",
                        "verdict=pass 时 issues 必须为空",
                    ));
                }
                if verdict == ReviewVerdict::NeedsFix && issues.is_empty() {
                    return Err(tool_error(
                        "verdict_conflict",
                        "verdict=needs_fix 时必须给出至少一个具体问题",
                    ));
                }
                let review = Review {
                    chunk_id: chunk_id.clone(),
                    draft_version: draft.version,
                    verdict,
                    issues,
                    notes: args
                        .get("notes")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .chars()
                        .take(2_000)
                        .collect(),
                };
                write_review(&ctx, &review).map_err(|error| tool_error("storage", &error))?;
                let versions = phase_versions(&ctx);
                stamp(&ctx, "review", &chunk_id, versions.review);
                // 草稿的 reviewed / needs_repair 标记随审查结论同步（同版本覆盖写）。
                if let Some(mut draft) = read_draft(&ctx, &chunk_id).ok().flatten()
                    && draft.version == review.draft_version
                {
                    draft.reviewed = verdict == ReviewVerdict::Pass;
                    draft.needs_repair = verdict == ReviewVerdict::NeedsFix;
                    let _ = write_draft(&ctx, &draft);
                }
                let _ = ctx.touch_checkpoint(Phase::Review);
                let _ = ctx.refresh_counts();
                tool_result(json!({
                    "chunk_id": chunk_id,
                    "verdict": verdict_str(review.verdict),
                    "issues": review.issues.len(),
                }))
            })
        },
    )
}

// ----------------------------------------------------------------------
// 计数与宿主辅助
// ----------------------------------------------------------------------

/// 依据产物重新计算计数（不含预算）。
pub fn compute_counts(ctx: &JobContext) -> crate::agent::JobCounts {
    let chunks = plan_chunks(ctx).unwrap_or_default();
    let versions = phase_versions(ctx);
    let mut translated = 0u32;
    let mut reviewed = 0u32;
    let mut needs_repair = 0u32;
    for chunk in &chunks {
        let draft = read_draft(ctx, &chunk.id).ok().flatten();
        let draft_valid = draft.as_ref().is_some_and(|draft| {
            stamped(ctx, "draft", &chunk.id, versions.translate)
                && validate_draft_with_language(chunk, draft, Some(&ctx.target_language)).is_ok()
        });
        if draft_valid {
            translated += 1;
        }
        if draft.as_ref().is_some_and(|draft| draft.needs_repair) {
            needs_repair += 1;
        }
        let review_ok = read_review(ctx, &chunk.id)
            .ok()
            .flatten()
            .is_some_and(|review| {
                stamped(ctx, "review", &chunk.id, versions.review)
                    && draft
                        .as_ref()
                        .is_some_and(|draft| review.draft_version == draft.version)
                    && review.verdict == ReviewVerdict::Pass
            });
        if review_ok {
            reviewed += 1;
        }
    }
    let terms = read_constraints(ctx)
        .ok()
        .flatten()
        .map(|constraints| constraints.terms.len() as u32)
        .unwrap_or(0);
    let pages = ctx
        .read_manifest()
        .map(|manifest| manifest.counts.pages)
        .unwrap_or(0);
    crate::agent::JobCounts {
        pages,
        chunks: chunks.len() as u32,
        translated,
        reviewed,
        needs_repair,
        terms,
    }
}

/// 子运行使用的统一取消令牌（父任务取消即传播）。
pub fn child_cancel(ctx: &JobContext) -> CancelToken {
    ctx.control.cancel.child()
}

/// 供测试断言：计划中的块数（包括细分后的派生块）。
pub fn planned_chunk_count(ctx: &JobContext) -> usize {
    plan_chunks(ctx).map(|chunks| chunks.len()).unwrap_or(0)
}

/// 供测试 / 诊断读取合并报告。
pub fn merge_report(ctx: &JobContext) -> Option<MergeReport> {
    ctx.with_artifacts(|artifacts| artifacts.read_merge_report().ok().flatten())
}

/// 供测试 / 诊断读取全部块 ID。
pub fn all_chunk_ids(ctx: &JobContext) -> Vec<String> {
    list_chunks(ctx).unwrap_or_default()
}

/// 完成判据用：读单块草稿（只按计划内的块判定）。
pub fn read_draft_for_verify(ctx: &JobContext, chunk_id: &str) -> Result<Option<Draft>, String> {
    read_draft(ctx, chunk_id)
}

/// 完成判据用：读单块审查结论。
pub fn read_review_for_verify(ctx: &JobContext, chunk_id: &str) -> Result<Option<Review>, String> {
    read_review(ctx, chunk_id)
}

// ----------------------------------------------------------------------
// 单元测试（工具契约 / 阶段依赖 / 诊断）
// ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_reset_invalidates_only_target_and_downstream() {
        let mut versions = PhaseVersions::default();
        let before = versions;

        versions.reset(Phase::PrepareConstraints);
        assert_eq!(versions.prepare_source, before.prepare_source, "上游保留");
        assert!(versions.prepare_constraints > before.prepare_constraints);
        assert!(versions.translate > before.translate);
        assert!(versions.review > before.review);
        assert!(versions.global_review > before.global_review);

        let mut versions = PhaseVersions::default();
        let before = versions;
        versions.reset(Phase::Review);
        assert_eq!(versions.translate, before.translate, "Pass 2 保留");
        assert!(versions.review > before.review);
        assert!(versions.global_review > before.global_review);

        let mut versions = PhaseVersions::default();
        let before = versions;
        versions.reset(Phase::PrepareSource);
        assert!(versions.prepare_source > before.prepare_source);
        assert!(versions.prepare_constraints > before.prepare_constraints);
    }

    #[test]
    fn phase_get_maps_repair_and_publish_to_their_groups() {
        let versions = PhaseVersions {
            prepare_source: 3,
            prepare_constraints: 5,
            translate: 7,
            review: 11,
            global_review: 13,
        };
        assert_eq!(versions.get(Phase::PrepareSource), 3);
        assert_eq!(versions.get(Phase::PrepareConstraints), 5);
        assert_eq!(versions.get(Phase::Translate), 7);
        assert_eq!(versions.get(Phase::Repair), 7, "返工属于翻译组");
        assert_eq!(versions.get(Phase::Review), 11);
        assert_eq!(versions.get(Phase::GlobalReview), 13);
        assert_eq!(versions.get(Phase::Publish), 13, "发布属于复核组");
    }

    #[test]
    fn coordinator_contract_names_every_documented_tool() {
        for name in [
            "load_resource",
            "prepare_source",
            "read_chunk_summary",
            "read_chunk",
            "submit_scan",
            "freeze_constraints",
            "dispatch_translation",
            "dispatch_review",
            "read_review_summary",
            "merge_drafts",
            "record_global_review",
            "publish",
            "reset_phase",
            "job_status",
        ] {
            assert!(
                COORDINATOR_TOOL_CONTRACT.contains(name),
                "协调者契约缺少 {name}"
            );
        }
        for name in ["load_resource", "read_chunk", "submit_translation"] {
            assert!(TRANSLATOR_TOOL_CONTRACT.contains(name));
        }
        for name in [
            "load_resource",
            "read_chunk",
            "read_translation",
            "submit_review",
        ] {
            assert!(REVIEWER_TOOL_CONTRACT.contains(name));
        }
        // 角色不得越权：翻译 / 审查契约里不出现协调者写入工具。
        assert!(!TRANSLATOR_TOOL_CONTRACT.contains("dispatch_translation"));
        assert!(!REVIEWER_TOOL_CONTRACT.contains("submit_translation"));
        assert!(!REVIEWER_TOOL_CONTRACT.contains("publish"));
    }

    #[test]
    fn tool_error_payloads_are_structured_and_side_effect_free() {
        let error = tool_error("unknown_chunk", "分块不存在");
        let payload: Value =
            serde_json::from_str(&error.model_output().render()).expect("结构化错误是 JSON");
        assert_eq!(payload["error"]["code"], "unknown_chunk");
        assert_eq!(payload["error"]["message"], "分块不存在");

        let error = tool_error_with(
            "unknown_chunk",
            "分块不在当前计划中",
            json!({"unknown": ["c9999"]}),
        );
        let payload: Value = serde_json::from_str(&error.model_output().render()).unwrap();
        assert_eq!(payload["unknown"][0], "c9999");
    }

    #[test]
    fn tool_result_truncates_over_limit_with_a_summary() {
        let big = "字".repeat(TOOL_RESULT_TOKEN_LIMIT as usize * 2);
        let output = tool_result(json!({"text": big})).expect("成功返回");
        let payload: Value =
            serde_json::from_str(output.as_text().expect("文本输出")).expect("JSON");
        assert_eq!(payload["truncated"], true);
        assert_eq!(payload["limit"], TOOL_RESULT_TOKEN_LIMIT);
        assert!(
            payload["summary"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );

        let small = tool_result(json!({"chunks": 2})).expect("成功返回");
        let payload: Value = serde_json::from_str(small.as_text().unwrap()).unwrap();
        assert!(payload.get("truncated").is_none(), "未超限时不加截断标记");
    }

    #[test]
    fn plan_paths_are_job_relative_and_kind_specific() {
        assert_eq!(PLAN_PATH, "preparation/chunk-plan.json");
        assert_eq!(meta_path("draft", "c0001"), "chunks/c0001.draft.meta.json");
        assert_eq!(meta_path("review", "c0001"), "reviews/c0001.meta.json");
        assert_eq!(
            meta_path("scan", "c0001"),
            "preparation/scan-c0001.meta.json"
        );
        assert_eq!(
            meta_path("global_review", "global"),
            "output/global-review.meta.json"
        );
        // 路径只能是相对路径，不能含盘符或父目录。
        for path in [
            PLAN_PATH,
            PHASES_PATH,
            RESET_NOTES_PATH,
            &meta_path("draft", "c0001"),
        ] {
            assert!(!path.starts_with('/') && !path.contains(".."));
        }
    }
}
