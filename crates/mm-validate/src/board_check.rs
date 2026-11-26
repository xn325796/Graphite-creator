//! `BoardCheck.java` 的 1:1 移植（源文件共 27 行）。
//!
//! 逐段对应：`:6` 的类 javadoc → 下方 [`BoardCheck`] 的文档注释（直译）；`:7` 的
//! `final class BoardCheck implements ProjectCheck` → [`BoardCheck`] 与 [`ProjectCheck`] 的 impl；
//! `:9-12` 的 `id()`；`:14-26` 的 `check(...)`（`:16` 取 project、`:17-18` 双层遍历 boards/nodes、
//! `:19-21` 三个短路条件、`:22` 报告）。
//! 判定分支、遍历与报告顺序、消息键与参数数量逐条照搬，本文件不新增也不删减任何规则。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 的 `final class BoardCheck`（`:7`）是
//! **包私有**。Rust 没有「包」这一层可见性，本模块按已验收的 `check_support` 同一取舍写成
//! `pub mod` + `pub struct`：`Validator` 本体要到 P3 最后一轮才落地，crate 内暂无调用方，写成
//! `pub(crate)` 会触发 `dead_code` warning，而豁免 lint 的属性写法是本轮禁词。`Validator` 落地之后
//! 这里可收紧回 `pub(crate)`。
//!
//! Java 的隐式无参构造器（`Validator.java` 里的 `new BoardCheck()`）对应 Rust 的单元结构体
//! [`BoardCheck`]。

use mm_model::canvas_node::KIND_CONTENT;

use crate::issue::Issue;
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `:6` 的类 javadoc 直译：
///
/// 「内容记录已经不存在的卡片——它们既没法渲染，也没法写出。」
///
/// 对应 Java `:7` 的 `final class BoardCheck implements ProjectCheck`。
pub struct BoardCheck;

impl ProjectCheck for BoardCheck {
    /// 对应 Java `:9-12`，返回 `"boards"`。
    fn id(&self) -> &str {
        "boards"
    }

    /// 本类**没有**覆写 Java 的 `requiresInternalName()`，因此取 `ProjectCheck.java:22-24` 那个
    /// `default` 的值 **true**（显式写出只为留档，删掉它与 trait 默认方法体行为相同）。
    fn requires_internal_name(&self) -> bool {
        true
    }

    /// 对应 Java `:14-26` 的 `check(ValidationContext context)`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // Java :16。交出的 `&'a ModProject` 不挂在 `&self` 上，所以可以「先取 project、
        // 边遍历 boards/nodes 边 report」，与 Java 的写法一致。
        let project = context.project();
        // Java :17-18 的 `for (var board : project.boards) for (var node : board.nodes)`：
        // 两层都按 `Vec` 的原顺序遍历，报告顺序因此与 Java 逐条一致（一个悬空卡报一条，
        // 同板多卡就多报几条——Java 同样没有去重）。
        for board in &project.boards {
            for node in &board.nodes {
                // Java :19-21 是一条 `&&` 链，短路次序为 kind → contentId != null →
                // contentById == null。下面三个 `continue` 与这条链一一对应：只有三者全真才报告，
                // 前一个条件为假时后面的求值根本不发生（Java 里也就此结束本次迭代体，
                // 循环体在 `:23` 之后再没有别的语句，所以 `continue` 与 Java 的短路结束等价）。
                if node.kind != KIND_CONTENT {
                    // Java :19 `CanvasNode.KIND_CONTENT.equals(node.kind)`：常量在左的 `String.equals`，
                    // 即逐字符相等（`node.kind` 为 null 时返回 false，不抛 NPE）。Rust 侧
                    // `CanvasNode` 的 `kind` 字段是非空 `String`，空串同样不等于 `"content"`，
                    // 于是 Java 的 null 分支与 Rust 的空串分支都落到同一个 continue，无差异。
                    continue;
                }
                // Java :20 `node.contentId != null`。Rust 侧是 `Option<String>`：`None` 就是 Java 的
                // null，这里用 `let ... else` 跳过，**不**把 None 换成空串去查（那会多查一次表，
                // 且把「没有 id」伪装成「id 是空串」；`content_by_id("")` 理论上还能命中一条
                // id 为空串的记录，属于静默改语义）。
                let Some(content_id) = node.content_id.as_deref() else {
                    continue;
                };
                // Java :21 `project.contentById(node.contentId) == null` ↔ Rust 的 `Option` 为 `None`。
                if project.content_by_id(content_id).is_none() {
                    context.report(Issue::warning(
                        "validate.board.danglingCard",
                        &[board.name.as_str()],
                    ));
                }
            }
        }
    }
}
