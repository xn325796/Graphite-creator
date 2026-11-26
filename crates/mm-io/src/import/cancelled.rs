//! `ImportCancelledException.java` 的等价移植：用户取消导入时从流水线里抛出的信号。
//!
//! 对应 Java `dev.modmaker.core.io.ImportCancelledException`。

use std::fmt;
use std::io;

/// 当 `ImportProgress::is_cancelled()` 变 true 时从导入流水线里抛出。
///
/// **传播方式的对应**：Java 原文特意把它做成**不受检**异常（`extends RuntimeException`），
/// 理由原文：「故意不受检：每个 reader、importer、writer 的签名都保持原样，只有 UI 入口
/// 需要 catch 它」。全仓库的 catch 点只有 `ui/AppShell.java:1392` 与 `ui/ProjectActions.java:98`
/// 两处。Rust 没有「绕过签名的异常」可抄（panic 不作业务控制流），因此取消改由 `Result` 通道
/// 显式传播：
/// - `ImportProgress::check_cancelled` 返回 `Result<(), ImportCancelled>`；
/// - 带进度参数的公共入口（如 `crate::package::read_with`）把它经 [`ImportError::Cancelled`]
///   浮给调用方，由调用方处理——正对应 Java 里「只有 UI 入口 catch」的那两处。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCancelled;

impl fmt::Display for ImportCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Java: super("import cancelled")
        f.write_str("import cancelled")
    }
}

impl std::error::Error for ImportCancelled {}

/// 读包入口实际可能失败的两条通道，合成一个 Result：Java 侧是「声明的 `IOException` 加上
/// 借不受检形式穿过签名的 `ImportCancelledException`」。Rust 既不能把「用户取消」折进
/// `io::Error`（Java 没有这么混，折进去就丢了两类失败在 UI 里的不同处理），也不能两个错误
/// 类型共用一个 `Result`，于是显式定义双变体枚举，让调用方分派：`Cancelled` 对应 Java 的
/// catch 点，`Io` 对应 Java 的 `IOException`。
#[derive(Debug)]
pub enum ImportError {
    /// Java 声明的 `IOException`。
    Io(io::Error),
    /// Java 不受检抛出、UI 入口 catch 的 `ImportCancelledException`。
    Cancelled(ImportCancelled),
    /// 文本通道里的字节不是合法 UTF-8（2026-11-20 v1 口径）。
    ///
    /// Java 的 `new String(bytes, UTF_8)` 走 REPLACE 策略，把坏字节静默换成 U+FFFD；那张字符串
    /// 是要**写回磁盘**的，于是「没改过的文件」在保存后就与原文不同了。v1 宁可大声拒绝，也不
    /// 悄悄改用户的文件：内容/meta/bundle 的文本通道一律严格解码，坏字节在这里显式报错，
    /// 二进制资产不走这条通道（按字节原样保存）。
    Encoding(String),
}

impl From<io::Error> for ImportError {
    fn from(error: io::Error) -> Self {
        ImportError::Io(error)
    }
}

impl From<ImportCancelled> for ImportError {
    fn from(cancelled: ImportCancelled) -> Self {
        ImportError::Cancelled(cancelled)
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(error) => error.fmt(f),
            ImportError::Cancelled(cancelled) => cancelled.fmt(f),
            ImportError::Encoding(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ImportError {}
