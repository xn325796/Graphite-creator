//! `ContentRecord.java` 的等价移植：单条内容记录，"双持有"——解析后的 `fields`（有序 Json
//! 树）与原始文本 `raw`（仅当内容来自磁盘文件时）。
//!
//! `fields` 用 [`mm_json::Object`]（保插入序、覆盖保位），忠实对应 Java 的
//! `LinkedHashMap<String, JsonElement>`；`Json` 树是 owned，快照即深拷贝，天然避免 Java 里
//! `snapshot()` 为防撕裂读而做的手工深拷贝。

use std::cmp::Ordering;

use mm_json::{Json, Object};
use mm_schema::{name_rules, ContentType};

/// 一条内容记录。
#[derive(Debug, Clone)]
pub struct ContentRecord {
    pub id: Option<String>,
    /// 内容类型；对应 Java 的 `type`（`type` 是 Rust 关键字，故命名为 `ctype`）。
    pub ctype: ContentType,
    pub name: String,
    /// 文件扩展名，默认 `json`。
    pub ext: String,
    /// 补丁记录保留被补丁内容的字面名。
    pub patch: bool,
    pub parse_failed: bool,
    /// 解析后的字段树。
    pub fields: Object,
    /// 从磁盘读到的原始文件文本；新建的记录为 `None`。
    pub raw: Option<String>,
    /// 原始文件相对模式根的路径。
    pub raw_path: Option<String>,
    /// 宿主记录 id；设置后本对象变成宿主文件里的内联对象。
    pub inline_owner_id: Option<String>,
    /// 相对宿主记录 `fields` 的内联位置路径。
    pub inline_path: Option<String>,
    pub dirty: bool,
}

impl Default for ContentRecord {
    fn default() -> Self {
        Self {
            id: None,
            ctype: ContentType::Item,
            name: String::new(),
            ext: "json".to_string(),
            patch: false,
            parse_failed: false,
            fields: Object::new(),
            raw: None,
            raw_path: None,
            inline_owner_id: None,
            inline_path: None,
            dirty: false,
        }
    }
}

impl ContentRecord {
    /// 内联（嵌套）记录写回宿主文件而非独立内容文件。
    pub fn is_inline(&self) -> bool {
        self.inline_owner_id.is_some()
    }

    /// 新建一条内容记录。
    pub fn create(id: &str, ctype: ContentType, name: &str) -> Self {
        Self {
            id: Some(id.to_string()),
            ctype,
            name: name.to_string(),
            dirty: true,
            ..Default::default()
        }
    }

    /// 内容文件省略 `type` 时游戏使用的类名（如 `Wall`）。
    pub fn class_name(&self) -> Option<String> {
        let value = self.fields.get("type")?;
        if value.is_null() {
            return None;
        }
        // Java: isJsonPrimitive -> getAsString（字符串/数字/布尔）。Arr/Obj -> null。
        if matches!(value, Json::Str(_) | Json::Num(_) | Json::Bool(_)) {
            Some(value.get_as_string())
        } else {
            None
        }
    }

    /// 写回 `type` 字段；`None`/空串移除它（恢复默认）。
    pub fn set_class_name(&mut self, class_name: Option<&str>) {
        match class_name {
            Some(s) if !s.is_empty() => self.fields.insert("type", Json::str(s)),
            _ => {
                self.fields.remove("type");
            }
        }
        self.dirty = true;
    }

    /// 在模式内的名字。
    pub fn full_name(&self, internal_mod_name: &str) -> String {
        name_rules::full_name(internal_mod_name, &self.name, self.patch)
    }

    /// 记录在 `content/<folder>/` 下的内容文件夹。
    pub fn folder(&self) -> String {
        self.ctype.folder_name().to_string()
    }

    /// `fields` 的当前树（快照副本）。
    pub fn tree(&self) -> Object {
        self.fields.clone()
    }

    /// 用（内联对象被就地编辑后的）树替换 `fields`。
    pub fn apply_tree(&mut self, tree: Object) {
        self.fields = tree;
    }

    /// 深拷贝，供后台写盘用，避免与 UI 编辑同时读写同一棵可变树。
    pub fn snapshot(&self) -> ContentRecord {
        self.clone()
    }

    /// `DETERMINISTIC_ORDER`：先文件后内联；文件按 `rawPath`，内联按 `inlinePath`、再按名字。
    pub fn deterministic_cmp(a: &ContentRecord, b: &ContentRecord) -> Ordering {
        a.is_inline()
            .cmp(&b.is_inline())
            .then_with(|| {
                let pa = if a.is_inline() { "" } else { a.raw_path.as_deref().unwrap_or("") };
                let pb = if b.is_inline() { "" } else { b.raw_path.as_deref().unwrap_or("") };
                pa.cmp(pb)
            })
            .then_with(|| {
                let ia =
                    if a.is_inline() { a.inline_path.as_deref().unwrap_or("") } else { "" };
                let ib =
                    if b.is_inline() { b.inline_path.as_deref().unwrap_or("") } else { "" };
                ia.cmp(ib)
            })
            .then_with(|| a.name.cmp(&b.name))
    }
}
