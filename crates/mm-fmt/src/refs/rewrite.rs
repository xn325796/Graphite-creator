//! `ReferenceRewrite.java` 的等价移植：内容记录被改名或删除时，保持引用图同步。
//!
//! 内容文件就是普通 JSON，所以一条引用只是一个字符串。改名后如果没人管，所有
//! `requirements`、`research`、`ammoTypes` 里的值都会指向一个不再存在的名字，悄无声息地把工程
//! 弄坏。这个类在一处把这些值改写过来，并精确报告改了哪些记录，调用方就能把整次改写做成一步撤销。
//!
//! 名字匹配跟着游戏里的两条查表路径走。带前缀的写法 `<mod>-<name>` 没有歧义，永远改写。裸名
//! 只有在它确实指本模组内容时才改写——对 [`vanilla_first`] 列出的那几个「原版优先」字段
//! （status 引用、`planet`、`research`）来说，裸名若同时也是原版内容的名字，就继续绑在原版上、
//! 原样保留。其它位置游戏先解析本模组内容，所以目标的裸名就是那个目标。
//!
//! 借用与可观察行为：Java 里 `ContentRecord` 是引用类型，可以在遍历 `project.contents` 的同时
//! 就地改 JSON 树。Rust 里改写全部通过 `&mut ContentRecord` 传下去（一次只碰一条记录），
//! `Match` 只借用 `SchemaRegistry`、`Target` 只存字符串快照，因此与 `&mut project` 不冲突。
//! 遍历按 `contents` 的下标升序进行（等价于 Java 的 `List.copyOf` 快照遍历，长度在本轮里固定），
//! 每条记录内部字段的处理顺序、`changes` 的先后顺序、`meta.content_order` 的最终顺序都与 Java 一致。

use mm_json::{Json, Object};
use mm_model::{ContentRecord, ModProject};
use mm_schema::{name_rules, ContentType, FieldDef, FieldType, SchemaRegistry};

const RESEARCH: &str = "research";
const PARENT: &str = "parent";
const OBJECTIVES: &str = "objectives";
const REQUIREMENTS: &str = "requirements";
const TYPE: &str = "type";
const CONTENT: &str = "content";
const PRESET: &str = "preset";
const PLANET: &str = "planet";

/// 游戏在「栈样」对象里当作内容引用读取的那些键。
const CONTENT_KEYS: [&str; 6] = ["item", "liquid", "payload", "block", "unit", "content"];

/// 精选 schema 把它们建模成原始 JSON、但游戏仍按引用读取的字段。放在这里（而不是改 schema 数据）
/// 可以避免动已发布的 schema 文件。
const EXTRA_REFERENCE_FIELDS: [&str; 6] =
    ["requirements", "ammoTypes", "consumes", "plans", "upgrades", "ammo"];

/// 一条字段树被改过的记录，含改动前后的完整树。
#[derive(Debug, Clone)]
pub struct Change {
    /// Java 里这里持有的是那条活记录；Rust 无法在改写其余记录的同时别名持有它，
    /// 因此存改动后的快照（与 `after` 同源）。`before`/`after` 都是独立深拷贝。
    pub record: ContentRecord,
    pub before: Object,
    pub after: Object,
}

/// 一次改写触及的记录，加上改动前后的 `meta.contentOrder` 两份列表。
/// 撤销恢复 "before" 一侧，重做应用 "after" 一侧。
///
/// （Java 的 `record Result`；这里改名以免遮蔽 `std::result::Result`。）
#[derive(Debug, Clone)]
pub struct RewriteResult {
    pub changes: Vec<Change>,
    pub order_before: Vec<String>,
    pub order_after: Vec<String>,
}

impl RewriteResult {
    pub fn changed(&self) -> bool {
        !self.changes.is_empty() || self.order_before != self.order_after
    }
}

/// 把指向 `target` 的所有引用从 `old_name` 改写成 `new_name`，返回被改的记录。
/// patch 记录永不改名：它的名字就是它覆盖的原版名，调用方必须放过它。
///
/// Java 的 `project/target/oldName/newName` 判空在 Rust 由类型系统排除（不可能为 null），
/// 其余守卫（`target.patch`、`oldName.equals(newName)`）逐条对应。
pub fn rename(
    project: &mut ModProject,
    registry: &SchemaRegistry,
    target: &ContentRecord,
    old_name: &str,
    new_name: &str,
) -> RewriteResult {
    if target.patch || old_name == new_name {
        return unchanged(project);
    }
    let m = Match::new(registry, vec![Target::new(project, target, old_name, Some(new_name))]);
    rewrite(project, registry, &m)
}

/// 删除一批记录时，一次性移除对它们的所有引用。（Java 的 `remove(project, registry, List)`。）
pub fn remove(
    project: &mut ModProject,
    registry: &SchemaRegistry,
    targets: &[&ContentRecord],
) -> RewriteResult {
    if targets.is_empty() {
        return unchanged(project);
    }
    let mut refs: Vec<Target> = Vec::new();
    for target in targets {
        // patch 注册在它覆盖的那个原版名下；删掉 patch 等于把原版内容还回来，
        // 所以指向那个名字的引用仍然指原版，必须留着。
        if !target.patch {
            refs.push(Target::new(project, target, &target.name, None));
        }
    }
    if refs.is_empty() {
        return unchanged(project);
    }
    rewrite(project, registry, &Match::new(registry, refs))
}

/// 单条记录的便捷重载（Java 的 `remove(project, registry, ContentRecord)`）。
pub fn remove_record(
    project: &mut ModProject,
    registry: &SchemaRegistry,
    target: &ContentRecord,
) -> RewriteResult {
    remove(project, registry, &[target])
}

fn unchanged(project: &ModProject) -> RewriteResult {
    let order = project.meta.content_order.clone();
    RewriteResult {
        changes: Vec::new(),
        order_before: order.clone(),
        order_after: order,
    }
}

fn rewrite(project: &mut ModProject, registry: &SchemaRegistry, m: &Match) -> RewriteResult {
    let order_before = project.meta.content_order.clone();
    let mut changes: Vec<Change> = Vec::new();
    // 先定住长度再遍历（Java 的 List.copyOf）：同一轮里不会看到被改写到一半的集合，
    // 也不会顺手改写到本轮之后才加进来的记录。
    let total = project.contents.len();
    for record in project.contents.iter_mut().take(total) {
        let before = record.tree();
        rewrite_record(record, registry, m);
        let after = record.tree();
        if before != after {
            record.dirty = true;
            changes.push(Change {
                record: record.clone(),
                before,
                after,
            });
        }
    }
    if rewrite_content_order(project, m) {
        project.meta_dirty = true;
    }
    RewriteResult {
        changes,
        order_before,
        order_after: project.meta.content_order.clone(),
    }
}

fn rewrite_record(record: &mut ContentRecord, registry: &SchemaRegistry, m: &Match) {
    let declared = record.class_name();
    let class_name = registry.resolve_class(record.ctype, declared.as_deref());
    // Java 用 LinkedHashSet；这里只做成员判断，插入顺序不影响行为。
    let mut handled: Vec<String> = Vec::new();
    if let Some(class_name) = class_name {
        for field in registry.fields_of(&class_name).iter() {
            if !field.is_reference() {
                continue;
            }
            handled.push(field.name().to_string());
            if RESEARCH == field.name() {
                // 游戏解析器里 research 也不是类字段；它按自己的形状
                // （parent/objectives/planet）在下面单独处理。
                continue;
            }
            if field.field_type() == FieldType::Struct && Some("ammoMap") == field.stack_kind() {
                // 引用是这张映射的键（KEY），不是值。
                rewrite_ammo_map(record, field.name(), m, key_type_of(field));
                continue;
            }
            rewrite_field(record, field.name(), m, Some(field), hint_of(field));
        }
    }
    for extra in EXTRA_REFERENCE_FIELDS {
        // 单位工厂的 "requirements" 建模成原始 JSON，而建筑的 "requirements" 是栈数组；
        // 只有没被建模的那种写法才需要这趟补漏。
        if !handled.iter().any(|name| name == extra) && record.fields.contains(extra) {
            if "ammoTypes" == extra {
                // ammoTypes 是 ObjectMap<Item, BulletType>：映射的键才是物品名，
                // 不只值是引用。只改值会让每座炮塔在改名或删除后仍指向旧物品。
                rewrite_ammo_map(record, extra, m, ContentType::Item);
            } else if "ammo" == extra {
                // LiquidTurret.ammo：同样的映射形状，键是液体。
                rewrite_ammo_map(record, extra, m, ContentType::Liquid);
            } else if REQUIREMENTS == extra {
                // 单位工厂 requirements：block 加 ItemStacks。
                rewrite_field(record, extra, m, None, Some(ContentType::Item));
            } else if "plans" == extra {
                // {unit, requirements, time} 计划：unit 键是引用。
                rewrite_field(record, extra, m, None, Some(ContentType::Unit));
            } else if "upgrades" == extra {
                // 重装塔的升级是 [[fromUnit, toUnit], ...] 的裸名对。
                rewrite_upgrades(record, m);
            } else {
                rewrite_field(record, extra, m, None, None);
            }
        }
    }
    rewrite_research(record, m);
}

/// 结构化映射字段的键所指向的内容类型：炮塔是物品，其余情况同理。
fn key_type_of(field: &FieldDef) -> ContentType {
    // Java：ContentType.valueOf(ctype)，未知常量名回落 item。
    if let Some(ctype) = field.content_ctype()
        && !ctype.is_empty()
    {
        return ContentType::from_name(ctype).unwrap_or(ContentType::Item);
    }
    ContentType::Item
}

/// 改写 Reconstructor 的 `upgrades` 单位对：[[from, to], ...]。
fn rewrite_upgrades(record: &mut ContentRecord, m: &Match) {
    let current = match record.fields.get("upgrades") {
        Some(value @ Json::Arr(_)) => value.clone(),
        _ => return,
    };
    let mut out: Vec<Json> = Vec::new();
    let Json::Arr(pairs) = &current else {
        return;
    };
    for pair in pairs {
        let Json::Arr(units) = pair else {
            out.push(pair.clone());
            continue;
        };
        let mut rewritten: Vec<Json> = Vec::new();
        let mut removed = false;
        for unit in units {
            // Java：unit.isJsonPrimitive() && isString()
            if let Json::Str(text) = unit {
                let target = m.match_unit(text);
                if target.is_some() && m.removing {
                    removed = true;
                    break;
                }
                match target {
                    Some(target) => {
                        rewritten.push(Json::str(target.replacement(text).unwrap_or_default()));
                    }
                    None => rewritten.push(unit.clone()),
                }
            } else {
                rewritten.push(unit.clone());
            }
        }
        if !removed && !rewritten.is_empty() {
            out.push(Json::Arr(rewritten));
        }
    }
    let out_empty = out.is_empty();
    let value = Json::Arr(out);
    let before = record.fields.get("upgrades");
    if out_empty && m.removing {
        record.fields.remove("upgrades");
    } else if Some(&value) != before {
        record.fields.insert("upgrades", value);
    }
}

fn rewrite_field(
    record: &mut ContentRecord,
    field: &str,
    m: &Match,
    definition: Option<&FieldDef>,
    hint: Option<ContentType>,
) {
    let Some(current) = record.fields.get(field) else {
        return;
    };
    if current.is_null() {
        return;
    }
    let current = current.clone();
    let rewritten = rewrite_value(current.clone(), m, definition, hint);
    let drop = rewritten.is_none()
        || (m.removing
            && rewritten
                .as_ref()
                .map(|value| value.is_array() && is_empty_array(value))
                .unwrap_or(false));
    if drop {
        record.fields.remove(field);
        return;
    }
    let rewritten = rewritten.unwrap_or(Json::Null);
    if rewritten != current {
        record.fields.insert(field, rewritten);
    }
}

/// 改写弹药映射的物品/液体键（以及嵌套值）。
fn rewrite_ammo_map(record: &mut ContentRecord, field_name: &str, m: &Match, key_type: ContentType) {
    let Some(current) = record.fields.get(field_name) else {
        return;
    };
    if current.is_null() {
        return;
    }
    let current = current.clone();
    let rewritten = rewrite_ammo_map_value(current.clone(), m, key_type);
    if rewritten.is_none() {
        record.fields.remove(field_name);
        return;
    }
    let rewritten = rewritten.unwrap_or(Json::Null);
    if rewritten != current {
        record.fields.insert(field_name, rewritten);
    }
}

fn rewrite_ammo_map_value(value: Json, m: &Match, key_type: ContentType) -> Option<Json> {
    // 老数据/手写数据有时用 {item, type} 对象数组。这种形状继续走通用遍历，
    // 但真正的映射形态里，键才是引用。
    let Json::Obj(object) = &value else {
        let rewritten = rewrite_value(value, m, None, None);
        return if m.removing
            && rewritten
                .as_ref()
                .map(|value| value.is_array() && is_empty_array(value))
                .unwrap_or(false)
        {
            None
        } else {
            rewritten
        };
    };
    let mut out = Object::new();
    for (entry_key, entry_value) in object.iter() {
        let target = m.match_ammo_key(entry_key, key_type);
        let mut key = entry_key.clone();
        if let Some(target) = target {
            if m.removing {
                continue;
            }
            key = target.replacement(&key).unwrap_or_default();
        }
        let nested = rewrite_value(entry_value.clone(), m, None, None);
        if let Some(nested) = nested {
            out.insert(key, nested);
        }
    }
    if out.is_empty() && m.removing {
        None
    } else {
        Some(Json::Obj(out))
    }
}

/// 改写一个字段的值（在副本上）。返回 `None` 表示一次删除把整个值消费掉了。
/// 数组保持顺序；对象把每个未知键原样留着。`hint` 是外层槽位期望的内容类型，
/// 让没进 schema 的字段也保持类型安全。
fn rewrite_value(
    value: Json,
    m: &Match,
    field: Option<&FieldDef>,
    hint: Option<ContentType>,
) -> Option<Json> {
    if value.is_null() {
        return Some(value);
    }
    if is_primitive(&value) {
        return rewrite_primitive(value, m, field, hint);
    }
    if let Json::Arr(items) = &value {
        let mut out: Vec<Json> = Vec::new();
        for element in items {
            if m.removing && references_target(element, m, field, hint) {
                continue;
            }
            if let Some(rewritten) = rewrite_value(element.clone(), m, field, hint) {
                out.push(rewritten);
            }
        }
        return Some(Json::Arr(out));
    }
    if let Json::Obj(object) = &value {
        let mut out = Object::new();
        for (entry_key, entry_value) in object.iter() {
            let child_hint = hint_for_key(entry_key, hint);
            if CONTENT_KEYS.contains(&entry_key.as_str()) {
                if let Some(rewritten) = rewrite_value(entry_value.clone(), m, field, child_hint) {
                    out.insert(entry_key.clone(), rewritten);
                }
                continue;
            }
            // 结构化键（比如 consumes.items）挂的是栈数组。容器要递归进去，
            // 但未知键下的任意字符串绝不当成引用："type": "BasicBulletType" 必须原样活下来。
            if entry_value.is_array() || entry_value.is_object() {
                if let Some(rewritten) = rewrite_value(entry_value.clone(), m, field, child_hint) {
                    out.insert(entry_key.clone(), rewritten);
                }
            } else {
                out.insert(entry_key.clone(), entry_value.clone());
            }
        }
        return if out.is_empty() && m.removing {
            None
        } else {
            Some(Json::Obj(out))
        };
    }
    Some(value)
}

/// 字段值指向的内容类型，用来让 raw/补漏字段保持类型安全。
fn hint_of(field: &FieldDef) -> Option<ContentType> {
    let ctype = field.content_ctype();
    if let Some(ctype) = ctype
        && !ctype.is_empty() && ctype != "any"
        && let Some(found) = ContentType::all().into_iter().find(|t| t.name() == ctype)
    {
        return Some(found);
    }
    let stack_kind = field.stack_kind();
    if let Some(stack_kind) = stack_kind
        && !stack_kind.is_empty() && stack_kind != "any"
        && let Some(found) = ContentType::all().into_iter().find(|t| t.name() == stack_kind)
    {
        return Some(found);
    }
    None
}

/// 一个结构化键隐含的类型。`consumes`、单位工厂 `requirements` 这类 raw 字段不在 schema 里，
/// 键名是唯一可用的类型证据；没有它，一次 status 改名可能把同名的物品栈也改掉。
fn hint_for_key(key: &str, inherited: Option<ContentType>) -> Option<ContentType> {
    match key {
        "item" | "items" | "requirements" => Some(ContentType::Item),
        "liquid" | "liquids" => Some(ContentType::Liquid),
        "block" => Some(ContentType::Block),
        "unit" => Some(ContentType::Unit),
        _ => inherited,
    }
}

fn rewrite_primitive(
    value: Json,
    m: &Match,
    field: Option<&FieldDef>,
    hint: Option<ContentType>,
) -> Option<Json> {
    // Java：!value.isString() → 原样返回该 primitive。
    let Json::Str(text) = &value else {
        return Some(value);
    };
    // 栈值用斜杠带数量（"copper/10"）；数量部分不是名字。
    let (head, suffix) = split_stack(text);
    let Some(target) = m.match_field(head, field, hint) else {
        return Some(value);
    };
    if m.removing {
        return None;
    }
    Some(Json::str(target.replacement(head).unwrap_or_default() + suffix))
}

/// 栈样的对象是否在某个具名键下带着指向某个目标的引用。
fn references_target(
    element: &Json,
    m: &Match,
    field: Option<&FieldDef>,
    hint: Option<ContentType>,
) -> bool {
    // Java：element.isJsonObject() && referencesTarget(element.getAsJsonObject(), ...)
    let Json::Obj(object) = element else {
        return false;
    };
    for key in CONTENT_KEYS {
        // Java：named.isJsonPrimitive() && isString()
        if let Some(Json::Str(text)) = object.get(key) {
            let (head, _) = split_stack(text);
            if m.match_field(head, field, hint).is_some() {
                return true;
            }
        }
    }
    false
}

fn rewrite_research(record: &mut ContentRecord, m: &Match) {
    let Some(research) = record.fields.get(RESEARCH) else {
        return;
    };
    if research.is_null() {
        return;
    }
    let research = research.clone();
    let rewritten = rewrite_research_value(research.clone(), m);
    if m.removing
        && (rewritten.is_none() || is_empty_research(rewritten.as_ref().unwrap_or(&Json::Null)))
    {
        record.fields.remove(RESEARCH);
        return;
    }
    match rewritten {
        None => {
            record.fields.remove(RESEARCH);
        }
        Some(rewritten) => {
            if rewritten != research {
                record.fields.insert(RESEARCH, rewritten);
            }
        }
    }
}

/// 被剥掉全部有意义内容的 research 对象整体丢弃。
fn is_empty_research(value: &Json) -> bool {
    if let Some(object) = value.as_object() {
        return object.is_empty();
    }
    is_primitive(value) && value.get_as_string().trim().is_empty()
}

fn rewrite_research_value(value: Json, m: &Match) -> Option<Json> {
    if is_primitive(&value) {
        // 裸的 parent 名在科技树里原版优先，所以只有它不同时也是原版内容名时才动。
        return rewrite_research_primitive(value, m, "any", None);
    }
    let Json::Obj(object) = &value else {
        return Some(value);
    };
    let mut out = Object::new();
    for (key, child) in object.iter() {
        if key == PARENT {
            // 复合的 parent 是我们看不懂的数据，原样留着。
            if !is_string(child) {
                out.insert(PARENT, child.clone());
                continue;
            }
            if let Some(rewritten) = rewrite_research_primitive(child.clone(), m, "any", None) {
                out.insert(PARENT, rewritten);
            }
            continue;
        }
        if key == OBJECTIVES {
            let rewritten = rewrite_objectives(child, m);
            if !rewritten.is_empty() {
                out.insert(OBJECTIVES, Json::Arr(rewritten));
            }
            continue;
        }
        if key == REQUIREMENTS {
            // 研究的 requirements 是 ItemStacks（"copper/10" 或 {item, amount}），跟建筑的建造
            // 成本一样；类字段循环走不到它，因为 research 在类解析器看到之前就被摘走了。
            let rewritten = rewrite_value(child.clone(), m, None, Some(ContentType::Item));
            let keep = match &rewritten {
                Some(value) => !(m.removing && value.is_array() && is_empty_array(value)),
                None => false,
            };
            if keep
                && let Some(rewritten) = rewritten
            {
                out.insert(REQUIREMENTS, rewritten);
            }
            continue;
        }
        if key == PLANET {
            if !is_string(child) {
                out.insert(PLANET, child.clone());
                continue;
            }
            if let Some(rewritten) =
                rewrite_research_primitive(child.clone(), m, "planet", Some(ContentType::Planet))
            {
                out.insert(PLANET, rewritten);
            }
            continue;
        }
        out.insert(key.clone(), child.clone());
    }
    if out.is_empty() && m.removing {
        None
    } else {
        Some(Json::Obj(out))
    }
}

fn rewrite_research_primitive(
    value: Json,
    m: &Match,
    ctype: &str,
    required_type: Option<ContentType>,
) -> Option<Json> {
    let Json::Str(text) = &value else {
        return Some(value);
    };
    let (head, suffix) = split_stack(text);
    let Some(target) = m.match_research(head, ctype, required_type) else {
        return Some(value);
    };
    if m.removing {
        return None;
    }
    Some(Json::str(target.replacement(head).unwrap_or_default() + suffix))
}

fn rewrite_objectives(value: &Json, m: &Match) -> Vec<Json> {
    let mut out: Vec<Json> = Vec::new();
    let Json::Arr(items) = value else {
        return out;
    };
    for element in items {
        let Json::Obj(objective) = element else {
            out.push(element.clone());
            continue;
        };
        let type_name = match objective.get(TYPE) {
            Some(value) if is_primitive(value) => value.get_as_string(),
            _ => String::new(),
        };
        let target_key = objective_target_key(&type_name);
        // Java：named.isJsonPrimitive() && isString()
        if let Some(Json::Str(text)) = objective.get(target_key) {
            let (head, suffix) = split_stack(text);
            let required = match type_name.as_str() {
                "SectorComplete" | "OnSector" => Some(ContentType::Sector),
                "OnPlanet" => Some(ContentType::Planet),
                _ => None,
            };
            let matched = m.match_research(head, ctype_of_objective(&type_name), required);
            if let Some(matched) = matched {
                if m.removing {
                    // 目标没了，整条 objective 也跟着消失——留着会让游戏报悬空 objective。
                    continue;
                }
                let mut copy = objective.clone();
                copy.insert(
                    target_key,
                    Json::str(matched.replacement(head).unwrap_or_default() + suffix),
                );
                out.push(Json::Obj(copy));
                continue;
            }
        }
        out.push(element.clone());
    }
    out
}

/// objective 的目标槽位期望的内容类型，用于原版优先判断。
fn ctype_of_objective(type_name: &str) -> &str {
    match type_name {
        "SectorComplete" | "OnSector" => "sector",
        "OnPlanet" => "planet",
        _ => "any",
    }
}

/// Mirrors `TechTreeRules.objectiveTargetKey` without the UI-facing helpers.
fn objective_target_key(type_name: &str) -> &str {
    match type_name {
        "SectorComplete" | "OnSector" => PRESET,
        "OnPlanet" => PLANET,
        _ => CONTENT,
    }
}

fn rewrite_content_order(project: &mut ModProject, m: &Match) -> bool {
    let before = project.meta.content_order.clone();
    let mut rewritten: Vec<String> = Vec::new();
    for entry in project.meta.content_order.clone() {
        match m.match_order(&entry) {
            None => rewritten.push(entry),
            Some(target) => {
                if !m.removing {
                    rewritten.push(target.replacement(&entry).unwrap_or_default());
                }
            }
        }
    }
    if before != rewritten {
        project.meta.content_order = rewritten;
        return true;
    }
    false
}

/// 游戏在这些字段里先查原版，所以裸的原版名继续绑在原版上、不是本模组的覆盖。
/// `status` 与 `planet` 走各自的注册表；解锁研究在解析器查找该条目之前，
/// 先用 `Content.hasParent()` 检查 parent 名的前缀（arc 法），所以那里同样原版优先。
/// 与 `Validator.vanillaFirst` 同一条规则。
fn vanilla_first(field: &FieldDef) -> bool {
    field.content_ctype() == Some("status") || field.name() == PLANET || field.name() == RESEARCH
}

/// 目标的一侧改写形态：`(old_name, full_old) -> (new_name, full_new)`，删除时后两者为 `None`。
struct Target {
    old_name: String,
    full_old: String,
    new_name: Option<String>,
    full_new: Option<String>,
    ctype: ContentType,
}

impl Target {
    fn new(
        project: &ModProject,
        record: &ContentRecord,
        old_name: &str,
        new_name: Option<&str>,
    ) -> Self {
        let internal = project.internal_name();
        Target {
            old_name: old_name.to_string(),
            full_old: name_rules::full_name(&internal, old_name, record.patch),
            new_name: new_name.map(str::to_string),
            full_new: new_name.map(|name| name_rules::full_name(&internal, name, record.patch)),
            ctype: record.ctype,
        }
    }

    /// `head` 匹配本目标时要写回的写法。
    ///
    /// Java 里这一步是字符串拼接，`null` 会拼成字面量 `"null"`；全部调用点都先挡住了
    /// `removing`，可达路径上必然有值，故 Rust 用 `unwrap_or_default()`。
    fn replacement(&self, head: &str) -> Option<String> {
        if head == self.full_old {
            self.full_new.clone()
        } else {
            self.new_name.clone()
        }
    }
}

/// 一次改写要匹配的一组目标，外加游戏两条查表路径蕴含的匹配规则。
struct Match<'r> {
    registry: &'r SchemaRegistry,
    targets: Vec<Target>,
    removing: bool,
}

impl<'r> Match<'r> {
    fn new(registry: &'r SchemaRegistry, targets: Vec<Target>) -> Self {
        let removing = targets.iter().all(|target| target.new_name.is_none());
        Match {
            registry,
            targets,
            removing,
        }
    }

    /// 该字段里的裸名是否指本模组内容。只有「原版优先」字段指向某个原版名时才为 false：
    /// 游戏在那里偏好原版，那个写法不属于我们改写。
    fn match_field(
        &self,
        head: &str,
        field: Option<&FieldDef>,
        hint: Option<ContentType>,
    ) -> Option<&Target> {
        for target in &self.targets {
            if let Some(field) = field
                && !accepts(field, target)
            {
                continue;
            }
            if let Some(hint) = hint
                && target.ctype != hint
            {
                continue;
            }
            let bare = match field {
                None => true,
                Some(field) => {
                    !vanilla_first(field) || !self.vanilla_has(&target.old_name, field.content_ctype())
                }
            };
            if head == target.full_old || (bare && head == target.old_name) {
                return Some(target);
            }
        }
        None
    }

    /// 真实 `ammoTypes` 映射键指向的物品。键走 ContentParser.readValue 的「前缀优先」内容查找，
    /// 所以裸键也指本模组的物品，即使原版里有同名内容。
    fn match_ammo_key(&self, head: &str, key_type: ContentType) -> Option<&Target> {
        self.targets
            .iter()
            .find(|&target| target.ctype == key_type && (head == target.full_old || head == target.old_name))
    }

    /// 工厂计划或重装塔升级对里的单位。
    fn match_unit(&self, head: &str) -> Option<&Target> {
        self.match_field(head, None, Some(ContentType::Unit))
    }

    /// 裸的研究 parent/objective 名指向的目标。`required_type` 过滤槽位特定的引用
    /// （planet 与 preset），免得一个同名物品/单位意外改写星球或星区槽位；
    /// parent/content 槽位传 `None`。
    fn match_research(
        &self,
        head: &str,
        ctype: &str,
        required_type: Option<ContentType>,
    ) -> Option<&Target> {
        for target in &self.targets {
            if let Some(required_type) = required_type
                && target.ctype != required_type
            {
                continue;
            }
            let bare = !self.vanilla_has(&target.old_name, Some(ctype));
            if head == target.full_old || (bare && head == target.old_name) {
                return Some(target);
            }
        }
        None
    }

    /// contentOrder 列的就是本工程自己的内容名，所以裸写法总是算数。
    fn match_order(&self, head: &str) -> Option<&Target> {
        self.targets.iter().find(|&target| head == target.full_old || head == target.old_name)
    }

    /// 该名字是否也是某个原版内容的名字。`ctype` 为空或 "any" 时横跨所有类型判断。
    fn vanilla_has(&self, name: &str, ctype: Option<&str>) -> bool {
        if name.is_empty() {
            return false;
        }
        let ctype = match ctype {
            Some(ctype) if !ctype.is_empty() && ctype != "any" => Some(ctype),
            _ => None,
        };
        match ctype {
            None => ContentType::all()
                .into_iter()
                .any(|type_| self.registry.is_vanilla_name(type_, name)),
            Some(ctype) => ContentType::all()
                .into_iter()
                .find(|type_| type_.name() == ctype)
                .map(|type_| self.registry.is_vanilla_name(type_, name))
                .unwrap_or(false),
        }
    }
}

/// 该字段的值能否引用 `target` 所在的内容类型。
fn accepts(field: &FieldDef, target: &Target) -> bool {
    if field.field_type() == FieldType::Struct {
        // 结构化字段混着多种引用（UnitReq 里既有 block 也有物品成本），它的 stack kind
        // 是形状名而不是类型；类型线索在对象键上。
        return true;
    }
    if let Some(ctype) = field.content_ctype()
        && !ctype.is_empty() && ctype != "any" && ctype != target.ctype.name()
    {
        return false;
    }
    match field.stack_kind() {
        None => true,
        Some(kind) => kind.is_empty() || kind == "any" || kind == target.ctype.name(),
    }
}

/// "name/amount" 写法里的名字部分与包含斜杠的后缀。
fn split_stack(text: &str) -> (&str, &str) {
    match text.find('/') {
        Some(slash) => (&text[..slash], &text[slash..]),
        None => (text, ""),
    }
}

fn is_empty_array(value: &Json) -> bool {
    value.as_array().map(|items| items.is_empty()).unwrap_or(false)
}

/// Gson 的 `JsonElement.isJsonPrimitive()`。
fn is_primitive(json: &Json) -> bool {
    matches!(json, Json::Str(_) | Json::Num(_) | Json::Bool(_))
}

/// Gson 的 `JsonPrimitive.isString()`。
fn is_string(json: &Json) -> bool {
    matches!(json, Json::Str(_))
}
