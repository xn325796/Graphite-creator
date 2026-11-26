//! `ClassSchema` 的等价移植：某个内容类已知的字段，加上它在类层次中的位置。

use crate::{
    content_type::ContentType, field::FieldDef, map::KeyedMap,
};

/// 一个内容类的字段与层级信息。
#[derive(Debug, Clone)]
pub struct ClassSchema {
    pub(crate) name: String,
    /// 字段合并用的父类；精选 schema 可覆盖。
    pub(crate) parent: Option<String>,
    /// 来自 Java 源扫描的父类，永不被精选 schema 覆盖。字段合并的捷径（如 Battery 直接继承
    /// Block）不应扭曲层级判断（"哪些类在同一分支"）。
    pub(crate) hierarchy_parent: Option<String>,
    pub(crate) ctype: Option<ContentType>,
    /// 内容类型的根类（Item、Block…）为 true。
    pub(crate) base: bool,
    /// schemas/schema.json 手工描述了这个类时为 true。
    pub(crate) curated: bool,
    pub(crate) fields: KeyedMap<FieldDef>,
}

impl ClassSchema {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parent: None,
            hierarchy_parent: None,
            ctype: None,
            base: false,
            curated: false,
            fields: KeyedMap::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ctype(&self) -> Option<ContentType> {
        self.ctype
    }

    pub fn is_base(&self) -> bool {
        self.base
    }

    pub fn is_curated(&self) -> bool {
        self.curated
    }

    /// 反映真实 Java 类层级的父类：`hierarchyParent ?? parent`。
    pub fn true_parent(&self) -> Option<&str> {
        self.hierarchy_parent.as_deref().or(self.parent.as_deref())
    }
}
