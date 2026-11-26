//! 原生关窗守卫的内核（tauri-free，口径同 `forest.rs`／`sprite_asset.rs`：判定逻辑
//! 抽成可单测的纯状态机，tauri 接线留在 `lib.rs`／`bridge.rs`）。
//!
//! ## 缺陷背景（UI 审计 P1-4）
//!
//! 原生 X／Alt+F4 原先直接走 `CloseRequested` 默认放行，绕过前端 dirty 守卫，
//! 120s 自动保存间隔内的编辑丢失。修复契约：壳拦下 `CloseRequested`，广播
//! `window-close-requested`（空载荷），前端确认放弃／已保存后调 `window_confirm_close`
//! 真正关窗；前端未接线（旧前端没有监听者）时由兜底计时放行，窗口不能关不掉。
//!
//! ## 代际（generation）状态机
//!
//! 四个入口、一个原子字：
//! - [`CloseGuard::request`]（主线程，`CloseRequested` 时）：登记新一代待确认请求；
//! - [`CloseGuard::confirm`]（壳线程，`window_confirm_close` 命令）：`swap(0)` 原子摘走
//!   当前请求，摘到才有关窗权；重复调用摘到 0 → `None`，壳回 `not-found`；
//! - [`CloseGuard::hold`]（壳线程，`window_close_hold` 命令，2026-11-25 契约扩展）：
//!   CAS 把当前代打上 hold 标志位（`gen → gen | HELD`），**摘除该代的兜底计时但不摘取
//!   请求本身**——之后 confirm 仍能摘到代，兜底对该代恒失败；
//! - [`CloseGuard::fallback_due`]（兜底计时线程）：`compare_exchange(gen, 0)`，只有
//!   「自己这一代仍是当前请求且未被 hold」才赢——confirm 已处理、已被 hold、或用户又按了
//!   一次 X（新一代顶掉旧一代）时，旧计时器自动作废。
//!
//! confirm 与兜底因此**恰好一方赢**，不会双关窗、也不会互相踩掉；hold 与兜底对同一个
//! 原子字做 CAS，同样恰好一方赢；hold 赢后 confirm 必须仍能成功（hold 只取消计时，
//! 关窗权仍留给前端决策）。`destroy` 本身不再触发 `CloseRequested`（tauri 语义），
//! 无防循环标志位的必要。
//!
//! `pending == 0` 表示无待确认请求；最高位保留给 hold 标志（[`HELD`]），代际从 1 起
//! 递增，`fetch_add` 要碰到标志位需要 2^63 次关窗请求，实际不可达，不做额外防护。

use std::sync::atomic::{AtomicU64, Ordering};

/// 兜底放行秒数（契约建议 3s）。理由：
/// - 旧前端没有 `window-close-requested` 监听者，`prevent_close` 之后若无人调
///   `window_confirm_close`，窗口会永远关不掉——不可接受，必须限时放行；
/// - 3s 足够覆盖一次 IPC 往返＋前端 dirty 判定（`history_state` 是快命令），
///   又不至于让无守卫的旧前端用户明显感到「点了 X 没反应」。
///
/// 注意：这是**无人应答**的兜底，不是给前端对话框的决策时限——前端接线后收到
/// `window-close-requested` 须**立即**调 `window_close_hold`（[`CloseGuard::hold`]）
/// 摘除该代计时，dirty 对话框由此获得无限决策时间；无人调 hold（旧前端没有监听者）
/// 时 3s 兜底照旧放行。此语义由共享契约钉死，壳侧不擅改。
pub const CLOSE_FALLBACK_SECS: u64 = 3;

/// hold 标志位：`pending` 原子字的最高位。置位表示「该代请求仍在，但兜底计时已被
/// `window_close_hold` 摘除」。代际从 1 起递增，碰到最高位需要 2^63 次关窗请求，
/// 实际不可达（口径同模块头的绕回说明）。
const HELD: u64 = 1 << 63;

/// 关窗请求的代际状态机。线程安全：`CloseRequested` 在主线程、confirm／hold 在壳线程、
/// 兜底计时在专属线程，四方只经这一个原子字交互。
pub struct CloseGuard {
    /// 代际发号器，从 1 起（0 保留给「无请求」）。
    next: AtomicU64,
    /// 当前待确认请求的代际；0 = 无。
    pending: AtomicU64,
}

impl Default for CloseGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl CloseGuard {
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
            pending: AtomicU64::new(0),
        }
    }

    /// `CloseRequested`：登记新一代请求，返回代际给兜底计时器比对。
    /// 已有待确认请求时直接顶掉（用户又按了一次 X）：旧计时器与旧 hold 标志一并作废，
    /// 重新计 3s。只在主线程调用（窗口事件串行），发号与登记之间不会与另一次 request 交错。
    pub fn request(&self) -> u64 {
        let generation = self.next.fetch_add(1, Ordering::Relaxed);
        self.pending.store(generation, Ordering::SeqCst);
        generation
    }

    /// `window_confirm_close`：原子摘走当前请求。`Some(gen)` = 赢得关窗权；
    /// `None` = 无待确认请求（重复调用／未经原生关闭触发），壳回 `not-found`。
    /// hold 过的请求同样摘得到（hold 只置标志位不摘取），返回的代际掩掉标志位，
    /// 与 [`Self::request`] 发出的代际逐字相同。
    pub fn confirm(&self) -> Option<u64> {
        let generation = self.pending.swap(0, Ordering::SeqCst);
        (generation != 0).then_some(generation & !HELD)
    }

    /// `window_close_hold`（2026-11-25 契约扩展）：摘除当前代的兜底计时——CAS 把
    /// `gen` 置为 `gen | HELD`，**不摘取请求本身**：之后 [`Self::confirm`] 仍能摘到代，
    /// [`Self::fallback_due`] 对该代恒失败（计时器静默作废），对话框获得无限决策时间。
    /// `None` = 无待确认请求或已被 hold（重复 hold），壳回 `not-found`，口径同 confirm。
    ///
    /// 无入参代际（事件载荷为空，前端不知道代际）：CAS 循环瞄准「当前代」，循环中途
    /// 被新一代顶掉就跟随新一代——前端要的始终是「停掉活着的请求的自动放行」。
    /// 与 confirm／兜底的互斥由同一个原子字上的 CAS 保证：谁先完成状态迁移谁赢。
    pub fn hold(&self) -> Option<u64> {
        let mut current = self.pending.load(Ordering::SeqCst);
        loop {
            if current == 0 || current & HELD != 0 {
                return None;
            }
            match self.pending.compare_exchange_weak(
                current,
                current | HELD,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Some(current),
                Err(actual) => current = actual,
            }
        }
    }

    /// 兜底计时到期：仅当自己这一代仍是当前请求**且未被 hold** 时赢得关窗权（并清掉请求）。
    /// `false` = confirm 已处理、已被 hold 摘除计时、或已被新一代顶掉，计时器静默退出。
    pub fn fallback_due(&self, generation: u64) -> bool {
        self.pending
            .compare_exchange(generation, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}
