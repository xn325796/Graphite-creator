//! `ReferenceOps.java` 的等价移植：在一条记录的字段树里写入与撤销跨内容引用——
//! 也就是在两张卡片之间拖出一条连线时的模型侧动作。
//!
//! 引用值写成裸的来源内容名：游戏会先在当前模组里解析裸名（ContentParser 的 find/locate
//! 系列 helper），而原版目标本来也不该加前缀。栈字段写成 vanilla JSON 使用的
//! `{"item":name,"amount":1}` 形状。
//!
//! Java 侧靠 Gson 的引用语义把「就地改活树」写得很隐式，Rust 是值语义，因此这里全部显式地
//! 取出→修改→塞回；凡是要用于 undo 的 `previous`，一律在进入修改之前先 `clone()` 成独立快照
//! （Java 里 stackArray 那种「往活数组尾部追加」会让 `previous` 与活树别名，Rust 的快照则
//! 是干净的前态——`revert` 的 `appended` 分支不读 `previous`，两种写法在这些路径上等价）。

use mm_json::{Json, Object};
use mm_model::ContentRecord;
use mm_schema::{FieldDef, FieldType};

/// 撤销一次 [`apply`] 所需的全部信息（Java 的 `record Revert`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revert {
    pub appended: bool,
    pub appended_index: i64,
    pub previous: Option<Json>,
}

/// 把引用写进目标记录的字段。
pub fn apply(target: &mut ContentRecord, field: &FieldDef, source_name: &str) -> Option<Revert> {
    apply_with_type(target, field, source_name, None)
}

/// 把一条引用写进目标的字段。`source_type_name` 是被引用内容的类型名（"item"、"liquid"……），
/// 已知时传入——结构化形状用它挑选组内的正确槽位（consume 组的 items 还是 liquids）；
/// `None` 回落到 item 槽位。
pub fn apply_with_type(
    target: &mut ContentRecord,
    field: &FieldDef,
    source_name: &str,
    source_type_name: Option<&str>,
) -> Option<Revert> {
    let name = field.name().to_string();
    if field.field_type() == FieldType::Struct {
        let previous = target.fields.get(&name).cloned();
        if !apply_struct(target, field, source_name, source_type_name) {
            return None;
        }
        return Some(Revert {
            appended: false,
            appended_index: -1,
            previous,
        });
    }
    if field.field_type() == FieldType::StackArray {
        let previous = target.fields.get(&name).cloned();
        let mut array = match previous.as_ref() {
            Some(Json::Arr(items)) => items.clone(),
            _ => Vec::new(),
        };
        let index = array.len() as i64;
        array.push(Json::Obj(stack_entry(field.stack_kind(), source_name)));
        target.fields.insert(name, Json::Arr(array));
        return Some(Revert {
            appended: true,
            appended_index: index,
            previous,
        });
    }
    let previous = target.fields.get(&name).cloned();
    if field.field_type() == FieldType::Stack {
        target
            .fields
            .insert(name, Json::Obj(stack_entry(field.stack_kind(), source_name)));
    } else {
        target.fields.insert(name, Json::str(source_name));
    }
    Some(Revert {
        appended: false,
        appended_index: -1,
        previous,
    })
}

/// 把一条引用写进结构化组字段。撤销恢复整棵前态树。
/// 当形状与来源类型无法组合时返回 false（什么都不写）——例如在任何工厂条目存在之前
/// 把一个 item 接到单位的 requirements 上。
fn apply_struct(
    target: &mut ContentRecord,
    field: &FieldDef,
    source_name: &str,
    source_type_name: Option<&str>,
) -> bool {
    let name = field.name().to_string();
    match shape(field) {
        "ammoMap" => {
            // 映射的键就是引用；空对象是一发合法的子弹——缺 "type" 时游戏默认
            // BasicBulletType（ContentParser.java:185）。
            let mut map = object_or_new(target.fields.get(&name));
            if !map.contains(source_name) {
                map.insert(source_name.to_string(), Json::object());
            }
            target.fields.insert(name, Json::Obj(map));
            true
        }
        "unitReq" | "plan" => {
            // {block/unit, requirements, time}——游戏对 UnitReq 默认 requirements={}、time=600
            // （ContentParser.UnitReq），所以接上一个工厂会创建一条成本列表为空的条目，
            // item 连线随后往里填。与条目键不匹配的连线（该放工厂的地方来了 item，或反之）
            // 没有可落地的槽位，被拒绝。
            let key = if shape(field) == "unitReq" { "block" } else { "unit" };
            let key_source = Some(key) == source_type_name;
            if key_source {
                let mut array = array_or_new(target.fields.get(&name));
                if has_named_entry(&array, key, source_name) {
                    return true;
                }
                let mut entry = Object::new();
                entry.insert(key.to_string(), Json::str(source_name));
                entry.insert("requirements".to_string(), Json::Arr(Vec::new()));
                array.push(Json::Obj(entry));
                target.fields.insert(name, Json::Arr(array));
                return true;
            }
            let mut placed = false;
            if let Some(current) = target.fields.get_mut(&name) {
                match current {
                    Json::Arr(items) => {
                        for element in items.iter_mut() {
                            if append_cost(element, source_name) {
                                placed = true;
                                break;
                            }
                        }
                    }
                    // 手写的单对象形态：Gson 里它就是 arrayOrNew 数组的唯一元素，
                    // 成本同样落在这个仍挂在树上的对象里。
                    other => placed = append_cost(other, source_name),
                }
            }
            placed
        }
        "consume" => {
            // 每种 item/liquid 组合一个 consume 组：接 item 追加到第一个组的 "items"，
            // 接 liquid 追加到它的 "liquids"，需要时创建组。
            let liquid = source_type_name == Some("liquid");
            let slot = if liquid { "liquids" } else { "items" };
            if matches!(target.fields.get(&name), Some(Json::Obj(_))) {
                // 单对象组形态：就地修改，保持对象形状。
                if let Some(group) = target
                    .fields
                    .get_mut(&name)
                    .and_then(|value| value.as_object_mut())
                {
                    fill_group_slot(group, slot, liquid, source_name);
                }
                return true;
            }
            let mut array = array_or_new(target.fields.get(&name));
            let index = first_group_with_slot(&mut array, slot);
            if let Some(Json::Obj(group)) = array.get_mut(index) {
                fill_group_slot(group, slot, liquid, source_name);
            }
            target.fields.insert(name, Json::Arr(array));
            true
        }
        _ => {
            // 未知形状：回落到栈数组行为，那至少是个合法的结构化列表。
            let mut array = array_or_new(target.fields.get(&name));
            array.push(Json::Obj(stack_entry(None, source_name)));
            target.fields.insert(name, Json::Arr(array));
            true
        }
    }
}

fn shape(field: &FieldDef) -> &str {
    // Java：kind == null || kind.isEmpty() ? "" : kind
    field.stack_kind().unwrap_or("")
}

fn object_or_new(current: Option<&Json>) -> Object {
    match current {
        Some(Json::Obj(object)) => object.clone(),
        _ => Object::new(),
    }
}

fn array_or_new(current: Option<&Json>) -> Vec<Json> {
    match current {
        Some(Json::Arr(items)) => items.clone(),
        // 手写的单个对象成为数组的第一个元素（游戏两种写法都能读）。
        Some(Json::Obj(object)) => vec![Json::Obj(object.clone())],
        _ => Vec::new(),
    }
}

fn has_named_entry(array: &[Json], key: &str, source_name: &str) -> bool {
    array.iter().any(|element| {
        element
            .as_object()
            .and_then(|object| object.get(key))
            .map(|named| {
                is_primitive(named) && named.get_as_string() == source_name
            })
            .unwrap_or(false)
    })
}

/// 应该接收给定槽位（items 或 liquids）的那个 consume 组（对象）：
/// 已带该槽位的组优先，否则第一个组获得它，空数组则新建一个组。
fn first_group_with_slot(array: &mut Vec<Json>, slot: &str) -> usize {
    let mut first: Option<usize> = None;
    for (index, element) in array.iter().enumerate() {
        let Json::Obj(group) = element else { continue };
        if group.contains(slot) {
            return index;
        }
        if first.is_none() {
            first = Some(index);
        }
    }
    if let Some(index) = first {
        return index;
    }
    array.push(Json::object());
    array.len() - 1
}

fn fill_group_slot(group: &mut Object, slot: &str, liquid: bool, source_name: &str) {
    let mut stacks = match group.get(slot) {
        Some(Json::Arr(items)) => items.clone(),
        _ => Vec::new(),
    };
    let kind = if liquid { "liquid" } else { "item" };
    stacks.push(Json::Obj(stack_entry(Some(kind), source_name)));
    group.insert(slot.to_string(), Json::Arr(stacks));
}

/// 把一条 item 成本追加进带 "requirements" 的条目对象（对应 Java 里
/// `getAsJsonArray("requirements")` 后 `add`）；不是那样的条目返回 false。
fn append_cost(element: &mut Json, source_name: &str) -> bool {
    let Some(group) = element.as_object_mut() else {
        return false;
    };
    if !group.contains("requirements") {
        return false;
    }
    let costs = group
        .get_mut("requirements")
        .and_then(|value| value.as_array_mut())
        // Gson 的 getAsJsonArray(String)：成员存在却不是数组时抛 IllegalStateException。
        .expect("ReferenceOps: requirements 成员不是数组");
    costs.push(Json::Obj(stack_entry(Some("item"), source_name)));
    true
}

pub fn revert(target: &mut ContentRecord, field: &FieldDef, revert_info: &Revert) {
    let name = field.name().to_string();
    if revert_info.appended {
        let mut emptied = false;
        if let Some(Json::Arr(array)) = target.fields.get_mut(&name) {
            if revert_info.appended_index < array.len() as i64 {
                array.remove(revert_info.appended_index as usize);
            }
            emptied = array.is_empty();
        }
        if emptied {
            target.fields.remove(&name);
        }
        return;
    }
    match revert_info.previous.clone() {
        None => {
            target.fields.remove(&name);
        }
        Some(previous) => {
            target.fields.insert(name, previous);
        }
    }
}

/// 没有撤销信息时的尽力移除（例如从磁盘载入的一条边）。
pub fn remove(target: &mut ContentRecord, field: &FieldDef, source_name: &str) -> bool {
    let name = field.name().to_string();
    let Some(current) = target.fields.get(&name).cloned() else {
        return false;
    };
    if field.field_type() == FieldType::Struct {
        return remove_struct(target, field, &current, source_name);
    }
    let stack_kind = field.stack_kind();
    if let Json::Arr(items) = &current {
        let index = (0..items.len()).find(|&i| matches(&items[i], stack_kind, source_name));
        let Some(index) = index else {
            return false;
        };
        let mut emptied = false;
        if let Some(Json::Arr(array)) = target.fields.get_mut(&name) {
            array.remove(index);
            emptied = array.is_empty();
        }
        if emptied {
            target.fields.remove(&name);
        }
        return true;
    }
    if matches(&current, stack_kind, source_name) {
        target.fields.remove(&name);
        return true;
    }
    false
}

fn remove_struct(
    target: &mut ContentRecord,
    field: &FieldDef,
    current: &Json,
    source_name: &str,
) -> bool {
    let name = field.name().to_string();
    match shape(field) {
        "ammoMap" => {
            if !current.is_object() {
                return false;
            }
            let mut removed = false;
            let mut emptied = false;
            if let Some(Json::Obj(map)) = target.fields.get_mut(&name) {
                removed = map.remove(source_name).is_some();
                emptied = map.is_empty();
            }
            if removed && emptied {
                target.fields.remove(&name);
            }
            removed
        }
        "unitReq" | "plan" => {
            let key = if shape(field) == "unitReq" { "block" } else { "unit" };
            if is_primitive(current) {
                return false;
            }
            if let Some(object) = current.as_object() {
                // 这一支没有 isJsonPrimitive 守卫：成员是数组/对象时 Gson 的 getAsString 抛错。
                let named = object.get(key);
                if let Some(named) = named
                    && named.get_as_string() == source_name
                {
                    target.fields.remove(&name);
                    return true;
                }
                return false;
            }
            let mut removed = false;
            let emptied = match target.fields.get_mut(&name) {
                Some(Json::Arr(array)) => {
                    let mut index = array.len();
                    while index > 0 {
                        index -= 1;
                        let hit = array[index]
                            .as_object()
                            .and_then(|object| object.get(key))
                            .map(|named| named.get_as_string() == source_name)
                            .unwrap_or(false);
                        if hit {
                            array.remove(index);
                            removed = true;
                        }
                    }
                    array.is_empty()
                }
                // Java 在此 getAsJsonArray()，非数组抛 IllegalStateException。
                Some(other) => panic!("ReferenceOps: {name} 不是数组（{other}）"),
                None => return false,
            };
            if removed && emptied {
                target.fields.remove(&name);
            }
            removed
        }
        "consume" => {
            let stripped = strip_stack(current, source_name, "item");
            match stripped {
                None => {
                    target.fields.remove(&name);
                    true
                }
                Some(stripped) => {
                    if stripped != *current {
                        target.fields.insert(name, stripped);
                        return true;
                    }
                    false
                }
            }
        }
        _ => false,
    }
}

/// 深剥离 consume 结构里所有指向 `sourceName` 的栈条目。空组或空数组被丢弃（`None`）；
/// 被清空的组保留它的其它键，例如 `power`。`kind` 是该值所在槽位隐含的栈键。
fn strip_stack(node: &Json, source_name: &str, kind: &str) -> Option<Json> {
    if let Json::Arr(items) = node {
        let mut out: Vec<Json> = Vec::new();
        for element in items {
            if let Some(cleaned) = strip_stack(element, source_name, kind) {
                out.push(cleaned);
            }
        }
        return if out.is_empty() { None } else { Some(Json::Arr(out)) };
    }
    if let Json::Obj(object) = node {
        if is_stack_object(object, source_name) {
            return None;
        }
        let mut out = Object::new();
        for (key, value) in object.iter() {
            let slot_kind = if key == "liquids" { "liquid" } else { kind };
            if let Some(cleaned) = strip_stack(value, source_name, slot_kind) {
                out.insert(key.clone(), cleaned);
            }
        }
        return if out.is_empty() { None } else { Some(Json::Obj(out)) };
    }
    if node.is_string() && stack_name(&node.get_as_string()) == source_name {
        return None;
    }
    Some(node.clone())
}

/// "name/amount" 栈写法的内容名，或文本本身。
fn stack_name(text: &str) -> &str {
    match text.find('/') {
        Some(slash) if slash > 0 => &text[..slash],
        _ => text,
    }
}

fn is_stack_object(object: &Object, source_name: &str) -> bool {
    ["item", "liquid", "payload"].iter().any(|key| {
        object
            .get(key)
            .map(|named| is_primitive(named) && named.get_as_string() == source_name)
            .unwrap_or(false)
    })
}

fn stack_entry(stack_kind: Option<&str>, source_name: &str) -> Object {
    let mut entry = Object::new();
    let key = match stack_kind {
        Some(kind) if !kind.is_empty() => kind,
        _ => "item",
    };
    entry.insert(key.to_string(), Json::str(source_name));
    entry.insert("amount".to_string(), Json::num("1"));
    entry
}

/// 指向 `sourceName` 的那条条目所带的数量，没有则 -1。
pub fn amount_of(target: &ContentRecord, field: &FieldDef, source_name: &str) -> i64 {
    let current = target.fields.get(field.name());
    let entry = if field.field_type() == FieldType::Struct {
        find_stack_entry(current, source_name)
    } else {
        entry_for(current, field.stack_kind(), source_name)
    };
    let Some(entry) = entry else {
        return -1;
    };
    if let Json::Str(text) = entry {
        // "copper/10" 这种写法把数量带在字符串自身里。
        if let Some(slash) = text.find('/')
            && slash > 0
        {
            // Java：Integer.parseInt 失败 → 1。
            return text[slash + 1..].trim().parse::<i64>().unwrap_or(1);
        }
        return 1;
    }
    if let Json::Obj(object) = entry
        && let Some(amount) = object.get("amount")
        && amount.is_number()
        && let Some(value) = amount.as_i64()
    {
        return value;
    }
    1
}

/// 设置指向 `sourceName` 的栈条目的数量。裸名条目被升级为显式的
/// `{"item":name,"amount":n}` 对象。返回撤销信息；字段里没有匹配条目时返回 None。
pub fn set_amount(
    target: &mut ContentRecord,
    field: &FieldDef,
    source_name: &str,
    amount: i64,
) -> Option<Revert> {
    let name = field.name().to_string();
    let current = target.fields.get(&name).cloned()?;
    let snapshot = current.clone();
    if field.field_type() == FieldType::Struct {
        if current.is_string() && stack_name(&current.get_as_string()) == source_name {
            // 裸的 "copper/10" 值变成显式对象形式。
            let mut entry = stack_entry(Some("item"), source_name);
            entry.insert("amount".to_string(), Json::num(amount.to_string()));
            target.fields.insert(name, Json::Obj(entry));
            return Some(Revert {
                appended: false,
                appended_index: -1,
                previous: Some(snapshot),
            });
        }
        let changed = match target.fields.get_mut(&name) {
            Some(node) => set_amount_deep(node, source_name, amount, "item"),
            None => false,
        };
        return if changed {
            Some(Revert {
                appended: false,
                appended_index: -1,
                previous: Some(snapshot),
            })
        } else {
            None
        };
    }
    if let Json::Arr(items) = &current {
        let index = (0..items.len()).find(|&i| matches(&items[i], field.stack_kind(), source_name));
        let index = index?;
        let updated = with_amount(&items[index], field.stack_kind(), source_name, amount);
        if let Some(Json::Arr(array)) = target.fields.get_mut(&name) {
            array[index] = updated;
        }
        return Some(Revert {
            appended: false,
            appended_index: -1,
            previous: Some(snapshot),
        });
    }
    if matches(&current, field.stack_kind(), source_name) {
        target.fields.insert(
            name,
            with_amount(&current, field.stack_kind(), source_name, amount),
        );
        return Some(Revert {
            appended: false,
            appended_index: -1,
            previous: Some(snapshot),
        });
    }
    None
}

/// 在结构化字段里深搜指向 `sourceName` 的栈条目。
fn find_stack_entry<'a>(node: Option<&'a Json>, source_name: &str) -> Option<&'a Json> {
    let json = node?;
    if json.is_null() {
        return None;
    }
    if is_primitive(json) {
        return if json.is_string() && stack_name(&json.get_as_string()) == source_name {
            Some(json)
        } else {
            None
        };
    }
    if let Json::Arr(items) = json {
        for element in items {
            if let Some(found) = find_stack_entry(Some(element), source_name) {
                return Some(found);
            }
        }
        return None;
    }
    let Json::Obj(object) = json else {
        return None;
    };
    if is_stack_object(object, source_name) {
        return Some(json);
    }
    for (_key, value) in object.iter() {
        if let Some(found) = find_stack_entry(Some(value), source_name) {
            return Some(found);
        }
    }
    None
}

/// 在结构化字段内部就地更新数量。裸的 "name/amount" 条目被升级为显式对象；
/// `kind` 跟随值所在槽位（"items" → item，"liquids" → liquid）。
fn set_amount_deep(node: &mut Json, source_name: &str, amount: i64, kind: &str) -> bool {
    if node.is_null() || is_primitive(node) {
        return false;
    }
    if let Json::Arr(items) = node {
        for item in items.iter_mut() {
            let hit = matches!(
                item,
                Json::Str(text) if stack_name(text) == source_name
            );
            if hit {
                let mut entry = stack_entry(Some(kind), source_name);
                entry.insert("amount".to_string(), Json::num(amount.to_string()));
                *item = Json::Obj(entry);
                return true;
            }
            if set_amount_deep(item, source_name, amount, kind) {
                return true;
            }
        }
        return false;
    }
    let Json::Obj(object) = node else {
        return false;
    };
    if is_stack_object(object, source_name) {
        object.insert("amount".to_string(), Json::num(amount.to_string()));
        return true;
    }
    let keys: Vec<String> = object.keys().cloned().collect();
    for key in keys {
        let slot_kind = if key == "liquids" {
            "liquid"
        } else if key == "items" {
            "item"
        } else {
            kind
        };
        let Some(value) = object.get_mut(&key) else {
            continue;
        };
        if set_amount_deep(value, source_name, amount, slot_kind) {
            return true;
        }
    }
    false
}

fn entry_for<'a>(
    current: Option<&'a Json>,
    stack_kind: Option<&str>,
    source_name: &str,
) -> Option<&'a Json> {
    let current = current?;
    if let Json::Arr(items) = current {
        return items
            .iter()
            .find(|element| matches(element, stack_kind, source_name));
    }
    if matches(current, stack_kind, source_name) {
        Some(current)
    } else {
        None
    }
}

fn with_amount(
    entry: &Json,
    stack_kind: Option<&str>,
    source_name: &str,
    amount: i64,
) -> Json {
    let mut object = match entry {
        Json::Obj(object) => object.clone(),
        _ => stack_entry(stack_kind, source_name),
    };
    object.insert("amount".to_string(), Json::num(amount.to_string()));
    Json::Obj(object)
}

fn matches(element: &Json, stack_kind: Option<&str>, source_name: &str) -> bool {
    if is_primitive(element) {
        return element.get_as_string() == source_name;
    }
    let Some(object) = element.as_object() else {
        return false;
    };
    let key = match stack_kind {
        Some(kind) if !kind.is_empty() => kind,
        _ => "item",
    };
    object
        .get(key)
        .map(|named| is_primitive(named) && named.get_as_string() == source_name)
        .unwrap_or(false)
}

/// Gson 的 `JsonElement.isJsonPrimitive()`。
fn is_primitive(json: &Json) -> bool {
    matches!(json, Json::Str(_) | Json::Num(_) | Json::Bool(_))
}
