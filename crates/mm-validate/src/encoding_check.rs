//! `EncodingCheck.java` 的 1:1 移植（源文件共 40 行）。
//!
//! 逐段对应：`:6-11` 的类 javadoc → 下方 [`EncodingCheck`] 的文档注释（直译，不丢信息）；
//! `:12` 的 `final class EncodingCheck implements ProjectCheck` → [`EncodingCheck`] 与
//! [`ProjectCheck`] 的 impl；`:14-15` 的 `private static final String UTF8_BOM` → 同名私有 `const`；
//! `:17-20` 的 `id()`；`:22-25` 的 `requiresInternalName()`；`:27-39` 的 `check(...)`
//! （`:29` 取 project、`:30-32` 元数据 BOM、`:33-38` 逐条内容 BOM）。
//! 判定分支、遍历顺序、消息键与参数数量逐条照搬，本文件不新增也不删减任何规则。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 的 `final class EncodingCheck`（`:12`）是
//! **包私有**。Rust 没有「包」这一层可见性，本模块按已验收的 `check_support` 同一取舍写成
//! `pub mod` + `pub struct`：`Validator` 本体要到 P3 最后一轮才落地，crate 内暂无调用方，写成
//! `pub(crate)` 会触发 `dead_code` warning，而豁免 lint 的属性写法是本轮禁词。`Validator` 落地之后
//! 这里可收紧回 `pub(crate)`；`UTF8_BOM` 保持私有，与 Java 的 `private static final` 对齐。
//!
//! Java 的隐式无参构造器（`Validator.java:59` 的 `new EncodingCheck()`）对应 Rust 的单元结构体
//! [`EncodingCheck`]。

use crate::issue::{Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `:14-15` 的 `private static final String UTF8_BOM = "\uFEFF"`：
/// UTF-8 字节顺序标记；Windows 记事本会加上它，而游戏的 HJSON 读取器会被它卡住。
const UTF8_BOM: &str = "\u{FEFF}";

/// Java `:6-11` 的类 javadoc 直译：
///
/// 「游戏的 HJSON 读取器**扛不住**的编码损伤。
///
/// Windows 记事本把 UTF-8 存成带 BOM 的形式，读取器会把 BOM 当成第一个键。只有**未被改动**的
/// 记录才会被标记：编辑过的记录在保存时会被重新序列化成干净的。」
///
/// 对应 Java `:12` 的 `final class EncodingCheck implements ProjectCheck`。
pub struct EncodingCheck;

impl ProjectCheck for EncodingCheck {
    /// 对应 Java `:17-20`，返回 `"encoding"`。
    fn id(&self) -> &str {
        "encoding"
    }

    /// 对应 Java `:22-25`：这里**显式覆写**为 false。
    ///
    /// 本规则只看原始文本的头三个字节，跟内部名（内容名前缀）无关，内部名为空的工程也必须跑它。
    fn requires_internal_name(&self) -> bool {
        false
    }

    /// 对应 Java `:27-39` 的 `check(ValidationContext context)`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // Java :29 `ModProject project = context.project();`。交出的 `&'a ModProject` 不挂在
        // `&self` 上，于是 Java 那种「先取 project、边遍历 contents 边 report」的写法在这里成立。
        let project = context.project();
        // Java :30-32 `!project.metaDirty && project.metaText != null
        //     && project.metaText.startsWith(UTF8_BOM)`。
        //
        // Rust 侧 `meta_text` 是 `String`（mm-model/mm-io 已确立并验收的「空串表示没有值」口径）：
        // Java 的 `!= null` 对应 `!is_empty()`。该守卫在这里其实是**冗余**的——空串本来就
        // `starts_with(BOM) == false`——但为了把 Java 的两个条件逐条写清、让真值表一目了然而保留：
        // - Java `metaText == null` ⇒ Rust `""` ⇒ 不报告；
        // - Java `""` ⇒ Rust `""` ⇒ 不报告；
        // - Java 以 BOM 开头的文本 ⇒ 两边都报告。
        // 没有放宽、也没有新增判定。
        if !project.meta_dirty && !project.meta_text.is_empty()
            && project.meta_text.starts_with(UTF8_BOM)
        {
            context.report(Issue::error("validate.encoding.metaBom", &[] as &[&str]));
        }
        // Java :33-38：按 `project.contents` 的原顺序遍历（`for (ContentRecord record : ...)`），
        // 报告顺序因此与 Java 逐条一致。
        for record in &project.contents {
            // Java :34-35 的四个短路条件原次序写成一条 `&&` 链（`:34` 的 `!record.isInline()`、
            // `!record.dirty`，`:35` 的 `record.raw != null`、`record.raw.startsWith(UTF8_BOM)`）。
            // `record.raw != null` ↔ Rust 的 `Option<String>`：`as_deref()` + `is_some_and` 让
            // `None`（Java 的 null）直接为假，`Some(text)` 才比 BOM——**没有**把 `None` 当成空串
            // 去比前缀，短路位置也和 Java 一致（前两个条件为假时第三个根本不求值）。
            if !record.is_inline()
                && !record.dirty
                && record
                    .raw
                    .as_deref()
                    .is_some_and(|raw| raw.starts_with(UTF8_BOM))
            {
                // Java :36 `Issue.errorOn(record.id, key, record.name)`。
                //
                // 这里用 [`Issue::new`] 而不是 [`Issue::error_on`]，是本轮唯一必要的 API 取舍：
                // `error_on` 的签名是 `content_id: impl Into<String>`（非空），而 Java 的
                // `record.id` 是**可空** `String`，`new Issue(..., contentId)` 允许传 null。
                // `ContentRecord::id` 是 `Option<String>`，`Issue::new` 的第四参正是
                // `Option<String>`，与 Java 的 record 规范构造器一一对应：
                // `id == None` ⇒ Java 的 `contentId == null`（`content_id()` 交出 `None`），
                // `id == Some(x)` ⇒ Java 的 `contentId == x`。
                // 若改用 `error_on(record.id.unwrap_or_default(), ...)` 会把「没有 id」变成
                // 「id 是空串」，`content_id()` 由 `None` 变成 `Some("")`，属于静默改语义，故不采用。
                context.report(Issue::new(
                    Level::Error,
                    "validate.encoding.contentBom",
                    &[record.name.as_str()],
                    record.id.clone(),
                ));
            }
        }
    }
}
