//! mm-io 的「读包」层：把磁盘上的一个模组（目录或 zip/jar）读成原始模组包。
//!
//! 对应 Java `dev.modmaker.core.io` 的 `ModPackage`（数据持有者）与
//! `ModPackageReader`（按游戏的读法解析归档/目录）。

pub mod mod_package;
pub mod reader;

pub use mod_package::{ModPackage, SourceContent};
pub use reader::{read, read_with};
