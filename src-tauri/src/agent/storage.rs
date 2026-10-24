//! 缓存拥有者：一个 Job 的目录与 manifest。
//!
//! 目录固定为 `data/translations/v2/<novel_id>/<job_id>`，Job 拥有全部中间产物、
//! 会话、检查点与临时文件。所有路径参数只接受相对路径，并拒绝绝对路径、
//! `..`、Windows 盘符 / UNC / `\\?\`、符号链接与 junction 逃逸。
//!
//! 防复活：写入前校验「全局清理世代未变 + Job 目录仍存在 + manifest 未标记
//! deleted + 无 `.deleting` 标记」。校验失败只返回错误，**不重建目录**。

use std::{
    fs,
    io::Write as _,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    agent::{
        AttemptBudget, ConstraintVersion, JobCounts, JobId, JobState, Phase, ResourceVersions,
        SafeError, TokenUsage,
    },
    paths::AppPaths,
};

/// 缓存世代：清理成功后递增；迟到回调携带旧世代即被拒绝。
#[derive(Clone, Debug, Default)]
pub struct CacheGeneration(Arc<AtomicU64>);

impl CacheGeneration {
    pub fn new(value: u64) -> Self {
        Self(Arc::new(AtomicU64::new(value)))
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    /// 递增并返回新世代。
    pub fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// 一个 Job 的稳定身份。`generation` 是创建/恢复时观察到的缓存世代。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobScope {
    pub novel_id: i64,
    pub job_id: JobId,
    pub generation: u64,
}

/// Job manifest；字段沿用冻结接口，schema 固定为 2。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub job_id: JobId,
    pub novel_id: i64,
    pub generation: u64,
    pub created_at: String,
    pub updated_at: String,
    pub state: JobState,
    pub phase: Phase,
    pub input_hash: String,
    pub target_language: String,
    pub source_language: String,
    pub resources: ResourceVersions,
    pub settings_snapshot: Value,
    pub counts: JobCounts,
    pub usage: TokenUsage,
    pub attempts: AttemptBudget,
    pub constraint_version: ConstraintVersion,
    pub last_error: Option<SafeError>,
}

/// 当前 manifest schema 版本。
pub const MANIFEST_SCHEMA: u32 = 2;

impl Manifest {
    /// 新建一个 queued 状态的最小 manifest。
    pub fn new(scope: &JobScope) -> Self {
        let now = now_rfc3339();
        Self {
            schema: MANIFEST_SCHEMA,
            job_id: scope.job_id.clone(),
            novel_id: scope.novel_id,
            generation: scope.generation,
            created_at: now.clone(),
            updated_at: now,
            state: JobState::Queued,
            phase: Phase::PrepareSource,
            input_hash: String::new(),
            target_language: String::new(),
            source_language: String::new(),
            resources: ResourceVersions::current(),
            settings_snapshot: Value::Object(Map::new()),
            counts: JobCounts::default(),
            usage: TokenUsage::default(),
            attempts: AttemptBudget::default(),
            constraint_version: 0,
            last_error: None,
        }
    }
}

/// 存储层错误。所有错误都带明确类别，调用方可区分「不可用」与「越界」。
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("路径必须是 Job 内的相对路径: {0}")]
    InvalidPath(String),
    #[error("Job 目录不存在或不可用: {0}")]
    JobMissing(String),
    #[error("Job 正在删除（存在 .deleting 标记）")]
    Deleting,
    #[error("Job 已被标记删除，拒绝写入")]
    JobDeleted,
    #[error("缓存世代不一致：期望 {expected}，当前 {actual}")]
    GenerationMismatch { expected: u64, actual: u64 },
    #[error("manifest 缺失")]
    MissingManifest,
    #[error("manifest 损坏: {0}")]
    CorruptManifest(String),
    #[error("路径逃逸出 Job 根目录: {0}")]
    RootEscape(String),
    #[error("Job 标识无效: {0}")]
    InvalidJobId(String),
    #[error("IO 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 失败: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// 目录列表里的一项；损坏或删除中的目录不作为可恢复任务。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSummary {
    pub job_id: JobId,
    pub path: PathBuf,
    pub state: Option<JobState>,
    /// 无 manifest、manifest 损坏或身份不一致。
    pub corrupt: bool,
    /// 存在 `.deleting`：清理未完成，不可打开。
    pub pending_delete: bool,
}

/// 一个 Job 的存储句柄。
pub struct JobStore {
    root: PathBuf,
    canonical_root: PathBuf,
    v2_root: PathBuf,
    scope: JobScope,
    generation: Option<CacheGeneration>,
}

const JOB_SUBDIRS: [&str; 9] = [
    "source",
    "preparation",
    "chunks",
    "reviews",
    "output",
    "sessions",
    "checkpoints",
    "temporary",
    "",
];

const DELETING_MARKER: &str = ".deleting";
const MANIFEST_FILE: &str = "manifest.json";

impl JobStore {
    /// 创建（或补齐）Job 目录并确保 manifest 存在。不会覆盖已有 manifest。
    pub fn create(paths: &AppPaths, scope: &JobScope) -> Result<Self> {
        validate_scope(scope)?;
        let (v2_root, root) = job_paths(paths, scope);
        fs::create_dir_all(&v2_root)?;
        fs::create_dir_all(&root)?;
        ensure_not_reparse(&root)?;
        for dir in JOB_SUBDIRS {
            if !dir.is_empty() {
                fs::create_dir_all(root.join(dir))?;
            }
        }
        let canonical_root = root.canonicalize()?;
        let canonical_v2 = v2_root.canonicalize()?;
        if !canonical_root.starts_with(&canonical_v2) {
            return Err(StoreError::RootEscape(root.display().to_string()));
        }
        let store = Self {
            root,
            canonical_root,
            v2_root,
            scope: scope.clone(),
            generation: None,
        };
        if store.root.join(DELETING_MARKER).exists() {
            return Err(StoreError::Deleting);
        }
        if !store.root.join(MANIFEST_FILE).is_file() {
            let manifest = Manifest::new(scope);
            let bytes = serde_json::to_vec_pretty(&manifest)?;
            store.write_atomic_inner(MANIFEST_FILE, &bytes, false)?;
        }
        Ok(store)
    }

    /// 创建并绑定全局世代；写入前会校验世代未变。
    pub fn create_guarded(
        paths: &AppPaths,
        scope: &JobScope,
        generation: CacheGeneration,
    ) -> Result<Self> {
        let store = Self::create(paths, scope)?;
        store.with_generation(generation)
    }

    /// 打开已有 Job；`.deleting`、deleted、损坏 manifest 与世代不一致都拒绝。
    pub fn open(paths: &AppPaths, scope: &JobScope) -> Result<Self> {
        validate_scope(scope)?;
        let (v2_root, root) = job_paths(paths, scope);
        let meta = fs::symlink_metadata(&root)
            .map_err(|_| StoreError::JobMissing(root.display().to_string()))?;
        if !meta.is_dir() || is_reparse(&meta) {
            return Err(StoreError::JobMissing(root.display().to_string()));
        }
        if root.join(DELETING_MARKER).exists() {
            return Err(StoreError::Deleting);
        }
        let canonical_root = root.canonicalize()?;
        let canonical_v2 = v2_root.canonicalize()?;
        if !canonical_root.starts_with(&canonical_v2) {
            return Err(StoreError::RootEscape(root.display().to_string()));
        }
        let store = Self {
            root,
            canonical_root,
            v2_root,
            scope: scope.clone(),
            generation: None,
        };
        let manifest = store.read_manifest()?;
        if manifest.generation != scope.generation {
            return Err(StoreError::GenerationMismatch {
                expected: scope.generation,
                actual: manifest.generation,
            });
        }
        if manifest.state == JobState::Deleted {
            return Err(StoreError::JobDeleted);
        }
        Ok(store)
    }

    /// 打开并绑定全局世代。
    pub fn open_guarded(
        paths: &AppPaths,
        scope: &JobScope,
        generation: CacheGeneration,
    ) -> Result<Self> {
        let store = Self::open(paths, scope)?;
        store.with_generation(generation)
    }

    /// 绑定全局世代并立即校验一次（含 manifest 中记录的世代）。
    pub fn with_generation(mut self, generation: CacheGeneration) -> Result<Self> {
        let actual = generation.get();
        if actual != self.scope.generation {
            return Err(StoreError::GenerationMismatch {
                expected: self.scope.generation,
                actual,
            });
        }
        // 绑定后再读一次 manifest，确保打开与绑定之间没有被清理。
        let manifest = self.read_manifest()?;
        if manifest.generation != self.scope.generation {
            return Err(StoreError::GenerationMismatch {
                expected: self.scope.generation,
                actual: manifest.generation,
            });
        }
        self.generation = Some(generation);
        Ok(self)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn scope(&self) -> &JobScope {
        &self.scope
    }

    /// 读取相对路径；不存在返回 `Ok(None)`。符号链接目标视为不存在。
    pub fn read(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        let abs = self.validated_target(rel)?;
        let Ok(meta) = fs::symlink_metadata(&abs) else {
            return Ok(None);
        };
        if is_reparse(&meta) || !meta.is_file() {
            return Ok(None);
        }
        Ok(Some(fs::read(abs)?))
    }

    /// 原子写入：先写 `temporary/`，再 rename 到目标。
    pub fn write_atomic(&self, rel: &str, bytes: &[u8]) -> Result<()> {
        self.write_atomic_inner(rel, bytes, true)
    }

    /// `check_live = false` 只用于创建 Job 时的首个 manifest。
    fn write_atomic_inner(&self, rel: &str, bytes: &[u8], check_live: bool) -> Result<()> {
        let abs = self.validated_target(rel)?;
        if check_live {
            self.ensure_live()?;
        }
        let parent = abs
            .parent()
            .ok_or_else(|| StoreError::InvalidPath(rel.to_owned()))?;
        fs::create_dir_all(parent)?;
        let canonical_parent = parent.canonicalize()?;
        if !canonical_parent.starts_with(&self.canonical_root) {
            return Err(StoreError::RootEscape(rel.to_owned()));
        }
        if let Ok(meta) = fs::symlink_metadata(&abs)
            && is_reparse(&meta)
        {
            return Err(StoreError::InvalidPath(rel.to_owned()));
        }

        let temp_dir = self.root.join("temporary");
        fs::create_dir_all(&temp_dir)?;
        let canonical_temp = temp_dir.canonicalize()?;
        if !canonical_temp.starts_with(&self.canonical_root) {
            return Err(StoreError::RootEscape(rel.to_owned()));
        }
        let temp = temp_dir.join(format!(
            "write-{}-{}.tmp",
            std::process::id(),
            next_temp_id()
        ));
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        if let Err(err) = fs::rename(&temp, &abs) {
            // Windows 不允许 rename 覆盖已有文件；目标存在时先移除再重命一次。
            let target_exists = abs.exists();
            let retry = if target_exists {
                fs::remove_file(&abs).and_then(|()| fs::rename(&temp, &abs))
            } else {
                Err(err)
            };
            if let Err(err) = retry {
                let _ = fs::remove_file(&temp);
                return Err(StoreError::Io(err));
            }
        }
        Ok(())
    }

    pub fn exists(&self, rel: &str) -> bool {
        let Ok(abs) = self.validated_target(rel) else {
            return false;
        };
        match fs::symlink_metadata(&abs) {
            Ok(meta) => !is_reparse(&meta) && meta.is_file(),
            Err(_) => false,
        }
    }

    /// 删除本 Job 根目录。先写 `.deleting`，失败时保留标记并返回错误。
    pub fn remove_tree(&self) -> Result<()> {
        let meta = fs::symlink_metadata(&self.root)
            .map_err(|_| StoreError::JobMissing(self.root.display().to_string()))?;
        if !meta.is_dir() || is_reparse(&meta) {
            return Err(StoreError::JobMissing(self.root.display().to_string()));
        }
        // 只允许删除 `<v2>/<novel>/<job>` 这一层。
        let expected_parent = self.v2_root.join(self.scope.novel_id.to_string());
        if self.root.parent() != Some(expected_parent.as_path())
            || self.root.file_name().and_then(|name| name.to_str())
                != Some(self.scope.job_id.as_str())
        {
            return Err(StoreError::InvalidPath(self.root.display().to_string()));
        }
        if let Err(err) = fs::write(self.root.join(DELETING_MARKER), b"1") {
            return Err(StoreError::Io(err));
        }
        match fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(err) => Err(StoreError::Io(err)),
        }
    }

    pub fn read_manifest(&self) -> Result<Manifest> {
        let path = self.root.join(MANIFEST_FILE);
        let Ok(bytes) = fs::read(&path) else {
            return Err(StoreError::MissingManifest);
        };
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|err| StoreError::CorruptManifest(err.to_string()))?;
        if manifest.job_id != self.scope.job_id || manifest.novel_id != self.scope.novel_id {
            return Err(StoreError::CorruptManifest(
                "manifest 与 Job 身份不一致".into(),
            ));
        }
        Ok(manifest)
    }

    /// 读-改-写 manifest（原子写）。
    pub fn update_manifest(&self, f: impl FnOnce(&mut Manifest)) -> Result<()> {
        self.ensure_live()?;
        let mut manifest = self.read_manifest()?;
        f(&mut manifest);
        manifest.updated_at = now_rfc3339();
        self.write_manifest(&manifest)
    }

    /// 记录阶段检查点：manifest 的 phase 与 `checkpoints/latest.json` 同批更新。
    pub fn touch_checkpoint(&self, phase: Phase) -> Result<()> {
        self.update_manifest(|manifest| manifest.phase = phase)?;
        let checkpoint = serde_json::json!({
            "phase": phase,
            "updated_at": now_rfc3339(),
        });
        let bytes = serde_json::to_vec_pretty(&checkpoint)?;
        self.write_atomic("checkpoints/latest.json", &bytes)
    }

    fn write_manifest(&self, manifest: &Manifest) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(manifest)?;
        self.write_atomic(MANIFEST_FILE, &bytes)
    }

    /// 写入前的存活校验：世代、目录、`.deleting`、manifest 状态。
    fn ensure_live(&self) -> Result<()> {
        if let Some(generation) = &self.generation {
            let actual = generation.get();
            if actual != self.scope.generation {
                return Err(StoreError::GenerationMismatch {
                    expected: self.scope.generation,
                    actual,
                });
            }
        }
        let meta = fs::symlink_metadata(&self.root)
            .map_err(|_| StoreError::JobMissing(self.root.display().to_string()))?;
        if !meta.is_dir() || is_reparse(&meta) {
            return Err(StoreError::JobMissing(self.root.display().to_string()));
        }
        if self.root.join(DELETING_MARKER).exists() {
            return Err(StoreError::Deleting);
        }
        let manifest = self.read_manifest()?;
        if manifest.generation != self.scope.generation {
            return Err(StoreError::GenerationMismatch {
                expected: self.scope.generation,
                actual: manifest.generation,
            });
        }
        if manifest.state == JobState::Deleted {
            return Err(StoreError::JobDeleted);
        }
        Ok(())
    }

    /// 相对路径校验 + 现有祖先的符号链接检查。
    fn validated_target(&self, rel: &str) -> Result<PathBuf> {
        let rel_path = validate_relative(rel)?;
        let target = self.root.join(&rel_path);
        let mut cursor = self.root.clone();
        if let Ok(meta) = fs::symlink_metadata(&cursor)
            && is_reparse(&meta)
        {
            return Err(StoreError::RootEscape(rel.to_owned()));
        }
        for component in rel_path.components() {
            let Component::Normal(part) = component else {
                return Err(StoreError::InvalidPath(rel.to_owned()));
            };
            cursor.push(part);
            if let Ok(meta) = fs::symlink_metadata(&cursor)
                && is_reparse(&meta)
            {
                return Err(StoreError::RootEscape(rel.to_owned()));
            }
        }
        Ok(target)
    }
}

fn validate_scope(scope: &JobScope) -> Result<()> {
    if scope.novel_id <= 0 {
        return Err(StoreError::InvalidJobId(format!(
            "novel_id 必须为正整数: {}",
            scope.novel_id
        )));
    }
    let job = scope.job_id.trim();
    if job.is_empty()
        || job.len() > 64
        || !job
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(StoreError::InvalidJobId(scope.job_id.clone()));
    }
    Ok(())
}

fn job_paths(paths: &AppPaths, scope: &JobScope) -> (PathBuf, PathBuf) {
    let v2_root = paths.data_dir.join("translations").join("v2");
    let root = v2_root
        .join(scope.novel_id.to_string())
        .join(scope.job_id.trim());
    (v2_root, root)
}

/// 相对路径白名单：只有普通组件，拒绝盘符、UNC、`\\?\`、`..`、`.` 与越界字符。
fn validate_relative(rel: &str) -> Result<PathBuf> {
    if rel.is_empty() || rel.len() > 512 || rel.contains('\0') {
        return Err(StoreError::InvalidPath(rel.to_owned()));
    }
    if rel.starts_with('\\') || rel.starts_with("//") {
        return Err(StoreError::InvalidPath(rel.to_owned()));
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(StoreError::InvalidPath(rel.to_owned()));
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.is_empty()
                    || part == DELETING_MARKER
                    || part.contains(':')
                    || part.contains('\0')
                {
                    return Err(StoreError::InvalidPath(rel.to_owned()));
                }
                clean.push(part.as_ref());
            }
            _ => return Err(StoreError::InvalidPath(rel.to_owned())),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(StoreError::InvalidPath(rel.to_owned()));
    }
    Ok(clean)
}

fn ensure_not_reparse(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if is_reparse(&meta) {
        return Err(StoreError::RootEscape(path.display().to_string()));
    }
    Ok(())
}

/// 符号链接 / Windows junction（重解析点）。
fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(target_os = "windows"))]
    {
        meta.file_type().is_symlink()
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn next_temp_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// 列出某本小说的全部 Job，按目录名倒序；损坏/删除中的目录只作标记。
pub fn list_jobs(paths: &AppPaths, novel_id: i64) -> Result<Vec<JobSummary>> {
    let dir = paths
        .data_dir
        .join("translations")
        .join("v2")
        .join(novel_id.to_string());
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if is_reparse(&meta) {
            continue;
        }
        let path = entry.path();
        let Some(job_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let pending_delete = path.join(DELETING_MARKER).exists();
        if pending_delete {
            jobs.push(JobSummary {
                job_id,
                path,
                state: None,
                corrupt: true,
                pending_delete: true,
            });
            continue;
        }
        let manifest = fs::read(path.join(MANIFEST_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok());
        match manifest {
            Some(manifest) if manifest.job_id == job_id && manifest.novel_id == novel_id => {
                jobs.push(JobSummary {
                    job_id,
                    path,
                    state: Some(manifest.state),
                    corrupt: false,
                    pending_delete: false,
                });
            }
            _ => jobs.push(JobSummary {
                job_id,
                path,
                state: None,
                corrupt: true,
                pending_delete: false,
            }),
        }
    }
    jobs.sort_by(|a, b| b.job_id.cmp(&a.job_id));
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        paths: AppPaths,
        dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture() -> Fixture {
        let dir = std::env::temp_dir().join(format!("pixiv-agent-store-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths {
            data_dir: dir.join("data"),
            config_dir: dir.join("config"),
            logs_dir: dir.join("data").join("logs"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();
        Fixture { paths, dir }
    }

    fn scope(generation: u64) -> JobScope {
        JobScope {
            novel_id: 42,
            job_id: "20261024210000-abc123".into(),
            generation,
        }
    }

    #[test]
    fn atomic_write_round_trips_and_leaves_no_temp_files() {
        let fixture = fixture();
        let store = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        store
            .write_atomic("chunks/c0001.draft.json", b"{\"v\":1}")
            .unwrap();
        store
            .write_atomic("chunks/c0001.draft.json", b"{\"v\":2}")
            .unwrap();
        assert_eq!(
            store.read("chunks/c0001.draft.json").unwrap().as_deref(),
            Some(&b"{\"v\":2}"[..])
        );
        assert!(!store.exists("chunks/missing.json"));
        assert!(store.root().join("chunks/c0001.draft.json").is_file());
        let temporaries: Vec<_> = fs::read_dir(store.root().join("temporary"))
            .unwrap()
            .collect();
        assert!(temporaries.is_empty(), "提交后不应遗留临时文件");
        assert!(store.read_manifest().is_ok());
    }

    #[test]
    fn rejects_escape_paths_without_touching_outside() {
        let fixture = fixture();
        let store = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        for rel in [
            "../escape.json",
            "chunks/../../escape.json",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "C:relative",
            "\\\\server\\share\\x",
            "\\\\?\\C:\\x",
            ".deleting",
            "chunks/manifest.json/../../x",
            "",
        ] {
            assert!(store.write_atomic(rel, b"x").is_err(), "{rel} 必须拒绝写入");
            assert!(store.read(rel).is_err(), "{rel} 必须拒绝读取");
        }
        assert!(!fixture.paths.data_dir.join("escape.json").exists());
    }

    #[test]
    fn generation_change_blocks_writes_and_open() {
        let fixture = fixture();
        let generation = CacheGeneration::new(7);
        let scope = scope(7);
        let store = JobStore::create_guarded(&fixture.paths, &scope, generation.clone()).unwrap();
        store.write_atomic("chunks/a.json", b"1").unwrap();
        generation.bump();
        let err = store.write_atomic("chunks/a.json", b"2").unwrap_err();
        assert!(matches!(err, StoreError::GenerationMismatch { .. }));
        assert!(
            JobStore::open_guarded(&fixture.paths, &scope, generation).is_err(),
            "旧世代不得打开"
        );
        let stale = scope_with_generation(8);
        assert!(JobStore::open(&fixture.paths, &stale).is_err());
    }

    fn scope_with_generation(generation: u64) -> JobScope {
        JobScope {
            generation,
            ..scope(generation)
        }
    }

    #[test]
    fn remove_tree_marks_then_deletes_and_write_fails_afterwards() {
        let fixture = fixture();
        let store = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        store.write_atomic("output/result.json", b"{}").unwrap();
        let root = store.root().to_path_buf();
        store.remove_tree().unwrap();
        assert!(!root.exists(), "删除后 Job 根目录必须消失");
        assert!(
            matches!(
                store.write_atomic("output/result.json", b"{}"),
                Err(StoreError::JobMissing(_))
            ),
            "删除后写入必须失败且不重建目录"
        );
        assert!(!root.exists());
    }

    #[test]
    fn repeated_delete_reports_failure_instead_of_success() {
        let fixture = fixture();
        let store = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        store.remove_tree().expect("首次删除成功");
        assert!(
            matches!(store.remove_tree(), Err(StoreError::JobMissing(_))),
            "已不存在的 Job 再次删除必须返回错误，不能静默成功"
        );
    }

    #[test]
    fn deleting_marker_blocks_open_and_write() {
        let fixture = fixture();
        let scope = scope(1);
        let store = JobStore::create(&fixture.paths, &scope).unwrap();
        fs::write(store.root().join(DELETING_MARKER), b"1").unwrap();
        assert!(matches!(
            JobStore::open(&fixture.paths, &scope),
            Err(StoreError::Deleting)
        ));
        assert!(matches!(
            store.write_atomic("chunks/a.json", b"1"),
            Err(StoreError::Deleting)
        ));
    }

    #[test]
    fn deleted_manifest_blocks_writes_without_recreating() {
        let fixture = fixture();
        let store = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        store
            .update_manifest(|manifest| manifest.state = JobState::Deleted)
            .unwrap();
        assert!(matches!(
            store.write_atomic("source/source.json", b"{}"),
            Err(StoreError::JobDeleted)
        ));
        assert!(matches!(
            JobStore::open(&fixture.paths, &scope(1)),
            Err(StoreError::JobDeleted)
        ));
        assert!(store.root().is_dir(), "拒绝写入不等于删除目录");
    }

    #[test]
    fn corrupt_manifest_is_flagged_not_resumable() {
        let fixture = fixture();
        let good = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        fs::write(good.root().join(MANIFEST_FILE), b"not json").unwrap();
        let newer = JobScope {
            job_id: "20261024210001-def456".into(),
            ..scope(1)
        };
        let _newer = JobStore::create(&fixture.paths, &newer).unwrap();
        let jobs = list_jobs(&fixture.paths, 42).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_id, "20261024210001-def456", "按目录名倒序");
        assert!(!jobs[0].corrupt);
        assert!(jobs[1].corrupt, "损坏 manifest 必须标记 corrupt");
        assert!(jobs[1].state.is_none());
    }

    #[test]
    fn touch_checkpoint_updates_manifest_and_latest() {
        let fixture = fixture();
        let store = JobStore::create(&fixture.paths, &scope(1)).unwrap();
        store.touch_checkpoint(Phase::Translate).unwrap();
        assert_eq!(store.read_manifest().unwrap().phase, Phase::Translate);
        let latest = store.read("checkpoints/latest.json").unwrap().unwrap();
        let value: Value = serde_json::from_slice(&latest).unwrap();
        assert_eq!(value["phase"], "translate");
    }
}
