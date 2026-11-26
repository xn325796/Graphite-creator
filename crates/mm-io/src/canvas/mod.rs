//! `core.io` 的画布层：JSON Canvas 文件格式、导入内容的确定性自动布局、引用关系图与科技树索引。
//!
//! 对应 Java `dev.modmaker.core.io.JsonCanvas`、`dev.modmaker.core.io.AutoLayout`、
//! `dev.modmaker.core.io.LinkGraph` 与 `dev.modmaker.core.io.TechTreeIndex`。四类都只在内存里搬运
//! mm-model 的领域对象与 mm-json 的有序树，不碰磁盘，也不依赖尚未移植的 `ProjectIo`、`ModImporter`。

pub mod auto_layout;
pub mod json_canvas;
pub mod link_graph;
pub mod tech_tree_index;
