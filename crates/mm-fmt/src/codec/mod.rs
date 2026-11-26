//! 格式契约编解码器：`.properties` bundle、bundle 键约定、`mod.json` 元数据。
//!
//! 对应 Java `core.fmt.PropertiesFile`、`core.fmt.BundleCodec` 与 `core.io.ModMetaCodec`。

pub mod bundle;
pub mod meta;
pub mod properties;

pub use bundle::{
    content_name_of, file_name_of, is_bundle_file, locale_of, parse_key, ContentKey,
};
pub use meta::{from_json as mod_meta_from_json, java_double_text, to_json as mod_meta_to_json};
pub use properties::{escape_key, escape_value, unescape, PropertiesFile};
