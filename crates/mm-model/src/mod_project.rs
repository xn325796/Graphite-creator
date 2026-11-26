//! `ModProject.java` 的等价移植：整个可编辑模组——元数据、内容记录、字符串表、画布与资源。
//!
//! 这是导入器与导出器共同工作的唯一真源，画布因此无需了解模组包布局。

use std::collections::BTreeMap;
use std::path::PathBuf;

use mm_schema::ContentType;

use crate::{
    asset_ref::AssetRef, board::Board, content_record::ContentRecord, mod_meta::ModMeta,
    string_table::StringTable,
};

/// 存档格式版本。
pub const FORMAT: i32 = 1;
/// 本工具针对的 Mindustry 构建族；主版本门槛由它推导。
pub const TARGET_MAJOR: i32 = 160;
/// 生成 schemas 所用的 Mindustry 版本（唯一真源）。
pub const SCHEMA_PROFILE: &str = "160.8";

/// 整个模组模型。
#[derive(Debug, Clone)]
pub struct ModProject {
    pub format: i32,
    pub meta: ModMeta,
    /// 磁盘上发现的元数据文件名（mod.json、plugin.hjson……）。
    pub meta_file: String,
    /// 格式转换前的元数据文件名；下次保存时删除旧文件。
    pub previous_meta_file: Option<String>,
    /// 原始元数据文本；元数据未被改动时原样写回。
    pub meta_text: String,
    /// 导入时元数据文件解析失败：`meta_text` 保存原始字节，`meta` 为空。
    pub meta_parse_failed: bool,
    pub meta_dirty: bool,
    /// 画布改动（拖动、删除、整理）时被置位，供保存/退出守卫响应。
    pub boards_dirty: bool,
    /// 本会话已展示"编辑会把文件重写为 JSON"的警告。
    pub reserialize_notice: bool,
    /// 内容语言，"en" 在前。决定写哪些 bundle 文件。
    pub locales: Vec<String>,
    pub strings: StringTable,
    pub contents: Vec<ContentRecord>,
    pub boards: Vec<Board>,
    pub assets: Vec<AssetRef>,
    /// 磁盘上的项目目录；保存前为 `None`。
    pub root: Option<PathBuf>,
    /// 导入来源，仅用于展示。
    pub imported_from: String,
    /// 创作层（modmaker/project.json 或某个画布）读不出来时非空。
    pub authoring_warning: Option<String>,
    /// 生成 schemas 所用的 Mindustry 版本，如 "160.8"。
    pub schema_profile: String,
    /// 脚本推断出的「内容名 → 类名」表（2026-11-26，产品评审 R-02）。
    ///
    /// 真实模组的方块 JSON 常常不写 `type`，类由 `scripts/**.js` 里的
    /// `extend(<Class>, '<name>')` 决定；这张表就是那次扫描的结果，供解析有效类时排在
    /// 「声明类」之后、「类型默认类」之前。
    ///
    /// **永不落盘**：它不是模组文件的真源，只是打开项目时对 `scripts/` 的一次快照
    /// （`ModProject` 没有序列化实现，保存路径只写内容/元数据/画布/作者层），
    /// 每次打开项目都重新扫描，脚本改了不需要迁移。
    pub script_classes: BTreeMap<String, String>,
}

impl Default for ModProject {
    fn default() -> Self {
        Self {
            format: FORMAT,
            meta: ModMeta::default(),
            meta_file: String::new(),
            previous_meta_file: None,
            meta_text: String::new(),
            meta_parse_failed: false,
            meta_dirty: false,
            boards_dirty: false,
            reserialize_notice: false,
            locales: vec!["en".to_string()],
            strings: StringTable::default(),
            contents: Vec::new(),
            boards: Vec::new(),
            assets: Vec::new(),
            root: None,
            imported_from: String::new(),
            authoring_warning: None,
            schema_profile: SCHEMA_PROFILE.to_string(),
            script_classes: BTreeMap::new(),
        }
    }
}

impl ModProject {
    pub fn internal_name(&self) -> String {
        self.meta.internal_name()
    }

    /// 对应 Java `contentById(String)`（`ModProject.java:63-70`）。
    pub fn content_by_id(&self, id: &str) -> Option<&ContentRecord> {
        self.contents.iter().find(|r| r.id.as_deref() == Some(id))
    }

    /// Java 的 `contentById`（`:63-70`）返回的是那同一条记录，调用点可以直接改它
    /// （如 `ProjectController.java:208` `record.dirty = true`）。Rust 的借用规则把这一点
    /// 显式成 `_mut` 兄弟方法，查找条件与 `:65` 的 `record.id != null && record.id.equals(id)`
    /// 逐字同一。
    pub fn content_by_id_mut(&mut self, id: &str) -> Option<&mut ContentRecord> {
        self.contents
            .iter_mut()
            .find(|r| r.id.as_deref() == Some(id))
    }

    /// 内容名在同类型内唯一，因为它们会变成文件名。
    pub fn content_by_name(&self, ctype: ContentType, name: &str) -> Option<&ContentRecord> {
        self.contents.iter().find(|r| r.ctype == ctype && r.name == name)
    }

    /// 资源路径集合，去重保序（镜像 `LinkedHashSet`）。
    pub fn asset_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for asset in &self.assets {
            if !out.contains(&asset.path) {
                out.push(asset.path.clone());
            }
        }
        out
    }

    pub fn display_name(&self) -> String {
        if self.meta.display_name.is_empty() {
            self.meta.name.clone()
        } else {
            self.meta.display_name.clone()
        }
    }

    /// 一次校验会读取内容的深只读副本。owned 模型 `clone` 即真深拷贝，
    /// 因此后台 worker 不会读到 UI 线程正在原地改写的字段树。
    pub fn snapshot(&self) -> ModProject {
        self.clone()
    }
}
