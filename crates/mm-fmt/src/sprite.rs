//! `SpriteRegions.java` 的等价移植：校验器与贴图面板共用的贴图记账——本工程的贴图资源
//! 产出哪些图集（atlas）区域，以及哪些内容记录还缺自己的贴图。
//!
//! Java 侧的 `Set<String>`（`LinkedHashSet`）在这里对应 [`Vec<String>`]：去重且保持首次出现的
//! 顺序，因此区域的顺序同样是可观察的。

use mm_model::{ContentRecord, ModProject};
use mm_schema::name_rules;

use crate::ContentType;

/// 游戏是否为这条记录期待一张贴图：方块、单位、状态需要；补丁编辑的是已有内容
/// （贴图本来就在那里），内联内容不是文件。
pub fn needs_sprite(record: &ContentRecord) -> bool {
    matches!(record.ctype, ContentType::Block | ContentType::Unit | ContentType::Status)
        && !record.patch
        && !record.is_inline()
}

/// 本工程的贴图资源产出的图集区域（`Mods.packSprites` 的命名规则，Mods.java 的打包路径）。
///
/// Java 的 `internalName == null` 分支在 Rust 里由空串承担：`Option<&str>` 会把可空性
/// 传染给整条调用链，而工程内部名总是由 `ModProject::internal_name()` 现算，永不为 null。
pub fn regions_of(project: &ModProject, internal_name: &str) -> Vec<String> {
    let mut regions: Vec<String> = Vec::new();
    if internal_name.is_empty() {
        return regions;
    }
    for asset in &project.assets {
        let override_ = asset.path.starts_with("sprites-override/");
        if !override_ && !asset.path.starts_with("sprites/") {
            continue;
        }
        let file_name = &asset.path[asset.path.rfind('/').map(|i| i + 1).unwrap_or(0)..];
        let dot = match file_name.rfind('.') {
            Some(d) => d,
            None => continue,
        };
        if &file_name[dot + 1..] != "png" {
            continue;
        }
        let region = name_rules::sprite_region(internal_name, &file_name[..dot], !override_);
        if !regions.contains(&region) {
            regions.push(region);
        }
    }
    regions
}

/// 一条记录应当填上的那个图集区域，例如 `my-mod-kiln`。
pub fn expected_region(record: &ContentRecord, internal_name: &str) -> String {
    name_rules::sprite_region(internal_name, &record.name, true)
}

/// 某个区域是否与 `base` 属于同一贴图家族：游戏递归打包 `sprites/` 下的每张 png 并按文件名命名
/// 区域，而内容自己带的是变体与部件、并非一张裸贴图——地板取自 `basalt1.png`..`basalt3.png`
/// （Floor.java：`Core.atlas.find(name + (i + 1))`），导管取自 `<name>-top-#` / `<name>-bottom-#`
/// （Duct.java），炮塔还会加 `-glow` 与 `-heat` 部件。所以一张贴图算数，当区域就是名字本身、
/// 名字加数字、或名字加连字符后缀。
///
/// Java 的 `String.length()` 是 UTF-16 计数，这里按 `chars()` 计数；`Character.isDigit`
/// 认 Unicode Nd 类，这里只认 ASCII 数字，非 ASCII 数字后缀在 Java 里同样落在 `-` 之外的分支上，
/// 只有阿拉伯-印度数字这类才会与 Java 判定不同。
pub fn matches_family(base: &str, region: &str) -> bool {
    if region == base {
        return true;
    }
    if region.chars().count() <= base.chars().count() || !region.starts_with(base) {
        return false;
    }
    let tail = &region[base.len()..];
    let first = tail.chars().next();
    first == Some('-') || tail.chars().all(|c| c.is_ascii_digit())
}

/// 工程的区域是否覆盖了这条记录的贴图，变体与部件也算在内。
pub fn has_sprite(regions: &[String], region: &str) -> bool {
    for candidate in regions {
        if matches_family(region, candidate) {
            return true;
        }
    }
    false
}

/// 需要贴图、但工程资源一张也没提供（变体算在内）的记录。
///
/// Java 返回的是那批活对象本身，Rust 返回借用，避免深拷贝整棵 `fields` 树。
pub fn without_sprite<'a>(project: &'a ModProject, internal_name: &str) -> Vec<&'a ContentRecord> {
    let mut out: Vec<&ContentRecord> = Vec::new();
    let regions = regions_of(project, internal_name);
    if regions.is_empty() {
        return out;
    }
    for record in &project.contents {
        if needs_sprite(record) && !has_sprite(&regions, &expected_region(record, internal_name)) {
            out.push(record);
        }
    }
    out
}
