//! `ProjectCheck.java` 的等价移植：工程校验里的一条规则，可独立运行。
//!
//! 对应 Java `ProjectCheck.java:3-11` 的接口注释：`Validator` 过去是一个方法按固定顺序调用十几
//! 个私有 helper——加一条规则就得改主流程、规则无法单独关掉、只需要一族发现的场景（比如重命名
//! 之后的引用扫描）也得为全部规则付账。于是一条 check 变成一个带稳定 id、函数体自包含、与邻居
//! 没有顺序依赖的小对象，唯一共享的状态是只读的 [`ValidationContext`]。
//!
//! 与 Java 的语言差异（只有下面这几处）：
//! - Java `interface` ↔ Rust `trait`，三个方法都以 `&self` 接收实现者，因此 trait 是对象安全的：
//!   下一步的 `Validator` 可以原样把 Java 的 `List<ProjectCheck>` 移植成
//!   `Vec<Box<dyn ProjectCheck>>`。
//! - Java `String id()` ↔ `fn id(&self) -> &str`：Java 的 `String` 本来就是不可变值，Rust 交出
//!   借用视图语义等价；调用方要 owning 的 `String` 时自行 `.to_string()`（与 `Issue::key()`
//!   交 `&str` 的既有口径一致）。
//! - Java `void check(ValidationContext context)` ↔ `fn check(&self, &mut ValidationContext)`：
//!   Java 靠注释约定「只往 context 里追加发现、不得改动 project」，Rust 用 `&mut` 精确表达这一半
//!   （可追加），project 只能经 [`ValidationContext::project`] 拿到共享借用，改动那一半由借用
//!   检查器保证。

use crate::validation_context::ValidationContext;

/// 对应 Java `ProjectCheck.java:12` 的 `public interface ProjectCheck`。
pub trait ProjectCheck {
    /// 对应 Java `ProjectCheck.java:15` 的 `String id()`。
    ///
    /// 稳定的短 id；用于选择性运行（Java `Validator.validate(project, onlyIds)`）并让测试失败
    /// 信息保持可读。
    fn id(&self) -> &str;

    /// 对应 Java `ProjectCheck.java:22-24` 的 `default boolean requiresInternalName()`。
    ///
    /// Java 的 `default` 方法 ↔ Rust trait 的默认方法体，规则不写就取 `true`。
    ///
    /// 当规则要通过模组的内部名去读内容名时为 true。内部名为空的工程会**整条跳过**这类规则：
    /// 那时每个内容文件名和每个引用前缀都没有意义。
    fn requires_internal_name(&self) -> bool {
        true
    }

    /// 对应 Java `ProjectCheck.java:27` 的 `void check(ValidationContext context)`。
    ///
    /// 把本规则的发现追加进 context，不得改动 project。
    fn check(&self, context: &mut ValidationContext<'_>);
}
