//! 全局模型请求调度：全局 FIFO 闸门 + 单小说上限 + 活跃小说上限。
//!
//! 全局闸默认自适应（AIMD，ADR 0050）：模型请求连续成功时容量倍增爬升逼近
//! 设置上限；限流 / 超时 / 断流等负信号把容量减半。关闭自适应时容量恒等于
//! 上限（静态语义）。实际并发取「全局并发」与「单小说并发」的较小值，因此
//! 一本长篇无法占满所有槽位。等待许可可被取消（tokio `Semaphore` 的等待者
//! 随 future drop 移除，不泄漏许可）。

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};

use super::FailureClass;
use crate::settings::Settings;

/// 调度参数（来自二级高级设置；越界值在构造时收敛到安全区间）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// 全局模型请求并发的自适应上限，1~16；自适应关闭时即固定并发。
    pub global_concurrency: usize,
    /// 单小说子任务并发，1~16，实际受全局上限约束。
    pub novel_concurrency: usize,
    /// 同时活跃小说数，1~8。
    pub active_novels: usize,
    /// 全局并发是否自适应收敛（AIMD）。关闭时容量恒等于 `global_concurrency`。
    pub adaptive_concurrency: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            global_concurrency: 8,
            novel_concurrency: 6,
            active_novels: 2,
            adaptive_concurrency: true,
        }
    }
}

impl RuntimeConfig {
    /// 从设置快照构造；非法区间收敛而不是让调度器 panic。
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            global_concurrency: clamp(settings.translation_agent_global_concurrency, 1, 16),
            novel_concurrency: clamp(settings.translation_agent_novel_concurrency, 1, 16),
            active_novels: clamp(settings.translation_agent_active_novels, 1, 8),
            adaptive_concurrency: settings.translation_agent_adaptive_concurrency,
        }
    }

    fn normalized(mut self) -> Self {
        self.global_concurrency = self.global_concurrency.clamp(1, 16);
        self.novel_concurrency = self.novel_concurrency.clamp(1, 16);
        self.active_novels = self.active_novels.clamp(1, 8);
        self
    }
}

fn clamp(value: i64, min: usize, max: usize) -> usize {
    (value.max(min as i64).min(max as i64)) as usize
}

/// 连续成功达到该次数后容量倍增（slow-start 式爬升）。
const AIMD_SUCCESS_THRESHOLD: u32 = 2;

/// 全局闸的自适应状态。
struct AimdState {
    /// 当前生效容量（目标值）。
    current: usize,
    /// 连续成功计数；达到阈值时容量倍增爬升。
    consecutive_success: u32,
    /// 缩容时未能立即回收的许可数：由 [`DynamicGatePermit`] 归还时吞掉。
    deficit: usize,
}

/// 全局模型请求闸：tokio `Semaphore` + AIMD 容量控制。
///
/// 扩容直接 `add_permits`；缩容尽量立即 `forget` 可用许可，差额记入
/// `deficit`，由在途许可归还时吞掉（不再返还），最终收敛到目标容量。
struct DynamicGate {
    sem: Arc<Semaphore>,
    state: Mutex<AimdState>,
    max: usize,
    adaptive: bool,
}

impl DynamicGate {
    fn new(max: usize, adaptive: bool) -> Self {
        let initial = if adaptive {
            // 半上限起步：连续成功即倍增，两个请求周期内逼近上限；
            // 冷启动即满速容易直接触发限流。
            max.div_ceil(2).max(1)
        } else {
            max
        };
        Self {
            sem: Arc::new(Semaphore::new(initial)),
            state: Mutex::new(AimdState {
                current: initial,
                consecutive_success: 0,
                deficit: 0,
            }),
            max,
            adaptive,
        }
    }

    async fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.sem.clone().acquire_owned().await.ok()
    }

    fn available(&self) -> usize {
        self.sem.available_permits()
    }

    fn current_capacity(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.current)
            .unwrap_or(self.max)
    }

    /// 成功完成一次模型请求：连续成功达阈值时容量倍增（封顶上限）。
    fn report_success(&self) {
        if !self.adaptive {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.consecutive_success = state.consecutive_success.saturating_add(1);
        if state.consecutive_success < AIMD_SUCCESS_THRESHOLD || state.current >= self.max {
            return;
        }
        state.consecutive_success = 0;
        let old = state.current;
        let new = old.saturating_mul(2).min(self.max);
        Self::apply(&self.sem, &mut state, new);
        log::info!("AIMD 全局调度容量 {old}→{new}（连续成功爬升）");
    }

    /// 一次模型请求失败：只有限流 / 超时 / 断流类负信号才减半容量；
    /// 鉴权、余额、内容拒绝等与速率无关的错误不收缩。
    fn report_failure(&self, class: FailureClass) {
        if !self.adaptive {
            return;
        }
        if !matches!(
            class,
            FailureClass::Retryable | FailureClass::UnknownOutcome | FailureClass::NotSent
        ) {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.consecutive_success = 0;
        if state.current <= 1 {
            return;
        }
        let old = state.current;
        let new = (state.current / 2).max(1);
        Self::apply(&self.sem, &mut state, new);
        log::info!("AIMD 全局调度容量 {old}→{new}（负信号：{class}）");
    }

    /// 把目标容量应用到信号量；返回后 `state.current == new`。
    fn apply(sem: &Semaphore, state: &mut AimdState, new: usize) {
        if new > state.current {
            sem.add_permits(new - state.current);
        } else if new < state.current {
            let want = state.current - new;
            let mut removed = 0;
            while removed < want {
                match sem.try_acquire() {
                    Ok(permit) => {
                        permit.forget();
                        removed += 1;
                    }
                    Err(_) => break,
                }
            }
            state.deficit += want - removed;
        }
        state.current = new;
    }

    /// 许可归还：缩容欠账未清时吞掉本次归还（不再返还信号量）。
    fn reclaim_on_release(&self) -> bool {
        if !self.adaptive {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.deficit > 0 {
            state.deficit -= 1;
            true
        } else {
            false
        }
    }
}

/// 全局闸许可；drop 归还，缩容欠账时被吞掉（协助容量收敛）。
struct DynamicGatePermit {
    /// `None` 表示已被 forget 消费。
    permit: Option<OwnedSemaphorePermit>,
    gate: Arc<DynamicGate>,
}

impl Drop for DynamicGatePermit {
    fn drop(&mut self) {
        if self.gate.reclaim_on_release()
            && let Some(permit) = self.permit.take()
        {
            permit.forget();
        }
    }
}

struct NovelSlot {
    /// 单小说模型请求上限。
    model: Arc<Semaphore>,
    /// 当前进入该小说的句柄数（含等待活跃小说许可的调用）。
    active: AtomicUsize,
    /// 该小说持有的活跃小说许可；最后一个句柄离开时释放。
    active_permit: AsyncMutex<Option<OwnedSemaphorePermit>>,
}

struct Inner {
    global: Arc<DynamicGate>,
    active_novels: Arc<Semaphore>,
    novel_concurrency: usize,
    slots: Mutex<HashMap<i64, Arc<NovelSlot>>>,
}

/// 公平调度器。克隆共享同一组信号量。
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
    config: RuntimeConfig,
}

impl Scheduler {
    pub fn new(config: &RuntimeConfig) -> Self {
        let config = config.clone().normalized();
        Self {
            inner: Arc::new(Inner {
                global: Arc::new(DynamicGate::new(
                    config.global_concurrency,
                    config.adaptive_concurrency,
                )),
                active_novels: Arc::new(Semaphore::new(config.active_novels)),
                novel_concurrency: config.novel_concurrency,
                slots: Mutex::new(HashMap::new()),
            }),
            config,
        }
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// 进入一本活跃小说；同一小说的多个句柄共享一个活跃槽位。
    ///
    /// 第一个句柄取得活跃许可后存放在 slot 中，后续句柄直接共享；最后一个句柄
    /// 离开时才释放。等待期间被取消不会泄漏计数或许可。
    pub async fn enter_novel(&self, novel_id: i64) -> NovelPermit {
        let slot = self.slot(novel_id);
        slot.active.fetch_add(1, Ordering::SeqCst);
        // 先占住计数再等待；等待期间被取消时 claim 的 Drop 会补偿。
        let claim = ActiveClaim { slot: slot.clone() };
        {
            let mut held = slot.active_permit.lock().await;
            if held.is_none() {
                if let Ok(permit) = self.inner.active_novels.clone().acquire_owned().await {
                    *held = Some(permit);
                }
            }
        }
        NovelPermit { _claim: claim }
    }

    /// 取一次模型请求许可：先单小说、再全局，均为 FIFO。
    pub async fn model_permit(&self, novel_id: i64) -> ModelPermit {
        let slot = self.slot(novel_id);
        let novel = slot.model.clone().acquire_owned().await.ok();
        let global = self.inner.global.acquire().await.map(|permit| {
            let gate = self.inner.global.clone();
            DynamicGatePermit {
                permit: Some(permit),
                gate,
            }
        });
        ModelPermit {
            _slot: slot,
            _novel: novel,
            _global: global,
        }
    }

    /// 模型请求成功完成：自适应容量按连续成功爬升（静态模式为空操作）。
    pub fn report_success(&self) {
        self.inner.global.report_success();
    }

    /// 模型请求失败：限流 / 超时 / 断流类负信号收缩全局容量（静态模式为空操作）。
    pub fn report_failure(&self, class: FailureClass) {
        self.inner.global.report_failure(class);
    }

    /// 当前全局生效容量（自适应目标值，诊断用）。
    pub fn current_global_capacity(&self) -> usize {
        self.inner.global.current_capacity()
    }

    /// 当前全局可用许可（诊断 / 测试）。
    pub fn available_global(&self) -> usize {
        self.inner.global.available()
    }

    /// 当前单小说可用许可（诊断 / 测试）。
    pub fn available_novel(&self, novel_id: i64) -> Option<usize> {
        self.inner.slots.lock().ok().and_then(|slots| {
            slots
                .get(&novel_id)
                .map(|slot| slot.model.available_permits())
        })
    }

    /// 当前登记的活跃小说数（空条目会被顺带清理）。
    pub fn active_novel_count(&self) -> usize {
        let Ok(mut slots) = self.inner.slots.lock() else {
            return 0;
        };
        slots.retain(|_, slot| {
            slot.active.load(Ordering::SeqCst) > 0 || Arc::strong_count(slot) > 1
        });
        slots.len()
    }

    fn slot(&self, novel_id: i64) -> Arc<NovelSlot> {
        let mut slots = self
            .inner
            .slots
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        // 没有句柄、也没有许可在途的条目可以回收。
        slots.retain(|_, slot| {
            slot.active.load(Ordering::SeqCst) > 0 || Arc::strong_count(slot) > 1
        });
        slots
            .entry(novel_id)
            .or_insert_with(|| {
                Arc::new(NovelSlot {
                    model: Arc::new(Semaphore::new(self.inner.novel_concurrency)),
                    active: AtomicUsize::new(0),
                    active_permit: AsyncMutex::new(None),
                })
            })
            .clone()
    }
}

/// 活跃小说句柄；drop 归还活跃槽位。
pub struct NovelPermit {
    _claim: ActiveClaim,
}

/// 活跃计数声明：先计数后等待，取消时 Drop 自动补偿。
struct ActiveClaim {
    slot: Arc<NovelSlot>,
}

impl Drop for ActiveClaim {
    fn drop(&mut self) {
        if self.slot.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            // 最后一个句柄离开：释放活跃小说许可。
            if let Ok(mut held) = self.slot.active_permit.try_lock() {
                *held = None;
            }
        }
        // 空条目由下一次 `slot()` / `active_novel_count()` 统一回收；此处不能
        // 只看强引用数，否则可能误删仍有 ModelPermit 在途的槽位。
    }
}

/// 单次模型请求许可；drop 归还。持有 slot 防止在许可在途时被误回收。
pub struct ModelPermit {
    _slot: Arc<NovelSlot>,
    _novel: Option<OwnedSemaphorePermit>,
    _global: Option<DynamicGatePermit>,
}

impl ModelPermit {
    pub fn has_permits(&self) -> bool {
        self._novel.is_some() && self._global.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn config(global: usize, novel: usize, active: usize) -> RuntimeConfig {
        RuntimeConfig {
            global_concurrency: global,
            novel_concurrency: novel,
            active_novels: active,
            adaptive_concurrency: false,
        }
    }

    fn adaptive_config(max: usize) -> RuntimeConfig {
        RuntimeConfig {
            adaptive_concurrency: true,
            ..config(max, 4, 4)
        }
    }

    #[tokio::test]
    async fn global_limit_is_enforced() {
        let scheduler = Scheduler::new(&config(2, 4, 4));
        let first = scheduler.model_permit(1).await;
        let second = scheduler.model_permit(1).await;
        let third =
            tokio::time::timeout(Duration::from_millis(100), scheduler.model_permit(1)).await;
        assert!(third.is_err(), "全局并发 2 时第三次必须等待");
        drop(first);
        let third = tokio::time::timeout(Duration::from_millis(500), scheduler.model_permit(1))
            .await
            .expect("释放后应获得许可");
        assert!(third.has_permits());
        drop(second);
    }

    #[tokio::test]
    async fn single_novel_limit_is_enforced_even_with_global_capacity() {
        let scheduler = Scheduler::new(&config(8, 2, 4));
        let _a = scheduler.model_permit(1).await;
        let _b = scheduler.model_permit(1).await;
        let blocked =
            tokio::time::timeout(Duration::from_millis(100), scheduler.model_permit(1)).await;
        assert!(blocked.is_err(), "单小说上限 2 时第三次必须等待");
        // 另一本小说仍可立即获得许可（公平性：不出现一本占满）。
        let other = tokio::time::timeout(Duration::from_millis(500), scheduler.model_permit(2))
            .await
            .expect("另一本小说不应被阻塞");
        assert!(other.has_permits());
    }

    #[tokio::test]
    async fn multi_novel_fairness_never_lets_one_novel_take_everything() {
        let scheduler = Scheduler::new(&config(4, 2, 4));
        let _a1 = scheduler.model_permit(1).await;
        let _a2 = scheduler.model_permit(1).await;
        // A 已占满自己的 2 个槽位；B 必须能拿到 2 个，而不是被 A 饿死。
        let b1 = tokio::time::timeout(Duration::from_millis(500), scheduler.model_permit(2))
            .await
            .expect("B 的首个许可");
        let b2 = tokio::time::timeout(Duration::from_millis(500), scheduler.model_permit(2))
            .await
            .expect("B 的第二个许可");
        assert!(b1.has_permits() && b2.has_permits());
        assert_eq!(scheduler.available_global(), 0);
    }

    #[tokio::test]
    async fn dropping_permits_returns_capacity() {
        let scheduler = Scheduler::new(&config(2, 2, 2));
        let permit = scheduler.model_permit(1).await;
        assert_eq!(scheduler.available_global(), 1);
        drop(permit);
        assert_eq!(scheduler.available_global(), 2);
        assert_eq!(scheduler.available_novel(1), Some(2));
    }

    #[tokio::test]
    async fn active_novel_limit_and_reuse_for_same_novel() {
        let scheduler = Scheduler::new(&config(4, 4, 1));
        let first = scheduler.enter_novel(1).await;
        // 同一小说的第二个句柄不额外占活跃槽位。
        let second = tokio::time::timeout(Duration::from_millis(100), scheduler.enter_novel(1))
            .await
            .expect("同一小说可重入");
        let other =
            tokio::time::timeout(Duration::from_millis(100), scheduler.enter_novel(2)).await;
        assert!(other.is_err(), "活跃小说上限 1 时第二本必须排队");
        drop(first);
        drop(second);
        let other = tokio::time::timeout(Duration::from_millis(500), scheduler.enter_novel(2))
            .await
            .expect("释放后可进入第二本");
        drop(other);
        assert_eq!(scheduler.active_novel_count(), 0, "空条目应被清理");
    }

    #[tokio::test]
    async fn cancelling_a_waiter_does_not_leak_permits() {
        let scheduler = Scheduler::new(&config(1, 1, 1));
        let held = scheduler.model_permit(1).await;
        let waiting = {
            let scheduler = scheduler.clone();
            tokio::spawn(async move {
                let _permit = scheduler.model_permit(1).await;
                tokio::time::sleep(Duration::from_secs(30)).await;
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        waiting.abort();
        let _ = waiting.await;
        drop(held);
        let again = tokio::time::timeout(Duration::from_millis(500), scheduler.model_permit(1))
            .await
            .expect("取消等待者后许可必须归还");
        assert!(again.has_permits());
    }

    #[tokio::test]
    async fn aimd_starts_at_half_and_climbs_to_max() {
        let scheduler = Scheduler::new(&adaptive_config(4));
        assert_eq!(scheduler.current_global_capacity(), 2, "半上限起步");
        // 单次成功不爬升；连续两次成功容量倍增到上限。
        scheduler.report_success();
        assert_eq!(scheduler.current_global_capacity(), 2);
        scheduler.report_success();
        assert_eq!(scheduler.current_global_capacity(), 4);
        // 已到上限：继续成功不再变化。
        scheduler.report_success();
        assert_eq!(scheduler.current_global_capacity(), 4);
    }

    #[tokio::test]
    async fn aimd_growth_expands_immediately_available_permits() {
        let scheduler = Scheduler::new(&adaptive_config(4));
        let _first = scheduler.model_permit(1).await;
        let _second = scheduler.model_permit(1).await;
        assert_eq!(scheduler.available_global(), 0);
        scheduler.report_success();
        scheduler.report_success();
        assert_eq!(scheduler.current_global_capacity(), 4);
        let third = tokio::time::timeout(Duration::from_millis(100), scheduler.model_permit(1))
            .await
            .expect("容量爬升后第三个请求应立即可得");
        assert!(third.has_permits());
    }

    #[tokio::test]
    async fn aimd_failure_halves_capacity_reclaiming_inflight_permits() {
        let scheduler = Scheduler::new(&adaptive_config(8));
        assert_eq!(scheduler.current_global_capacity(), 4);
        // 占满初始容量 4（FIFO 下顺序取得）。
        let mut permits = Vec::new();
        for _ in 0..4 {
            permits.push(scheduler.model_permit(1).await);
        }
        assert_eq!(scheduler.available_global(), 0);
        // 负信号减半：目标 2，全部许可在途 → 欠账 2。
        scheduler.report_failure(FailureClass::Retryable);
        assert_eq!(scheduler.current_global_capacity(), 2);
        // 先归还 2 个：被缩容欠账吞掉，可用许可仍为 0（在途 2 = 目标容量 2）。
        drop(permits.pop());
        drop(permits.pop());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(scheduler.available_global(), 0, "归还应被缩容欠账吞掉");
        // 剩余 2 个归还后欠账已清，正常返还：可用 = 目标容量 2。
        drop(permits.pop());
        drop(permits.pop());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(scheduler.available_global(), 2);
    }

    #[tokio::test]
    async fn aimd_ignores_non_rate_failures() {
        let scheduler = Scheduler::new(&adaptive_config(4));
        scheduler.report_failure(FailureClass::Permanent);
        scheduler.report_failure(FailureClass::ContentRefused);
        scheduler.report_failure(FailureClass::InputTooLarge);
        assert_eq!(
            scheduler.current_global_capacity(),
            2,
            "与速率无关的错误不收缩容量"
        );
    }

    #[tokio::test]
    async fn adaptive_disabled_is_static() {
        let scheduler = Scheduler::new(&config(4, 4, 4));
        scheduler.report_success();
        scheduler.report_success();
        scheduler.report_failure(FailureClass::Retryable);
        assert_eq!(
            scheduler.current_global_capacity(),
            4,
            "关闭自适应时容量恒等于上限"
        );
    }
}
