//! 跨内容引用的读写：`core.fmt.ReferenceOps` 与 `core.fmt.ReferenceRewrite` 的落点。

pub mod ops;
pub mod rewrite;

pub use ops::{amount_of, apply, apply_with_type, remove, revert, set_amount, Revert};
// `ReferenceRewrite` 的 `remove` 与 `ReferenceOps.remove` 同名不同义（前者删掉整张工程里的
// 引用，后者摘掉单个字段上的一条边），facade 里按语义改名；模块路径 `refs::rewrite::remove`
// 仍保持 Java 原名。
pub use rewrite::{
    remove as remove_references, remove_record as remove_record_references, rename, Change,
    RewriteResult,
};
