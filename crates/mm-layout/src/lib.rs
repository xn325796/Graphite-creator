//! mm-layout：`core.layout`（2 类）的等价移植——科技树画布布局。
//!
//! 这两个类存在的意义是「可证明一致」：`BranchTreeLayout` 是游戏 `ResearchDialog` 真正跑的 abego
//! TreeLayout 游走算法的逐行移植，`TechTreeCanvasLayout` 在上面加了游戏在根节点上做的上下对半拆分。
//! 编辑器画布上的坐标因此与游戏内的坐标同源，间距常量也是同一份。

/// 2026-11-13 P5-M1 追加：本 crate 还收纳 `dev.modmaker.ui` 包里**纯计算**的那两类——
/// `ui/EdgePathMath.java`（连线路由形状）与 `ui/MinimapMath.java`（小地图投影 / LOD）。
/// 它们按 P5 计划 §三（已签字）落在这里而不是 `mm-app` 或前端 TS：`mm-layout` 的
/// `Cargo.toml` 里没有任何依赖，「几何只在 Rust 侧离线测、前端只画不算」于是由依赖层强制。
/// Java 侧两者都是 `package-private`，Rust 侧**不能**对应 `pub(crate)`：那会让这些几何函数在
/// 非 test 构建里只被 `#[cfg(test)]` 引用而报 `dead_code`，而 M1 的验收门要求 0 警告、
/// `allow(` 又是禁词。因此这里与模块内部一律用 `pub`（对已签字口径的更正，详见
/// [`edge_path_math`] 顶部注释）；跨 crate 的调用方（M2/M3 的画板快照）本来也需要 `pub`。
/// 分层仍由依赖层强制：本 crate 零依赖，前端拿不到它。
pub mod edge_path_math;
pub mod minimap_math;
/// 2026-11-16 P6-M2 追加：同一个切分下的第三类——`dev.modmaker.ui.PlanetLanes`（科技树行星泳道的
/// 包围盒 / 落点命中 / 稳定配色，纯静态、零依赖）。它 Java 侧就在 `ui` 包里，但和本 crate 前两类
/// 一样「只有数学、没有控件」，所以照 P5 §三 那条已签字口径落在依赖面为零的这里。
pub mod planet_lanes;

pub mod branch_tree_layout;
pub mod tech_tree_canvas_layout;
