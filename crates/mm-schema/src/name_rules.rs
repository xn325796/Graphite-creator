//! `NameRules.java` 的等价移植：把显示名折成内部名、内容/文件名规则、bundle 键、精灵区域。
//!
//! 迁移方案把 `NameRules` 归到 `mm-fmt::refs`（与 `ContentType` 同组）。但依赖图要求
//! `mm-fmt` 依赖 `mm-model`，而 `mm-model`（`ModMeta.internalName`、`ContentRecord.fullName`）
//! 需要 `NameRules`——若 `NameRules` 留在 `mm-fmt` 就会形成 `model → fmt → model` 的环。
//! 因此与 `ContentType` 一样，把它下沉到最底层的 `mm-schema`；`mm-fmt` 之后原样 re-export。

use crate::content_type::ContentType;

/// 内容名的硬上限，以匹配游戏内部名字的最大合理长度。
pub const MAX_NAME_LENGTH: usize = 100;

/// 编辑器在 `*.bundle` 里发布的内容字符串字段。
pub const BUNDLE_FIELDS: [&str; 4] = ["name", "description", "details", "credit"];

const COLOR_TAGS: [&str; 38] = [
    "ACID",
    "BLACK",
    "BLUE",
    "BRICK",
    "BROWN",
    "CLEAR",
    "CORAL",
    "CRIMSON",
    "CYAN",
    "DARK_GRAY",
    "DARK_GREY",
    "FOREST",
    "GOLD",
    "GOLDENROD",
    "GRAY",
    "GREEN",
    "GREY",
    "LIGHT_GRAY",
    "LIGHT_GREY",
    "LIME",
    "MAGENTA",
    "MAROON",
    "NAVY",
    "OLIVE",
    "ORANGE",
    "PINK",
    "PURPLE",
    "RED",
    "ROYAL",
    "SALMON",
    "SCARLET",
    "SKY",
    "SLATE",
    "TAN",
    "TEAL",
    "VIOLET",
    "WHITE",
    "YELLOW",
];

/// Windows 保留设备名——按这些命名文件会写出但之后无法编辑。
const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// 文件名去掉最后一个扩展名之前的部分，等价 `path.substring(0, path.indexOf('.'))`。
pub fn file_name_base(name: &str) -> &str {
    match name.find('.') {
        Some(dot) => &name[..dot],
        None => name,
    }
}

/// 基础名（大小写无关）是否是 Windows 保留设备名。
pub fn is_windows_reserved_name(name: &str) -> bool {
    let base = file_name_base(name).to_lowercase();
    WINDOWS_RESERVED.iter().any(|t| *t == base)
}

/// 编辑器采用的名字：去掉颜色标签、小写、空格换连字符。
pub fn internal_name(display_name: &str) -> String {
    strip_colors(display_name)
        .to_lowercase()
        .replace(' ', "-")
}

fn is_hex_color(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() < 2 || chars.len() > 9 || chars[0] != '#' {
        return false;
    }
    chars[1..].iter().all(|c| c.is_ascii_hexdigit())
}

fn is_color_tag(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    if is_hex_color(name) {
        return true;
    }
    COLOR_TAGS
        .iter()
        .any(|t| *t == name || t.replace('_', "").to_lowercase() == name)
}

/// 去掉游戏样式颜色标记（`[scarlet]`、`[aabbccdd]`）；其它方括号片段原样保留。
pub fn strip_colors(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut index = 0usize;
    while index < n {
        let c = chars[index];
        if c == '['
            && let Some(end) = (index + 1..n).position(|j| chars[j] == ']').map(|p| index + 1 + p)
        {
            let inner: String = chars[index + 1..end].iter().collect();
            if is_color_tag(&inner) {
                index = end + 1;
                continue;
            }
        }
        out.push(c);
        index += 1;
    }
    out
}

/// 内部名是否可安全用作内容文件名。
pub fn is_folder_safe(internal: &str) -> bool {
    !internal.trim().is_empty()
        && internal.chars().count() <= MAX_NAME_LENGTH
        && !is_windows_reserved_name(internal)
        && !internal.contains('/')
        && !internal.contains('\\')
        && !internal.contains("..")
        && !internal.contains(':')
}

/// 内容在模式内的名字。补丁内容保留字面名，否则加上命名空间。
pub fn full_name(internal_mod_name: &str, content_name: &str, patch: bool) -> String {
    if patch {
        content_name.to_string()
    } else {
        format!("{internal_mod_name}-{content_name}")
    }
}

/// 不带补丁标志的 `full_name`（非补丁内容）。
pub fn full_name_plain(internal_mod_name: &str, content_name: &str) -> String {
    full_name(internal_mod_name, content_name, false)
}

pub fn is_valid_content_name(name: &str) -> bool {
    content_name_problem(name).is_none()
}

/// 为什么这个名字不能作为内容文件的一部分（若可用则返回 `None`）。
pub fn content_name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("name is empty".to_string());
    }
    for c in name.chars() {
        if c.is_whitespace() {
            return Some(format!("name contains '{c}', which breaks content lookup on disk"));
        }
        if (c as u32) < 0x20 {
            return Some(format!("name contains control character 0x{:02x}", c as u32));
        }
        if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            return Some(format!(
                "name contains '{}', which cannot appear in a file name",
                c
            ));
        }
    }
    let starts_dot = name.starts_with('.');
    let ends_dot = name.ends_with('.');
    if starts_dot || ends_dot {
        return Some("name cannot start or end with a dot".to_string());
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Some(format!("name is longer than {MAX_NAME_LENGTH} characters"));
    }
    if is_windows_reserved_name(name) {
        return Some("name collides with a reserved Windows device name (con, nul, com1, ...)"
            .to_string());
    }
    None
}

/// 打包时写入 mod bundle 的键，镜像 Mindustry 的 bundle 命名。
pub fn bundle_key(ctype: ContentType, full_name: &str, field: &str) -> String {
    format!("{}.{}.{}", ctype.name(), full_name, field)
}

/// 精灵区域，即 `Mindustry.imageName()` 的产物。区域目录是扁平的，所以非前缀化目录里的
/// 内容会带上命名空间，其 `TextureRegion` 才能被找到。
pub fn sprite_region(internal: &str, file_name_wo_ext: &str, prefixed_folder: bool) -> String {
    if file_name_wo_ext.is_empty() {
        return String::new();
    }
    let hyphen = file_name_wo_ext.find('-');
    let prefix = format!("{internal}-");
    let already_prefixed =
        hyphen.map(|h| file_name_wo_ext.len() > h + 1 && file_name_wo_ext[h + 1..].starts_with(&prefix)).unwrap_or(false);
    if prefixed_folder && !already_prefixed {
        format!("{internal}-{file_name_wo_ext}")
    } else {
        file_name_wo_ext.to_string()
    }
}

/// 游戏在 `RegionLoader` 里查找一个区域时剥掉的扩展名之后的部分。
pub fn region_lookup_name(name: &str) -> String {
    match name.find('.') {
        Some(dot) => name[..dot].to_string(),
        None => name.to_string(),
    }
}
