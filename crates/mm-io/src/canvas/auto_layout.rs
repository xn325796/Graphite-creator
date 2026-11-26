//! `AutoLayout.java` 的等价移植：导入内容的确定性布局。
//!
//! 对应 Java `dev.modmaker.core.io.AutoLayout`（`public final class` + 私有构造的纯静态工具类）。
//!
//! 每种内容类型占一条列带，顺序按类型声明序，某类型条目过多时折进额外的子列。之所以强调
//! **确定性**：同一份内容表跑出来的坐标必须每次一样，导入结果才可复现、可比对。

use mm_fmt::ids::uid;
use mm_model::canvas_node::KIND_CONTENT;
use mm_model::{Board, CanvasNode, ContentType, ModProject};
use mm_schema::KeyedMap;

/// 卡片列宽（Java `public static final double CARD_WIDTH`）。
pub const CARD_WIDTH: f64 = 260.0;
/// 卡片行高。
pub const CARD_HEIGHT: f64 = 130.0;
/// item 与 liquid 渲染成轻量载体卡，所以它们的节点更矮。
pub const CARRIER_HEIGHT: f64 = 64.0;

const GAP_X: f64 = 60.0;
const GAP_Y: f64 = 28.0;
const MARGIN: f64 = 60.0;
/// 一列最多放这么多行，超出的折进下一条子列。
const ROWS_PER_COLUMN: i32 = 18;

/// 布局原点，与「单独放一张新卡」的代码共用（Java 的 `margin()`）。
pub fn margin() -> f64 {
    MARGIN
}

/// 卡片之间的纵向间距，同样的原因（Java 的 `gap()` 返回的是 `GAP_Y`，不是 `GAP_X`）。
pub fn gap() -> f64 {
    GAP_Y
}

/// 每种内容记录摆一个节点，返回「记录 id -> 节点 id」的映射（Java 的 `apply`）。
///
/// Java 里返回值是 `LinkedHashMap`，Rust 用 [`KeyedMap`] 对应——插入序就是 `project.contents`
/// 的遍历序。
pub fn apply(project: &ModProject, board: &mut Board) -> KeyedMap<String> {
    let types = ContentType::all();
    let mut nodes_by_content = KeyedMap::new();
    let mut counts = vec![0i32; types.len()];
    for record in &project.contents {
        counts[type_index(&types, record.ctype)] += 1;
    }

    // Java 的 `columnStart`：列起点按 `ContentType.values()` 的声明序从左到右铺开。
    let mut column_start = vec![None; types.len()];
    let mut x = MARGIN;
    for ctype in &types {
        let count = counts[type_index(&types, *ctype)];
        if count == 0 {
            continue;
        }
        let index = type_index(&types, *ctype);
        column_start[index] = Some(x);
        let sub_columns = (count + ROWS_PER_COLUMN - 1) / ROWS_PER_COLUMN;
        x += (sub_columns as f64) * (CARD_WIDTH + GAP_X);
    }

    let mut placed = vec![0i32; types.len()];
    for record in &project.contents {
        let index = type_index(&types, record.ctype);
        // Java：`placed.merge(record.type, 1, Integer::sum) - 1`。
        placed[index] += 1;
        let placed_index = placed[index] - 1;
        let sub_column = placed_index / ROWS_PER_COLUMN;
        let row = placed_index % ROWS_PER_COLUMN;
        let carrier = record.ctype == ContentType::Item || record.ctype == ContentType::Liquid;
        let row_height = if carrier { CARRIER_HEIGHT } else { CARD_HEIGHT };
        let mut node = CanvasNode::default();
        let node_id = uid("n");
        node.id = Some(node_id.clone());
        node.kind = KIND_CONTENT.to_string();
        node.content_id = record.id.clone();
        // Java 此处直接对 `columnStart.get(type)` 拆箱：缺键就是 NPE。列起点与计数同源于
        // 同一份声明序数组，凡是进过 `counts` 的类型必有起点，故用 expect 保留同样的「越界即错」。
        node.x = column_start[index].expect("计过数的类型必有列起点")
            + (sub_column as f64) * (CARD_WIDTH + GAP_X);
        node.y = MARGIN + (row as f64) * (row_height + GAP_Y);
        node.h = row_height;
        board.nodes.push(node);
        // 记录总是带 id（导入与新建时分配），Java 里的 null 键分支在 `KeyedMap<String>` 上无法
        // 表达、也不会发生。
        if let Some(record_id) = &record.id {
            nodes_by_content.put(record_id.clone(), node_id);
        }
    }
    nodes_by_content
}

/// 列起点表，按 `ContentType` 声明序定长；没内容的类型是 `None`。
fn column_starts(project: &ModProject, types: &[ContentType]) -> Vec<Option<f64>> {
    let mut starts = vec![None; types.len()];
    let mut counts = vec![0i32; types.len()];
    for record in &project.contents {
        counts[type_index(types, record.ctype)] += 1;
    }
    let mut x = MARGIN;
    for ctype in types {
        let index = type_index(types, *ctype);
        let count = counts[index];
        if count == 0 {
            continue;
        }
        starts[index] = Some(x);
        let sub_columns = (count + ROWS_PER_COLUMN - 1) / ROWS_PER_COLUMN;
        x += (sub_columns as f64) * (CARD_WIDTH + GAP_X);
    }
    starts
}

/// 重新摆放一块画布上的节点，供「整理布局」这个动作用（Java 的 `tidy`）。
///
/// 摆放顺序取自 `project.contents`，列带取自 `ContentType` 声明序，两者都固定，所以结果可复现。
pub fn tidy(project: &ModProject, board: &mut Board) {
    let types = ContentType::all();
    let mut order = KeyedMap::new();
    for (i, record) in project.contents.iter().enumerate() {
        if let Some(id) = &record.id {
            order.put(id.clone(), i as i32);
        }
    }

    // Java：`board.nodes.stream().filter(...).sorted(comparator).toList()`。`sorted` 是稳定排序，
    // 所以这里按「板内下标 + 排序键」稳定排序，得到的序列与 Java 完全一致（键并列时保持原相对顺序）。
    let mut content_nodes: Vec<(usize, i32)> = Vec::new();
    for (i, node) in board.nodes.iter().enumerate() {
        if node.kind == KIND_CONTENT {
            let key = node
                .content_id
                .as_deref()
                .and_then(|id| order.get(id).copied())
                // Java 的兜底是 `Integer.MAX_VALUE`：查不到记录的行按 id 字典序排在尾部。
                .unwrap_or(i32::MAX);
            content_nodes.push((i, key));
        }
    }
    content_nodes.sort_by_key(|a| a.1);

    let column_start = column_starts(project, &types);
    let mut placed = vec![0i32; types.len()];
    let mut positions: KeyedMap<[f64; 2]> = KeyedMap::new();
    for (node_index, _) in &content_nodes {
        let node = &board.nodes[*node_index];
        let record = node
            .content_id
            .as_deref()
            .and_then(|id| project.content_by_id(id));
        let record = match record {
            Some(record) => record,
            None => continue,
        };
        let index = type_index(&types, record.ctype);
        // Java：`placed.merge(record.type, 1, Integer::sum) - 1`。
        placed[index] += 1;
        let placed_index = placed[index] - 1;
        let sub_column = placed_index / ROWS_PER_COLUMN;
        let row = placed_index % ROWS_PER_COLUMN;
        let carrier = record.ctype == ContentType::Item || record.ctype == ContentType::Liquid;
        if let Some(id) = &node.id {
            let row_height = if carrier { CARRIER_HEIGHT } else { CARD_HEIGHT };
            positions.put(
                id.clone(),
                [
                    // Java `getOrDefault(type, MARGIN)`：类型没占列时退回原点。
                    column_start[index].unwrap_or(MARGIN)
                        + (sub_column as f64) * (CARD_WIDTH + GAP_X),
                    MARGIN + (row as f64) * (row_height + GAP_Y),
                ],
            );
        }
    }
    for node in &mut board.nodes {
        if let Some(position) = node.id.as_deref().and_then(|id| positions.get(id)) {
            node.x = position[0];
            node.y = position[1];
        }
    }
}

/// Java 用 `ContentType.values()` 与 `Map<ContentType, …>`，两者都按枚举声明序。Rust 侧
/// [`ContentType::all`] 就是同一份顺序，且类型总数固定，因此「按类型的计数/列起点」直接用这个
/// 顺序的定长 `Vec` 来表达：下标即声明序，迭代顺序即声明顺序，全程不引入任何哈希表，
/// 布局结果与输入顺序无关。
fn type_index(types: &[ContentType], ctype: ContentType) -> usize {
    types
        .iter()
        .position(|candidate| *candidate == ctype)
        .expect("ContentType::all() 覆盖全部枚举值")
}
