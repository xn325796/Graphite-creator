//! `TechTreeRules.java` 的等价移植：`research` 字段的读取与写出——这是模组建内容进入科技树的
//! 唯一途径（没有科技树文件；`ContentParser.java:1337-1418` 从内容里构建节点）。
//!
//! 一个普通字符串表示"父内容名"。一个对象可以带 parent、requirements、objectives、planet 与
//! root。这里的一切都逐键对应游戏的 `ContentParser.readFields`（`ContentParser.java:1348-1396`）：
//! 游戏不读的键永不写出，只点名一个 parent 的节点保持为裸字符串。
//!
//! Java 侧是 Gson 的 `JsonElement`/`JsonObject`/`JsonArray`，这里是 [`mm_json::Json`] 与
//! [`mm_json::Object`]；Gson 的 `isJsonPrimitive()` 对应 [`is_primitive`]，`deepCopy()` 在
//! owned 的 Rust 树里就是一次 `clone()`。

use mm_json::{Json, Object};

/// 承载研究字段的键。
pub const FIELD: &str = "research";

/// 游戏自带的那些 objective 类型，即 `Objectives` 里的类（Objectives.java）。
/// 元素顺序是可观察的，原样照搬。
pub const OBJECTIVE_TYPES: &[&str] =
    &["Research", "Produce", "SectorComplete", "OnSector", "OnPlanet"];

/// 游戏会读到的一个研究节点。`requirements` 保留游戏接受 `ItemStack` 的 `"item/amount"`
/// 文本形式（`ContentParser.java:466`），`objectives` 保留为原始对象，
/// 于是未知键经过一次编辑也原样存活。
///
/// 对应 Java 的 `record Node`，字段顺序与可空性一致；Java 的不可变性由 `with_*`
/// 一律返回新节点来保持。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub parent: Option<String>,
    pub root: bool,
    pub name: Option<String>,
    pub requires_unlock: bool,
    pub planet: Option<String>,
    pub requirements: Vec<String>,
    pub objectives: Vec<Object>,
}

impl Node {
    /// record 的规范构造器。
    pub fn new(
        parent: Option<String>,
        root: bool,
        name: Option<String>,
        requires_unlock: bool,
        planet: Option<String>,
        requirements: Vec<String>,
        objectives: Vec<Object>,
    ) -> Node {
        Node { parent, root, name, requires_unlock, planet, requirements, objectives }
    }

    pub fn empty() -> Node {
        Node::new(None, false, None, false, None, Vec::new(), Vec::new())
    }

    /// 什么都没有设置，即该字段不携带任何研究节点。
    pub fn is_empty(&self) -> bool {
        self.parent.is_none()
            && !self.root
            && self.name.is_none()
            && !self.requires_unlock
            && self.planet.is_none()
            && self.requirements.is_empty()
            && self.objectives.is_empty()
    }

    /// 换了一个 parent 的副本（root 同时被清掉，因为游戏在那之后会忽略 parent）。
    pub fn with_parent(&self, new_parent: Option<&str>) -> Node {
        Node::new(
            blank_to_null(new_parent),
            false,
            self.name.clone(),
            self.requires_unlock,
            self.planet.clone(),
            self.requirements.clone(),
            self.objectives.clone(),
        )
    }

    pub fn with_root(&self, new_root: bool) -> Node {
        Node::new(
            if new_root { None } else { self.parent.clone() },
            new_root,
            self.name.clone(),
            self.requires_unlock,
            self.planet.clone(),
            self.requirements.clone(),
            self.objectives.clone(),
        )
    }

    pub fn with_planet(&self, new_planet: Option<&str>) -> Node {
        Node::new(
            self.parent.clone(),
            self.root,
            self.name.clone(),
            self.requires_unlock,
            blank_to_null(new_planet),
            self.requirements.clone(),
            self.objectives.clone(),
        )
    }

    pub fn with_requirements(&self, new_requirements: Vec<String>) -> Node {
        Node::new(
            self.parent.clone(),
            self.root,
            self.name.clone(),
            self.requires_unlock,
            self.planet.clone(),
            new_requirements,
            self.objectives.clone(),
        )
    }

    pub fn with_objectives(&self, new_objectives: Vec<Object>) -> Node {
        Node::new(
            self.parent.clone(),
            self.root,
            self.name.clone(),
            self.requires_unlock,
            self.planet.clone(),
            self.requirements.clone(),
            new_objectives,
        )
    }
}

/// 读取 `ContentParser.java:1348-1396` 会读的那七个键。
pub fn read(research: Option<&Json>) -> Node {
    let Some(research) = research else {
        return Node::empty();
    };
    if research.is_null() {
        return Node::empty();
    }
    if is_primitive(research) {
        return Node::new(
            blank_to_null(Some(&research.get_as_string())),
            false,
            None,
            false,
            None,
            Vec::new(),
            Vec::new(),
        );
    }
    let Some(object) = research.as_object() else {
        return Node::empty();
    };
    Node::new(
        string(object, "parent"),
        bool(object, "root"),
        string(object, "name"),
        bool(object, "requiresUnlock"),
        string(object, "planet"),
        requirements(object),
        objectives(object),
    )
}

/// 按游戏读取的方式序列化一个节点：只写 parent 名字时保持为字符串，其它任何东西变成对象。
/// 没有可写内容时返回 `None`，好让调用方把字段整个移除。
pub fn write(node: Option<&Node>) -> Option<Json> {
    let node = node.filter(|node| !node.is_empty())?;
    let bare = !node.root
        && node.parent.is_some()
        && node.planet.is_none()
        && node.name.is_none()
        && !node.requires_unlock
        && node.requirements.is_empty()
        && node.objectives.is_empty();
    if bare {
        // Java: new JsonPrimitive(node.parent())
        return Some(Json::str(node.parent.clone().expect("bare node has a parent")));
    }
    let mut object = Object::new();
    if node.root {
        object.insert("root", Json::Bool(true));
        if let Some(name) = &node.name {
            object.insert("name", Json::str(name.clone()));
        }
        if node.requires_unlock {
            object.insert("requiresUnlock", Json::Bool(true));
        }
    } else if let Some(parent) = &node.parent {
        object.insert("parent", Json::str(parent.clone()));
    }
    if let Some(planet) = &node.planet {
        object.insert("planet", Json::str(planet.clone()));
    }
    if !node.requirements.is_empty() {
        let requirements: Vec<Json> =
            node.requirements.iter().map(|text| Json::str(text.clone())).collect();
        object.insert("requirements", Json::Arr(requirements));
    }
    if !node.objectives.is_empty() {
        // Java: objectives.add(objective.deepCopy())；owned 的 clone 就是深拷贝。
        let objectives: Vec<Json> =
            node.objectives.iter().map(|objective| Json::Obj(objective.clone())).collect();
        object.insert("objectives", Json::Arr(objectives));
    }
    Some(Json::Obj(object))
}

/// `ContentParser.java:1338` —— 补丁（覆盖原版内容的东西）不得碰科技树；
/// 游戏告警 "Modifying the tech tree is not allowed (@)" 并忽略该字段。
pub fn allows_research(patch: bool) -> bool {
    !patch
}

/// objective 的目标用哪个 JSON 键，直接取自 `Objectives` 里的类：
/// `Research`/`Produce` 持有 `content`，`SectorComplete`/`OnSector` 持有 `preset`，
/// `OnPlanet` 持有 `planet`。
pub fn objective_target_key(kind: Option<&str>) -> &'static str {
    match kind.unwrap_or("") {
        "SectorComplete" | "OnSector" => "preset",
        "OnPlanet" => "planet",
        _ => "content",
    }
}

/// 构造游戏能读出的最小 objective 对象：`{type: <type>, <key>: <target>}`。
pub fn objective(kind: Option<&str>, target: Option<&str>) -> Object {
    let kind = kind.filter(|k| OBJECTIVE_TYPES.contains(k)).unwrap_or(OBJECTIVE_TYPES[0]);
    let mut objective = Object::new();
    objective.insert("type", Json::str(kind));
    if let Some(target) = target.filter(|t| !t.trim().is_empty()) {
        objective.insert(objective_target_key(Some(kind)), Json::str(target.trim()));
    }
    objective
}

/// 父内容名；当这是 root 节点，或者值不可用时为 `None`。
pub fn parent(research: Option<&Json>) -> Option<String> {
    let research = research?;
    if research.is_null() {
        return None;
    }
    if is_primitive(research) {
        let parent = research.get_as_string();
        return if parent.trim().is_empty() { None } else { Some(parent) };
    }
    if let Some(object) = research.as_object() {
        if is_root_object(object) {
            return None;
        }
        if let Some(parent) = object.get("parent")
            && is_primitive(parent)
        {
            let name = parent.get_as_string();
            return if name.trim().is_empty() { None } else { Some(name) };
        }
    }
    None
}

/// `ContentParser.java:1394` —— `root:true` 的研究节点自成一棵科技树。
pub fn is_root(research: Option<&Json>) -> bool {
    match research {
        Some(value) => value.as_object().is_some_and(is_root_object),
        None => false,
    }
}

/// `isRoot(JsonObject)`：私有重载，Java 里靠签名区分，这里单独命名。
fn is_root_object(research: &Object) -> bool {
    match research.get("root") {
        Some(root) => is_primitive(root) && root.as_bool().unwrap_or(false),
        None => false,
    }
}

/// 该值的形状是否像游戏会接受的一个研究节点。
pub fn is_well_formed(research: Option<&Json>) -> bool {
    if !is_root(research) && parent(research).is_none() {
        // 没有 parent 的非 root 节点会被游戏丢弃并告警。
        return false;
    }
    true
}

// --- 辅助 -------------------------------------------------------------------------------------

/// Gson `JsonElement.isJsonPrimitive()`：`Null` 与数组/对象都不是原语。
fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Str(_) | Json::Num(_) | Json::Bool(_))
}

fn string(object: &Object, key: &str) -> Option<String> {
    match object.get(key) {
        Some(value) if is_primitive(value) => blank_to_null(Some(&value.get_as_string())),
        _ => None,
    }
}

fn bool(object: &Object, key: &str) -> bool {
    match object.get(key) {
        // isJsonPrimitive && isBoolean && getAsBoolean
        Some(value) => is_primitive(value) && value.as_bool().unwrap_or(false),
        None => false,
    }
}

/// Java 的 `String.isBlank()` 用 `Character.isWhitespace`，Rust 的 `trim` 用 Unicode
/// `White_Space`；对 ASCII 空白一致，`U+00A0` 这类只有 Java 认它为非空白。
fn blank_to_null(value: Option<&str>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty()).map(|v| v.to_string())
}

/// 把 `requirements` 读成游戏解析的 `"item/amount"` 字符串。
fn requirements(object: &Object) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(value) = object.get("requirements")
        && let Some(items) = value.as_array()
    {
        for element in items {
            if let Some(text) = requirement_text(Some(element)) {
                out.push(text);
            }
        }
    }
    out
}

/// 一条 requirement，两种被接受的形式都认：`"copper/10"` 与 `{item: copper, amount: 10}`。
fn requirement_text(element: Option<&Json>) -> Option<String> {
    let element = element?;
    if is_primitive(element) {
        let text = element.get_as_string();
        return if text.trim().is_empty() { None } else { Some(text) };
    }
    let stack = element.as_object()?;
    let item = stack.get("item").filter(|item| is_primitive(item))?;
    let count = match stack.get("amount").filter(|amount| is_primitive(amount)) {
        Some(amount) => amount.get_as_string(),
        None => "1".to_string(),
    };
    Some(format!("{}/{}", item.get_as_string(), count))
}

fn objectives(object: &Object) -> Vec<Object> {
    let mut out = Vec::new();
    if let Some(value) = object.get("objectives")
        && let Some(items) = value.as_array()
    {
        for element in items {
            if let Some(objective) = element.as_object() {
                out.push(objective.clone());
            }
        }
    }
    out
}
