//! `StringsCheck.java` 的 1:1 移植（源文件共 37 行）。
//!
//! 逐段对应：`:9` 的类 javadoc → 下方 [`StringsCheck`] 的文档注释（直译）；`:10` 的
//! `final class StringsCheck implements ProjectCheck` → [`StringsCheck`] 与 [`ProjectCheck`] 的
//! impl；`:12-15` 的 `id()`；`:17-36` 的 `check(...)`（`:19` 取 project、`:20-21` 遍历
//! `strings.values` 的 entrySet、`:22-25` 解析键并在解析不出时跳过、`:26` 取裸名、
//! `:27-28` 两个存在性判定、`:29-33` 报告）。
//! 判定分支、遍历与报告顺序、消息键与参数数量逐条照搬，本文件不新增也不删减任何规则。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 的 `final class StringsCheck`（`:10`）是
//! **包私有**。Rust 没有「包」这一层可见性，本模块按已验收的 `check_support` 同一取舍写成
//! `pub mod` + `pub struct`：`Validator` 本体要到 P3 最后一轮才落地，crate 内暂无调用方，写成
//! `pub(crate)` 会触发 `dead_code` warning，而豁免 lint 的属性写法是本轮禁词。`Validator` 落地之后
//! 这里可收紧回 `pub(crate)`。
//!
//! Java 的隐式无参构造器（`Validator.java` 里的 `new StringsCheck()`）对应 Rust 的单元结构体
//! [`StringsCheck`]。

use mm_fmt::codec::bundle::{content_name_of, parse_key};

use crate::issue::Issue;
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `:9` 的类 javadoc 直译：
///
/// 「既指不到本模组的内容、也指不到原版内容的 bundle 键——它们是重命名留下的残余。」
///
/// 对应 Java `:10` 的 `final class StringsCheck implements ProjectCheck`。
pub struct StringsCheck;

impl ProjectCheck for StringsCheck {
    /// 对应 Java `:12-15`，返回 `"strings"`。
    fn id(&self) -> &str {
        "strings"
    }

    /// 本类**没有**覆写 Java 的 `requiresInternalName()`，因此取 `ProjectCheck.java:22-24` 那个
    /// `default` 的值 **true**（显式写出只为留档，删掉它与 trait 默认方法体行为相同）——本规则
    /// 确实要靠内部名去剥内容名前缀，跳过是对的。
    fn requires_internal_name(&self) -> bool {
        true
    }

    /// 对应 Java `:17-36` 的 `check(ValidationContext context)`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // Java :19。
        let project = context.project();
        // Java :20-21 的循环头是 `for (Map.Entry<String, ...> entry :
        // project.strings.values.entrySet())`，被遍历的那个 map 是**保持插入顺序**的类型，
        // `entrySet()` 因此按插入顺序迭代，发现的先后依赖这个顺序。
        //
        // Rust 侧对应 `mm_schema::KeyedMap`（mm-schema 里专门镜像该 Java 有序 map 的有序向量结构），
        // `iter()` 同样按插入顺序交出 `(&String, &KeyedMap<String>)`，顺序逐条对齐；本 crate 不引入
        // 任何散列映射类型，避免把顺序换成不确定的散列序。
        // 本规则只用到键（值那半是 locale -> 文本），故第二个元素直接丢弃，不对 locale 层再循环
        // ——Java 也只在键上判定，一个键只报一条。
        for (key_name, _locales) in project.strings.values.iter() {
            // Java :22-25 `BundleCodec.parseKey(entry.getKey())`，为 null 就 `continue`。
            // Rust 的 `Option` 与 Java 的 null 一一对应：`None` ⇒ 跳过（`category.*`、`stat.*`、
            // 脚本键这些本工具不管理的键保持不被评判）。
            let Some(key) = parse_key(key_name) else {
                continue;
            };
            // Java :26 `BundleCodec.contentNameOf(key.fullName(), context.internalName())`。
            // `internalName()` 像 Java 那样**在循环里逐条现算**（`ModProject::internal_name()`
            // 每次从 `meta.name` 现算，Java 也没有把它提到循环外）。
            let bare = content_name_of(&key.full_name, &context.internal_name());
            if project.content_by_name(key.ctype, &bare).is_none()
                && !context.registry().is_vanilla_name(key.ctype, &bare)
            {
                // Java :29-32 的原注释直译：
                // 「一个既指不到本模组内容、也指不到原版内容的键就是残余：模组自己的 bundle 文件里
                // 还留着一条给已被重命名或被删除的内容的字符串。覆盖原版条目
                // （`team.blue.name` 之类）是正当的——Mods.java 会把模组的 bundle 装进游戏自己的
                // bundle 链里。」
                context.report(Issue::warning(
                    "validate.bundle.orphanKey",
                    &[key_name.as_str()],
                ));
            }
        }
    }
}
