//! `ContentOrderCheck.java` 的 1:1 移植（源文件共 28 行）。
//!
//! 逐段对应：`:8` 的类 javadoc → 下方 [`ContentOrderCheck`] 的文档注释（直译）；`:9` 的
//! `final class ContentOrderCheck implements ProjectCheck` → [`ContentOrderCheck`] 与
//! [`ProjectCheck`] 的 impl；`:11-14` 的 `id()`；`:16-27` 的 `check(...)`（`:18` 取 project、
//! `:19-21` 收集非内联记录名、`:22-26` 逐条比 contentOrder）。
//! 判定分支、报告顺序、消息键与参数数量逐条照搬，本文件不新增也不删减任何规则。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 的 `final class ContentOrderCheck`（`:9`）是
//! **包私有**。Rust 没有「包」这一层可见性，本模块按已验收的 `check_support` 同一取舍写成
//! `pub mod` + `pub struct`：`Validator` 本体要到 P3 最后一轮才落地，crate 内暂无调用方，写成
//! `pub(crate)` 会触发 `dead_code` warning，而豁免 lint 的属性写法是本轮禁词。`Validator` 落地之后
//! 这里可收紧回 `pub(crate)`。
//!
//! Java 的隐式无参构造器（`Validator.java` 里的 `new ContentOrderCheck()`）对应 Rust 的单元结构体
//! [`ContentOrderCheck`]。

use crate::issue::Issue;
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `:8` 的类 javadoc 直译：
///
/// 「元数据的 content-order 列表里，那些**指不到本模组任何内容**的条目。」
///
/// 对应 Java `:9` 的 `final class ContentOrderCheck implements ProjectCheck`。
pub struct ContentOrderCheck;

impl ProjectCheck for ContentOrderCheck {
    /// 对应 Java `:11-14`，返回 `"contentOrder"`。
    fn id(&self) -> &str {
        "contentOrder"
    }

    /// 本类**没有**覆写 Java 的 `requiresInternalName()`，因此取
    /// `ProjectCheck.java:22-24` 那个 `default` 的值 **true**（显式写出来并在此说明，
    /// 与 Rust trait 的默认方法体行为相同，删掉这两个字也不改变结果）。
    fn requires_internal_name(&self) -> bool {
        true
    }

    /// 对应 Java `:16-27` 的 `check(ValidationContext context)`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // Java :18。
        let project = context.project();
        // Java :19 `Set<String> names = new LinkedHashSet<>()` + `:20-21` 的
        // `contents.stream().filter(record -> !record.isInline()).forEach(record -> names.add(record.name))`。
        //
        // `LinkedHashSet` 的**插入顺序**在这里是可见行为（本轮禁止引入散列映射/集合类型），
        // 因此用 `Vec<String>` 手工复刻它的两条语义：
        // - `add` 去重：同名只留**第一次**出现的那一项（`!names.contains(...)` 才 push），
        //   这正是 `LinkedHashSet.add` 在已存在时「什么也不动」的行为——既不重复、也不把该元素
        //   挪到队尾；
        // - 遍历顺序：按 `project.contents` 的原顺序追加，与 Java 的 stream 顺序一致
        //   （`stream()` 是顺序流，`filter` 不改序）。
        // 注意本规则的**报告**顺序只由 `contentOrder` 的遍历决定，`names` 只用来判存在，
        // 但集合的构造顺序仍按上面复刻，避免任何潜在的可见差异。
        let mut names: Vec<String> = Vec::new();
        for record in &project.contents {
            if !record.is_inline() && !names.iter().any(|existing| existing == &record.name) {
                names.push(record.name.clone());
            }
        }
        // Java :22-26：按 `meta.contentOrder` 的列表顺序遍历（Java 是 `List<String>`，
        // Rust 是 `Vec<String>`，两者都是有序可重复），每条指不到内容的条目报一次 warning。
        for ordered in &project.meta.content_order {
            if !names.iter().any(|name| name == ordered) {
                context.report(Issue::warning("validate.contentOrder.unknown", &[ordered.as_str()]));
            }
        }
    }
}
