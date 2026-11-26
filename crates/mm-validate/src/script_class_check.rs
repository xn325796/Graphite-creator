//! 新规则（2026-11-26 产品评审 R-02 授权的新设计，Java 侧没有对应类）：方块既没有声明 `type`，
//! 脚本里也没有 `extend` 过它的名字——游戏会按 `Block` 加载，字段集多半不对。
//!
//! ## 为什么是 warning 而不是 error
//!
//! 游戏**能**加载这样的方块：`Block` 是合法类，文件不会失败。错的是字段集——作者写了
//! `payload`、`consumes` 一类只属于子类的键，游戏按 `Block` 解析时根本不看（`ContentParser`
//! 忽略未知键，只记日志）。这正是 Java 校验器的 warning 档要表达的东西（`Validator.java:12-33`
//! 的类注释：只有会让内容加载失败才是 error）。
//!
//! ## 与 `records`（`RecordCheck`）的边界
//!
//! `RecordCheck.validateClass`（`RecordCheck.java:102-108`）只在**该类型没有默认类**时对缺
//! `type` 报 error（weather），并且只看 JSON；本规则只补「方块有默认类、但真实类写在脚本里」
//! 这一档，所以两条规则不会重复报同一件事：
//!
//! - 显式 `type` → 本规则不看（`records` 管类名是否认识）；
//! - 无 `type` 但有脚本 `extend` 命中 → 本规则不报（`mm-app` 会按推断出的类显示字段集）；
//! - 无 `type` 且脚本里也没有 → warning（本规则唯一的出路）。
//!
//! ## 覆盖边界的如实登记
//!
//! 脚本扫描是静态的（`mm_io::script_scan` 模块头第 2 条）：`extend(GenericCrafter, name, …)`
//! 这类**变量名**给不出内容名，会被本规则报出来。那是「工具推断不出」而不是「游戏加载错」，
//! 所以是 warning；作者在 JSON 里补一个 `type` 即可消掉。

use mm_model::ModProject;
use mm_schema::ContentType;

use crate::issue::{Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// 规则本体。无状态，可 `Copy`。
#[derive(Debug, Clone, Copy, Default)]
pub struct ScriptClassCheck;

impl ProjectCheck for ScriptClassCheck {
    /// 稳定短 id。与其它 check 一样是 `default_checks()` 里选择运行的键。
    fn id(&self) -> &str {
        "scriptClass"
    }

    /// 规则只看记录名与 `script_classes`，不碰内部名前缀，故不参与内部名闸门。
    fn requires_internal_name(&self) -> bool {
        false
    }

    fn check(&self, context: &mut ValidationContext<'_>) {
        let project: &ModProject = context.project();
        for record in &project.contents {
            // 只看会成为新的方块文件的记录：补丁是改既有内容（不需要 `type`），内联块跟着宿主走。
            if record.ctype != ContentType::Block || record.patch || record.is_inline() {
                continue;
            }
            // 显式 `type`（含非空白字符串）：交给 `records` 那条规则，这里不重复。
            if record
                .class_name()
                .as_deref()
                .is_some_and(|declared| !declared.trim().is_empty())
            {
                continue;
            }
            // 脚本里 `extend(<Class>, '<这个名字>')` 命中：有效类有了来源，不报。
            if project.script_classes.contains_key(&record.name) {
                continue;
            }
            context.report(Issue::new(
                Level::Warning,
                "validate.class.noScriptClass",
                &[record.name.as_str()],
                record.id.clone(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_model::ContentRecord;
    use mm_schema::SchemaRegistry;
    use std::path::Path;

    fn registry() -> SchemaRegistry {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        SchemaRegistry::load(&dir).expect("../../schemas 必须在仓库里")
    }

    fn block(name: &str, declared: Option<&str>) -> ContentRecord {
        let mut record = ContentRecord {
            ctype: ContentType::Block,
            name: name.to_string(),
            ..Default::default()
        };
        if let Some(declared) = declared {
            record.set_class_name(Some(declared));
        }
        record
    }

    fn run(contents: Vec<ContentRecord>, scripts: &[(&str, &str)]) -> Vec<Issue> {
        let registry = registry();
        let mut project = ModProject::default();
        project.meta.name = "script-check".to_string();
        project.contents = contents;
        for (name, class) in scripts {
            project
                .script_classes
                .insert((*name).to_string(), (*class).to_string());
        }
        let mut context = ValidationContext::new(&project, &registry);
        ScriptClassCheck.check(&mut context);
        context.issues().to_vec()
    }

    /// 反例：没有 type、脚本里也没有 ⇒ 一条 warning，消息带方块名。
    #[test]
    fn block_without_type_and_without_script_warns() {
        let issues = run(vec![block("没人认领的方块", None)], &[]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].level(), Level::Warning);
        assert_eq!(issues[0].key(), "validate.class.noScriptClass");
        assert_eq!(
            issues[0].message(),
            "block '没人认领的方块' declares no \"type\" and no script extends it with that name; \
             the game loads it as Block, so subclass fields may be missing"
        );
    }

    /// 正例：脚本 `extend` 命中 ⇒ 不报（这正是 R-02 要修的那批方块）。
    #[test]
    fn script_extended_block_is_quiet() {
        let issues = run(
            vec![block("载荷发射台", None)],
            &[("载荷发射台", "PayloadBlock")],
        );
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// 正例：显式 type ⇒ 不报（类名是否认识归 `records` 管）。
    #[test]
    fn declared_type_is_quiet() {
        let issues = run(vec![block("铱装卸器", Some("unloader"))], &[]);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// 空白 type 与没写 type 同义：仍然要报。
    #[test]
    fn blank_declared_type_still_warns() {
        let issues = run(vec![block("空串声明", Some("   "))], &[]);
        assert_eq!(issues.len(), 1);
    }

    /// 补丁与内联方块不参与（它们不产生新的方块文件）。
    #[test]
    fn patches_and_inline_blocks_are_skipped() {
        let mut patch = block("补丁方块", None);
        patch.patch = true;
        let mut inline = block("内联方块", None);
        inline.inline_owner_id = Some("owner".to_string());
        let issues = run(vec![patch, inline], &[]);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// 只盯方块：同样没有 type 的 item 不归这条规则管。
    #[test]
    fn non_block_records_are_ignored() {
        let mut item = block("某种材料", None);
        item.ctype = ContentType::Item;
        let issues = run(vec![item], &[]);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
