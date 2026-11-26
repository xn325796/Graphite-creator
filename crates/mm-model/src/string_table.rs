//! `StringTable.java` 的等价移植：本地化字符串，分为本工具管理的键与模组 bundle 里的其余键。
//!
//! 有序映射用 [`mm_schema::KeyedMap`]（镜像 `LinkedHashMap`）。管理键遵循游戏约定
//! `<type>.<internalModName>-<contentName>.<name|description|details|credit>`
//! （UnlockableContent.java:90）。额外键（`category.*`、`stat.*`、脚本键）按语言保留，写回时原样不动。

use std::sync::Arc;

use mm_schema::KeyedMap;

/// 本地化字符串表。
#[derive(Debug, Clone, Default)]
pub struct StringTable {
    /// key -> locale -> value。
    pub values: KeyedMap<KeyedMap<String>>,
    /// locale -> 本工具不管理的有序 bundle 条目。
    pub extra: KeyedMap<KeyedMap<String>>,
    /// 每语言见到的 bundle 文件名，如 "zh_CN" -> "bundle_zh_CN.properties"。
    pub bundle_files: KeyedMap<String>,
    /// 每语言的原始 bundle 文本；该 bundle 未被改动时原样写回。
    /// `Arc<str>`：撤销栈的 DirtyFlags 每条命令都要快照全语言原文，共享引用使未改动
    /// 的语言零拷贝（200 条历史 × 全量文本曾是数百 MB 级内存）。
    pub bundle_raw: KeyedMap<Arc<str>>,
    /// 磁盘内容与 bundle 文本不再一致的语言。
    pub bundle_dirty: KeyedMap<bool>,
}

impl StringTable {
    pub fn is_bundle_dirty(&self, locale: &str) -> bool {
        matches!(self.bundle_dirty.get(locale), Some(true))
    }

    pub fn mark_bundle_dirty(&mut self, locale: &str) {
        self.bundle_dirty.put(locale.to_string(), true);
    }

    /// `values.computeIfAbsent(key).put(locale, value)`。
    pub fn put(&mut self, key: &str, locale: &str, value: &str) {
        self.values
            .entry_or_insert_with(key.to_string(), KeyedMap::new)
            .put(locale.to_string(), value.to_string());
    }

    pub fn get(&self, key: &str, locale: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|by_locale| by_locale.get(locale))
            .map(|s| s.as_str())
    }

    pub fn remove_key(&mut self, key: &str) {
        self.values.remove(key);
    }

    /// 供脱离模型线程做只读工作的分离副本。`KeyedMap` 为 owned，`clone` 即深拷贝。
    pub fn snapshot(&self) -> StringTable {
        self.clone()
    }
}
