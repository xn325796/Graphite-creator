//! mm-fmt：`core.fmt`（16 类）的等价移植——bundle/properties 编解码、引用重写、精灵区域、
//! 科技树与星球规则。迁移方案 §二 P2 的「格式契约」全部落在这里。
//!
//! `ContentType` 与 `NameRules` 在 Java 里位于 `core.fmt`，但因 `mm-schema` 的类解析离不开它们、
//! 而依赖图禁止 `mm-model → mm-fmt`（会成环），故下沉在 `mm-schema`；此处原样 re-export，
//! 让调用方仍可按 Java 的包结构取用。

pub mod codec;
pub mod ids;
pub mod planet;
pub mod refs;
pub mod sprite;
pub mod tech;
pub mod text;
pub mod workspace;

pub use mm_schema::ContentType;
pub use mm_schema::name_rules;
