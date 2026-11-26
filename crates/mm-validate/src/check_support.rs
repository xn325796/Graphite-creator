//! `CheckSupport.java` 的等价移植：好几条校验规则共用的那些小工具。
//!
//! **可见性取舍（与 Java 的唯一结构性差异）**：Java 侧 `CheckSupport` 是 package-private 的
//! `final class`（`CheckSupport.java:14`）配一个私有构造器（`CheckSupport.java:20-21`）和全
//! `static` 成员，类注释写明「Private to the validation package: the individual checks are the
//! public seam, not these utilities」。Rust 没有「包」这一层可见性，最贴近的表达本来是
//! `pub(crate) mod` + `pub(crate)` 成员——本 crate 的每个 check 模块对应的正是 Java 的那个包。
//! 但 P3 的这一步只落地支撑层：16 个 `*Check` 与 `Validator` 还没写，crate 内**没有任何**调用方，
//! 而 `pub(crate)` 成员在没有调用方时会被 rustc 的 `dead_code` 判为「never used」（实测 7 条
//! warning，`pub mod` 外壳并不能豁免——成员的有效可见性被父模块的 `pub(crate)` 封顶）。这一步的
//! 约束又不许写豁免属性、也不许写占位调用，所以只能把模块与成员一并放成 `pub`。
//!
//! 这一放宽的代价与边界：`mm_validate::check_support::*` 暂时出现在 crate 的公开面里，但它是
//! **无泛型、无状态、无 trait 实现**的自由函数与常量，外部用了也不会改变本 crate 的任何行为；
//! 语义上它仍不属于本 crate 想对外承诺的 API（公开的 seam 是 `ProjectCheck`、`ValidationContext`、
//! `Issue`）。私有构造器没有对应物也不需要：模块不是可实例化的类型，本来就没有构造入口可被误用。
//!
//! 其余差异逐条写在各自成员的注释里（`String` ↔ `Option<&str>` 的 null 传播、`Double` ↔
//! `Option<f64>`、`Math.rint` ↔ `round_ties_even`、`String.valueOf` ↔ 既有
//! `mm_fmt::codec::java_double_text`）。

use std::collections::BTreeSet;

use mm_json::Json;
use mm_schema::SchemaRegistry;

/// 对应 Java `CheckSupport.java:16-18` 的 `static final String[] ENVIRONMENT_ROOTS`。
///
/// 其贴图必须放在 `sprites/blocks/environment/` 下的环境方块根类。Java 是 `String[]`（引用
/// `final`、内容可变），Rust 用静态切片：这里没有任何一处写入，只读遍历（`is_environment_class`）
/// 与 Java 的 `for (String root : ENVIRONMENT_ROOTS)` 一一对应。
pub const ENVIRONMENT_ROOTS: &[&str] = &[
    "Floor", "OreBlock", "StaticWall", "TallBlock", "Cliff", "SeaBush", "Prop",
];

/// 对应 Java `CheckSupport.java:23-35` 的 `isA(SchemaRegistry, String, String)`。
///
/// 该类本身或它的任一 schema 祖先是 `ancestor` 时为 true（两边都比简单名）。
///
/// null 传播与 Java 逐步对应：入参 `class_name` 可为 null（Java 的 `String className`），
/// `simpleName` 交出 null，`while (current != null …)` 因而不进循环，返回 false；
/// 祖先链读到注册表里没有的类时 `schema()` 交出 null，同样终止循环。
/// Java 用一个散列集合 `seen` + `seen.add(current)` 的返回值作循环守卫，这里换成 `BTreeSet`
/// （本工程不用散列集合）；`insert` 返回「是否新加入」正是 `Set.add` 的返回值，
/// 因此 `!insert(...)` 时 `break` 与 Java `while (... && seen.add(current))` 条件为假退出等价。
pub fn is_a(registry: &SchemaRegistry, class_name: Option<&str>, ancestor: &str) -> bool {
    let mut current = simple_name(class_name);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    while let Some(name) = current {
        if !seen.insert(name) {
            break;
        }
        if name == ancestor {
            return true;
        }
        // Java: current = schema == null ? null : simpleName(schema.trueParent());
        current = match registry.schema(name) {
            Some(schema) => simple_name(schema.true_parent()),
            None => None,
        };
    }
    false
}

/// 对应 Java `CheckSupport.java:37-45` 的 `isEnvironmentClass(SchemaRegistry, String)`。
///
/// 贴图必须放在 `sprites/blocks/environment/` 下的地板、墙与道具。
pub fn is_environment_class(registry: &SchemaRegistry, class_name: Option<&str>) -> bool {
    for root in ENVIRONMENT_ROOTS {
        if is_a(registry, class_name, root) {
            return true;
        }
    }
    false
}

/// 对应 Java `CheckSupport.java:47-49` 的 `simpleName(String)`。
///
/// Java 的 null 进 null 出用 `Option<&str>` 原样表达，**不**拿空串顶替 null：调用方
/// （[`is_a`]）正是靠 null 结束祖先循环。没有 `.` 的类名整段就是简单名，对应 Java
/// `lastIndexOf('.') + 1 == 0` 时的 `substring(0)`。
pub fn simple_name(class_name: Option<&str>) -> Option<&str> {
    class_name.map(|name| match name.rfind('.') {
        Some(dot) => &name[dot + 1..],
        None => name,
    })
}

/// 对应 Java `CheckSupport.java:51-55` 的 `stripPrefix(String, String)`。
///
/// 去掉模组的 `<internal>-` 前缀，让裸引用像游戏那样可比。判定顺序照 Java：先要求内部名非空，
/// 再看是否以 `<internal>-` 开头；都不满足就原样返回。Java 的 `substring(prefix.length())` 是
/// 一次拷贝（Java 7u6 起），Rust 交出借用切片，字符串不可变，观察效果一致；返回值只会来自
/// `name`，因此生命周期只标在 `name` 上。
pub fn strip_prefix<'a>(name: &'a str, internal: &str) -> &'a str {
    let prefix = format!("{internal}-");
    if !internal.is_empty() && name.starts_with(prefix.as_str()) {
        &name[prefix.len()..]
    } else {
        name
    }
}

/// 对应 Java `CheckSupport.java:57-67` 的 `number(JsonElement)`。
///
/// 取数值，缺失或非数字时为 null（Java 返回装箱的 `Double`，Rust 用 `Option<f64>`）；入参可为
/// null（`JsonElement`），对应 `Option<&Json>`。
///
/// 三条取值/失败路径逐一对应：
/// - `value == null || !value.isJsonPrimitive()` → `Json::Null`、`Json::Arr`、`Json::Obj` 一律
///   `None`（Gson 里 `JsonNull` 不是 primitive）。
/// - `try { getAsDouble() } catch (NumberFormatException) { return null }` → 数字与字符串走
///   `mm_json` 的 `as_f64()`，它把解析失败表达成 `None`，正是 Java 那个 catch 的去向；这里
///   **不**新增 panic。
/// - 唯一的语言差异：布尔值。Gson 的 `isJsonPrimitive()` 放过 `JsonBoolean`，随后
///   `getAsDouble()` 抛的是 `UnsupportedOperationException`，**不在** `catch (NumberFormatException)`
///   的范围内——即 Java 在这里是崩掉整轮校验。Rust 没有异常可抛，按方法自身注释承诺的兜底
///   （"or null when absent or not numeric"）交 `None`，即「非数字，规则跳过」。
pub fn number(value: Option<&Json>) -> Option<f64> {
    let value = value?;
    match value {
        Json::Num(_) | Json::Str(_) => value.as_f64(),
        Json::Null | Json::Bool(_) | Json::Arr(_) | Json::Obj(_) => None,
    }
}

/// 对应 Java `CheckSupport.java:69-72` 的 `trim(double)`。
///
/// 消息文本里的紧凑数字呈现：150.0 变 150，0.2 仍是 0.2。
///
/// 逐步对应 Java `value == Math.rint(value) ? String.valueOf((long) value) : String.valueOf(value)`：
/// - `Math.rint`（就近取整、整点逢二取偶，NaN→NaN，±Inf→±Inf）↔ `f64::round_ties_even`，
///   因此等值判定的四个边界一致：有限的整数值为 true；±Infinity 为 true；NaN 为 false；
///   -0.0 为 true（`-0.0 == 0.0`）。
/// - `(long) value`（Java 的饱和转换：超界取 `Long.MAX_VALUE`/`Long.MIN_VALUE`，NaN 取 0）↔
///   Rust 的 `as i64`（同样饱和），再各自渲染成十进制整数串。于是 +∞ 两边都是
///   "9223372036854775807"，-∞ 两边都是 "-9223372036854775808"，-0.0 两边都是 "0"，
///   1e20 这类「整数值但超出 long」两边都落到 `i64::MAX` 的字面量。
/// - `String.valueOf(double)`（JDK 的 `Double.toString`：至少一位小数，量级 <1e-3 或 ≥1e7 改
///   `E` 记法）↔ 本仓库既有且已带测试的 `mm_fmt::codec::java_double_text`，Rust 原生 `Display`
///   的写法（`2` 而非 `2.0`、大数不用 `E`）在此不采用。这一支只会在「非整数」或 NaN 时到达：
///   NaN 两边都是 "NaN"，±Infinity 已在整值支被取走。
pub fn trim(value: f64) -> String {
    if value == value.round_ties_even() {
        (value as i64).to_string()
    } else {
        mm_fmt::codec::java_double_text(value)
    }
}
