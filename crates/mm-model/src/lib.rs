//! mm-model：ModProject / ContentRecord / Board / Edge 等领域模型。
//!
//! 对应 Java 侧 `core.model`（8 类）。`ContentRecord` 保持
//! "解析后的字段树 `fields` + 原始文件文本 `raw`" 的双持有结构。
//!
//! 依赖方向：mm-model 只用最底层的 mm-json 与 mm-schema。`ContentType` 与 `NameRules`
//! 之所以落在 mm-schema（而非 mm-fmt），是为了让 model 能引用它们又不与 `mm-fmt → model`
//! 形成环；mm-fmt（P2）之后原样 re-export。这里再 re-export `ContentType` 便于调用方使用。

pub mod asset_ref;
pub mod board;
pub mod canvas_edge;
pub mod canvas_node;
pub mod content_record;
pub mod mod_meta;
pub mod mod_project;
pub mod string_table;

pub use asset_ref::AssetRef;
pub use board::{Board, Viewport};
pub use canvas_edge::CanvasEdge;
pub use canvas_node::CanvasNode;
pub use content_record::ContentRecord;
pub use mod_meta::ModMeta;
pub use mod_project::ModProject;
pub use string_table::StringTable;

pub use mm_schema::ContentType;
