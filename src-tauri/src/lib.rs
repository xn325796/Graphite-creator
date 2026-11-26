//! P5-M4：Tauri 壳接线。只做接线，不做前端交互、不拆 `ProjectController` 重接回调。
//!
//! > **2026-11-17 就地更正**：命令面 20 → 21（补 `import_cancel`，接 M4 明账第 4 条的
//! > 导入取消欠账）；导入读包段移到 IO 线程（M4 明账第 3 条就此落地），事件表不变。
//! > 扩列与更正的逐字理由在 `bridge.rs` 模块头及各自命令文档。
//!
//! > **2026-11-25 就地更正**：命令面 41 → 42（补 `sprite_import`，贴图导入）。
//! > 入参是用户选出的源 PNG 路径（前端禁 dialog 插件），目标路径由 Rust 按读侧口径解析；
//! > 契约与事件口径登记在命令处与 `bridge::Actor::sprite_import`。事件表不变。
//!
//! > **2026-11-25 就地更正（P1-4 原生关窗守卫）**：命令面账目更正——上一批记 42，
//! > 但 `cosmos_scene` 漏记，`generate_handler!` 实测 43；本批补 `window_confirm_close`，
//! > 43 → 44。事件表 5 → 6（补 `window-close-requested`，空载荷）：原生 X/Alt+F4 被
//! > `on_window_event` 拦下后广播，前端确认后调 `window_confirm_close` 真正关窗；
//! > 旧前端无监听者时 3s 兜底放行（秒数与理由在 `close_guard::CLOSE_FALLBACK_SECS`）。
//! > 代际状态机在 tauri-free 的 [`close_guard`]，接线在本文件 `run()`。
//!
//! > **2026-11-25 就地更正（P1-4 契约扩展）**：命令面 44 → 45（补 `window_close_hold`）。
//! > 前端收到 `window-close-requested` 后立即调它摘除该代兜底计时——dirty 对话框获得
//! > 无限决策时间，解决「3s 兜底 vs 用户决策」的张力；无人调 hold（旧前端）时 3s 兜底
//! > 照旧放行。hold 只取消计时不摘取请求：确认关闭仍走 `window_confirm_close`，取消则
//! > 不调命令（窗口保持打开）。事件表不变；状态机扩展见 [`close_guard::CloseGuard::hold`]。
//!
//! 命令表与事件表逐字来自方案文档 §四 M4 那张表（`P5-实施计划与两面板.md:267-272`），
//! 20 条命令 / 5 条事件，与 (c) 备胎一致，所以前端不受选型影响。命令**实现**全部在
//! [`bridge`]，DTO **形状**全部在 [`dto`]，本文件只剩三件事：
//!
//! 1. `setup` 里起壳线程（`ProjectController` 不是 `Send`，不能 `.manage()` 它，理由逐条写在
//!    `bridge.rs` 模块头）；
//! 2. 把 20 条命令挂进 `generate_handler!`；
//! 3. 声明 5 条事件的名字由壳侧常量单点拥有（见 [`bridge::EVENT_GAME_LOG`] 等），
//!    前端只 `listen`，不自己拼字符串。
//!
//! 「16→19→20」就地更正：那张表在 M4 是 16 条。M5-D 补了 reroute 一族三条
//! `edge_insert_reroute`／`edge_remove_reroute`／`edge_move_reroute`（对应 Java
//! `ProjectController.java:1397`／`:1429`／`:1465`）；M5-E 落地画布交互时又补了第 20 条
//! `node_move_commit`——几何交给 Vue 实测之后，卡片坐标只能由前端回流，
//! 而 Java 那边「拖动即改模型 + 松手记撤销栈」这对动作（`NodeCanvasPane.java:1270-1271`
//! ＋ `ProjectController.java:1340`）在旧命令面里**一条都没有**，不补就是拖完不生效、
//! 也不可撤销。两次的理由都逐字写在各自命令的文档上。
//!
//! ## 为什么命令是 `async fn`
//!
//! 同步 command 在 Tauri 2 里跑在主线程上，而壳线程的 `app.emit` 也要回主线程投递——
//! 同步命令阻塞等壳线程回话，会把这条投递排在后面。`async fn` 把命令放到异步运行时的工作线程，
//! 壳线程反过来占的是主线程的空闲。代价是一条命令占一个工作线程直到回话；壳侧本来就是
//! 单线程串行处理（对应 Java FX 线程），并发命令只会排队，不存在死锁环。
//!
//! ## 参数名的对外口径
//!
//! Tauri 把 Rust 的 snake_case 参数自动映射成 JS 的 camelCase：`board_id` → `boardId`、
//! `record_ids` → `recordIds`、`field_key` → `fieldKey`。M5/M6 的前端按 camelCase 传。
//!
//! 模板遗留的 `greet`（原 `:3-5`）与 `tauri_plugin_opener`（P5-Q3 已删依赖）都不在这里了；
//! `src-vue` 侧对应的调用点同批删除，壳与前端不留死 IPC。

pub mod bridge;
pub mod close_guard;
pub mod dto;
pub mod forest;
pub mod fs_explore;
pub mod sprite_asset;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::dto::{
    BoardSnapshotDto, BuildPlanDto, ExportReportDto, FsListingDto, HistoryStateDto,
    ImportReportDto, ProjectSummaryDto, RecordSchemaDto, SelectionDto, SpriteDto,
};

/// 对应 Java `ProjectController.openProject(Path)`（`:347`）＋ `ProjectActions.readProject`（`:40-42`）。
#[tauri::command]
async fn project_open(
    bridge: tauri::State<'_, bridge::Bridge>,
    path: String,
) -> Result<ProjectSummaryDto, bridge::ShellError> {
    bridge.send(move |actor| actor.open_project(path)).await?
}

/// 对应 Java `ProjectActions.readImport(...)` ＋ `ProjectController.publishImport`。
///
/// > **2026-11-17 就地更正（原文一字不改地留在下方）**：读包段已移到 `mm-import` IO 线程
/// > （`bridge.rs` 模块头「导入的 IO 线程」），壳线程只发布，读包期间别的命令不再被冻结。
/// > 命令改走 `send_deferred`，返回值语义不变。
///
/// 导入在壳线程同步跑（Java 的 io worker 线程这一层不落地，见 `bridge.rs` 模块头明账），
/// 进度与结果分别走 `import-progress` 事件与本次返回值。
#[tauri::command]
async fn project_import(
    bridge: tauri::State<'_, bridge::Bridge>,
    source: String,
    destination: String,
) -> Result<ImportReportDto, bridge::ShellError> {
    bridge.send_deferred(move |actor, reply| actor.import_package(source, destination, reply)).await?
}

/// **第 21 条命令**（2026-11-17）：取消进行中的导入。接 M4 明账第 4 条的欠账，
/// 理由与登记口径见 `bridge::Actor::import_cancel`。前端取消按钮尚未接线。
#[tauri::command]
async fn import_cancel(bridge: tauri::State<'_, bridge::Bridge>) -> Result<(), bridge::ShellError> {
    bridge.send(|actor| actor.import_cancel()).await?
}

/// 对应 Java `ProjectController.save()`（`:569-582`）。返回项目摘要供前端刷新标题栏与脏标记。
#[tauri::command]
async fn project_save(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<ProjectSummaryDto, bridge::ShellError> {
    bridge.send(|actor| actor.save()).await?
}

/// 对应 Java `ProjectController.planBuild()`（`:850`）。构建计划留在壳侧，前端只拿摘要
/// （`Plan` 与 `SavePlan` 借用了注册表，跨不了 IPC）。
#[tauri::command]
async fn project_plan_build(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<BuildPlanDto, bridge::ShellError> {
    bridge.send(|actor| actor.plan_build()).await?
}

/// 对应 Java `ProjectActions.writeBuildFolder(plan, dest)`（`:126-129`）/
/// `writeBuildZip(plan, file)`（`:131-134`）；`zip == true` 走后者。
#[tauri::command]
async fn project_write_build(
    bridge: tauri::State<'_, bridge::Bridge>,
    destination: String,
    zip: bool,
) -> Result<ExportReportDto, bridge::ShellError> {
    bridge.send(move |actor| actor.write_build(destination, zip)).await?
}

/// 画板快照：节点 + **Rust 算好的边路径点列** + world bounds + 校验结果。
/// 对应 Java 里 `NodeCanvasPane.rebuild()` 读模型的那一段；前端只画，不重算。
#[tauri::command]
async fn board_snapshot(
    bridge: tauri::State<'_, bridge::Bridge>,
    board_id: Option<usize>,
) -> Result<BoardSnapshotDto, bridge::ShellError> {
    bridge.send(move |actor| actor.board_snapshot(board_id)).await?
}

/// 对应 Java `ProjectController.select(record)`；`None` 就是 `NodeCanvasPane.java:1189`
/// 的 `controller.select(null)`，照样广播一条空载荷的 `selection-changed`。
#[tauri::command]
async fn record_select(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: Option<String>,
) -> Result<SelectionDto, bridge::ShellError> {
    bridge.send(move |actor| actor.record_select(record_id)).await?
}

/// 字段规则的描述符（控件类型、枚举域、默认值），全部由 `mm-schema` 读 `fields.json` 算出。
#[tauri::command]
async fn record_schema(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
) -> Result<RecordSchemaDto, bridge::ShellError> {
    bridge.send(move |actor| actor.record_schema(record_id)).await?
}

/// 对应 Java `ProjectController.setField(record, field, value)`；`value == None` 删字段。
///
/// M6：文本 → 值的换算搬进 Rust（`mm_app::inspector::commit_text`，Java
/// `RecordInspector.java:389-406` 那张表），所以这里的 `value` 是**框里的原始文本**。
/// 返回 `false` = 这一笔被规则拒收、什么都没写（Java 给控件加 `editor-invalid` 然后 `return`），
/// 前端据此标红；`true` = 已写入。
#[tauri::command]
async fn record_set_field(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
    field: String,
    value: Option<String>,
) -> Result<bool, bridge::ShellError> {
    bridge.send(move |actor| actor.record_set_field(record_id, field, value)).await?
}

/// 对应 Java `ProjectController.rename(record, newName)`。
#[tauri::command]
async fn record_rename(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
    name: String,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.record_rename(record_id, name)).await?
}

/// 对应 Java `ProjectController.deleteContents(records…)`；空数组直接忽略（Java `:529` 同款）。
#[tauri::command]
async fn record_delete(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_ids: Vec<String>,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.record_delete(record_ids)).await?
}

/// 对应 Java `ProjectController.connect(node, node, key)`（`:885`）；返回值与 Java 一致。
#[tauri::command]
async fn edge_connect(
    bridge: tauri::State<'_, bridge::Bridge>,
    from_node: String,
    to_node: String,
    field_key: String,
) -> Result<bool, bridge::ShellError> {
    bridge.send(move |actor| actor.edge_connect(from_node, to_node, field_key)).await?
}

/// 对应 Java `ProjectController.disconnect(edge)`；`None` 即 `disconnect(null)`，Java 直接返回。
#[tauri::command]
async fn edge_disconnect(
    bridge: tauri::State<'_, bridge::Bridge>,
    edge_id: Option<String>,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.edge_disconnect(edge_id)).await?
}

/// 对应 Java `ProjectController.reroute` 一族（报告 P2-9）。M5-D 起前端能改中转点，
/// 但**只有几何交出去**：坐标由前端实测后送回，边仍由 Rust 持有、仍进撤销栈。
/// `x`/`y` 是世界坐标，与快照里 `reroutes`、节点 `x`/`y` 同一坐标系。
#[tauri::command]
async fn edge_insert_reroute(
    bridge: tauri::State<'_, bridge::Bridge>,
    edge_id: String,
    x: f64,
    y: f64,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.reroute_insert(edge_id, x, y)).await?
}

/// 对应 Java `ProjectController.removeReroute(edge, index)`（`:1429-1458`）。
#[tauri::command]
async fn edge_remove_reroute(
    bridge: tauri::State<'_, bridge::Bridge>,
    edge_id: String,
    index: usize,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.reroute_remove(edge_id, index)).await?
}

/// 对应 Java `ProjectController.commitRerouteMove(edge, index, before, after)`（`:1465`）。
/// `before` 是拖动起点的实测坐标，前端在 `pointerdown` 取一次、随命令一起发来。
#[tauri::command]
async fn edge_move_reroute(
    bridge: tauri::State<'_, bridge::Bridge>,
    edge_id: String,
    index: usize,
    before: [f64; 2],
    after: [f64; 2],
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.reroute_commit(edge_id, index, before, after)).await?
}

/// 卡片拖动落定：前端实测的坐标回流。对应 Java 的 `NodeCanvasPane.dragTo`
/// （`:1270-1271` 拖动中就地改模型）＋ `ProjectController.commitMoves`（`:1340` 入撤销栈）这一对。
///
/// **这条是 M5-E 补的第 20 条**，不在 §四 M4 那张表里，也不是 M5-D 那三条 reroute：
/// M5-D 把几何交给 Vue 之后，前端成了唯一知道「卡片被拖到哪儿」的一方，坐标必须能回流，
/// 否则拖动既不可撤销、保存也拿不到新位置（Java 那边这两件事都成立）。落模型与入栈都在
/// 控制器里做（`mm-app` 的 `ProjectController::move_nodes`），这里只做接线。
/// `moves` 每项 `{id, before, after}`，坐标是世界单位，与快照同一坐标系。
#[tauri::command]
async fn node_move_commit(
    bridge: tauri::State<'_, bridge::Bridge>,
    moves: Vec<dto::NodeMoveDto>,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.node_move_commit(moves)).await?
}

/// 对应 Java `ProjectController.undo()`（`:460`）。
#[tauri::command]
async fn undo(bridge: tauri::State<'_, bridge::Bridge>) -> Result<HistoryStateDto, bridge::ShellError> {
    bridge.send(|actor| actor.undo()).await?
}

/// 对应 Java `ProjectController.redo()`。
#[tauri::command]
async fn redo(bridge: tauri::State<'_, bridge::Bridge>) -> Result<HistoryStateDto, bridge::ShellError> {
    bridge.send(|actor| actor.redo()).await?
}

/// 工具条的撤销／重做可用性与脏标记（Java 直接读 `undoManager` 与 `dirty` 字段）。
#[tauri::command]
async fn history_state(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<HistoryStateDto, bridge::ShellError> {
    bridge.send(|actor| actor.history_state()).await?
}

// ===== 2026-11-19 就地更正：命令面 21 → 34（资源管理首页接线）=====
// 原表（§四 M4 的 20 条 + import_cancel）围绕画布工作流；Creator-rs 原型迁移落地后，
// 首页（Java ItemsPane/ContextPanel/StringsPane/AppShell 游戏段）缺 13 条通道。
// 领域层方法 M2/M3 早已移植（add_content/set_patch/apply_raw_tree/set_string/
// create_project/game_launcher），本批全部是薄接线，零新增第三方依赖。
// 唯一欠账：game_stop 的 destroy 能力未移植（登记在 bridge::Actor::game_stop）。
// > 2026-11-20 M7a 销账：destroy 已补进 mm_app::game_launcher::ProcessHandle，
// > game_stop 不再对运行中的会话回 domain 错误。

/// 资源网格的记录清单（Java ItemsPane 直接读活记录的 IPC 等价物）。
#[tauri::command]
async fn project_contents(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<Vec<dto::RecordListItemDto>, bridge::ShellError> {
    bridge.send(|actor| actor.contents()).await
}

/// 只读概况：`project-changed` 事件后前端刷新标题栏/状态栏。
#[tauri::command]
async fn project_summary(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<ProjectSummaryDto, bridge::ShellError> {
    bridge.send(|actor| actor.summary_query()).await
}

/// 新建内容记录并自动选中（Java `addContent` + ItemsPane:749）。
#[tauri::command]
async fn record_add(
    bridge: tauri::State<'_, bridge::Bridge>,
    content_type: String,
) -> Result<SelectionDto, bridge::ShellError> {
    bridge.send(move |actor| actor.record_add(content_type)).await?
}

/// patch 开关（Java `setPatch`）。
#[tauri::command]
async fn record_set_patch(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
    patch: bool,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.record_set_patch(record_id, patch)).await?
}

/// 检查器「原始 JSON」读侧；新建记录为 null。
#[tauri::command]
async fn record_raw(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
) -> Result<Option<String>, bridge::ShellError> {
    bridge.send(move |actor| actor.record_raw(record_id)).await?
}

/// 原始 JSON 整树替换（Java raw 编辑器「应用」）；false = 解析失败拒收。
#[tauri::command]
async fn record_apply_raw(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
    text: String,
) -> Result<bool, bridge::ShellError> {
    bridge.send(move |actor| actor.record_apply_raw(record_id, text)).await?
}

/// 字符串表快照（管理键）。
#[tauri::command]
async fn project_strings(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<dto::StringsDto, bridge::ShellError> {
    bridge.send(|actor| actor.strings()).await
}

/// 字符串表格编辑（Java `setString`）。
#[tauri::command]
async fn string_set(
    bridge: tauri::State<'_, bridge::Bridge>,
    key: String,
    locale: String,
    value: String,
) -> Result<(), bridge::ShellError> {
    bridge.send(move |actor| actor.string_set(key, locale, value)).await?
}

/// 新建项目（Java 新建项目对话框）。
#[tauri::command]
async fn project_create(
    bridge: tauri::State<'_, bridge::Bridge>,
    name: String,
    root: String,
) -> Result<ProjectSummaryDto, bridge::ShellError> {
    bridge.send(move |actor| actor.create_project(name, root)).await?
}

/// 打开随仓库分发的示例项目 demo-mod/。
#[tauri::command]
async fn project_open_demo(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<ProjectSummaryDto, bridge::ShellError> {
    bridge.send(|actor| actor.open_demo()).await?
}

/// F6 安装并启动游戏（Java GameLauncher.buildAndLaunch）。
#[tauri::command]
async fn game_launch(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<dto::GameSessionDto, bridge::ShellError> {
    bridge.send(|actor| actor.game_launch()).await?
}

/// 停止游戏（Java AppShell.stopGame：destroy + 两条日志；M7a 起真正停得下来）。
#[tauri::command]
async fn game_stop(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<dto::GameSessionDto, bridge::ShellError> {
    bridge.send(|actor| actor.game_stop()).await?
}

/// 游戏会话三问（打开测试目录的可用性判据）。
#[tauri::command]
async fn game_session(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<dto::GameSessionDto, bridge::ShellError> {
    bridge.send(|actor| actor.game_session()).await
}

// ===== 2026-11-20 M7a 就地更正：命令面 34 → 36（批量数据通道）=====
// 科技树/星球战役工作区原先逐记录拉 record_schema（真实 mod 一次刷新 265 次 IPC），
// 且森林在前端自算——领域层 mm_io::canvas::tech_tree_index（Java TechTreeIndex 的移植）
// 早已存在，本批把它接成 tech_forest 一条命令；records_detail 是 record_schema 的批量版，
// 供星球战役一次拉齐 planet/sector 的字段。同批销账 game_stop 的 destroy 欠账
// （ProcessHandle::destroy 已补进 mm-app，见 bridge::Actor::game_stop 的更正块）。
// 精灵缩略图仍递延：SpriteStore/SpriteRegions/iconRegion（Java 共 302 行）未移植，
// 且下发图片还需 asset protocol + CSP img-src 扩面，归 M7b。

/// 科技森林一次性快照（Java TechTreeIndex.build 的 IPC 等价物）。
#[tauri::command]
async fn tech_forest(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<dto::TechForestDto, bridge::ShellError> {
    bridge.send(|actor| actor.tech_forest()).await
}

/// 批量记录详情（record_schema 的批量版）；找不到的 id 静默跳过。
#[tauri::command]
async fn records_detail(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_ids: Vec<String>,
) -> Result<Vec<RecordSchemaDto>, bridge::ShellError> {
    bridge.send(move |actor| actor.records_detail(record_ids)).await
}


// ===== 2026-11-20 编辑器基线：命令面 36 → 41 =====
// 前端已按这批契约编码（目录浏览 / 缩略图 / 格式切换 / 整板重排 / 重算连线），命令名与
// DTO 字段不得擅改。目录浏览是纯外壳能力（不碰模型），走 blocking 池；其余四条走壳线程。

/// 列一个目录（目录优先排序；`isMod`=含 mod.json/mod.hjson，`isArchive`=.zip/.jar）。
/// 失败统一回 `fs-error`——前端据此提示「目录打不开」，与领域错误区分。
#[tauri::command]
async fn fs_list_dir(path: String) -> Result<FsListingDto, bridge::ShellError> {
    tauri::async_runtime::spawn_blocking(move || crate::fs_explore::list_dir(&path))
        .await
        .map_err(|_| bridge::ShellError::new("panic", "目录浏览任务被取消"))?
        .map_err(|message| bridge::ShellError::new("fs-error", message))
}

/// 一条记录的贴图（`data:image/png;base64,…`）；找不到回 `null`。
#[tauri::command]
async fn record_sprite(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
) -> Result<Option<SpriteDto>, bridge::ShellError> {
    bridge.send(move |actor| actor.record_sprite(record_id)).await?
}

/// ===== 2026-11-25 命令面 41 → 42：贴图导入（sprite_import）=====
/// 前端分层守卫禁 `@tauri-apps/plugin-*`（含 dialog），所以入参是**用户经
/// PathPicker/fs_list_dir 选出的源 PNG 绝对路径**，不是文件对话框结果。
/// 目标路径由 Rust 按读侧口径解析（`sprites/<记录名>.png`），前端不传目标路径。
/// 返回与 `record_sprite` 非空时同形的 DTO，前端拿到即可刷新缩略图；
/// 成功后广播 `project-changed`（口径见 `bridge::Actor::sprite_import`）。
#[tauri::command]
async fn sprite_import(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
    source_path: String,
) -> Result<SpriteDto, bridge::ShellError> {
    bridge.send(move |actor| actor.sprite_import(record_id, source_path)).await?
}

/// 内容格式批量切换（`json` | `hjson`），返回**将转换**的记录数；落盘要再调一次保存。
#[tauri::command]
async fn project_convert_format(
    bridge: tauri::State<'_, bridge::Bridge>,
    ext: String,
) -> Result<usize, bridge::ShellError> {
    bridge.send(move |actor| actor.convert_format(ext)).await?
}

/// 整块画板重排，返回重排后的快照（前端不必再拉一次 `board_snapshot`）。
#[tauri::command]
async fn board_tidy_layout(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<BoardSnapshotDto, bridge::ShellError> {
    bridge.send(|actor| actor.tidy_layout()).await?
}


/// 星球战役场景（Java `PlanetPreview.setSource` 的数据半边）：区块网格 / 星球外观 / 星系轨道。
/// 只读命令，无事件；非 planet/sector 记录回 `domain` 错误。
#[tauri::command]
async fn cosmos_scene(
    bridge: tauri::State<'_, bridge::Bridge>,
    record_id: String,
) -> Result<dto::CosmosSceneDto, bridge::ShellError> {
    bridge.send(move |actor| actor.cosmos_scene(record_id)).await?
}

/// 按记录重算连线，返回改动的边数（新增 + 删除）。
#[tauri::command]
async fn project_rebuild_links(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<usize, bridge::ShellError> {
    bridge.send(|actor| actor.rebuild_links()).await?
}

/// ===== 2026-11-25 命令面 43 → 44：原生关窗确认（window_confirm_close，P1-4）=====
/// 原生 X/Alt+F4 被 `on_window_event` 拦下并广播 `window-close-requested`（空载荷）后，
/// 前端确认放弃编辑／已保存时调用本命令，壳侧 `destroy` 主窗口（不再触发
/// `CloseRequested`，无循环）。无入参；dirty 判断完全在前端。
/// 错误口径（稳定码 `not-found`）：重复调用／兜底已放行／无待确认请求；主窗口已不存在。
/// 实现与防循环理由见 `bridge::Actor::window_confirm_close`。
#[tauri::command]
async fn window_confirm_close(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<(), bridge::ShellError> {
    bridge.send(|actor| actor.window_confirm_close()).await?
}

/// ===== 2026-11-25 命令面 44 → 45：摘除关窗兜底计时（window_close_hold，P1-4 扩展）=====
/// 前端收到 `window-close-requested` 后**立即**调用本命令：摘除当前代的 3s 兜底计时，
/// dirty 对话框获得无限决策时间（兜底只留给无监听者的旧前端）。无入参。
/// 之后确认关闭走 `window_confirm_close`（hold 不摘取请求，confirm 仍能摘到代）；
/// 取消关闭不调任何命令，窗口保持打开。连按 X 发新代时旧 hold 随旧代一并作废，
/// 前端对每次事件各调一次 hold 即可（重复 hold 回 `not-found`，可安全忽略）。
/// 错误口径（稳定码 `not-found`）：重复 hold／无待确认请求。事件表不变。
/// 实现见 `bridge::Actor::window_close_hold`，状态机见 [`close_guard::CloseGuard::hold`]。
#[tauri::command]
async fn window_close_hold(
    bridge: tauri::State<'_, bridge::Bridge>,
) -> Result<(), bridge::ShellError> {
    bridge.send(|actor| actor.window_close_hold()).await?
}

/// Windows 上 WebView2 的进程级参数（打包后的自包含启动）。
///
/// `--no-sandbox` 是 Win11 Insider 26200/26300 上 Chromium 沙箱 CHECK 崩溃
/// （WebView2Feedback #5777）的**临时豁免**。2026-11-21 实测：去掉 `--disable-gpu`
/// 后 GPU 进程正常启动，ANGLE 使用 NVIDIA D3D11（RTX 4050），Canvas 2D 回到硬件光栅；
/// 原先把整个 GPU 进程一起关掉属于过度规避。若该回归复发，可手工设置
/// `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--no-sandbox --disable-gpu` 兜底。
///
/// 已存在 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`（例如排障时手工注入）就不覆盖：
/// 环境变量是唯一信道，后写者会整串顶掉前一个值，这里只补默认值。
/// `set_var` 在 Rust 2024 是 unsafe：本轮在**任何线程起 webview 之前**调用，没有并发读环境
/// 变量的机会，因此该约束在此成立。
#[cfg(windows)]
fn configure_webview2_browser_args() {
    const DEFAULT_ARGS: &str = "--no-sandbox";
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        // 安全性：见上——只在本进程的单线程启动阶段调用一次。
        unsafe {
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", DEFAULT_ARGS);
        }
    }
}

#[cfg(not(windows))]
fn configure_webview2_browser_args() {}

/// 兜底放行计时线程（P1-4）：emit `window-close-requested` 后
/// [`close_guard::CLOSE_FALLBACK_SECS`] 秒内前端没调 `window_confirm_close`，
/// 就由壳自己 `destroy` 主窗口。旧前端没有监听者，`prevent_close` 之后无人应答，
/// 不设兜底窗口就永远关不掉——不可接受，所以限时放行（秒数的取舍理由在常量处）。
///
/// 代际比对（`fallback_due`）保证与 confirm 恰好一方赢：前端已确认、或用户又按了
/// 一次 X（新一代顶掉旧一代）时，本计时器静默作废。前端调 `window_close_hold` 后
/// 该代被打上 hold 标志，`fallback_due` 恒失败——计时同样静默作废，请求留给 confirm。
/// panic 防线口径与 actor／导入线程一致（release 无 `panic=abort`）：`catch_unwind` 兜住；
/// 计时线程 panic 时仍尽力放行一次——宁可关窗，不能把用户困在关不掉的窗口里。
fn spawn_close_fallback(app: tauri::AppHandle, guard: Arc<close_guard::CloseGuard>, generation: u64) {
    let thread_app = app.clone();
    let thread_guard = guard.clone();
    let spawned = std::thread::Builder::new()
        .name("mm-close-fallback".to_string())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(close_guard::CLOSE_FALLBACK_SECS));
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if thread_guard.fallback_due(generation) {
                    destroy_main_window(&thread_app);
                }
            }));
            if outcome.is_err() {
                destroy_main_window(&thread_app);
            }
        });
    if spawned.is_err() && guard.fallback_due(generation) {
        // 起不了兜底线程（资源耗尽级故障）：等价于「无人应答」，立即放行。
        destroy_main_window(&app);
    }
}

/// `destroy` 而非 `close`：后者会再触发 `CloseRequested` 被守卫拦下，形成循环。
/// 窗口已不在（confirm 或上一次兜底已关掉）时静默——与 actor 的 emit 失败同口径。
fn destroy_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_webview2_browser_args();
    tauri::Builder::default()
        .setup(|app| {
            // 壳线程要 `AppHandle` 才能 emit，所以只能在 `setup` 里起（`:1757` 的 `Fx.runLater` 等价物）。
            let handle = app.handle().clone();
            // 关窗守卫三方共享：主线程的 CloseRequested、壳线程的 confirm 命令、兜底计时线程。
            let close_guard = Arc::new(close_guard::CloseGuard::new());
            app.manage(bridge::Bridge::spawn(handle.clone(), close_guard.clone()));
            // 窗口在这里手动建而非配置自动建：tauri 2.12.6 的 from_config 不接
            // additional_browser_args（死键），只有 builder 方法接通（webview_window.rs:1080）。
            //
            // > **2026-11-17 12:10 就地更正（原文一字不改地留在下方）**：manual builder 的
            // > `additional_browser_args()` 现已**移除**——A/B 矩阵需要让 loader 级环境变量
            // > `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`（run.bat 注入）成为唯一信道；builder
            // > 一旦显式设置 options 串，环境变量整串失效，两信道并存无法做对照。旗标本身与
            // > 撤销条件不变，见决策记录「GUI 启动故障诊断」节。
            //
            // --no-sandbox 是 Win11 Insider 26200/26300 上 Chromium 沙箱 CHECK 崩溃的
            // 临时豁免；--disable-gpu 已于 2026-11-21 移除（实测 D3D11 GPU 可用）。
            // disable-features 三项是 wry 的默认值，手动建窗后须自带。
            let window = tauri::WebviewWindowBuilder::new(
                &handle,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("mindustry-modcreator")
            .inner_size(1200.0, 650.0)
            .min_inner_size(960.0, 600.0)
            // 无框窗口：标题栏与最小化/最大化/关闭全部由前端 titlebar 提供
            // （data-tauri-drag-region 负责拖动与双击最大化，四边热区负责缩放）。
            .decorations(false)
            .build()?;
            // P1-4（2026-11-25）：原生 X/Alt+F4 不再直接放行——拦下 CloseRequested，
            // 广播 `window-close-requested`（空载荷，app.emit 口径同 bridge::Actor::emit），
            // 由前端 dirty 守卫决定何时调 `window_confirm_close`；无人应答时 3s 兜底放行。
            // 挂在建成后的窗口上：tauri 2.12.6 的 WebviewWindowBuilder 没有
            // on_window_event 方法（只有 WindowBuilder 有），WebviewWindow 实例才有。
            let event_guard = close_guard.clone();
            let event_handle = handle.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let generation = event_guard.request();
                    let _ = event_handle.emit(bridge::EVENT_WINDOW_CLOSE_REQUESTED, ());
                    spawn_close_fallback(event_handle.clone(), event_guard.clone(), generation);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            project_open,
            project_import,
            import_cancel,
            project_save,
            project_plan_build,
            project_write_build,
            board_snapshot,
            record_select,
            record_schema,
            record_set_field,
            record_rename,
            record_delete,
            edge_connect,
            edge_disconnect,
            edge_insert_reroute,
            edge_remove_reroute,
            edge_move_reroute,
            node_move_commit,
            undo,
            redo,
            history_state,
            project_contents,
            project_summary,
            record_add,
            record_set_patch,
            record_raw,
            record_apply_raw,
            project_strings,
            string_set,
            project_create,
            project_open_demo,
            game_launch,
            game_stop,
            game_session,
            tech_forest,
            records_detail,
            cosmos_scene,
            fs_list_dir,
            record_sprite,
            sprite_import,
            project_convert_format,
            board_tidy_layout,
            project_rebuild_links,
            window_confirm_close,
            window_close_hold
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 编译期锚点：6 条事件的名字必须只由 [`bridge`] 一处拥有，前端 `listen` 的字符串与这里逐字相同。
/// 用 `const _:` 形式而不是 `allow(dead_code)`（M3 签字条目立的口径，先例 `mm-layout/src/lib.rs:13`）。
const _EVENT_NAMES: [fn() -> &'static str; 6] = [
    || bridge::EVENT_PROJECT_CHANGED,
    || bridge::EVENT_SELECTION_CHANGED,
    || bridge::EVENT_VALIDATION_REFRESHED,
    || bridge::EVENT_GAME_LOG,
    || bridge::EVENT_IMPORT_PROGRESS,
    || bridge::EVENT_WINDOW_CLOSE_REQUESTED,
];
