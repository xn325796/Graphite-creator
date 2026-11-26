//! `ModPackage.java` 的等价移植：磁盘上模组模样的**原始**持有者。
//!
//! 对应 Java `dev.modmaker.core.io.ModPackage`（含其静态内部类 `ModPackage.SourceContent`）。
//! Java 侧它是一个纯数据袋子（public 字段被 `ModPackageReader`/`ModImporter` 直接读写），
//! 所以这里同样用 `pub` 字段一一对应，不加访问器、不做归一化。
//!
//! Java 的两个 `LinkedHashMap` 都靠插入序保真，这里用 `mm_schema::KeyedMap`
//! （它就是为替代 indexmap 而写的「保插入序映射」，`put` 与 `LinkedHashMap.put` 语义一致：
//! 同名键覆盖值但保留原有位置）。`SourceContent.fields` 对应 Java 的
//! `LinkedHashMap<String, JsonElement>`，直接用 mm-json 的保序对象别名 [`Object`]。

use mm_fmt::ContentType;
use mm_json::Object;
use mm_model::ModMeta;
use mm_schema::KeyedMap;

/// 在磁盘上找到的模组包：元数据、内容文件、bundle 以及其余每一个文件，都还是原样。
///
/// 这里什么都不做归一化——它是 [`crate::package::reader`] 的输出、也是后续 `ModImporter`
/// 的输入，正因为保留了原样，一个没被改过的模组才能逐字节地写回去。
#[derive(Debug, Default)]
pub struct ModPackage {
    pub meta: ModMeta,
    pub meta_file: String,
    pub meta_text: String,
    /// 元数据文件存在却解析不出来时的原因；非空表示解析失败，模组其余部分照常载入。
    pub meta_error: String,

    pub contents: Vec<SourceContent>,

    /// bundle 文件路径 -> 原文文本，完全按打包时的样子保留。
    pub bundle_files: KeyedMap<String>,

    /// 全部资产文件：模组相对路径 -> 原始字节（精灵图、脚本、音效、地图、散文件）。
    pub asset_files: KeyedMap<Vec<u8>>,

    /// 刻意没有带过来的路径（操作系统垃圾、版本控制元数据）。
    pub skipped: Vec<String>,

    /// 底下没有任何文件的目录（比如一个空的 `sprites-override/`）。
    /// 它们也是模组打包内容的一部分，所以解包和构建时都要重新创建出来。
    pub empty_dirs: Vec<String>,

    /// 这个包来自 zip/jar 而不是目录时为真。
    pub from_zip: bool,
    /// zip 里那层唯一的包装目录，按游戏的 `Mods.resolveRoot` 那样剥掉。
    pub root_prefix: String,
    /// 人类可读的来源描述，用于导入报告。
    pub source_label: String,
}

/// Java `ModPackage.SourceContent`：`content/<文件夹>/` 下一个 JSON/HJSON 文件的原始记录。
#[derive(Debug, Clone)]
pub struct SourceContent {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub text: String,
    /// Java 的字段名是 `type`，`type` 在 Rust 是关键字，按 `mm_fmt::ContentKey` 的同款写法叫 `ctype`。
    pub ctype: ContentType,
    /// 顶层字段，按文件里的书写顺序（HJSON 键序必须保住，逐字节重建时要用）。
    pub fields: Object,
    /// 解析不出来时置上（Java 默认 null）；该记录仍会被导入。
    pub parse_error: Option<String>,
}
