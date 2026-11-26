//! `MinGameVersionCheck.java` 的 1:1 移植（源文件共 86 行）。
//!
//! 逐段对应：`:8-20` 的类 javadoc → 下方 [`MinGameVersionCheck`] 的文档注释（直译，全部行号引用
//! 与 Java 一致）；`:21` 的 `final class MinGameVersionCheck implements ProjectCheck` →
//! [`MinGameVersionCheck`] 与 [`ProjectCheck`] 的 impl；`:23-24`/`:25-26`/`:27-28` 三个
//! `private static final int` → 同名私有 `const`；`:30-31` 的 `Pattern BUILD_NUMBER` → 私有
//! [`is_build_number`]（本轮不许引入 regex，手写等价判定）；`:33-36` 的 `id()`；`:38-41` 的
//! `requiresInternalName()`；`:43-75` 的 `check(...)`；`:77-85` 的 `private static int
//! leadingNumber(String)` → 同名自由函数 [`leading_number`]。
//!
//! **两处 `return;` 是硬约束**：`:50` 与 `:56` 之后的分支在 Java 里绝不执行，Rust 用同位置的
//! `return` 提前结束整个 `check`，因此 `empty` 与 `badFormat` 两条发现**永不**与后面三条
//! (`tooHigh`/`classModTooOld`/`tooOld`) 同时出现。判定分支、报告顺序、消息键与参数数量逐条照搬。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 的 `final class MinGameVersionCheck`（`:21`）
//! 是**包私有**。Rust 没有「包」这一层可见性，本模块按已验收的 `check_support` 同一取舍写成
//! `pub mod` + `pub struct`：`Validator` 本体要到 P3 最后一轮才落地，crate 内暂无调用方，写成
//! `pub(crate)` 会触发 `dead_code` warning，而豁免 lint 的属性写法是本轮禁词。`Validator` 落地之后
//! 这里可收紧回 `pub(crate)`；三个 `const` 与所有私有 `fn` 本来就保持私有，与 Java 的
//! `private static` 对齐。
//!
//! Java 的隐式无参构造器（`Validator.java:58` 的 `new MinGameVersionCheck()`）对应 Rust 的单元
//! 结构体 [`MinGameVersionCheck`]。

use mm_model::mod_project::TARGET_MAJOR;

use crate::issue::{Arg, Issue};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `:23-24` 的 `private static final int CURRENT_MAJOR = ModProject.TARGET_MAJOR`。
///
/// Rust 侧该常量是 `mm_model` 里的模块级 `pub const TARGET_MAJOR`（不是关联常量），取值同为 160。
/// 本工具针对的构建族；更新的 major 在这里不会加载。
const CURRENT_MAJOR: i32 = TARGET_MAJOR;

/// Java `:25-26` 的 `private static final int MIN_MOD_MAJOR = 136`：
/// Vars.java:53 —— 低于它的模组一律算 outdated，除非 legacyCompatible。
const MIN_MOD_MAJOR: i32 = 136;

/// Java `:27-28` 的 `private static final int MIN_JAVA_MOD_MAJOR = 154`：
/// Vars.java:55 —— 低于它的类模组算 outdated，除非 legacyCompatible。
const MIN_JAVA_MOD_MAJOR: i32 = 154;

/// Java `:8-20` 的类 javadoc 直译（一条不漏）：
///
/// 「决定游戏是否**根本**加载这个包的版本闸门。没能通过它的模组会被标成 unsupported/outdated
/// 并整包丢弃——没有内容、没有 bundle、没有脚本（Mods.java:1314-1331, 575, 614）——而且这一丢
/// 是静默的，所以这里的每一条发现都是「能加载」与「什么都没发生」之间的差别。
///
/// 游戏只读第一个点之前的那个数字（`getMinMajor`，Mods.java:1457-1461），任何它解析不出来的东西
/// 都回落到 0，而 0 低于它强制的两个下限：对每个模组的 `minModGameVersion` 136（Vars.java:53），
/// 以及对类模组的 `minJavaModGameVersion` 154（Vars.java:55，第二道闸门在 Mods.java:1180）。
/// `legacyCompatible: true` 是脱离这两个下限的唯一出路（Mods.java:1329-1331）；它保存在
/// `ModMeta.extra` 里，并通过 `ModMeta.legacyCompatible()` 读取（ModMetaCodec.java:36-40）。」
///
/// 对应 Java `:21` 的 `final class MinGameVersionCheck implements ProjectCheck`。
pub struct MinGameVersionCheck;

impl ProjectCheck for MinGameVersionCheck {
    /// 对应 Java `:33-36`，返回 `"minGameVersion"`。
    fn id(&self) -> &str {
        "minGameVersion"
    }

    /// 对应 Java `:38-41`：这里**显式覆写**为 false。
    ///
    /// 版本闸门读的是 `meta.minGameVersion` 本身，跟内部名无关；内部名为空的工程照样必须跑它，
    /// 不能被 `Validator` 的跳过规则带走。
    fn requires_internal_name(&self) -> bool {
        false
    }

    /// 对应 Java `:43-75` 的 `check(ValidationContext context)`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // Java :45-46 `ModMeta meta = ...; String value = meta.minGameVersion;`。
        let meta = &context.project().meta;
        let value = meta.min_game_version.as_str();
        // Java :47 `value == null || value.isBlank()`。Rust 侧 `min_game_version` 是 `String`，
        // 空串即「没有值」（mm-model/mm-io 已确立并验收的口径）：`is_blank("")` 因 `all` 空泛成立
        // 返回 true，于是 Java 的 `null` 分支与 `""` 分支在 Rust 合成同一个条件，三种输入
        // （缺失/空串/纯空白）的去向与 Java 逐一相同——都是报告 empty 并**结束**。
        if is_blank(value) {
            // 省略或空白：游戏把它默认成 "0"（Mods.java:1417）-> outdated -> 丢弃。
            context.report(Issue::error(
                "validate.meta.minGameVersion.empty",
                &[] as &[&str],
            ));
            return; // Java :50 的 `return;`
        }
        let trimmed = trim_java(value);
        // Java :53 `!BUILD_NUMBER.matcher(trimmed).matches()`。`matches()` 是**整串**匹配，
        // 对应下面手写的 [`is_build_number`]（本轮禁止引入 regex）。
        if !is_build_number(trimmed) {
            // 其它任何东西都解析成 0（Mods.java:1457-1461）-> outdated -> 丢弃。
            context.report(Issue::error("validate.meta.minGameVersion.badFormat", &[trimmed]));
            return; // Java :56 的 `return;`
        }
        let major = leading_number(trimmed);
        // Java :59 `meta.java || (meta.mainClass != null && !meta.mainClass.isBlank())`。
        // Rust 侧 `main_class` 是 `String`，空串即「没有值」：Java 的 `!= null` 那一半被类型吸收
        // （`""` 走 `!is_empty()` 为假 ⇒ 整个括号为假，与 Java null ⇒ 整个括号为假一致），
        // `!isBlank()` 那一半原样保留。两边真值表：null/`""` ⇒ false；`" "` ⇒ false；
        // `"Main"` ⇒ true。完全一致，故不放宽成只判 `!is_empty()`。
        let class_mod = meta.java || (!meta.main_class.is_empty() && !is_blank(&meta.main_class));
        let legacy_compatible = meta.legacy_compatible();
        if major > CURRENT_MAJOR {
            // Version.isAtLeast 不成立（Version.java:64-73）：模组被列出来但从不加载。
            // 对类模组来说这个结论不能当成作者的本意，所以那边是硬 error，纯内容模组才是 warning。
            // Java :65-67 是**一次** `context.report(...)`，参数位上用一个三元表达式
            // `classMod ? Issue.error(...) : Issue.warning(...)`，两条分支的消息键与两个参数
            //（`:66-67` 的 `trimmed, major`）完全相同。下面用绑定 + if/else 产出同一个 Issue 对象、
            // 再交同一次 report，与 Java 逐字对应（不是「先判级别再各报一次」）。
            //
            // 参数口径：Java 的 `Issue.error(String key, Object... args)` 在 `:66-67` 交出
            // `trimmed`（`String`）与 `major`（`int`）**两个不同类型**，`Issue.args()` 是
            // `List<Object>`，所以那个 `int` 一路带着类型走到 `--json` 报告，被
            // `ModMakerCli.argument(Object)`（`ModMakerCli.java:255-266`）写成 JSON 数字
            // `161`，而不是字符串 `"161"`。这里用 [`Arg::text`] / [`Arg::int`] 保住这个类型差；
            // 只有在渲染消息文案时才经 `String.valueOf(int)` 收成十进制串（`i32` 与 Java `int`
            // 的 `toString` 规则相同，且这里的 `major` 经 [`is_build_number`] 保证非负，
            // 不会有符号差异），两边文案一致。
            let issue = if class_mod {
                Issue::error(
                    "validate.meta.minGameVersion.tooHigh",
                    &[Arg::text(trimmed), Arg::int(major as i64)],
                )
            } else {
                Issue::warning(
                    "validate.meta.minGameVersion.tooHigh",
                    &[Arg::text(trimmed), Arg::int(major as i64)],
                )
            };
            context.report(issue);
        }
        if class_mod && major < MIN_JAVA_MOD_MAJOR && !legacy_compatible {
            context.report(Issue::error(
                "validate.meta.minGameVersion.classModTooOld",
                &[trimmed],
            ));
        }
        if major < MIN_MOD_MAJOR && !legacy_compatible {
            context.report(Issue::error("validate.meta.minGameVersion.tooOld", &[trimmed]));
        }
    }
}

/// Java `:30-31` 的 `Pattern BUILD_NUMBER = Pattern.compile("^\\d+(\\.\\d+){0,2}$")`，
/// 配合 `:53` 的 `matcher(trimmed).matches()`（整串匹配，不是 find）。
///
/// `:30` 上那行字段 javadoc 直译：「游戏能读的形状：一个纯构建号，至多两段点分。」
///
/// 本轮不许引入 regex，故按该正则的结构手写（与本仓库 `mm-json/src/reader.rs:24-71`
/// 的 `looks_like_number` 同一做法：手写逐字符的**全串锚定**匹配来顶掉 Java 的 `Pattern`）：
/// 「**一个或多个** ASCII 数字」+「**0 到 2** 组（一个 `.` 紧跟**一个或多个** ASCII 数字）」，
/// 且必须覆盖整个串。`\d` 在 Java 默认只匹配 ASCII `0-9`（未开 `UNICODE_CHARACTER_CLASS`），
/// 所以这里同样只认 `is_ascii_digit()`；按字节扫描安全——`.` 与数字都是 ASCII，
/// UTF-8 的多字节序列里不会出现落在 `0x2E`/`0x30-0x39` 的延续字节，非 ASCII 输入一律落到
/// 「数字个数为 0」或「不是 `.`」这两条 false 分支，与 Java 逐码点判定的结论相同。
///
/// 边界逐例核对（与 Java 正则的实测结果一致）：
/// - `""` → 首段数字个数 0 → false（正则 `^\d+` 要求至少一位）；
/// - `"146"` → true；`"0"` → true；
/// - `"1.2"` / `"1.2.3"` → true（点分段分别 1、2 段）；
/// - `"1.2.3.4"` → 要点第 3 段时 `dotted_parts == 2` → false（`{0,2}` 的上限）；
/// - `"1."` → 点之后数字个数 0 → false（`\.\d+` 要求点后被数字填满）；
/// - `"."` / `".."` → 首段数字个数 0 → false（前导点不合法）；
/// - `"1..2"` → 第一个点后的数字个数 0 → false；
/// - `"1.2."`（尾随点）→ 第二段点之后数字个数 0 → false；
/// - `"+1"` / `"-1"` / `"1a"` / `" 1"`（`trim()` 之后不该再有空白，出现即为 false）/ `"1,2"`
///   → 分别落在「首段数字 0」或「非 `.` 的多余字符」→ false；
/// - `"1.2a"` → 第二段数字后剩 `a`，既不是 `.` 也不是数字 → false。
fn is_build_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0usize;
    // `^\d+`：首段至少一位 ASCII 数字。
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return false;
    }
    // `(\.\d+){0,2}`：至多两段，每段必须是「一个点 + 至少一位数字」。
    let mut dotted_parts = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'.' {
            return false;
        }
        if dotted_parts == 2 {
            return false;
        }
        dotted_parts += 1;
        i += 1;
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    // 走到这里说明整串被吃完，等价于正则的 `$` 锚点。
    true
}

/// Java `:77-85` 的 `private static int leadingNumber(String value)`。
///
/// javadoc 直译：「`Strings.parseInt(..., 0)` 的语义：第一个点之前的那些数字，溢出时为 0。」
///
/// - Java `value.indexOf('.')`（`char` 索引）↔ Rust `value.find('.')`（**字节**索引）。到这里
///   为止 [`is_build_number`] 已确认串全由 ASCII 数字与 `.` 组成，字节索引与字符索引重合，
///   因此 Java 的 `substring(0, dot)` 与 Rust 的 `&value[..dot]` 取到的是同一段文本；
///   `dot == -1`（没找到）对应 Rust 的 `None`，两边都是「整串就是首段」。
/// - Java `Integer.parseInt` 溢出抛 `NumberFormatException`，被 `:82-84` 的 catch 变成 0；
///   Rust `parse::<i32>()` 溢出返回 `Err`，`unwrap_or(0)` 是同一去向。`i32` 与 Java `int`
///   的范围完全相同（`-2^31..2^31-1`），所以「哪些值溢出」也相同。首段已被判定为纯 ASCII 数字，
///   不存在符号、空白之类的其它失败原因（Java 那边同样不会遇到）。
fn leading_number(value: &str) -> i32 {
    let digits = match value.find('.') {
        Some(dot) => &value[..dot],
        None => value,
    };
    digits.parse::<i32>().unwrap_or(0)
}

/// Java `String.isBlank()`：字符串为空，或**每个**码点都是空白时为真。
///
/// 采用本仓库既有且更精确的那一套移植（`mm-io` 的 `canvas/json_canvas.rs:354-357` 与 `:359-369`、
/// `mm-fmt` 的 `codec/properties.rs:12-22` 的 `is_ws`，两者字符类相同）；它们在各自的 crate 里是
/// 私有/`pub(crate)` 项，本 crate 取不到，本轮又禁止新增共享文件，故原样复刻同一判定，
/// 不另立口径。`trim().is_empty()` 那种宽松写法（`mm-io` 的 `search_paths.rs`）空白集与 Java
/// 不一致，本文件不采用。
fn is_blank(text: &str) -> bool {
    text.chars().all(java_whitespace)
}

/// Java `Character.isWhitespace`（与 `json_canvas::java_whitespace`、`properties::is_ws` 同一复刻）：
/// 含 Unicode `White_Space`，加上 Java 认而 `White_Space` 不认的 U+001C-U+001F，减去 Java 明确不算
/// 空白的 U+00A0 与 U+0085。残余差异如实记录（沿用既有移植）：Java 还把 U+2007、U+202F 排除在
/// 空白之外，而这里的 `char::is_whitespace` 会把两者算进去。
fn java_whitespace(c: char) -> bool {
    if c == '\u{a0}' || c == '\u{85}' {
        return false;
    }
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
        || matches!(c, '\u{1c}'..='\u{1f}')
        || c == ' '
        || c.is_whitespace()
}

/// Java `String.trim()`（`:52` 的 `value.trim()`）：只剥掉首尾**码点值 `<= ' '`（U+0020）**的字符，
/// 与 `strip()`（按 `Character.isWhitespace`）不是一回事——例如 U+00A0 不间断空格不会被它剥掉，
/// 而 U+001C 会被剥掉。所以这里**不能**用 Rust 的 `str::trim()`：后者按 Unicode `White_Space`
/// 判定，会把 U+00A0/U+2028/U+3000 等剥掉（Java 不剥），又不剥 U+001C-U+001F（Java 剥）。
/// `trimmed` 会作为 `{0}` 参数进消息文本，剥错了文案就变了，故逐字符复刻 Java 的规则。
///
/// 与本仓库既有的两处 Java `trim()` 移植同一实现（`mm-fmt` 的 `codec/properties.rs:29-32`
/// 用 `trim_matches(|c: char| c <= ' ')`，`mm-json` 的 `reader.rs:73-85` 用字节扫描）：
/// Java 比较的是 **UTF-16 code unit** `<= 0x20`，Rust 的 `char <= ' '` 比较的是码点；两者在
/// `< 0x10000` 区间完全一致，而 `<= 0x20` 全在这一区间内，因此码点比较与码元比较等价。
/// 返回借用切片：Java 每次 `trim()` 新建一个 `String`，Rust 交出原串的片段；两边字符串都不可变，
/// 观察效果一致（`Issue` 的参数会被 `copy_args` 拷成 owned `String`）。
fn trim_java(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}
