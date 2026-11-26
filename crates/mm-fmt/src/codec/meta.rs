//! `ModMetaCodec.java` 的等价移植：`mod.json` / `mod.hjson` 的读写，镜像 `Mods.ModMeta`
//! （Mods.java:1392-1416）。

use mm_json::{Json, Object};
use mm_model::ModMeta;

const KNOWN: [&str; 16] = [
    "name",
    "displayName",
    "author",
    "description",
    "subtitle",
    "version",
    "minGameVersion",
    "dependencies",
    "softDependencies",
    "hidden",
    "repo",
    "texturescale",
    "contentOrder",
    "main",
    "java",
    "kotlin",
];

/// 由 `mod.json` 的对象构造元数据。
pub fn from_json(object: &Object) -> ModMeta {
    let mut meta = ModMeta::default();
    meta.name = string(object, "name", "");
    meta.display_name = string(object, "displayName", &meta.name.clone());
    meta.author = string(object, "author", "");
    meta.description = string(object, "description", "");
    meta.subtitle = string(object, "subtitle", "");
    meta.version = string(object, "version", "1.0");
    // 与 Java 的偏离（用户 2026-11-25 授权）：`ModMetaCodec.java:25` 的回退值是 "146"，这里改取
    // "160"，与 160.8 目标对齐——新建项目的默认值本就是 "160"（`ModMeta::default`、
    // `project_factory::create`），导入缺字段的旧包不应再落到过旧版本。仅影响缺字段路径，
    // 显式写出的 minGameVersion 仍原样保留。
    meta.min_game_version = string(object, "minGameVersion", "160");
    meta.dependencies = strings(object, "dependencies");
    meta.soft_dependencies = strings(object, "softDependencies");
    meta.hidden = boolean(object, "hidden", false);
    meta.repo = string(object, "repo", "");
    meta.texturescale = number(object, "texturescale", 1.0);
    meta.content_order = strings(object, "contentOrder");
    meta.main_class = string(object, "main", "");
    meta.java = boolean(object, "java", false);
    meta.kotlin = boolean(object, "kotlin", false);

    for (key, value) in object.iter() {
        if !KNOWN.contains(&key.as_str()) {
            meta.extra.insert(key.clone(), value.clone());
        }
    }
    meta
}

/// 写回元数据。只输出有意义的键，但保留 [`ModMeta::extra`]，使模组声明的未知字段能往返存活。
pub fn to_json(meta: &ModMeta) -> Object {
    let mut object = Object::new();
    object.insert("name", Json::str(meta.name.clone()));
    if not_blank(&meta.display_name) && meta.display_name != meta.name {
        object.insert("displayName", Json::str(meta.display_name.clone()));
    }
    if not_blank(&meta.author) {
        object.insert("author", Json::str(meta.author.clone()));
    }
    if not_blank(&meta.description) {
        object.insert("description", Json::str(meta.description.clone()));
    }
    if not_blank(&meta.subtitle) {
        object.insert("subtitle", Json::str(meta.subtitle.clone()));
    }
    if not_blank(&meta.version) {
        object.insert("version", Json::str(meta.version.clone()));
    }
    if not_blank(&meta.min_game_version) {
        object.insert("minGameVersion", Json::str(meta.min_game_version.clone()));
    }
    if not_blank(&meta.repo) {
        object.insert("repo", Json::str(meta.repo.clone()));
    }
    if meta.hidden {
        object.insert("hidden", Json::Bool(true));
    }
    if meta.texturescale != 1.0 {
        object.insert(
            "texturescale",
            Json::num(java_double_text(meta.texturescale)),
        );
    }
    if !meta.dependencies.is_empty() {
        object.insert("dependencies", string_array(&meta.dependencies));
    }
    if !meta.soft_dependencies.is_empty() {
        object.insert("softDependencies", string_array(&meta.soft_dependencies));
    }
    if !meta.content_order.is_empty() {
        object.insert("contentOrder", string_array(&meta.content_order));
    }
    if not_blank(&meta.main_class) {
        object.insert("main", Json::str(meta.main_class.clone()));
    }
    if meta.java {
        object.insert("java", Json::Bool(true));
    }
    if meta.kotlin {
        object.insert("kotlin", Json::Bool(true));
    }
    for (key, value) in meta.extra.iter() {
        object.insert(key.clone(), value.clone());
    }
    object
}

fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Bool(_) | Json::Num(_) | Json::Str(_))
}

fn string(object: &Object, key: &str, fallback: &str) -> String {
    match object.get(key) {
        Some(v) if is_primitive(v) => v.get_as_string(),
        _ => fallback.to_string(),
    }
}

fn boolean(object: &Object, key: &str, fallback: bool) -> bool {
    match object.get(key) {
        // Gson 的 JsonPrimitive.getAsBoolean() 走 Boolean.parseBoolean(getAsString())，大小写无关。
        Some(v @ (Json::Bool(_) | Json::Num(_) | Json::Str(_))) => {
            v.get_as_string().eq_ignore_ascii_case("true")
        }
        _ => fallback,
    }
}

fn number(object: &Object, key: &str, fallback: f64) -> f64 {
    let Some(value) = object.get(key) else {
        return fallback;
    };
    if !is_primitive(value) {
        return fallback;
    }
    if let Json::Num(_) = value {
        return value.as_f64().unwrap_or(fallback);
    }
    // 带引号的数字同样被接受。但布尔值或非数字字符串不能把整个元数据带下水：`getAsDouble()`
    // 对二者都会抛 NumberFormatException，调用方会丢掉文件里的每一个字段，而不只是写错的那一个。
    value.as_f64().unwrap_or(fallback)
}

fn strings(object: &Object, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(Json::Arr(array)) = object.get(key) {
        for element in array {
            if is_primitive(element) {
                out.push(element.get_as_string());
            }
        }
    }
    out
}

fn string_array(values: &[String]) -> Json {
    Json::Arr(values.iter().map(|v| Json::str(v.clone())).collect())
}

fn not_blank(value: &str) -> bool {
    !value.is_empty() && !value.chars().all(super::properties::is_ws)
}

/// Java `Double.toString` 的等价形式：至少保留一位小数，量级 <1e-3 或 ≥1e7 时改用 `E` 记法。
/// Gson 把 `Number` 原样交给 `JsonWriter`，后者输出的是 `Number.toString()` 的文本，所以这里必须
/// 复刻 Java 的拼写，而不是 Rust 的（Rust 写 `2` 而非 `2.0`，大数用完整十进制）。
/// JDK 19 起 Java 也采用「最短可回读」数字，与 Rust 的 `Display` 一致（本工程用 Java 25）。
pub fn java_double_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let abs = value.abs();
    if abs == 0.0 {
        return format!("{sign}0.0");
    }

    // Rust 最短十进制 → 有效数字串 `lead` 与「小数点前有多少位」`dbp`，即值 = 0.lead × 10^dbp。
    let text = format!("{abs}");
    let (ip, fp) = match text.split_once('.') {
        Some((a, b)) => (a, b),
        None => (text.as_str(), ""),
    };
    let all = format!("{ip}{fp}");
    let zeros_removed = all.chars().count() - all.trim_start_matches('0').chars().count();
    let mut lead: String = all.trim_start_matches('0').to_string();
    while lead.chars().count() > 1 && lead.ends_with('0') {
        lead.pop();
    }
    let dbp = ip.chars().count() as i32 - zeros_removed as i32;

    if dbp >= 8 || dbp <= -3 {
        let chars: Vec<char> = lead.chars().collect();
        let frac = if chars.len() > 1 {
            chars[1..].iter().collect::<String>()
        } else {
            "0".to_string()
        };
        return format!("{sign}{}.{}E{}", chars[0], frac, dbp - 1);
    }
    if dbp <= 0 {
        return format!("{sign}0.{}{}", "0".repeat((-dbp) as usize), lead);
    }
    let chars: Vec<char> = lead.chars().collect();
    if dbp as usize >= chars.len() {
        let pad = "0".repeat(dbp as usize - chars.len());
        return format!("{sign}{}{pad}.0", lead);
    }
    let int_part: String = chars[..dbp as usize].iter().collect();
    let frac_part: String = chars[dbp as usize..].iter().collect();
    format!("{sign}{int_part}.{frac_part}")
}

#[cfg(test)]
mod tests {
    use super::{from_json, java_double_text};

    #[test]
    fn missing_min_game_version_falls_back_to_160() {
        // 2026-11-25 用户授权的偏离：Java 回退 "146"（ModMetaCodec.java:25），这里对齐 160.8 取 "160"。
        let object = mm_json::parse_object(r#"{"name": "demo", "version": "1.0"}"#)
            .expect("解析缺 minGameVersion 的 meta");
        assert_eq!(from_json(&object).min_game_version, "160");

        // 偏离只作用于缺字段路径：显式写出的值（哪怕是旧版本）必须原样存活。
        let explicit = mm_json::parse_object(r#"{"name": "demo", "minGameVersion": "126"}"#)
            .expect("解析显式 minGameVersion 的 meta");
        assert_eq!(from_json(&explicit).min_game_version, "126");
    }

    #[test]
    fn matches_java_double_to_string() {
        // Java: 1.0→"1.0"、2.0→"2.0"、0.5→"0.5"、100.0→"100.0"、1.0E7→"1.0E7"、
        // 12345678.0→"1.2345678E7"、0.001→"0.001"、0.0001→"1.0E-4"、-1.5→"-1.5"。
        assert_eq!(java_double_text(1.0), "1.0");
        assert_eq!(java_double_text(2.0), "2.0");
        assert_eq!(java_double_text(0.5), "0.5");
        assert_eq!(java_double_text(100.0), "100.0");
        assert_eq!(java_double_text(1.0e7), "1.0E7");
        assert_eq!(java_double_text(12345678.0), "1.2345678E7");
        assert_eq!(java_double_text(0.001), "0.001");
        assert_eq!(java_double_text(0.0001), "1.0E-4");
        assert_eq!(java_double_text(-1.5), "-1.5");
        assert_eq!(java_double_text(0.0), "0.0");
    }
}
