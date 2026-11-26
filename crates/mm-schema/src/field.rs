//! `FieldDef` 的等价移植：内容类的一个可编辑字段。

use mm_json::Json;

use crate::field_type::FieldType;

/// 内容类的一个可编辑字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    /// 字段名，与内容 JSON 中出现的一致。
    pub name: String,
    /// 控件种类。
    pub field_type: FieldType,
    /// 精选 schema 的 tooltip 文本，或 None。
    pub description: Option<String>,
    /// 当 `field_type` 为枚举时允许的值。
    pub enum_values: Vec<String>,
    /// 值可引用的内容类型（无约束时为 "any"）。
    pub content_ctype: Option<String>,
    /// 栈字段的 kind：item | liquid | payload。
    pub stack_kind: Option<String>,
    /// 精选 schema 的默认值，或 None。
    pub default_value: Option<Json>,
    /// true 表示手写的精选字段，而非源扫描草稿。
    pub curated: bool,
}

impl FieldDef {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        name: String,
        field_type: FieldType,
        description: Option<String>,
        enum_values: Vec<String>,
        content_ctype: Option<String>,
        stack_kind: Option<String>,
        default_value: Option<Json>,
        curated: bool,
    ) -> Self {
        Self {
            name,
            field_type,
            description,
            enum_values,
            content_ctype,
            stack_kind,
            default_value,
            curated,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn field_type(&self) -> FieldType {
        self.field_type
    }

    pub fn enum_values(&self) -> &[String] {
        &self.enum_values
    }

    pub fn stack_kind(&self) -> Option<&str> {
        self.stack_kind.as_deref()
    }

    pub fn content_ctype(&self) -> Option<&str> {
        self.content_ctype.as_deref()
    }

    pub fn is_curated(&self) -> bool {
        self.curated
    }

    /// 字段值是否引用其它内容（等价于 `type.isReference()`）。
    pub fn is_reference(&self) -> bool {
        self.field_type.is_reference()
    }
}
