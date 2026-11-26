//! `TechTreeCanvasLayout.java` 的等价移植（Java `dev.modmaker.core.layout.TechTreeCanvasLayout`，:16-110）。
//!
//! 研究界面的科技树布局：一次普通 [`BranchTreeLayout`] 游走，加上游戏在根节点上做的那个对半拆分，
//! 于是编辑器画布画出的形状与游戏 `ResearchDialog.treeLayout`（ResearchDialog.java:332-372）一致。
//! 根的孩子分成两半：前 `ceil(n/2)` 个用 `TreeLocation.top`（层往下长），其余用 `bottom`（层往上长），
//! 再把上半截整体平移，让两半在根的终态 Y 上接合。兄弟始终沿 X 铺开，nodeSize 与两个间距都取游戏的值。
//!
//! # 与 Java 的环境差异（都在语义等价的前提下处理，不改变任何计算）
//!
//! 1. **`Box` 改名 `TechBox`**。Java 里它是 `TechTreeCanvasLayout.Box`，靠外部类名限定；Rust 里
//!    `use ...::Box` 会遮蔽 `std::boxed::Box`，故改名，字段、构造器与语义一字未动。
//! 2. **`Object payload` → 泛型参数 `P`**（:24）。Java 用擦除后的 `Object` 携带 `TechTreeIndex.Entry`，
//!    取用时强转；Rust 直接用类型参数，省掉强转也不改变布局（布局从不读 payload）。
//! 3. **两趟布局靠「把孩子挪进挪出根节点」实现，而不是 `subList` 视图**（:79-98）。Java 的
//!    `leftHalf`/`rightHalf` 是同一个 `children` 副本上的视图，`clear()`/`addAll()` 换的是根的可变列表，
//!    节点对象自始至终是同一批。Rust 的所有权模型下等价做法是把 `Vec<TechBox<P>>` 整体移动：
//!    布局前把上半截移进 `root.children`，跑 `top`；再把已经摆好的上半截移出来、把下半截移进去，跑
//!    `bottom`；最后按原顺序拼回。移动的是节点本身，坐标因此得以保留，结果与 Java 逐点相同。
//! 4. **私有构造的空工具类（:67-68）→ 模块级自由函数**，与 `mm-io` 里 `AutoLayout` 的移植口径一致。

use crate::branch_tree_layout::{BranchTreeLayout, LayoutNode, TreeLocation};

/// Java `NODE_SIZE`（:19）：`ResearchDialog.nodeSize`（scale 1 时的 `Scl.scl(60f)`）。
pub const NODE_SIZE: f32 = 60.0;
/// Java `SPACING`（:20）：两趟布局共用的间距。
pub const SPACING: f32 = 20.0;

/// Java `TechTreeCanvasLayout.Box`（:23-65）：一个画布节点，携带 payload 与可变坐标，就地被布局。
pub struct TechBox<P> {
    /// Java `public final Object payload`（:24）。
    pub payload: P,
    /// Java `public final List<Box> children`（:25）。
    pub children: Vec<TechBox<P>>,
    /// Java `public float x`（:26）。
    pub x: f32,
    /// Java `public float y`（:27）。
    pub y: f32,
    /// Java `public final float width`（:28）。
    pub width: f32,
    /// Java `public final float height`（:29）。
    pub height: f32,
}

impl<P> TechBox<P> {
    /// Java `Box(Object)`（:31-33）：正方形节点，边长取 `NODE_SIZE`。
    pub fn new(payload: P) -> Self {
        Self::with_size(payload, NODE_SIZE, NODE_SIZE)
    }

    /// Java `Box(Object, float, float)`（:35-39）。
    pub fn with_size(payload: P, width: f32, height: f32) -> Self {
        Self {
            payload,
            children: Vec::new(),
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }
}

impl<P> LayoutNode for TechBox<P> {
    /// Java `children()`（:42-44）。
    fn children(&self) -> Vec<&Self> {
        self.children.iter().collect()
    }

    /// 见 `branch_tree_layout` 文件头差异 3：只是同一个 `children` 的可变版本。
    fn children_mut(&mut self) -> Vec<&mut Self> {
        self.children.iter_mut().collect()
    }

    /// Java `width()`（:47-49）。
    fn width(&self) -> f32 {
        self.width
    }

    /// Java `height()`（:52-54）。
    fn height(&self) -> f32 {
        self.height
    }

    /// Java `setX(float)`（:57-59）。
    fn set_x(&mut self, x: f32) {
        self.x = x;
    }

    /// Java `setY(float)`（:62-64）。
    fn set_y(&mut self, y: f32) {
        self.y = y;
    }
}

/// 摆好 `root` 及其整棵子树。根位于自己两个布局半区之间，与游戏完全一致：上半截（`top`）整体平移
/// 两趟根 Y 之差（ResearchDialog.java:355，Java :70-99）。
pub fn layout<P>(root: &mut TechBox<P>) {
    let total = root.children.len();
    // Java :77 `(int) Math.ceil(total / 2f)`：先做浮点除法再向上取整。
    let left_count = (total as f32 / 2.0).ceil() as usize;

    // Java :79-81：`new ArrayList<>(root.children)` 的副本，再取两个 subList 视图。
    let mut rest = std::mem::take(&mut root.children).into_iter();
    let mut left_half: Vec<TechBox<P>> = rest.by_ref().take(left_count).collect();
    let mut right_half: Vec<TechBox<P>> = rest.collect();

    // Java :83-84 `clear()` + `addAll(leftHalf)`。
    root.children = std::mem::take(&mut left_half);
    // Java :85。
    BranchTreeLayout::new(TreeLocation::Top, SPACING, SPACING).layout(root);
    let last_y = root.y;
    // 上半截摆好后先挪回 `left_half`：Java 的 `leftHalf` 是视图，元素始终是同一批节点（:80），
    // Rust 要在后面平移它们、最后再拼回去，就得把所有权拿在自己手里（见文件头差异 3）。
    std::mem::swap(&mut left_half, &mut root.children);

    if !right_half.is_empty() {
        // Java :89-91：换成下半截。
        root.children = std::mem::take(&mut right_half);
        // Java :92。
        BranchTreeLayout::new(TreeLocation::Bottom, SPACING, SPACING).layout(root);
        // Java :93-94：上半截按两趟根 Y 之差平移，让根落在上下两半中间。
        shift(&mut left_half, root.y - last_y);
        right_half = std::mem::take(&mut root.children);
    }

    // Java :97-98 `clear()` + `addAll(children)`：按原顺序恢复全部孩子。
    left_half.extend(right_half);
    root.children = left_half;
}

/// Java `shift(List<Box>, float)`（:102-109）：把一棵子树（含全部后代）按 Y 偏移整体挪动。
fn shift<P>(children: &mut [TechBox<P>], amount: f32) {
    for node in children.iter_mut() {
        node.y += amount;
        if !node.children.is_empty() {
            shift(&mut node.children, amount);
        }
    }
}
