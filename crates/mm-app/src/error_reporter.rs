//! Java `dev.modmaker.ui.ErrorReporter`（71 行）里**与对话框无关的那一半**的等价移植：
//! 每一次失败都要经过的那一行渲染，加上「报告失败这件事本身绝不再抛」的契约。
//!
//! 类注释（`ErrorReporter.java:8-16`）直译：错误处理从前是散着的——io worker 自己 `new Alert`、
//! 菜单与工具栏处理器只记一条裸异常、退出保存又是第三种写法，于是同一个失败可能看不见、
//! 可能被记进日志、也可能弹成对话框，全看代码走的是哪条路，而无头测试一个都观察不到。
//! 现在所有路径都过同一个对象：消息**总是**进日志，交互路径另外再弹一个对话框。
//!
//! ## 本模块只覆盖 `describe` 与 `reportQuietly`（Java 侧的对话框那一半不移植）
//!
//! Java 的 `report(context, failure)`（`:31-39`）＝ 组一行消息 → `log.accept(message)` →
//! `onFxThread(...)` 里 `new Alert(AlertType.ERROR, message)` + `showAndWait()`；
//! `onFxThread`（`:60-70`）在工具包没起来时把 `Platform.runLater` 抛的 `IllegalStateException`
//! 吞掉，注释原文是「Headless: the log line is all there is.」（`:68`）。
//! Rust 侧没有 JavaFX，弹窗在 Tauri 架构里属于前端壳（`src-vue/` 只画不算、由 `src-tauri` 广播事件），
//! 所以这里**不**移植 `report`／`onFxThread`：那正是 Java 自己在无头环境下的行为——只剩一行日志。
//! 这笔账登记在 P6 文档的 M2 签字条目里（对话框那一半随科技树画板/P7 的壳层一起处置）。
//!
//! ## 与 Java 的两处结构性偏离（逐字登记，不记暗账）
//!
//! 1. **`Throwable` → [`Failure`]**。Java 的 `describe(Throwable)`（`:50-58`）读两样东西：
//!   `failure.getClass().getSimpleName()` 与 `failure.getMessage()`。Rust 的错误是 `Box<dyn Error>`
//!   （`crates/mm-app/src/project_controller.rs:44` 的 `BoxError`），trait 对象拿不到类型名，
//!   所以把「异常类名 + 可空消息」显式建模成 [`Failure`]——调用方本来就知道自己面对的是哪个错误类型
//!   （`ImportError`、`HjsonError`……），类名由它给，这与 Java 靠运行时反射拿是同一份信息。
//! 2. **`describe(null)` → `describe(None)`**（`:51-53` 那条 null 分支返回 `"unknown error"`）。`Consumer<String> log`
//!   （`:19`）承接为 `Box<dyn Fn(&str)>`，与 `project_controller.rs:1642` 的 `log_sink` 同一口径。

/// Java `describe()` 所要的那个 `Throwable` 的最小对应物：类名（`getSimpleName()`）+ 消息
/// （`getMessage()`，可空）。见模块头差异第 1 条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    class_name: String,
    message: Option<String>,
}

impl Failure {
    /// `class_name` 对应 Java 的 `failure.getClass().getSimpleName()`；
    /// `message` 对应 `failure.getMessage()`，没有消息时传 `None`。
    pub fn new(class_name: &str, message: Option<&str>) -> Self {
        Self {
            class_name: class_name.to_string(),
            message: message.map(|value| value.to_string()),
        }
    }

    /// Java `failure.getClass().getSimpleName()`。
    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    /// Java `failure.getMessage()`。
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

/// Java `ErrorReporter.describe(Throwable)`（`:50-58`）：一次失败的一行渲染。
/// `getMessage()` 经常是 null（比如被锁住的文件抛出的 `AccessDeniedException`），所以类名总会出现
/// （Java 方法注释 `:46-49` 的原话）。
pub fn describe(failure: Option<&Failure>) -> String {
    let Some(failure) = failure else {
        // Java `:51-53`。
        return "unknown error".to_string();
    };
    // Java `:55-57`：`message == null || message.isBlank()` 时只给类名，否则「类名: 消息」。
    // `isBlank()` 判的是「全为空白字符」，故 `"  "` 这类消息同样回落到类名。
    match &failure.message {
        Some(message) if !message.trim().is_empty() => {
            format!("{}: {}", failure.class_name, message)
        }
        _ => failure.class_name.clone(),
    }
}

/// Java `ErrorReporter`（`:17-44`）持有 `Consumer<String> log` 的那个对象，这里只保留记账的那一半。
pub struct ErrorReporter {
    log: Box<dyn Fn(&str)>,
}

impl ErrorReporter {
    /// 对应 Java 构造函数 `ErrorReporter(Consumer<String> log)`（`:21-23`）。
    pub fn new(log: Box<dyn Fn(&str)>) -> Self {
        Self { log }
    }

    /// Java `reportQuietly(String context, Throwable failure)`（`:42-44`）：把失败记进日志而不打断
    /// 用户（后台清理、可选动作）。
    ///
    /// 「报告失败本身不是错误」这条契约（Java 用例 `reportingANullFailureIsNotAnErrorItself`）在
    /// Rust 侧由类型与实现共同保证：`failure` 可为 `None`，`describe(None)` 给出 `"unknown error"`，
    /// 全程没有可失败的分支。
    pub fn report_quietly(&self, context: &str, failure: Option<&Failure>) {
        (self.log)(&format!("{context}: {}", describe(failure)));
    }
}
