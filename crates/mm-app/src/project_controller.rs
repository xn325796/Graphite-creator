//! `ProjectController` 的等价移植（Java `dev.modmaker.ui.ProjectController`，2382 行）。
//!
//! 对应 Java：`ProjectController.java:3-56`（类注释与门面定位）。Java 把「视图订阅 + 项目生命周期 +
//! 编辑命令 + 通知扇出」收在这一类里，撤销历史在 [`UndoManager`](crate::undo_manager::UndoManager)、
//! 选中项在 [`SelectionModel`](crate::selection_model::SelectionModel)、校验缓存在
//! [`ValidationSession`](crate::validation_session::ValidationSession)（Java
//! `dev.modmaker.ui.ValidationSession`，248 行）、
//! 读写与构建在 `mm_io` / `mm_validate`。
//!
//! ## 与 Java 的两处结构性差异（Java 用闭包与对象同一性，Rust 两样都没有）
//!
//! 1. **命令是数据，不是闭包**。Java `UndoManager.Command` 的匿名实现捕获 `project` / `board` /
//!    `registry`。Rust 侧命令只存 id 与快照，执行时由 [`EditState`] 现场借出这三样，
//!    见 [`crate::undo_manager`] 模块头的同款说明。
//! 2. **没有对象同一性**。Java 到处 `record == other.record`、`contents.remove(record)`、
//!    `nodes.contains(node)`、`board.edges.contains(edge)`，靠引用相等。Rust 侧 `ModProject` 独占
//!    `Vec<ContentRecord>`，命令与脏标记一律改按 `id` 定位（`ContentRecord::id`、`CanvasNode::id`、
//!    `CanvasEdge::id` 都由 `Ids.uid` 生成，唯一性就是 Java 依赖的那个性质）。
//!    id 查不到时的处理见每个 `restore_*` 里的注释——那对应 Java 里「引用已从集合摘掉」的状态。

use std::error::Error;
use std::path::Path;

use mm_fmt::refs::RewriteResult;
use mm_fmt::{ids, planet};
use mm_io::canvas::auto_layout;
use mm_io::canvas::link_graph::{self, Diff};
use mm_io::export::build_pipeline::Plan;
use mm_io::export::export_report::ExportReport;
use mm_io::import::ImportProgress;
use mm_io::project_factory;
use mm_io::project_io::{self, SavePlan};
use mm_json::{escape_segment, leaf_name, Json, Object};
use mm_model::{Board, CanvasEdge, CanvasNode, ContentRecord, ModProject};
use mm_schema::{name_rules, ContentType, FieldDef, KeyedMap, SchemaRegistry};

use crate::class_resolution::EffectiveClass;
use crate::project_actions;
use crate::selection_model::SelectionModel;
use crate::undo_manager::{CommandError, UndoManager};
use crate::validation_session::{Findings, ValidationSession};

/// 对应 Java 里到处出现的 `throws Exception`（`ProjectController.java:347`、`:365`、`:403` 等）。
/// Rust 没有受检异常，统一装箱业务异常，保证「任何一步都可能带因失败」这条语义不丢。
pub type BoxError = Box<dyn Error + Send + Sync>;

mod commands;
use commands::*;
// 贴图导入内核（src-tauri/sprite_asset.rs）要在导入成功后推一条历史条目，所以只放开
// **这一种**命令的类型与一个窄入口（[`ProjectController::push_sprite_import`]）；
// `AppCommand` 本身仍不可名状，其余命令族只能经控制器的方法入栈（2026-11-25，贴图导入纳入 undo 栈）。
pub use commands::SpriteImportCommand;

/// 对应 Java `dev.modmaker.ui.ProjectController`（`:57` 类声明）。
///
/// Java 的字段（`:59-92`）逐个落到下面：`registry`（`:59`）、`project`（`:67`）、`board`（`:69`）、
/// `selection`（`:74`）、`validation`（`:68`）、`history`（`:81`）、`logSink`（`:76`）、
/// `overwriteApprover`（`:87`）、`canvasDrift`（`:92`）。
///
/// ## 字段形态上的差异
///
/// - `actions`（`:61`）：Java 的 `ProjectActions` 只持有 `registry` 与 `pipeline`，是无状态编排。
///   Rust 侧把它做成自由函数模块 [`project_actions`]，参数显式传入，因此这里没有对应字段。
/// - `validation`（`:68`）：搬成了真正的字段 [`ValidationSession`]，见 [`validation_session`] 模块头
///   对「缓存契约保留、后台去抖不落地」的逐条说明。
/// - `overwriteApprover`（`:87`）：Java 的初值是 `path -> true`，Rust 用 `None` 表达同一个默认，
///   见 [`set_overwrite_approver`](Self::set_overwrite_approver)；导入链（`:399-447`）已随 M2b 落地。
/// - `projectListeners`（`:76`）：没有对应字段——监听器扇出由 `src-tauri` 的 `emit` + 前端 `listen`
///   承担（Rust 侧不能持 `Rc<RefCell<…>>` 式的回调表）。
///
/// ## `board` 为什么是 `Option<usize>`
///
/// Java 的 `board` 是 `project.boards` 里某个元素的引用，切换画板只是换个引用。Rust 里
/// `ModProject` 独占 `Vec<Board>`，控制器不能既借出 `&mut project` 又留着 `&mut board`，
/// 所以存下标；所有 `board` 的读写都走 [`EditState`] 的同款取值路径。
pub struct ProjectController {
    /// 对应 Java `registry`（`:59`）：Java 由构造器传入，这里同样由 [`new`](Self::new) 传入。
    registry: SchemaRegistry,
    /// 对应 Java `project`（`:67`）：`null` 就是「没打开任何项目」。
    project: Option<ModProject>,
    /// 对应 Java `board`（`:69`）：当前画板在 `project.boards` 中的下标。
    board: Option<usize>,
    /// 对应 Java `selection`（`:74`）：见 [`SelectionModel`]。
    selection: SelectionModel,
    /// 对应 Java `history`（`:81`）：见 [`UndoManager`]。
    history: UndoManager<AppCommand>,
    /// 对应 Java `logSink`（`:76`）：Java 的默认值是空 lambda，`None` 就是同一个意思。
    log_sink: Option<LogSink>,
    /// 对应 Java `overwriteApprover`（`:83-87`）：导入要拆进一个**非空**项目目录之前先问它，
    /// 返回 `false` 就取消。Java 的初值是 `path -> true`（`:87`），注释 `:84-86` 写明
    /// 「默认一律批准，好让命令行的跑法保持非交互」——Rust 用 `None` 表达这个默认，
    /// 判定处见 [`read_import`](Self::read_import)。
    overwrite_approver: Option<OverwriteApprover>,
    /// 对应 Java `canvasDrift`（`:92`）：加载时发现、每次重建时刷新的画板漂移。
    canvas_drift: Diff,
    /// 对应 Java `validation`（`:64-68`）：全项目唯一的校验结论缓存，见 [`ValidationSession`]。
    validation: ValidationSession,
}

/// `logSink` 回调（Java `Consumer<String>`）的别名，只为收敛字段类型的复杂度。
type LogSink = Box<dyn Fn(&str)>;
/// `overwriteApprover` 回调（Java `Predicate<Path>`）的别名，同上。
type OverwriteApprover = Box<dyn Fn(&Path) -> bool>;

impl ProjectController {
    /// 对应 Java 构造器（`:263-267`）：存下 registry，另两个字段在 Java 里是
    /// `new ProjectActions(registry)`（`:265`）与 `new ValidationSession(registry, this::project)`
    /// （`:266`）——前者在 Rust 是无状态自由函数（见结构体注释），后者对应这里的
    /// [`ValidationSession::new`]：Java 传的两样东西（registry 与「取当前模型」的 lambda）
    /// 在这里都改成了每次调用显式传入，见 [`validation_session`] 模块头第 1 点。
    /// 其余字段全是 Java 的字段初始化器（`:67-92`）。
    pub fn new(registry: SchemaRegistry) -> Self {
        Self {
            registry,
            project: None,
            board: None,
            selection: SelectionModel::new(),
            history: UndoManager::new(),
            log_sink: None,
            overwrite_approver: None,
            canvas_drift: Diff {
                orphan_nodes: 0,
                missing_edges: 0,
                stale_edges: 0,
            },
            validation: ValidationSession::new(),
        }
    }

    // --- 访问器 ---------------------------------------------------------------------------------

    /// 对应 `registry()`（`:269-271`）。
    pub fn registry(&self) -> &SchemaRegistry {
        &self.registry
    }

    /// 对应 `project()`（`:273-275`）：Java 返回可变的 `ModProject` 引用（调用点直接改字段），
    /// Rust 侧返回只读引用；写路径全部走命令，见模块头第 1 点。
    pub fn project(&self) -> Option<&ModProject> {
        self.project.as_ref()
    }

    /// 对应 `project()`（`:273-275`）的可变版本：Java 那份返回的就是可变的 `ModProject`，
    /// 测试里 `crafter.setClassName("GenericCrafter")`、`unit.fields.remove("type")` 这类
    /// 「直接改动项目里的活对象」的写法要靠它，语义与 Java 一致。
    pub fn project_mut(&mut self) -> Option<&mut ModProject> {
        self.project.as_mut()
    }

    /// 对应 `board()`（`:277-279`）。
    pub fn board(&self) -> Option<&Board> {
        let index = self.board?;
        self.project.as_ref()?.boards.get(index)
    }

    /// 对应 `board()`（`:277-279`）的可变版本：Java 返回可变的 `Board`，测试里
    /// `nodeOf(block).contentId = null`、`controller.board().edges.clear()` 都这么用。
    pub fn board_mut(&mut self) -> Option<&mut Board> {
        let index = self.board?;
        self.project.as_mut()?.boards.get_mut(index)
    }

    /// 对应 `selected()`（`:281-283`）= `selection.selected()`。
    pub fn selected(&self) -> Option<&ContentRecord> {
        self.selection.selected()
    }

    /// 对应 `snapshotProject()`（`:328-331`）：给工作线程做只读用的模型快照。
    pub fn snapshot_project(&self) -> ModProject {
        self.require_project();
        self.live_project().snapshot()
    }

    // --- 校验一族（Java `:293-331` 与 `:2015-2048`）---------------------------------------------

    /// 对应 `validateQuietly()`（`:298-302`）：「为当前模型算出的结论，需要时在本线程就地算」。
    ///
    /// Java 方法体两行：`requireProject();` 与 `return validation.current();`。这里因为
    /// 缓存需要 registry 与模型（见 [`validation_session`] 模块头第 1 点），把两者显式交给
    /// [`ValidationSession::current`]，取的就是 Java 那个 `Supplier<ModProject>`（`:266`）的值。
    ///
    /// 返回 [`Findings`]（`Rc<Vec<Issue>>`）而不是 `Vec<Issue>`：Java 那边
    /// `assertSame(first, controller.validateQuietly())`（测试 `#33`）靠的是「缓存命中返回同一实例」，
    /// Rust 侧要保住这条契约只能用引用计数，见 [`ValidationSession::same`]。
    pub fn validate_quietly(&mut self) -> Findings {
        self.require_project(); // :300
                                // :301 `validation.current()`：Java 的 session 自己调
                                // `projects.get()`，这里把同一个值（当前模型，可能为 null）显式递过去。
        self.validation.current(&self.registry, self.project.as_ref())
    }

    /// 对应 `latestIssues()`（`:304-307`）：「最后一次算出来的结论，绝不做计算：活动面板渲染的就是它」。
    pub fn latest_issues(&self) -> Findings {
        self.validation.latest() // :306
    }

    /// 对应 `validationPending()`（`:314-317`）：「有后台校验在排队或在跑」。
    ///
    /// Java 的两个方法体各一行。这份移植不落地后台模式（见 [`validation_session`] 模块头第 3 点），
    /// 因此和 Java 未调用 `useValidationWorker`（`:293-296`）时完全同值：恒 `false`。
    /// `useValidationWorker`／`onValidationRefreshed`／`refreshValidationAsync`
    /// （`:293-296`、`:309-312`、`:319-325`）三者都是后台模式与 FX 监听器的入口，Rust 侧没有对应
    /// 机制，故不建空壳方法——前端要「正在检查」指示灯时，读的就是这个恒 `false` 的值。
    pub fn validation_pending(&self) -> bool {
        self.validation.pending() // :316
    }

    /// 对应 `validateSnapshot(ModProject snapshot)`（`:2042-2048`）：「在当前线程上校验项目的一份
    /// 快照。外壳的 io worker 用它，好让一次构建的校验不跑在 FX 线程上；快照在那之前就已取好」。
    /// 方法体只有一行 `return validation.validate(snapshot);`——**不碰缓存**。
    pub fn validate_snapshot(&self, snapshot: &ModProject) -> Findings {
        ValidationSession::validate_snapshot(&self.registry, snapshot) // :2047
    }

    /// 对应 `validate()`（`:2014-2021`）：类注释写的是「F5／'validate'：一次新鲜的同步校验，
    /// 绕过任何已缓存或正在跑的结果」。
    ///
    /// Java 四行原样：`requireProject()`（`:2016`，Rust 里同 [`require_project`] 直接 panic，
    /// 与本文件其它 `requireProject()` 落点一致）→ `validation.forceSync()`（`:2017`）→
    /// `errors = issues.stream().filter(Issue::isError).count()`（`:2018`）→ 日志（`:2019`）→
    /// `return issues`（`:2020`）。
    ///
    /// 那条日志在 Java 是**硬编码英文**（`"validation: " + errors + " errors, " + …`），不走
    /// `messages` 包，所以这里逐字照抄，不做任何「改进」。
    ///
    /// [`require_project`]: Self::require_project
    pub fn validate(&mut self) -> Findings {
        self.require_project(); // :2016
        let issues = self.validation.force_sync(&self.registry, self.project.as_ref()); // :2017
        let errors = issues.iter().filter(|issue| issue.is_error()).count(); // :2018
        self.log(&format!(
            "validation: {} errors, {} warnings",
            errors,
            issues.len() - errors // :2019 `(issues.size() - errors)`
        ));
        issues // :2020
    }

    /// 对应 `planBuild()`（`:2029-2031`）：构建的**模型那一半**——在校验之上定下这次保存、
    /// 抓打包器要用的值。`:2023-2028` 的注释写明它「必须跑在持有模型的线程上」，且「UI 因此
    /// 永远不会在工作线程上碰模型」；`:2030` 把已算好的发现一并交给 `actions`，规则因此
    /// 不会为一趟构建跑两遍。
    ///
    /// Java 一行写完 `actions.planBuild(project, validate())`；Rust 侧这两半对 `self` 的借用
    /// 不能并存（`validate` 要 `&ModProject`、`planBuild` 要 `&mut ModProject`），于是照 Java
    /// 实参的求值顺序拆成两步：先 `validate()`，再交给
    /// [`plan_build_with`](crate::project_actions::plan_build_with)。
    ///
    /// 那里那句 `issues.to_vec()` 对应 `BuildPipeline.java:96` 的 `List.copyOf(issues)`：
    /// [`Findings`] 是 `Rc<Vec<Issue>>`（共享用的壳），`Plan` 要的是一份自有所有权的列表。
    pub fn plan_build(&mut self) -> Result<Plan, BoxError> {
        self.require_project(); // :2029
        let issues = self.validate(); // :2030 的实参 `validate()`
        // 借用必须停在字段上：`plan_build_with` 同时要模型（可变）与注册表（只读）。
        let project = self
            .project
            .as_mut()
            .expect("no project is open (require_project() must run first)"); // :2030 的实参 `project`
        Ok(crate::project_actions::plan_build_with(project, &self.registry, issues.to_vec())?)
    }

    /// 对应 `writeBuildFolder(BuildPipeline.Plan plan, Path destination)`（`:2051-2056`）：
    /// 构建的**磁盘那一半**——纯文件工作，所以工作线程可以跑它（`:2046-2050` 的注释原话），
    /// 它先保存工程、再打包产物，保存失败会把未保存标记滚回去。`:2054` 播报一行摘要。
    pub fn write_build_folder(
        &mut self,
        plan: &Plan,
        destination: &Path,
    ) -> Result<ExportReport, BoxError> {
        let report = self.write_build_folder_only(plan, destination)?; // :2053
        self.log(&format!("build: {}", report.summary())); // :2054
        Ok(report)
    }

    /// 对应 `writeBuildZip(BuildPipeline.Plan plan, Path zipFile)`（`:2059-2063`）：
    /// zip 产物的磁盘那一半，同样在 `:2061` 播报一行摘要。
    pub fn write_build_zip(&mut self, plan: &Plan, zip_file: &Path) -> Result<ExportReport, BoxError> {
        let report = self.write_build_zip_only(plan, zip_file)?; // :2060
        self.log(&format!("build: {}", report.summary())); // :2061
        Ok(report)
    }

    /// [`write_build_folder`](Self::write_build_folder) 的「不播报」内核：把 `project` 与
    /// `registry` 的借用收在一处，外层才能在借用结束后调 [`log`](Self::log)（Java 没有这个
    /// 约束——它的 `actions` 与 `project` 是两个独立字段，`:2053` 与 `:2054` 想怎么写就怎么写）。
    ///
    /// 多出来的 `project` 形参见 `project_actions` 模块头第 2 点：Rust 的回滚要拿到模型本体。
    fn write_build_folder_only(
        &mut self,
        plan: &Plan,
        destination: &Path,
    ) -> Result<ExportReport, BoxError> {
        Ok(crate::project_actions::write_build_folder(
            self.project
                .as_mut()
                .expect("no project is open (require_project() must run first)"),
            &self.registry,
            plan,
            destination,
        )?)
    }

    /// [`write_build_folder_only`](Self::write_build_folder_only) 的 zip 版。
    fn write_build_zip_only(
        &mut self,
        plan: &Plan,
        zip_file: &Path,
    ) -> Result<ExportReport, BoxError> {
        Ok(crate::project_actions::write_build_zip(
            self.project
                .as_mut()
                .expect("no project is open (require_project() must run first)"),
            &self.registry,
            plan,
            zip_file,
        )?)
    }

    /// 对应 `setLogSink(Consumer<String>)`（`:333-335`）。
    pub fn set_log_sink(&mut self, sink: Box<dyn Fn(&str)>) {
        self.log_sink = Some(sink);
    }

    /// 对应 `setOverwriteApprover(Predicate<Path>)`（`:337-340`）：
    /// 设置导入覆写非空项目目录前要问的那个回调。Java 的 `approver == null ? path -> true : approver`
    /// （`:339`）在 Rust 里就是传 `None`——`None` 与「从没设过」同为「一律批准」（见字段注释）。
    pub fn set_overwrite_approver(&mut self, approver: Option<OverwriteApprover>) {
        self.overwrite_approver = approver;
    }

    /// 对应 `log(String message)`（`:342-344`）：方法体只有 `logSink.accept(message)` 一行。
    /// Java 的 sink 非空（默认空 lambda），`None` 走的是同一条「什么都不做」的路。
    pub fn log(&self, message: &str) {
        if let Some(sink) = &self.log_sink {
            sink(message);
        }
    }

    // --- project lifecycle ----------------------------------------------------------------------
    // 对应 Java `// --- project lifecycle ---` 段（`:346-536`）；本段里 meta 一族（`:538-670`）
    // 与格式转换一族（`:484-515`）按计划归 M2b 之后的批次，导入一族（`:399-447`）已在下面落地。

    /// 对应 `createProject(String modName, Path root)`（`:348-358`）。
    ///
    /// 唯一的签名差异：Java 末尾 `return project;` 把新建的模型交出去，而 Rust 里模型由控制器
    /// 独占（`self.project`），交出去就等于同时存在两个所有者。调用方要读它用
    /// [`project`](Self::project)，Java 的 `createProject(...)` 返回值在测试里也没被用过。
    pub fn create_project(&mut self, mod_name: &str, root: &Path) -> Result<(), BoxError> {
        // :349 `project = ProjectFactory.create(modName, root);`
        let project = project_factory::create(mod_name, root)?;
        // :350 `board = project.boards.isEmpty() ? null : project.boards.get(0);`
        let board = if project.boards.is_empty() {
            None
        } else {
            Some(0)
        };
        self.project = Some(project);
        self.board = board;
        self.selection.clear(); // :351
        // :352 `canvasDrift = new LinkGraph.Diff(0, 0, 0);`
        self.canvas_drift = Diff {
            orphan_nodes: 0,
            missing_edges: 0,
            stale_edges: 0,
        };
        self.clear_history(); // :353
        self.validation.reset(); // :354 新模型没有结论，`published` 也要一起清空
        self.log(&format!("new project: {}", root.display())); // :355
        self.fire_project_changed(); // :356
        Ok(())
    }

    /// 对应 `readProject(Path root)`（`:365-367`）：Java 一行 `return actions.readProject(root);`。
    /// 类注释 `:360-364` 说明这是「贵的那一半」（读包、解析内容），只碰视图还看不到的对象，
    /// 所以可以在 io 线程跑；结果交给 [`publish_project`](Self::publish_project)。
    pub fn read_project(&self, root: &Path) -> Result<ModProject, BoxError> {
        Ok(project_actions::read_project(root, &self.registry)?)
    }

    /// 对应 `publishProject(ModProject loaded, Path root)`（`:370-390`）：把刚读好的项目发布为
    /// 当前模型。Java 注释 `:369` 写明它必须跑在拥有模型的线程上（Rust 侧由 `&mut self` 保证）。
    pub fn publish_project(&mut self, loaded: ModProject, root: &Path) {
        let board = if loaded.boards.is_empty() {
            None // :372 `project.boards.isEmpty() ? null : ...`
        } else {
            Some(0)
        };
        self.project = Some(loaded);
        self.board = board;
        self.selection.clear(); // :373
        self.clear_history(); // :374
        self.validation.reset(); // :375 刚读进来的项目与上一个项目的结论无关
        // :376 `log("opened project: " + root + " (" + contents.size() + " records, " + assets.size() + " assets)")`
        let (records, assets) = {
            let project = self.live_project();
            (project.contents.len(), project.assets.len())
        };
        self.log(&format!(
            "opened project: {} ({} records, {} assets)",
            root.display(),
            records,
            assets
        ));
        // :378-380 `if (loaded.authoringWarning != null) log("  warn: " + loaded.authoringWarning);`
        let warning = self
            .project
            .as_ref()
            .and_then(|project| project.authoring_warning.clone());
        if let Some(warning) = warning {
            self.log(&format!("  warn: {}", warning));
        }
        // :381-384 画板是用户可编辑的文本，可能相对真正的 mod 文件过期（工具外增删过内容、
        // 老画板）。加载时查一次，而不是默默显示一个已不再镜像记录的画板。
        self.canvas_drift = self.canvas_diff();
        if !self.canvas_drift.clean() {
            // :386-388
            self.log(&format!(
                "canvas does not match the records ({}); use View > Rebuild Links to resync",
                self.canvas_drift.summary()
            ));
        }
        self.fire_project_changed(); // :389
    }

    /// 对应 `openProject(Path root)`（`:393-397`）：读 + 发布一步到位，给已经拥有模型的调用方。
    pub fn open_project(&mut self, root: &Path) -> Result<(), BoxError> {
        let loaded = self.read_project(root)?;
        self.publish_project(loaded, root);
        Ok(())
    }

    /// 对应 `readImport(Path source, Path destinationRoot)`（`:399-403`）：Java 的方法体只有一行
    /// `actions.readImport(source, destinationRoot, overwriteApprover, this::log)`。
    ///
    /// Java 注释 `:399` 之前那一段（`:405-408` 的同族注释）写明「读好的包还没被任何视图看到，
    /// 所以 io 线程可以跑它」；`&self` 而不是 `&mut self` 正是这一条在 Rust 里的表达——它一个字
    /// 也不碰活模型。批准器取 [`overwrite_approver`](Self::overwrite_approver)，
    /// `None` 就是 Java 字段初值 `path -> true`（`:87`）的「一律批准」。
    pub fn read_import(
        &self,
        source: &Path,
        destination_root: &Path,
    ) -> Result<project_actions::ImportResult, BoxError> {
        Ok(project_actions::read_import(
            source,
            destination_root,
            &self.registry,
            &mut |path: &Path| -> bool {
                match &self.overwrite_approver {
                    Some(approve) => approve(path),
                    None => true,
                }
            },
            &mut |message: &str| self.log(message),
        )?)
    }

    /// 对应 `readImport(Path, Path, ImportProgress)`（`:405-412`）：带进度回调的那一个重载。
    /// Java 注释 `:405-408`：把一个模组包（zip 或目录）读出来并拆到一个项目目录里，
    /// **不**发布它。
    pub fn read_import_with(
        &self,
        source: &Path,
        destination_root: &Path,
        progress: &mut dyn ImportProgress,
    ) -> Result<project_actions::ImportResult, BoxError> {
        Ok(project_actions::read_import_with(
            source,
            destination_root,
            &self.registry,
            progress,
            &mut |path: &Path| -> bool {
                match &self.overwrite_approver {
                    Some(approve) => approve(path),
                    None => true,
                }
            },
            &mut |message: &str| self.log(message),
        )?)
    }

    /// 对应 `publishImport(ProjectActions.ImportResult imported, Path source)`（`:414-440`）：
    /// 发布一个拆好的包，并把它装了什么报出来。Java 注释 `:414` 写明它必须跑在 FX 线程上
    /// （Rust 侧由 `&mut self` 保证）。
    ///
    /// 与 [`publish_project`](Self::publish_project) 有两处**不同**，都是 Java 原文自己的差异，
    /// 不是移植偏差：`:419` 这里把 `canvasDrift` 直接置成 `Diff(0, 0, 0)`（刚拆出来的画板
    /// 就是这次导入写的，谈不上过期），而 `publishProject` 在 `:381` 要现算一次；日志头一句
    /// 是 `imported <source>` ＋ 报告摘要（`:423-424`），不是 `opened project: …`。
    ///
    /// 签名差异：Java 末尾 `return;`（`void`），这里也是 `()`。Java 的 `imported` 参数是 record，
    /// 靠访问器取 `project()`／`report()`；Rust 侧直接解构三个字段，`root` 这里用不上。
    pub fn publish_import(&mut self, imported: project_actions::ImportResult, source: &Path) {
        let project_actions::ImportResult { project, report, .. } = imported;
        // :416-417 `project = imported.project(); board = project.boards.isEmpty() ? null : get(0);`
        let board = if project.boards.is_empty() {
            None
        } else {
            Some(0)
        };
        self.project = Some(project);
        self.board = board;
        self.selection.clear(); // :418
        // :419 `canvasDrift = new LinkGraph.Diff(0, 0, 0);`
        self.canvas_drift = Diff {
            orphan_nodes: 0,
            missing_edges: 0,
            stale_edges: 0,
        };
        self.clear_history(); // :420
        self.validation.reset(); // :421
        // :423-424
        self.log(&format!("imported {}", source.display()));
        self.log(&format!("  {}", report.summary()));
        // :425-431
        if !report.skipped.is_empty() {
            self.log(&format!(
                "  skipped {} file(s) that are not content:",
                report.skipped.len()
            ));
            for skipped in report.skipped.iter().take(10) {
                self.log(&format!("    {skipped}"));
            }
            if report.skipped.len() > 10 {
                self.log(&format!("    ...{} more", report.skipped.len() - 10));
            }
        }
        // :432-437
        if !report.parse_failures.is_empty() {
            self.log(&format!(
                "  {} content file(s) failed to parse and were kept as raw text:",
                report.parse_failures.len()
            ));
            for failure in report.parse_failures.keys().take(10) {
                self.log(&format!("    {failure}"));
            }
        }
        // :438
        for warning in report.warnings.iter().take(20) {
            self.log(&format!("  warn: {warning}"));
        }
        self.fire_project_changed(); // :439
    }

    /// 对应 `importPackage(Path source, Path destinationRoot)`（`:442-447`）：读、拆、发布
    /// 一步到位，给已经拥有模型的调用方。Java 注释 `:442` 就是这句话。
    ///
    /// 唯一的签名差异与 [`create_project`](Self::create_project) 同一条：Java 末尾
    /// `return imported.project();`，Rust 里模型由控制器独占，故返回 `()`；
    /// 调用方用 [`project`](Self::project) 读它。
    pub fn import_package(
        &mut self,
        source: &Path,
        destination_root: &Path,
    ) -> Result<(), BoxError> {
        let imported = self.read_import(source, destination_root)?; // :444
        self.publish_import(imported, source); // :445
        Ok(())
    }

    /// 对应 `save()`（`:454-460`）：在调用线程上同步保存。Java 注释 `:449-453` 写明这只给
    /// 已经拥有模型的调用方（退出流程、测试）；外壳的后台保存用
    /// [`plan_save`](Self::plan_save) 在 FX 线程定计划、在工作线程写盘。
    pub fn save(&mut self) -> Result<(), BoxError> {
        self.require_project(); // :455
        let plan = self.plan_save()?; // :456
        self.write_save_plan(&plan)?; // :457
        self.mark_saved(); // :458
        self.log(&format!("saved {}", plan.root().display())); // :459
        Ok(())
    }

    /// 对应 `planSave()`（`:467-470`）：决定一次保存要写什么。
    ///
    /// Java 注释 `:462-466` 强调它**必须**跑在拥有模型的线程上：它读字段树并刷新
    /// `raw`/`dirty`，中途进来的编辑要么弄坏快照要么抛异常。Rust 侧由 `&mut ModProject` 保证。
    ///
    /// `:469` 的 `project.root` 在 Rust 是 `Option<PathBuf>`：为 `None` 时 Java 抛 NPE，
    /// 等价物就是这里的 panic（新建/打开的项目都必然有 root，所以只有外部塞进模型的裸项目会命中）。
    pub fn plan_save(&mut self) -> Result<SavePlan, BoxError> {
        self.require_project();
        let project = self.live_project_mut();
        let root = project.root.clone().expect("project has no root");
        Ok(project_io::plan(project, &root)?)
    }

    /// 对应 `rollbackSave(ProjectIo.SavePlan plan)`（`:473-475`）：把「还没写盘」的标记放回原处。
    ///
    /// Java 的 `SavePlan` 是 `ProjectIo` 的静态内部类，建它的时候就把 `project` 存在里面，
    /// 所以 `:474` 只是 `plan.rollback()`。Rust 的 [`SavePlan`] 不持有模型（借用不能跨结构体存活），
    /// `rollback` 因此要多一个显式的 `&mut ModProject` 参数——传的就是这个控制器自己的项目。
    pub fn rollback_save(&mut self, plan: &SavePlan) {
        if let Some(project) = self.project.as_mut() {
            plan.rollback(project);
        }
    }

    /// 对应 `markSaved()`（`:478-482`）：一次完成的保存里，写盘阶段碰不到的那个标记由模型这边清掉。
    /// Java 的 `if (project != null)` 守卫原样保住。
    pub fn mark_saved(&mut self) {
        if let Some(project) = self.project.as_mut() {
            project.boards_dirty = false;
        }
    }

    /// 对应 `isDirty()`（`:518-536`）：上次保存之后有没有任何东西变过——记录、bundle、元数据或画板。
    ///
    /// `ModProject` 上没有 `isDirty`（Java 的 `ModProject` 也没有）：这个判断本来就在控制器里，
    /// 因为它要同时看四类标记。
    pub fn is_dirty(&self) -> bool {
        let Some(project) = self.project.as_ref() else {
            return false; // :519-521 `if (project == null) return false;`
        };
        if project.meta_dirty || project.boards_dirty {
            return true; // :522-524
        }
        for record in &project.contents {
            if record.dirty {
                return true; // :525-529
            }
        }
        for locale in project.strings.bundle_dirty.keys() {
            if project.strings.is_bundle_dirty(locale) {
                return true; // :530-534
            }
        }
        false // :535
    }

    // --- content --------------------------------------------------------------------------------
    // 对应 Java `// --- content ---` 段（`ProjectController.java:667-1300`）。

    /// 对应 `addContent(ContentType type)`（`:670-673`）。
    ///
    /// Java 靠方法重载给出「不落卡片位置」的入口，Rust 没有重载，于是拆成
    /// [`Self::add_content`]（等价单参版）与 [`Self::add_content_at`]（等价三参版）。
    pub fn add_content(&mut self, ctype: ContentType) -> String {
        self.add_content_at(ctype, None, None) // :671
    }

    /// 对应 `addContent(ContentType type, Double worldX, Double worldY)`（`:675-751`）。
    ///
    /// 返回值是 Java 交还的那个 `ContentRecord` 的 id：Rust 侧控制器独占 `Vec<ContentRecord>`，
    /// 交不出一个「与 `contents` 里那条同一个」的对象（见模块头第 2 点）。测试与面板拿这个 id
    /// 走 [`ModProject::content_by_id`] 去读，语义与 Java 拿着活对象读字段一致。
    pub fn add_content_at(
        &mut self,
        ctype: ContentType,
        world_x: Option<f64>,
        world_y: Option<f64>,
    ) -> String {
        self.require_project(); // :676
        let name = Self::unique_name(self.live_project(), ctype); // :677

        let mut record = ContentRecord::create(&ids::uid("c"), ctype, &name); // :678
        // :680-684 需要显式类名的类型（effect 一族）在创建时就写死基类，别退化成 modmaker.Effect。
        let base_class = self.registry.base_class_of(ctype); // :680
        if base_class.is_some() && base_class.as_deref() != Some(ctype.default_class()) {
            // :681-682
            record.set_class_name(base_class.as_deref()); // :683
        }
        // :686-693 有样板的类型连数值一起铺进去，不留一堆只有键没有值的空字段。
        if let Some(template) = planet::templates::template_for(ctype) {
            for (key, value) in template.iter() {
                record.fields.insert(key, value.clone());
            }
            record.dirty = true; // :692
        }
        // :696 脏标记必须在任何东西进入项目状态之前捕获。
        let mut flags = DirtyFlags::new(self.live_project(), &[Some(&record)]);
        // 对应 `added` / `placed` / `stringsKey` / `commandBoard` 四个捕获量（`:716-721`）。
        // 画板为空时 Java 连命令都不建（`:703` 的 `if (board != null)` 罩住整段），这里同样留 None。
        let mut placed: Option<CanvasNode> = None;
        let mut strings_key_captured: Option<String> = None;
        let mut command_board: Option<usize> = None;
        let selected_board = self.board;
        {
            let project = self.live_project_mut();
            project.contents.push(record.clone()); // :697
            // :698 新内容自带名字，所以默认语言的 bundle 要当场写一次。
            let strings_key = Self::string_key_of(project, &record, "name");
            project.strings.put(&strings_key, "en", &name);
            project.strings.mark_bundle_dirty("en"); // :699
            if !project.locales.iter().any(|locale| locale == "en") {
                // :700-702 这个语言还没被认下来，插到最前面，让检查器知道这条案要写。
                project.locales.insert(0, "en".to_string());
            }
            // :703-714 记录 + 卡片 + 连线由下面这一条命令一起撤销。
            if let Some(index) = selected_board {
                let mut node = CanvasNode {
                    id: Some(ids::uid("n")), // :705
                    kind: mm_model::canvas_node::KIND_CONTENT.to_string(), // :706
                    content_id: record.id.clone(), // :707
                    ..Default::default()
                };
                if let (Some(x), Some(y)) = (world_x, world_y) {
                    node.x = x; // :709
                    node.y = y; // :710
                } else {
                    // :712 Java 用的是 `board` 字段，正是 `self.board` 指过来的那块。
                    let board = &project.boards[index];
                    Self::place_below_all(board, &mut node);
                }
                project.boards[index].nodes.push(node.clone()); // :714
                placed = Some(node); // :717
                strings_key_captured = Some(strings_key); // :718
                command_board = Some(index); // :721
            }
            flags.capture_after(project); // :722
        }
        if let (Some(node), Some(strings_key), Some(board)) =
            (placed, strings_key_captured, command_board)
        {
            let command = AddCommand {
                record: record.clone(), // :716
                node,
                strings_key,
                board,
                flags,
            };
            self.push_command(AppCommand::Add(command)); // :723
        }
        // :745-747
        let template_name = planet::templates::name(ctype);
        let message = match template_name {
            Some(template) => format!(
                "added {} '{}' (from the {} template)",
                ctype.name(),
                name,
                template
            ),
            None => format!("added {} '{}'", ctype.name(), name),
        };
        self.log(&message);
        self.fire_project_changed(); // :748
        self.select(Some(&record)); // :749
        record
            .id
            .expect("ContentRecord::create always assigns an id") // :750
    }

    /// 对应 `uniqueName(ContentType type)`（`:2345-2356`）。
    ///
    /// Java 用 `LinkedHashSet` 去重（这里的顺序没有意义），Rust 用保序去重的 `Vec` 顶替。
    fn unique_name(project: &ModProject, ctype: ContentType) -> String {
        let mut taken: Vec<String> = Vec::new();
        for record in &project.contents {
            // :2348-2349 只盯同类型的非 inline 记录。
            if record.ctype == ctype
                && !record.is_inline()
                && !taken.contains(&record.name)
            {
                taken.push(record.name.clone());
            }
        }
        let base = format!("new-{}", ctype.name()); // :2350
        let mut index = 1usize; // :2351
        while taken.iter().any(|name| *name == format!("{base}-{index}")) {
            index += 1; // :2353
        }
        format!("{base}-{index}") // :2355
    }

    /// 对应 `placeBelowAll(CanvasNode node)`（`:754-761`）：新卡片默认落在整块画板最下方。
    fn place_below_all(board: &Board, node: &mut CanvasNode) {
        node.x = auto_layout::margin(); // :755
        let mut lowest = auto_layout::margin(); // :756-757
        for existing in &board.nodes {
            lowest = lowest.max(existing.y + existing.h); // :758-759
        }
        node.y = lowest + auto_layout::gap(); // :760
    }

    /// 对应 `rename(ContentRecord record, String newName)`（`:763-828`）。
    ///
    /// Java 交进来的是项目里那条**活记录**，改名直接写在它身上；Rust 侧控制器独占
    /// `Vec<ContentRecord>`，调用方只能交出一份副本（见模块头第 2 点）。因此这里只拿它的 `id`
    /// 去回查活记录，而 `patch` / `ctype` / `name` 三个分量在改名之前没人动过、与活记录同值，
    /// 可以直接用副本判断，省掉一次深拷贝。
    pub fn rename(&mut self, record: &ContentRecord, new_name: &str) {
        self.require_project(); // :765
        // :766-768 `record == null || newName == null || newName.equals(record.name)`。
        // Rust 的 `new_name: &str` 排除了 null；无 id 的记录无法按 id 回查，与 `record == null`
        // 同一处理（本工程所有记录都由 `Ids.uid` 带上 id）。
        let record_id = match record.id.as_deref() {
            Some(id) => id.to_string(),
            None => return,
        };
        if new_name == record.name {
            return;
        }
        // :769-773 名字不合规则只记一条日志，什么也不改。
        if let Some(problem) = name_rules::content_name_problem(new_name) {
            self.log(&format!("cannot rename to '{new_name}': {problem}"));
            return;
        }
        // :774-780 patch  registered 在它覆盖的那个原版名下，改名会把 patch 变成新内容。
        if record.patch {
            self.log(&format!(
                "cannot rename '{}': a patch keeps the vanilla name it overrides",
                record.name
            ));
            return;
        }
        // :781-791 同类型的名字必须唯一：Java 用 `other != record` 这条引用不等排除自己，
        // Rust 换成 id 不等。
        let taken = self.live_project().contents.iter().any(|other| {
            other.id.as_deref() != Some(record_id.as_str())
                && !other.is_inline()
                && other.ctype == record.ctype
                && other.name == new_name
        });
        if taken {
            self.log(&format!(
                "cannot rename to '{new_name}': {} '{new_name}' already exists; \
                 pick another name or rename that one first",
                record.ctype.name()
            ));
            return;
        }
        let previous = record.name.clone(); // :792
        // :793-795 任何一条记录都可能引用被改名的这条，所以盯的是**整个工程**的脏标记，
        // 只盯改名那条会让撤销后剩下的宿主仍然是脏的。
        let mut flags = {
            let project = self.live_project();
            let watched: Vec<Option<&ContentRecord>> = project.contents.iter().map(Some).collect();
            DirtyFlags::new(project, &watched)
        };
        // :796-797 全工程的引用重写；`target` 传副本，改写只按它的 id 与名字匹配。
        let rewrite = mm_fmt::refs::rename(
            self.project
                .as_mut()
                .expect("no project is open (require_project() must run first)"),
            &self.registry,
            record,
            &previous,
            new_name,
        );
        // :798-800 内联子弹存成 `ammoTypes.<item>`，只改宿主的键会让保存时 mergedTree 又把
        // 旧键塞回来，所以路径也得跟着搬。
        let inline_edits = Self::remap_ammo_inline_paths(
            self.live_project_mut(),
            &rewrite,
            record,
            &previous,
            new_name,
        );
        // :801-804 改活记录的名字、标脏、搬受管 bundle 键，然后记下这条命令留下了什么。
        {
            let project = self.live_project_mut();
            if let Some(live) = project.content_by_id_mut(&record_id) {
                live.name = new_name.to_string(); // :801
                live.dirty = true; // :802
            }
            // `migrateBundleKeys` 读的是改名之后的 `record`，Java 拿活对象，这里当场取一份
            // 同值副本（只用到 `ctype` / `patch` / `name`）。
            let current = project.content_by_id(&record_id).cloned();
            if let Some(current) = current {
                Self::migrate_bundle_keys(project, &current, &previous); // :803
            }
            flags.capture_after(project); // :804
        }
        self.push_command(AppCommand::Rename(RenameCommand {
            record_id,
            old_name: previous.clone(),
            new_name: new_name.to_string(),
            rewrite,
            inline_edits,
            flags,
        })); // :805-825
        self.fire_project_changed(); // :826
        self.log(&format!("renamed '{previous}' to '{new_name}'; the file moves on save")); // :827
    }

    /// 对应 `migrateBundleKeys(ContentRecord record, String oldName)`（`:830-849`）。
    ///
    /// 受管 bundle 键（name/description/details/credit）从记录的旧内容名搬到当前内容名，
    /// 本地化的显示名才不会因为一次改名而丢掉；撤销靠同样的调用反向搬回来。
    /// Java 直接在 `project.strings.values` 上 `remove` / `put`，这里同形。
    fn migrate_bundle_keys(project: &mut ModProject, record: &ContentRecord, old_name: &str) {
        let internal = project.internal_name(); // :836
        for suffix in ["name", "description", "details", "credit"] {
            // :838-839 旧键按**旧名**算，`record.patch` 决定要不要带命名空间。
            let old_key = name_rules::bundle_key(
                record.ctype,
                &name_rules::full_name(&internal, old_name, record.patch),
                suffix,
            );
            // :840-843 `var byLocale = values.get(oldKey); if (byLocale == null) continue;`
            // `values` 是 `KeyedMap`，取出来要一份副本才能既 `put` 给新键又遍历它的语言。
            let Some(by_locale) = project.strings.values.get(&old_key).cloned() else {
                continue;
            };
            let locales: Vec<String> = by_locale.keys().cloned().collect();
            // :844 新键按 `record` 的**当前名**算。
            let new_key =
                name_rules::bundle_key(record.ctype, &record.full_name(&internal), suffix);
            project.strings.values.remove(&old_key); // :845
            project.strings.values.put(new_key, by_locale); // :846
            // :847 `byLocale.keySet().forEach(project.strings::markBundleDirty);`
            for locale in locales {
                project.strings.mark_bundle_dirty(&locale);
            }
        }
    }

    /// 对应 `remapAmmoInlinePaths(...)`（`:2075-2110`）：宿主 ammoTypes 的键改名后，
    /// 跟着改它那些 `ammoTypes.<item>` 内联子弹的路径。
    ///
    /// 返回的就是 `edits` 列表；Java 在返回前已经就地改了 `inlinePath` 与 `name`（`:2106-2107`），
    /// 这里同样就地改，返回的表只给撤销用。
    fn remap_ammo_inline_paths(
        project: &mut ModProject,
        rewrite: &RewriteResult,
        target: &ContentRecord,
        old_name: &str,
        new_name: &str,
    ) -> Vec<InlinePathEdit> {
        if target.ctype != ContentType::Item {
            return Vec::new(); // :2081-2083 只有 item 会成为 ammoTypes 的键。
        }
        let internal = project.internal_name(); // :2084
        let full_old = name_rules::full_name(&internal, old_name, false); // :2085
        let full_new = name_rules::full_name(&internal, new_name, false); // :2086
        let mut edits: Vec<InlinePathEdit> = Vec::new(); // :2087
        // :2088 Java 的 for-each 遍历 `project.contents`；这个循环只改字段、不增删元素，
        // 所以按下标升序遍历与它等价。
        for index in 0..project.contents.len() {
            // :2089-2091 `!inline.isInline() || inline.inlinePath == null` 的一律跳过；
            // Java 的 `isInline()` 就是「有宿主」，这里连 id 一起取，缺任何一个都跳过。
            let (Some(record_id), Some(inline_path), Some(owner_id)) = (
                project.contents[index].id.clone(),
                project.contents[index].inline_path.clone(),
                project.contents[index].inline_owner_id.clone(),
            ) else {
                continue;
            };
            // :2092-2095 宿主必须在这次重写里被改过，否则这条路径压根没跟着动。
            let owner_changed = project
                .content_by_id(&owner_id)
                .is_some_and(|owner| Self::changed_by(rewrite, owner));
            if !owner_changed {
                continue;
            }
            // :2096-2102 先按裸名换前缀，换不动再按带命名空间的全名换一次；两次都不中就跳过。
            let Some(new_path) = Self::remap_ammo_path(&inline_path, old_name, new_name)
                .or_else(|| Self::remap_ammo_path(&inline_path, &full_old, &full_new))
            else {
                continue;
            };
            // :2103-2105 记下改动前后的路径与末段名。
            let new_leaf = leaf_name(&new_path);
            let old_leaf = project.contents[index].name.clone();
            project.contents[index].inline_path = Some(new_path.clone()); // :2106
            project.contents[index].name = new_leaf.clone(); // :2107
            edits.push(InlinePathEdit {
                record_id,
                old_path: inline_path,
                new_path,
                old_name: old_leaf,
                new_name: new_leaf,
            });
        }
        edits
    }

    /// 对应 `applyInlinePathEdits(List<InlinePathEdit> edits, boolean before)`（`:2112-2118`）：
    /// 撤销时把 ammoTypes 内联路径改回旧的一侧，重做时再改回新的一侧。
    ///
    /// Java 直接改 `edit.record()` 那个活对象，Rust 按 id 回查后再改（模块头第 2 点）。
    fn apply_inline_path_edits(project: &mut ModProject, edits: &[InlinePathEdit], before: bool) {
        for edit in edits {
            if let Some(record) = project.content_by_id_mut(&edit.record_id) {
                // :2115 `edit.record().inlinePath = before ? edit.oldPath() : edit.newPath();`
                record.inline_path = Some(if before {
                    edit.old_path.clone()
                } else {
                    edit.new_path.clone()
                });
                // :2116 `edit.record().name = before ? edit.oldName() : edit.newName();`
                record.name = if before {
                    edit.old_name.clone()
                } else {
                    edit.new_name.clone()
                };
            }
        }
    }

    /// 对应 `remapAmmoPath(String path, String oldKey, String newKey)`（`:2234-2241`）：
    /// 换上新的 ammoTypes 键之后的路径，不匹配时返回 `None`（Java 的 `null`）。
    fn remap_ammo_path(path: &str, old_key: &str, new_key: &str) -> Option<String> {
        let prefix = format!("ammoTypes.{}", escape_segment(old_key));
        if path != prefix && !path.starts_with(&format!("{prefix}.")) {
            return None;
        }
        Some(format!(
            "ammoTypes.{}{}",
            escape_segment(new_key),
            &path[prefix.len()..]
        ))
    }

    /// 对应 `changedBy(ReferenceRewrite.Result rewrite, ContentRecord record)`（`:2243-2245`）。
    ///
    /// Java 用 `change.record() == record` 这条引用相等；Rust 比 id，两个 `None` 不算相等——
    /// 那对应 Java 里两个不同的无 id 对象，引用相等同样是 false。
    fn changed_by(rewrite: &RewriteResult, record: &ContentRecord) -> bool {
        rewrite.changes.iter().any(|change| {
            change.record.id.is_some() && change.record.id == record.id
        })
    }

    /// 对应 `applyRewrite(ReferenceRewrite.Result result, boolean before)`（`:2247-2259`）：
    /// 应用一次引用重写的某一侧。
    ///
    /// Java 那边 `change.record()` 是活对象，直接 `applyTree`；Rust 的 [`Change::record`] 只是
    /// 改动后的快照副本（`mm_fmt::refs::rewrite` 的注释写明原因），所以必须按 id 回查活记录。
    /// 查不到就是这条记录已被摘出 `contents`（删除命令里就会这样），它不再属于项目，跳过即可。
    /// `deepCopy()`（`:2254`）在 Rust 由 owned 的 [`mm_json::Object`] 天然满足—— clone 即深拷贝。
    fn apply_rewrite(project: &mut ModProject, rewrite: &RewriteResult, before: bool) {
        for change in &rewrite.changes {
            let tree = if before {
                change.before.clone() // :2253
            } else {
                change.after.clone()
            };
            let Some(id) = change.record.id.as_deref() else {
                continue;
            };
            if let Some(record) = project.content_by_id_mut(id) {
                record.apply_tree(tree); // :2253-2254
                record.dirty = true; // :2255
            }
        }
        // :2257-2258 保存顺序整表换掉。
        project.meta.content_order.clear();
        let order = if before {
            &rewrite.order_before
        } else {
            &rewrite.order_after
        };
        project.meta.content_order.extend(order.iter().cloned());
    }

    /// 对应 `bundleLocales(ContentRecord record)`（`:1739-1750`）：这条记录那四个受管 bundle 键里
    /// **真的有译文**的那些，连各自的 locale 一起返回。
    ///
    /// Java 用 `LinkedHashMap` 保住键序（`BUNDLE_FIELDS` 的固定顺序），这里用 `Vec` 表达同一件事。
    fn bundle_locales(project: &ModProject, record: &ContentRecord) -> Vec<(String, KeyedMap<String>)> {
        let mut found: Vec<(String, KeyedMap<String>)> = Vec::new(); // :1741
        for field in ["name", "description", "details", "credit"] {
            // :1743 `String key = stringKey(record, field);`
            let key = Self::string_key_of(project, record, field);
            // :1744-1747 `var byLocale = project.strings.values.get(key); if (byLocale != null) …`
            if let Some(by_locale) = project.strings.values.get(&key) {
                found.push((key, by_locale.clone()));
            }
        }
        found
    }

    /// 对应 `markKeyLocalesDirty(String key)`（`:1752-1758`）：把这个键上有值的每个 locale 标脏，
    /// 下次保存才会重写那些 bundle。
    fn mark_key_locales_dirty(project: &mut ModProject, key: &str) {
        // :1754-1757 `var byLocale = project.strings.values.get(key); if (byLocale != null) …`
        if let Some(locales) = project.strings.values.get(key).map(|by_locale| by_locale.keys().cloned().collect::<Vec<String>>()) {
            for locale in locales {
                project.strings.mark_bundle_dirty(&locale);
            }
        }
    }

    /// 对应 `board.nodes.stream().filter(r -> record.id.equals(r.contentId)).findFirst()`
    /// （`:878-880`、`:2144-2146`、`:2180-2182` 三处同形）：按内容 id 找那张卡片。
    /// 没有画板（Java 的 `board == null`）或找不到就是 `None`。
    fn node_for_content(project: &ModProject, board: Option<usize>, content_id: &str) -> Option<CanvasNode> {
        project
            .boards
            .get(board?)?
            .nodes
            .iter()
            .find(|node| node.content_id.as_deref() == Some(content_id))
            .cloned()
    }

    /// 对应 `board.edges.stream().filter(e -> node.id.equals(e.from) || node.id.equals(e.to))`
    /// （`:884-886`、`:2148-2150`、`:2184-2186` 三处同形）：这张卡片当时挂着的全部边。
    fn edges_touching(project: &ModProject, board: Option<usize>, node_id: &str) -> Vec<CanvasEdge> {
        // `board == null || node == null` 时 Java 给的是 `List.of()`（`:2147`、`:2183`）。
        let Some(board) = board.and_then(|board| project.boards.get(board)) else {
            return Vec::new();
        };
        board
            .edges
            .iter()
            .filter(|edge| edge.from.as_deref() == Some(node_id) || edge.to.as_deref() == Some(node_id))
            .cloned()
            .collect()
    }

    /// 对应 `JsonPath.remove(tree, record.inlinePath)` 加上 `owner.applyTree(tree)`
    /// 与 `owner.dirty = true`（`:922-927`、`:1001-1006` 两处同形）：把内联对象从宿主的字段树里
    /// 摘掉。返回是否真的摘掉了什么（Java 用 `if (JsonPath.remove(…))` 决定要不要回写宿主）。
    fn remove_inline_from_owner(project: &mut ModProject, owner_id: &str, path: &str) -> bool {
        let owner = match project.content_by_id_mut(owner_id) {
            Some(owner) => owner,
            None => return false, // :921 `if (owner != null)`
        };
        // :922 `JsonObject tree = owner.tree();` —— `mm_json::remove` 收 `&mut Json`，
        // 所以把宿主那棵树包一层再就地摘，摘完拆开回写。
        let mut tree = Json::Obj(owner.tree());
        if !mm_json::remove(&mut tree, path) {
            return false; // :923 `if (JsonPath.remove(tree, record.inlinePath))`
        }
        if let Json::Obj(tree) = tree {
            owner.apply_tree(tree); // :924
        }
        owner.dirty = true; // :925
        true
    }

    /// 对应 `underAmmoKey(String path, String key)`（`:2229-2232`）。
    fn under_ammo_key(path: &str, key: &str) -> bool {
        let prefix = format!("ammoTypes.{}", escape_segment(key));
        path == prefix || path.starts_with(&format!("{prefix}."))
    }

    /// 对应 `removeAmmoInlineChildren(targets, rewrite)`（`:2120-2161`）：
    /// 删掉挂在被删的 ammoTypes 键下面的内联子弹，连同它们的卡片。撤销由
    /// [`apply_removed_inline`](Self::apply_removed_inline) 放回来，重做再摘一次。
    fn remove_ammo_inline_children(
        project: &mut ModProject,
        board: Option<usize>,
        targets: &[&ContentRecord],
        rewrite: &RewriteResult,
    ) -> Vec<RemovedInline> {
        let internal = project.internal_name(); // :2126
        let mut removed: Vec<RemovedInline> = Vec::new(); // :2127
        for target in targets {
            // :2129-2131 `target == null || target.patch || target.type != item` 一律跳过。
            // Rust 的 `targets` 里没有 null（见 `delete_contents` 的说明）。
            if target.patch || target.ctype != ContentType::Item {
                continue;
            }
            let full = name_rules::full_name(&internal, &target.name, false); // :2132
            // :2133 倒着扫，摘一个不影响还没扫到的那些下标。
            let mut victims: Vec<RemovedInline> = Vec::new();
            for index in (0..project.contents.len()).rev() {
                let inline = &project.contents[index];
                // :2135 不是内联、没有路径、或者本身就是这批目标之一（下一段会删它），都跳过。
                let (Some(inline_path), Some(owner_id), Some(inline_id)) = (
                    inline.inline_path.clone(),
                    inline.inline_owner_id.clone(),
                    inline.id.clone(),
                ) else {
                    continue;
                };
                if !inline.is_inline()
                    || targets.iter().any(|t| t.id.as_deref() == Some(inline_id.as_str()))
                {
                    continue;
                }
                // :2138-2143 宿主必须在这次重写里被改过，且这条路径确实挂在这个被删的键下
                // （裸名与带命名空间的全名各试一次）。
                let owner = project.content_by_id(&owner_id);
                let hit = match owner {
                    Some(owner) => {
                        Self::changed_by(rewrite, owner)
                            && (Self::under_ammo_key(&inline_path, &target.name)
                                || Self::under_ammo_key(&inline_path, &full))
                    }
                    None => false,
                };
                if !hit {
                    continue;
                }
                // :2144-2151 连卡片和它挂着的边一起收进快照。
                let node = Self::node_for_content(project, board, &inline_id);
                let edges = match node.as_ref().and_then(|node| node.id.as_ref()) {
                    Some(node_id) => Self::edges_touching(project, board, node_id),
                    None => Vec::new(),
                };
                victims.push(RemovedInline {
                    record: inline.clone(),
                    index,
                    path: Some(inline_path),
                    name: inline.name.clone(),
                    node,
                    edges,
                });
            }
            // :2152-2156 真正动手：从 `contents` 里摘掉，再摘卡片与边。
            // 倒序收集，所以按下标删除时前面的下标不会被后面的删除移动。
            for item in victims {
                // :2151 快照同时是撤销要放回的东西。
                removed.push(item.clone());
                project.contents.remove(item.index);
                if let (Some(board), Some(node)) = (board, item.node.as_ref())
                    && let Some(board) = project.boards.get_mut(board)
                {
                    let node_id = node.id.clone().unwrap_or_default();
                    board.nodes.retain(|live| live.id.as_deref() != Some(node_id.as_str()));
                    let edge_ids: Vec<String> = item
                        .edges
                        .iter()
                        .map(|edge| edge.id.clone().unwrap_or_default())
                        .collect();
                    board.edges.retain(|live| {
                        !edge_ids.iter().any(|id| Some(id) == live.id.as_ref())
                    });
                }
            }
        }
        removed.sort_by_key(|item| item.index); // :2159
        removed
    }

    /// 对应 `removeOwnedInlineChildren(targets)`（`:2163-2196`）：删掉宿主（比如一座炮塔）时，
    /// 把它按 `inlineOwnerId` 拥有的那些内联记录连卡片带边一并摘掉，免得留下孤儿记录和过期卡片。
    /// 撤销与重做跟 ammoTypes 那一族一样走 [`apply_removed_inline`](Self::apply_removed_inline)。
    fn remove_owned_inline_children(
        project: &mut ModProject,
        board: Option<usize>,
        targets: &[&ContentRecord],
    ) -> Vec<RemovedInline> {
        let mut removed: Vec<RemovedInline> = Vec::new(); // :2169
        for target in targets {
            // :2171-2173 `target == null || target.id == null` 跳过。
            let Some(target_id) = target.id.as_deref() else {
                continue;
            };
            let mut victims: Vec<RemovedInline> = Vec::new();
            for index in (0..project.contents.len()).rev() {
                let inline = &project.contents[index];
                // :2176-2179 必须是「内联 + 有宿主 id + 宿主正是这条目标 + 本身不在这批目标里」。
                // 注意这里**不**要求 `inlinePath` 非空，与 Java 一致（`:2187` 原样把可能为 null 的
                // 路径存进快照）。
                let Some(inline_id) = inline.id.clone() else {
                    continue;
                };
                if !inline.is_inline()
                    || inline.inline_owner_id.is_none()
                    || inline.inline_owner_id.as_deref() != Some(target_id)
                    || targets.iter().any(|t| t.id.as_deref() == Some(inline_id.as_str()))
                {
                    continue;
                }
                // :2180-2187 同样连卡片带边一起收。
                let node = Self::node_for_content(project, board, &inline_id);
                let edges = match node.as_ref().and_then(|node| node.id.as_ref()) {
                    Some(node_id) => Self::edges_touching(project, board, node_id),
                    None => Vec::new(),
                };
                victims.push(RemovedInline {
                    record: inline.clone(),
                    index,
                    path: inline.inline_path.clone(),
                    name: inline.name.clone(),
                    node,
                    edges,
                });
            }
            // :2188-2192 动手摘除。
            for item in victims {
                removed.push(item.clone());
                project.contents.remove(item.index);
                if let (Some(board), Some(node)) = (board, item.node.as_ref())
                    && let Some(board) = project.boards.get_mut(board)
                {
                    let node_id = node.id.clone().unwrap_or_default();
                    board.nodes.retain(|live| live.id.as_deref() != Some(node_id.as_str()));
                    let edge_ids: Vec<String> = item.edges
                        .iter()
                        .map(|edge| edge.id.clone().unwrap_or_default())
                        .collect();
                    board.edges.retain(|live| {
                        !edge_ids.iter().any(|id| Some(id) == live.id.as_ref())
                    });
                }
            }
        }
        removed
    }

    /// 对应 `applyRemovedInline(List<RemovedInline> removed, boolean restore)`（`:2198-2227`）：
    /// 按方向应用这批内联摘除（撤销 = 放回，重做 = 摘掉）。
    fn apply_removed_inline(
        project: &mut ModProject,
        board: Option<usize>,
        removed: &[RemovedInline],
        restore: bool,
    ) {
        for item in removed {
            let record_id = item.record.id.clone().unwrap_or_default();
            if restore {
                // :2202-2203 先把路径与名字放回摘掉前的值。Java 改的是活对象，Rust 改的是副本，
                // 副本随后 (:2206) 插入 `contents`，效果相同。
                let mut record = item.record.clone();
                record.inline_path = item.path.clone();
                record.name = item.name.clone();
                // :2204-2207 插回原来的下标（越界就夹到末尾），已经在了就不重复插。
                let index = item.index.min(project.contents.len());
                if !project.contents.iter().any(|live| live.id.as_deref() == Some(record_id.as_str()))
                {
                    project.contents.insert(index, record);
                }
                // :2208-2215 卡片与边放回，同样逐条判重。
                if let (Some(board), Some(node)) = (board, item.node.as_ref())
                    && let Some(board) = project.boards.get_mut(board)
                {
                    let node_id = node.id.clone().unwrap_or_default();
                    if !board.nodes.iter().any(|live| live.id.as_deref() == Some(node_id.as_str()))
                    {
                        board.nodes.push(node.clone());
                        for edge in &item.edges {
                            let edge_id = edge.id.clone().unwrap_or_default();
                            if !board
                                .edges
                                .iter()
                                .any(|live| live.id.as_deref() == Some(edge_id.as_str()))
                            {
                                board.edges.push(edge.clone());
                            }
                        }
                    }
                }
            } else {
                // :2217-2221 摘掉。
                project
                    .contents
                    .retain(|live| live.id.as_deref() != Some(record_id.as_str()));
                if let (Some(board), Some(node)) = (board, item.node.as_ref())
                    && let Some(board) = project.boards.get_mut(board)
                {
                    let node_id = node.id.clone().unwrap_or_default();
                    board.nodes.retain(|live| live.id.as_deref() != Some(node_id.as_str()));
                    let edge_ids: Vec<String> = item
                        .edges
                        .iter()
                        .map(|edge| edge.id.clone().unwrap_or_default())
                        .collect();
                    board.edges.retain(|live| {
                        !edge_ids.iter().any(|id| Some(id) == live.id.as_ref())
                    });
                }
            }
        }
        // :2224-2226 摘除过东西才把画板标脏（Java 的条件里有 `board != null`）。
        if !removed.is_empty() && board.is_some() {
            project.boards_dirty = true;
        }
    }

    /// 对应 `deleteContent(ContentRecord record)`（`:851-853`）：删一条，走批量删除那同一条路。
    pub fn delete_content(&mut self, record: Option<&ContentRecord>) {
        match record {
            Some(record) => self.delete_contents(&[record]), // :852 `List.of(record)`
            None => self.delete_contents(&[]),               // :852 `List.of()`
        }
    }

    /// 对应 `deleteContents(List<ContentRecord> records)`（`:855-1029`）：把一批记录作为**一个**
    /// 可撤销步骤删掉（框选多选的批量删除）。所有恢复原样需要的东西都在动手之前采集完，
    /// 所以同一批里含同一宿主的内联记录时，撤销也能逐字节还原。
    ///
    /// Java 的 `records` 里允许 `null`（`:865` 的 `filter(Objects::nonNull)`），Rust 由
    /// `&[&ContentRecord]` 在类型上排除；`distinct()` 靠引用相等，这里换成按 id 去重。
    pub fn delete_contents(&mut self, records: &[&ContentRecord]) {
        self.require_project(); // :861
        if records.is_empty() {
            return; // :862-864
        }
        // :865-868 filter(nonNull).distinct()，空了就返回。
        let mut target_ids: Vec<String> = Vec::new();
        for record in records {
            let Some(id) = record.id.as_deref() else { continue };
            if !target_ids.iter().any(|seen| seen == id) {
                target_ids.push(id.to_string());
            }
        }
        if target_ids.is_empty() {
            return;
        }
        let board = self.board; // :950 `Board commandBoard = board;`
        // :869-891 动手之前把每条目标的卡片、边、宿主字段树、bundle 键全部快照下来。
        let order_before = self.live_project().meta.content_order.clone(); // :869
        let mut captures: Vec<DeleteTarget> = Vec::new();
        for id in &target_ids {
            let project = self.live_project();
            let record = match project.content_by_id(id) {
                Some(record) => record.clone(),
                None => continue,
            };
            // :878-881 这张卡片（可能没有）。
            let node = Self::node_for_content(project, board, id);
            // :882-886 卡片挂着的那些边；没有画板或没有卡片就是空表。
            let touching = match node.as_ref().and_then(|node| node.id.as_ref()) {
                Some(node_id) => Self::edges_touching(project, board, node_id),
                None => Vec::new(),
            };
            // :887-889 内联记录的宿主，连同宿主改动前的整棵字段树。
            let (owner_id, owner_tree) = if record.is_inline() {
                match project.content_by_id(record.inline_owner_id.as_deref().unwrap_or_default()) {
                    Some(owner) => (owner.id.clone(), Some(owner.tree())),
                    None => (None, None),
                }
            } else {
                (None, None)
            };
            // :890 `keyLocales.add(bundleLocales(record));`
            let key_locales = Self::bundle_locales(project, &record);
            captures.push(DeleteTarget {
                record,
                node,
                touching,
                owner_id,
                owner_tree,
                key_locales,
            });
        }
        // :893-895 被删的记录、每个引用宿主和两张脏标记表都在**任何改动之前**捕获，
        // 所以撤销能把项目还原得像从没改过。盯的是整个工程的记录。
        let mut flags = {
            let project = self.live_project();
            let watched: Vec<Option<&ContentRecord>> = project.contents.iter().map(Some).collect();
            DirtyFlags::new(project, &watched)
        };
        let target_refs: Vec<&ContentRecord> = captures.iter().map(|item| &item.record).collect();
        // :896-898 先把对这批目标的所有引用移除：那些持有者马上就要失去让这些名字可解析的记录。
        let rewrite = mm_fmt::refs::remove_references(
            self.project
                .as_mut()
                .expect("no project is open (require_project() must run first)"),
            &self.registry,
            &target_refs,
        );
        // :899-901 删掉的物品若是 ammoTypes 的键，挂在 `ammoTypes.<item>` 下的内联子弹必须
        // 一起删，否则保存时 mergedTree 会把已删的那条又造出来。
        let mut removed_inline = Self::remove_ammo_inline_children(
            self.live_project_mut(),
            board,
            &target_refs,
            &rewrite,
        );
        // :902-904 非物品的宿主靠 `inlineOwnerId` 拥有它的内联子弹，删宿主就得连子记录一起删。
        let owned = Self::remove_owned_inline_children(
            self.live_project_mut(),
            board,
            &target_refs,
        );
        removed_inline.extend(owned);
        removed_inline.sort_by_key(|item| item.index); // :905
        let order_after = {
            let project = self.live_project_mut();
            // :906-910 patch 目标被 ReferenceRewrite 跳过（删掉它等于把原版内容还回来），
            // 但它在 contentOrder 里那一行仍然指着被删的文件，必须跟着去掉。
            for target in &captures {
                if let Some(position) = project
                    .meta
                    .content_order
                    .iter()
                    .position(|entry| *entry == target.record.name)
                {
                    project.meta.content_order.remove(position);
                }
            }
            let order_after = project.meta.content_order.clone(); // :911
            if order_after != order_before {
                project.meta_dirty = true; // :912-914
            }
            // :916-943 逐条真正删除。
            for target in &captures {
                let record_id = target.record.id.clone().unwrap_or_default();
                // :918-928 内联内容只存在于宿主体内，摘掉节点就是摘掉那个对象。
                if target.record.is_inline()
                    && let (Some(owner_id), Some(path)) = (
                        target.record.inline_owner_id.as_deref(),
                        target.record.inline_path.as_deref(),
                    )
                {
                    Self::remove_inline_from_owner(project, owner_id, path);
                }
                // :929-934 bundle 里还留着这条已删记录的受管四行：把有值的每个 locale 都标脏，
                // 只删 name 会把 description/details/credit 留在出厂的 bundle 里。
                let keys: Vec<String> = target.key_locales.iter().map(|(key, _)| key.clone()).collect();
                for key in &keys {
                    Self::mark_key_locales_dirty(project, key);
                }
                for key in &keys {
                    project.strings.remove_key(key);
                }
                // :935-942 从 `contents` 里摘掉，再摘卡片和它挂着的边。
                project.contents.retain(|live| live.id.as_deref() != Some(record_id.as_str()));
                if let Some(board) = board
                    && let Some(board) = project.boards.get_mut(board)
                {
                    // :937 `board.nodes.removeIf(candidate -> record.id.equals(candidate.contentId))`
                    board
                        .nodes
                        .retain(|live| live.content_id.as_deref() != Some(record_id.as_str()));
                    // :938-941 只有采集到过卡片的才摘边，而且判据是「此刻这条边还挂着那张卡片」
                    // ——不是采集时的那份 `touching` 快照。
                    if let Some(node) = target.node.as_ref() {
                        let node_id = node.id.clone().unwrap_or_default();
                        board.edges.retain(|edge| {
                            edge.from.as_deref() != Some(node_id.as_str())
                                && edge.to.as_deref() != Some(node_id.as_str())
                        });
                    }
                }
            }
            order_after // :911 `List<String> orderAfter = List.copyOf(project.meta.contentOrder);`
        };
        self.selection.clear(); // :944
        self.live_project_mut().boards_dirty = true; // :945
        let single = if captures.len() == 1 {
            Some((captures[0].record.ctype, captures[0].record.name.clone())) // :1024
        } else {
            None
        };
        let target_count = captures.len();
        flags.capture_after(self.live_project()); // :951
        self.push_command(AppCommand::Delete(DeleteCommand {
            targets: captures,
            removed_inline,
            rewrite,
            order_before,
            order_after,
            flags,
            board,
        })); // :952-1021
        if let Some((ctype, record_name)) = single {
            // :1023-1024 `log("deleted " + targets.get(0).type + " '" + targets.get(0).name + "'")`
            self.log(&format!("deleted {} '{}'", ctype.name(), record_name));
        } else {
            self.log(&format!("deleted {} records", target_count)); // :1026
        }
        self.fire_project_changed(); // :1028
    }

    /// 对应 `connect(String fromNodeId, String toNodeId, String fieldKey)`（`:1037-1114`）：
    /// 画一条引用线——把源内容的名字写进目标的字段，并在画板上加那条边。
    ///
    /// Java `:1031-1036` 的注释：源名字是**裸写**的——游戏先拿它和当前 mod 比对，原版目标绝不能
    /// 带命名空间前缀。源可以是一张完整的内容卡片，也可以是一张绑定了记录或绑定了原版名的载体卡片。
    pub fn connect(&mut self, from_node_id: &str, to_node_id: &str, field_key: &str) -> bool {
        self.require_project(); // :1038
        // :1039-1041 没有画板就什么也连不成。
        let board = match self.board {
            Some(board) => board,
            None => return false,
        };
        let from_node = self.node_by_id(from_node_id); // :1042
        let to_node = self.node_by_id(to_node_id); // :1043
        // :1044 `fromNode == toNode` 比的是引用，也就是「同一张卡片不能连自己」。
        let (from_node, to_node) = match (from_node, to_node) {
            (Some(from_node), Some(to_node)) => (from_node, to_node),
            _ => return false, // :1044-1046
        };
        if from_node.id == to_node.id {
            return false; // :1044
        }
        let from = self.content_of(Some(&from_node)); // :1047
        let to = self.content_of(Some(&to_node)); // :1048
        // :1049-1051 目标必须是卡片；源可以是纯原版名（`refName` 非空）的载体卡片。
        let to = match to {
            Some(to) => to,
            None => return false,
        };
        if from.is_none() && from_node.ref_name.is_none() {
            return false; // :1049
        }
        // :1052-1054 源与目标是同一条记录（Java 比记录引用）。
        if let Some(from) = &from
            && from.id == to.id
        {
            return false;
        }
        // :1055-1060 这条边已经在了就什么都不做：那会写出重复值，而重复值永远撤不干净。
        let exists = self.live_project()
            .boards
            .get(board)
            .map(|board| {
                board.edges.iter().any(|edge| {
                    edge.from.as_deref() == Some(from_node_id)
                        && edge.to.as_deref() == Some(to_node_id)
                        && edge.field_key.as_deref() == Some(field_key)
                })
            })
            .unwrap_or(false);
        if exists {
            self.log("that reference already exists"); // :1058
            return false; // :1059
        }
        // :1061-1065 字段必须是这条记录类型上的一个引用字段。
        let field = match self.reference_field(&to, field_key) {
            Some(field) => field,
            None => {
                self.log(&format!(
                    "{} is not a reference field of {}",
                    field_key, to.name // :1063
                ));
                return false; // :1064
            }
        };
        let source_name = self.source_name_of(Some(&from_node)); // :1067
        // :1068-1071 结构化形状按被引内容的类型分流；载体卡片没有记录，它给的就是那个原版类型名。
        let source_type_name: Option<String> = match &from {
            Some(record) => Some(record.ctype.name().to_string()),
            None => from_node.ref_type.clone(),
        };
        // :1072 这份标记只盯目标记录一条。
        let to_id = to.id.clone().unwrap_or_default();
        let mut flags = DirtyFlags::new(self.live_project(), &[Some(&to)]);
        // :1073 真正把值写进去。`apply` 就地改动活记录，与 Java 同形（失败时留下的半成品改动也一样）。
        let revert = match self.live_project_mut()
            .content_by_id_mut(&to_id)
        {
            Some(record) => mm_fmt::refs::apply_with_type(
                record,
                &field,
                &source_name,
                source_type_name.as_deref(),
            ),
            None => None,
        };
        if revert.is_none() {
            // :1074-1079 落不进任何槽位：告诉用户该先接什么进来。
            self.log(&format!(
                "wire a {} into {}.{} first - there is no slot for a {} yet",
                Self::entry_key_of(&field),
                to.name,
                field_key,
                source_type_name.clone().unwrap_or_else(|| "reference".to_string()) // :1076-1077
            ));
            return false;
        }
        let revert = revert.expect("上面已挡下 None");
        let edge = CanvasEdge {
            id: Some(ids::uid("e")), // :1082
            from: Some(from_node_id.to_string()), // :1083
            to: Some(to_node_id.to_string()), // :1084
            field_key: Some(field_key.to_string()), // :1085
            label: source_name.clone(), // :1086
            ..Default::default()
        };
        {
            let project = self.live_project_mut();
            // :1080 记录必然还在项目里（刚从里面读出来的），取不到就是画板与记录已经脱节。
            if let Some(record) = project.content_by_id_mut(&to_id) {
                record.dirty = true;
            }
            if let Some(board) = project.boards.get_mut(board) {
                board.edges.push(edge.clone()); // :1087
            }
            flags.capture_after(project); // :1091
        }
        // :1089-1090 `commandBoard` / `appliedSource` 都是当时的快照。
        self.push_command(AppCommand::Connect(ConnectCommand {
            to_id,
            field,
            revert,
            source_name: source_name.clone(),
            applied_source: source_type_name,
            edge,
            flags,
            board: Some(board),
        })); // :1092-1110
        self.log(&format!("{} -> {}.{}", source_name, to.name, field_key)); // :1111
        self.fire_project_changed(); // :1112
        true // :1113
    }

    /// 对应 `disconnect(CanvasEdge edge)`（`:1206-1280`）：摘掉一条边，并撤掉它代表的那份字段值。
    ///
    /// 「这条边在不在板上」Java 用 `board.edges.contains(edge)`（引用相等），这里按边 id 比。
    pub fn disconnect(&mut self, edge: Option<&CanvasEdge>) {
        self.require_project(); // :1207
        // :1208 Java 这里直接读 `board.edges`，画板为 null 会 NPE；`require_project` 之后画板必然
        // 随项目一起设好，取不到就按「这条边不在板上」处理。
        let board = match self.board {
            Some(board) => board,
            None => return,
        };
        // :1208 入参为 null 直接返回；`contains(edge)` 换成按 id 比。
        let edge = match edge {
            Some(edge) => edge.clone(),
            None => return,
        };
        let edge_id = edge.id.clone().unwrap_or_default();
        let contains = self.live_project()
            .boards
            .get(board)
            .map(|board| {
                board
                    .edges
                    .iter()
                    .any(|live| live.id.as_deref() == Some(edge_id.as_str()))
            })
            .unwrap_or(false);
        if !contains {
            return; // :1208-1210
        }
        let from_node = edge.from.as_deref().and_then(|id| self.node_by_id(id)); // :1211
        let to = self.content_of(
            edge.to
                .as_deref()
                .and_then(|id| self.node_by_id(id))
                .as_ref(),
        ); // :1212
        // :1213-1241 边指向的记录已经没了（画板上的陈旧授权数据）。摘掉它仍是用户做的一次编辑，
        // 所以必须像其它编辑一样可撤销——这一处裸删曾是唯一 Ctrl+Z 撤不回来的删除动作。
        let to = match to {
            Some(to) => to,
            None => {
                let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1218
                let project = self.live_project_mut();
                if let Some(board) = project.boards.get_mut(board) {
                    board
                        .edges
                        .retain(|live| live.id.as_deref() != Some(edge_id.as_str())); // :1219
                }
                project.boards_dirty = true; // :1220-1222
                flags.capture_after(project); // :1223
                self.push_command(AppCommand::DetachEdge(DetachEdgeCommand {
                    edge,
                    flags,
                    board: Some(board),
                })); // :1224-1238
                self.fire_project_changed(); // :1239
                return; // :1240
            }
        };
        // :1242 卡片没了就用边上当年写下的标签。
        let source_name = match &from_node {
            None => edge.label.clone(),
            Some(node) => self.source_name_of(Some(node)),
        };
        let from = self.content_of(from_node.as_ref()); // :1243
        // :1244
        let source_type_name: Option<String> = match (&from, &from_node) {
            (Some(record), _) => Some(record.ctype.name().to_string()),
            (None, Some(node)) => node.ref_type.clone(),
            (None, None) => None,
        };
        // :1245 `edge.fieldKey` 为 null 时 Java 传进 `referenceField` 也只会比成不相等，
        // 所以这里整段跳过。
        let field = edge
            .field_key
            .as_deref()
            .and_then(|key| self.reference_field(&to, key));
        // :1246
        let mut flags = DirtyFlags::new(self.live_project(), &[Some(&to)]);
        let to_id = to.id.clone().unwrap_or_default();
        // :1247 `field != null && ReferenceOps.remove(...)`。
        let mut removed = false;
        if let Some(field) = field.as_ref()
            && let Some(record) = self.live_project_mut().content_by_id_mut(&to_id)
        {
            removed = mm_fmt::refs::remove(record, field, &source_name);
        }
        if removed {
            // :1248-1250
            if let Some(record) = self.live_project_mut().content_by_id_mut(&to_id) {
                record.dirty = true;
            }
        }
        {
            let project = self.live_project_mut();
            if let Some(board) = project.boards.get_mut(board) {
                board
                    .edges
                    .retain(|live| live.id.as_deref() != Some(edge_id.as_str())); // :1251
            }
            flags.capture_after(project); // :1254
        }
        // :1255-1277
        self.push_command(AppCommand::Disconnect(DisconnectCommand {
            to_id,
            field,
            source_name: source_name.clone(),
            source_type_name,
            edge: edge.clone(),
            flags,
            board: Some(board),
        }));
        // :1278 Java 的字符串拼接会把 null 打成 `null`，这里如实保留。
        self.log(&format!(
            "removed {} -> {}.{}",
            source_name,
            to.name,
            edge.field_key.as_deref().unwrap_or("null")
        ));
        self.fire_project_changed(); // :1279
    }

    /// 对应 `commitMoves(Map<String, Point2D> before, Map<String, double[]> after)`
    /// （`:1340-1379`）：卡片拖动结束后，把这一次已经落地的移动记进撤销栈。
    ///
    /// 两侧的值类型统一成 `[f64; 2]`，理由见 [`CommitMovesCommand`] 的文档。
    /// 这里**不调** [`Self::require_project`]——Java 也没有：`:1346` 是一句弱守卫
    /// `if (project != null)`，不是 `requireProject()`。Java 里 `project` 为 null 时其实走不到
    /// 那句守卫：`:1345` 的 `new DirtyFlags()` 在其构造器 `:131` 就读 `project.strings`，先抛 NPE；
    /// Rust 用同一个位置的 `unwrap()` 复刻这次 panic，守卫本身照原样留着。
    pub fn commit_moves(&mut self, before: KeyedMap<[f64; 2]>, after: KeyedMap<[f64; 2]>) {
        // :1341-1344 原地按下又松开不算一次编辑：不入撤销栈，也不翻脏标记。
        if before.is_empty() || !Self::moves_anything(&before, &after) {
            return;
        }
        let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1345
        if let Some(project) = self.project.as_mut() {
            project.boards_dirty = true; // :1346-1348
        }
        flags.capture_after(self.live_project()); // :1349
        let command_board = self.board; // :1350 `Board commandBoard = board;`
        self.push_command(AppCommand::CommitMoves(CommitMovesCommand {
            before,
            after,
            flags,
            board: command_board,
        })); // :1351-1378
    }

    /// 卡片拖动落定：**先把坐标写进模型，再记撤销栈**。
    ///
    /// Java 侧这是画布与控制器之间的**一对**动作，而不是一个方法：拖动过程中
    /// `NodeCanvasPane.dragTo` 就地改 `CanvasNode.x/y`（`:1270-1271`，带 `snap()` 网格吸附），
    /// 松手时才调 `ProjectController.commitMoves(before, after)`（`:1340`）把这一次记进撤销栈。
    /// 两边活在同一进程里，模型早被 JavaFX 那侧改好了，`commitMoves` 只负责记账。
    ///
    /// 拆成 Rust + WebView 后卡片层在浏览器里，碰不到 Rust 的 `CanvasNode`，命令面
    /// （`node_move_commit`）能拿到的只有「前端实测的终点」，所以落模型这一步必须补在这里：
    /// 复用现成的 [`Self::apply_positions`]（`:1988-1999` 的移植——`CommitMovesCommand::apply`
    /// 与恢复快照走的就是同一个循环，不另起一份）。
    ///
    /// **顺序与 Java 一致**：先落坐标、后入栈。Java 进 `commitMoves` 时坐标已经落地，
    /// 而 `DirtyFlags::capture_after`（`:1349`）盯的是记录级脏标记、不含坐标，
    /// 所以先落坐标不会污染它。空 `before` 或坐标压根没变，与 Java `:1342-1344` 一样
    /// 整条忽略（连撤销栈都不进）。
    pub fn move_nodes(&mut self, before: KeyedMap<[f64; 2]>, after: KeyedMap<[f64; 2]>) {
        if before.is_empty() || !Self::moves_anything(&before, &after) {
            return; // :1342-1344 `if (before.isEmpty() || !anyMovement(before, after)) return;`
        }
        // `dragTo`（`:1270-1271`）的等价物：坐标先落到当前画板的节点上，保存才拿得到新位置。
        // 板子取不到就是它已不在项目里，与 Java 拿着陈旧引用是同一种状态，静默跳过。
        if let Some(board) = self.board
            && let Some(project) = self.project.as_mut()
            && let Some(board) = project.boards.get_mut(board)
        {
            ProjectController::apply_positions(board, &after);
        }
        self.commit_moves(before, after); // :1340
    }

    /// 对应私有静态 `movesAnything(Map<String, Point2D>, Map<String, double[]>)`（`:1382-1392`）：
    /// 至少有一张被拖的卡真的落到了别处。私有静态 → Rust 的私有关联函数。
    fn moves_anything(before: &KeyedMap<[f64; 2]>, after: &KeyedMap<[f64; 2]>) -> bool {
        for (node_id, position) in after.iter() {
            // :1384 `Point2D origin = before.get(entry.getKey());`
            let Some(origin) = before.get(node_id) else {
                continue; // :1386 的 `origin != null`
            };
            // :1385-1386 的 `position != null` 在 Rust 里恒真：`iter()` 给的就是值本身。
            // :1387 用的是 `double` 的 `!=`（NaN 与任何东西都不相等），f64 的 `!=` 同义。
            if origin[0] != position[0] || origin[1] != position[1] {
                return true; // :1388
            }
        }
        false // :1391
    }

    // --- reroute 中转点 -------------------------------------------------------------------------
    // 对应 Java `// --- reroute waypoints (report P2-9) ---` 段（`:1394-1495`）。
    // 三个方法都压撤销栈：Java 在 `:1396`（`Undoable.`）、`:1428`（`Undoable.`）与
    // `:1460-1464`（`Records a finished reroute drag so it can be undone`）各写了一次。

    /// Java 三个 reroute 方法各自开头那句重复守卫（`:1399`、`:1431`、`:1467`）：
    /// `board == null || !board.edges.contains(edge)`。这里把**当前画板**上那条边的点列拷出来，
    /// `None` 即守卫失败。两处塌缩如实登记：`edge == null` 与「板上查不到这条 id」同样是 `None`
    /// （Rust 侧前端只拿 id，没有「传了个 null 边进来」这种调用形态）；Java 的 `index < 0`
    /// （`:1432`、`:1468`）由 `usize` 类型在编译期承担。
    fn board_reroutes(&self, edge_id: &str) -> Option<Vec<[f64; 2]>> {
        let board = self.board?;
        let project = self.project.as_ref()?;
        let edge = project
            .boards
            .get(board)?
            .edges
            .iter()
            .find(|edge| edge.id.as_deref() == Some(edge_id))?;
        Some(edge.reroutes.clone())
    }

    /// 对应 `insertReroute(CanvasEdge edge, double worldX, double worldY)`（`:1397-1426`）：
    /// 往一条连线补一个 reroute 点（世界坐标），可撤销。
    pub fn insert_reroute(&mut self, edge_id: &str, world_x: f64, world_y: f64) {
        self.require_project(); // :1398
        let reroutes = match self.board_reroutes(edge_id) {
            Some(reroutes) => reroutes,
            None => return, // :1399-1401
        };
        let index = reroutes.len(); // :1402 `edge.reroutes.size()`——**插入前**的长度
        let point = [world_x, world_y]; // :1403
        let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1404
        if let Some(edge) = reroute_edge(self.live_project_mut(), edge_id) {
            edge.reroutes.push(point); // :1405
        }
        self.live_project_mut().boards_dirty = true; // :1406
        flags.capture_after(self.live_project()); // :1407
        self.push_command(AppCommand::InsertReroute(InsertRerouteCommand {
            edge_id: edge_id.to_string(),
            index,
            point,
            flags,
        })); // :1408-1424
        self.fire_project_changed(); // :1425
    }

    /// 对应 `removeReroute(CanvasEdge edge, int index)`（`:1429-1458`）：摘掉一个 reroute 点。
    pub fn remove_reroute(&mut self, edge_id: &str, index: usize) {
        self.require_project(); // :1430
        let reroutes = match self.board_reroutes(edge_id) {
            Some(reroutes) => reroutes,
            None => return, // :1431-1434
        };
        if index >= reroutes.len() {
            return; // :1432
        }
        let point = reroutes[index]; // :1435 `edge.reroutes.get(index)`
        let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1436
        if let Some(edge) = reroute_edge(self.live_project_mut(), edge_id) {
            edge.reroutes.remove(index); // :1437
        }
        self.live_project_mut().boards_dirty = true; // :1438
        flags.capture_after(self.live_project()); // :1439
        self.push_command(AppCommand::RemoveReroute(RemoveRerouteCommand {
            edge_id: edge_id.to_string(),
            index,
            point,
            flags,
        })); // :1440-1456
        self.fire_project_changed(); // :1457
    }

    /// 对应 `commitRerouteMove(CanvasEdge edge, int index, double[] before, double[] after)`
    /// （`:1465-1495`）：记下一次已落地的 reroute 拖动。
    ///
    /// 与 [`Self::commit_moves`] 一样**不调** [`Self::fire_project_changed`]：Java 那段
    /// （`:1478-1494`）到 `}` 就结束了，没有 `:1425`/`:1457` 那一句。原因写在 Java `:1461-1463`
    /// ——拖动期间活点已经被就地挪过了，重画会销毁正在拖的那个节点句柄。
    pub fn commit_reroute_move(
        &mut self,
        edge_id: &str,
        index: usize,
        before: [f64; 2],
        after: [f64; 2],
    ) {
        self.require_project(); // :1466
        let reroutes = match self.board_reroutes(edge_id) {
            Some(reroutes) => reroutes,
            None => return, // :1467-1470
        };
        if index >= reroutes.len() {
            return; // :1468
        }
        // :1471-1473 按下又松开在原地：什么都不压、什么都不弄脏。用的是 `double` 的 `==`
        // （NaN 与任何东西都不相等，于是 NaN 位移照样记账），f64 的 `==` 同义。
        if before[0] == after[0] && before[1] == after[1] {
            return;
        }
        if let Some(edge) = reroute_edge(self.live_project_mut(), edge_id) {
            edge.reroutes[index] = after; // :1474
        }
        let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1475
        self.live_project_mut().boards_dirty = true; // :1476
        flags.capture_after(self.live_project()); // :1477
        self.push_command(AppCommand::CommitRerouteMove(CommitRerouteMoveCommand {
            edge_id: edge_id.to_string(),
            index,
            before,
            after,
            flags,
        })); // :1478-1494
    }

    /// 对应 `select(ContentRecord record)`（`:1844-1846`）。
    ///
    /// [`SelectionModel`] 存的是记录副本（Rust 没有共享对象），所以这里把当前状态拷进去。
    /// Java 存的是活引用；两边都在每次编辑后重新 select，语义一致。
    ///
    /// 入参收成 `Option` 是 M4 的建模更正：Java 的调用方除了 `select(record)`（如 `:749`）
    /// 还有 **`select(null)`**（`NodeCanvasPane.java:1189`、`:1383` 用它取消选择），
    /// 而 Java 的 notify 不看 null，照样广播一条 `null` 载荷。`SelectionModel` 模块头记着这条
    /// 与 `clear()` 的区别。
    pub fn select(&mut self, record: Option<&ContentRecord>) {
        self.selection.select(record.cloned()); // :1845
    }

    // --- 画板与布局 -----------------------------------------------------------------------------
    // 对应 Java `addBoard` / `setBoard`（`:1848-1885`）与 `// --- layout, validation, build ---`
    // 段里的 `tidyLayout`（`:1889-1918`）、`rebuildLinks` 一族（`:1938-2012`）。同段的
    // `canvasDiff` / `canvasDrift`（`:1921-1930`）与 `validate` / `latestIssues` 已在上面各自小节落地。

    /// 对应 `addBoard(String name)`（`:1848-1880`）：新建一块画板、设为当前画板，并记一条可撤销命令。
    ///
    /// 返回新画板在 `project.boards` 里的**下标**：Java 返回的是 `Board` 活对象（`:1848`、`:1879`），
    /// Rust 里 `ModProject` 独占那张 `Vec<Board>`、外部拿不到引用（模块头第 2 点）。调用方拿下标
    /// 配 [`Self::board`] 断言的是同一块板。
    pub fn add_board(&mut self, name: &str) -> usize {
        self.require_project(); // :1849
        let created = Board { // :1850 `new Board()`
            id: Some(ids::uid("b")), // :1851
            name: name.to_string(), // :1852
            file: self.unique_board_file(name), // :1853
            ..Default::default()
        };
        let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1854
        let index = {
            let project = self.live_project_mut();
            project.boards.push(created.clone()); // :1855 `project.boards.add(created)`
            project.boards_dirty = true; // :1856
            project.boards.len() - 1 // Java 没有下标概念，这里只为 :1858 和 :1879 的返回值
        };
        let previous = self.board; // :1857 `Board previous = board;`
        self.board = Some(index); // :1858 `board = created;`
        flags.capture_after(self.live_project()); // :1859
        self.push_command(AppCommand::AddBoard(AddBoardCommand {
            board: created,
            previous,
            flags,
        })); // :1860-1877
        self.fire_project_changed(); // :1878
        index // :1879 `return created;`
    }

    /// 对应 `setBoard(Board next)`（`:1882-1885`）：就两行——换当前画板，然后广播。
    ///
    /// 参数从 `Board` 引用换成下标（同 [`Self::add_board`]），Java 的 `null` 就是这里的 `None`。
    pub fn set_board(&mut self, index: Option<usize>) {
        self.board = index; // :1883
        self.fire_project_changed(); // :1884
    }

    /// 把内容文件（以及 mod.json/mod.hjson 的 meta）批量翻到目标扩展名。
    ///
    /// 对应 Java `FormatConverter.convert(project, ext)` 的 UI 入口（M6 递延的「格式切换」）。
    /// 返回**本次会被转换**的记录数（转换前算，与对话框「将转换 N 个文件」同口径）；
    /// 转换本身只改 `ext`/`dirty` 并丢掉 `raw`，真正的落盘发生在下一次 `save`。
    /// 这条动作**不进撤销栈**（Java 也没有对应命令），转换后想反悔只能重开项目。
    ///
    /// 非法扩展名回 `Err`（2026-11-25 加固：领域层不再 panic，见
    /// `mm_io::import::UnsupportedFormatError`）；失败时项目未被改动，也不广播 changed。
    pub fn convert_format(&mut self, ext: &str) -> Result<usize, BoxError> {
        self.require_project();
        let planned = mm_io::import::pending_count(self.live_project(), ext) as usize;
        let project = self.live_project_mut();
        mm_io::import::convert(project, ext)?;
        self.fire_project_changed();
        Ok(planned)
    }

    /// 对应 `tidyLayout()`（`:1889-1918`）：整块画板重排，两端坐标都快照下来才好撤销（`:1894`）。
    ///
    /// 为通过借用检查做的非平凡妥协（必须逐条核对）：`auto_layout::tidy(&ModProject, &mut Board)`
    /// 要同时借 `self.project` 两次（一次整份只读、一次那块板可变），Rust 借不过去。解法是
    /// **把当前那块 `Board` 克隆出来，在克隆上跑 tidy，再按 node id 只把 `x`/`y` 回填真板**。
    /// 这跟 Java 就地 tidy 语义等价、不丢状态，因为 `auto_layout::tidy`（`auto_layout.rs:117-181`）
    /// 实测最后只把算出来的坐标写回 `node.x` / `node.y`（`auto_layout.rs:175-180`），
    /// 节点集合、连线、视口、板名/文件名等一概不动；回填也只动这两个字段。
    pub fn tidy_layout(&mut self) {
        self.require_project(); // :1890
        if self.board.is_none() {
            return; // :1891-1893
        }
        let before = self.snapshot_positions(); // :1895
        // :1896 `AutoLayout.tidy(project, board);`——见上面的克隆说明。
        let index = self.board.unwrap();
        let tidied = {
            let project = self.live_project();
            match project.boards.get(index) {
                // Java 拿的是活对象，不存在「下标越界」；越界意味着这块板已不在项目里，
                // 那就等价于「没有任何可回填的位置」。
                Some(board) => {
                    let mut working = board.clone();
                    auto_layout::tidy(project, &mut working); // :1896
                    let mut positions: KeyedMap<[f64; 2]> = KeyedMap::new();
                    for node in &working.nodes {
                        if let Some(id) = &node.id {
                            positions.put(id.clone(), [node.x, node.y]);
                        }
                    }
                    positions
                }
                None => KeyedMap::new(),
            }
        };
        if let Some(board) = self.live_project_mut().boards.get_mut(index) {
            Self::apply_positions(board, &tidied); // :1896 的效果落到真板上（只动 x/y）
        }
        let after = self.snapshot_positions(); // :1897
        if !Self::same_positions(&before, &after) {
            // :1898
            let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1899
            self.live_project_mut().boards_dirty = true; // :1900
            flags.capture_after(self.live_project()); // :1901
            let command_board = self.board; // :1902 `Board commandBoard = board;`
            self.push_command(AppCommand::TidyLayout(TidyLayoutCommand {
                board: command_board,
                before,
                after,
                flags,
            })); // :1903-1915
        }
        self.fire_project_changed(); // :1917——在 `if` 外面，无条件广播
    }

    /// 对应 `rebuildLinks()`（`:1938-1976`）：按记录重算连线，返回本次改动了几条边。
    pub fn rebuild_links(&mut self) -> usize {
        self.require_project(); // :1939
        let Some(index) = self.board else {
            return 0; // :1940-1942
        };
        let (before, rebuild) = {
            let project = self.live_project();
            let Some(board) = project.boards.get(index) else {
                // 同 `tidy_layout`：Java 拿着活引用不会越界，越界就是那块板已不在项目里。
                return 0;
            };
            let before = board.edges.clone(); // :1943 `new ArrayList<>(board.edges)`
            let rebuild = link_graph::rebuild(project, board, &self.registry); // :1944
            (before, rebuild)
        };
        if rebuild.added == 0 && rebuild.removed == 0 {
            // :1945-1948
            self.log("links already match the records"); // :1946
            return 0; // :1947
        }
        let after = rebuild.edges.clone(); // :1950 `List<CanvasEdge> after = rebuild.edges();`
        let mut flags = DirtyFlags::new(self.live_project(), &[]); // :1951
        {
            let project = self.live_project_mut();
            if let Some(board) = project.boards.get_mut(index) {
                // :1952-1953 `board.edges.clear(); board.edges.addAll(after);`——整表赋值等效。
                board.edges = after.clone();
            }
            project.boards_dirty = true; // :1954
            flags.capture_after(project); // :1955
        }
        self.push_command(AppCommand::RebuildLinks(RebuildLinksCommand {
            before,
            after,
            flags,
            board: Some(index), // :1949 `Board commandBoard = board;`
        })); // :1956-1970
        self.canvas_drift = self.canvas_diff(); // :1971
        let total = self.live_project()
            .boards
            .get(index)
            .map(|board| board.edges.len())
            .unwrap_or(0); // :1973 `board.edges.size()`
        self.log(&format!(
            "rebuilt links: +{} -{} ({} total)",
            rebuild.added, rebuild.removed, total
        )); // :1972-1973
        self.fire_project_changed(); // :1974
        rebuild.added + rebuild.removed // :1975
    }

    /// 对应私有 `snapshotPositions()`（`:1978-1986`）：当前画板每张卡的 `node.id -> {x, y}`。
    ///
    /// Java 直接用 `node.id` 当 map key（`:1982`），Rust 侧 `id` 是 `Option<String>`：
    /// `None` 的节点跳过。Java 里那等于往 map 塞一个 null key，实际不会发生——所有节点 id
    /// 都由 `Ids.uid("n")` 生成。
    fn snapshot_positions(&self) -> KeyedMap<[f64; 2]> {
        let mut snapshot: KeyedMap<[f64; 2]> = KeyedMap::new(); // :1979
        if let Some(index) = self.board {
            // :1980 `if (board != null)`
            if let Some(board) = self
                .project
                .as_ref()
                .and_then(|project| project.boards.get(index))
            {
                for node in &board.nodes {
                    // :1981-1983
                    if let Some(id) = &node.id {
                        snapshot.put(id.clone(), [node.x, node.y]);
                    }
                }
            }
        }
        snapshot // :1985
    }

    /// 对应私有 `applyPositions(Board target, Map<String, double[]> positions)`（`:1988-1999`）。
    ///
    /// Java 是私有**实例**方法，但方法体里一点没用 `this`，所以 Rust 做成私有**关联函数**。
    /// `:1989-1991` 的 `if (target == null) return;` 守卫由调用方那层 `if let Some(board)` 承担
    /// （见 [`CommitMovesCommand::apply`] 与 [`TidyLayoutCommand`] 的两个方法）。
    fn apply_positions(board: &mut Board, positions: &KeyedMap<[f64; 2]>) {
        for node in &mut board.nodes {
            // :1992
            if let Some(position) = node.id.as_deref().and_then(|id| positions.get(id)) {
                // :1993-1994 `positions.get(node.id)` 为 null 就 continue
                node.x = position[0]; // :1995
                node.y = position[1]; // :1996
            }
        }
    }

    /// 对应私有静态 `samePositions(Map, Map)`（`:2002-2012`）：先比 key 集合，再按内容比每个值。
    fn same_positions(before: &KeyedMap<[f64; 2]>, after: &KeyedMap<[f64; 2]>) -> bool {
        // :2003 `!before.keySet().equals(after.keySet())`——两边 key 都唯一，长度相等 + 互相包含即相等。
        let same_keys = before.len() == after.len()
            && before.keys().all(|node_id| after.contains_key(node_id));
        if !same_keys {
            return false; // :2004
        }
        for node_id in before.keys() {
            // :2006 `before.get(nodeId)` 与 `after.get(nodeId)`
            let (origin, position) = match (before.get(node_id), after.get(node_id)) {
                (Some(origin), Some(position)) => (origin, position),
                // key 集合上面已判过相等，这里取不到只会是实现出错；Java 那两步拿到 null 会 NPE。
                _ => return false,
            };
            // :2007 用的是 `java.util.Arrays.equals`，它按 `Double.compare` 逐元素比：NaN 与自身相等、
            // +0.0 与 -0.0 不等。Rust 的 `f64 ==` 是 IEEE 比较，这两处正好相反，故用 `total_cmp`
            // 复刻 `Double.compare` 的语义。
            if origin[0].total_cmp(&position[0]) != std::cmp::Ordering::Equal
                || origin[1].total_cmp(&position[1]) != std::cmp::Ordering::Equal
            {
                return false; // :2008
            }
        }
        true // :2011
    }

    // --- undo / redo ----------------------------------------------------------------------------
    // 对应 Java `// --- undo / redo ---` 段（`:1497-1536`）。

    /// 对应 `pushCommand(Command command)`（`:1499-1501`）：方法体只有 `history.push(command);`。
    ///
    /// 这里**没有**合并逻辑：连打合并发生在调用点（`StringCommand` 的注释 `:293` 指向
    /// `:1770-1772`，已随 `setString` 落地）。Java 的 `pushCommand` 从来就是纯入栈。
    pub fn push_command(&mut self, command: AppCommand) {
        self.history.push(command);
    }

    /// 贴图导入成功后推一条贴图变更条目（2026-11-25，UI 审计缺陷 #5，**授权的新设计**：
    /// Java modmaker 没有应用内贴图导入，无对应物）。
    ///
    /// 只放开这一个窄入口、不放开整个 `AppCommand`：贴图命令的快照参数（旧字节、新字节、
    /// 账本旧条目）只有写盘的壳在写入前收得齐，控制器不替调用方猜。与字段编辑条目同栈，
    /// 按时间先后 undo/redo。细节与已知语义见 [`SpriteImportCommand`] 的文档。
    pub fn push_sprite_import(&mut self, command: SpriteImportCommand) {
        self.history.push(AppCommand::SpriteImport(command));
    }

    /// 对应 `canUndo()`（`:1503-1505`）。
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// 对应 `canRedo()`（`:1507-1509`）。
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// 对应 `undoStackDepth()`（`:1512-1514`）：Java 是包私有 `(test hook)`，
    /// Rust 的测试在另一个 crate，所以这里放开为 `pub`。
    pub fn undo_stack_depth(&self) -> usize {
        self.history.depth()
    }

    /// 对应 `undo()`（`:1516-1523`）：历史为空时直接返回，然后记一条日志、广播变更。
    ///
    /// 2026-11-25（贴图导入纳入 undo 栈）：返回值带上 [`CommandError`]——贴图命令写磁盘
    /// 可失败；失败时历史不被破坏（条目留栈可重试），**不**记日志、**不**广播（什么都没改）。
    /// 既有内存命令恒 `Ok`，行为与 Java 逐字一致。
    pub fn undo(&mut self) -> Result<(), CommandError> {
        self.require_project(); // :1517
        let Some(project) = self.project.as_mut() else {
            return Ok(());
        };
        let mut state = EditState {
            project,
            board: &mut self.board,
            registry: &self.registry,
        };
        match self.history.undo(&mut state) {
            Ok(None) => return Ok(()), // :1518-1520 `if (history.undo() == null) return;`
            Ok(Some(_)) => {}
            Err(err) => return Err(err),
        }
        self.log("undo"); // :1521
        self.fire_project_changed(); // :1522
        Ok(())
    }

    /// 对应 `redo()`（`:1525-1532`）：与 `undo` 同构（失败口径同上）。
    pub fn redo(&mut self) -> Result<(), CommandError> {
        self.require_project(); // :1526
        let Some(project) = self.project.as_mut() else {
            return Ok(());
        };
        let mut state = EditState {
            project,
            board: &mut self.board,
            registry: &self.registry,
        };
        match self.history.redo(&mut state) {
            Ok(None) => return Ok(()), // :1527-1529
            Ok(Some(_)) => {}
            Err(err) => return Err(err),
        }
        self.log("redo"); // :1530
        self.fire_project_changed(); // :1531
        Ok(())
    }

    /// 对应 `clearHistory()`（`:1534-1536`）：Java 是 private，只在生命周期里被调
    /// （`:353`、`:374`、`:420`），所以这里同为 private。
    fn clear_history(&mut self) {
        self.history.clear();
    }

    // --- 编辑命令 ---------------------------------------------------------------------------------
    // 对应 Java `setField`／`setString`（`:1696-1777`）。Java 这一段还有 `setPatch`（`:1779-1806`）、
    // `applyRawTree`（`:1808-1842`）与画板、引用一族，已随 M2b 落到下面。
    //
    // 签名上唯一的差异：Java 的第一参数是 `ContentRecord record`（调用方从 `project.contents`
    // 里拿到的那个引用），Rust 侧模型由控制器独占，外部递不进引用，所以收 **id**；
    // Java 的 `if (record == null) return;` 与「id 查不到」是同一件事，逐处注明。

    /// 对应 `setField(ContentRecord record, String field, JsonElement value)`（`:1698-1737`）：
    /// 改一条记录的一个字段并翻脏，让它在下次保存时重新序列化。可撤销；对同一字段的快速连打
    /// （调色盘一点点 nudging 数值）合并成一步（`:1696-1697` 的注释）。
    pub fn set_field(&mut self, record_id: &str, field: &str, value: Option<Json>) {
        self.require_project(); // :1699
        // :1700-1702 `if (record == null) return;`
        let Some(record) = self.live_project().content_by_id(record_id) else {
            return;
        };
        // :1703 `JsonElement previous = record.fields.get(field);`——Java 留的是引用，Rust 取出克隆。
        let previous = record.fields.get(field).cloned();
        // :1704-1708：对象与数组是被结构化编辑器**原地**改的（mesh 编辑器改的就是记录自己的
        // JsonObject），所以「引用相等」仍然意味着「变过」，这两个提前返回对它们不适用。
        let mutable = previous.as_ref().is_some_and(|value| value.is_object() || value.is_array())
            || value.as_ref().is_some_and(|value| value.is_object() || value.is_array());
        if !mutable {
            // :1710-1712 两边都是 `null`：没有要写的东西。
            if value.is_none() && previous.is_none() {
                return;
            }
            // :1713-1715 `previous != null && previous.equals(value)`：值没变，一个命令都不该留。
            if previous.is_some() && previous == value {
                return;
            }
        }
        // :1717 `new DirtyFlags(record)`——只盯这一条记录。
        let flags = {
            let project = self.live_project();
            let record = project.content_by_id(record_id).expect("record checked above");
            DirtyFlags::new(project, &[Some(record)])
        };
        // :1718-1723 先落字节再翻脏。
        {
            let record = self.live_project_mut()
                .content_by_id_mut(record_id)
                .expect("record checked above");
            match &value {
                // Java 的 `fields.put` 对应 `Object::insert`（同键保位换值），与
                // [`FieldCommand::apply`] 走的是同一个写法。
                Some(value) => record.fields.insert(field.to_string(), value.clone()),
                None => {
                    record.fields.remove(field);
                }
            }
            record.dirty = true;
        }
        self.warn_about_reserialisation(record_id); // :1724
        // :1725-1727「原地改过的对象需要快照，undo 才有意义」。Java 的 `deepCopy()` 在这里
        // 就是 `Clone`——`Json` 整棵树按值克隆，不存在共享子树。
        let old_snapshot = if mutable { previous.clone() } else { previous };
        let new_snapshot = if mutable { value.clone() } else { value };
        let mut field_command = FieldCommand::new(
            self.live_project()
                .content_by_id(record_id)
                .expect("record checked above"),
            field,
            old_snapshot,
            new_snapshot,
            flags,
        );
        // :1729 `flags.captureAfter()`：命令持有的就是这同一份标记对象，所以在命令身上调它。
        field_command.flags.capture_after(self.live_project());
        // :1730-1735 连打合并：栈顶就是同一条记录的同一个字段时，只把它的 `newValue` 换掉
        // 并作废 redo 分支，历史深度不变；否则才真的压一条新命令。
        let coalesce = matches!(self.history.peek_undo(),
            Some(AppCommand::Field(top)) if top.same_target(&field_command));
        if coalesce {
            let new_value = field_command.new_value.clone();
            if let Some(AppCommand::Field(top)) = self.history.peek_undo() {
                top.new_value = new_value; // :1731 `top.newValue = newSnapshot;`
            }
            self.history.clear_redo(); // :1732
        } else {
            self.push_command(AppCommand::Field(field_command)); // :1734
        }
        self.fire_project_changed(); // :1736
    }

    /// 对应 `setString(String key, String locale, String value)`（`:1760-1777`）：改一条 bundle
    /// 文案，同一格子的快速连打合并（`:225` 的 `StringCommand` 注释指的就是 `:1770-1772`）。
    ///
    /// 与 Java 的一处表示差异：Java 的 `value` 可以为 `null`（写进 `values` 后由 `renderBundle`
    /// 过滤掉，见 [`StringCommand::apply`] 的说明），这里收 `&str`。全项目 59 个调用点
    /// （`StringsPane.java:117`、`RecordInspector.java:267` 与三个测试）传的都是表格里的新文本，
    /// 没有一处传 `null`；清空文案走的是 `rename`／`deleteContent` 一族，不是 `setString`。
    pub fn set_string(&mut self, key: &str, locale: &str, value: &str) {
        self.require_project(); // :1761
        // :1761 `String previous = project.strings.get(key, locale);`——该格子没填过时是 `null`。
        let previous = self.live_project()
            .strings
            .get(key, locale)
            .map(|value| value.to_string());
        // :1762-1764 `Objects.equals(previous, value)`：文案没变就不留命令。
        if previous.as_deref() == Some(value) {
            return;
        }
        // :1765 `new DirtyFlags()`：一个记录都不盯，只盯 bundle／画板／meta 这三类总标记。
        let flags = DirtyFlags::new(self.live_project(), &[]);
        // :1766-1767 与 [`StringCommand::apply`] 是同两条语句，直接复用它，免得写两遍。
        StringCommand::apply(&mut self.live_project_mut().strings, key, locale, Some(value));
        let mut string_command = StringCommand::new(key, locale, previous, value.to_string(), flags);
        // :1769 `flags.captureAfter();`
        string_command.flags.capture_after(self.live_project());
        // :1770-1775 合并判定同 `set_field`。
        let coalesce = matches!(self.history.peek_undo(),
            Some(AppCommand::String(top)) if top.same_target(&string_command));
        if coalesce {
            let new_value = string_command.new_value.clone();
            if let Some(AppCommand::String(top)) = self.history.peek_undo() {
                top.new_value = new_value; // :1771 `top.newValue = value;`
            }
            self.history.clear_redo(); // :1772
        } else {
            self.push_command(AppCommand::String(string_command)); // :1774
        }
        self.fire_project_changed(); // :1776
    }

    /// 对应 `setPatch(ContentRecord record, boolean patch)`（`:1779-1806`）：把一条记录标记成
    /// 「覆盖原版内容」或改回新增内容。
    ///
    /// 快速连打**不**合并（`:1779` 的注释）——这里没有开关要合并，一次翻动就是一条命令。
    /// 签名同样收 id 而不是 `ContentRecord` 引用，`record == null`（`:1782`）就是 id 查不到。
    pub fn set_patch(&mut self, record_id: &str, patch: bool) {
        self.require_project(); // :1781
        // :1782 `if (record == null || record.patch == patch) return;`
        let Some(record) = self.live_project().content_by_id(record_id) else {
            return;
        };
        if record.patch == patch {
            return;
        }
        let previous = record.patch; // :1785
        // :1786 `new DirtyFlags(record)`——只盯这一条记录。
        let mut flags = DirtyFlags::new(self.live_project(), &[Some(record)]);
        // :1787-1788 「现在就装上新标记，好让 captureAfter 看得见」。
        {
            let record = self.live_project_mut()
                .content_by_id_mut(record_id)
                .expect("record checked above");
            record.patch = patch; // :1787
            record.dirty = true; // :1788
        }
        flags.capture_after(self.live_project()); // :1789
        self.push_command(AppCommand::Patch(PatchCommand::new(
            record_id, previous, patch, flags,
        ))); // :1790
        self.fire_project_changed(); // :1805
    }

    /// 对应 `applyRawTree(ContentRecord record, JsonObject tree)`（`:1808-1842`）：把原始 JSON
    /// 面板编辑出来的那棵字段树**整体**替换到记录上。
    ///
    /// 快速连打**不**合并（`:1808` 的注释）。Java `:1814-1815` 的注释解释了两端快照为什么必需：
    /// 编辑器替换整棵树，而树里的嵌套值与结构化编辑器共享、被它们原地改——Rust 侧不存在共享子树，
    /// 但两端快照仍旧要留（[`RawTreeCommand`]），否则 undo 拿到的就是被后续编辑改过的那一棵。
    pub fn apply_raw_tree(&mut self, record_id: &str, tree: Object) {
        self.require_project(); // :1810
        // :1811 `if (record == null) return;`
        if self.live_project()
            .content_by_id(record_id)
            .is_none()
        {
            return;
        }
        let before = self.live_project()
            .content_by_id(record_id)
            .expect("record checked above")
            .tree(); // :1816 `record.tree().deepCopy()`
        if before == tree {
            return; // :1817-1819 「没变过就不留历史」
        }
        // :1820 `new DirtyFlags(record)`——只盯这一条记录。
        let mut flags = DirtyFlags::new(
            self.live_project(),
            &[Some(
                self.live_project()
                    .content_by_id(record_id)
                    .expect("record checked above"),
            )],
        );
        // :1821-1822 `record.applyTree(tree.deepCopy()); record.dirty = true;`
        {
            let record = self.live_project_mut()
                .content_by_id_mut(record_id)
                .expect("record checked above");
            record.apply_tree(tree.clone());
            record.dirty = true;
        }
        self.warn_about_reserialisation(record_id); // :1823
        flags.capture_after(self.live_project()); // :1824
        // :1825 `JsonObject after = record.tree().deepCopy();`——在 `applyTree` **之后**取的这一棵，
        // 正是 redo 要装回去的那一棵（与 `setField` 的 `newSnapshot` 同一理由，见 `:1725-1727`）。
        let after = self.live_project()
            .content_by_id(record_id)
            .expect("record checked above")
            .tree();
        self.push_command(AppCommand::RawTree(RawTreeCommand::new(
            record_id, before, after, flags,
        ))); // :1826
        self.fire_project_changed(); // :1841
    }

    // --- canvas drift ---------------------------------------------------------------------------

    /// 对应 `canvasDiff()`（`:1921-1927`）：当前画板偏离它所镜像的记录有多远。
    /// `project` 或 `board` 为 `null` 时 Java 给 `new LinkGraph.Diff(0, 0, 0)`，原样保住。
    pub fn canvas_diff(&self) -> Diff {
        let Some(project) = self.project.as_ref() else {
            return Diff {
                orphan_nodes: 0,
                missing_edges: 0,
                stale_edges: 0,
            };
        };
        let Some(index) = self.board else {
            return Diff {
                orphan_nodes: 0,
                missing_edges: 0,
                stale_edges: 0,
            };
        };
        let Some(board) = project.boards.get(index) else {
            return Diff {
                orphan_nodes: 0,
                missing_edges: 0,
                stale_edges: 0,
            };
        };
        link_graph::diff(project, board, &self.registry)
    }

    /// 对应 `canvasDrift()`（`:1929-1931`）：加载时发现、上一次重建刷新的那份漂移。
    pub fn canvas_drift(&self) -> Diff {
        self.canvas_drift
    }

    // --- small conveniences the UI needs --------------------------------------------------------
    // 对应 Java 同名段（`:2271-2309`）；`typeName`/`subtitleOf`（`:2295-2309`）依赖
    // `Messages`/`Labels` 这两张 UI 文案表，属于壳层，随 M2b 之后再说。

    /// 对应 `stringKey(ContentRecord record, String field)`（`:2274-2277`）：
    /// 一条记录四个可本地化字段之一的 bundle 键。
    pub fn string_key(&self, record: &ContentRecord, field: &str) -> String {
        self.require_project(); // :2275
        Self::string_key_of(self.live_project(), record, field)
    }

    /// 同 [`Self::string_key`]，但把项目显式收进来：Java 的 `stringKey` 读 `project` 字段，
    /// 而 `addContent` 调它的那一刻项目正被可变借用（`:698`、`:718`），Rust 得把借用拆开。
    fn string_key_of(project: &ModProject, record: &ContentRecord, field: &str) -> String {
        name_rules::bundle_key(
            record.ctype,
            &record.full_name(&project.internal_name()),
            field,
        )
    }

    /// 对应 `stringValue(ContentRecord record, String field, String locale)`（`:2279-2282`）：
    /// 没填过时 Java 拿到 `null`，这里换成空串（Java 也是 `value == null ? "" : value`）。
    pub fn string_value(&self, record: &ContentRecord, field: &str, locale: &str) -> String {
        let key = self.string_key(record, field);
        let project = self.live_project();
        project
            .strings
            .get(&key, locale)
            .map(|value| value.to_string())
            .unwrap_or_default()
    }

    /// 对应 `nodeById(String nodeId)`（`:1551-1555`）：在当前画板的卡片里按 id 找一张。
    ///
    /// Java 返回的是那张**活卡片**，Rust 返回它的副本（模块头第 2 点）；调用点只读它，
    /// 需要改的时候一律改完再写回。`board == null` 那一支 Java 给 `null`，这里给 `None`。
    fn node_by_id(&self, node_id: &str) -> Option<CanvasNode> {
        let board = self.board?;
        self.project
            .as_ref()?
            .boards
            .get(board)?
            .nodes
            .iter()
            .find(|node| node.id.as_deref() == Some(node_id))
            .cloned()
    }

    /// 对应 `contentOf(CanvasNode node)`（`:1557-1560`）：卡片绑定的那条记录；
    /// 直接绑原版名的载体卡片（原版内容）返回 `None`。
    fn content_of(&self, node: Option<&CanvasNode>) -> Option<ContentRecord> {
        // :1559 三道判空：卡片为空、`contentId` 为空、项目为空。
        let content_id = node?.content_id.as_ref()?;
        self.project.as_ref()?.content_by_id(content_id).cloned()
    }

    /// 对应 `sourceNameOf(CanvasNode node)`（`:1562-1569`）：这张卡片代表的**裸内容名**——
    /// 绑定记录的名字，或者卡片自己写着的直接引用名。Java 这里是 `public`，所以 Rust 也留 `pub`。
    pub fn source_name_of(&self, node: Option<&CanvasNode>) -> String {
        // :1564-1566 卡片为空就是空串（不是 `null`，Java 直接 `return ""`）。
        let node = match node {
            Some(node) => node,
            None => return String::new(),
        };
        match self.content_of(Some(node)) {
            Some(record) => record.name, // :1568
            None => node.ref_name.clone().unwrap_or_default(),
        }
    }

    /// 对应 `rebindCarrier(String nodeId, String newContentId, String newRefName)`
    /// （`:1590-1622`）：把一张**已经存在**的卡片改绑到另一条记录、或绑到一个直接引用名上。
    ///
    /// Java `:1571-1588` 的那段注释讲的是「为什么不重画画板」：从调色盘拖进来时卡片是空的，
    /// 用户随后把它接到别的资产上；把 `contentId` 清空、再重画整块板，会**销毁正在拖的那张连线**
    /// ——FX 拿着被摘掉的节点句柄。这条方法把绑定**原地**装上，只触发一次普通的 `fireProjectChanged()`。
    /// 这里的结构差异要登记清楚：Java 靠「不动 `nodes` 列表」保住连线，Rust 同样只写节点的两个字段、
    /// 不重建画板，所以那个性质原样成立。
    pub fn rebind_carrier(&mut self, node_id: &str, new_content_id: Option<&str>, new_ref_name: Option<&str>) {
        self.require_project(); // :1591
        // :1593-1595 `CanvasNode node = nodeById(nodeId); if (node == null) return;`
        let Some(node) = self.node_by_id(node_id) else {
            return;
        };
        let old_content_id = node.content_id.clone(); // :1596
        let old_ref_name = node.ref_name.clone(); // :1597
        // :1598-1600 两个字段都没变，一个命令都不该留。
        if old_content_id.as_deref() == new_content_id && old_ref_name.as_deref() == new_ref_name {
            return;
        }
        // :1601 `new DirtyFlags()`——改绑定不动任何记录的字段，一条记录都不盯。
        let mut flags = DirtyFlags::new(self.live_project(), &[]);
        // :1602-1603 「现在就改，好让 captureAfter 看见结果」。
        let new_content_id: Option<String> = new_content_id.map(str::to_string);
        let new_ref_name: Option<String> = new_ref_name.map(str::to_string);
        {
            let index = self.board.expect("nodeById returned a node");
            let board = self.live_project_mut()
                .boards
                .get_mut(index)
                .expect("board checked above");
            if let Some(node) = board
                .nodes
                .iter_mut()
                .find(|seen| seen.id.as_deref() == Some(node_id))
            {
                node.content_id = new_content_id.clone();
                node.ref_name = new_ref_name.clone();
            }
        }
        self.live_project_mut().boards_dirty = true; // :1604
        flags.capture_after(self.live_project()); // :1605
        // :1606 `board` 就是命令的作用板（Java 由捕获的 `node` 引用隐式携带，见 [`RebindCarrierCommand`]）。
        let board = self.board;
        self.push_command(AppCommand::RebindCarrier(RebindCarrierCommand::new(
            board,
            node_id,
            old_content_id,
            old_ref_name,
            new_content_id,
            new_ref_name,
            flags,
        )));
        self.fire_project_changed(); // :1621
    }

    /// 对应 `detachNodes(List<String> nodeIds)`（`:1628-1692`）：把**原版内容的载体卡片**从画板上
    /// 摘掉，连同它的全部连线，但**不动**卡片指向的那条记录。
    ///
    /// Java `:1624-1627` 的注释讲的是这条命令存在的理由：原版资产（`contentId == null`）没法
    /// `delete`——根本没有可删的记录——于是用户把卡片拖到垃圾桶时，只摘卡片与连线，记录留在
    /// 资产工作区里，重开文件它会回来。
    ///
    /// Java 的入参是 `List<CanvasNode>`，并且 `:1631` 允许 `nodeIds == null`；Rust 侧收 id 切片，
    /// `null` 入参对应的就是空切片，走的是同一条早退。
    pub fn detach_nodes(&mut self, node_ids: &[String]) {
        self.require_project(); // :1629
        // :1630-1632 `if (nodeIds == null || nodeIds.isEmpty() || board == null) return;`
        if node_ids.is_empty() || self.board.is_none() {
            return;
        }
        // :1633-1644 先收集，**一步都不改**：`touching` 必须在摘除之前抓全。
        let mut removed: Vec<CanvasNode> = Vec::new();
        let mut touching: Vec<Vec<CanvasEdge>> = Vec::new();
        for node_id in node_ids {
            // :1636-1638 `if (node == null) continue;`
            let Some(node) = self.node_by_id(node_id) else {
                continue;
            };
            // :1639-1643 `board.edges.stream().filter(e -> node.id.equals(e.from) || node.id.equals(e.to)).toList()`
            let edges: Vec<CanvasEdge> = self.live_project()
                .boards
                .get(self.board.expect("checked above"))
                .expect("checked above")
                .edges
                .iter()
                .filter(|edge| {
                    DetachNodesCommand::same_id(&node.id, &edge.from)
                        || DetachNodesCommand::same_id(&node.id, &edge.to)
                })
                .cloned()
                .collect();
            removed.push(node);
            touching.push(edges);
        }
        if removed.is_empty() {
            return; // :1645-1647
        }
        // :1650 `final Board commandBoard = board;`
        let board = self.board;
        // :1651 `new DirtyFlags()`——摘卡片不动任何记录的字段。
        let mut flags = DirtyFlags::new(self.live_project(), &[]);
        // :1652-1655 摘掉卡片与它们的连线。Java `:1656` 那句 `project.boardsDirty = true;`
        // 在循环**体内**，但它和循环体改的不是同一个对象、且幂等，这里提到循环之后（`:1658`
        // 本来还要再置一次），可观测结果一致。
        {
            let index = board.expect("checked above");
            let board = self.live_project_mut()
                .boards
                .get_mut(index)
                .expect("board checked above");
            for node in &removed {
                if let Some(position) = board
                    .nodes
                    .iter()
                    .position(|seen| DetachNodesCommand::same_id(&seen.id, &node.id))
                {
                    board.nodes.remove(position); // :1653
                }
                DetachNodesCommand::drop_edges_of(board, &node.id); // :1654-1655
            }
        }
        self.selection.clear(); // :1657 「被摘掉的卡片不再可选」
        self.live_project_mut().boards_dirty = true; // :1656 与 :1658
        flags.capture_after(self.live_project()); // :1659
        self.push_command(AppCommand::DetachNodes(DetachNodesCommand::new(
            board, removed.clone(), touching, flags,
        ))); // :1661
        self.log(&format!("detach {} carrier(s)", removed.len())); // :1690
        self.fire_project_changed(); // :1691
    }

    /// 对应 `referenceField(ContentRecord record, String fieldKey)`（`:1538-1549`）：
    /// 只有当这个键在该记录类型上确实是一个引用字段时才返回它。
    fn reference_field(&self, record: &ContentRecord, field_key: &str) -> Option<FieldDef> {
        // :1539-1542 类解析不出来就没有字段可言。
        // 2026-11-26（R-02）：类名改走有效类——脚本 `extend` 推断出的子类同样有引用字段，
        // 按 `Block` 查会让载荷/工厂连线的引用槽认不出来。
        let EffectiveClass { class_name, .. } = self.effective_class_of(record);
        if class_name.is_empty() {
            return None;
        }
        // :1543-1547 名字先匹配上，再看它是不是引用字段：不是就返回 null 并**停住**，
        // 不会继续往后找同名的其它字段。`find` + `filter` 与这个「一旦同名就定胜负」一致，
        // 因为全名唯一（Java 的字段表按名索引）。
        self.registry
            .fields_of(&class_name)
            .iter()
            .find(|def| def.name() == field_key)
            .filter(|def| def.is_reference())
            .cloned()
    }

    /// 对应 `entryKeyOf(FieldDef field)`（`:1571-1584`，Java 是 `private static`）：
    /// 结构化字段的第一个槽位该由什么键开头（"factory block" / "unit"），用在连线的失败提示里。
    fn entry_key_of(field: &FieldDef) -> &'static str {
        // :1576-1583 `stackKind()` 为 null 时两次 `equals` 都是 false，落到最后一个分支。
        match field.stack_kind() {
            Some("unitReq") => "factory block",
            Some("plan") => "unit",
            _ => "reference",
        }
    }

    /// 一条记录的有效类与来源。2026-11-26（产品评审 R-02）新增：Java `classNameOf` 只有
    /// 「声明 `type` → 类型默认类」两级，这里插入脚本 `extend(<Class>, '<名>')` 那一级，
    /// 规则与偏离理由全在 `crate::class_resolution` 模块头，控制器的其他读取点一律走这里，
    /// 不再各自调 `resolve_class`。
    pub fn effective_class_of(&self, record: &ContentRecord) -> EffectiveClass {
        crate::class_resolution::effective_class(self.project(), record)
    }

    /// 对应 `classNameOf(ContentRecord record)`（`:2284-2287`）：解析不出类时回落到类型名。
    ///
    /// 2026-11-26（R-02）：类名从 `resolve_class` 的两级改成有效类的三级（见
    /// [`Self::effective_class_of`]）。回落分支与 Java 逐字同一：只有 `default_class()` 为空的
    /// 类型（weather）才走得到。**不写回模型**——`record.class_name()` 仍是磁盘上的 `type`。
    pub fn class_name_of(&self, record: &ContentRecord) -> String {
        self.class_name_and_source_of(record).0
    }

    /// 有效类名（含 Java 的「回落类型名」兜底）＋它的来源。DTO 层要同时下发这两样，
    /// 这里一次算完，避免「两个方法各解析一遍」出现口径漂移。
    pub fn class_name_and_source_of(
        &self,
        record: &ContentRecord,
    ) -> (String, crate::class_resolution::ClassSource) {
        let EffectiveClass { class_name, source } = self.effective_class_of(record);
        if class_name.is_empty() {
            (record.ctype.name().to_string(), source)
        } else {
            (class_name, source)
        }
    }

    /// 对应 `displayName(ContentRecord record)`（`:2290-2293`）：卡片和列表显示的名字——
    /// 英文文案，没有就用记录名。Java 用 `value.isBlank()`，即全空白也算没有。
    pub fn display_name(&self, record: &ContentRecord) -> String {
        let value = self.string_value(record, "name", "en");
        if value.trim().is_empty() {
            record.name.clone()
        } else {
            value
        }
    }

    /// 对应 `typeName(ContentType type)`（`:2296-2298`）：`Messages.t("ctype." + type.name())`。
    ///
    /// Java 按界面语言查 bundle；这里与 `Issue::message()` 同一口径，固定取英文底账
    /// （`messages.properties:133-144`），12 个键与 [`ContentType`] 的 12 个变体一一对应，
    /// 不存在缺键回落。与 Java 切语言唯一偏差，登记在计划文档 M5 签字条目。
    pub fn type_name(&self, ctype: ContentType) -> &'static str {
        match ctype {
            ContentType::Item => "Item",
            ContentType::Block => "Block",
            ContentType::Liquid => "Liquid",
            ContentType::Status => "Effect",
            ContentType::Unit => "Unit",
            ContentType::Weather => "Weather",
            ContentType::Sector => "Sector",
            ContentType::Planet => "Planet",
            ContentType::Team => "Team",
            ContentType::Bullet => "Bullet",
            ContentType::UnitCommand => "Command",
            ContentType::UnitStance => "Stance",
        }
    }

    /// 对应 `subtitleOf(ContentRecord record)`（`:2301-2309`）：卡片副标题——类型标签，
    /// 子类再多拼一段 `" · " + Labels.clazz(className)`。
    ///
    /// `Labels.clazz`（`Labels.java:19-21`）查 class-labels 表，而英文底账**按设计为空**
    /// （`class-labels.properties:1`：unknown classes fall back to their raw class name），
    /// 未命中回原类名本身，所以这里直接回 `class_name`，不搬那张空表。
    /// 类名解析走 [`Self::class_name_of`]，即 Java 的 `classNameOf(record)`（`:2284-2287`）；
    /// Java `:2303` 的 `baseClassOf` 可能为 null，`String.equals(null)` 恒 false，
    /// 对位到 Rust 就是 `Option` 与 `Some(class_name)` 的比较。
    pub fn subtitle_of(&self, record: &ContentRecord) -> String {
        let class_name = self.class_name_of(record);
        let base = self.registry.base_class_of(record.ctype);
        // Java `:2304`：类型与类名说的是同一件事时不重复，只有子类才把类名写出来。
        if base.as_deref() == Some(class_name.as_str()) || class_name == record.ctype.default_class() {
            return self.type_name(record.ctype).to_string();
        }
        format!("{} · {}", self.type_name(record.ctype), class_name)
    }

    // --- 私有小工具 ------------------------------------------------------------------------------

    /// 对应 `writeSavePlan(ProjectIo.SavePlan plan)`（`:2262-2269`）：写一份保存计划，
    /// 写不完就把「未保存」标记放回去。Java 的 `catch (IOException failure) { plan.rollback();
    /// throw failure; }` 在 Rust 就是错误分支里补一次回滚后原样返回错误。
    fn write_save_plan(&mut self, plan: &SavePlan) -> Result<(), BoxError> {
        match project_io::write(plan) {
            Ok(()) => Ok(()),
            Err(failure) => {
                if let Some(project) = self.project.as_mut() {
                    plan.rollback(project);
                }
                Err(failure.into())
            }
        }
    }

    /// 对应 `warnAboutReserialisation(ContentRecord record)`（`:2331-2337`）：改一个从 mod 里
    /// 来的文件会把它重新序列化成标准 JSON——原文里的注释与排版对这个文件就没了。
    /// Java 的注释 `:2329-2330` 写明「告诉用户一次，不是每次编辑都告诉」，所以 `reserializeNotice`
    /// 一旦置上就不再重复。Java 直接拿 `record` 引用；这里先按 id 读名字与 `raw`（读不到记录
    /// 就是 Java 里 `record == null` 的分支之外的状态——那条路径上调用方刚改完这条记录，
    /// 不可能已经不在项目里，`return` 只是把 Java 的 NPE 变成无事发生）。
    fn warn_about_reserialisation(&mut self, record_id: &str) {
        let name = {
            let project = match self.project.as_ref() {
                Some(project) => project,
                None => return,
            };
            let record = match project.content_by_id(record_id) {
                Some(record) => record,
                None => return,
            };
            // :2332 `!project.reserializeNotice && record.raw != null`
            if project.reserialize_notice || record.raw.is_none() {
                return;
            }
            record.name.clone()
        };
        self.live_project_mut().reserialize_notice = true; // :2333
        self.log(&format!( // :2334-2335
            "note: editing '{}' re-serialises its file as standard JSON - comments in the original text are lost for that file",
            name
        ));
    }

    /// 壳直接落盘资产并同步 `project.assets` 之后调它作废校验缓存（2026-11-25，
    /// `sprite_import` 命令）。「缺贴图」一族结论（`mm_fmt::sprite::without_sprite`）
    /// 依赖资产清单，而资产导入不走任何会 `fire_project_changed` 的编辑命令——
    /// 不作废的话旧结论会一直活到下一次模型编辑。等价 Java `fireProjectChanged()`
    /// 的第一件事（`:2315`）；广播那一半仍由壳持有（见 [`Self::fire_project_changed`]）。
    pub fn note_assets_changed(&mut self) {
        self.fire_project_changed();
    }

    /// 对应 `fireProjectChanged()`（`:2311-2327`）。
    ///
    /// Java 这个方法做两件事：先 `validation.invalidate()`（`:2315`，注释 `:2312-2314` 说明
    /// 必须在广播之前失效——「监听器（以及它们读到的一切）绝不能看到上一个状态的结论，
    /// 无论这次改动来自哪个线程」），再把 `projectListeners` 在 FX 线程上跑一遍
    /// （`:2316-2326`，无头测试里退回就地执行）。
    /// Rust 侧只有前者落在这个 crate：[`ValidationSession::invalidate`] 就在下面这行；
    /// 后者是 `src-tauri` 的 `emit` + 前端 `listen`（见结构体注释），这里保留 Java 的调用点与顺序。
    fn fire_project_changed(&mut self) {
        self.validation.invalidate(); // :2315
    }

    /// 对应 `requireProject()`（`:2339-2343`）：Java 抛非受检的 `IllegalStateException`，
    /// 消息是 `no project is open`。Rust 没有受检异常，这类「编程错误」直接用 panic 等价表达。
    /// 命令路径上的模型访问：`require_project()` 之后调用，是唯一的 panic 点。
    ///
    /// 2026-11-20：原先 79 处 `self.project.as_ref()/as_mut().unwrap()` 各自 panic，
    /// 消息只有 `called Option::unwrap()`；现在收敛成这两个 `#[track_caller]` 入口，
    /// 崩溃点与文案都只有一份，`RUST_BACKTRACE` 里能看到真正的调用方。
    /// 类型层面仍是 panic（等价 Java 的 `IllegalStateException`），命令层
    /// （`src-tauri/src/bridge.rs`）在投递前就查过「有没有项目」，所以它在 IPC 路径上不可达。
    #[track_caller]
    fn live_project(&self) -> &ModProject {
        self.project.as_ref().expect("no project is open (require_project() must run first)")
    }

    #[track_caller]
    fn live_project_mut(&mut self) -> &mut ModProject {
        self.project.as_mut().expect("no project is open (require_project() must run first)")
    }

    fn require_project(&self) {
        if self.project.is_none() {
            panic!("no project is open");
        }
    }

    /// 对应私有 `uniqueBoardFile(String name)`（`:2363-2381`）：给新画板挑一个别家没占用的
    /// canvas 文件名（`:2358-2362` 的注释解释了为什么必须唯一：否则两块板每次保存都互相覆盖）。
    ///
    /// 零新增依赖 ⇒ 没有正则库，`:2367` 那三个连写的 `replaceAll` 用一次字符分类扫描手写出来，
    /// 每个手写步骤都在行尾注明它对应哪一个 `replaceAll`。
    fn unique_board_file(&self, name: &str) -> String {
        let project = self.live_project(); // :2365 直接读 `project` 字段，为 null 时 Java 就 NPE
        // :2364 `Set<String> taken = new LinkedHashSet<>();`：零依赖，用 Vec 做保序去重的等价物。
        let mut taken: Vec<&String> = Vec::new();
        for existing in &project.boards {
            // :2365 `taken.add(existing.file)`（Set 的重复 add 是 no-op）
            if !taken.contains(&&existing.file) {
                taken.push(&existing.file);
            }
        }
        // :2366 `name.toLowerCase(Locale.ROOT)` → `to_ascii_lowercase()`：只有非 ASCII 大写字母上
        // 有差别（Java 会折成小写、可能因此落进 `[a-z]`），而下一步凡是落不进 `[a-z0-9\-_]` 的
        // 字符都要被塌成 `-`，两种折叠在这一串里同结果。`name == null ? ""` 那半句在 Rust 里无处安放：
        // 参数是 `&str`，不可能是 null。
        let lowered = name.to_ascii_lowercase();
        // :2367 第一个 `replaceAll("[^a-z0-9\\-_]+", "-")`：一段**连续**的非法字符塌成**一个** `-`。
        let mut slug = String::new();
        let mut in_replaced_run = false;
        for ch in lowered.chars() {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_' {
                slug.push(ch);
                in_replaced_run = false;
            } else if !in_replaced_run {
                slug.push('-');
                in_replaced_run = true;
            }
        }
        // :2367 第二个 `replaceAll("^-+", "")`：去掉开头**所有** `-`（不是一个）。
        let slug = slug.trim_start_matches('-');
        // :2367 第三个 `replaceAll("-+$", "")`：去掉结尾**所有** `-`。
        let mut base = slug.trim_end_matches('-').to_string();
        if base.is_empty() || name_rules::is_windows_reserved_name(&base) {
            base = "board".to_string(); // :2368-2370
        }
        let max_stem = 120 - ".canvas".len(); // :2371
        // :2372-2373 `base.substring(0, maxStem)`：Java 按 UTF-16 码元切，Rust 按字节切会劈开多字节
        // 字符，所以这里按**字符**切。此时 base 只剩 `[a-z0-9-_]`（全 ASCII），两种切法等价。
        if base.chars().count() > max_stem {
            base = base.chars().take(max_stem).collect();
        }
        let mut candidate = format!("{base}.canvas"); // :2375
        let mut index = 2usize; // :2376 `int index = 2;`
        while taken.contains(&&candidate) {
            // :2377-2379 `candidate = base + "-" + index++ + ".canvas";`
            candidate = format!("{base}-{index}.canvas");
            index += 1;
        }
        candidate // :2380
    }
}

