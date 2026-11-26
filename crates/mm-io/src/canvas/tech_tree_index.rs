//! `TechTreeIndex.java` 的等价移植：记录所描述的科技树——对每条记录 `research` 字段的派生视图，
//! 绝不是它的第二份拷贝。
//!
//! 对应 Java `dev.modmaker.core.io.TechTreeIndex`。
//!
//! 忠于 `ContentParser.java:1337-1418`：
//! - 只要一条记录带着 `research` 值，无论它长什么样子，节点就存在，因此它可以是别人的父节点
//!   （`TechNode` 在其构造函数里就登记进了 `TechTree.java:88`）；
//! - patch 根本不许带 research（ContentParser.java:1338-1341）——这些记录被单独上报，永不入树；
//! - 既不是 `root` 也没点出可查找父节点的节点被留在树外，游戏会对它告警并忽略（ContentParser.java:1414-1416）；
//! - 父节点既不是模组节点也不是已知的原版名字，节点同样脱落，游戏也会对此告警（ContentParser.java:1403-1405）；
//! - 原版树在任何模组加载之前就已建好（ContentLoader.java:72-79），所以一个两边都存在的裸名字
//!   绑到**原版**节点上——只有 `<mod>-name` 这种拼法才够得着模组自己的节点（ContentParser.java:1401）。

use mm_fmt::{tech, ContentType};
use mm_json::Json;
use mm_model::{ContentRecord, ModProject};
use mm_schema::{KeyedMap, SchemaRegistry};

/// 一条记录为什么不是树里的普通成员——镜像游戏的那些告警。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    None,
    /// 既不是 root，也不是游戏能找到的父节点；游戏告警且从不接上它。
    Ignored,
    /// `parent` 里的名字既匹配不上任何模组节点，也匹配不上任何已知原版内容。
    UnresolvedParent,
    /// 一条 patch：游戏拒绝让它改科技树。
    PatchRefused,
}

/// 一个树节点。模组只是指向的那个原版节点，其 `content_id` 为 null。
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub content_id: Option<String>,
    pub label: String,
    pub vanilla: bool,
    pub problem: Problem,
    pub children: Vec<Entry>,
}

/// Java `TechTreeIndex.Forest`：完整图景——要展示的森林，加上刻意不在里面的那些记录。
/// `unresolved_parents` 与 `ignored` 里的条目同时也出现在 `roots` 里——它们的子节点在游戏里
/// 仍然挂在它们下面，因为它们的节点确实存在于 `TechTree.all` 中。
#[derive(Debug, Clone, PartialEq)]
pub struct Forest {
    pub roots: Vec<Entry>,
    pub unassigned: Vec<Entry>,
    pub unresolved_parents: Vec<Entry>,
    pub ignored: Vec<Entry>,
    pub patches_with_research: Vec<Entry>,
}

/// Java `TechTreeIndex.build`。
pub fn build(project: &ModProject, registry: &SchemaRegistry) -> Forest {
    let internal = project.internal_name();
    let mut records: Vec<&ContentRecord> = Vec::new();
    for record in &project.contents {
        if !record.is_inline() {
            records.push(record);
        }
    }

    // 任何带着 research 的记录都是可能被点名为父节点的节点，无论那个值长什么样——
    // 游戏在校验它之前就把节点登记好了。名字→记录的插入序由 `KeyedMap` 承接。
    let mut nodes_by_name: KeyedMap<&ContentRecord> = KeyedMap::new();
    for &record in &records {
        if is_tree_node(record) {
            nodes_by_name.put_if_absent(record.name.clone(), record);
            nodes_by_name.put_if_absent(record.full_name(&internal), record);
        }
    }

    let mut root_records: Vec<&ContentRecord> = Vec::new();
    let mut ignored_records: Vec<&ContentRecord> = Vec::new();
    let mut unresolved_records: Vec<&ContentRecord> = Vec::new();
    let mut unassigned: Vec<Entry> = Vec::new();
    let mut refused: Vec<Entry> = Vec::new();
    let mut mod_children: KeyedMap<Vec<&ContentRecord>> = KeyedMap::new();
    let mut vanilla_children: KeyedMap<Vec<&ContentRecord>> = KeyedMap::new();

    for &record in &records {
        let research = research_of(record);
        let research = match research {
            Some(research) => research,
            None => {
                unassigned.push(Entry {
                    content_id: record.id.clone(),
                    label: record.name.clone(),
                    vanilla: false,
                    problem: Problem::None,
                    children: Vec::new(),
                });
                continue;
            }
        };
        if !tech::allows_research(record.patch) {
            refused.push(Entry {
                content_id: record.id.clone(),
                label: record.name.clone(),
                vanilla: false,
                problem: Problem::PatchRefused,
                children: Vec::new(),
            });
            continue;
        }
        if tech::is_root(Some(research)) {
            root_records.push(record);
            continue;
        }
        let parent = match tech::parent(Some(research)) {
            Some(parent) => parent,
            None => {
                ignored_records.push(record);
                continue;
            }
        };
        let prefix = format!("{}-", internal);
        let parent_record = if parent.starts_with(prefix.as_str()) {
            nodes_by_name.get(parent.as_str()).copied()
        } else if is_vanilla(registry, &parent) {
            vanilla_children
                .entry_or_insert_with(parent.to_lowercase(), Vec::new)
                .push(record);
            continue;
        } else {
            nodes_by_name.get(parent.as_str()).copied()
        };
        match parent_record {
            Some(parent_record) => {
                mod_children
                    .entry_or_insert_with(parent_record.name.clone(), Vec::new)
                    .push(record);
            }
            None => unresolved_records.push(record),
        }
    }

    // Java `LinkedHashSet<String>`：以「Vec + 保序 + 去重」承接。
    let mut placed: Vec<String> = Vec::new();
    let mut roots: Vec<Entry> = Vec::new();
    for &record in &root_records {
        roots.push(build_entry(record, Problem::None, &mod_children, &mut placed));
    }
    for &record in &unresolved_records {
        roots.push(build_entry(
            record,
            Problem::UnresolvedParent,
            &mod_children,
            &mut placed,
        ));
    }
    for &record in &ignored_records {
        roots.push(build_entry(
            record,
            Problem::Ignored,
            &mod_children,
            &mut placed,
        ));
    }
    for (group_key, group) in vanilla_children.iter() {
        let mut children: Vec<Entry> = Vec::new();
        for &child in group {
            children.push(build_entry(child, Problem::None, &mod_children, &mut placed));
        }
        roots.push(Entry {
            content_id: None,
            label: group_key.clone(),
            vanilla: true,
            problem: Problem::None,
            children,
        });
    }
    // 一条成环的父节点链会让某些节点从任何根都到不了；把它们浮出来，而不是从这个视图里
    // 悄悄丢掉。必须最后跑，否则已经被某个原版组认领的节点还会多出一条假根。
    for &record in &records {
        if is_tree_node(record) && !placed.contains(&record_key(record)) {
            roots.push(build_entry(record, Problem::None, &mod_children, &mut placed));
        }
    }

    unassigned.sort_by(|a, b| a.label.cmp(&b.label));
    let unresolved_parents = problem_entries(&roots, Problem::UnresolvedParent);
    let ignored = problem_entries(&roots, Problem::Ignored);
    Forest {
        roots,
        unassigned,
        unresolved_parents,
        ignored,
        patches_with_research: refused,
    }
}

/// 字段的值；记录完全没带 research 时为 null。
fn research_of(record: &ContentRecord) -> Option<&Json> {
    record
        .fields
        .get(tech::FIELD)
        .filter(|research| !research.is_null())
}

fn is_tree_node(record: &ContentRecord) -> bool {
    research_of(record).is_some() && tech::allows_research(record.patch)
}

fn is_vanilla(registry: &SchemaRegistry, name: &str) -> bool {
    let lower = name.to_lowercase();
    for ctype in ContentType::all() {
        if registry.is_vanilla_name(ctype, &lower) {
            return true;
        }
    }
    false
}

fn build_entry<'a>(
    record: &'a ContentRecord,
    problem: Problem,
    mod_children: &KeyedMap<Vec<&'a ContentRecord>>,
    placed: &mut Vec<String>,
) -> Entry {
    placed_push(placed, record_key(record));
    let mut children: Vec<Entry> = Vec::new();
    for child in mod_children
        .get(record.name.as_str())
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if !placed.contains(&record_key(child)) {
            children.push(build_entry(child, Problem::None, mod_children, placed));
        }
    }
    Entry {
        content_id: record.id.clone(),
        label: record.name.clone(),
        vanilla: false,
        problem,
        children,
    }
}

/// 带某个给定问题的条目，按展示顺序、不重复。
fn problem_entries(roots: &[Entry], problem: Problem) -> Vec<Entry> {
    let mut found: Vec<Entry> = Vec::new();
    for root in roots {
        if root.problem == problem {
            found.push(root.clone());
        }
    }
    found
}

/// Java 的 placed 集合以 `record.id`（String）为键；Rust 侧 id 是 `Option`，缺失时并入 ""。
/// 现实中记录总是带 id（导入与新建时分配，见 `ContentRecord::create`）。
fn record_key(record: &ContentRecord) -> String {
    record.id.clone().unwrap_or_default()
}

/// Java `LinkedHashSet.add`。
fn placed_push(placed: &mut Vec<String>, value: String) {
    if !placed.contains(&value) {
        placed.push(value);
    }
}
