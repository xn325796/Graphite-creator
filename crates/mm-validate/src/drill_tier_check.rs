//! `DrillTierCheck.java`（`dev.modmaker.core.validate.DrillTierCheck`，78 行、2 个 `@Override`、
//! 1 个私有静态常量）的 1:1 忠实移植。
//!
//! Java 类注释（`DrillTierCheck.java:12-15`）直译：钻头挖不动自己 tier 以上的硬度，而硬度高于
//! 一切可用钻头（模组自己的最大 tier 与 vanilla 最大值）的物品，谁都永远挖不到。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. `Issue.warningOn(drill.id, key, args...)`（`:52`、`:58-59`、`:73-74`）⇒
//!    `Issue::new(Level::Warning, key, &[args], id.clone())`，因为 Java 的 `contentId` 可 null 而
//!    `warning_on` 交不出 `None`（同 `team_check.rs` 第 1 条）。
//! 2. `LinkedHashMap` ⇒ [`mm_schema::KeyedMap`]（保插入序、`put` 覆盖保位，正是
//!    `LinkedHashMap.put` 的两个观察性质）：
//!    - `items`（`:29`、`:36`）以 `record.name` 为键，**同名物品后者覆盖前者**且位置留在首次插入处，
//!      与 Java 一致；`:55`、`:70` 遍历 `items.values()` 因此按「各名字首次出现」的顺序，逐条对应。
//!    - 表里存的是记录引用（Java 的引用语义），Rust 用 `&'a ContentRecord`：`context.project()`
//!      交出的是 `&'a ModProject`（不挂在 `&self` 上，见 `validation_context.rs` 模块头），所以
//!      持有这些引用跨 `context.report(&mut)` 与 Java 的「边遍历边 report」同样成立。
//! 3. `List<ContentRecord> drills = new ArrayList<>()` + `add`（`:39`、`:46`）⇒ `Vec<&ContentRecord>`，
//!    追加顺序一致，`:49`、`:64` 两次遍历都是同一份表。
//! 4. `(int) Math.ceil(x)`（`:59`、`:74`）⇒ [`java_ceil_i32`]：Java 先 `Math.ceil`（向 +∞ 取整，
//!    NaN→NaN、±∞ 不变、-0.5→-0.0），再**窄化转换** `(int)`（JLS 5.1.3：截断向零、NaN→0、超界饱和到
//!    `Integer.MAX_VALUE`/`MIN_VALUE`）。Rust `f64::ceil()` 与 Java `Math.ceil` 逐值相同，`as i32`
//!    自 Rust 1.45 起同样是「截断向零 + NaN→0 + 饱和」，故直接 `value.ceil() as i32` 即为等价，
//!    不可写成 `x.round() as i32`（`.5` 一侧会翻）。
//!    消息参数位置是 `Object...`，Java 经 `String.valueOf(int)` 呈现 ⇒ Rust 侧 `to_string()`。
//! 5. `Math.max(bestModTier, MAX_VANILLA_DRILL_TIER)`（`:72`）：Java 把 int 常量提升到 double 后调
//!    `Math.max(double,double)`，这里用 `best_mod_tier.max(常量 as f64)`。之所以不需要 NaN 特殊口径：
//!    `best_mod_tier` 从 0 起，只在 `tier > best_mod_tier` 时更新，而 NaN 参与的 `>` 恒 false，
//!    所以它永远不会是 NaN；常量 7.0 也非 NaN。两支都安全，`f64::max` 与 `Math.max` 在此逐值相同。
//! 6. `record.type != ContentType.item`（`:31`）、`!= ContentType.block`（`:41`）⇒
//!    `record.ctype != ContentType::Item`/`::Block`；`record.className()` ⇒
//!    `ContentRecord::class_name()`（返回 `Option<String>`，Java 的可 null `String`）。
//! 7. `context.registry().resolveClass(record.type, record.className())`（`:44`）⇒
//!    `SchemaRegistry::resolve_class`；`CheckSupport.isA(registry, className, "Drill")` ⇒
//!    [`crate::check_support::is_a`]，Java 传的是非 null `String`，这里对应 `Some(&str)`。

use mm_model::{ContentRecord, ModProject};
use mm_schema::{ContentType, KeyedMap};

use crate::check_support;
use crate::issue::{Arg, Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `MAX_VANILLA_DRILL_TIER`（`DrillTierCheck.java:18-19`）：最强 vanilla 钻头的 tier
/// （`Blocks.java` 到 7 为止），模组钻头可以超过它。Java 声明为 `int`，这里同为 `i32`：
/// `:72` 的比较处按 Java 的二元提升规则显式 `as f64`，`:74` 的消息参数则保持整数呈现 "7"。
const MAX_VANILLA_DRILL_TIER: i32 = 7;

/// Java `(int) Math.ceil(x)`（`DrillTierCheck.java:59`、`:74`），口径论证见模块头第 4 条。
fn java_ceil_i32(value: f64) -> i32 {
    value.ceil() as i32
}

/// Java `class DrillTierCheck implements ProjectCheck`（`DrillTierCheck.java:16`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct DrillTierCheck;

impl ProjectCheck for DrillTierCheck {
    /// Java `id()`（`DrillTierCheck.java:21-24`）。
    fn id(&self) -> &str {
        "drillTiers"
    }

    /// Java `check(ValidationContext)`（`DrillTierCheck.java:26-77`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :27  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :28-38  Map<String, ContentRecord> items = new LinkedHashMap<>();
        //         遍历 contents，只收「非内联的 item 且 hardness > 0」
        let mut items: KeyedMap<&ContentRecord> = KeyedMap::new();
        for record in &project.contents {
            // :30-33  if (record.type != ContentType.item || record.isInline()) continue;
            if record.ctype != ContentType::Item || record.is_inline() {
                continue;
            }
            // :34  Double hardness = CheckSupport.number(record.fields.get("hardness"));
            let hardness: Option<f64> = check_support::number(record.fields.get("hardness"));
            // :35-37  if (hardness != null && hardness > 0) items.put(record.name, record);
            if hardness.is_some_and(|h| h > 0.0) {
                items.put(record.name.clone(), record);
            }
        }

        // :39-48  List<ContentRecord> drills：非内联 block 且解析后的类是 Drill 的后代
        let mut drills: Vec<&ContentRecord> = Vec::new();
        for record in &project.contents {
            // :40-43
            if record.ctype != ContentType::Block || record.is_inline() {
                continue;
            }
            // :44  String className = context.registry().resolveClass(record.type, record.className());
            let class_name: Option<String> =
                context.registry().resolve_class(record.ctype, record.class_name().as_deref());
            // :45-47  if (className != null && CheckSupport.isA(..., "Drill")) drills.add(record);
            if let Some(class_name) = class_name.as_deref()
                && check_support::is_a(context.registry(), Some(class_name), "Drill")
            {
                drills.push(record);
            }
        }

        // :49-62  每个钻头 vs 每个可挖物品
        for drill in &drills {
            // :50  Double tier = CheckSupport.number(drill.fields.get("tier"));
            let tier: Option<f64> = check_support::number(drill.fields.get("tier"));
            // :51-54  tier 缺失 ⇒ validate.drill.noTier，然后 continue
            let Some(tier) = tier else {
                context.report(Issue::new(
                    Level::Warning,
                    "validate.drill.noTier",
                    &[drill.name.as_str()],
                    drill.id.clone(),
                ));
                continue;
            };
            // :55  for (ContentRecord item : items.values())
            for item in items.values() {
                // :56  Double hardness = CheckSupport.number(item.fields.get("hardness"));
                let hardness: Option<f64> = check_support::number(item.fields.get("hardness"));
                // :57-60  hardness 存在且 > tier ⇒ tierTooLow（Java 的 `>` 对 NaN 恒 false）
                if let Some(hardness) = hardness
                    && hardness > tier
                {
                    // Java 的 `:59` 交的是 `(int) Math.ceil(tier)` 与 `(int) Math.ceil(hardness)`
                    // 两个 `int` 本身 ⇒ [`Arg::int`]，`--json` 报告里是 JSON 数字。
                    context.report(Issue::new(
                        Level::Warning,
                        "validate.drill.tierTooLow",
                        &[
                            Arg::text(drill.name.as_str()),
                            Arg::int(java_ceil_i32(tier) as i64),
                            Arg::text(item.name.as_str()),
                            Arg::int(java_ceil_i32(hardness) as i64),
                        ],
                        drill.id.clone(),
                    ));
                }
            }
        }

        // :63-69  double bestModTier = 0; for (drill : drills) { tier > bestModTier ⇒ 取它 }
        let mut best_mod_tier: f64 = 0.0;
        for drill in &drills {
            let tier: Option<f64> = check_support::number(drill.fields.get("tier"));
            if let Some(tier) = tier
                && tier > best_mod_tier
            {
                best_mod_tier = tier;
            }
        }

        // :70-76  硬度高于「模组最大 tier 与 7 的较大者」的物品，谁也别想挖
        for item in items.values() {
            // :71  Double hardness = CheckSupport.number(item.fields.get("hardness"));
            let hardness: Option<f64> = check_support::number(item.fields.get("hardness"));
            // :72  hardness != null && hardness > Math.max(bestModTier, MAX_VANILLA_DRILL_TIER)
            if let Some(hardness) = hardness
                && hardness > best_mod_tier.max(MAX_VANILLA_DRILL_TIER as f64)
            {
                // Java 的 `:74` 交的是 `(int) Math.ceil(hardness)` 与 `int` 常量
                // `MAX_VANILLA_DRILL_TIER`（`:19`）⇒ 两支都是 [`Arg::int`]。
                context.report(Issue::new(
                    Level::Warning,
                    "validate.item.unmineable",
                    &[
                        Arg::text(item.name.as_str()),
                        Arg::int(java_ceil_i32(hardness) as i64),
                        Arg::int(MAX_VANILLA_DRILL_TIER as i64),
                    ],
                    item.id.clone(),
                ));
            }
        }
    }
}
