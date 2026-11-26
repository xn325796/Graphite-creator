//! `LinkGraph.java` 的等价移植：一个项目的记录所隐含的引用连线，以及画板偏离它多远。
//!
//! 对应 Java `dev.modmaker.core.io.LinkGraph`。
//!
//! 边的方向在这里一次性定死：`from` 是**被引用**的内容——箭头从其 ID 端口离开的那张卡——
//! `to` 是那个引用字段 `fieldKey` 持有该引用的记录，也就是箭头进入其输入端口的那张卡。
//! 画布（`NodeCanvasPane`）、`ProjectController.connect` 和本类都用这一个约定，所以一条导入的
//! 连线、一条手画的连线与一条重建出来的连线含义相同。（在本类出现之前，导入器把连线反着写，
//! 于是每条导入的箭头都画反，删掉一条还会把字段留在原地。）
//!
//! 只有两端都能解析成记录的连线才会被推导出来。碰到没有记录的卡（绑到某个原版名字上的载体卡，
//! 或者一张脱落的卡）的连线是作者自己的东西，不是导入器的，[`rebuild`] 对它不予改动。

use mm_fmt::ids::uid;
use mm_fmt::tech;
use mm_json::{leaf_name, steps, Json};
use mm_model::{Board, CanvasEdge, ContentRecord, ModProject};
use mm_schema::{FieldDef, FieldType, KeyedMap, SchemaRegistry};

/// Java `LinkGraph.Diff`：画板偏离它本该镜像的记录有多远。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    pub orphan_nodes: usize,
    pub missing_edges: usize,
    pub stale_edges: usize,
}

impl Diff {
    pub fn clean(&self) -> bool {
        self.orphan_nodes == 0 && self.missing_edges == 0 && self.stale_edges == 0
    }

    pub fn summary(&self) -> String {
        format!(
            "{} orphan card(s), {} missing link(s), {} stale link(s)",
            self.orphan_nodes, self.missing_edges, self.stale_edges
        )
    }
}

/// Java `LinkGraph.Rebuild`：一块画板重建后的连线表加上变化了什么，供调用方写日志与撤销栈。
#[derive(Debug, Clone)]
pub struct Rebuild {
    pub edges: Vec<CanvasEdge>,
    pub added: usize,
    pub removed: usize,
}

/// Java `LinkGraph.derive`：记录所隐含的连线，且只限画板上在场的节点。确定性的。
pub fn derive(project: &ModProject, board: &Board, registry: &SchemaRegistry) -> Vec<CanvasEdge> {
    let node_by_content = node_by_content(board);
    // Java `LinkedHashMap` 的插入序由 `KeyedMap` 承接：名字的先后顺序直接决定边的生成顺序。
    let mut records_by_name: KeyedMap<Vec<&ContentRecord>> = KeyedMap::new();
    for record in &project.contents {
        if !record.is_inline() {
            records_by_name
                .entry_or_insert_with(record.name.clone(), Vec::new)
                .push(record);
        }
    }

    let mut edges: Vec<CanvasEdge> = Vec::new();
    // Java `LinkedHashSet`：本仓库以「Vec + 保序 + 去重」承接（同 `ModProject::asset_paths`）。
    let mut seen: Vec<String> = Vec::new();
    let internal = project.internal_name();

    for record in &project.contents {
        let record_node = match lookup_node(&node_by_content, record.id.as_deref()) {
            Some(node) => node,
            None => continue,
        };
        if record.is_inline() {
            // 包含关系：嵌套记录是被引用的内容，它的持有者保存着容纳它的字段。
            let owner_node = lookup_node(&node_by_content, record.inline_owner_id.as_deref());
            if let Some(owner_node) = owner_node {
                // Java 在 inlinePath 为 null 时抛 NullPointerException；内联记录总是与 ownerId
                // 一起由导入器写下带 path 的那一半，故此处按 panic 承接。
                let inline_path = record
                    .inline_path
                    .as_deref()
                    .expect("内联记录必有 inlinePath（Java 在此处会抛 NullPointerException）");
                add(
                    &mut edges,
                    &mut seen,
                    record_node,
                    owner_node,
                    &first_segment(inline_path),
                    &leaf_name(inline_path),
                );
            }
            continue;
        }

        let class_name = registry.resolve_class(record.ctype, record.class_name().as_deref());
        if let Some(class_name) = class_name {
            for field in registry.fields_of(&class_name).iter() {
                if !field.is_reference() {
                    continue;
                }
                let value = match record.fields.get(field.name()) {
                    Some(value) => value,
                    None => continue,
                };
                for name in reference_names(Some(value), Some(field)) {
                    link(
                        &mut edges,
                        &mut seen,
                        &records_by_name,
                        &node_by_content,
                        record,
                        &name,
                        Some(field),
                        &internal,
                        field.name(),
                    );
                }
            }
        }

        // research 是模组内容接入科技树的唯一途径；schema 把它标得很宽松，所以单独处理。
        let parent = tech::parent(record.fields.get(tech::FIELD));
        if let Some(parent) = parent {
            link(
                &mut edges,
                &mut seen,
                &records_by_name,
                &node_by_content,
                record,
                &parent,
                None,
                &internal,
                tech::FIELD,
            );
        }
    }
    edges
}

/// Java `LinkGraph.diff`：把画板与记录对照——孤立的卡、缺的连线、说谎的连线。
pub fn diff(project: &ModProject, board: &Board, registry: &SchemaRegistry) -> Diff {
    let mut orphan_nodes = 0;
    for node in &board.nodes {
        if let Some(content_id) = &node.content_id
            && project.content_by_id(content_id).is_none()
        {
            orphan_nodes += 1;
        }
    }

    let mut derived: Vec<String> = Vec::new();
    for edge in derive(project, board, registry) {
        set_add(&mut derived, edge_key(&edge));
    }
    let mut existing: Vec<String> = Vec::new();
    let mut stale_edges = 0;
    for edge in &board.edges {
        let key = edge_key(edge);
        set_add(&mut existing, key.clone());
        // 连到一张没有记录的卡（原版载体、脱落的卡）上的连线不归导入器管，
        // 所以它不在推导集合里并不算「说谎」。
        if !derived.contains(&key) && resolves_both_ends(project, board, edge) {
            stale_edges += 1;
        }
    }
    let mut missing_edges = 0;
    for key in &derived {
        if !existing.contains(key) {
            missing_edges += 1;
        }
    }
    Diff {
        orphan_nodes,
        missing_edges,
        stale_edges,
    }
}

/// Java `LinkGraph.rebuild`：从记录重算一块画板的连线。两端都有记录的连线变成记录所隐含的样子
/// （复用已有的边对象，它们的 id 因此保持稳定）；碰到无记录卡片的连线原样带过，
/// 所以手绘的、指向原版内容的引用永远不会被丢掉。
pub fn rebuild(project: &ModProject, board: &Board, registry: &SchemaRegistry) -> Rebuild {
    let derived = derive(project, board, registry);

    let mut existing_by_key: KeyedMap<CanvasEdge> = KeyedMap::new();
    let mut kept: Vec<CanvasEdge> = Vec::new();
    for edge in &board.edges {
        existing_by_key.put(edge_key(edge), edge.clone());
        if !resolves_both_ends(project, board, edge) {
            kept.push(edge.clone());
        }
    }

    let before: Vec<String> = existing_by_key.keys().cloned().collect();
    let mut edges: Vec<CanvasEdge> = Vec::new();
    let mut after: Vec<String> = Vec::new();
    for edge in &derived {
        let key = edge_key(edge);
        set_add(&mut after, key.clone());
        // Java 复用同一个 CanvasEdge 实例以保持 id 稳定；Rust 的所有权模型下改为克隆那个
        // 既有对象，id 与其余字段同样原封不动地带过来。
        match existing_by_key.get(&key) {
            Some(prior) => edges.push(prior.clone()),
            None => edges.push(copy(edge)),
        }
    }
    for edge in kept {
        if set_add(&mut after, edge_key(&edge)) {
            edges.push(edge);
        }
    }

    let mut added = 0;
    for key in &after {
        if !before.contains(key) {
            added += 1;
        }
    }
    let mut removed = 0;
    for key in &before {
        if !after.contains(key) {
            removed += 1;
        }
    }
    Rebuild {
        edges,
        added,
        removed,
    }
}

// --- 辅助 --------------------------------------------------------------------------------

// 参数表与 Java 侧链接辅助方法的入参一一对应，为保持对应关系不打包成结构体。
#[allow(clippy::too_many_arguments)]
fn link(
    edges: &mut Vec<CanvasEdge>,
    seen: &mut Vec<String>,
    records_by_name: &KeyedMap<Vec<&ContentRecord>>,
    node_by_content: &KeyedMap<Option<String>>,
    record: &ContentRecord,
    target_name: &str,
    field: Option<&FieldDef>,
    internal: &str,
    field_key: &str,
) {
    let target = resolve_target(records_by_name, target_name, field, internal);
    let target = match target {
        Some(target) => target,
        None => return,
    };
    let from_node = lookup_node(node_by_content, target.id.as_deref());
    let to_node = lookup_node(node_by_content, record.id.as_deref());
    match (from_node, to_node) {
        (Some(from_node), Some(to_node)) if from_node != to_node => {
            add(edges, seen, from_node, to_node, field_key, target_name);
        }
        _ => {}
    }
}

fn add(
    edges: &mut Vec<CanvasEdge>,
    seen: &mut Vec<String>,
    from: &str,
    to: &str,
    field_key: &str,
    label: &str,
) {
    let mut edge = CanvasEdge {
        from: Some(from.to_string()),
        to: Some(to.to_string()),
        field_key: Some(field_key.to_string()),
        label: label.to_string(),
        ..Default::default()
    };
    if !set_add(seen, edge_key(&edge)) {
        return;
    }
    edge.id = Some(uid("e"));
    edges.push(edge);
}

fn copy(edge: &CanvasEdge) -> CanvasEdge {
    let mut clone = CanvasEdge::default();
    clone.id = Some(uid("e"));
    clone.from = edge.from.clone();
    clone.to = edge.to.clone();
    clone.field_key = edge.field_key.clone();
    clone.label = edge.label.clone();
    clone
}

/// Java `key(CanvasEdge)`：Java 的字符串拼接把 null 写成字面量 "null"，此处逐字照搬。
fn edge_key(edge: &CanvasEdge) -> String {
    format!(
        "{}->{}#{}",
        null_as_str(edge.from.as_deref()),
        null_as_str(edge.to.as_deref()),
        null_as_str(edge.field_key.as_deref())
    )
}

fn null_as_str(value: Option<&str>) -> &str {
    value.unwrap_or("null")
}

fn node_by_content(board: &Board) -> KeyedMap<Option<String>> {
    let mut by_content: KeyedMap<Option<String>> = KeyedMap::new();
    for node in &board.nodes {
        if let Some(content_id) = &node.content_id {
            // Java `putIfAbsent`：同一个 contentId 上后到的节点不覆盖先到的。
            by_content.put_if_absent(content_id.clone(), node.id.clone());
        }
    }
    by_content
}

/// Java 的 `map.get(key)`：key 为 null 返回 null，映射到的值为 null 同样返回 null。
fn lookup_node<'m>(map: &'m KeyedMap<Option<String>>, key: Option<&str>) -> Option<&'m str> {
    let key = key?;
    map.get(key).and_then(|value| value.as_deref())
}

fn record_of_node<'a>(
    project: &'a ModProject,
    board: &'a Board,
    node_id: Option<&str>,
) -> Option<&'a ContentRecord> {
    for node in &board.nodes {
        if node.id.is_some() && node.id.as_deref() == node_id {
            return match &node.content_id {
                None => None,
                Some(content_id) => project.content_by_id(content_id),
            };
        }
    }
    None
}

fn resolves_both_ends(project: &ModProject, board: &Board, edge: &CanvasEdge) -> bool {
    record_of_node(project, board, edge.from.as_deref()).is_some()
        && record_of_node(project, board, edge.to.as_deref()).is_some()
}

/// Java `LinkGraph.referencedNames`：一个字段值引用到的内容名字，容忍画布逐块连线视图所需的
/// 游戏能读出来的每一种形状。
pub fn referenced_names(value: Option<&Json>, field: Option<&FieldDef>) -> Vec<String> {
    reference_names(value, field)
}

fn reference_names(value: Option<&Json>, field: Option<&FieldDef>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    collect_names(value, field, &mut names);
    names
}

fn collect_names(value: Option<&Json>, field: Option<&FieldDef>, out: &mut Vec<String>) {
    let value = match value {
        Some(value) => value,
        None => return,
    };
    if value.is_null() {
        return;
    }
    // 炮塔的弹药表：键（KEYS）就是 item/liquid 引用（值是内联子弹，上面的包含关系连线
    // 已经把它们覆盖）。
    if let Some(field) = field
        && field.field_type() == FieldType::Struct
        && field.stack_kind() == Some("ammoMap")
        && value.is_object()
    {
        if let Some(object) = value.as_object() {
            for key in object.keys() {
                out.push(key.clone());
            }
        }
        return;
    }
    if value.is_array() {
        if let Some(items) = value.as_array() {
            for element in items {
                collect_names(Some(element), field, out);
            }
        }
        return;
    }
    // Java 先判 `isJsonPrimitive()`、再在其内部用 `isString()` 早退；数字与布尔在这里同样被丢弃。
    if matches!(value, Json::Str(_) | Json::Num(_) | Json::Bool(_)) {
        let text = match value {
            Json::Str(text) => text,
            _ => return,
        };
        let mut text = text.trim().to_string();
        if text.is_empty() || text.starts_with("icon-") {
            return;
        }
        if let Some(field) = field
            && (field.field_type().is_stack() || field.field_type() == FieldType::Struct)
            && let Some(slash) = text.find('/')
            && slash > 0
        {
            text.truncate(slash);
        }
        out.push(text);
        return;
    }
    if value.is_object() {
        let object = match value.as_object() {
            Some(object) => object,
            None => return,
        };
        for key in CONTENT_REFERENCE_KEYS {
            collect_named_string(object.get(key), field, out);
        }
        // 结构化组把引用嵌在下一层（UnitReq 的 requirements 数组、consume 组里的 items）。
        // 只递归进容器：未知键下的任意字符串——比如子弹的 "type"——必须原样不动。
        for (entry_key, entry_value) in object.iter() {
            if !CONTENT_REFERENCE_KEYS.contains(&entry_key.as_str())
                && (entry_value.is_array() || entry_value.is_object())
            {
                collect_names(Some(entry_value), field, out);
            }
        }
    }
}

/// 一个带名字的引用值（"liquid": "water/0.25"）——栈的拼写被剥掉。
fn collect_named_string(named: Option<&Json>, field: Option<&FieldDef>, out: &mut Vec<String>) {
    let named = match named {
        Some(named) => named,
        None => return,
    };
    let named = match named {
        Json::Str(named) => named,
        _ => return,
    };
    let mut text = named.trim().to_string();
    if text.is_empty() || text.starts_with("icon-") {
        return;
    }
    if let Some(field) = field
        && (field.field_type().is_stack() || field.field_type() == FieldType::Struct)
        && let Some(slash) = text.find('/')
        && slash > 0
    {
        text.truncate(slash);
    }
    out.push(text);
}

/// Java `LinkGraph.CONTENT_REFERENCE_KEYS`。
const CONTENT_REFERENCE_KEYS: [&str; 6] = [
    "item",
    "liquid",
    "payload",
    "block",
    "unit",
    "content",
];

fn resolve_target<'a>(
    records_by_name: &KeyedMap<Vec<&'a ContentRecord>>,
    name: &str,
    field: Option<&FieldDef>,
    internal: &str,
) -> Option<&'a ContentRecord> {
    let mut bare = name;
    let prefix = format!("{}-", internal);
    if !internal.is_empty() && bare.starts_with(prefix.as_str()) {
        bare = &bare[prefix.len()..];
    }
    let want_ctype = field.and_then(|field| field.content_ctype());
    let is_struct = matches!(field, Some(field) if field.field_type() == FieldType::Struct);
    let mut untyped: Option<&'a ContentRecord> = None;
    for record in records_by_name
        .get(bare)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if want_ctype.is_none()
            || want_ctype == Some("any")
            || want_ctype == Some(record.ctype.name())
        {
            return Some(record);
        }
        // 结构化组混用引用种类（一个 UnitReq 同时点出工厂方块与 item 成本）；
        // 记住第一个名字上命中、类型任意的记录作为兜底。
        if is_struct && untyped.is_none() {
            untyped = Some(record);
        }
    }
    untyped
}

/// 内联路径所落在的字段：`ammoTypes[0]` 与 `ammoTypes.copper` 都指向 `ammoTypes`，
/// 于是包含关系的连线指向持有者真正拥有的那个端口。
fn first_segment(path: &str) -> String {
    let path_steps = steps(path);
    let first = match path_steps.first() {
        Some(first) => first,
        None => return path.to_string(),
    };
    if first.is_index {
        path.to_string()
    } else {
        first.key.clone().unwrap_or_default()
    }
}

/// Java `LinkedHashSet.add`：值不存在则按序追加并返回 true（与 Java 的返回值语义一致）。
fn set_add(set: &mut Vec<String>, value: String) -> bool {
    if set.iter().any(|existing| existing == &value) {
        return false;
    }
    set.push(value);
    true
}
