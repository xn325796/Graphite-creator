//! 壳线程桥：把非 `Send` 的 [`ProjectController`] 关在一个专属线程里，命令走通道进出。
//!
//! ## 为什么不是 `State<Mutex<ProjectController>>`
//!
//! 方案文档 §三 D5 原本写着「代价只是 `lib.rs` 多写一层 `State<Mutex<…>>` 包装」，
//! 2026-11-14 实测**不成立**，已在原文就地更正。`ProjectController` 不是 `Send`：
//! - `log_sink: Option<Box<dyn Fn(&str)>>`（`project_controller.rs:1495`）与
//!   `overwrite_approver: Option<Box<dyn Fn(&Path) -> bool>>`（`:1500`）都没有 `+ Send`；
//! - 校验结论是 `Findings = Rc<Vec<Issue>>`（`validation_session.rs:50`），`Rc` 不是 `Send`。
//!
//! 于是 `Mutex<ProjectController>` 不满足 `State` 的 `Send + Sync + 'static` 要求，
//! 编译期就过不去。这里按 D5 预留的退路走 **actor 线程**：控制器只在壳线程上被碰，
//! Tauri 的工作线程把「要做的事」当成闭包投过来，结果用一次性回话通道送回。
//! 零新增依赖——只用 `std::sync::mpsc` 与 `std::thread`。
//!
//! ## 事件扇出（Java 的三组回调在这里接线）
//!
//! Java `ProjectController` 有 `onProjectChanged(Runnable)`、`onSelectionChanged(Consumer<ContentRecord>)`、
//! `setLogSink(Consumer<String>)` 三组回调，`fireProjectChanged()`（`:2311` 段）会先
//! `validation.invalidate()` 再广播；`ValidationSession` 另有 `onValidationRefreshed(Runnable)`（`:74-77`），
//! 而它真正广播的地方**只有 `reset()`（`:117`）与 worker 的 `publish()`**——同步模式下
//! `current()`/`forceSync()` 换了 `published` 也不广播。这份移植不落地后台模式，所以：
//!
//! - `project-changed`：控制器里那条监听器表没有搬到 `mm-app`（模块头写明交给壳），
//!   所以由壳在**会改模型或改画板的命令**之后补发，载荷为空（对齐 Java 的 `Runnable`）；
//! - `validation-refreshed`：判据是「`published` 那个实例真的换了」。壳在命令前后各取一次
//!   `latest_issues()`，用 [`ValidationSession::same`]（`Rc::ptr_eq`）比。命令后取的时候缓存已被
//!   `invalidate()` 清空，`latest()` 落到 `published`——普通编辑因此**不会**误发这条事件，
//!   只有 `publish_project`/`reset_validation`/`publish_import` 这些走到 `reset()` 的路径会发。
//!   这条判据与 Java 的广播点一一对应，不是「每次改动都假发一条」。
//! - `selection-changed`：载荷可空。Java `NodeCanvasPane.java:1189`、`:1383` 走 `controller.select(null)`，
//!   `SelectionModel.java:26-29` 无条件 notify，所以取消选择也要发一条带 `null` 的事件；
//!   为此 M4 把 `SelectionModel::select`／`ProjectController::select` 的入参改成 `Option`
//!   （Java `select(null)` 的字面等价物），登记在 M4 签字条目。
//! - `game-log`：`set_log_sink` 的闭包在**壳线程内**构造（它只需要 `AppHandle`，那是 `Send + Clone`），
//!   正好绕开「`Box<dyn Fn(&str)>` 不是 `Send`」这条限制。
//!   > **2026-11-17 就地更正**：逐行 emit 改为攒批——日志线进共享通道，`mm-log` 冲刷线程
//!   > 把队列里已有的线一次性发出（载荷从单行 `String` 变为 `Vec<String>`）。突发日志
//!   > （导入期数百行）不再逐行跨 IPC；单行日志不额外等待。前端尚无监听者，契约变更无消费方受影响。
//! - `import-progress`：`ImportProgress` 适配器同样在壳线程内构造，见 [`EmitProgress`]。
//! - `window-close-requested`（2026-11-25，事件表 5 → 6）：原生 X／Alt+F4 被
//!   `on_window_event` 拦下（`api.prevent_close()`）后广播，空载荷，口径同上。
//!   前端收到后**立即**调 `window_close_hold`（[`Actor::window_close_hold`]，同日契约
//!   扩展）摘除该代兜底计时，dirty 对话框获得无限决策时间；确认放弃／已保存后调
//!   `window_confirm_close`（[`Actor::window_confirm_close`]）真正关窗，取消则不调命令。
//!   无人调 hold（旧前端没有监听者）时 3s 兜底照旧放行（`lib.rs` 的计时线程，秒数与
//!   理由在 `close_guard::CLOSE_FALLBACK_SECS`）。代际状态机在 tauri-free 的
//!   [`crate::close_guard`]。
//!
//! ## panic 隔离
//!
//! > **2026-11-17 就地更正（原文一字不改地留在下方）**：原文「壳继续活着」只保了线程，
//! > 没保状态——panic 可能停在 `set_field`/`delete_contents` 中途，模型半更新，后续命令
//! > 会在脏状态上继续跑；且变更命令若在 `after_mutation` 清计划之前 panic，`build_plan`
//! > 仍是旧模型的产物，`write_build` 会写出过期包。现在 panic 后壳线程仍活着，但置
//! > `poisoned` 标记：除 `project_open`（从盘上重载即恢复）外的命令一律拒绝，同时清掉
//! > `build_plan`。
//!
//! 控制器里有 `require_project()` 这类会 panic 的守卫。命令在投过来之前一律先查「有没有项目」，
//! 但为了不让一次意外 panic 把整个壳线程带走，回话闭包外层再套一层 [`std::panic::catch_unwind`]：
//! panic 时回话通道的那端 `Sender` 被丢弃，调用方拿到一条错误文本，壳继续活着。
//!
//! ## 导入的 IO 线程（2026-11-17）
//!
//! M4 明账第 3 条「Java 的 io worker 线程这一层不落地」在此落地：读包/拆包
//! （`project_actions::read_import_with`，只读注册表、不碰活模型）移到专属 `mm-import`
//! 线程，壳线程只负责发布（`publish_import`）。读包期间画布快照、选中、撤销等命令照常
//! 处理，不再被长导入冻结。注册表在 IO 线程上重新 `load_default()`（`SchemaRegistry`
//! 含 `Rc` 缓存跨不了线程，schema 是静态文件，重读一次代价远小于读包本身）。
//! 取消按钮同时接上：第 21 条命令 `import_cancel` 置共享原子标志，
//! `EmitProgress::is_cancelled` 读它（M4 明账第 4 条「恒 false」就此更正）。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use mm_app::project_actions::{self, ImportResult};
use mm_app::project_controller::ProjectController;
use mm_app::validation_session::{Findings, ValidationSession};
use mm_io::import::progress::{worth_reporting, ImportProgress, Stage};
use mm_io::import::ImportError;
use mm_model::ContentRecord;
use mm_schema::{KeyedMap, SchemaRegistry};

use crate::dto;

/// 事件名（Java 回调 → Tauri event 的对应表见模块头）。
pub const EVENT_PROJECT_CHANGED: &str = "project-changed";
pub const EVENT_SELECTION_CHANGED: &str = "selection-changed";
pub const EVENT_VALIDATION_REFRESHED: &str = "validation-refreshed";
pub const EVENT_GAME_LOG: &str = "game-log";
pub const EVENT_IMPORT_PROGRESS: &str = "import-progress";
/// 原生关窗被拦下时广播（2026-11-25，P1-4）；空载荷，前端确认后调 `window_confirm_close`。
pub const EVENT_WINDOW_CLOSE_REQUESTED: &str = "window-close-requested";

/// 命令出错时回给前端的结构化错误（Tauri 的 `Result<_, ShellError>` 通道）。
///
/// > **2026-11-17 就地更正（原文一字不改地留在下方）**：原「不做结构化错误码」改为
/// > `{code, message}`——纯文本让前端分不清「还没有打开项目」（应静默降级成空画布）
/// > 与 panic／IO 错误，只能整体吞错。`code` 是稳定枚举串（no-project / not-ready /
/// > poisoned / panic / shell-exited / not-found / import-busy / io / domain），
/// > `message` 保留原文案。前端取 message 显示（`src-vue/lib/errors.js`）。
///
/// 命令出错时回给前端的文本（Tauri 的 `Result<_, String>` 通道）。
/// 就是 Java 那边往 UI 上弹的 `e.toString()`，不做结构化错误码。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ShellError {
    pub code: &'static str,
    pub message: String,
}

impl ShellError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    /// 领域层错误（控制器／IO）：文案原样保留，统一归 domain 类。
    fn domain(err: impl std::fmt::Display) -> Self {
        Self::new("domain", err.to_string())
    }

    /// 壳未就绪：有启动错误就回启动错误，没有就是裸的「壳未就绪」。
    fn not_ready(startup: &Option<ShellError>) -> Self {
        startup.clone().unwrap_or_else(|| Self::new("not-ready", "壳未就绪"))
    }
}

/// 读包/打开失败 → 前端错误。`ImportError::Encoding` 单独归到稳定码 `encoding`
/// （v1 口径：非 UTF-8 文本文件显式拒绝，不能与普通 IO 错误混成同一个分支）。
fn import_error(err: ImportError) -> ShellError {
    match err {
        ImportError::Encoding(message) => ShellError::new("encoding", message),
        other => ShellError::domain(other),
    }
}

/// 控制器错误包装了 `ImportError`（`open_project` 走 `?` 装箱），把编码失败从 `BoxError`
/// 里捞回来；其余照旧归 `domain`。
fn project_error(err: mm_app::project_controller::BoxError) -> ShellError {
    if let Some(ImportError::Encoding(message)) = err.downcast_ref::<ImportError>() {
        return ShellError::new("encoding", message.clone());
    }
    ShellError::domain(err)
}

/// 贴图导入失败 → 前端稳定码（2026-11-25，`sprite_import`）。分类在领域侧
/// （`sprite_asset::SpriteImportError`，tauri-free 可测），这里只做码的映射：
/// 非 PNG 归 `encoding`——与「非 UTF-8 文本显式拒绝」同族的内容格式拒收，
/// 不与普通 IO 错误（`io`）混成同一个分支。
fn sprite_import_error(err: crate::sprite_asset::SpriteImportError) -> ShellError {
    use crate::sprite_asset::SpriteImportError as E;
    let code = match &err {
        E::NoProject => "no-project",
        E::RecordNotFound(_) | E::SourceMissing(_) => "not-found",
        E::NotPng(_) => "encoding",
        E::Unimportable(_) => "domain",
        E::Io(_) => "io",
    };
    ShellError::new(code, err.to_string())
}

/// undo/redo 执行失败 → 前端稳定码（2026-11-25，贴图导入纳入 undo 栈）。
/// 分类在领域侧（`mm_app::undo_manager::CommandError`），这里只做码的映射：
/// 记录被带外删除 → not-found（与 `sprite_import` 的 RecordNotFound 同族），
/// 磁盘写失败 → io，领域守卫拒收 → domain。失败时历史不被破坏（条目留栈可重试），
/// 所以这些码对前端是「这次撤销没生效，可以再试」，不是「历史坏了」。
fn command_error(err: mm_app::undo_manager::CommandError) -> ShellError {
    use mm_app::undo_manager::CommandError as E;
    let code = match &err {
        E::NotFound(_) => "not-found",
        E::Io(_) => "io",
        E::Domain(_) => "domain",
    };
    ShellError::new(code, err.to_string())
}

/// 投给壳线程的一次请求。
///
/// 闭包要求 `Send`——它要跨线程从 Tauri 的工作线程搬到壳线程；它捕获的只有命令行参
/// （都是 `String`/`Option<String>`/`Vec<String>` 这类 `Send` 值）与回话 `Sender`。
/// 而**在壳线程内**才构造的那些非 `Send` 适配器（`log_sink`、`ImportProgress`）不在闭包类型里，
/// 所以不受 `Send` 约束。
pub struct Request {
    run: Box<dyn FnOnce(&mut Actor) + Send>,
}

/// 导入结果要从 `mm-import` 线程搬回壳线程，`Send` 是这条搬运用法的编译期守卫
/// （`const _:` 锚点口径，先例 `lib.rs` 的 `_EVENT_NAMES`）。
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<ImportResult>();
};

impl Request {
    fn new<F>(run: F) -> Self
    where
        F: FnOnce(&mut Actor) + Send + 'static,
    {
        Self { run: Box::new(run) }
    }
}

/// 一个项目的命令结果：值已经通过回话通道送回，这里只是把它包成 Tauri 要的形态。
pub struct Bridge {
    requests: Mutex<Sender<Request>>,
}

impl Bridge {
    /// 起壳线程。线程名 `mm-shell`，它就是 Java 那边「拥有模型的 FX 线程」的等价物
    /// （Java `ProjectActions.java:18-21` 的类注释：导入与构建在 IO 线程上读，模型属于 FX 线程，
    /// 只有壳把工作线程读好的东西发布过去）。
    ///
    /// `close_guard` 由 `lib.rs` 创建并共享：主线程的 `CloseRequested` 拦截与兜底计时
    /// 线程都要碰它，而 confirm 命令走桥层到壳线程——三方共用同一份代际状态机。
    pub fn spawn(app: AppHandle, close_guard: Arc<crate::close_guard::CloseGuard>) -> Self {
        let (tx, rx) = channel::<Request>();
        let actor_app = app.clone();
        let actor_tx = tx.clone();
        // unwrap 之外无退路：起线程失败就没有壳了，与 Java `Main.start` 起不来即退出同语义。
        std::thread::Builder::new()
            .name("mm-shell".to_string())
            .spawn(move || run_actor(actor_app, rx, actor_tx, close_guard))
            .expect("spawn mm-shell thread");
        Self {
            requests: Mutex::new(tx),
        }
    }

    /// 把一次操作投给壳线程并等结果。
    ///
    /// 每条命令一个一次性回话通道（Java 的 `CompletableFuture` 在这里不需要池化：
    /// 命令本身就是「一次一个」的 IPC）。返回 `Err` 有两种：壳线程已退出，或那次操作在
    /// 壳线程内 panic 了（`Sender` 被丢弃），错误文本会区分这两种情况。
    pub async fn send<T>(&self, op: impl FnOnce(&mut Actor) -> T + Send + 'static) -> Result<T, ShellError>
    where
        T: Send + 'static,
    {
        let (done_tx, done_rx): (Sender<T>, Receiver<T>) = channel();
        let request = Request::new(move |actor| {
            let value = op(actor);
            let _ = done_tx.send(value);
        });
        self.dispatch(request)?;
        wait_reply(done_rx).await
    }

    /// [`Self::send`] 的延迟回话版：操作拿到回话 `Sender`，可以不在本次闭包内回话
    /// （导入把读包交给 IO 线程，发布完成后才回话）。壳线程在等待期间照常处理别的命令。
    pub async fn send_deferred<T>(
        &self,
        op: impl FnOnce(&mut Actor, Sender<T>) + Send + 'static,
    ) -> Result<T, ShellError>
    where
        T: Send + 'static,
    {
        let (done_tx, done_rx): (Sender<T>, Receiver<T>) = channel();
        let request = Request::new(move |actor| op(actor, done_tx));
        self.dispatch(request)?;
        wait_reply(done_rx).await
    }

    /// 投递请求到壳线程。克隆出 Sender 即丢锁卫：`MutexGuard` 不是 `Send`，
    /// 不能活到后面的 `.await` 之后。
    fn dispatch(&self, request: Request) -> Result<(), ShellError> {
        let sender = self
            .requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        sender
            .send(request)
            .map_err(|_| ShellError::new("shell-exited", "壳线程已退出，命令未执行"))
    }
}

/// 等壳线程回话（两种 `send` 共用）。阻塞 `recv` 挪到 blocking 池：async 命令跑在
/// tokio 工作线程上，同步 recv 会占住一个 worker 直到壳回话，并发命令多时耗尽运行时。
async fn wait_reply<T>(done_rx: Receiver<T>) -> Result<T, ShellError>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || done_rx.recv())
        .await
        .map_err(|_| ShellError::new("panic", "命令等待任务被取消"))?
        .map_err(|_| ShellError::new("panic", "命令在壳线程内 panic，结果未回送"))
}

/// 壳线程的全部状态：`ProjectController` 与它的所有权边界。
pub struct Actor {
    app: AppHandle,
    /// `None` 只有一种来路：`SchemaRegistry::load_default()` 失败。Java `Main.start` 在这种
    /// 情况下直接返回 false 让进程退出；壳不能退出，于是把失败原因留着，每条命令都返回它。
    controller: Option<ProjectController>,
    startup_error: Option<ShellError>,
    /// 上一次 `plan_build` 算出的计划。Java 的 `Build Pipeline` 对话框把 `Plan` 对象
    /// 一路传给 `writeBuildFolder`／`writeBuildZip`（`ProjectActions.java:95-121`），
    /// 而 `Plan` 里装着 `issues` 与 `SavePlan`，跨 IPC 反序列化不回来——所以前端只传
    /// 目的地，计划留在壳这一侧。任何改模型的命令都会把它清掉，避免写出过期的产物。
    build_plan: Option<mm_io::export::build_pipeline::Plan>,
    /// panic 后的受损标记（模块头「panic 隔离」更正块）：模型可能停在半更新状态，
    /// 除 `project_open` 外的命令一律拒绝，直到从盘上重新打开项目。
    poisoned: Option<ShellError>,
    /// 壳线程自己的请求入口：IO 线程完成读包后把「发布」投回来（导入两段式）。
    self_tx: Sender<Request>,
    /// 进行中的导入的取消标志；`None` = 当前没有导入。`import_cancel` 命令置位。
    import_cancel_flag: Option<Arc<AtomicBool>>,
    /// game-log 攒批入口（壳 sink 与导入 IO 线程共用），冲刷线程见 [`spawn_log_flusher`]。
    log_tx: Sender<String>,
    /// 最近一次游戏会话（2026-11-19 首页接线）。Java AppShell 的 `gameSession` 字段：
    /// 停止后仍保留，「打开测试目录」的可用性判据是 `gameSession != null` 而非进程存活。
    session: Option<mm_app::game_launcher::Session>,
    /// 原生关窗守卫的代际状态机（2026-11-25，P1-4），与主线程／兜底计时线程共享。
    close_guard: Arc<crate::close_guard::CloseGuard>,
}

impl Actor {
    /// 在壳线程上建控制器（所以 `ProjectController::new` 之后它永远只被这个线程碰）。
    fn new(app: AppHandle, self_tx: Sender<Request>, close_guard: Arc<crate::close_guard::CloseGuard>) -> Self {
        let (controller, startup_error) = match SchemaRegistry::load_default() {
            Ok(registry) => (Some(ProjectController::new(registry)), None),
            Err(err) => (None, Some(ShellError::new("not-ready", format!("schema 加载失败：{err}")))),
        };
        let log_tx = spawn_log_flusher(app.clone());
        let mut actor = Self {
            app,
            controller,
            startup_error,
            build_plan: None,
            poisoned: None,
            self_tx,
            import_cancel_flag: None,
            log_tx,
            session: None,
            close_guard,
        };
        // `setLogSink(Consumer<String>)` → 攒批通道 → `emit("game-log", Vec<String>)`。
        // Java 的 sink 是 FX 线程上的调用方塞进来的；这里唯一的使用方就是壳自己。
        if let Some(controller) = actor.controller.as_mut() {
            let log_tx = actor.log_tx.clone();
            controller.set_log_sink(Box::new(move |line: &str| {
                let _ = log_tx.send(line.to_string());
            }));
        }
        actor
    }

    /// 跑壳线程的主循环：`recv` 到 `Err` 即所有 `Bridge` 已释放，线程正常退出。
    fn run(self, rx: Receiver<Request>) {
        let mut this = self;
        while let Ok(request) = rx.recv() {
            // panic 不外溢：回话 `Sender` 在这里被丢弃，调用方得到错误文本。
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| (request.run)(&mut this))) {
                // 模型可能停在半更新状态：拒绝后续命令直到重新打开项目；
                // 缓存的构建计划也必须作废，否则 write_build 写出过期产物。
                this.build_plan = None;
                this.poisoned = Some(ShellError::new(
                    "poisoned",
                    format!("壳线程曾 panic，模型状态不可信，请重新打开项目（{}）", panic_message(&panic)),
                ));
            }
        }
    }

    /// 发一条事件。`emit` 的失败（没有前端监听）在这里不重要，Java 的监听器表为空时同样静默。
    fn emit<T>(&self, event: &str, payload: T)
    where
        T: Serialize + Clone,
    {
        let _ = self.app.emit(event, payload);
    }

    /// 取控制器；启动失败时把原因回给前端。受损标记优先：panic 之后模型不可信，
    /// 一切读写都拒绝，恢复入口只有 `project_open`（它不走这个守卫）。
    fn controller(&mut self) -> Result<&mut ProjectController, ShellError> {
        if let Some(reason) = &self.poisoned {
            return Err(reason.clone());
        }
        match self.controller.as_mut() {
            Some(controller) => Ok(controller),
            None => Err(ShellError::not_ready(&self.startup_error)),
        }
    }

    /// 「这条命令需要一个已打开的项目」。控制器的 `require_project()` 会 panic，
    /// 壳一律先查再调，把 panic 留在理论上不可达的路径里。
    fn require_project(&mut self) -> Result<&mut ProjectController, ShellError> {
        let controller = self.controller()?;
        if controller.project().is_some() {
            Ok(controller)
        } else {
            Err(ShellError::new("no-project", "还没有打开项目"))
        }
    }

    /// 当前结论的 `Rc`，用于广播前后的实例比对。控制器不存在（schema 加载失败）时给一份空结论，
    /// 与 Java `ValidationSession.published` 的初值 `List.of()`（`:56`）同形。
    fn findings(&self) -> Findings {
        match &self.controller {
            Some(controller) => controller.latest_issues(),
            None => Rc::new(Vec::new()),
        }
    }

    /// 选中记录的 id（比对用；Java 比的是引用，Rust 按 id，见 `project_controller.rs` 模块头第 2 点）。
    fn selected_id(&self) -> Option<String> {
        self.controller
            .as_ref()
            .and_then(|controller| controller.selected())
            .and_then(|record| record.id.clone())
    }

    /// 广播「模型变了」：先 `project-changed`，再按 `published` 是否换了实例补 `validation-refreshed`。
    fn broadcast_project(&mut self, before: &Findings) {
        let after = self.findings();
        let refreshed = !ValidationSession::same(before, &after);
        self.emit(EVENT_PROJECT_CHANGED, ());
        if refreshed {
            self.emit(EVENT_VALIDATION_REFRESHED, ());
        }
    }

    /// 选中项是否真的换了（用于那些「顺带改选中」的命令：`rename` 内部会 `select`）。
    fn broadcast_selection(&mut self, before: Option<String>) {
        let after = self.selected_id();
        if after != before {
            self.emit(EVENT_SELECTION_CHANGED, self.selection_payload());
        }
    }

    /// 当前选中记录的下发载荷；没选中就是 `null`，与 Java `select(null)` 的广播同形。
    fn selection_payload(&self) -> dto::SelectionDto {
        let Some(controller) = &self.controller else {
            return dto::SelectionDto::None;
        };
        match controller.selected() {
            // 类名与来源：有效类（声明 > 脚本推断 > 默认），规则在 `mm_app::class_resolution`。
            Some(record) => {
                let (class_name, source) = controller.class_name_and_source_of(record);
                dto::SelectionDto::Selected(dto::record_dto(record, &class_name, source.as_str()))
            }
            None => dto::SelectionDto::None,
        }
    }

    /// 项目概况。
    fn summary(&self) -> dto::ProjectSummaryDto {
        match &self.controller {
            Some(controller) => dto::project_summary_dto(controller.project(), controller.is_dirty()),
            None => dto::project_summary_dto(None, false),
        }
    }

    /// 记录 id → 克隆。所有需要 `&ContentRecord` 的命令（`rename`/`delete_contents`）都用它，
    /// 因为借用了模型就借不了 `&mut self.controller`；Java 靠引用同一性，这里按 id 回查，
    /// 克隆体与活记录只有「不是同一个对象」这一处区别，而控制器内部本来就按 id 定位。
    fn record_of(&self, record_id: &str) -> Result<mm_model::ContentRecord, ShellError> {
        let controller = match &self.controller {
            Some(controller) => controller,
            None => return Err(ShellError::not_ready(&self.startup_error)),
        };
        controller
            .project()
            .and_then(|project| project.content_by_id(record_id))
            .cloned()
            .ok_or_else(|| ShellError::new("not-found", format!("找不到记录：{record_id}")))
    }

    /// 会改模型的命令之后统一走这里：清掉缓存的构建计划并广播。
    fn after_mutation(&mut self, before: &Findings, selection_before: Option<String>) {
        self.build_plan = None;
        self.broadcast_project(before);
        self.broadcast_selection(selection_before);
    }

    // --- 16 条命令的实现 ------------------------------------------------------------------------

    /// `project_open`：对应 Java `openProject(Path)` → `publishProject`。
    /// 这也是 panic 受损后唯一的恢复入口：从盘上重载即拿到干净模型，故不走
    /// [`Self::controller`] 的受损守卫，成功后清掉标记。
    pub fn open_project(&mut self, root: String) -> Result<dto::ProjectSummaryDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let controller = match self.controller.as_mut() {
            Some(controller) => controller,
            None => return Err(ShellError::not_ready(&self.startup_error)),
        };
        controller
            .open_project(Path::new(&root))
            .map_err(project_error)?;
        self.poisoned = None;
        self.build_plan = None;
        self.broadcast_project(&before);
        self.broadcast_selection(selection_before);
        Ok(self.summary())
    }

    /// `project_import`：Java `ProjectActions.importPackage` 的两段——先在 IO 侧读包并拆到
    /// 目的地（这段可以带进度），再在拥有模型的线程上发布（`publishImport`，必须模型的线程）。
    ///
    /// > **2026-11-17 就地更正（原文一字不改地留在下方）**：原文「桥这边两步都在壳线程上跑」
    /// > 已不成立——读包段移到 `mm-import` 线程（模块头「导入的 IO 线程」），壳线程只发布。
    /// > 本命令因此改走 [`Bridge::send_deferred`]：投出读包任务后立即让出壳线程，
    /// > 读包期间别的命令照常处理；IO 线程完成后把 [`Self::finish_import`] 投回壳线程，
    /// > 发布并回话。进度仍走 `import-progress` 事件。
    ///
    /// 桥这边两步都在壳线程上跑，进度通过 `import-progress` 事件出去。
    pub fn import_package(
        &mut self,
        source: String,
        dest: String,
        reply: Sender<Result<dto::ImportReportDto, ShellError>>,
    ) {
        if let Err(err) = self.controller() {
            let _ = reply.send(Err(err));
            return;
        }
        if self.import_cancel_flag.is_some() {
            let _ = reply.send(Err(ShellError::new("import-busy", "已有一个导入在进行中")));
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.import_cancel_flag = Some(Arc::clone(&cancelled));
        let app = self.app.clone();
        let self_tx = self.self_tx.clone();
        let log_tx = self.log_tx.clone();
        let source_path = PathBuf::from(&source);
        let dest_path = PathBuf::from(&dest);
        let finish_source = source_path.clone();
        // 回话通道放共享槽：起线程失败时闭包被丢弃，仍要能把错误回给前端。
        let reply_slot = Arc::new(Mutex::new(Some(reply)));
        let worker_reply = Arc::clone(&reply_slot);
        let spawned = std::thread::Builder::new()
            .name("mm-import".to_string())
            .spawn(move || {
                // panic 也要把结果投回壳线程，否则回话通道永远等不到结果。
                let outcome: Result<ImportResult, ShellError> =
                    catch_unwind(AssertUnwindSafe(|| {
                        // 注册表含 Rc 缓存跨不了线程；schema 是静态文件，IO 线程重读一份。
                        let registry = SchemaRegistry::load_default()
                            .map_err(|err| ShellError::new("not-ready", format!("schema 加载失败：{err}")))?;
                        let mut progress = EmitProgress {
                            app: app.clone(),
                            cancelled: Arc::clone(&cancelled),
                        };
                        // 壳从未设置覆写批准器，控制器口径是 None → 一律批准
                        // （project_controller.rs read_import_with 的闭包）。
                        project_actions::read_import_with(
                            &source_path,
                            &dest_path,
                            &registry,
                            &mut progress,
                            &mut |_path| true,
                            &mut |message| {
                                let _ = log_tx.send(message.to_string());
                            },
                        )
                        .map_err(import_error)
                    }))
                    .unwrap_or_else(|panic| Err(ShellError::new("panic", format!("导入线程 panic：{}", panic_message(&panic)))));
                let reply = worker_reply
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                    .expect("回话通道只被取一次");
                let _ = self_tx.send(Request::new(move |actor| {
                    actor.finish_import(outcome, finish_source, reply);
                }));
            });
        if let Err(err) = spawned {
            self.import_cancel_flag = None;
            if let Some(reply) = reply_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                let _ = reply.send(Err(ShellError::new("io", format!("起导入线程失败：{err}"))));
            }
        }
    }

    /// 导入的第二段：在壳线程上发布 IO 线程读好的包（或把读包错误回给前端）。
    /// 广播取样放在这里而不是读包开始前——读包期间别的命令可能已经改过模型。
    fn finish_import(
        &mut self,
        outcome: Result<ImportResult, ShellError>,
        source: PathBuf,
        reply: Sender<Result<dto::ImportReportDto, ShellError>>,
    ) {
        self.import_cancel_flag = None;
        let result = (|| -> Result<dto::ImportReportDto, ShellError> {
            let imported = outcome?;
            let before = self.findings();
            let selection_before = self.selected_id();
            // `ImportReport` 没有 `Clone`，而 `publish_import` 会把 `ImportResult` 整个吞掉，
            // 所以先把报告转成 DTO，再发布。
            let report = dto::import_report_dto(&imported.report);
            self.controller()?.publish_import(imported, &source);
            self.build_plan = None;
            self.broadcast_project(&before);
            self.broadcast_selection(selection_before);
            Ok(report)
        })();
        let _ = reply.send(result);
    }

    /// `import_cancel`：**第 21 条命令**（2026-11-17，接 M4 明账第 4 条的欠账）。
    /// 置位进行中的导入的取消标志；读包段在文件之间尊重它（Java `readImport` 的取消语义：
    /// 写入已开始后取消，新建的半成品项目目录会被删掉）。没有进行中的导入时是空操作。
    /// 前端取消按钮尚未接线，命令面先备好（与 M5-D/E 扩列同一登记口径）。
    pub fn import_cancel(&mut self) -> Result<(), ShellError> {
        if let Some(flag) = &self.import_cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    /// `project_save`：对应 Java `save()`（`:455-459`）。Java 的 `save()` 里**没有**
    /// `fireProjectChanged()`，所以这里不广播——脏标记的刷新由返回值（概况里 `dirty = false`）带走。
    pub fn save(&mut self) -> Result<dto::ProjectSummaryDto, ShellError> {
        self.require_project()?.save().map_err(ShellError::domain)?;
        Ok(self.summary())
    }

    /// `project_plan_build`：只算不写（Java `planBuild`，`ProjectActions.java:95-104`）。
    /// 算出的计划留在壳这一侧，供 `project_write_build` 用。
    pub fn plan_build(&mut self) -> Result<dto::BuildPlanDto, ShellError> {
        let controller = self.require_project()?;
        let plan = controller.plan_build().map_err(ShellError::domain)?;
        let summary = dto::build_plan_dto(&plan);
        self.build_plan = Some(plan);
        Ok(summary)
    }

    /// `project_write_build`：Java 有两个入口 `writeBuildFolder`／`writeBuildZip`
    /// （`ProjectController.java:2051-2063`），`zip` 布尔选哪个，**不用扩展名猜**。
    /// 计划取自上一次 `plan_build`；没算过就现算一次（Java 的对话框总是先 `planBuild`，
    /// 这里让前端漏掉那一步也不会报错，只是多算一次）。
    pub fn write_build(
        &mut self,
        destination: String,
        zip: bool,
    ) -> Result<dto::ExportReportDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let destination_path = PathBuf::from(&destination);
        // 计划先从壳这一侧取走（`Option::take`），否则 `self.build_plan` 的借用会和
        // `&mut self.controller` 撞在一起。写盘会先保存工程（Java `:2053` 的 `save()` 在
        // `writeBuildFolder` 里），脏标记随之变化。
        let plan = match self.build_plan.take() {
            Some(plan) => plan,
            None => {
                let controller = self.require_project()?;
                controller.plan_build().map_err(ShellError::domain)?
            }
        };
        let report = {
            let controller = self.require_project()?;
            if zip {
                controller
                    .write_build_zip(&plan, &destination_path)
                    .map_err(ShellError::domain)?
            } else {
                controller
                    .write_build_folder(&plan, &destination_path)
                    .map_err(ShellError::domain)?
            }
        };
        // 计划用完即弃（`Option::take` 已经把它从 `self.build_plan` 摘走，这里不装回去）：
        // `write_build_*` 内部先跑了一次保存，计划里那份 `SavePlan` 已经落过盘，
        // 下一次构建必须基于当前模型重算，不能复用过期计划。
        self.broadcast_project(&before);
        self.broadcast_selection(selection_before);
        Ok(dto::export_report_dto(&report))
    }

    /// `board_snapshot`：画布一次快照（节点 + 卡片规则 + 边 + 结论；几何不在这里，
    /// 2026-11-14 口径见 `dto.rs` 模块头）。
    ///
    /// `board` 给了索引就先切过去——Java 的画板下拉框监听器就是 `controller.setBoard(...)`
    /// （`set_board` 内部 `fireProjectChanged()`，`:1884`），所以切换会带出 `project-changed`。
    /// 读结论之前先 `validate_quietly()` 刷缓存，对应 Java AppShell 那四处「读之前先刷 cache」；
    /// 这一步只换缓存实例，`broadcast` 已经在本命令开头取样，所以不会误发 `validation-refreshed`。
    pub fn board_snapshot(&mut self, board: Option<usize>) -> Result<dto::BoardSnapshotDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let controller = self.controller()?;
        if controller.project().is_none() {
            // 没有项目：画空画布。Java 的画板下拉在没有工程时也是空的。
            return Ok(dto::board_snapshot_dto(
                None,
                None,
                None,
                dto::parse_style(None),
                &[],
                Vec::new(),
            ));
        }
        let mut switched = false;
        if let Some(index) = board
            && current_board_index(controller) != Some(index)
        {
            controller.set_board(Some(index));
            switched = true;
        }
        let index = current_board_index(controller);
        controller.validate_quietly();
        let issues = controller.latest_issues();
        let snapshot = {
            let project = controller.project().expect("上面已判过有项目");
            let board = index.and_then(|index| project.boards.get(index));
            let style = dto::parse_style(board.map(|board| board.edge_style.as_str()));
            let cards = card_table(controller, board);
            dto::board_snapshot_dto(Some(project), board, index, style, &issues, cards)
        };
        if switched {
            self.broadcast_project(&before);
            self.broadcast_selection(selection_before);
        }
        Ok(snapshot)
    }

    /// `record_select`：`None` 就是 Java 的 `controller.select(null)`——它照样广播一条载荷为
    /// `null` 的 `selection-changed`（`SelectionModel.java:26-29`），而 `clear()`（`:35-37`）
    /// 存 `None` 但**不**广播。这个区别是 M4 把 `select` 入参改成 `Option` 的全部理由。
    pub fn record_select(
        &mut self,
        record_id: Option<String>,
    ) -> Result<dto::SelectionDto, ShellError> {
        let record = match record_id.as_deref() {
            Some(record_id) => Some(self.record_of(record_id)?),
            None => None,
        };
        let controller = self.controller()?;
        controller.select(record.as_ref());
        self.emit(EVENT_SELECTION_CHANGED, self.selection_payload());
        Ok(self.selection_payload())
    }

    /// `record_schema`：类名 + 字段描述符 + 当前值。**不含**必填标记——Java `RecordInspector.java`
    /// 全文没有 required 概念（M4 实测），不伪造。
    ///
    /// M6 起描述符还带上渲染一格控件真正需要的东西：`control`（`:389-406` 那张换算表的键）、
    /// `shown`（`:310-314` 那五项摘除）、`text`/`checked`/`swatch`（**当前值已按 Java 的规则
    /// 换算成控件初值**）与 `candidates`（`:677-697` 的引用候选名单）。换算只在
    /// `mm_app::inspector` 这一份里做，前端拿到什么就显示什么。
    ///
    /// 类名与来源走 [`Actor::class_name_and_source_of`]（Java `classNameOf` + R-02 的三级来源），
    /// 不在 DTO 层另解析一条口径。
    pub fn record_schema(&mut self, record_id: String) -> Result<dto::RecordSchemaDto, ShellError> {
        let record = self.record_of(&record_id)?;
        let registry = match &self.controller {
            Some(controller) => controller.registry(),
            None => return Err(ShellError::not_ready(&self.startup_error)),
        };
        let (class_name, source) = self.class_name_and_source_of(&record)?;
        let project = self.controller.as_ref().and_then(|c| c.project());
        Ok(dto::record_schema_dto(
            &record,
            registry,
            &class_name,
            source.as_str(),
            project,
        ))
    }

    /// 这一格该走哪个控件。对应 Java `editorRow()`（`:389-406`）拿到的那个 `definition.type()`：
    /// Java 是**建控件时**就知道，我们只收到一个字段名，所以回 schema 里查。
    /// 查不到 = 这一行来自 `addFieldRow`（`:646-674`），Java 那边没有 `FieldDef` 可依，
    /// 走 `commit_text(None, …)` 的原文解析支。
    fn field_type_of(&self, record: &ContentRecord, field: &str) -> Option<mm_schema::FieldType> {
        let registry = self.controller.as_ref()?.registry();
        let class_name = self.controller.as_ref()?.class_name_of(record);
        registry
            .fields_of(&class_name)
            .iter()
            .find(|def| def.name == field)
            .map(|def| def.field_type)
    }

    /// 有效类名＋来源（Java `classNameOf` + 2026-11-26 R-02 的脚本那一级）。
    /// 壳未就绪时没有别的来源可回落——`dto::record_schema_dto` 要拿它去 `fieldsOf`。
    fn class_name_and_source_of(
        &self,
        record: &ContentRecord,
    ) -> Result<(String, mm_app::class_resolution::ClassSource), ShellError> {
        match &self.controller {
            Some(controller) => Ok(controller.class_name_and_source_of(record)),
            None => Err(ShellError::not_ready(&self.startup_error)),
        }
    }

    /// `record_set_field`：前端**只传原始文本**，文本 → 值的换算全在 Rust（[`mm_app::inspector::commit_text`]），
    /// 逐字对齐 Java `editorRow()` 那张换算表（`RecordInspector.java:389-406`）里每个控件
    /// 自己的提交语义。返回 `false` = 这一笔**没有写进去**，对应 Java 给控件加 `editor-invalid`
    /// 样式然后直接 `return`（数字格 `:550-554`、颜色格 `:591-597`）——前端据此标红。
    ///
    /// ## 2026-11-15 就地更正（M6）：原来这段注释引错了出处，规则也照错的实现
    ///
    /// M4 版在这里写「逐字对齐 `RecordInspector.java:572-575` 的 raw 编辑器」，并据此把
    /// **所有**字段一律「全空白存空串、能 `Json.parse` 就用解析结果、parse 不过存原文」。
    /// `:572-575` 实测在 `colorEditor` 里（`:572-573` 解不出色落 `Color.WHITE`、`:574-575` 建那个
    /// hex `TextField`），不是 raw 编辑器；
    /// 真正的 raw 编辑器在 `:630-644`，而 `:661-667`（`addFieldRow`）才是「parse 或存原文」那条。
    /// 后果不是注释难看，是**行为错**：`copper, lead` 进 `text[]` 格会存成**一个字符串**而不是
    /// 数组（Java `:393-394` → `arrayOf` `:722-731`），`abc` 进数字格会被**存成字符串**而不是标红拒收
    /// （Java `:390` → `numberEditor` `:536-557`）。故改为按 `field_type_of` 分控件换算。
    ///
    /// > **2026-11-16 P5-M6 就地更正上面这段本身**（硬约束第 5 条）：更正块里「真正的 raw 编辑器在
    /// > `:645-674`」这句又引错了一次——M6-E 签字前重新实测：`rawEditor` 在 **`:630-644`**，
    /// > `:645` 是空行、`addFieldRow` 在 **`:646-674`**（`:661-667` 那个 try/catch 正是「空白存空串、
    /// > parse 得动用解析值、parse 不过存原文」，本句引用的这段一直是对的）。
    /// > 同理，原文写「Java `:391` → `arrayOf`」也不准：`:391-392` 是 `text, sprite` 臂（直接
    /// > `new JsonPrimitive(input)`，不经过 `arrayOf`），`textArray` 臂在 **`:393-394`**。
    /// > 数字臂 `:389` → **`:390`**（`:389` 是 `switch` 那一行）。
    /// > **这三处都只是注释行号，代码语义与下面 `record_set_field` 的实现一字未动。**
    ///
    /// `value == None` 仍是「删掉这一行」，与 Java `commit.accept(null)` → `setField(…, null)` 同义；
    /// 注意**空白输入不是 `None`**——前端照 Java 把框里的文本原样发来，空白该不该落成空串、
    /// 还是干脆移除这一行，由 `commit_text` 按控件决定（数字/堆格空白 → 移除，文本格空白 → 空串）。
    pub fn record_set_field(
        &mut self,
        record_id: String,
        field: String,
        value: Option<String>,
    ) -> Result<bool, ShellError> {
        let record = self.record_of(&record_id)?;
        let field_type = self.field_type_of(&record, &field);
        let commit = match value.as_deref() {
            None => mm_app::inspector::Commit::Unset,
            Some(text) => mm_app::inspector::commit_text(field_type, text),
        };
        let parsed = match commit {
            mm_app::inspector::Commit::Reject => return Ok(false),
            mm_app::inspector::Commit::Unset => None,
            mm_app::inspector::Commit::Value(json) => Some(json),
        };
        let before = self.findings();
        let selection_before = self.selected_id();
        self.controller()?
            .set_field(&record_id, &field, parsed);
        self.after_mutation(&before, selection_before);
        Ok(true)
    }

    /// `record_rename`：Java `rename(record, newName)` 内部会 `select(record)`（`:2126`），
    /// 所以这条命令顺带的选中变化由 [`Actor::broadcast_selection`] 如实带出。
    pub fn record_rename(&mut self, record_id: String, name: String) -> Result<(), ShellError> {
        let record = self.record_of(&record_id)?;
        let before = self.findings();
        let selection_before = self.selected_id();
        self.controller()?.rename(&record, &name);
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `record_delete`：Java `deleteContents(List<ContentRecord>)`（`:892` 段）。
    /// 控制器要的是 `&[&ContentRecord]`，这里用 id 回查出来的克隆体——它只读 `id`。
    pub fn record_delete(&mut self, record_ids: Vec<String>) -> Result<(), ShellError> {
        let mut records = Vec::with_capacity(record_ids.len());
        for record_id in &record_ids {
            records.push(self.record_of(record_id)?);
        }
        let refs: Vec<&mm_model::ContentRecord> = records.iter().collect();
        let before = self.findings();
        let selection_before = self.selected_id();
        self.controller()?.delete_contents(&refs);
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `edge_connect`：返回 Java `connect(...)` 的布尔（成/不成，不成的原因走 `game-log`）。
    pub fn edge_connect(
        &mut self,
        from_node: String,
        to_node: String,
        field_key: String,
    ) -> Result<bool, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let connected = self
            .require_project()?
            .connect(&from_node, &to_node, &field_key);
        self.after_mutation(&before, selection_before);
        Ok(connected)
    }

    /// `reroute_insert`：Java `ProjectController.java:1397 insertReroute(edge, x, y)`（报告 P2-9）。
    ///
    /// 交出去的是**坐标**，不是所有权：中转点仍存在 Rust 的模型里、仍参与保存与撤销栈，
    /// 前端只负责把它实测到的落点送回来（点列算法在 `src-vue/lib/edgePath.js` 跑，
    /// 2026-11-14 口径变更的逐字登记见 `dto.rs` 模块头）。
    pub fn reroute_insert(&mut self, edge_id: String, x: f64, y: f64) -> Result<(), ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.require_project()?
            .insert_reroute(&edge_id, x, y);
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `reroute_remove`：Java `:1429-1458 removeReroute(edge, index)`。
    /// 下标由前端持有（可见圆点的序号是渲染产物，Java 那边是 `rerouteDotsByEdge`），
    /// 越界由控制器按 Java `:1432` 忽略。
    pub fn reroute_remove(&mut self, edge_id: String, index: usize) -> Result<(), ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.require_project()?
            .remove_reroute(&edge_id, index);
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `reroute_commit`：Java `:1465 commitRerouteMove(edge, index, before, after)`。
    /// 一次拖动结束时把**拖动前坐标**原样送回，撤销栈才有可逆的旧值——Java 用字段
    /// `dragRerouteBefore`（`NodeCanvasPane.java:137` 那一片）存它，前端在 `pointerdown`
    /// 时取一次、`pointerup` 时随命令一起发来。
    pub fn reroute_commit(
        &mut self,
        edge_id: String,
        index: usize,
        before: [f64; 2],
        after: [f64; 2],
    ) -> Result<(), ShellError> {
        let before_findings = self.findings();
        let selection_before = self.selected_id();
        self.require_project()?
            .commit_reroute_move(&edge_id, index, before, after);
        self.after_mutation(&before_findings, selection_before);
        Ok(())
    }

    /// `node_move_commit`：卡片拖动落定。对应 Java 的**一对**动作——拖动过程中
    /// `NodeCanvasPane.dragTo` 就地改 `CanvasNode.x/y`（`:1270-1271`），松手时
    /// `ProjectController.commitMoves(before, after)`（`:1340`）记撤销栈。
    ///
    /// 前端只交实测坐标，**落模型 + 入栈都在控制器里**（[`ProjectController::move_nodes`]），
    /// 空移动短路、脏标记、捕获哪块板这些口径全部在那边逐字登记；这里只负责把
    /// 扁平的数组摊成两张保序表。表的插入顺序就是前端 `Object.keys` 的顺序，
    /// Java 那侧用的是 `LinkedHashMap`，同构。
    pub fn node_move_commit(&mut self, moves: Vec<dto::NodeMoveDto>) -> Result<(), ShellError> {
        let before_findings = self.findings();
        let selection_before = self.selected_id();
        let mut before: KeyedMap<[f64; 2]> = KeyedMap::new();
        let mut after: KeyedMap<[f64; 2]> = KeyedMap::new();
        for mv in moves {
            before.put(mv.id.clone(), mv.before);
            after.put(mv.id, mv.after);
        }
        self.require_project()?.move_nodes(before, after);
        self.after_mutation(&before_findings, selection_before);
        Ok(())
    }

    /// `edge_disconnect`：控制器签名收 `Option<&CanvasEdge>`（Java `disconnect(edge)` 的 null 支
    /// 直接返回，`:1208`），所以这里按边 id 在当前画板上回查后把克隆体递进去——
    /// id 找不到就按「这条边不在板上」处理，与 Java 的 `contains(edge)` 同一语义。
    pub fn edge_disconnect(&mut self, edge_id: Option<String>) -> Result<(), ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let edge = match edge_id.as_deref() {
            Some(edge_id) => {
                let controller = self.controller()?;
                controller
                    .board()
                    .and_then(|board| board.edges.iter().find(|edge| edge.id.as_deref() == Some(edge_id)))
                    .cloned()
            }
            None => None,
        };
        let controller = self.require_project()?;
        controller.disconnect(edge.as_ref());
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `undo`：对应 Java `undo()`（`:1516-1523`）。
    ///
    /// 2026-11-25（贴图导入纳入 undo 栈）：栈里现在可能有贴图条目，执行可失败
    /// （磁盘写失败 → io/domain，记录被删 → not-found，见 [`command_error`]）；
    /// 失败时条目留栈、模型未动，所以**不**走 `after_mutation`（不广播、不清计划）。
    pub fn undo(&mut self) -> Result<dto::HistoryStateDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.require_project()?.undo().map_err(command_error)?;
        self.after_mutation(&before, selection_before);
        self.history_state()
    }

    /// `redo`：对应 Java `redo()`（`:1525-1532`）。失败口径与 `undo` 一致。
    pub fn redo(&mut self) -> Result<dto::HistoryStateDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.require_project()?.redo().map_err(command_error)?;
        self.after_mutation(&before, selection_before);
        self.history_state()
    }

    /// `history_state`：撤销/重做按钮的三态。只读，不广播。
    pub fn history_state(&mut self) -> Result<dto::HistoryStateDto, ShellError> {
        let controller = self.controller()?;
        Ok(dto::HistoryStateDto {
            can_undo: controller.can_undo(),
            can_redo: controller.can_redo(),
            undo_depth: controller.undo_stack_depth(),
            dirty: controller.project().is_some() && controller.is_dirty(),
        })
    }

    // --- 资源管理首页接线（2026-11-19，命令面 21 → 34，登记见 lib.rs 模块头） -------------------

    /// `project_contents`：资源网格的记录清单。Java `ItemsPane` 直接读活记录，
    /// IPC 下只发 [`dto::RecordListItemDto`] 那几样；选中记录的字段树另走 selection 通道。
    pub fn contents(&self) -> Vec<dto::RecordListItemDto> {
        let Some(controller) = &self.controller else { return Vec::new() };
        let Some(project) = controller.project() else { return Vec::new() };
        project
            .contents
            .iter()
            .map(|record| dto::record_list_item_dto(record, &controller.display_name(record)))
            .collect()
    }

    /// `project_summary`：只读概况（`project-changed` 事件后前端刷新标题栏/状态栏用）。
    pub fn summary_query(&self) -> dto::ProjectSummaryDto {
        self.summary()
    }

    /// `record_add`：Java `addContent(type)` ＋ `ItemsPane.java:749` 的自动选中。
    /// 控制器交还 id（见 `add_content_at` 文档），这里按 id 回查后 select，
    /// 顺带的选中变化由 `after_mutation` 如实广播。
    pub fn record_add(&mut self, content_type: String) -> Result<dto::SelectionDto, ShellError> {
        let ctype = dto::parse_content_type(&content_type)
            .ok_or_else(|| ShellError::new("domain", format!("未知内容类型：{content_type}")))?;
        let before = self.findings();
        let selection_before = self.selected_id();
        let id = self.require_project()?.add_content(ctype);
        let record = self.record_of(&id)?;
        self.controller()?.select(Some(&record));
        self.after_mutation(&before, selection_before);
        Ok(self.selection_payload())
    }

    /// `record_set_patch`：Java `ProjectController.setPatch`（`:2413` 段）。
    pub fn record_set_patch(&mut self, record_id: String, patch: bool) -> Result<(), ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.controller()?.set_patch(&record_id, patch);
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `record_raw`：检查器「原始 JSON」段的读侧。新建记录 `raw == None`（Java 同），
    /// 前端拿 None 时按字段树自拼展示文本，不伪造磁盘原文。
    pub fn record_raw(&self, record_id: String) -> Result<Option<String>, ShellError> {
        Ok(self.record_of(&record_id)?.raw)
    }

    /// `record_apply_raw`：Java raw 编辑器「应用」= `Json.parse` 整树替换
    /// （`RecordInspector.java:630-644` → `applyRawTree`）。解析失败回 `false`
    /// （前端记日志、不关编辑器），与 `record_set_field` 的拒收口径同形。
    pub fn record_apply_raw(&mut self, record_id: String, text: String) -> Result<bool, ShellError> {
        let tree = match mm_json::parse_object(&text) {
            Ok(tree) => tree,
            Err(_) => return Ok(false),
        };
        let before = self.findings();
        let selection_before = self.selected_id();
        self.controller()?.apply_raw_tree(&record_id, tree);
        self.after_mutation(&before, selection_before);
        Ok(true)
    }

    /// `project_strings`：字符串表快照（管理键；非管理键 Java 只读，M6 递延）。
    pub fn strings(&self) -> dto::StringsDto {
        dto::strings_dto(self.controller.as_ref().and_then(|c| c.project()))
    }

    /// `string_set`：Java `setString(key, locale, value)`（`:2372`）。
    pub fn string_set(&mut self, key: String, locale: String, value: String) -> Result<(), ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.controller()?.set_string(&key, &locale, &value);
        self.after_mutation(&before, selection_before);
        Ok(())
    }

    /// `project_create`：Java 新建项目（`create_project`，`:329`）。与 `open_project`
    /// 同为「拿到干净模型」的入口，panic 受损标记一并清除。
    pub fn create_project(&mut self, name: String, root: String) -> Result<dto::ProjectSummaryDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let controller = match self.controller.as_mut() {
            Some(controller) => controller,
            None => return Err(ShellError::not_ready(&self.startup_error)),
        };
        controller
            .create_project(&name, Path::new(&root))
            .map_err(ShellError::domain)?;
        self.poisoned = None;
        self.build_plan = None;
        self.broadcast_project(&before);
        self.broadcast_selection(selection_before);
        Ok(self.summary())
    }

    /// `project_open_demo`：打开随仓库分发的示例项目（`demo-mod/`）。
    /// 路径解析沿用 `SchemaRegistry::load_default` 的候选目录口径（环境变量优先，
    /// 再按 cwd 相对回落）：dev 时 cwd=src-tauri（`../demo-mod`），
    /// 产物 exe 在 target/debug|release（`../../demo-mod`）。
    pub fn open_demo(&mut self) -> Result<dto::ProjectSummaryDto, ShellError> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("MODMAKER_DEMO") {
            candidates.push(PathBuf::from(p));
        }
        candidates.push(PathBuf::from("./demo-mod"));
        candidates.push(PathBuf::from("../demo-mod"));
        candidates.push(PathBuf::from("../../demo-mod"));
        let dir = candidates
            .into_iter()
            .find(|dir| dir.join("mod.json").exists() || dir.join("mod.hjson").exists())
            .ok_or_else(|| ShellError::new("not-found", "找不到示例项目 demo-mod/"))?;
        self.open_project(dir.to_string_lossy().into_owned())
    }

    /// `game_launch`：Java F6「安装并启动游戏」（AppShell → `GameLauncher.buildAndLaunch`）。
    /// 计划先取壳侧缓存（与 `write_build` 同一口径），Java/jar 探测失败按 domain 错误回。
    /// 日志走与控制器同一条攒批通道（`game-log` 事件）。
    pub fn game_launch(&mut self) -> Result<dto::GameSessionDto, ShellError> {
        let plan_opt = self.build_plan.take();
        let log_tx = self.log_tx.clone();
        let controller = self.require_project()?;
        let plan = match plan_opt {
            Some(plan) => plan,
            None => controller.plan_build().map_err(ShellError::domain)?,
        };
        let java = mm_app::game_launcher::detect_java()
            .ok_or_else(|| ShellError::new("domain", "未找到 Java 运行时（JAVA_HOME/PATH）"))?;
        let jar = mm_app::game_launcher::detect_game_jar()
            .ok_or_else(|| ShellError::new("domain", "未找到 Mindustry.jar"))?;
        let sink: mm_app::game_launcher::LogSink =
            std::sync::Arc::new(move |line: &str| {
                let _ = log_tx.send(line.to_string());
            });
        let session = mm_app::game_launcher::build_and_launch(controller, &plan, &jar, &java, sink)
            .map_err(ShellError::domain)?;
        self.session = Some(session);
        Ok(self.game_session())
    }

    /// `game_stop`：Java AppShell.stopGame（`:1769-1777`）——会话存活则记一条
    /// "stopping the game..." 并 `process.destroy()`，否则记 "no game is running"。
    /// 两条文案走与游戏输出同一条攒批通道（`game-log` 事件）。
    ///
    /// > **2026-11-20 M7a 销账**：原欠账「Rust 侧 Session 尚无 destroy 能力」已补——
    /// > `ProcessHandle::destroy` 落在 `mm_app::game_launcher`（平台差异登记在该文件模块头第 3 点）。
    ///
    /// 与 Java 的一处合并：已退出的会话在这里顺带 `close()` 清一次性数据目录——Java 由
    /// `discardFinishedSession`（`:1780-1787`）在 watcher 收尾时做，壳没有 watcher 退出事件，
    /// 停止按钮是唯一的清理入口。会话本身保留（`Actor::session` 文档：「打开测试目录」的
    /// 可用性判据是 `gameSession != null` 而非进程存活）。
    pub fn game_stop(&mut self) -> Result<dto::GameSessionDto, ShellError> {
        match &self.session {
            Some(session) if session.running() => {
                let _ = self.log_tx.send("stopping the game...".to_string());
                session.process.destroy();
            }
            Some(session) => {
                let _ = self.log_tx.send("no game is running".to_string());
                session.close();
            }
            None => {
                let _ = self.log_tx.send("no game is running".to_string());
            }
        }
        Ok(self.game_session())
    }

    /// `game_session`：会话三问（在不在/数据目录/产物目录）。无会话时全空。
    pub fn game_session(&self) -> dto::GameSessionDto {
        match &self.session {
            Some(session) => dto::GameSessionDto {
                running: session.running(),
                data_dir: Some(session.data_dir.to_string_lossy().into_owned()),
                mod_folder: Some(session.mod_folder.to_string_lossy().into_owned()),
            },
            None => dto::GameSessionDto { running: false, data_dir: None, mod_folder: None },
        }
    }

    // --- M7a 补口（2026-11-20，命令面 34 → 36，登记见 lib.rs 模块头） -------------------------

    /// `tech_forest`：科技树工作区的一次性森林快照。构建规则在领域层
    /// （[`mm_io::canvas::tech_tree_index::build`]，Java `TechTreeIndex.build` 的等价移植），
    /// 这里只把 Entry 树配上显示名/色块/research 摘要下发。原先前端逐记录拉 `record_schema`
    /// （真实 mod 265 次 IPC）再自算森林的走法就此退出；research 的归一化走
    /// [`mm_fmt::tech::read`]，前端不再解析 JSON 文本。无项目时回空森林（与 `contents` 同口径）。
    pub fn tech_forest(&self) -> dto::TechForestDto {
        match &self.controller {
            Some(controller) => crate::forest::tech_forest_dto(controller),
            None => dto::TechForestDto {
                roots: Vec::new(),
                unassigned: Vec::new(),
                refused: Vec::new(),
            },
        }
    }

    /// `records_detail`：批量版 `record_schema`。星球战役工作区原先为每条 planet/sector
    /// 记录各拉一次 `record_schema`，这里一次 IPC 带回同一批描述符 + 当前值
    /// （[`dto::RecordSchemaDto`] 原形复用，前端物化口径不变）。
    /// 找不到的 id 静默跳过——刷新途中记录可能刚被删/改名，与逐条拉时前端吞 not-found 同口径。
    pub fn records_detail(&self, record_ids: Vec<String>) -> Vec<dto::RecordSchemaDto> {
        let Some(controller) = &self.controller else { return Vec::new() };
        let Some(project) = controller.project() else { return Vec::new() };
        record_ids
            .iter()
            .filter_map(|id| {
                let record = project.content_by_id(id)?;
                let (class_name, source) = controller.class_name_and_source_of(record);
                Some(dto::record_schema_dto(
                    record,
                    controller.registry(),
                    &class_name,
                    source.as_str(),
                    Some(project),
                ))
            })
            .collect()
    }

    /// `cosmos_scene`：星球战役三页签的场景快照（Java `PlanetPreview.setSource` 的数据半边）。
    /// 领域层 [`mm_app::cosmos::scene`] 只读模型；渲染仍在 Vue。
    pub fn cosmos_scene(&mut self, record_id: String) -> Result<dto::CosmosSceneDto, ShellError> {
        let controller = self.controller()?;
        let project = controller
            .project()
            .ok_or_else(|| ShellError::new("no-project", "没有打开的项目"))?;
        let scene = mm_app::cosmos::scene(project, &record_id)
            .map_err(|err| ShellError::new("domain", err.to_string()))?;
        Ok(dto::cosmos_scene_dto(&scene))
    }

    // --- 2026-11-20 编辑器基线补口（命令面 36 → 41，登记见 lib.rs 模块头）---------------------

    /// `record_sprite`：一条记录的贴图缩略图。规则在 `mm_fmt::sprite`，这里只搬运字节。
    /// 记录不存在 / 没有项目根 / 没有匹配的 png 都回 `None`（编辑中途的正常状态，不是错误）。
    pub fn record_sprite(&mut self, record_id: String) -> Result<Option<dto::SpriteDto>, ShellError> {
        let controller = self.controller()?;
        Ok(crate::sprite_asset::sprite_for(controller, &record_id))
    }

    /// `sprite_import`（2026-11-25，命令面 41 → 42）：把用户经 PathPicker 选择的 PNG
    /// 复制为一条记录的贴图。内核在 [`crate::sprite_asset::import_sprite`]（tauri-free）：
    /// 源路径只读、目标落在项目根内、原子复制字节保真、同步资产账本。
    ///
    /// 事件口径与其余改模型的命令一致（`after_mutation`）：清缓存的构建计划（贴图参与
    /// 打包，旧计划会漏掉新资产）、广播 `project-changed`；选中不变则不发
    /// `selection-changed`。内核已 `note_assets_changed()` 作废校验缓存，但 `published`
    /// 实例未换，所以按既有判据**不**发 `validation-refreshed`——前端收到
    /// `project-changed` 后重拉快照时自然会拿到新鲜结论。
    ///
    /// 2026-11-25（UI 审计缺陷 #5）：导入成功同时向领域历史推一条贴图变更条目
    /// （内核负责），undo/redo 既有命令自然覆盖回滚/重放；undo 后的缩略图刷新走
    /// 既有 `project-changed` 重拉链路，前端无需新接线。命令面、事件表均不变。
    pub fn sprite_import(
        &mut self,
        record_id: String,
        source_path: String,
    ) -> Result<dto::SpriteDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let controller = self.require_project()?;
        let sprite = crate::sprite_asset::import_sprite(controller, &record_id, Path::new(&source_path))
            .map_err(sprite_import_error)?;
        self.after_mutation(&before, selection_before);
        Ok(sprite)
    }

    /// `project_convert_format`：把所有内容文件（含 meta）翻到 `ext`，返回**将转换**的条数。
    /// 只改模型不落盘，真正的写入发生在下一次 `project_save`（与 Java 的对话框两步一致）。
    pub fn convert_format(&mut self, ext: String) -> Result<usize, ShellError> {
        // 2026-11-25 加固后领域层对未知扩展名回错误（不再 panic）；这道前置守卫保留，
        // 作为第一道防线：坏输入在进控制器之前就被挡下，并给出面向用户的中文文案。
        if ext != "json" && ext != "hjson" {
            return Err(ShellError::new(
                "domain",
                format!("不支持的格式：{ext}（只接受 json 或 hjson）"),
            ));
        }
        let before = self.findings();
        let selection_before = self.selected_id();
        let converted = self.require_project()?.convert_format(&ext).map_err(project_error)?;
        self.after_mutation(&before, selection_before);
        Ok(converted)
    }

    /// `board_tidy_layout`：整块画板重排后回一次快照（前端拿到新坐标就能立刻重画）。
    pub fn tidy_layout(&mut self) -> Result<dto::BoardSnapshotDto, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        self.require_project()?.tidy_layout();
        self.after_mutation(&before, selection_before);
        self.board_snapshot(None)
    }

    /// `project_rebuild_links`：按记录重算连线，返回改动的边数（新增 + 删除）。
    pub fn rebuild_links(&mut self) -> Result<usize, ShellError> {
        let before = self.findings();
        let selection_before = self.selected_id();
        let changed = self.require_project()?.rebuild_links();
        self.after_mutation(&before, selection_before);
        Ok(changed)
    }

    /// `window_confirm_close`（2026-11-25，P1-4）：前端确认放弃／已保存后真正关窗。
    ///
    /// 刻意**不走** `controller()`／`require_project()`／poisoned 守卫：关窗与模型健康
    /// 无关，壳受损时用户也必须能离开。代际状态机保证 confirm 与 3s 兜底恰好一方赢
    /// （[`crate::close_guard`]）；重复调用或兜底已放行时 `confirm()` 摘不到请求，
    /// 回稳定码 `not-found`。被 `window_close_hold` 摘除计时的请求同样摘得到——
    /// hold 只取消计时不摘取请求，hold 后 confirm 必须成功。用 `destroy` 而非 `close`：
    /// 后者会再触发 `CloseRequested` 被守卫再次拦下，形成关不掉的循环。
    pub fn window_confirm_close(&mut self) -> Result<(), ShellError> {
        if self.close_guard.confirm().is_none() {
            return Err(ShellError::new("not-found", "没有待确认的关窗请求（重复调用，或未经原生关闭触发）"));
        }
        let window = self.app.get_webview_window("main")
            .ok_or_else(|| ShellError::new("not-found", "主窗口已不存在"))?;
        window.destroy().map_err(|err| ShellError::new("domain", err.to_string()))
    }

    /// `window_close_hold`（2026-11-25，P1-4 契约扩展，命令面 44 → 45）：前端收到
    /// `window-close-requested` 后**立即**调用，摘除当前代的兜底计时——dirty 对话框
    /// 获得无限决策时间，3s 兜底不再强制关窗（兜底只留给无监听者的旧前端）。
    ///
    /// 只取消计时，**不摘取请求、不碰窗口**：确认关闭仍走 [`Self::window_confirm_close`]
    /// （hold 后仍能摘到代）；取消关闭不调任何命令，窗口保持打开。刻意**不走**
    /// `controller()`／`require_project()`／poisoned 守卫，口径与 confirm 一致：
    /// 壳受损时用户也必须能停掉自动放行。重复 hold／无待确认请求 → 稳定码 `not-found`。
    pub fn window_close_hold(&mut self) -> Result<(), ShellError> {
        if self.close_guard.hold().is_none() {
            return Err(ShellError::new("not-found", "没有待确认的关窗请求（重复 hold，或未经原生关闭触发）"));
        }
        Ok(())
    }
}

/// 起线程用的自由函数版 [`Bridge::spawn`] 的内核。
fn run_actor(app: AppHandle, rx: Receiver<Request>, self_tx: Sender<Request>, close_guard: Arc<crate::close_guard::CloseGuard>) {
    Actor::new(app, self_tx, close_guard).run(rx);
}

/// game-log 的冲刷线程：收到一行就把队列里已到的全部 drains 成一批 emit。
/// 突发（导入期）合并成少数几批；零星日志来一行发一行，不引入等待。
/// 所有 `Sender` 丢弃（壳退出）后线程自行结束。
fn spawn_log_flusher(app: AppHandle) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    std::thread::Builder::new()
        .name("mm-log".to_string())
        .spawn(move || {
            while let Ok(line) = rx.recv() {
                let mut batch = vec![line];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                }
                let _ = app.emit(EVENT_GAME_LOG, batch);
            }
        })
        .expect("spawn mm-log thread");
    tx
}

/// 把 `catch_unwind` 的载荷转成可读文本（panic 消息通常是 `&str` 或 `String`）。
fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "未知 panic".to_string()
    }
}

/// 当前画板下标。Java 的 `ProjectController` 没有 `currentBoardIndex()`，
/// Rust 侧 `board: Option<usize>` 字段也没有 getter，只有 `board()`（`:1553`）返回活引用——
/// 于是壳按**引用同一性**在 `project.boards` 里找它，这正好是 Java 语义（它拿的就是那个对象）。
fn current_board_index(controller: &ProjectController) -> Option<usize> {
    let board = controller.board()?;
    let project = controller.project()?;
    project
        .boards
        .iter()
        .position(|candidate| std::ptr::eq(candidate, board))
}

/// 建卡规则表：Java `NodeCanvasPane.java:342-364` 那段「遍历节点 → 查记录 → 建卡」里
/// **不进前端几何**的部分，实际判定树在 `:500-555 buildCard()`。
///
/// 返回与 `board.nodes` **同序**，第 `i` 格就是第 `i` 个节点的卡片，`None` = 这个节点不建卡，
/// 逐字对应 Java `:353-355`：
///
/// ```java
/// if (record == null && node.refName == null) continue;   // group/note 卡
/// ```
///
/// 那两个「跳过」条件在 Rust 侧变成 `None` 而不是「少一格」，因为快照里的 `cards` 要和
/// `nodes` 同序——前端按同一个下标把两者配起来，少一格就全体错位。
/// group/note 节点的卡片 Java 是 `GroupCard`／`NoteCard`，M5-E 若真要画它们再补这里。
/// Java 还有第二处跳过 `:357-359`（`buildCard` 返回 null 时），实测**永不触发**——
/// `:500-555 buildCard()` 里三个分支都有返回值，没有一处 `return null`，所以这里不复制它。
///
/// `displayName`/`class_name_of`/`subtitle_of`/`type_name` 只能问 controller（要读类名覆盖表
/// 和注册表），所以这张表建在壳层而不是 `dto.rs` 里——`dto::card_dto` 收到的是已经解析好的值。
///
/// ## 卡片文案四味（M5-E 补齐）
///
/// Java `buildCard()` 交给卡片的非几何信息一共四样，这里全部算好传给 `dto::card_dto`，
/// 由它按分支各取所需（内容卡取 subtitle，补给卡取 type_label）：
///
/// - Java `:503`/`:538 controller.displayName(record)` → 标题；
/// - Java `:539 controller.subtitleOf(record)` → 内容卡 Tooltip
///   （Rust [`ProjectController::subtitle_of`]，Java `ProjectController.java:2301-2309`）；
/// - Java `:508`/`:529 controller.typeName(type)` → 补给卡类型徽章
///   （Rust [`ProjectController::type_name`]，Java `:2296-2298`，固定英文底账）；
/// - Java `classNameOf`（`:2284-2287`）→ `class_name_of`，卡片行/端口规则的输入。
fn card_table(
    controller: &ProjectController,
    board: Option<&mm_model::Board>,
) -> Vec<Option<dto::CardDto>> {
    let (Some(project), Some(board)) = (controller.project(), board) else {
        return Vec::new();
    };
    let registry = controller.registry();
    let mut ports_cache: KeyedMap<Vec<String>> = KeyedMap::new();
    // id → 记录一次建表：逐节点全量扫 contents 是 O(节点×内容)/次快照。
    // 重复 id 取第一个（与原先 find 的首中口径一致）。
    let mut by_id: KeyedMap<&mm_model::ContentRecord> = KeyedMap::new();
    for record in &project.contents {
        if let Some(id) = record.id.as_deref() {
            by_id.put_if_absent(id.to_string(), record);
        }
    }
    board
        .nodes
        .iter()
        .map(|node| {
            let record = node
                .content_id
                .as_deref()
                .and_then(|content_id| by_id.get(content_id).copied());
            if record.is_none() && node.ref_name.is_none() {
                return None; // Java :353-355
            }
            // Java `:503`/`:538` 都走 `controller.displayName(record)`；无记录那条分支
            // Java 在 `:523` 算的是 `node.refName == null ? "" : node.refName`。
            let display_name = match record {
                Some(record) => controller.display_name(record),
                None => node.ref_name.clone().unwrap_or_default(),
            };
            // `class_name` 只有内容卡那一支用得到；补给卡两支（Java `:502`/`:515`）不读它，
            // 所以无记录时给空串，不去伪造一个「默认的 class」。
            let class_name = match record {
                Some(record) => controller.class_name_of(record),
                None => String::new(),
            };
            // Java `:539`：subtitleOf 只在内容卡分支才算；这里对补给记录也算一份，
            // 由 `dto::card_dto` 按分支丢弃（谁进哪支的判定不复制第二遍）。
            let subtitle = record.map(|record| controller.subtitle_of(record));
            // Java `:508`/`:529`：无记录那一支的 type 与 card_dto 取色用的是同一个
            // `ref_type_or_item` 回落口径（`:517-522`）。
            let type_label = controller
                .type_name(match record {
                    Some(record) => record.ctype,
                    None => mm_app::card_geometry::ref_type_or_item(node.ref_type.as_deref()),
                })
                .to_string();
            Some(dto::card_dto(
                node,
                record,
                &display_name,
                &class_name,
                subtitle.as_deref(),
                Some(&type_label),
                registry,
                &mut ports_cache,
            ))
        })
        .collect()
}

/// `ImportProgress` 的壳实现：阶段与计数变成 `import-progress` 事件。
///
/// 计数上报走 [`worth_reporting`]——它就是 Java `ImportProgress.worthReporting` 那 64 个文件一报的节流，
/// 不自己写第二个规则。
///
/// > **2026-11-17 就地更正（原文一字不改地留在下方）**：取消已接上——`import_cancel`
/// > 命令置位共享原子标志，`is_cancelled` 读它。原文「恒 false」的明账就此销账。
/// > 构造地点也随导入两段式改为 `mm-import` IO 线程（捕获的 `AppHandle` 与原子标志
/// > 都是 `Send`，跨线程无障碍）。
///
/// 只在壳线程内构造（捕获的 `AppHandle` 是 `Send + Clone`，但这一实现本身不需要跨线程）。
/// 取消按钮 M4 还没接（Java 的取消来自导入对话框），所以
/// [`ImportProgress::is_cancelled`] 恒 `false`，即 Java `ImportProgress.NONE` 的行为；
/// 这条明账登记在 M4 签字条目。
struct EmitProgress {
    app: AppHandle,
    cancelled: Arc<AtomicBool>,
}

impl ImportProgress for EmitProgress {
    fn stage_started(&mut self, stage: Stage, total: i32) {
        self.app.emit(
            EVENT_IMPORT_PROGRESS,
            dto::ImportProgressDto {
                stage: dto::stage_key(stage).to_string(),
                done: 0,
                total,
            },
        )
        .ok();
    }

    fn progressed(&mut self, stage: Stage, done: i32, total: i32) {
        if !worth_reporting(done, total) {
            return;
        }
        self.app.emit(
            EVENT_IMPORT_PROGRESS,
            dto::ImportProgressDto {
                stage: dto::stage_key(stage).to_string(),
                done,
                total,
            },
        )
        .ok();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
