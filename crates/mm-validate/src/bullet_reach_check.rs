//! `BulletReachCheck.java`（`dev.modmaker.core.validate.BulletReachCheck`，44 行、2 个
//! `@Override`）的 1:1 忠实移植。
//!
//! Java 类注释（`BulletReachCheck.java:7-10`）直译：炮塔的子弹必须够得着炮塔声明的射程——子弹
//! 射程 = speed × lifetime（像素），这对数值偏小就意味着射程圈边缘的目标永远打不到。
//!
//! Java 的 `final class BulletReachCheck` 是包私有，这里用 `pub struct`（取舍与 `check_support.rs`
//! 相同：`Validator` 落地前 `pub(crate)` 会触发 dead_code 警告），`Validator` 落地后收紧。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. `Issue.warningOn(record.id, key, args...)`（`:34-37`、`:39-42`）：Java 第一参 `contentId`
//!    是可 null 的 `String`，而本 crate 的 `warning_on` 收 `impl Into<String>` 交不出 null，故用
//!    规范构造器 `Issue::new(Level::Warning, .., .., record.id.clone())`（末参正是
//!    `Option<String>`）。理由与 `team_check.rs` 第 1 条相同：`unwrap_or_default()` 会把 `None`
//!    变成 `Some("")`，属静默改语义。
//! 2. Java 的两个分支（`:33` `if`、`:38` `else if`）构造的 Issue **形状完全相同**（同 level、同
//!    参数表、同顺序），只有键名不同，因此这里把键名收成一次 `if / else if / continue`，再走同一条
//!    `context.report(...)`。上报顺序与 Java 逐条一致：同一条记录只可能命中其一（`reach < range*0.9`
//!    与 `reach > range*3` 在 `range > 0` 时互斥）。
//! 3. `Math.round(reach / 8)`（`:36`、`:41`）⇒ [`java_round_double`]：Java 的
//!    `Math.round(double)` 定义为 `(long) Math.floor(a + 0.5d)`，返回 `long`，**不是** Rust 的
//!    `f64::round()`（后者远离零取整、且不做饱和转换）。三个边界因此必须靠 Java 定义复刻：
//!    `+0.5` 后 `floor`（`.floor()`）、再饱和窄化（Rust `as i64` 与 Java `(long)` 同样是
//!    NaN→0、+∞→`i64::MAX`、-∞→`i64::MIN`，自 Rust 1.45 起保证）。`.5` 落在「向 +∞ 取整」一侧，
//!    故 `Math.round(2.5)=3` 而 Rust `2.5f64.round()=3` 只是巧合，`-2.5` 两边分别是 `-2` 与 `-3`。
//!    消息参数位置是 `Object...`，Java 交的是那个 `long` **本身**（不是先 `String.valueOf` 再交），
//!    ⇒ Rust 侧 [`Arg::int`]，`--json` 报告里是 JSON 数字；只有在渲染文案时才经
//!    `String.valueOf(long)` 变成十进制串（见 `issue.rs` 的 `Arg` 文档）。
//! 4. `record.type != ContentType.bullet`（`:21`）⇒ `record.ctype != ContentType::Bullet`；
//!    消息参数 `host.type`（`:35`、`:40`）是枚举本身，Java 经 `String.valueOf(Object)` →
//!    默认 `toString()` → 枚举常量名（小写，见 `team_check.rs` 同类说明）⇒ `host.ctype.name()`。
//! 5. `project.contentById(record.inlineOwnerId)`（`:24`）⇒ `record.inline_owner_id.as_deref()
//!    .and_then(|id| project.content_by_id(id))`：Java 的 `contentById(String)` 收到 null 时恒返回
//!    null（`ContentRecord.id` 可 null ⇒ `inlineOwnerId` 可 null），Rust 的 `content_by_id` 只收
//!    `&str`，用 `as_deref()` 把 null 传播出来即为等价。
//! 6. `CheckSupport.number(...)` ⇒ 复用 [`crate::check_support::number`]，`Double` ↔ `Option<f64>`；
//!    `CheckSupport.trim(...)` ⇒ 复用 [`crate::check_support::trim`]。
//! 7. Java 的判空守卫 `:29`（`range == null || speed == null || lifetime == null || range <= 0`）
//!    把 `host == null`（`:23` 三元的 null 支）一并覆盖：host 为 null ⇒ range 为 null ⇒ continue，
//!    于是 `:35`/`:40` 处 `host.name`、`host.type` 必定非 null。Rust 用一条
//!    `let (Some(host), Some(range), Some(speed), Some(lifetime)) = ... else { continue }`
//!    同时表达这四件事，条件顺序与短路点一致。

use mm_model::{ContentRecord, ModProject};
use mm_schema::ContentType;

use crate::check_support;
use crate::issue::{Arg, Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `Math.round(double)`（`BulletReachCheck.java:36`、`:41` 里的 `Math.round(reach / 8)`）。
///
/// 定义照 `java.lang.Math`：`return (long)Math.floor(a + 0.5d);`。NaN 与 ±∞ 的去向由 Java 的
/// 窄化转换决定，Rust 的 `as i64` 饱和口径与之逐值相同（见模块头第 3 条）。
fn java_round_double(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// Java `class BulletReachCheck implements ProjectCheck`（`BulletReachCheck.java:12`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct BulletReachCheck;

impl ProjectCheck for BulletReachCheck {
    /// Java `id()`（`BulletReachCheck.java:14-17`）。
    fn id(&self) -> &str {
        "bulletReach"
    }

    /// Java `check(ValidationContext)`（`BulletReachCheck.java:19-43`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :20  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :21  for (ContentRecord record : project.contents) {
        for record in &project.contents {
            // :22-24  只管内联子弹（文件型的 bullet 不存在，见 ContentType::file_backed）
            if record.ctype != ContentType::Bullet || !record.is_inline() {
                continue;
            }

            // :25  ContentRecord host = project.contentById(record.inlineOwnerId);
            let host: Option<&ContentRecord> = record
                .inline_owner_id
                .as_deref()
                .and_then(|id| project.content_by_id(id));

            // :26  Double range = host == null ? null : CheckSupport.number(host.fields.get("range"));
            let range: Option<f64> = match host {
                Some(host) => check_support::number(host.fields.get("range")),
                None => None,
            };
            // :27  Double speed = CheckSupport.number(record.fields.get("speed"));
            let speed: Option<f64> = check_support::number(record.fields.get("speed"));
            // :28  Double lifetime = CheckSupport.number(record.fields.get("lifetime"));
            let lifetime: Option<f64> = check_support::number(record.fields.get("lifetime"));

            // :29-31  if (range == null || speed == null || lifetime == null || range <= 0) continue;
            // （`host == null` 已被 `range == null` 蕴含，见模块头第 7 条）
            let (Some(host), Some(range), Some(speed), Some(lifetime)) =
                (host, range, speed, lifetime)
            else {
                continue;
            };

            // :32  double reach = speed * lifetime;
            let reach: f64 = speed * lifetime;

            // :33  if (reach < range * 0.9) / :38  else if (reach > range * 3)
            // 两支只键名不同，其余形状一致（模块头第 2 条）。
            let key: &str = if reach < range * 0.9 {
                "validate.bullet.reachShort"
            } else if reach > range * 3.0 {
                "validate.bullet.reachLong"
            } else {
                continue;
            };

            // :34-37 / :39-42  record.name, host.type, host.name,
            //                  Math.round(reach / 8), CheckSupport.trim(reach), CheckSupport.trim(range)
            let tiles: i64 = java_round_double(reach / 8.0);
            let reach_text: String = check_support::trim(reach);
            let range_text: String = check_support::trim(range);
            context.report(Issue::new(
                Level::Warning,
                key,
                &[
                    Arg::text(record.name.as_str()),
                    Arg::text(host.ctype.name()),
                    Arg::text(host.name.as_str()),
                    Arg::int(tiles),
                    Arg::text(reach_text),
                    Arg::text(range_text),
                ],
                record.id.clone(),
            ));
        }
    }
}
