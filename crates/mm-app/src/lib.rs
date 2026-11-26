//! mm-app：Java `dev.modmaker.ui` 包里**与 GUI 无关的那部分状态**的等价移植。
//!
//! P5 搬三类：`ui/UndoManager.java`（撤销/重做历史）、`ui/SelectionModel.java`（当前选中记录）
//! 与 `ui/ProjectController.java`（项目生命周期 + 编辑命令门面，见 [`project_controller`]）。
//! 它们在 Java 侧就是「为了让单测不依赖 JavaFX 才从 `ProjectController` 里抽出来」的独立类
//! （`UndoManager.java:6-12`、`SelectionModel.java:9-15` 的类注释都写明了这一点），
//! 所以 Rust 侧同样可以完全离线测。
//!
//! 决策 D5：**零 tauri 依赖**（见 `Cargo.toml`）。撤销/选中/画板快照/meta 编辑全部在这里算，
//! `src-tauri/` 只做 event 广播与命令表。
//!
//! D5 原话「代价只是 `src-tauri/src/lib.rs` 多写一层 `State<Mutex<…>>` 包装」在 M4 实测**不成立**：
//! `ProjectController` 根本进不了 `Mutex`——它的 `log_sink: Option<Box<dyn Fn(&str)>>`
//! （`project_controller.rs:1495`）与 `overwrite_approver`（`:1500`）都没有 `+ Send`，
//! 校验结论那一格装的是 `Findings = Rc<Vec<Issue>>`（`validation_session.rs:50`）。
//! 真实代价是壳侧一个 actor 线程 + `std::sync::mpsc` 一次性回话通道
//! （[`project_controller`] 模块头与 `src-tauri/src/bridge.rs` 模块头各记一份明账）。

pub mod card_geometry;
/// 有效类解析（2026-11-26，产品评审 R-02）：声明类 > 脚本 `extend` 推断 > 类型默认类。
/// Java 只有两级，见模块头的偏离登记。
pub mod class_resolution;
/// 星球战役场景组装（Java `ui/PlanetPreview` 的数据半边）：网格、外观层、大气、光照与星系。
pub mod cosmos;
/// 每一次失败都要经过的那一行渲染（Java `ui/ErrorReporter.java` 的 `describe`／`reportQuietly`）。
/// P6-M2 新增；Java 侧那个 `Alert` 对话框不在这里，理由写在模块头。
pub mod error_reporter;
pub mod game_launcher;
/// 记录面板的文本 ↔ 值换算（Java `ui/RecordInspector.java` 的 8 个 `static` 纯函数与
/// `editorRow()` 那张换算表）。M6 新增；它决定字段落成什么字节，所以留在 Rust，前端只消费。
pub mod inspector;
pub mod project_actions;
pub mod project_controller;
pub mod selection_model;
/// 科技树的解锁三态（Java `ui/TechUnlockState.java`）：只从 `mm-io` 那片科技树森林推出来的
/// 纯表计算，渲染层只消费结果、自己不再走树。P6-M2 新增。
pub mod tech_unlock_state;
pub mod undo_manager;
pub mod validation_session;
