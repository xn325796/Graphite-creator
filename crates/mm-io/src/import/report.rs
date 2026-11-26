//! `ImportReport.java` 的等价移植：一次导入/导出干了什么。
//!
//! 对应 Java `dev.modmaker.core.io.ImportReport`。

use mm_schema::KeyedMap;

/// 一次导入或导出干了什么，做成 UI 能展示、测试能断言的形态。
#[derive(Debug, Default)]
pub struct ImportReport {
    pub source: String,
    pub content_count: i32,
    pub inline_count: i32,
    pub sprite_count: i32,
    pub asset_count: i32,
    pub bundle_count: i32,
    pub string_count: i32,
    pub edge_count: i32,
    pub patch_count: i32,
    pub warnings: Vec<String>,
    /// 解析失败的内容文件；按原始文本导入，什么都不丢。
    /// Java `LinkedHashMap` 的插入序（含覆盖保位）由 [`KeyedMap`] 承接。
    pub parse_failures: KeyedMap<String>,
    pub skipped: Vec<String>,
}

impl ImportReport {
    pub fn warn(&mut self, message: &str) {
        self.warnings.push(message.to_string());
    }

    pub fn summary(&self) -> String {
        format!(
            "content={} inline={} patches={} edges={} sprites={} assets={} bundles={} strings={} parseFailures={} warnings={}",
            self.content_count,
            self.inline_count,
            self.patch_count,
            self.edge_count,
            self.sprite_count,
            self.asset_count,
            self.bundle_count,
            self.string_count,
            self.parse_failures.len(),
            self.warnings.len(),
        )
    }
}
