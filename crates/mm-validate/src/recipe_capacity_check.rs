//! `RecipeCapacityCheck.java`（`dev.modmaker.core.validate.RecipeCapacityCheck`，87 行、
//! 2 个 `@Override`、1 个私有静态常量、1 个私有静态递归方法 `maxStackAmount`）的 1:1 忠实移植。
//!
//! Java 类注释（`RecipeCapacityCheck.java:11-13`）直译：合成台每个槽位只有 `itemCapacity`
//! （默认 10）的空间，而请求的批量写在堆叠串里（如 `copper/50`）——批量超过容量，合成台永远
//! 集不齐原料。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. `Issue.warningOn(record.id, "validate.crafter.batchTooBig", args...)`（`:43-46`）⇒
//!    `Issue::new(Level::Warning, key, &[args], record.id.clone())`（同 `team_check.rs` 第 1 条）。
//! 2. 消息参数**四个**，逐项照 Java `:47-49`：`record.name`、`(int) Math.ceil(batch)`、
//!    `(int) cap`、`capacity == null ? " (the game default)" : ""`。第四个是独立参数（模板
//!    `validate.crafter.batchTooBig` 里 `{2}{3}` 分列两处），**不能**并进第三个——早先的移植把
//!    后缀拼到了 `cap` 后面，那既让 `{3}` 落到参数表外，也让 `--json` 报告少一个元素。
//! 3. `value.isJsonArray()/isJsonObject()/isJsonNull()/isJsonPrimitive()` ⇒ `mm_json::Json` 的
//!    `is_array()/is_object()/is_null()` 与本地 [`is_json_primitive`]；`getAsJsonArray()` ⇒
//!    `as_array()`，`getAsJsonObject().get("amount")` ⇒ `as_object()?.get("amount")`——Java 里
//!    这些 cast 只在 `isXxx()` 已判真的分支后发生，`as_*` 返回 `Option` 的 `?/else` 分支因此不可达。
//! 4. `CheckSupport.number(...)` ⇒ [`crate::check_support::number`]（Gson `isJsonPrimitive` +
//!    `getAsDouble`）；`CheckSupport.trim(...)` ⇒ [`crate::check_support::trim`]。
//! 5. `Math.max(max, maxStackAmount(element))`（`:57`）⇒ [`java_max`]：**必须**按 Java 的
//!    `Math.max(double,double)`（javadoc：任一参数为 NaN ⇒ 结果 NaN）而不是 Rust `f64::max`
//!    （NaN ⇒ 返回另一个参数）。这里可达：堆叠串写成 `"item/NaN"` 时 `Double.parseDouble` 交出
//!    NaN，Java 让整个 `max` 变 NaN，`batch < 0` 判定（NaN 参与的比较恒 false）因此**不**成立，
//!    最后 `batch > cap` 也是 false ⇒ 不上报；若照 Rust 的 `.max()` 就会把 NaN 吞成 -1，转而落到
//!    `item` 分支甚至误报。
//! 6. `Double.parseDouble(text.substring(slash + 1))`（`:82`）⇒ [`java_parse_double`]，口径差异
//!    在该 helper 头部逐条登记。`String.trim()`（`:80`）⇒ [`java_trim`]（只剥码点 ≤ U+0020，
//!    与 Rust `str::trim()` 的 Unicode 空白集不同）。
//! 7. `text.indexOf('/')` 返回 UTF-16 码元下标，`substring(slash + 1)` 从斜杠之后截取；Rust 的
//!    `find('/')` 返回 UTF-8 字节下标。两者切出来的**内容**逐字符相同：ASCII 斜杠不可能出现在
//!    多字节序列内部，且「斜杠后一位」在两种编码里都是紧随其后的第一个字符起点，故 `&text[i+1..]`
//!    恒落在字符边界上。
//! 8. `(int) Math.ceil(batch)` 与 `(int) cap`（`:44-45`）⇒ [`java_ceil_i32`] / [`java_trunc_i32`]：
//!    Java 的 `(int)` 是**窄化转换**（JLS 5.1.3：向零截断、NaN→0、超界饱和到 `Integer.MAX_VALUE`/
//!    `MIN_VALUE`），Rust 的 `as i32` 自 1.45 起逐值相同；`Math.ceil` 与 `f64::ceil` 亦逐值相同。
//!    消息参数位置是 `Object...`，Java 交出的是那两个 `int` 本身 ⇒ Rust 侧 [`Arg::int`]；
//!    渲染文案时才经 `String.valueOf(int)` 收成十进制串，`--json` 报告保持 JSON 数字。
//! 9. `record.type` ⇒ `record.ctype`（`type` 是 Rust 关键字，改名已在 `mm-model` 登记）。

use mm_json::Json;
use mm_model::ModProject;
use mm_schema::ContentType;

use crate::check_support;
use crate::issue::{Arg, Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `DEFAULT_ITEM_CAPACITY`（`RecipeCapacityCheck.java:15`）：游戏在字段缺省时的值。
const DEFAULT_ITEM_CAPACITY: i32 = 10;

/// Gson `JsonElement.isJsonPrimitive()`：`Bool`/`Num`/`Str` 三种。
fn is_json_primitive(value: &Json) -> bool {
    value.is_bool() || value.is_number() || value.is_string()
}

/// Java `Math.max(double, double)`（`:57`）：任一参数为 NaN ⇒ NaN；否则取较大者。
/// Rust 的 `f64::max` 在 NaN 时返回**另一个**参数，故不能直接用（见模块头第 5 条）。
fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b {
        a
    } else {
        b
    }
}

/// Java `String.trim()`（`:80`）：剥掉**首尾**所有码点 ≤ U+0020 的字符（不是 Unicode 空白集）。
/// 返回借用切片；`char_indices` 保证切点落在字符边界。
fn java_trim(text: &str) -> &str {
    let lead = text
        .char_indices()
        .find(|(_, c)| *c > '\u{20}')
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let trail = text
        .char_indices()
        .rev()
        .find(|(_, c)| *c > '\u{20}')
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(lead);
    // 全空白（或空串）时 lead == trail == len，取空切片
    if lead >= trail {
        return "";
    }
    &text[lead..trail]
}

/// Java `(int) Math.ceil(x)`（`:44`）。口径论证见模块头第 8 条。
fn java_ceil_i32(value: f64) -> i32 {
    value.ceil() as i32
}

/// Java `(int) cap`（`:45`）：直接窄化（向零截断），**不含** ceil。
fn java_trunc_i32(value: f64) -> i32 {
    value as i32
}

/// Java `Double.parseDouble(String)`（`:82`），`NumberFormatException` ⇒ `None`（对应 `catch`）。
///
/// **与 Rust `str::parse::<f64>()` 的口径差异（逐条处理，不静默放宽）**：
/// - 前后空白：Java 的 `Double.valueOf(String)` javadoc 明写「Leading and trailing whitespace
///   characters in s will be ignored」，解析器按 `c <= ' '` 跳过；Rust 的 `parse` 一律拒绝空白。
///   ⇒ 这里先按 [`java_trim`] 的口径剥空白。注意 `:80` 已经 `trim()` 过整串，斜杠后的**内层**空白
///   仍会走到这里，故不能省。
/// - `NaN`/`Infinity`：Java 只认这两个拼写（可带正负号），**不**认小写 `inf`/`infinity`/`nan`；
///   Rust 的 `parse` 反倒认 `inf`/`infinity`/`nan` 的各种大小写。⇒ 先按 Java 的标识符显式判定，
///   再对剩下的部分做十进制形状校验（[`is_java_decimal`]），挡掉 Rust 宽于 Java 的字面量。
/// - 类型后缀 `f/F/d/D`：Java 允许（`"1.5d"` → 1.5），Rust 拒绝。⇒ 剥掉一个尾后缀再校验。
/// - 十六进制浮点字面量（Java 认 `"0x1p3"` → 8.0）：**登记为不实现**。能走到这一行的只有模组
///   JSON 里手写的字符串堆叠值（`"copper/0x1p3"`），Gson 的数字原始值不会是这个形状；本仓库其余
///   已验收的数字读取路径（`CheckSupport.number` ⇒ `mm_json::Json::as_f64`）同样只按十进制处理，
///   单点补上十六进制只会在两处制造更大的不一致。
/// - 下划线分隔（Java 的 `parseDouble` **不**接受 `1_000.0`）：Rust 的 `parse` 也不接受 ⇒ 一致。
fn java_parse_double(text: &str) -> Option<f64> {
    let core = java_trim(text);
    if core.is_empty() {
        return None;
    }
    // 符号（Java 的 FloatValue 允许前导 '+'）
    let (negative, body) = if let Some(rest) = core.strip_prefix('-') {
        (true, rest)
    } else {
        (false, core.strip_prefix('+').unwrap_or(core))
    };
    if body.is_empty() {
        return None;
    }
    // 标识符两支：Java 里 NaN 不带符号语义，"-NaN" 同样是 NaN
    if body == "NaN" {
        return Some(f64::NAN);
    }
    if body == "Infinity" {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    // 剥掉一个 Java 允许的类型后缀
    let digits = match body.chars().next_back() {
        Some('f') | Some('F') | Some('d') | Some('D') => &body[..body.len() - 1],
        _ => body,
    };
    if digits.is_empty() || !is_java_decimal(digits) {
        return None;
    }
    digits.parse::<f64>().ok()
}

/// Java 十进制浮点语法 `DecimalFloatingPointLiteral`：整数部分与小数部分（数字总数 ≥ 1，
/// 小数点至多一个），可跟 `e`/`E` 加带符号的**至少一位**十进制指数。
/// `"5."`、`".5"`、`"+.5e-3"` 合法；`"."`、`"1e"`、`"1e+"`、`"0x10"`、`"1_0"`、`"inf"` 拒绝。
fn is_java_decimal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut seen_digit = false;
    let mut seen_dot = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
            index += 1;
            continue;
        }
        if !byte.is_ascii_digit() {
            break;
        }
        seen_digit = true;
        index += 1;
    }
    if !seen_digit {
        return false;
    }
    if index == bytes.len() {
        return true;
    }
    if bytes[index] != b'e' && bytes[index] != b'E' {
        return false;
    }
    index += 1;
    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        index += 1;
    }
    let start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    index > start && index == bytes.len()
}

/// Java `private static double maxStackAmount(JsonElement value)`（`:52-65`）的递归体。
///
/// 返回 -1.0 表示「没声明批量」，Java 的调用方正以此判断（`:35`、`:37`）。注意递归的数组支
/// 可能返回 NaN（见 [`java_max`]），此时 `-1 < NaN` 与 `NaN < 0` 都是 false，控制流与 Java 一致。
fn max_stack_amount(value: Option<&Json>) -> f64 {
    // :53  if (value == null || value.isJsonNull()) return -1;
    let Some(value) = value else {
        return -1.0;
    };
    if value.is_null() {
        return -1.0;
    }
    // :54-58  数组：逐个递归取最大（初值 -1；NaN 口径见 java_max）
    if let Json::Arr(items) = value {
        let mut max = -1.0;
        for element in items {
            max = java_max(max, max_stack_amount(Some(element)));
        }
        return max;
    }
    // :59-61  对象：读 "amount"，走 CheckSupport.number（primitive 才算数）
    if let Json::Obj(object) = value {
        return check_support::number(object.get("amount")).unwrap_or(-1.0);
    }
    // :62  剩下的 Only 三种 primitive 会走到这里；Arr/Obj/Null 已在上面返回。
    //      Java 写的是 `!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString()`，
    //      前半在这里恒 false（可达分支已穷尽），保留判定只为形状对齐。
    if !is_json_primitive(value) || !value.is_string() {
        return -1.0;
    }
    // :63-65  String text = value.getAsString().trim(); int slash = text.indexOf('/');
    //         if (slash < 0) return -1;
    let Json::Str(text) = value else {
        return -1.0;
    };
    let text = java_trim(text);
    let Some(slash) = text.find('/') else {
        return -1.0;
    };
    // :66  try { return Double.parseDouble(text.substring(slash + 1)); }
    //      catch (NumberFormatException ignored) { return -1; }
    java_parse_double(&text[slash + 1..]).unwrap_or(-1.0)
}

/// Java `class RecipeCapacityCheck implements ProjectCheck`（`RecipeCapacityCheck.java:14`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct RecipeCapacityCheck;

impl ProjectCheck for RecipeCapacityCheck {
    /// Java `id()`（`RecipeCapacityCheck.java:17-20`）。
    fn id(&self) -> &str {
        "recipeCapacity"
    }

    /// Java `check(ValidationContext)`（`RecipeCapacityCheck.java:22-50`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :21  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :23  for (ContentRecord record : project.contents) {
        for record in &project.contents {
            // :24-26  只看非内联的 block
            if record.ctype != ContentType::Block || record.is_inline() {
                continue;
            }
            // :27  String className = context.registry().resolveClass(record.type, record.className());
            let class_name: Option<String> =
                context.registry().resolve_class(record.ctype, record.class_name().as_deref());
            // :28-31  className 为 null 或不是 GenericCrafter 的后代 ⇒ 整条跳过
            let Some(class_name) = class_name.as_deref() else { continue };
            if !check_support::is_a(context.registry(), Some(class_name), "GenericCrafter") {
                continue;
            }
            // :32-35  JsonElement consumes = record.fields.get("consumes");
            //         if (consumes == null || !consumes.isJsonObject()) continue;
            let Some(consume) = record.fields.get("consumes") else {
                continue;
            };
            let Json::Obj(consume_object) = consume else {
                continue;
            };

            // :37-43  double batch = maxStackAmount(consumeObject.get("items"));
            //         if (batch < 0) batch = maxStackAmount(consumeObject.get("item"));
            //         if (batch < 0) continue;
            let mut batch: f64 = max_stack_amount(consume_object.get("items"));
            if batch < 0.0 {
                batch = max_stack_amount(consume_object.get("item"));
            }
            if batch < 0.0 {
                continue;
            }

            // :44-45  Double capacity = ...; double cap = capacity == null ? DEFAULT : capacity;
            let capacity: Option<f64> = check_support::number(record.fields.get("itemCapacity"));
            let cap: f64 = capacity.unwrap_or(DEFAULT_ITEM_CAPACITY as f64);

            // :43-47  batch > cap ⇒ batchTooBig
            if batch > cap {
                // Java 的 `:48-49` 交的是**四个**参数：`record.name`（`String`）、
                // `(int) Math.ceil(batch)`（`int`）、`(int) cap`（`int`）、以及那个三元后缀
                // （`String`）。模板 `validate.crafter.batchTooBig` 用 `{2}{3}` 两处，所以后缀
                // 绝不能并进第三个参数——它自己就是第四个。
                let suffix: &str = if capacity.is_none() {
                    " (the game default)"
                } else {
                    ""
                };
                context.report(Issue::new(
                    Level::Warning,
                    "validate.crafter.batchTooBig",
                    &[
                        Arg::text(record.name.as_str()),
                        Arg::int(java_ceil_i32(batch) as i64),
                        Arg::int(java_trunc_i32(cap) as i64),
                        Arg::text(suffix),
                    ],
                    record.id.clone(),
                ));
            }
        }
    }
}
