//! `dev.modmaker.core.io.ModExporter` / `BuildManifest` / `ExportReport` 的等价实现（Java 源文件
//! 313 / 158 / 29 行）。
//!
//! 切分方式沿用 [`crate::package`] 与 [`crate::import`]：三个 Java 类各自一个文件，
//! 这里只做 `pub use` 汇总。类与 Rust 文件的对应关系：
//!
//! - [`mod_exporter`] —— `ModExporter.java`：构建目录树 / zip 包，以及那套「什么才算 mod 内容」
//!   的可见性判定（`isModPath` / `modFiles` / `modDirectories`）。可见性逐名对齐 Java：
//!   `public static` 的八项（`buildFolder` 的两个重载、`buildZip` 的两个重载、`modFiles`、
//!   `modDirectories`、`requireRoot`、`requireInternalName`）是 `pub` 并从本模块再导出；
//!   `private static` 的（`pruneStale`、`directoriesWithFiles`、`isModPath`、`requireOutside`、
//!   `startsWithIgnoreCase`、`relative`）与两个 `private` 常量（`FIXED_TIME`、`MOD_DIRECTORIES`）
//!   在这里同样是私有的。只有路径 helper 与 `walk_tree`、`equals_ignore_case` 因为要给
//!   [`build_manifest`] 共用而放宽到 `pub(super)`，理由写在各自注释里。
//! - [`build_manifest`] —— `BuildManifest.java`：构建目录里那份内容指纹。Java 里 `public` 的
//!   常量与方法全部 `pub` 并再导出（`create` / `write` 因为名字太泛，再导出时加了
//!   `create_manifest` / `write_manifest` 前缀，只有这一点是表达差异）。
//!   `hashTree` **不**调用 `isModPath`（Java 第 112-121 行自己那套过滤：`excludeAuthoring`
//!   加 `TEMP_SUFFIX`），它只与 `modFiles` / `modDirectories` 共用 [`mod_exporter::walk_tree`]，
//!   因为那三处的 `Files.walk` 是同一件事。
//! - [`export_report`] —— `ExportReport.java`：Java `record` 的等价 struct，字段访问器
//!   （`target()` / `zip()` / `files()` / `bytes()` / `issues()` / `pruned()` / `hasErrors()`）
//!   对应到同名字段与方法，`summary()` 逐字符照抄。
//!
//! ## 关于 Java 的 `synchronized` 与这里的 `SAVE_LOCK`
//!
//! `ModExporter.java` 全文没有 `synchronized`，而且**一次 `ProjectIo` 的方法都不调用**——它只用
//! `ProjectIo` 的两个常量（Java 第 104、247 行的 `AUTHORING_DIR` 与第 195 行的 `TEMP_SUFFIX`）。
//! 因此导出这条链根本不进 `crate::project_io::SAVE_LOCK` 罩着的那段类锁临界区（Java 的
//! `ProjectIo.save` / `ProjectIo.write` 是 `static synchronized`，Rust 用进程级 `SAVE_LOCK`
//! 覆盖同一段，见 `crate::project_io` 第 47-61 行），export 模块既不取它、也不自造第二套锁。
//! 后续阶段若要在构建前先保存模型，必须调用 `project_io` 的**公开加锁 API**（`save` / `write`），
//! 并且调用点自身不得持有 `SAVE_LOCK`：Rust 的 `Mutex` 不可重入，同线程重复取锁会自死锁，
//! 而 Java 的类锁是可重入监视器——这是两边唯一的粒度差别。
//!
//! ## 未移植的依赖
//!
//! 本模块不引用任何尚未移植的 Java 类型：`BuildPipeline`（`core.pipeline`）、`Validator`
//! （`core.validate`）、`ui.*` 与 `BuildAction` 都不在 Java 这三个类的引用列表里，Java 侧把它们
//! 的职责留在了调用方（构建前先 `Validator.validate` 再把 issues 传进来）。`plan` / `validate` /
//! `SavePlan` 的形态与缺口见 `crates/mm-io/src/project_io.rs` 与
//! `crates/mm-io/tests/project_io_save_plan.rs`，
//! 因此这里没有需要桩掉或简化的地方：`build_folder` / `build_folder_root_internal` /
//! `build_zip` / `build_zip_root_internal` 都会把整个 source root 再读一遍（Java 也是这样），
//! 调用方负责先把内存模型落盘。与 Java 的差异都写在模块注释里。

pub mod build_manifest;
pub mod build_pipeline;
pub mod export_report;
pub mod mod_exporter;

pub use build_manifest::{
    create as create_manifest, sha256, source_hash, tree_hash, write as write_manifest, FORMAT,
    TOOL, TOOL_VERSION,
};
pub use export_report::ExportReport;
pub use mod_exporter::{
    build_folder, build_folder_root_internal, build_zip, build_zip_root_internal, mod_directories,
    mod_files, require_internal_name, require_root,
};
