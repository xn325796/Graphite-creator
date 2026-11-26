//! `ExportReport.java` 的等价移植（`dev.modmaker.core.io.ExportReport`，29 行）：一次导出的结果。
//!
//! Java 侧是一个 `record`：六个规范化构造参数 + 三个派生查询 + 一个 `summary()`。record 生成的
//! 访问器（`target()`、`zip()`、`files()`、`bytes()`、`issues()`、`pruned()`）在 Rust 里就是
//! 直接读同名的 `pub` 字段，不再另写一遍方法——写出来只会和字段撞名。
//!
//! 与 Java 的两处表达差异，都不改变可观察行为：
//! - record 的规范化构造器**存的是调用方 List 的引用**（`List.copyOf` 都没做），所以 Java 那边
//!   报告与实际列表是同一份可变数据；Rust 侧字段是自有所有权的 `Vec`，由调用方把同一份数据
//!   `clone` 进来。目前唯一的调用方 `ModExporter` 在构造报告后就不再动那些列表，行为一致。
//! - `errorCount()` / `warningCount()` 在 Java 里返回 `long`（`Stream.count()`），这里照原样
//!   用 `i64`，不改成 `usize`，好让 `summary()` 里字符串拼接出来的十进制数一模一样。

use mm_validate::Issue;

/// Java `record ExportReport(String target, boolean zip, List<String> files, long bytes,
/// List<Issue> issues, List<String> pruned)`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// 产物路径，Java 那边是 `Path.toAbsolutePath().toString()`（**不** normalize）。
    pub target: String,
    /// true = `.zip`，false = 解开的目录树。
    pub zip: bool,
    /// 写出的文件，全部是相对 mod 根的路径。
    pub files: Vec<String>,
    /// 写出的总字节数。
    pub bytes: i64,
    /// 导出时顺带报出的校验问题。
    pub issues: Vec<Issue>,
    /// 被清掉的过期文件，相对产物目录。
    pub pruned: Vec<String>,
}

impl ExportReport {
    /// record 的规范化构造器。
    pub fn new(
        target: String,
        zip: bool,
        files: Vec<String>,
        bytes: i64,
        issues: Vec<Issue>,
        pruned: Vec<String>,
    ) -> ExportReport {
        ExportReport { target, zip, files, bytes, issues, pruned }
    }

    /// Java `hasErrors()`：`issues.stream().anyMatch(Issue::isError)`。
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(Issue::is_error)
    }

    /// Java `errorCount()`。
    pub fn error_count(&self) -> i64 {
        self.issues.iter().filter(|issue| issue.is_error()).count() as i64
    }

    /// Java `warningCount()`。
    pub fn warning_count(&self) -> i64 {
        self.issues.iter().filter(|issue| !issue.is_error()).count() as i64
    }

    /// Java `summary()`：一行人类可读的文本，可直接进日志。字符串逐字符照抄 Java 的拼接
    /// （第 26-28 行），包括 `pruned=` 只在非空时才追加这一段。
    pub fn summary(&self) -> String {
        let kind = if self.zip { "zip" } else { "folder" };
        let mut text = format!(
            "{kind} {} files={} bytes={} errors={} warnings={}",
            self.target,
            self.files.len(),
            self.bytes,
            self.error_count(),
            self.warning_count()
        );
        if !self.pruned.is_empty() {
            text.push_str(" pruned=");
            text.push_str(&self.pruned.len().to_string());
        }
        text
    }
}
