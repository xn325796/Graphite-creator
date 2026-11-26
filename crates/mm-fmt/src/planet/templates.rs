//! `PlanetTemplates.java` 的等价移植：星球的预置模板。
//!
//! Java 从 classpath 读取 `/templates/planet-serpulo.json`（源文件在
//! `Mindustry-modmaker\src\main\resources\templates\planet-serpulo.json`）。Rust 侧不依赖仓库外的
//! 路径：该 JSON 已**逐字节原样副本**放在本 crate 的
//! `crates\mm-fmt\assets\templates\planet-serpulo.json`，由 `include_str!` 在编译期嵌入，内容与
//! Java 资源文件完全一致（SHA-256 相同），未做任何改写。
//!
//! Java 的 `ConcurrentHashMap<String> TEXT_CACHE`（文本只装载一次）在这里天然成立：嵌入的文本是
//! 编译期静态常量。`templateFor` 每次调用都重新解析，所以各次调用之间互相独立（Java 靠每次重新
//! parse 达到同一效果）。

use mm_json::{read_object, Object};
use mm_schema::ContentType;

/// Java `TEXT_CACHE` 里那条资源的文本：`/templates/planet-serpulo.json` 的原样副本。
const PLANET_SERPULO_JSON: &str = include_str!("../../assets/templates/planet-serpulo.json");

/// Java `NAMES`：类型 → 显示名，元素顺序即 Java 的 `put` 顺序。
const NAMES: &[(ContentType, &str)] = &[(ContentType::Planet, "serpulo")];

/// Java `SOURCES`：类型 → 模板资源路径，元素顺序即 Java 的 `put` 顺序。
const SOURCES: &[(ContentType, &str)] = &[(ContentType::Planet, "/templates/planet-serpulo.json")];

/// Java `name(ContentType)`：该类型预置模板的名字，没有模板时返回 `None`。
pub fn name(ctype: ContentType) -> Option<&'static str> {
    NAMES
        .iter()
        .find(|(item, _)| *item == ctype)
        .map(|(_, display_name)| *display_name)
}

/// Java `templateFor(ContentType)`：该类型默认模板的可编辑副本，没有模板时返回 `None`。
pub fn template_for(ctype: ContentType) -> Option<Object> {
    // Java: String source = SOURCES.get(type); if (source == null) return null;
    let source = SOURCES
        .iter()
        .find(|(item, _)| *item == ctype)
        .map(|(_, path)| *path)?;
    let text = load(source)?;
    // Java 的 Json.parseObject 在 templateFor 里不被 try 包住，解析失败会照常抛出；
    // 这里同样让它显式失败。
    Some(read_object(text).expect("the embedded planet template must parse"))
}

/// Java `load(String)` + `TEXT_CACHE.computeIfAbsent(...)`：装载资源文本。
///
/// Java 读不到资源或读取抛异常时返回 `null`；资源文本只装载一次。
fn load(source: &str) -> Option<&'static str> {
    match source {
        "/templates/planet-serpulo.json" => Some(PLANET_SERPULO_JSON),
        _ => None,
    }
}
