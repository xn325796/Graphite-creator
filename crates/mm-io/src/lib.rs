//! mm-io：`core.io`（21 类）的等价移植——读包、建项目、存盘、导出，字节保真、原子写、
//! JSON Canvas 画布。P2 逐件填充。

pub mod files;
pub mod project;
pub mod package;
pub mod import;
pub mod canvas;
pub mod importer;
pub mod project_io;
pub mod project_factory;
/// 脚本类推断（2026-11-26，产品评审 R-02）：从 `scripts/**/*.js` 的
/// `extend(<Class>, '<name>')` 得到「内容名 → 类名」表。Java 没有这一步，见模块头。
pub mod script_scan;
pub mod autosave;
pub mod export;
pub mod fixture_project;
