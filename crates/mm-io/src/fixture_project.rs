//! `FixtureProject.java` 的等价移植（`dev.modmaker.core.io.FixtureProject`，**main 源码**）。
//!
//! Java 是一个 `final` 类加一个私有构造器（`FixtureProject.java:22`、`:34-35`），成员全是
//! `static`；Rust 里对应成模块：公开常量 + `pub fn`，Java 的 private 常量对应私有 `const`，
//! Java 的 private 静态方法 `asModRoot` 对应本文件的私有 `fn`。
//!
//! Java 的类注释（`FixtureProject.java:13-21`）直译：
//! 找出「命令行没指定任何项目时应用启动要打开、金标用例要跑」的那个真实模组目录，无论它在这台
//! 机器上的哪一处。这里不点名任何一个具体模组：显式设置优先，其次是常规工作区布局提供的那个
//! 相对位置（`../mod`），再往后是对工作目录旁边那些文件夹的一次浅扫，找任意一个游戏本身愿意
//! 当成模组的目录——认的是游戏自己探测的那些元数据文件名，所以 `mod.hjson` 与 `mod.json` 同样
//! 有效。什么都没命中时由调用方干净跳过而不是失败：应用就空着启动，测试就跳过。
//!
//! **与 Java 的事实差异（都不改变语义）**：
//! 1. Java 的 `hasMetaFile`（`FixtureProject.java:60`）与 `probe`（`:38`）是包内可见/public 静态
//!    方法；`tests/fixture_project.rs` 是独立的集成测试二进制，看不见 `pub(crate)`，故
//!    [`has_meta_file`] 放宽为 `pub`，与 Java 里 `FixtureProjectTest` 同包直接调用它一致。
//! 2. Java `catch (IOException | RuntimeException)`（`:72`）那两类失败在 Rust 里都以 `Result`
//!    的 `Err` 返回（读文件、解析 HJSON 都不抛未受检异常），所以两处 `Err` 都对应那个 catch：
//!    吞掉、换下一个候选文件名，绝不 panic。
//!
//! **Java `TestFixtures.realModRoot()`（`TestFixtures.java:16-18`）= [`probe`]**：那个 test
//! 侧的门面类只有一个私有构造器和一个转调 `FixtureProject.probe()` 的方法，没有任何自己的逻辑；
//! Rust 里给它单立一个空壳模块只会多出第二个「真相来源」，违背它类注释里「一个实现、两个调用方」
//! 的意图（`TestFixtures.java:5-9`）。后续 Rust 集成测试请直接
//! `use mm_io::fixture_project::probe;`，等价于 Java 里 `TestFixtures.realModRoot()`。

use std::fs;
use std::path::{Path, PathBuf};

use mm_fmt::codec::mod_meta_from_json;
use mm_model::ModMeta;

use crate::files::search_paths;

/// Java `FixtureProject.PROPERTY`（`FixtureProject.java:24-25`）：用 `-Dmodmaker.fixture=<dir>`
/// 把探测指向某个模组。Rust 侧没有 JVM 系统属性这条通道，见 `search_paths::override_path` 的说明。
pub const PROPERTY: &str = "modmaker.fixture";
/// Java `FixtureProject.ENV`（`FixtureProject.java:26`）：同上，走环境变量 `MODMAKER_FIXTURE`。
pub const ENV: &str = "MODMAKER_FIXTURE";

/// Java `FixtureProject.WORKSPACE_HINT`（`FixtureProject.java:28-29`）：工作区按文档里的布局时，
/// 夹具通常所在的那个相对位置。
const WORKSPACE_HINT: &str = "../mod";

/// Java `FixtureProject.SCAN_DEPTH`（`FixtureProject.java:31-32`）：浅扫时在工作目录的父目录下
/// 搜索的层数。
const SCAN_DEPTH: i32 = 3;

/// Java `FixtureProject.probe()`（`FixtureProject.java:37-52`）：启动时要打开的那个目录；
/// 这台机器上没有任何东西像个模组时返回 `None`。
///
/// 探测顺序与 Java 逐行一致，一步都不许调：显式设置（`:39-45`）→ 工作区提示 `../mod`
/// （`:46-50`）→ 父目录下最多 [`SCAN_DEPTH`] 层的浅扫（`:51`）。前两步都过 [`as_mod_root`]，
/// 最后一步直接把 [`has_meta_file`] 交给 `SearchPaths.find`（Java 的方法引用
/// `FixtureProject::hasMetaFile`）。
pub fn probe() -> Option<PathBuf> {
    // Java FixtureProject.java:39-45 的显式设置。
    if let Some(explicit) = search_paths::override_path(PROPERTY, ENV)
        && let Some(module) = as_mod_root(&explicit)
    {
        return Some(module);
    }
    // Java FixtureProject.java:46-50 的 workspace 提示：workingDirectory().resolve("../mod")。
    let working = search_paths::working_directory();
    if let Some(hinted) = as_mod_root(&working.join(WORKSPACE_HINT)) {
        return Some(hinted);
    }
    // Java FixtureProject.java:51：SearchPaths.find(working.getParent(), SCAN_DEPTH, hasMetaFile)。
    // `Option<&Path>` 与 Java 的 `working.getParent()` 可能为 null 正好对上。
    search_paths::find(working.parent(), SCAN_DEPTH, &|folder: &Path| has_meta_file(folder))
}

/// Java `FixtureProject.hasMetaFile(Path)`（`FixtureProject.java:54-77`）：
/// `folder` 里有一个游戏愿意当成模组的元数据文件时为真——该文件必须存在、能解析，并声明一个
/// 非空白的 `name`。光有个叫 `mod.json` 的文件不够：Mindustry 自带同名的 schema 片段
/// （例如 `tools/extra-schemas/mod.json`），它们没有 `name`，绝不能被当作模组打开。
pub fn has_meta_file(folder: &Path) -> bool {
    // Java FixtureProject.java:61 遍历 ModMeta.META_FILES（游戏探测顺序，Mods.java:34）。
    for meta_file in ModMeta::META_FILES {
        let meta_path = folder.join(meta_file);
        // Java FixtureProject.java:63-65：!Files.isRegularFile → 继续下一个。
        // `Path::is_file` 即「存在且是常规文件」，目录与被跟随的符号链接目标都排除在外。
        if !meta_path.is_file() {
            continue;
        }
        // Java FixtureProject.java:67：Files.readString(metaPath, UTF_8)；非 UTF-8 字节在
        // Java 里抛 MalformedInputException（IOException 的子类），在 Rust 里是
        // `read_to_string` 的 Err —— 同一分支，都被下面的 catch 吞掉。
        let Ok(text) = fs::read_to_string(&meta_path) else {
            continue;
        };
        // Java FixtureProject.java:68：HjsonReader.readObject(text) → ModMetaCodec.fromJson(meta)。
        // 解析失败即那个 catch 里的 RuntimeException，同样吞掉。
        let Ok(object) = mm_json::read_object(&text) else {
            continue;
        };
        let meta = mod_meta_from_json(&object);
        // Java FixtureProject.java:69：meta.name != null && !meta.name.isBlank()。
        // Rust 的 `ModMeta::name` 是 `String`，键缺失时 `mod_meta_from_json` 已填 `""`
        // （`codec/meta.rs:29`），所以 `!= null` 这一半被类型吸收，只剩 `isBlank()`；
        // `trim().is_empty()` 是本 crate 既有的 `isBlank()` 写法（`search_paths.rs:31`）。
        if !meta.name.trim().is_empty() {
            return true;
        }
    }
    // Java FixtureProject.java:76：四个候选名都不合格 → 这个文件夹不是模组。
    false
}

/// Java `FixtureProject.asModRoot(Path)`（`FixtureProject.java:79-98`，private static）：
/// 目录自身就是模组时返回它，否则返回那个「单独包了一层」的、确实是模组的子目录。
fn as_mod_root(folder: &Path) -> Option<PathBuf> {
    // Java FixtureProject.java:81-83：!Files.isDirectory → null。
    if !folder.is_dir() {
        return None;
    }
    // Java FixtureProject.java:84-86：folder.toAbsolutePath().normalize()。
    if has_meta_file(folder) {
        return Some(search_paths::normalize_absolute(folder));
    }
    // Java FixtureProject.java:87-96：容忍一层包装目录，就像游戏的 resolveRoot 那样。
    // `Files.list` 与 `fs::read_dir` 都只列一层、顺序都由文件系统决定（Java 同样不保证顺序），
    // 建立列举失败（Java 的 IOException）就落到下面的 `return None`，对应 `:94-96` 的
    // 「继续往下一个地方试」。
    let Ok(entries) = fs::read_dir(folder) else {
        return None;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        // Java FixtureProject.java:89：entries.filter(Files::isDirectory)
        if !child.is_dir() {
            continue;
        }
        if has_meta_file(&child) {
            return Some(search_paths::normalize_absolute(&child));
        }
    }
    // Java FixtureProject.java:97
    None
}
