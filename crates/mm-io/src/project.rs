//! `ProjectIo.java` 的落点文件。
//!
//! 本步**只**移植 `ProjectIo` 的常量区（Java L44-L58）：`AUTHORING_DIR`、`PROJECT_FILE`、
//! `BOARDS_DIR`、`TEMP_SUFFIX`。四个常量被 `AtomicFiles`、`Autosave`、`ModExporter`、
//! `BuildManifest`、`ModPackageReader` 共享引用，因此单独放在这里，后续步骤把 `ProjectIo`
//! 的主体逐段追加进同一个文件。
//!
//! Java 原类的文档（项目目录结构）一并转写在下面，因为三个目录常量正是这套结构的骨架：
//!
//! ```text
//! <project>/mod.json              真实元数据文件（在被编辑前逐字节保持原样）
//! <project>/content/**            真实内容文件（在被编辑前逐字节保持原样）
//! <project>/bundles/**            真实 bundle 文件
//! <project>/sprites|scripts|...   真实资源
//! <project>/modmaker/project.json 画板列表、记录 id、来源信息
//! <project>/modmaker/boards/*.canvas  JSON Canvas 画板
//! ```
//!
//! 因为状态由模组文件自己承载，项目目录可以直接丢进游戏的 mods 目录去试跑；保存只重写真正
//! 改过的东西，构建就是一次普通拷贝。从未被触碰的记录保留其原始的字节。

/// `ProjectIo.AUTHORING_DIR`：作者侧元数据所在的子目录名。
pub const AUTHORING_DIR: &str = "modmaker";

/// `ProjectIo.PROJECT_FILE`：作者侧元数据文件的文件名。
pub const PROJECT_FILE: &str = "project.json";

/// `ProjectIo.BOARDS_DIR`：JSON Canvas 画板所在的子目录名（位于 `AUTHORING_DIR` 之下）。
pub const BOARDS_DIR: &str = "boards";

/// `ProjectIo.TEMP_SUFFIX`：一次保存先写入、随后换入位置的临时文件后缀。
///
/// 凡是带这个后缀的都是崩溃残留：读取端把它当噪音忽略，导出端永远不把它打包进产物。
pub const TEMP_SUFFIX: &str = ".mm-tmp";
