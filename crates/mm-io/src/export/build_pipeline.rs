//! `dev.modmaker.core.pipeline.BuildPipeline` 的等价实现（Java 源文件 119 行）。
//!
//! ## 落在 `mm_io::export` 而不是新 crate 的理由
//!
//! Java 把它放在独立包 `core.pipeline`，但它引用的每一个类型都来自 `core.io`
//! （`ProjectIo`、`ModExporter`、`ExportReport`）、`core.schema`（`SchemaRegistry`）、
//! `core.validate`（`Validator`、`Issue`）与 `core.model`（`ModProject`），自己不含任何状态。
//! 迁移方案 §P2 的映射表已把它连同 `ModExporter` / `BuildManifest` 一并归到 `mm_io::export`，
//! 这里照办；包名差异只体现在文件位置上，类名、方法名、语义逐条对齐。
//!
//! ## 与 Java 的差别（只有三处，全是所有权模型逼出来的表达差异）
//!
//! 1. **`Validator` 的构造时机**。Java 在构造器里 `new Validator(registry)` 存成字段
//!    （`BuildPipeline.java:36`）。Rust 的 [`mm_validate::Validator`] 借用了 registry
//!    （`Validator<'a>`），把两者同时装进一个结构体就是自引用，所以这里只存
//!    `&'a SchemaRegistry`，`validator()` 这个私有 helper 按需现造。`Validator` 除了
//!    registry 与 checks 之外没有任何可变状态（见 `crates/mm-validate/src/validator.rs:85-117`），
//!    因此「每次现造」与「构造一次存着」在这条链上行为完全一致。
//! 2. **`write` 多一个 `project` 参数**。Java 的 `ProjectIo.SavePlan` 内部攥着模型引用，
//!    所以 `plan.save().rollback()` 不需要别的东西（`BuildPipeline.java:107`）；Rust 的
//!    [`crate::project_io::SavePlan`] 把回滚所需的脏标记按值存了下来，但 `rollback` 要
//!    往回套就必须拿到模型本体（`crates/mm-io/src/project_io.rs:373`）。这个差异在
//!    `SavePlan` 自己的文档注释里已经登记过（`project_io.rs:299-306`），这里只是把它
//!    传导到调用点。回滚的时机与条件逐行照搬：只在 `ProjectIo::write` 失败时回滚，然后
//!    原样把错误抛给调用方。
//! 3. **`plan` 的两个重载用 `Option<Vec<Issue>>` 合成一个 `plan` + 一个 `plan_with`**。
//!    Java 靠重载区分（`:85-87` 与 `:93-97`），Rust 没有同名不同参的重载，命名沿用
//!    本仓库既有做法（`mod_exporter::build_folder` / `build_folder_root_internal`）。

use std::io;
use std::path::{Path, PathBuf};

use mm_model::ModProject;
use mm_schema::SchemaRegistry;
use mm_validate::{Issue, Validator};

use crate::export::export_report::ExportReport;
use crate::export::mod_exporter;
use crate::import::cancelled::ImportError;
use crate::project_io;

/// Java `BuildPipeline`（`core/pipeline/BuildPipeline.java:29-119`）：
/// headless CLI 与桌面端共用的一条「载入、校验、保存、导出」流程。
///
/// Java 的类文档（`:15-28`）转写如下，这段话解释了它为什么存在：
/// 从前两边各自实现过一遍这条流程——CLI 自己开工程再驱动 `Validator` + `ProjectIo` +
/// `ModExporter`，UI 的控制器又通过它的 save-plan 与 build 方法重跑一遍。于是任何改动
/// （新增一段校验、换一种保存顺序、换一种产物布局）都得改两处，而这两处会漂移。
/// pipeline 就是那条流程，两个前端只往上加真正属于自己的东西：CLI 加退出码与清单，
/// UI 加线程与覆盖确认。
///
/// 阶段是刻意拆开的：[`plan`] 是模型那一半，必须跑在持有模型的线程上（应用里就是 FX 线程）；
/// [`write`] 只碰文件系统，可以跑在工作线程上。[`build`] 把两半按顺序跑完，给那些本来就
/// 独占模型、不需要拆线程的调用方。
pub struct BuildPipeline<'a> {
    registry: &'a SchemaRegistry,
}

/// Java `BuildPipeline.Plan`（`:47-61`）：在模型还读得到的时候就定下来的一次构建——
/// 校验发现、产物从哪个目录打包、以及写入那一半必须先执行的那次保存。
pub struct Plan {
    /// Java `Plan.root()`。
    pub root: PathBuf,
    /// Java `Plan.internalName()`。
    pub internal_name: String,
    /// Java `Plan.issues()`。
    pub issues: Vec<Issue>,
    /// Java `Plan.save()`，类型是 `ProjectIo.SavePlan`。
    pub save: project_io::SavePlan,
}

impl Plan {
    /// Java `boolean hasErrors()`（`:50-52`）：`issues.stream().anyMatch(Issue::isError)`。
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(Issue::is_error)
    }

    /// Java `long errorCount()`（`:54-56`）。
    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|issue| issue.is_error()).count()
    }

    /// Java `long warningCount()`（`:58-60`）：Java 用 `!isError()` 过滤，这里同样
    /// 不是去匹配 `Level::Warning`，而是「非错误」，以保证两边口径一致。
    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|issue| !issue.is_error()).count()
    }
}

impl<'a> BuildPipeline<'a> {
    /// Java `BuildPipeline(SchemaRegistry)`（`:34-37`）。
    ///
    /// Java 在这里同时 `new Validator(registry)`；差别见模块头第 1 条。
    pub fn new(registry: &'a SchemaRegistry) -> Self {
        BuildPipeline { registry }
    }

    /// Java `SchemaRegistry registry()`（`:39-41`）。
    pub fn registry(&self) -> &'a SchemaRegistry {
        self.registry
    }

    /// Java 构造器里那句 `new Validator(registry)`。
    fn validator(&self) -> Validator<'a> {
        Validator::new(self.registry)
    }

    /// Java `ModProject open(Path root, String profile)`（`:63-70`）：
    /// 每个命令都需要的那种开法，带一个可选的 schema profile。
    ///
    /// Java 的 profile 形参可以为 null，判空用的是 `profile != null && !profile.isBlank()`；
    /// Rust 侧用 `Option<&str>` 承接 null，非 None 时仍照抄 `is_blank()` 这一道闸门
    /// （`Some(" ")` 同样不落进模型）。
    pub fn open(&self, root: &Path, profile: Option<&str>) -> Result<ModProject, ImportError> {
        let mut project = project_io::open(root, self.registry)?;
        if let Some(version) = profile
            && !version.trim().is_empty()
        {
            project.schema_profile = version.to_string();
        }
        Ok(project)
    }

    /// Java `List<Issue> validate(ModProject project)`（`:72-79`）：把整套规则跑过一个已打开的工程。
    ///
    /// Java 的注释写明它是纯读，但对编辑不是线程安全的：要么跑在模型所属的线程上，
    /// 要么跑在快照上（见 `ModProject::snapshot`，`crates/mm-model/src/mod_project.rs:115`）。
    pub fn validate(&self, project: &ModProject) -> Vec<Issue> {
        self.validator().validate(project)
    }

    /// Java `Plan plan(ModProject project)`（`:81-87`）：构建的模型那一半，自己跑校验。
    pub fn plan(&self, project: &mut ModProject) -> io::Result<Plan> {
        let issues = self.validator().validate(project);
        self.plan_with(project, issues)
    }

    /// Java `Plan plan(ModProject project, List<Issue> issues)`（`:89-97`）：
    /// 同上，但用调用方已经算好的发现（UI 在 FX 线程之外校验快照，所以这一趟不必重跑）。
    ///
    /// Java 两处都套了 `List.copyOf(issues)`——那是「交出一个不可变副本」，Rust 的入参
    /// 已是 owned `Vec`，直接 move 进 `Plan` 即可，语义等价（`Plan.issues` 不对外暴露 `&mut`）。
    ///
    /// **必须跑在持有模型的线程上**，与 [`crate::project_io::plan`] 同样。
    pub fn plan_with(&self, project: &mut ModProject, issues: Vec<Issue>) -> io::Result<Plan> {
        let root = mod_exporter::require_root(project)?;
        let internal = mod_exporter::require_internal_name(project)?;
        let save = project_io::plan(project, &root)?;
        Ok(Plan { root, internal_name: internal, issues, save })
    }

    /// Java `ExportReport write(Plan plan, Path destination, boolean zip)`（`:99-113`）：
    /// 构建的磁盘那一半——先保存工程，再打包产物。保存失败会把未保存标记滚回去，
    /// 于是一次写了一半的保存永远不会看起来像已持久化。
    ///
    /// 差别见模块头第 2 条：Rust 的 `rollback` 需要模型本体，所以多收一个 `project`。
    pub fn write(
        &self,
        project: &mut ModProject,
        plan: &Plan,
        destination: &Path,
        zip: bool,
    ) -> io::Result<ExportReport> {
        if let Err(failure) = project_io::write(&plan.save) {
            plan.save.rollback(project);
            return Err(failure);
        }
        if zip {
            mod_exporter::build_zip_root_internal(
                &plan.root,
                &plan.internal_name,
                destination,
                Some(&plan.issues),
            )
        } else {
            mod_exporter::build_folder_root_internal(
                &plan.root,
                &plan.internal_name,
                destination,
                Some(&plan.issues),
            )
        }
    }

    /// Java `ExportReport build(ModProject project, Path destination, boolean zip)`（`:115-118`）：
    /// 不拆阶段直接构建，给那些已经独占模型的调用方。
    pub fn build(
        &self,
        project: &mut ModProject,
        destination: &Path,
        zip: bool,
    ) -> io::Result<ExportReport> {
        let plan = self.plan(project)?;
        self.write(project, &plan, destination, zip)
    }
}
