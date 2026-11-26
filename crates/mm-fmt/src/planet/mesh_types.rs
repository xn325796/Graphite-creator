//! `MeshTypes.java` 的等价移植：游戏 JSON 解析器（`ContentParser.parseMesh`）接受的网格类型，
//! 以及它针对每种类型读取的 JSON 键。星球的 `mesh` 与 `cloudMesh` 就是照这张固定表编辑的。
//!
//! `parseMesh` 有两点容易被朴素的字段列表弄错：它从不读取的键会被直接忽略（每个键都带着自己的
//! 默认值显式读取），而未识别的 *type* 会抛 "Unknown mesh type"——类型名会经过
//! `Strings.capitalize`，所以 `noiseMesh` 被接受而 `noisemesh` 不被接受（只抬高首字母）。
//!
//! 值得注意：serpulo 自己的 HexMesh 无法从 JSON 解析，因此绝不能把它提供给用户。

use mm_schema::FieldType;

/// Java `MeshTypes.Field`：一个可编辑的网格字段——它的 JSON 键与编辑它的控件。
///
/// Java record 的访问器 `type()` 在 Rust 里是字段 `ctype`（`type` 是关键字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub ctype: FieldType,
}

/// Java `MeshTypes.Spec`：一个网格类型与它的固定字段表（MultiMesh/MatMesh 在编辑器里嵌套子网格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub type_: &'static str,
    pub fields: &'static [Field],
}

/// Java `MeshTypes.ALL`：`ContentParser.parseMesh` 的固定类型表，元素顺序照 Java 字面顺序。
pub const ALL: &[Spec] = &[
    Spec {
        type_: "NoiseMesh",
        fields: &[
            Field { name: "seed", ctype: FieldType::Number },
            Field { name: "divisions", ctype: FieldType::Number },
            Field { name: "radius", ctype: FieldType::Number },
            Field { name: "octaves", ctype: FieldType::Number },
            Field { name: "persistence", ctype: FieldType::Number },
            Field { name: "scale", ctype: FieldType::Number },
            Field { name: "mag", ctype: FieldType::Number },
            Field { name: "color1", ctype: FieldType::Color },
            Field { name: "color2", ctype: FieldType::Color },
            // parseMesh 读取 color1 与 color2 时各自回退到 "color"，这正是模组（以及 serpulo 模板）
            // 一次给出两个颜色的方式：color1 -> color -> "ffffff"。
            Field { name: "color", ctype: FieldType::Color },
            Field { name: "colorOct", ctype: FieldType::Number },
            Field { name: "colorPersistence", ctype: FieldType::Number },
            Field { name: "colorScale", ctype: FieldType::Number },
            Field { name: "colorThreshold", ctype: FieldType::Number },
        ],
    },
    Spec {
        type_: "SunMesh",
        fields: &[
            Field { name: "divisions", ctype: FieldType::Number },
            Field { name: "octaves", ctype: FieldType::Number },
            Field { name: "persistence", ctype: FieldType::Number },
            Field { name: "scl", ctype: FieldType::Number },
            Field { name: "pow", ctype: FieldType::Number },
            Field { name: "mag", ctype: FieldType::Number },
            Field { name: "colorScale", ctype: FieldType::Number },
            Field { name: "colors", ctype: FieldType::TextArray },
        ],
    },
    Spec {
        type_: "HexSkyMesh",
        fields: &[
            Field { name: "seed", ctype: FieldType::Number },
            Field { name: "speed", ctype: FieldType::Number },
            Field { name: "radius", ctype: FieldType::Number },
            Field { name: "divisions", ctype: FieldType::Number },
            Field { name: "color", ctype: FieldType::Color },
            Field { name: "octaves", ctype: FieldType::Number },
            Field { name: "persistence", ctype: FieldType::Number },
            Field { name: "scale", ctype: FieldType::Number },
            Field { name: "thresh", ctype: FieldType::Number },
        ],
    },
    // MultiMesh："meshes" 是子网格列表（编辑器递归构建它们）。
    Spec { type_: "MultiMesh", fields: &[] },
    // MatMesh："mesh" 是嵌套网格，"mat" 是原始 Mat3D 矩阵（编辑器对二者都有特殊处理）。
    Spec { type_: "MatMesh", fields: &[] },
];

/// Java `MeshTypes.PARSEABLE`：`parseMesh` 接受的类型名集合，其余一律抛 "Unknown mesh type"。
///
/// Java 用 `Collectors.toUnmodifiableSet` 收集（因此无序）；这里去重保序，等价于集合的成员判定。
pub fn parseable() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for spec in ALL {
        if !out.contains(&spec.type_) {
            out.push(spec.type_);
        }
    }
    out
}

/// Java `MeshTypes.spec(String)`：某个网格类型的 spec；游戏无法解析该类型时返回 `None`。
///
/// Java 的入参可以为 `null`，故这里用 `Option<&str>` 对应。
pub fn spec(type_: Option<&str>) -> Option<&'static Spec> {
    let canonical = canonical(type_);
    ALL.iter().find(|&item| Some(item.type_) == canonical.as_deref())
}

/// Java `MeshTypes.canonical(String)`：`parseMesh` 最终用于 switch 的类型名。
///
/// `Strings.capitalize` 只抬高首字母、其余原样，所以 `noiseMesh` 是 NoiseMesh 类型，而
/// `noisemesh` 仍是被游戏拒绝的未知类型。
///
/// Java 对 `null` 入参返回 `null`；首字母判断走 `charAt(0)`（UTF-16 码元），这里取第一个
/// `char`——对 BMP 内的字符二者一致。
pub fn canonical(type_: Option<&str>) -> Option<String> {
    let value = type_?;
    let first = match value.chars().next() {
        Some(c) => c,
        None => return Some(value.to_string()),
    };
    if !first.is_lowercase() {
        return Some(value.to_string());
    }
    let upper = first.to_uppercase().next().unwrap_or(first);
    let rest = &value[first.len_utf8()..];
    Some(format!("{upper}{rest}"))
}

/// Java `MeshTypes.isParseable(String)`：游戏能否（经过同样的首字母大写后）从 JSON 构建该类型。
pub fn is_parseable(type_: Option<&str>) -> bool {
    spec(type_).is_some()
}
