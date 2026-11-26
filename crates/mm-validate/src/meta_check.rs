//! `MetaCheck.java` 的 1:1 移植（源文件共 35 行）。
//!
//! 逐段对应：`MetaCheck.java:7-10` 的类 javadoc → 下方 [`MetaCheck`] 的文档注释（直译，不丢信息）；
//! `:11` 的 `final class MetaCheck implements ProjectCheck` → [`MetaCheck`] 与 [`ProjectCheck`] 的
//! impl；`:13-16` 的 `id()` → [`ProjectCheck::id`]；`:18-21` 的 `requiresInternalName()` →
//! [`ProjectCheck::requires_internal_name`]；`:23-34` 的 `check(ValidationContext)` →
//! [`ProjectCheck::check`]（`:25` 取 `metaFile`、`:26-30` 元数据文件名判定、`:31-33` 空名判定）。
//! 判定分支、报告次序、消息键与参数数量全部逐条照搬，本文件不新增也不删减任何规则。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 的 `final class MetaCheck`（`:11`）是**包私有**。
//! Rust 没有「包」这一层可见性，本模块按已验收的 `check_support` 同一取舍写成 `pub mod` +
//! `pub struct`：`Validator` 本体要到 P3 最后一轮才落地，crate 内暂无调用方，写成 `pub(crate)`
//! 会被 rustc 的 `dead_code` 判为「never used」，而豁免 lint 的属性写法是本轮禁词。`Validator`
//! 落地之后这里可收紧回 `pub(crate)`。
//!
//! Java 的隐式无参构造器（`Validator.java:57` 的 `new MetaCheck()`）对应 Rust 的单元结构体
//! [`MetaCheck`]：该类没有任何字段，是无状态的规则对象。

use mm_model::ModMeta;

use crate::issue::Issue;
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `MetaCheck.java:7-10` 的类 javadoc 直译：
///
/// 「决定其余任何东西是否连**被寻址**的资格都没有的那份元数据：游戏去探测的那个文件名，
/// 以及每个内容名都以其为前缀的模组名。」
///
/// 对应 `MetaCheck.java:11` 的 `final class MetaCheck implements ProjectCheck`。
pub struct MetaCheck;

impl ProjectCheck for MetaCheck {
    /// 对应 Java `MetaCheck.java:13-16`，返回 `"meta"`。
    fn id(&self) -> &str {
        "meta"
    }

    /// 对应 Java `MetaCheck.java:18-21`：这里**显式覆写**为 false。
    ///
    /// 本规则读的正是决定内部名的那个 `meta.name`，因此内部名为空的工程也必须跑它——不能跟着
    /// `Validator` 那条「内部名为空就整条跳过」的短路一起被跳掉。
    fn requires_internal_name(&self) -> bool {
        false
    }

    /// 对应 Java `MetaCheck.java:23-34` 的 `check(ValidationContext context)`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // Java :24-25 `ModProject project = context.project(); String metaFile = project.metaFile;`
        // ——Rust 把后一句推迟到用到的地方直接读 `project().meta_file`（String 不可变，
        // 先拷一份局部变量与就地借用没有观察差异）。
        let project = context.project();
        // Java :26-30：游戏只探测包根下的四个元数据文件名，别的名字找不到。
        //
        // Java 的守卫是 `metaFile != null && !metaFile.isEmpty() && !META_FILES.contains(metaFile)`；
        // Rust 侧 `ModProject::meta_file` 是 `String`，**空串就是「没有值」**（mm-model/mm-io 已确立
        // 并验收的口径，不是本文件的决定）。真值表逐行核对：
        // - Java `null` ⇒ Rust `""` ⇒ `is_empty()` 为真 ⇒ 整个条件为假 ⇒ 不报告（与 Java 短路一致）；
        // - Java `""`（非 null 但空）⇒ Rust `""` ⇒ 同样不报告；
        // - Java 非空字符串 ⇒ Rust 同一字符串 ⇒ `is_empty()` 为假 ⇒ 比 membership，两边一致。
        // 于是 Java 的 `!= null` 与 `!isEmpty()` 两个条件在 Rust 合并成一个 `!is_empty()`，
        // 三种输入的去向完全相同。
        if !project.meta_file.is_empty()
            && !ModMeta::META_FILES.iter().any(|name| *name == project.meta_file)
        {
            context.report(Issue::error("validate.meta.badFileName", &[project.meta_file.as_str()]));
        }
        // Java :31-33：`meta.name == null || meta.name.isBlank()`。
        //
        // Rust 侧 `ModMeta::name` 是 `String`（键缺失时为 `""`）。Java 的两个分支在 Rust 合成一个
        // `is_blank(...)`：`is_blank` 走 `chars().all(...)`，空串上 `all` **空泛成立**返回 true，
        // 正好接住 Java 的 `null`（⇒ Rust `""`）与 Java 的 `""` 两种输入——两边都是「报告」。
        // 非空串的空白判定由下方 [`is_blank`] 按 Java `isBlank()` 的字符类逐码点复刻，
        // 没有用 `trim().is_empty()`（空白集不同，见 [`is_blank`] 注释）。
        if is_blank(&project.meta.name) {
            context.report(Issue::error("validate.meta.emptyName", &[] as &[&str]));
        }
    }
}

/// Java `String.isBlank()`：字符串为空，或**每个**码点都是空白时为真。
///
/// 采用本仓库既有且更精确的那一套移植（`mm-io` 的 `canvas/json_canvas.rs:354-357` 与 `:359-369`、
/// `mm-fmt` 的 `codec/properties.rs:12-22` 的 `is_ws`——两者字符类相同），本 crate 取不到它们
/// （`fn is_blank` 是私有的、`is_ws` 是 `pub(crate)` 的，而本轮禁止新增共享模块文件），
/// 于是原样复刻同一判定，不在这里另立第三套口径。工程里另有 `trim().is_empty()` 的宽松写法
/// （`mm-io` 的 `search_paths.rs`/`fixture_project.rs`），其空白集与 Java 不一致，本文件不采用。
fn is_blank(text: &str) -> bool {
    text.chars().all(java_whitespace)
}

/// Java `Character.isWhitespace`（`mm_io::canvas::json_canvas::java_whitespace` 与
/// `mm_fmt::codec::properties::is_ws` 的同一复刻）：
/// - 含 Unicode `White_Space`（U+0009-U+000D、U+0020、U+0085、U+00A0、U+1680、U+2000-U+200A、
///   U+2028、U+2029、U+202F、U+205F、U+3000）；
/// - 加上 Java 认定为空白、Unicode `White_Space` 却不认的 U+001C-U+001F（四个分隔符）；
/// - 减去 Java 明确**不**算空白的 U+00A0 不间断空格与 U+0085 NEL。
///
/// 残余差异如实记录（沿用既有移植，本文件不改动它，以免同一工程出现两套 `isBlank`）：
/// Java 还把 U+2007 FIGURE SPACE、U+202F NARROW NO-BREAK SPACE 排除在空白之外，而这里的
/// `char::is_whitespace` 会把两者算进去。
fn java_whitespace(c: char) -> bool {
    if c == '\u{a0}' || c == '\u{85}' {
        return false;
    }
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
        || matches!(c, '\u{1c}'..='\u{1f}')
        || c == ' '
        || c.is_whitespace()
}
