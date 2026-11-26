//! `Validator.java`（120 行）的等价移植：规则编排器——按固定顺序跑完所有规则，只做聚合。
//!
//! 类注释（`Validator.java:12-33`）直译：
//! 按游戏加载器真正会执行的规则检查工程，再加上那些在游戏里静默、但确实写错的东西（缺贴图、
//! 悬空引用、bundle 里有键却没有对应内容）。
//!
//! 这些规则曾经挤在一个上千行的 `validate()` 里、外加十几个私有 helper——于是每加一条规则都要改
//! 主流程，调用方也没法只要其中一族发现。现在它们是彼此独立的 [`ProjectCheck`]，由本类按固定顺序
//! 运行；check 才是扩展点，本类只做聚合。
//!
//! 刻意保守：只有会让内容加载失败的东西才是 error。玩法层面的坑（子弹射程、钻头层级、配方容量……）
//! 来自 Obsidian 笔记「Mindustry-JSON键值工程指南」里的模组教程提炼，保持 warning 级：游戏容忍它们，
//! 玩家只是看得到后果。
//!
//! JSON 里的未知字段刻意不检查，而这段注释过去声称它们在这里是 warning、实际从来不是。游戏只对这些
//! 发警告——`ignoreUnknownFields` 为 true（ContentParser.java:58），未知键会被记一条日志后跳过
//! （ContentParser.java:1277-1280）——而 schema 没列出的键，更可能是生成的字段数据有洞，而不是作者
//! 写错，全都报警会把真问题埋进噪音里。它们被挪到能判断的地方呈现：检视器在 raw-JSON 区域里展示记录
//! 完整的字段树，不管 schema 认不认识那个键。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. **构造器重载** `Validator(SchemaRegistry)`（`:39-41`）/ `Validator(SchemaRegistry,
//!    List<ProjectCheck>)`（`:44-47`）⇒ [`Validator::new`] / [`Validator::with_checks`]：Rust 没有
//!    重载，Java 的「单参构造器委托给双参构造器」在这里就是 `new` 调 `with_checks(registry,
//!    default_checks())`，委托方向与 `:40` 逐字相同。
//! 2. **方法重载** `validate(ModProject)`（`:80-82`）/ `validate(ModProject, Set<String>)`
//!    （`:90-108`）⇒ [`Validator::validate`] / [`Validator::validate_only`]，同名合并的歧义按
//!    参数形状命名，行为逐行照搬。
//! 3. `List<ProjectCheck>`（`:37`、`:46`、`:55`、`:75`）⇒ `Vec<Box<dyn ProjectCheck>>`：`ProjectCheck`
//!    trait 是对象安全的（`project_check.rs` 模块头第 1 条已论证）。Java `List.copyOf(checks)`
//!    （`:46`）是**不可变快照**，Rust 收下调用方交来的 owning `Vec` 即为同一效果（本类之后只读、
//!    不再有人持有原列表的可变引用）；`checks()`（`:75-77`）交 `&[Box<dyn ProjectCheck>]` 只读切片。
//! 4. `Set<String> onlyIds`（`:90`）⇒ [`Option`]`<&[`&str`]`>`：Java 允许传 `null`（`:91` 的
//!    `onlyIds == null` ⇒ 跑全部），`Option` 正是这份 null。成员判定 `onlyIds.contains(check.id())`
//!    （`:98`）是线性查找，与 `Set` 的哈希查找在结果上等价（元素都是普通字符串，无自定义 equals）。
//!    **登记一处不可复现的差别**：`:102` 的 `unknown` 列表顺序取自 `onlyIds` 的迭代顺序，而 Java
//!    常见的调用写法 `Set.of(...)`（`:10` import 的 `java.util.Set`）是**无序**集合——同一条异常消息
//!    在不同进程里可能换序。Rust 按调用方给出的切片顺序输出，是 Java 的一种合法迭代序，属**收敛**。
//! 5. `throw new IllegalArgumentException("unknown validation check(s): " + unknown + "; known: " +
//!    known)`（`:104-105`）⇒ [`UnknownChecksError`] 携带**逐字符相同**的消息文本：`unknown` 是
//!    `List<String>`、`known` 是 `LinkedHashSet<String>`，两者都经 `AbstractCollection.toString()`
//!    打成 `[a, b]` ⇒ [`java_collection_to_string`]（口径与 `record_check.rs` 模块头第 8 条一致）。
//!    `known` 的顺序由 `:96-101` 的循环按 checks 顺序插入 `LinkedHashSet` 决定，与
//!    `defaultChecks()` 顺序一致。
//!    **2026-11-25 加固决策**：这里原先按「不受检异常直达顶层」映射成 `panic!`，但 check id
//!    来自 CLI/用户配置，属用户输入可达路径——panic 会直接杀掉进程，坏输入不该有这种杀伤力。
//!    改为 `Result` 显式传播（与 `mm_io::import::cancelled` 模块头「panic 不作业务控制流」的
//!    既有口径一致），消息文本不变；测试侧对应 Java `assertThrows` 的写法是断言 `Err` 的
//!    `Display`。
//! 6. `private List<Issue> run(ModProject, Collection<ProjectCheck>)`（`:110-119`）的参数从
//!    `Collection<ProjectCheck>` ⇒ `&[&dyn ProjectCheck]`：两个调用方（`:81` 交 `checks`、`:107` 交
//!    `selected`）都只需按序迭代，Java 也从不调用集合的写方法；把「选中的规则」压成切片引用可以
//!    避免 `Box` 克隆，观察效果一致。返回值 `List.copyOf(context.issues())`（`:118`）⇒
//!    `to_vec()`（[`Issue`] 是 `Clone`，快照语义相同）。
//! 7. `project.internalName().isEmpty()`（`:113`）⇒ `project.internal_name().is_empty()`：Java 的
//!    `internalName()` 在此恒非 null（`ModProject.java` 里它是构造期就定下的 `String`），空串判定
//!    两边同义。
//! 8. `public static List<ProjectCheck> defaultChecks()`（`:49-73`）⇒ 本模块的**自由函数**
//!    [`default_checks`]（论证见其文档）；`new`（`:40`）与外部调用方取到的都是同一份顺序
//!    （Java 的 16 项 + 2026-11-26 的 `scriptClass`）。

use std::fmt;

use mm_model::ModProject;
use mm_schema::SchemaRegistry;

use crate::board_check::BoardCheck;
use crate::bullet_reach_check::BulletReachCheck;
use crate::content_order_check::ContentOrderCheck;
use crate::crafter_output_check::CrafterOutputCheck;
use crate::drill_tier_check::DrillTierCheck;
use crate::encoding_check::EncodingCheck;
use crate::environment_sprite_check::EnvironmentSpriteCheck;
use crate::issue::Issue;
use crate::meta_check::MetaCheck;
use crate::min_game_version_check::MinGameVersionCheck;
use crate::planet_check::PlanetCheck;
use crate::project_check::ProjectCheck;
use crate::record_check::RecordCheck;
use crate::script_class_check::ScriptClassCheck;
use crate::reference_check::ReferenceCheck;
use crate::recipe_capacity_check::RecipeCapacityCheck;
use crate::sound_reference_check::SoundReferenceCheck;
use crate::strings_check::StringsCheck;
use crate::team_check::TeamCheck;
use crate::validation_context::ValidationContext;

/// Java `AbstractCollection.toString()`（`Validator.java:104-105` 的 `unknown` 与 `known`）。
fn java_collection_to_string(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

/// `validate_only` 收到未知 check id 时的明确失败（Java `IllegalArgumentException` 的等价物）。
///
/// 消息文本与 Java 逐字符相同；决策理由（panic → Result）见模块头第 5 条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownChecksError {
    /// 调用方点名、但 checks 里不存在的 id（按调用方给出的顺序）。
    pub unknown: Vec<String>,
    /// 全量已知 id（按 checks 的规范顺序，保序去重）。
    pub known: Vec<String>,
}

impl fmt::Display for UnknownChecksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown validation check(s): {}; known: {}",
            java_collection_to_string(&self.unknown),
            java_collection_to_string(&self.known)
        )
    }
}

impl std::error::Error for UnknownChecksError {}

/// Java `public final class Validator`（`Validator.java:34`）。
pub struct Validator<'a> {
    /// Java `private final SchemaRegistry registry`（`:36`）。
    registry: &'a SchemaRegistry,
    /// Java `private final List<ProjectCheck> checks`（`:37`）。
    checks: Vec<Box<dyn ProjectCheck>>,
}

impl<'a> Validator<'a> {
    /// Java `public Validator(SchemaRegistry)`（`Validator.java:39-41`）。
    pub fn new(registry: &'a SchemaRegistry) -> Self {
        // :40  this(registry, defaultChecks());
        Self::with_checks(registry, default_checks())
    }

    /// Java `public Validator(SchemaRegistry, List<ProjectCheck>)`（`Validator.java:43-47`）。
    ///
    /// 方法注释（`:43`）直译：在一组显式给定的规则之上建一个校验器，按给定的顺序运行。
    pub fn with_checks(registry: &'a SchemaRegistry, checks: Vec<Box<dyn ProjectCheck>>) -> Self {
        Self {
            registry,
            checks,
        }
    }

    /// Java `public List<ProjectCheck> checks()`（`Validator.java:75-77`）。
    pub fn checks(&self) -> &[Box<dyn ProjectCheck>] {
        &self.checks
    }

    /// Java `public List<Issue> validate(ModProject)`（`Validator.java:79-82`）。
    ///
    /// 方法注释（`:79`）直译：对工程跑完所有规则。纯读取；任何一条规则都不得改动 project。
    pub fn validate(&self, project: &ModProject) -> Vec<Issue> {
        // :81  return run(project, checks);
        let all: Vec<&dyn ProjectCheck> = self.checks.iter().map(|check| check.as_ref()).collect();
        self.run(project, &all)
    }

    /// Java `public List<Issue> validate(ModProject, Set<String>)`（`Validator.java:84-108`）。
    ///
    /// 方法注释（`:84-89`）直译：只跑被点名的那些规则，并保持它们的规范顺序。用在「场景知道自己哪
    /// 一族发现可能变了」的地方——改名之后，只有引用扫描值得重跑。
    ///
    /// `@throws IllegalArgumentException for an unknown check id`：见模块头第 5 条，
    /// Rust 以 [`UnknownChecksError`] 承载同一条消息（2026-11-25 起不再 panic）。
    pub fn validate_only(
        &self,
        project: &ModProject,
        only_ids: Option<&[&str]>,
    ) -> Result<Vec<Issue>, UnknownChecksError> {
        // :91-93  null 或空集 ⇒ 跑全部
        let Some(only_ids) = only_ids else {
            return Ok(self.validate(project));
        };
        if only_ids.is_empty() {
            return Ok(self.validate(project));
        }

        // :94-101  selected 按 checks 的规范顺序收集；known 记全量 id（保序去重）
        let mut selected: Vec<&dyn ProjectCheck> = Vec::new();
        let mut known: Vec<String> = Vec::new();
        for check in &self.checks {
            let id: &str = check.id();
            if !known.iter().any(|existing| existing == id) {
                known.push(id.to_string());
            }
            if only_ids.contains(&id) {
                selected.push(check.as_ref());
            }
        }

        // :102-106  未知的 id 一律拒绝，消息里带上未知项与已知项
        let unknown: Vec<String> = only_ids
            .iter()
            .filter(|id| !known.iter().any(|existing| existing == *id))
            .map(|id| id.to_string())
            .collect();
        if !unknown.is_empty() {
            return Err(UnknownChecksError { unknown, known });
        }

        // :107
        Ok(self.run(project, &selected))
    }

    /// Java `private List<Issue> run(ModProject, Collection<ProjectCheck>)`
    /// （`Validator.java:110-119`）。
    fn run(&self, project: &ModProject, selected: &[&dyn ProjectCheck]) -> Vec<Issue> {
        // :111  ValidationContext context = new ValidationContext(project, registry);
        let mut context: ValidationContext<'_> = ValidationContext::new(project, self.registry);

        // :113-115  内部名为空的工程整条跳过需要它的规则。
        // 空判定提到循环外：每条规则各现算一次 String 是白分配（判定结果一轮内不变）。
        let internal_name_empty = project.internal_name().is_empty();
        // :112-117
        for check in selected {
            if check.requires_internal_name() && internal_name_empty {
                continue;
            }
            check.check(&mut context);
        }

        // :118  return List.copyOf(context.issues());
        context.issues().to_vec()
    }
}

/// Java `public static List<ProjectCheck> defaultChecks()`（`Validator.java:49-73`）。
///
/// 方法注释（`:49-54`）直译：按运行顺序排列的内置规则。meta、minGameVersion 闸门、encoding 排在
/// 最前：名字决定记录还能不能被寻址，版本闸门决定游戏到底加不加载这个包。逐记录规则必须排在引用
/// 扫描之前，这样一个坏记录会在它的引用之前被报出来。
///
/// 模块头第 8 条：Java 是挂在类上的 `public static`，Rust 这里是自由函数——放进
/// `impl<'a> Validator<'a>` 会让外部调用方（如移植后的 `ValidatorTest`）无法推断那个与本函数无关的
/// `'a`。`List.of(...)`（`:56-72`）的 16 项顺序逐项照搬，一项不换；`scriptClass` 追加在
/// `records` 之后（2026-11-26，产品评审 R-02）——它与逐记录检查同族，排在引用扫描之前。
pub fn default_checks() -> Vec<Box<dyn ProjectCheck>> {
    vec![
        Box::new(MetaCheck),
        Box::new(MinGameVersionCheck),
        Box::new(EncodingCheck),
        Box::new(RecordCheck),
        // 2026-11-26（R-02）：Java 没有这一条，追加在 records 之后（见本函数文档）。
        Box::new(ScriptClassCheck),
        Box::new(ReferenceCheck),
        Box::new(ContentOrderCheck),
        Box::new(StringsCheck),
        Box::new(TeamCheck),
        Box::new(BoardCheck),
        Box::new(BulletReachCheck),
        Box::new(DrillTierCheck),
        Box::new(EnvironmentSpriteCheck),
        Box::new(CrafterOutputCheck),
        Box::new(RecipeCapacityCheck),
        Box::new(SoundReferenceCheck),
        Box::new(PlanetCheck),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 与 mm-app 集成测试同一口径：schemas 就在仓库里，离线可加载。
    fn registry() -> SchemaRegistry {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        SchemaRegistry::load(&dir).expect("../../schemas 必须在仓库里")
    }

    /// 加固回归（2026-11-25）：未知 check id 必须得到错误而非 panic，
    /// 消息文本与 Java `IllegalArgumentException` 逐字符相同（模块头第 5 条）。
    #[test]
    fn unknown_check_id_is_an_error_with_the_java_message() {
        let registry = registry();
        let validator = Validator::new(&registry);
        let project = ModProject::default();
        let err = validator
            .validate_only(&project, Some(&["meta", "nope"]))
            .expect_err("nope 不是已知的 check id");
        assert_eq!(err.unknown, ["nope".to_string()]);
        assert_eq!(err.known.len(), 17);
        assert_eq!(
            err.to_string(),
            "unknown validation check(s): [nope]; known: [meta, minGameVersion, encoding, \
             records, scriptClass, references, contentOrder, strings, teams, boards, bulletReach, \
             drillTiers, environmentSprites, crafterOutputs, recipeCapacity, sounds, planets]"
        );
    }

    /// 点名的都是已知 id 时照常运行，且只跑被点名的那族规则。
    #[test]
    fn known_subset_runs_only_the_named_checks() {
        let registry = registry();
        let validator = Validator::new(&registry);
        let mut project = ModProject::default();
        project.meta.name = "subset-mod".to_string();
        project.meta_file = "mod.hjson".to_string();
        let issues = validator
            .validate_only(&project, Some(&["meta"]))
            .expect("meta 是已知 id");
        assert!(
            issues.iter().all(|issue| issue.key().starts_with("validate.meta.")),
            "只点名 meta 时不得混入其它规则的发现：{issues:?}"
        );
    }

    /// None / 空集 ⇒ 跑全部，与 `validate` 的结果完全一致（`:91-93` 的口径不变）。
    #[test]
    fn none_or_empty_still_runs_everything() {
        let registry = registry();
        let validator = Validator::new(&registry);
        let project = ModProject::default();
        let all = validator.validate(&project);
        assert_eq!(validator.validate_only(&project, None).expect("None ⇒ 全部"), all);
        assert_eq!(
            validator.validate_only(&project, Some(&[])).expect("空集 ⇒ 全部"),
            all
        );
    }
}
