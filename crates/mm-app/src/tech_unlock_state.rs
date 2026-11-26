//! Java `dev.modmaker.ui.TechUnlockState`（88 行）的等价移植：只从树结构推出来的**解锁三态**
//! （报告 P1-8）。
//!
//! 类注释（`TechUnlockState.java:14-23`）直译：编辑器持有的是一份静态的树定义、不是一场战役，
//! 所以「已解锁」跟着游戏自己的规则走——一个 `root:true` 节点在对局一开始就是开的，凡是挂在
//! 一条完整父链下面的都可研究，而链永远够不着根的那些（没有父节点、父节点解析不出来、父节点成环）
//! 保持锁定。它是对 [`TechTreeIndex`] 已经解析完的那片森林的纯逻辑：渲染层只消费算出来的这张表，
//! 自己绝不再走一遍树。
//!
//! ## 落点理由
//!
//! Java 把它放在 `ui` 包里，但它依赖的三样东西全在 Rust 的领域层：森林类型 [`Forest`]／[`Entry`]
//! 来自 `mm-io` 的 [`crate`] 依赖链（`crates/mm-io/src/canvas/tech_tree_index.rs`），规则
//! [`mm_fmt::tech`]（Java `TechTreeRules`）与 [`ContentRecord`]。`mm-app` 正是「Java `ui` 包里与
//! GUI 无关的那部分状态」（决策 D5，见 `crates/mm-app/Cargo.toml` 的 `[package]` 下方注释），
//! 且它已依赖 mm-io、mm-fmt、mm-model，故落在这里，不新增任何一条 crate 依赖。
//!
//! ## 与 Java 的三处结构性偏离（逐字登记，不记暗账）
//!
//! 1. **`Map<String, Unlock>` → [`KeyedMap<Unlock>`]**。Java 用 `LinkedHashMap` 收、最后
//!   `Map.copyOf`（`:62`、`:66`）；Rust 侧的有序表达到处都是 [`KeyedMap`]（它就是
//!   `LinkedHashMap` 的承接者，见 `crates/mm-schema/src/map.rs:78` 的 `put`），键序同样是插入序。
//! 2. **`Set<String>`/`LinkedHashSet` → 保序去重的 `Vec<String>`**。`rootRecordIds` 的返回类型
//!   （`:41`）与 `derive` 的那个入参（`:61`）在 Java 侧只用得到「加入 + 是否包含」两件事，Rust 侧
//!   一律用 `Vec<String>` 承接：加入处显式去重（见 [`root_record_ids`] 里那句 `any`），包含处线性
//!   扫（见 [`root_state`]）——同 `tech_tree_index.rs` 里 `placed: Vec<String>` 那条已签字口径。
//! 3. **`record.id` 在 Rust 是 `Option<String>`**。Java 的 `ContentRecord.id` 是可为 null 的
//!   `String`，`rootRecordIds` 把它直接放进集合（`:48`）；Rust 侧沿用
//!   [`mm_io::canvas::tech_tree_index`] 里 `record_key()`（`tech_tree_index.rs:255`，其说明在
//!   `:253-254`）已签字的同一处置：缺失时并入 `""`，并在注释里点名——现实中记录总是带 id
//!   （导入与新建时分配，见 `ContentRecord::create`）。
//!
//! [`TechTreeIndex`]: mm_io::canvas::tech_tree_index
//! [`Forest`]: mm_io::canvas::tech_tree_index::Forest
//! [`Entry`]: mm_io::canvas::tech_tree_index::Entry

use mm_fmt::tech;
use mm_io::canvas::tech_tree_index::{Entry, Forest, Problem};
use mm_model::ContentRecord;
use mm_schema::KeyedMap;

/// Java `TechUnlockState.Unlock`（`:27-34`）：三个研究状态，名字与画布的那些样式类逐字一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unlock {
    /// 一个 `root:true` 节点：游戏一开始就是开的（Java `:28-29`）。
    Unlocked,
    /// 它的父链完整地够到了一个根（或一个原版锚点）（Java `:30-31`）。
    Researchable,
    /// 链够不到根；整片断掉的子树都是锁的（Java `:32-33`）。
    Locked,
}

/// Java `TechUnlockState.rootRecordIds(List<ContentRecord>)`（`:40-52`）：游戏会拿它们开局的那批
/// 记录——非 patch、非内联、且 `root:true`。
pub fn root_record_ids(records: &[ContentRecord]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for record in records {
        // Java `:43`：`record.isInline() || !TechTreeRules.allowsResearch(record.patch)` 一律跳过。
        if record.is_inline() || !tech::allows_research(record.patch) {
            continue;
        }
        // Java `:46-47`：`research != null && !research.isJsonNull() && TechTreeRules.isRoot(research)`。
        let research = record.fields.get(tech::FIELD).filter(|value| !value.is_null());
        if tech::is_root(research) {
            // 见模块头差异第 3 条：Java 直接 `ids.add(record.id)`，而 `LinkedHashSet` 会去重
            // （`:41`），所以这里同样只在「此前没有这个 id」时才追加。
            let id = record.id.clone().unwrap_or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Java `TechUnlockState.derive(TechTreeIndex.Forest, Set<String>)`（`:61-67`）：
/// 森林里每一个有记录撑着的条目给一个状态。结构性根只有在它是真正的 `root:true` 节点时才算
/// 已解锁：索引也会把断掉的链（带问题的根、成环后活下来的那些）浮成根，而它们把下面一整片锁住。
/// 一个原版锚点组是游戏自带的一个「开着的父节点」，所以它的孩子可研究；这个组自己没有记录，
/// 因而不进这张表（Java 类注释 `:54-60`）。
pub fn derive(forest: &Forest, true_root_ids: &[String]) -> KeyedMap<Unlock> {
    let mut states: KeyedMap<Unlock> = KeyedMap::new();
    for root in &forest.roots {
        walk(root, root_state(root, true_root_ids), &mut states);
    }
    states
}

/// Java `TechUnlockState.walk(Entry, Unlock, Map)`（`:69-77`）。
fn walk(entry: &Entry, state: Unlock, states: &mut KeyedMap<Unlock>) {
    if let Some(content_id) = &entry.content_id {
        states.put(content_id.clone(), state);
    }
    let child_state = if state == Unlock::Locked { Unlock::Locked } else { Unlock::Researchable };
    for child in &entry.children {
        walk(child, child_state, states);
    }
}

/// Java `TechUnlockState.rootState(Entry, Set<String>)`（`:79-87`）。
fn root_state(root: &Entry, true_root_ids: &[String]) -> Unlock {
    if root.vanilla {
        return Unlock::Unlocked;
    }
    if root.problem != Problem::None {
        return Unlock::Locked;
    }
    let true_root = root
        .content_id
        .as_ref()
        .is_some_and(|content_id| true_root_ids.iter().any(|id| id == content_id));
    if true_root {
        Unlock::Unlocked
    } else {
        Unlock::Locked
    }
}
