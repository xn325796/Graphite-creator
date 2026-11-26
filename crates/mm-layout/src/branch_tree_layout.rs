//! `BranchTreeLayout.java` 的等价移植（Java `dev.modmaker.core.layout.BranchTreeLayout`，:22-308）。
//!
//! Java 侧本身就是 `mindustry.ui.layout.BranchTreeLayout`（v160.5）的逐行移植，也就是游戏科技树
//! 真正跑的 abego TreeLayout 游走算法；保留它是为了证明「编辑器画出来的布局」与「游戏画出来的
//! 布局」在同一个间距常量下完全一致。Java 只保留了研究界面会用到的子集：节点一律是正方形
//! （`width == height == nodeSize`）、只用 `TreeLocation.top`/`bottom`，所以 `calcWidth()` 退化成
//! `width()`，也不需要量包围盒。本文件逐语句照抄，不增减算法步骤。
//!
//! # 与 Java 的环境差异（都在语义等价的前提下处理，不改变任何计算）
//!
//! 1. **`Walker` 的循环指针 → 扁平 arena + `usize` 下标**（Java :90-127）。Java 的 `Walker` 用
//!    `parent`/`thread`/`ancestor` 互相指，Rust 里要么 `unsafe` 裸指针、要么 `Rc<RefCell<_>>`，两者
//!    都在禁止清单上。这里把整棵包装树在建树期一次性摊平进 `Vec<Walker>`（前序编号，与 Java 构造器
//!    递归建 `children[]` 的顺序一致），所有引用换成 `usize` 下标、可空的换成 `Option<usize>`。
//!    `==` 的引用相等（Java :178 `ancestor.parent == parentOfV`）换成下标相等，同一对象只有一个下标，
//!    判定结果一致。
//! 2. **`interface Node` → `trait LayoutNode` + 泛型静态分发**（Java :27-37）。Java 的
//!    `List<? extends Node>` 是存在类型，Rust 的 `dyn` 对象安全限制太多（返回 `&mut` 子节点尤其麻
//!    烦），故 `layout` 收 `&mut N where N: LayoutNode`。行为差别只有「每个节点类型单态化一份」，
//!    算法本体一行未改。
//! 3. **写回 x/y 需要一个可变的节点句柄**（Java :298-299 `v.node.setX(...)`）。Java 的 `Node` 只有
//!    `children()`，Rust 里 `secondWalk` 要顺着树走就得再要一个 `children_mut()`；它只是 `children()`
//!    的可变版本，返回的是同一批子节点，不引入新的树结构。
//! 4. **`Walker.x`/`Walker.y` 两个字段省略**（Java :96-97）。Java 保留了 arc 的 `TreeNode` 形状，但
//!    在本类保留的子集里从头到尾没有任何语句读写它们（坐标只经 `node.setX/setY` 落到真实节点上），
//!    照搬只会得到两个 dead-field 告警。
//! 5. **`int level` → `usize`**（Java :62、:83、:283）。层号只从 0 起自增，Java 也没有负数路径；
//!    换成 `usize` 后 `sizeOfLevel.get(level)`（:84）越界时的 panic 与 Java 的
//!    `IndexOutOfBoundsException` 同为「不该发生的就崩」。
//! 6. **`Walker[] children` → `Vec<usize>`**（Java :93）。`children.length` → `len()`、
//!    `children[children.length - 1]` → `children[len - 1]`，索引语义相同。
//! 7. **`float` 除法的无穷大**（Java :183-185 `shift / subtrees`）。Java 里 `subtrees == 0` 得
//!    `Infinity` 而不抛异常，Rust 的 `f32 / f32` 同样得 `inf`，因此 `subtrees` 在 :182 的 `int` 差值
//!    之后转成 `f32` 再除，不引入整数除零 panic。

/// Java `BranchTreeLayout.TreeLocation`（:24）：根往哪个方向生长。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeLocation {
    Top,
    Left,
    Bottom,
    Right,
}

/// Java `BranchTreeLayout.Node`（:27-37）：被布局的节点。`children` 可以为空但不能缺失。
pub trait LayoutNode {
    /// Java `children()`（:28）。
    fn children(&self) -> Vec<&Self>;

    /// Java `children()` 的可变版本（见文件头差异 3），只用于 `secondWalk` 的写回。
    fn children_mut(&mut self) -> Vec<&mut Self>;

    /// Java `width()`（:30）。
    fn width(&self) -> f32;

    /// Java `height()`（:32）。
    fn height(&self) -> f32;

    /// Java `setX(float)`（:34）。
    fn set_x(&mut self, x: f32);

    /// Java `setY(float)`（:36）。
    fn set_y(&mut self, y: f32);
}

/// Java `BranchTreeLayout`（:22）。
pub struct BranchTreeLayout {
    /// Java `public TreeLocation rootLocation`（:39）。
    pub root_location: TreeLocation,
    /// Java `public float gapBetweenLevels`（:41）：研究界面把它设成 20。
    pub gap_between_levels: f32,
    /// Java `public float gapBetweenNodes`（:42）。
    pub gap_between_nodes: f32,
    /// Java `private final List<Float> sizeOfLevel`（:44）：每层最大的节点厚度。
    size_of_level: Vec<f32>,
}

/// Java `BranchTreeLayout.Walker`（:90-104）：携带算法状态的内部节点，对应 arc 的 `TreeNode<T>` 形状。
/// 字段全部换成 arena 下标（见文件头差异 1）；Java 的 `final Node node` 不再需要——写回时按同一前序
/// 遍历直接配对真实节点，见 `second_walk`。
struct Walker {
    parent: Option<usize>,
    children: Vec<usize>,
    width: f32,
    height: f32,
    prelim: f32,
    mode: f32,
    shift: f32,
    change: f32,
    thread: Option<usize>,
    ancestor: usize,
    number: i32,
}

impl BranchTreeLayout {
    /// Java 构造器（:46-50）。
    pub fn new(root_location: TreeLocation, gap_between_levels: f32, gap_between_nodes: f32) -> Self {
        Self {
            root_location,
            gap_between_levels,
            gap_between_nodes,
            size_of_level: Vec::new(),
        }
    }

    /// 布局以 `root` 为根的整棵树，把 x/y 写回每个节点（Java `layout`，:53-60）。
    pub fn layout<N: LayoutNode>(&mut self, root: &mut N) {
        // Java :54 `Walker wrapper = new Walker(root, null)`。
        let mut arena: Vec<Walker> = Vec::new();
        let wrapper = Self::build(&mut arena, root, None);
        // Java :55 `sizeOfLevel.clear()`。
        self.size_of_level.clear();
        self.first_walk(&mut arena, wrapper, None);
        self.calc_size_of_levels(&arena, wrapper, 0);
        let prelim = arena[wrapper].prelim;
        self.second_walk(&arena, wrapper, -prelim, 0, 0.0, root);
    }

    /// Java `Walker` 构造器（:106-117）：递归建子 Walker，`parent` 指向调用方，`ancestor = this`。
    /// 先 `push` 自身再递归，得到的前序编号与 Java 的 `Walker[] children` 结构一一对应。
    fn build<N: LayoutNode>(arena: &mut Vec<Walker>, node: &N, parent: Option<usize>) -> usize {
        let index = arena.len();
        arena.push(Walker {
            parent,
            children: Vec::new(),
            width: node.width(),
            height: node.height(),
            prelim: 0.0,
            mode: 0.0,
            shift: 0.0,
            change: 0.0,
            thread: None,
            ancestor: index,
            number: -1,
        });
        for child in node.children() {
            let child_index = Self::build(arena, child, Some(index));
            arena[index].children.push(child_index);
        }
        index
    }

    /// Java `Walker.isLeaf()`（:119-121）。
    fn is_leaf(arena: &[Walker], v: usize) -> bool {
        arena[v].children.is_empty()
    }

    /// Java `Walker.calcWidth()`（:124-126）：arc 的 `TreeNode.calcWidth`，这里就是普通宽度。
    fn calc_width(arena: &[Walker], v: usize) -> f32 {
        arena[v].width
    }

    /// Java `getWidthOrHeightOfNode(Walker, boolean)`（:129-131）。
    fn get_width_or_height_of_node(&self, arena: &[Walker], node: usize, return_width: bool) -> f32 {
        if return_width {
            Self::calc_width(arena, node)
        } else {
            arena[node].height
        }
    }

    /// Java `getNodeThickness(Walker)`（:133-135）。
    fn get_node_thickness(&self, arena: &[Walker], node: usize) -> f32 {
        self.get_width_or_height_of_node(arena, node, !self.is_level_change_in_y_axis())
    }

    /// Java `getNodeSize(Walker)`（:137-139）。
    fn get_node_size(&self, arena: &[Walker], node: usize) -> f32 {
        self.get_width_or_height_of_node(arena, node, self.is_level_change_in_y_axis())
    }

    /// Java `isLevelChangeInYAxis()`（:141-143）。
    fn is_level_change_in_y_axis(&self) -> bool {
        self.root_location == TreeLocation::Top || self.root_location == TreeLocation::Bottom
    }

    /// Java `getLevelChangeSign()`（:145-147）：`bottom`/`right` 往负方向生长。
    fn get_level_change_sign(&self) -> i32 {
        if self.root_location == TreeLocation::Bottom || self.root_location == TreeLocation::Right {
            -1
        } else {
            1
        }
    }

    /// Java `getGapBetweenNodes(Walker, Walker)`（:149-151）：arc 在此按节点内容微调间距，
    /// 本类保留的子集一律用固定间距，故两个入参都不参与计算。
    fn get_gap_between_nodes(&self, _a: usize, _b: usize) -> f32 {
        self.gap_between_nodes
    }

    /// Java `getDistance(Walker, Walker)`（:153-156）。
    fn get_distance(&self, arena: &[Walker], v: usize, w: usize) -> f32 {
        let size_of_nodes = self.get_node_size(arena, v) + self.get_node_size(arena, w);
        size_of_nodes / 2.0 + self.get_gap_between_nodes(v, w)
    }

    /// Java `nextLeft(Walker)`（:158-160）：叶子走 `thread`（可为 null），否则取第一个孩子。
    fn next_left(&self, arena: &[Walker], v: usize) -> Option<usize> {
        if Self::is_leaf(arena, v) {
            arena[v].thread
        } else {
            Some(arena[v].children[0])
        }
    }

    /// Java `nextRight(Walker)`（:162-164）。
    fn next_right(&self, arena: &[Walker], v: usize) -> Option<usize> {
        if Self::is_leaf(arena, v) {
            arena[v].thread
        } else {
            Some(arena[v].children[arena[v].children.len() - 1])
        }
    }

    /// Java `getNumber(Walker, Walker)`（:166-174）：第一次用到某个孩子时才给双亲的全部孩子编号。
    fn get_number(&self, arena: &mut [Walker], node: usize, parent_node: usize) -> i32 {
        if arena[node].number == -1 {
            let count = arena[parent_node].children.len();
            // Java :169-173 的 `number++` 从 1 起给双亲的孩子依序编号；zip 计数逐值一致。
            for (number, i) in (1i32..).zip(0..count) {
                let child = arena[parent_node].children[i];
                arena[child].number = number;
            }
        }
        arena[node].number
    }

    /// Java `ancestor(Walker, Walker, Walker)`（:176-179）。
    fn ancestor(&self, arena: &[Walker], v_i_minus: usize, parent_of_v: usize, default_ancestor: usize) -> usize {
        let ancestor = arena[v_i_minus].ancestor;
        // Java :178 的 `ancestor.parent == parentOfV` 是引用相等，下标相等与之等价（差异 1）。
        if arena[ancestor].parent == Some(parent_of_v) {
            ancestor
        } else {
            default_ancestor
        }
    }

    /// Java `moveSubtree(Walker, Walker, Walker, float)`（:181-188）。
    fn move_subtree(&self, arena: &mut [Walker], w_minus: usize, w_plus: usize, parent: usize, shift: f32) {
        // Java :182 先 `getNumber(wPlus)` 再 `getNumber(wMinus)`，前者可能触发编号，顺序照抄。
        let subtrees = self.get_number(arena, w_plus, parent) - self.get_number(arena, w_minus, parent);
        // Java :183-185 的 `shift / subtrees` 是浮点除法（`subtrees` 提升为 float），
        // 故此处转 f32 而不是整数除法；除数为 0 时两边同样得到 inf。
        let spread = shift / (subtrees as f32);
        arena[w_plus].change -= spread;
        arena[w_plus].shift += shift;
        arena[w_minus].change += spread;
        arena[w_plus].prelim += shift;
        arena[w_plus].mode += shift;
    }

    /// Java `apportion(Walker, Walker, Walker, Walker)`（:190-244）：把 `v` 与左侧子树分开，
    /// 返回下一轮用的 `defaultAncestor`。
    fn apportion(
        &self,
        arena: &mut [Walker],
        v: usize,
        default_ancestor: usize,
        left_sibling: Option<usize>,
        parent_of_v: usize,
    ) -> usize {
        let Some(left_sibling) = left_sibling else {
            return default_ancestor;
        };

        let mut v_o_plus = v;
        let mut v_i_plus = v;
        let mut v_i_minus = left_sibling;
        let mut v_o_minus = arena[parent_of_v].children[0];

        let mut s_i_plus = arena[v_i_plus].mode;
        let mut s_o_plus = arena[v_o_plus].mode;
        let mut s_i_minus = arena[v_i_minus].mode;
        let mut s_o_minus = arena[v_o_minus].mode;
        // Java :190 的形参 `defaultAncestor` 在两个收尾分支里可能被改写后返回，这里同样用可变局部量。
        let mut default_ancestor = default_ancestor;

        let mut next_right_v_i_minus = self.next_right(arena, v_i_minus);
        let mut next_left_v_i_plus = self.next_left(arena, v_i_plus);

        while let (Some(right_of_inner_minus), Some(left_of_inner_plus)) =
            (next_right_v_i_minus, next_left_v_i_plus)
        {
            v_i_minus = right_of_inner_minus;
            v_i_plus = left_of_inner_plus;
            // Java :211-212 直接 `nextLeft(vOMinus)`，为 null 时下一句 `vOMinus.mode` 就是 NPE；
            // 算法的不变式保证内层游标还没走完时外层游标也不会走完，故 expect 与 Java 同为崩溃。
            v_o_minus = self
                .next_left(arena, v_o_minus)
                .expect("内层左侧还有节点时外层左侧不应先走完");
            v_o_plus = self
                .next_right(arena, v_o_plus)
                .expect("内层左侧还有节点时外层右侧不应先走完");
            arena[v_o_plus].ancestor = v;
            let shift = (arena[v_i_minus].prelim + s_i_minus)
                - (arena[v_i_plus].prelim + s_i_plus)
                + self.get_distance(arena, v_i_minus, v_i_plus);

            if shift > 0.0 {
                let ancestor = self.ancestor(arena, v_i_minus, parent_of_v, default_ancestor);
                self.move_subtree(arena, ancestor, v, parent_of_v, shift);
                s_i_plus += shift;
                s_o_plus += shift;
            }
            s_i_minus += arena[v_i_minus].mode;
            s_i_plus += arena[v_i_plus].mode;
            s_o_minus += arena[v_o_minus].mode;
            s_o_plus += arena[v_o_plus].mode;

            next_right_v_i_minus = self.next_right(arena, v_i_minus);
            next_left_v_i_plus = self.next_left(arena, v_i_plus);
        }

        if next_right_v_i_minus.is_some() && self.next_right(arena, v_o_plus).is_none() {
            arena[v_o_plus].thread = next_right_v_i_minus;
            arena[v_o_plus].mode += s_i_minus - s_o_plus;
        }

        if next_left_v_i_plus.is_some() && self.next_left(arena, v_o_minus).is_none() {
            arena[v_o_minus].thread = next_left_v_i_plus;
            arena[v_o_minus].mode += s_i_plus - s_o_minus;
            default_ancestor = v;
        }
        default_ancestor
    }

    /// Java `executeShifts(Walker)`（:246-257）：把 `apportion` 记在孩子们身上的偏移从右往左累加落实。
    fn execute_shifts(&self, arena: &mut [Walker], v: usize) {
        let mut shift = 0.0f32;
        let mut change = 0.0f32;

        let count = arena[v].children.len();
        for i in (0..count).rev() {
            let w = arena[v].children[i];
            change += arena[w].change;
            arena[w].prelim += shift;
            arena[w].mode += shift;
            shift += arena[w].shift + change;
        }
    }

    /// Java `firstWalk(Walker, Walker)`（:259-281）：自底向上定 `prelim`。
    fn first_walk(&self, arena: &mut [Walker], v: usize, left_sibling: Option<usize>) {
        if Self::is_leaf(arena, v) {
            if let Some(left_sibling) = left_sibling {
                let distance = self.get_distance(arena, v, left_sibling);
                let prelim = arena[left_sibling].prelim + distance;
                arena[v].prelim = prelim;
            }
        } else {
            let mut default_ancestor = arena[v].children[0];
            let mut previous_child: Option<usize> = None;
            let count = arena[v].children.len();
            for i in 0..count {
                let w = arena[v].children[i];
                self.first_walk(arena, w, previous_child);
                default_ancestor = self.apportion(arena, w, default_ancestor, previous_child, v);
                previous_child = Some(w);
            }
            self.execute_shifts(arena, v);
            let midpoint = (arena[arena[v].children[0]].prelim + arena[arena[v].children[count - 1]].prelim) / 2.0;
            if let Some(left_sibling) = left_sibling {
                let distance = self.get_distance(arena, v, left_sibling);
                let prelim = arena[left_sibling].prelim + distance;
                arena[v].prelim = prelim;
                arena[v].mode = prelim - midpoint;
            } else {
                arena[v].prelim = midpoint;
            }
        }
    }

    /// Java `secondWalk(Walker, float, int, float)`（:283-307）：自顶向下把 `prelim` 变成绝对坐标并写回。
    fn second_walk<N: LayoutNode>(
        &self,
        arena: &[Walker],
        v: usize,
        m: f32,
        level: usize,
        level_start: f32,
        node: &mut N,
    ) {
        let level_change_sign = self.get_level_change_sign();
        let level_change_on_y_axis = self.is_level_change_in_y_axis();
        let level_size = self.get_size_of_level(level);

        let mut x = arena[v].prelim + m;

        let mut y = level_start + level_size
            - (level_change_sign as f32) * (self.get_node_thickness(arena, v) / 2.0);

        if !level_change_on_y_axis {
            std::mem::swap(&mut x, &mut y);
        }

        node.set_x(x);
        node.set_y(y);

        if !Self::is_leaf(arena, v) {
            let next_level_start = level_start + (level_size + self.gap_between_levels) * (level_change_sign as f32);
            let mode = arena[v].mode;
            let count = arena[v].children.len();
            let mut kids = node.children_mut();
            // Java :303-305 遍历 `v.children`；arena 的孩子下标与 `children_mut()` 同源同序（见 `build`）。
            for i in 0..count {
                let child = arena[v].children[i];
                self.second_walk(arena, child, m + mode, level + 1, next_level_start, &mut *kids[i]);
            }
        }
    }

    /// Java `calcSizeOfLevels(Walker, int)`（:62-81）：每层最大厚度。
    fn calc_size_of_levels(&mut self, arena: &[Walker], node: usize, level: usize) {
        let old_size = if self.size_of_level.len() <= level {
            self.size_of_level.push(0.0);
            0.0
        } else {
            self.size_of_level[level]
        };

        let size = self.get_node_thickness(arena, node);
        if old_size < size {
            self.size_of_level[level] = size;
        }

        if !Self::is_leaf(arena, node) {
            let count = arena[node].children.len();
            for i in 0..count {
                let child = arena[node].children[i];
                self.calc_size_of_levels(arena, child, level + 1);
            }
        }
    }

    /// Java `getSizeOfLevel(int)`（:83-85）。
    fn get_size_of_level(&self, level: usize) -> f32 {
        self.size_of_level[level]
    }
}
