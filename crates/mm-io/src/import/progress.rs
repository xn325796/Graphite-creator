//! `ImportProgress.java` 的等价移植：导入流水线边干活边上报的回调，也是「取消」点击往回传的通道。
//!
//! 对应 Java `dev.modmaker.core.io.ImportProgress`（接口 + `NONE` 常量 + 静态 `worthReporting`）。

use super::cancelled::ImportCancelled;

/// 导入依次经历的三个阶段，按顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Reading,
    Building,
    Writing,
}

/// 每处理这么多单元发一次 tick，最后一个单元再额外补一次。
pub const REPORT_EVERY: i32 = 64;

/// 导入流水线边干活边上报的回调，也是「取消」点击往回传的通道。每个 importer 入口都有一个
/// 不带它（或带 [`NoProgress`]，即 Java 的 `NONE`）的重载，所以命令行运行与测试都不受影响。
///
/// 上报是 io 线程的事；监听器自己决定怎么把这些数字送上 FX 线程。节流同样是调用方的事：
/// 发 tick 之前先调 [`worth_reporting`]，免得一个 1500 文件的模组往队列里灌 1500 条状态更新。
///
/// Java 的接口方法会改动实现者自己的状态（例如 UI 适配器往 FX 队列里塞数字、测试 Recorder
/// 往列表里 push），因此 Rust 侧回调方法取 `&mut self`；`is_cancelled` 只读，取 `&self`。
pub trait ImportProgress {
    /// 每个阶段开始前 announce 一次；`total` 未知时可能是 0 或 -1。
    fn stage_started(&mut self, stage: Stage, total: i32);

    /// 一个被节流的 tick：本阶段 `total` 个单元已处理 `done` 个。
    fn progressed(&mut self, stage: Stage, done: i32, total: i32);

    /// 在文件之间检查；用户要求停止导入后为 true。
    fn is_cancelled(&self) -> bool;

    /// 用户已取消时抛 `ImportCancelledException`——Rust 侧返回 `Err(ImportCancelled)`，
    /// 由带进度参数的入口把它浮给调用方（传播方式见 [`ImportCancelled`] 的文档）。
    fn check_cancelled(&self) -> Result<(), ImportCancelled> {
        if self.is_cancelled() {
            Err(ImportCancelled)
        } else {
            Ok(())
        }
    }
}

/// 这个计数器的 tick 值不值得发（每 [`REPORT_EVERY`] 个一次，外加最后一个）。
/// 对应 Java 接口静态方法 `ImportProgress.worthReporting(int, int)`。
pub fn worth_reporting(done: i32, total: i32) -> bool {
    done == total || done % REPORT_EVERY == 0
}

/// Java `ImportProgress.NONE`：给不展示进度的调用方用的静默、永不取消的监听器。
/// Java 是一个共享单例；Rust 侧它无状态，每次按值新建即可，语义完全一致。
pub struct NoProgress;

impl ImportProgress for NoProgress {
    fn stage_started(&mut self, _stage: Stage, _total: i32) {}

    fn progressed(&mut self, _stage: Stage, _done: i32, _total: i32) {}

    fn is_cancelled(&self) -> bool {
        false
    }
}
