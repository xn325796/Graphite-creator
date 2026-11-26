//! `JsonCanvas.java` 的等价移植：board 的 JSON Canvas 1.0 读写，让创作层保持「能在 Obsidian 里
//! 直接打开」。
//!
//! 对应 Java `dev.modmaker.core.io.JsonCanvas`（`public final class` + 私有构造的纯静态工具类）：
//! Rust 用模块与模块级 `pub fn` 对应，函数顺序、成员写出顺序与 Java 一一对应。
//!
//! 节点与边的语义里，画布表达不出来的那部分放在名为 "mindustry" 的扩展成员里：节点显示的是哪条
//! 内容记录，边代表的是哪个字段。Obsidian 只显示节点文本，其余忽略。

use mm_fmt::{codec::java_double_text, ids::uid, name_rules};
use mm_json::{child, read_object, write_compact, Json, Object};
use mm_model::canvas_node::{KIND_CONTENT, KIND_GROUP, KIND_NOTE};
use mm_model::{Board, CanvasEdge, CanvasNode, ModProject};

/// 记录查不到时给内容卡用的底色（Java 里写死的 `#3f3f46`）。
const MISSING_COLOR: &str = "#3f3f46";

/// 把一块画布写成 JSON Canvas 树（Java 的 `toCanvas`）：`{"nodes": [...], "edges": [...]}`。
///
/// `project` 允许缺席（Java 传 `null`）：没有项目就查不到记录，内容卡退化成占位文本加灰底色条。
pub fn to_canvas(project: Option<&ModProject>, board: &Board) -> Json {
    let mut canvas = Object::new();

    let mut nodes: Vec<Json> = Vec::new();
    for node in &board.nodes {
        let mut json = Object::new();
        json.insert("id", json_string(&node.id));
        json.insert(
            "type",
            Json::str(if node.kind == KIND_GROUP { "group" } else { "text" }),
        );
        json.insert("x", json_number(node.x));
        json.insert("y", json_number(node.y));
        json.insert("width", json_number(node.w));
        json.insert("height", json_number(node.h));
        json.insert("text", Json::str(node_text(project, node)));
        if node.kind == KIND_CONTENT {
            let record = match project {
                Some(project) => match &node.content_id {
                    Some(id) => project.content_by_id(id),
                    None => None,
                },
                None => None,
            };
            let color = record.map(|record| record.ctype.color()).unwrap_or(MISSING_COLOR);
            json.insert("color", Json::str(color));
        } else if !node.color.is_empty() {
            json.insert("color", Json::str(node.color.clone()));
        }

        let mut extension = Object::new();
        extension.insert("kind", Json::str(node.kind.clone()));
        if let Some(content_id) = &node.content_id {
            extension.insert("contentId", Json::str(content_id.clone()));
        }
        if let Some(ref_name) = &node.ref_name {
            extension.insert("refName", Json::str(ref_name.clone()));
            if let Some(ref_type) = &node.ref_type {
                extension.insert("refType", Json::str(ref_type.clone()));
            }
        }
        json.insert("mindustry", Json::Obj(extension));
        nodes.push(Json::Obj(json));
    }

    let mut edges: Vec<Json> = Vec::new();
    for edge in &board.edges {
        let mut json = Object::new();
        json.insert("id", json_string(&edge.id));
        json.insert("fromNode", json_string(&edge.from));
        json.insert("toNode", json_string(&edge.to));
        json.insert("fromSide", Json::str("right"));
        json.insert("toSide", Json::str("left"));
        if !edge.label.is_empty() {
            json.insert("label", Json::str(edge.label.clone()));
        }
        let mut extension = Object::new();
        if let Some(field_key) = &edge.field_key {
            extension.insert("fieldKey", Json::str(field_key.clone()));
        }
        if !edge.reroutes.is_empty() {
            let mut points: Vec<Json> = Vec::new();
            for point in &edge.reroutes {
                let pair = Json::Arr(vec![json_number(point[0]), json_number(point[1])]);
                points.push(pair);
            }
            if !points.is_empty() {
                extension.insert("reroutes", Json::Arr(points));
            }
        }
        json.insert("mindustry", Json::Obj(extension));
        edges.push(Json::Obj(json));
    }

    canvas.insert("nodes", Json::Arr(nodes));
    canvas.insert("edges", Json::Arr(edges));
    Json::Obj(canvas)
}

/// 读一块画布（Java 的 `fromCanvas`）。文件坏掉时返回只带 id/文件名/画布名的空板，与 Java 一样
/// 不把解析失败往上抛——创作层是用户可编辑的文本，读不出来也要能继续编辑。
pub fn from_canvas(text: &str, board_file: &str, board_id: &str) -> Board {
    let mut board = Board {
        id: Some(board_id.to_string()),
        file: board_file.to_string(),
        name: board_file.strip_suffix(".canvas").unwrap_or(board_file).to_string(),
        ..Default::default()
    };

    let canvas = match read_object(text) {
        Ok(canvas) => canvas,
        // Java：`try { Json.parseObject(text) } catch (RuntimeException broken) { return board; }`。
        Err(_) => return board,
    };

    // Java 用 `LinkedHashMap<String, CanvasNode>` 只为判存在，这里同样保序地记下已见节点 id。
    let mut nodes_by_id: Vec<String> = Vec::new();
    if let Some(nodes) = array(&canvas, "nodes") {
        for element in nodes {
            // Java：`if (!element.isJsonObject()) continue;`
            let json = match element {
                Json::Obj(json) => json,
                _ => continue,
            };
            let mut node = CanvasNode::default();
            // Java 的第三实参在调用前就求值：即使命中了文件里的 id，`Ids.uid("n")` 也照样被调用，
            // 全局计数器随之自增。逐字照搬，不改惰性默认值，免得 uid 序列与 Java 分叉。
            let generated_id = uid("n");
            let id = string(json, "id", &generated_id);
            node.id = Some(id.clone());
            node.x = number(json, "x", 0.0);
            node.y = number(json, "y", 0.0);
            // Java 在这里与下一行各调一次 `type(json)`；它是纯函数，这里取一次即可。
            let kind = type_of(json);
            let group = kind == KIND_GROUP;
            node.w = number(json, "width", if group { 400.0 } else { 260.0 });
            node.h = number(json, "height", if group { 300.0 } else { 130.0 });
            node.text = string(json, "text", "");
            node.color = string(json, "color", "");
            node.kind = kind;
            let extension = object(json, "mindustry");
            if let Some(extension) = extension {
                // 用 `self::` 限定：本函数入参名为 `text`（与 Java 同名），它会把模块级的
                // 辅助函数 `text` 遮成一个 &str。
                if let Some(content_id) = self::text(extension, "contentId") {
                    node.content_id = Some(content_id);
                }
                if let Some(ref_name) = self::text(extension, "refName") {
                    node.ref_name = Some(ref_name);
                    node.ref_type = Some(string(extension, "refType", "item"));
                }
            }
            if node.kind == KIND_CONTENT && node.content_id.is_none() && node.ref_name.is_none() {
                // 一张普通 Obsidian 卡片不承载内容；按便签留着，免得丢东西。
                node.kind = KIND_NOTE.to_string();
            }
            board.nodes.push(node);
            nodes_by_id.push(id);
        }
    }

    if let Some(edges) = array(&canvas, "edges") {
        for element in edges {
            // Java：`if (!element.isJsonObject()) continue;`
            let json = match element {
                Json::Obj(json) => json,
                _ => continue,
            };
            let mut edge = CanvasEdge::default();
            // 同上：Java 的 `Ids.uid("e")` 实参先求值。
            let generated_id = uid("e");
            edge.id = Some(string(json, "id", &generated_id));
            edge.from = Some(string(json, "fromNode", ""));
            edge.to = Some(string(json, "toNode", ""));
            edge.label = string(json, "label", "");
            if let Some(extension) = object(json, "mindustry") {
                if let Some(field_key) = self::text(extension, "fieldKey") {
                    edge.field_key = Some(field_key);
                }
                if let Some(points) = array(extension, "reroutes") {
                    // 文件是用户可编辑的文本：保留格式正确的控制点，其余丢掉。
                    for point in points {
                        let pair = match point {
                            Json::Arr(pair) => pair,
                            _ => continue,
                        };
                        if pair.len() < 2 || !is_primitive(&pair[0]) || !is_primitive(&pair[1]) {
                            continue;
                        }
                        // Java 先取 [0] 再取 [1]，任一分量抛 NumberFormatException 就整对丢掉；
                        // 丢掉的这个控制点不要了。
                        if let (Some(x), Some(y)) = (coordinate(&pair[0]), coordinate(&pair[1])) {
                            edge.reroutes.push([x, y]);
                        }
                    }
                }
            }
            let from_known = edge
                .from
                .as_deref()
                .is_some_and(|from| nodes_by_id.iter().any(|id| id == from));
            let to_known = edge
                .to
                .as_deref()
                .is_some_and(|to| nodes_by_id.iter().any(|id| id == to));
            if from_known && to_known {
                board.edges.push(edge);
            }
        }
    }
    board
}

/// 卡片正文（Java 的私有 `nodeText`）：内容卡由记录拼出 Markdown，其余节点用自身文本。
fn node_text(project: Option<&ModProject>, node: &CanvasNode) -> String {
    if node.kind == KIND_CONTENT {
        let record = match project {
            Some(project) => match &node.content_id {
                Some(id) => project.content_by_id(id),
                None => None,
            },
            None => None,
        };
        let record = match record {
            Some(record) => record,
            None => {
                return match &node.ref_name {
                    None => "**(missing content)**".to_string(),
                    Some(ref_name) => format!("**{ref_name}**"),
                };
            }
        };
        let class_name = match record.class_name() {
            Some(class_name) => class_name,
            None => record.ctype.default_class().to_string(),
        };
        let display = match project {
            // Java 里这段只可能从非 null 的 project 走到（record 就是从它取出来的）。
            Some(project) => project
                .strings
                .get(
                    &name_rules::bundle_key(
                        record.ctype,
                        &record.full_name(&project.internal_name()),
                        "name",
                    ),
                    "en",
                )
                .map(|value| value.to_string()),
            None => None,
        };
        let display = match display {
            Some(display) if !is_blank(&display) => display,
            _ => record.name.clone(),
        };
        return format!(
            "**{display}**\n{} · {class_name}{}{}",
            record.ctype.name(),
            if record.is_inline() { " · inline" } else { "" },
            if record.patch { " · patch" } else { "" }
        );
    }
    node.text.clone()
}

/// 节点语义类型（Java 的私有 `type`）：优先扩展里的 `kind`，否则由画布的 `type` 反推。
/// Java 的方法名 `type` 是 Rust 关键字，故改名 `type_of`。
fn type_of(json: &Object) -> String {
    let extension = object(json, "mindustry");
    let kind = match extension {
        Some(extension) => text(extension, "kind"),
        None => None,
    };
    if let Some(kind) = kind {
        return kind;
    }
    let kind_type = string(json, "type", "text");
    if kind_type == "group" {
        KIND_GROUP.to_string()
    } else {
        KIND_NOTE.to_string()
    }
}

/// Java `string(json, key, fallback)`：非原语（含缺失与 null）取兜底值。
fn string(json: &Object, key: &str, fallback: &str) -> String {
    match json.get(key) {
        Some(value) if is_primitive(value) => value.get_as_string(),
        _ => fallback.to_string(),
    }
}

/// Java `text(json, key)`：原语才要，否则 `null`。
fn text(json: &Object, key: &str) -> Option<String> {
    json.get(key).filter(|value| is_primitive(value)).map(|value| value.get_as_string())
}

/// Java `object(json, key)`：对象才要，否则 `null`。
fn object<'a>(json: &'a Object, key: &str) -> Option<&'a Object> {
    json.get(key).and_then(|value| value.as_object())
}

/// Java `array(json, key)`：数组才要，否则 `null`。
fn array<'a>(json: &'a Object, key: &str) -> Option<&'a Vec<Json>> {
    json.get(key).and_then(|value| value.as_array())
}

/// Java `number(json, key, fallback)`：非原语、解析失败或非有限值都退回兜底。
fn number(json: &Object, key: &str, fallback: f64) -> f64 {
    let value = match json.get(key) {
        Some(value) => value,
        None => return fallback,
    };
    if !is_primitive(value) {
        return fallback;
    }
    // Gson `getAsDouble` 对布尔走 `Double.parseDouble("true")` 抛 NumberFormatException，
    // 落到 Java 的 `catch (RuntimeException)`，所以这里布尔一律算解析失败。
    match value {
        Json::Str(text) | Json::Num(text) => match text.parse::<f64>() {
            Ok(parsed) if parsed.is_finite() => parsed,
            _ => fallback,
        },
        _ => fallback,
    }
}

/// 控制点分量：Gson `getAsDouble`，抛异常即 `None`（Java 的 `NumberFormatException` 分支）。
fn coordinate(value: &Json) -> Option<f64> {
    match value {
        Json::Str(text) | Json::Num(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

/// Gson `JsonElement.isJsonPrimitive()`：字符串/数字/布尔，`null` 不算。
fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Str(_) | Json::Num(_) | Json::Bool(_))
}

/// Java `addProperty(String, String)`：实参为 `null` 时写出的是 `null` 成员，而不是省略它。
fn json_string(value: &Option<String>) -> Json {
    match value {
        Some(value) => Json::str(value.clone()),
        None => Json::Null,
    }
}

/// Java `addProperty(String, double)`：Gson 把 `Number` 原样交给 `JsonWriter`，落纸的是
/// `Double.toString` 的拼写（`10.0` 而非 `10`），所以数字一律走 `java_double_text`。
fn json_number(value: f64) -> Json {
    Json::num(java_double_text(value))
}

/// Java `String.isBlank()`：逐字符按 `Character.isWhitespace` 判定。
fn is_blank(text: &str) -> bool {
    text.chars().all(java_whitespace)
}

/// Java `Character.isWhitespace`：**不含** U+00A0 不间断空格（bundle 里用它做视觉对齐，必须留在
/// 值内）与 U+0085 NEL。与 mm-fmt 里同名判定保持同一套字符类。
fn java_whitespace(c: char) -> bool {
    if c == '\u{a0}' || c == '\u{85}' {
        return false;
    }
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
        || matches!(c, '\u{1c}'..='\u{1f}')
        || c == ' '
        || c.is_whitespace()
}

/// Gson `JsonElement.toString()` 的等价物：紧凑（无缩进、无空格）JSON，且不做 HTML 转义。
///
/// Java 侧测试直接对返回的 `JsonObject` 调 `toString()`；Rust 的 [`Json`] 没有这个内建方法，
/// 故以模块级函数对应，输出逐字节等价。
pub fn to_text(canvas: &Json) -> String {
    write_compact(canvas)
}

/// 供测试使用：指向嵌套内容时用到的路径 helper（Java 的包级 `nestedPath`）。
///
/// Java 里它是包私有且暂无调用方；Rust 的包私有项若无引用会报 dead_code，故按 Java 文档的本意
/// （「Exposed for tests」）公开，行为不变。
pub fn nested_path(parent: &str, key: &str) -> String {
    child(parent, key)
}
