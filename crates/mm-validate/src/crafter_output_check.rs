//! `CrafterOutputCheck.java`（`dev.modmaker.core.validate.CrafterOutputCheck`，49 行、
//! 2 个 `@Override`）的 1:1 忠实移植。
//!
//! Java 类注释（`CrafterOutputCheck.java:8-11`）直译：合成台的产出错误，游戏是「静默惩罚」的——
//! `outputItems` 一设定，`outputItem` 就被忽略；液体产出除非显式 `ignoreLiquidFullness`，
//! 否则会淤满卡死。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. `Issue.warningOn(record.id, key, record.name)`（`:33-34`、`:44-45`）⇒
//!    `Issue::new(Level::Warning, key, &[record.name.as_str()], record.id.clone())`（同
//!    `team_check.rs` 第 1 条：`record.id` 可 null，`warning_on` 交不出 `None`）。
//! 2. `record.fields.containsKey(...)`（`:31-32`、`:42-43`）⇒ [`mm_json::Object::contains`]：
//!    Gson `JsonObject.has` 与「有键」一一对应，包括值为 `JsonNull` 时也算「有键」，两边一致。
//! 3. `ignoreFull.isJsonPrimitive()`（`:37`）⇒ [`is_json_primitive`]（Gson primitive =
//!    `Bool`/`Num`/`Str`）；`getAsBoolean()` ⇒ [`gson_as_boolean`]，逐条照 Gson
//!    `JsonPrimitive.getAsBoolean()` 的实现（见该 helper 的注释）。
//! 4. `CheckSupport.isA` ⇒ [`crate::check_support::is_a`]；`resolveClass` ⇒
//!    [`mm_schema::SchemaRegistry::resolve_class`]；`record.type` ⇒ `record.ctype`
//!    （`type` 是 Rust 关键字，改名已在 `mm-model` 登记）。
//! 5. Java 的守卫 `:26-29`（`className == null || !isA(...)` ⇒ 整条跳过）与 `:39-40` 的注释
//!    （`outputLiquid` 在 init 时被复制进 `outputLiquids` 数组，`GenericCrafter.java:125-127`，
//!    停摆判定只读数组，`GenericCrafter.java:214-229`）原样保留：两种拼法都算「这个方块输出液体」。

use mm_json::Json;
use mm_model::ModProject;
use mm_schema::ContentType;

use crate::check_support;
use crate::issue::{Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Gson `JsonElement.isJsonPrimitive()`：`Bool`/`Num`/`Str` 三种（`JsonNull`/`Arr`/`Obj` 为 false）。
fn is_json_primitive(value: &Json) -> bool {
    value.is_bool() || value.is_number() || value.is_string()
}

/// Gson `JsonPrimitive.getAsBoolean()`（`CrafterOutputCheck.java:37-38`），照其实现逐支对应：
/// - 布尔原始值 ⇒ 原值。
/// - 字符串 ⇒ `"true".equalsIgnoreCase(s)` → true、`"false".equalsIgnoreCase(s)` → false
///   （**大小写不敏感**，用 `eq_ignore_ascii_case`）。
/// - 其余（数字原始值，或上面两条都不匹配的字符串）⇒ `Double.valueOf(getAsString()).intValue() == 1`：
///   取**源文本**（Gson 对数字原始值的 `getAsString()` 交出 JSON 里的字面量，正是 `mm_json::Json::Num`
///   存原文的理由）、按 `Double.parseDouble` 解析、再 `Number.intValue()`（截断向零 + 饱和）与 1 比较。
///   解析这一步复用 [`crate::check_support::number`]：它就是 Gson `getAsDouble()` 对本仓库
///   primitive 三型的口径（同一支 `Double.parseDouble`），不在此另写一份解析器。
///
/// **一处必须登记的语义差异**：Java 在「字符串既不是 true/false 又解析不成数字」时（如 `"yes"`）抛
/// `NumberFormatException`，而 `Validator.run`（`Validator.java:110-118`）**没有**任何 per-check
/// 的 try/catch，异常会一路掀掉整轮校验。Rust 的 `fn check(&self, ...)` 交不出这个异常，也不许在
/// 此处 panic：这里取「解析失败 ⇒ false」，即视为「没有开启 `ignoreLiquidFullness`」，后果是最多
/// **多报**一条 `liquidFullnessUnset` 警告，而 Java 是整轮中断。触发它需要的 JSON 本身已经不是
/// 合法的布尔写法（`"ignoreLiquidFullness": "yes"`）。
///
/// `Json::Null`/`Arr`/`Obj` 三支走不到这里：`:37` 的 `isJsonPrimitive()` 已经把非 primitive 挡在
/// `getAsBoolean()` 之外（Rust 的 `&&` 同样短路），列出仅为与 Gson 源码分支一一对应。
fn gson_as_boolean(value: &Json) -> bool {
    match value {
        Json::Bool(b) => *b,
        Json::Str(s) => {
            if s.eq_ignore_ascii_case("true") {
                return true;
            }
            if s.eq_ignore_ascii_case("false") {
                return false;
            }
            as_boolean_fallback(value)
        }
        // Json::Num：getAsString() 交出源数字文本
        Json::Num(_) => as_boolean_fallback(value),
        Json::Null | Json::Arr(_) | Json::Obj(_) => false,
    }
}

/// `Double.valueOf(text).intValue() == 1`（`JsonPrimitive.getAsBoolean()` 的兜底支）：
/// 解析复用 [`crate::check_support::number`]（它对本仓库的 `Str`/`Num` 就是
/// `Double.parseDouble(源文本)` 的口径，含 `getAsDouble()` 异常⇒`None` 的登记）。
/// 解析失败按 [`gson_as_boolean`] 头部的登记返回 false；成功则按 Java `Number.intValue()`
/// 截断向零并饱和到 `i32`（Rust `as i32` 同口径）后与 1 比较。
fn as_boolean_fallback(value: &Json) -> bool {
    check_support::number(Some(value)).unwrap_or(0.0) as i32 == 1
}

/// Java `class CrafterOutputCheck implements ProjectCheck`（`CrafterOutputCheck.java:12`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct CrafterOutputCheck;

impl ProjectCheck for CrafterOutputCheck {
    /// Java `id()`（`CrafterOutputCheck.java:14-17`）。
    fn id(&self) -> &str {
        "crafterOutputs"
    }

    /// Java `check(ValidationContext)`（`CrafterOutputCheck.java:19-47`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :20  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :21  for (ContentRecord record : project.contents) {
        for record in &project.contents {
            // :22-24  只看非内联的 block
            if record.ctype != ContentType::Block || record.is_inline() {
                continue;
            }
            // :25  String className = context.registry().resolveClass(record.type, record.className());
            let class_name: Option<String> =
                context.registry().resolve_class(record.ctype, record.class_name().as_deref());
            // :26-29  className 为 null 或不是 GenericCrafter 的后代 ⇒ 整条跳过
            let Some(class_name) = class_name.as_deref() else { continue };
            if !check_support::is_a(context.registry(), Some(class_name), "GenericCrafter") {
                continue;
            }

            // :30-35  outputItem 与 outputItems 同时存在 ⇒ outputItem 被游戏忽略
            if record.fields.contains("outputItem") && record.fields.contains("outputItems") {
                context.report(Issue::new(
                    Level::Warning,
                    "validate.crafter.outputItemIgnored",
                    &[record.name.as_str()],
                    record.id.clone(),
                ));
            }

            // :36-38  JsonElement ignoreFull = record.fields.get("ignoreLiquidFullness");
            //         boolean ignoresFullness = ignoreFull != null
            //                             && ignoreFull.isJsonPrimitive() && ignoreFull.getAsBoolean();
            let ignore_full: Option<&Json> = record.fields.get("ignoreLiquidFullness");
            let ignores_fullness: bool =
                ignore_full.is_some_and(|value| is_json_primitive(value) && gson_as_boolean(value));

            // :39-46  单数 outputLiquid 会在 init 时被复制进 outputLiquids 数组，两种拼法同义
            if (record.fields.contains("outputLiquid") || record.fields.contains("outputLiquids"))
                && !ignores_fullness
            {
                context.report(Issue::new(
                    Level::Warning,
                    "validate.crafter.liquidFullnessUnset",
                    &[record.name.as_str()],
                    record.id.clone(),
                ));
            }
        }
    }
}
