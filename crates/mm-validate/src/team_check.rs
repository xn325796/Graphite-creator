//! `TeamCheck.java`（`dev.modmaker.core.validate.TeamCheck`，66 行、1 个 `@Override`、
//! 2 个私有静态常量）的 1:1 忠实移植。
//!
//! Java 类注释（`TeamCheck.java:11-15`）直译：
//! 一个 `content/teams` 记录必须命名一个 vanilla 队伍。当 key 缺失时解析器抛
//! "Team field missing."（`ContentParser.java:866-870`），当值既不是基础队伍名、也不落在
//! `0..255` 的 id 里时抛 "Unknown team"（`ContentParser.java:386-398`），因此两者都是硬错误：
//! 文件根本加载不了。
//!
//! Java 的 `final class TeamCheck` 是包私有，这里用 `pub struct`（取舍与 `check_support.rs`
//! 相同：`Validator` 落地前 `pub(crate)` 会触发 dead_code 警告）。`Validator` 落地后可收紧。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. `Issue.errorOn(record.id, key, args...)`（`:46`、`:50`、`:57-58`、`:61-62`）：Java 第一参
//!    `contentId` 是可 null 的 `String`，而本 crate 的 `error_on`/`warning_on` 收
//!    `impl Into<String>` 交不出 null，故一律用规范构造器
//!    `Issue::new(Level::Error, .., .., record.id.clone())`——它的末参正是 `Option<String>`，
//!    与 Java 规范构造器一一对应。用 `unwrap_or_default()` 会把 `content_id()` 从 `None` 变成
//!    `Some("")`，属静默改语义，禁止。
//! 2. `List.of(...)`（`:26-27`）⇒ [`VANILLA_TEAM_NAMES`]；`String.join(", ", list)`（`:62`）⇒
//!    `VANILLA_TEAM_NAMES.join(", ")`（与 Java `String.join` 逐字符一致）。
//! 3. `team.isJsonPrimitive()`（`:49`）⇒ [`is_json_primitive`]（Gson 的 primitive =
//!    `Bool`/`Num`/`Str` 三变体，见 `mm-json/src/value.rs:91-113` 的既有判定口径）。
//!    `team.getAsJsonPrimitive()`（`:53`）⇒ 直接借用 `team` 自身：Gson 里 `JsonElement` 与
//!    `JsonPrimitive` 在这个节点上是同一个对象，`mm_json::Json` 不区分二者，无观察差异。
//! 4. `JsonPrimitive.isNumber()`（`:54`）⇒ [`Json::is_number`]；`value.getAsInt()`（`:55`）⇒
//!    [`Json::as_i64`] 再 `as i32`（`as_i64` 是已验收的 Gson 数字取数口径，
//!    `mm-json/src/value.rs:191-201`，非本轮引入）。
//! 5. `value.getAsString()`（`:60`、`:62`）⇒ [`Json::get_as_string`]
//!    （`mm-json/src/value.rs:168-179`，注释原文即 Gson `JsonPrimitive.getAsString`）。
//!    Java 的 `isNumber()` 为真走 `getAsInt()`、为假才走 `getAsString()`，Rust 同分支。
//! 6. `record.type != ContentType.team`（`:41`）⇒ `record.ctype != ContentType::Team`
//!    （`type` 是 Rust 关键字，`mm-model/src/content_record.rs` 已登记该改名）。
//! 7. `record.id` 可 null ⇒ `Option<String>`（上面第 1 条已覆盖其后果）。
//! 8. Java 消息变参是 `Object...`，`:58` 直接交出 `int id` 与 `TEAM_IDS - 1` 两个 `int` 本身；
//!    Rust 侧用 [`Arg::int`] 承载（`issue.rs` 的 `Arg`），文案仍按 `String.valueOf(int)` 的十进制
//!    渲染，而 `--json` 报告保持 JSON 数字，与 Java 逐字节一致。

use crate::issue::{Arg, Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;
use mm_json::Json;
use mm_model::ModProject;
use mm_schema::ContentType;

/// Java `VANILLA_TEAM_NAMES`（`TeamCheck.java:19-27`）。
///
/// 字段 javadoc（`:19-25`）直译：字符串 "team" 值可以用哪些队伍名：恰好是
/// `Team.baseTeams`，构造器只为 id 小于 6 的队伍填它（`Team.java:30, 32-41, 67`）。
/// 这里写死而不是从 `dev.modmaker.core.schema.SchemaRegistry` 读，因为注册表的队伍表
/// 还带着 `neoplastic`：那支队伍 id 为 6（`Team.java:41`），只能按 id 抵达，
/// 字符串形式会被解析器抛 "Unknown team"（`ContentParser.java:388-389`）。
const VANILLA_TEAM_NAMES: [&str; 6] =
    ["derelict", "sharded", "crux", "malis", "green", "blue"];

/// Java `TEAM_IDS`（`TeamCheck.java:29-30`）：`Team.all.length`，id `0..255` 能解析，
/// 其余抛异常（`Team.java:28`、`ContentParser.java:392`）。
const TEAM_IDS: i32 = 256;

/// Gson `JsonElement.isJsonPrimitive()`（`TeamCheck.java:49`）：`Bool`/`Num`/`Str`。
/// mm_json 未导出同名方法，故在本文件内按 `mm-json/src/value.rs:91-113` 的既有判定实现。
fn is_json_primitive(value: &Json) -> bool {
    value.is_bool() || value.is_number() || value.is_string()
}

/// Java `class TeamCheck implements ProjectCheck`（`TeamCheck.java:17`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct TeamCheck;

impl ProjectCheck for TeamCheck {
    /// Java `id()`（`TeamCheck.java:32-35`）。
    fn id(&self) -> &str {
        "teams"
    }

    /// Java `check(ValidationContext)`（`TeamCheck.java:37-65`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :39  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :40  for (ContentRecord record : project.contents) {
        for record in &project.contents {
            // :41-43  if (record.type != ContentType.team || record.isInline()) continue;
            if record.ctype != ContentType::Team || record.is_inline() {
                continue;
            }

            // :44  JsonElement team = record.fields.get("team");
            let team: Option<&Json> = record.fields.get("team");

            // :45-48  if (team == null) { errorOn(...missingField); continue; }
            let Some(team) = team else {
                context.report(Issue::new(
                    Level::Error,
                    "validate.team.missingField",
                    &[record.name.as_str()],
                    record.id.clone(),
                ));
                continue;
            };

            // :49-52  if (!team.isJsonPrimitive()) { errorOn(...notPrimitive); continue; }
            if !is_json_primitive(team) {
                context.report(Issue::new(
                    Level::Error,
                    "validate.team.notPrimitive",
                    &[record.name.as_str()],
                    record.id.clone(),
                ));
                continue;
            }

            // :53  JsonPrimitive value = team.getAsJsonPrimitive();
            let value: &Json = team;

            // :54-63  if (value.isNumber()) { ... } else if (!VANILLA_TEAM_NAMES
            //             .contains(value.getAsString())) { ... }
            if value.is_number() {
                // :55-56  int id = value.getAsInt();
                let id: i32 = value.as_i64().unwrap_or_default() as i32;
                // :56-59  if (id < 0 || id >= TEAM_IDS) { ...idOutOfRange, name, id, TEAM_IDS - 1 }
                if !(0..TEAM_IDS).contains(&id) {
                    // Java 的 `:58` 交的是 `record.name`（`String`）、`id`（`int`）与
                    // `TEAM_IDS - 1`（`int`，`:30` 的 `TEAM_IDS = 256`）⇒ 后两支是 [`Arg::int`]。
                    context.report(Issue::new(
                        Level::Error,
                        "validate.team.idOutOfRange",
                        &[
                            Arg::text(record.name.as_str()),
                            Arg::int(id as i64),
                            Arg::int((TEAM_IDS - 1) as i64),
                        ],
                        record.id.clone(),
                    ));
                }
            } else {
                // :60  !VANILLA_TEAM_NAMES.contains(value.getAsString())
                let text: String = value.get_as_string();
                if !VANILLA_TEAM_NAMES.contains(&text.as_str()) {
                    // :61-62  badName, record.name, value.getAsString(), String.join(", ", names)
                    let joined: String = VANILLA_TEAM_NAMES.join(", ");
                    context.report(Issue::new(
                        Level::Error,
                        "validate.team.badName",
                        &[record.name.as_str(), text.as_str(), joined.as_str()],
                        record.id.clone(),
                    ));
                }
            }
        }
    }
}
