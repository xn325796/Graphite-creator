//! `FieldType` 的等价移植：字段控件种类，匹配 schema.json/fields.json 的 "type" 字符串。

/// 字段控件种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Number,
    Text,
    TextArray,
    Bool,
    Color,
    /// 固定列表中取一值（schema 拼写为 "enum"）。
    Enumeration,
    /// 按名字引用另一个内容对象。
    Content,
    /// 精灵/区域名。
    Sprite,
    /// 单个 item/liquid/payload 栈，写作 "name/amount"。
    Stack,
    /// 栈列表（schema 拼写 "stack[]"）。
    StackArray,
    /// 引用成员构成的结构化组（consumes、UnitReq requirements、ammoTypes、plans）。
    /// schema 拼写 "struct"，形状写在 stack kind 里（"struct:unitReq"）。
    Struct,
    /// 其它：以原始 JSON 编辑。
    Raw,
}

impl FieldType {
    /// `FieldType.parse(rawType)`：null → Raw；按前缀与关键字匹配，未知 → Raw。
    pub fn parse(raw_type: Option<&str>) -> FieldType {
        let value = match raw_type {
            None => return FieldType::Raw,
            Some(v) => v.trim(),
        };
        if value.starts_with("stack[]") {
            return FieldType::StackArray;
        }
        // "struct:unitReq" 等——形状随 stack kind 走（SchemaRegistry.stack_kind_of）。
        if value.starts_with("struct") {
            return FieldType::Struct;
        }
        match value {
            "stack" => FieldType::Stack,
            "text[]" => FieldType::TextArray,
            "enum" => FieldType::Enumeration,
            "number" => FieldType::Number,
            "text" => FieldType::Text,
            "bool" => FieldType::Bool,
            "color" => FieldType::Color,
            "content" => FieldType::Content,
            "sprite" => FieldType::Sprite,
            "struct" => FieldType::Struct,
            _ => FieldType::Raw,
        }
    }

    /// 值引用其它内容的字段：这些在画布上成为边。
    pub fn is_reference(&self) -> bool {
        matches!(
            self,
            FieldType::Content | FieldType::Stack | FieldType::StackArray | FieldType::Struct
        )
    }

    pub fn is_stack(&self) -> bool {
        matches!(self, FieldType::Stack | FieldType::StackArray)
    }
}
