//! mm-schema：SchemaRegistry / ClassSchema / FieldDef / FieldType / ContentType。
//!
//! 直接消费 `schemas/fields.json`（生成层）、`schemas/schema.json`（精选层，按字段覆盖）与
//! `schemas/vanilla-content.json`（原版内容名，用于补丁识别与引用校验），零重写成本。
//!
//! 这是 Java `core.schema`（4 类）+ `core.fmt.ContentType` 的等价移植。`ContentType` 原本位于
//! `core.fmt`，迁移方案把它归到 `mm-fmt`；但 `mm-fmt`（P2）依赖 `mm-schema`（P1），而 schema
//! 的类解析（`resolve_class`/`base_class_of`）又离不开 `ContentType`，故此处把 `ContentType`
//! 落在最底层的 `mm-schema`，`mm-fmt` 之后可原样 re-export，保持依赖图单向。

mod class_schema;
mod content_type;
mod field;
mod field_type;
mod map;
pub mod name_rules;
mod registry;

pub use class_schema::ClassSchema;
pub use content_type::ContentType;
pub use field::FieldDef;
pub use field_type::FieldType;
pub use map::KeyedMap;
pub use registry::{SchemaError, SchemaRegistry};
