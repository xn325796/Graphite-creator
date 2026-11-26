//! `BundleCodec.java` 的等价移植：bundle 文件命名与内容字符串键约定。
//!
//! `Mods.buildFiles`（Mods.java:641-663）接受 `bundles/` 里名字以 `bundle` 开头、以 `.properties`
//! 结尾的文件；基础名选择 locale，所以 `bundle.properties` 是回退，`bundle_zh_CN.properties`
//! 承载 zh_CN。内容键遵循 `<type>.<fullName>.<field>`（UnlockableContent.java:90）。

use mm_schema::{name_rules, ContentType};

/// 解析后的受管内容键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentKey {
    pub ctype: ContentType,
    pub full_name: String,
    pub field: String,
}

/// 文件名是否是 bundle。
pub fn is_bundle_file(file_name: &str) -> bool {
    file_name.starts_with("bundle") && file_name.ends_with(".properties")
}

/// `bundle.properties` → `""`，`bundle_zh_CN.properties` → `"zh_CN"`。
pub fn locale_of(file_name: &str) -> String {
    let base = file_name
        .strip_suffix(".properties")
        .unwrap_or(file_name)
        .to_string();
    match base.find('_') {
        Some(underscore) => base[underscore + 1..].to_string(),
        None => String::new(),
    }
}

/// 英语是游戏的基础 bundle，以 `bundle.properties` 而非 `bundle_en.properties` 发布
/// （vanilla `core/assets/bundles` 亦然）。
pub fn file_name_of(locale: Option<&str>) -> String {
    match locale {
        None | Some("") | Some("en") => "bundle.properties".to_string(),
        Some(l) => format!("bundle_{l}.properties"),
    }
}

/// 解析 `<type>.<fullName>.<field>`。本工具不管理的键（`category.*`、`stat.*`、脚本键，
/// 或形状不同的任何东西）返回 `None`，以便它们被保留。
pub fn parse_key(key: &str) -> Option<ContentKey> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let ctype = ContentType::from_name(parts[0])?;
    if !ctype.file_backed() {
        return None;
    }
    let field = parts[2];
    if !name_rules::BUNDLE_FIELDS.contains(&field) {
        return None;
    }
    Some(ContentKey {
        ctype,
        full_name: parts[1].to_string(),
        field: field.to_string(),
    })
}

/// 剥掉 bundle 键中内容名的模组前缀：`my-mod-turret` → `turret`。
pub fn content_name_of(full_name: &str, internal_mod_name: &str) -> String {
    let prefix = format!("{internal_mod_name}-");
    match full_name.strip_prefix(&prefix) {
        Some(rest) => rest.to_string(),
        None => full_name.to_string(),
    }
}
