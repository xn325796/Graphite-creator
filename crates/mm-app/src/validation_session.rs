//! `ValidationSession` 的等价移植（Java `dev.modmaker.ui.ValidationSession`，248 行）。
//!
//! 对应 Java 类注释 `ValidationSession.java:20-34`：这一类管的是「项目的校验结论——什么被缓存、
//! 什么时候重算、以及一次校验怎么离开 JavaFX 线程」。它刻意把自己与 [`ProjectController`] 分开，
//! 让「同一次校验服务所有面板」这件事有一处唯一的所有者。
//!
//! [`ProjectController`]: crate::project_controller::ProjectController
//!
//! ## 与 Java 的三处结构性差异
//!
//! 1. **`Validator` 与 `projects` 两个字段（`:48-49`）不存进结构体**。Java 的构造器收
//!    `SchemaRegistry` 与 `Supplier<ModProject>`；Rust 侧 [`Validator`] 按生命周期借用
//!    `SchemaRegistry`，而模型由控制器独占（见 `project_controller` 模块头），两者都没法既被结构体
//!    持有、又在方法调用时和控制器自己的借用并存——那是自引用。所以这里把它们做成
//!    [`current`]／[`force_sync`]／[`validate_snapshot`] 三个方法的显式参数，语义一模一样：
//!    「用当前 registry 校验当前模型」。
//! 2. **「同一份结论」用 `Rc` 表达**。Java 的 `assertSame(first, controller.validateQuietly())`
//!    靠的是对象同一性——缓存命中时返回的就是同一个 `List` 实例。Rust 侧 `Vec<Issue>` 每次返回都是
//!    新拷贝，同一性得靠 [`Rc`]：`cache` 存 `Rc<Vec<Issue>>`，命中时克隆 `Rc`（引用计数 +1，
//!    不复制结论），测试用 [`Rc::ptr_eq`] 断言。这里没有 `Rc<RefCell<…>>`——`Rc` 里面是**只读的
//!    不变数据**，发布之后就再也不改（Java 那边 `List.copyOf` 同样是发布即不可变，`:89`、`:128`）。
//! 3. **后台模式不落地**（`DEBOUNCE`/`useWorker`/`schedule`/`capture`/`runPass`/`publish`/
//!    `onFxThread`/`refreshAsync`，`:38-46`、`:66-72`、`:134-157`、`:169-244`）。Java 那一半
//!    存在的唯一理由就是「别让几十毫秒的规则集卡住 FX 线程」（`:22-31`）。Rust 侧没有 FX 线程：
//!    `src-tauri` 的命令本来就跑在 Tauri 自己的工作线程上，模型快照也在命令层做；把一条 150 毫秒的
//!    去抖定时器搬进这个 crate 需要线程 + 通道（零新增依赖的前提下只能手写），而它要解决的问题在这边
//!    不存在。落地的是 Java `:57` 那行注释写明的另一极——**`worker` 为空时全部同步**
//!    （`schedule()` `:169-172` 直接 `return`，`refreshAsync()` `:138-142` 退回 `forceSync()`），
//!    也就是 Java 自己给「无头调用方与测试」用的那一条路径。于是 `pending` 恒为 `false`，
//!    与 Java 无 worker 时的取值一致。
//!
//! 监听器扇出（`refreshedListeners` `:50`、`onRefreshed` `:74-77`、`reset` 里 `:117` 的广播）
//! 同 `ProjectController.projectListeners` 一样交给 `src-tauri` 的 `emit` + 前端 `listen`，
//! 不在这个 crate 里持回调表。
//!
//! [`Validator`]: mm_validate::validator::Validator
//! [`current`]: ValidationSession::current
//! [`force_sync`]: ValidationSession::force_sync
//! [`validate_snapshot`]: ValidationSession::validate_snapshot

use std::rc::Rc;

use mm_schema::SchemaRegistry;
use mm_validate::{issue::Issue, validator::Validator};
use mm_model::ModProject;

/// 一次校验的结论。Java 的 `List.copyOf(validator.validate(project))`（`:89`、`:94`、`:128`、
/// `:166`、`:220`）产出的是不可变列表，这里用 `Rc<Vec<Issue>>` 承担「同一份结论被多个面板共享」
/// 这一件事；共享之外它和 `Vec<Issue>` 没有区别。
pub type Findings = Rc<Vec<Issue>>;

/// 对应 Java `ValidationSession`（`:35` 类声明）。
///
/// 字段逐个落到下面：`cache`（`:54`）、`published`（`:56`）、`pending`（`:59`）、
/// `generation`（`:51`）。`validator`（`:48`）与 `projects`（`:49`）见模块头第 1 点，
/// `worker`（`:58`）见模块头第 3 点——它在这份移植里恒为「空」，所以不再需要一个字段来表达。
pub struct ValidationSession {
    /// 对应 `cache`（`:53-54`）：「当前模型状态的结论；一次后台校验在跑时为 null」。
    /// 没有后台模式，于是 `None` 只意味着「还没算过／已被 `invalidate` 清掉」。
    cache: Option<Findings>,
    /// 对应 `published`（`:55-56`）：「最后一次真正算出来的结论；永不为 null，可能是旧的」。
    published: Findings,
    /// 对应 `generation`（`:51`）：Java 用 `AtomicLong` 是因为后台线程要读它（`:191-193`、
    /// `:211-213`、`:229-233`）。这里全是同步调用，一个普通计数器等价——它仍然如实记录
    /// 「结论被作废了几次」，`invalidate`／`reset`／一次同步校验都要动它（`:93`、`:107`、
    /// `:115`、`:129`）。
    generation: u64,
    /// 对应 `pending`（`:59`）：恒为 `false`，见模块头第 3 点。`pending()` 保留是为了让视图层
    /// 拿到与 Java 相同的接口（`:159-161` 的「checking...」指示灯在 Rust 侧永远是灭的）。
    pending: bool,
}

impl Default for ValidationSession {
    /// 同 [`ValidationSession::new`]：对应 Java 的字段初始化器（`:56`、`:51`、`:59`）。
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationSession {
    /// 对应 Java 构造器（`:61-64`）。
    ///
    /// Java 的两行方法体是 `this.validator = new Validator(registry);` 与
    /// `this.projects = projects;`——两者在这里都改成了调用时显式传入（模块头第 1 点），
    /// 所以这个构造器只剩字段初始化器：`published = List.of()`（`:56`）对应空 `Rc`，
    /// `generation` 从 0 起（`:51` 的 `new AtomicLong()`），`cache`/`worker` 为空（`:54`、`:58`），
    /// `pending` 为 `false`（`:59`）。
    pub fn new() -> Self {
        Self {
            cache: None,
            published: Rc::new(Vec::new()),
            generation: 0,
            pending: false,
        }
    }

    /// 对应 `current()`（`:79-96`）：「当前模型的结论，需要时就地算」。
    ///
    /// Java 的注释（`:78`）与 `:24-27` 都在强调同一条契约：**缓存为空时就地同步计算**，
    /// 这样「拥有模型的调用方（CLI 式测试、退出闸门）立刻看到新鲜结论」。
    ///
    /// `project` 为 `None` 对应 Java 的 `projects.get() == null`（`:85-87`）：直接返回空结论，
    /// **不碰缓存**。那一支返回的 `List.of()` 在 Java 是共享的不可变空表常量，这里每次新建一个
    /// `Rc`——两条路径都不写缓存，所以「同一性」在这一点上没有可观测差异。
    pub fn current(
        &mut self,
        registry: &SchemaRegistry,
        project: Option<&ModProject>,
    ) -> Findings {
        // :81-84 命中缓存就原样交出去（同一个实例，`Rc::clone` 只加引用计数）。
        if let Some(cached) = &self.cache {
            return Rc::clone(cached);
        }
        let Some(project) = project else {
            return Rc::new(Vec::new()); // :85-87
        };
        // :89 `List.copyOf(validator.validate(project))`。
        let computed: Findings = Rc::new(Validator::new(registry).validate(project));
        // :90-95 注释写明「一次就地校验比任何在飞的后台结果都新：取消那条后台的」。
        // 没有后台任务，于是这里的「取消」就是推进代号 + 清 pending，与 Java 逐句对应。
        self.generation += 1;
        self.pending = false;
        self.cache = Some(Rc::clone(&computed));
        self.published = Rc::clone(&computed);
        computed
    }

    /// 对应 `latest()`（`:98-102`）：「最后一次算出来的结论；永不阻塞，`pending()` 时可能是旧的」。
    ///
    /// Java 的两行就是 `cache == null ? published : cache`；这里同为三元取值。
    pub fn latest(&self) -> Findings {
        match &self.cache {
            Some(cached) => Rc::clone(cached),
            None => Rc::clone(&self.published),
        }
    }

    /// 对应 `invalidate()`（`:104-109`）：模型变了，作废缓存。
    ///
    /// Java 的三行是 `cache = null;`、`generation.incrementAndGet();`、`schedule();`。
    /// 第三条在没有 worker 时立刻 `return`（`:169-172`），所以这里不写它的等价物：
    /// 「下一次读结论时会重算」这件事由 `cache = None` 单独保证。
    /// 这是 [`ProjectController::fire_project_changed`] 的第一件事（Java `:2315`）：
    /// 广播之前先失效，监听器绝不能读到上一状态的结论（Java `:2312-2314` 的注释）。
    ///
    /// [`ProjectController::fire_project_changed`]: crate::project_controller::ProjectController
    pub fn invalidate(&mut self) {
        self.cache = None; // :106
        self.generation += 1; // :107
                             // :108 `schedule();` —— Java `:169-172`：无 worker 即直接返回。
    }

    /// 对应 `reset()`（`:111-118`）：「把一切忘掉，包括已发布的结论；新打开的项目没有结论」。
    ///
    /// Java 打开项目、新建项目、关闭项目时都调它。与 `invalidate` 的区别就在 `published`
    /// 也要清空（`:114`），以及最后那句广播（`:117`，监听器改由 `emit` 承担）。
    pub fn reset(&mut self) {
        self.cache = None; // :113
        self.published = Rc::new(Vec::new()); // :114
        self.generation += 1; // :115
        self.pending = false; // :116
                              // :117 `refreshedListeners.forEach(Runnable::run);` → `src-tauri` 的 emit。
    }

    /// 对应 `forceSync()`（`:120-132`）：一次显式的「现在就校验」，**绕过缓存**。
    ///
    /// 这是 F5／构建闸门走的那条路（Java `ProjectController.validate()` `:2015-2021`）。
    /// 与 `current()` 的差别只有一处：它不看缓存，永远重算，因此返回的 `Rc` 必然是一个新实例
    /// （测试 `#33` 的 `assertNotSame(first, forced)` 断言的就是这一点）；算完再把缓存与
    /// 已发布结论一起换掉（`:128-130`）。
    ///
    /// `project` 为 `None` 时同 Java `:122-124`：返回空结论，**不写缓存**。
    pub fn force_sync(
        &mut self,
        registry: &SchemaRegistry,
        project: Option<&ModProject>,
    ) -> Findings {
        let Some(project) = project else {
            return Rc::new(Vec::new()); // :122-124
        };
        // :125 `List.copyOf(validator.validate(project))`。
        let findings: Findings = Rc::new(Validator::new(registry).validate(project));
        // :126-130 「取代任何已排队的校验」：推进代号、清 pending、两份字段一起换。
        self.generation += 1;
        self.pending = false;
        self.cache = Some(Rc::clone(&findings));
        self.published = Rc::clone(&findings);
        findings
    }

    /// 对应 `pending()`（`:159-161`）：「有后台校验在排队或在跑」——这份移植里恒为 `false`
    /// （模块头第 3 点），与 Java 未装 worker 时同一个值。
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// 对应 `validate(ModProject snapshot)`（`:164-167`）：「校验器本身，给那些自己拿快照的调用方」。
    ///
    /// Java 的注释写明它是给 shell 的 io worker 用的（`ProjectController.validateSnapshot`
    /// `:2042-2048`）。这一条**不碰任何缓存**：一次性的就地校验，算完就交。
    pub fn validate_snapshot(registry: &SchemaRegistry, snapshot: &ModProject) -> Findings {
        Rc::new(Validator::new(registry).validate(snapshot)) // :166
    }

    /// 对应 `generation` 的取值（`:51` 字段本身，Java 没有公开 getter，只在内部比对）。
    ///
    /// 这里放开是为了让测试能真的断言「`invalidate` 确实推进了代号」——Java 那边这个性质由
    /// 后台任务的代号比对间接保证，后台模式不落地后就只剩这一处可观测的地方。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 两份结论是否为「同一个实例」，对应 Java 的 `assertSame`／`assertNotSame`。
    pub fn same(a: &Findings, b: &Findings) -> bool {
        Rc::ptr_eq(a, b)
    }
}
