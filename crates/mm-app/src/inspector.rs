//! 记录面板的「文本 ↔ 值」换算。
//!
//! 对应 Java `dev.modmaker.ui.RecordInspector` 里那 8 个不带 JavaFX 的 `static` 纯函数
//! 与 `editorRow()` 那张「字段类型 → 控件 → 提交什么」的换算表：
//!
//! | 本模块 | Java 出处 |
//! |---|---|
//! | [`plain`] | `RecordInspector.java:701-711` |
//! | [`string_array_text`] | `:713-720` |
//! | [`array_of`] | `:722-731` |
//! | [`stack_text`] | `:733-745` |
//! | [`stack_of`] | `:747-750` |
//! | [`stack_array_text`] | `:752-759` |
//! | [`stack_array_of`] | `:761-770` |
//! | [`number_of`] | `:772-783` |
//! | [`display_text`] | `:389-406`（读侧，即各编辑器构造时的初值） |
//! | [`bool_checked`] | `:561` |
//! | [`color_swatch`] | `:566-573` 取值 + `:577-580` 写回色块 |
//! | [`commit_text`] | `:389-406`（写侧）＋ 各编辑器提交那一支 |
//! | [`reference_candidates`] | `:677-697`（唯一调用点是 `contentEditor` `:620`） |
//! | [`hidden_in_fields_block`] | `:310-314`（fields 段那五项 `continue`） |
//!
//! ## 为什么换算表在这里而不是前端
//!
//! 硬约束第 6 条：字段规则一律在 Rust。这些函数决定「一个字段的值写成什么字节」——
//! `copper, lead` 要落成数组、`copper` 要落成裸字符串、`abc` 在数字格里根本不提交——
//! 属于字节契约，前端只消费结果：显示文本由 [`display_text`] 算好随 `record_schema` 下发，
//! 用户改完的文本经 [`commit_text`] 换算后才进 [`crate::project_controller::ProjectController::set_field`]。
//! 这也是 §四 M6 那条验收门（改 `width` 只让那一个文件差那一处）成立的前提。
//!
//! ## 与 Java 的三处口径差别（登记在 §四 M6 签字条目，不悄悄留着）
//!
//! 1. [`number_of`] 存的是**源文书写**（`mm_json::Json::num` 保留用户敲的那串数字原文），
//!    Java 那边 `new JsonPrimitive(Double.parseDouble(t))` 会把 `1.50` 规格化成 `1.5`；
//!    数值相同，字节写法不同。源文书写是本仓 `mm-json` 从 M1 起就定下的字节契约
//!    （`crates/mm-json/src/value.rs:83`、`Json::num` 的注释），M2 那条「写回 `9`」的门也按它过。
//! 2. [`stack_text`] 遇到 `item`／`liquid` 成员不是字符串时返回空串，
//!    Java 那边是 `JsonElement.getAsString()` 抛 `UnsupportedOperationException`（`:743`）；
//!    非对象的堆值 Java 走 `getAsJsonObject()` 抛 `IllegalStateException`（`:738`），
//!    本函数退化成 [`plain`]。前端展示用的小函数不该把整个壳带走。
//! 3. 颜色格的**合法性**检查（Java `:588-596` 用 `Color.web` 试解析，失败就标红不提交）
//!    这一支 P5 不实现：`Color.web` 收的是整套 CSS 颜色语法（十六进制 + 148 个具名色 +
//!    `rgb()/rgba()/hsl()`），照搬等于在 Rust 里再养一份 CSS 颜色表。后果如实登记：
//!    非法颜色文本会照样写进文件，Java 会拦。归 P6。

use mm_json::Json;
use mm_model::ModProject;
use mm_schema::{ContentType, FieldDef, FieldType, SchemaRegistry};

/// Java 那边把 `null`（成员不存在）与 `JsonNull` 都渲染成空串，其余原样。
/// 对应 `RecordInspector.plain`（`:701-711`）。
pub fn plain(value: Option<&Json>) -> String {
    match value {
        // `:702`：`value == null || value.isJsonNull()`
        None | Some(Json::Null) => String::new(),
        // `:703`：`value.isJsonPrimitive() ? value.getAsString()`
        Some(Json::Str(text)) | Some(Json::Num(text)) => text.clone(),
        Some(Json::Bool(flag)) => flag.to_string(),
        // `:704-710`：结构化值交给 `Json.write`，读侧仍是可解析的 JSON
        Some(other) => mm_json::write(other),
    }
}

/// 字符串数组格（Java 的 `textArray`）的显示文本：逗号＋空格连接；非数组退化成 [`plain`]。
/// 对应 `stringArrayText`（`:713-720`）。
pub fn string_array_text(value: Option<&Json>) -> String {
    match value {
        Some(Json::Arr(items)) => {
            let parts: Vec<String> = items.iter().map(|item| plain(Some(item))).collect();
            parts.join(", ")
        }
        other => plain(other),
    }
}

/// 把 `copper, lead, , sand` 切成字符串数组；全空白或切完为空 → `None`（＝删字段）。
/// 对应 `arrayOf`（`:722-731`）。
///
/// Java 用 `text.split(",")`（丢掉尾部空串），Rust 用 `split(',')`（保留）；
/// 两边随后都 `trim` 并跳过空段（`:726-728`），结果逐字相同。
pub fn array_of(text: &str) -> Option<Json> {
    // `:723`：`text == null || text.isBlank()`
    if text.trim().is_empty() {
        return None;
    }
    let items: Vec<Json> = text
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(Json::str)
        .collect();
    // `:730`：空数组按 `null` 处理
    if items.is_empty() {
        None
    } else {
        Some(Json::Arr(items))
    }
}

/// 堆（`stack`）格的显示文本：`物品/数量`，数量缺省为 `1`；裸字符串原样。
/// 对应 `stackText`（`:733-745`）。
pub fn stack_text(value: Option<&Json>) -> String {
    // `:734`
    let Some(value) = value else {
        return String::new();
    };
    if value.is_null() {
        return String::new();
    }
    // `:735`：`isJsonPrimitive() → getAsString()`
    if let Some(text) = primitive_text(value) {
        return text;
    }
    let Json::Obj(object) = value else {
        // 口径差别 2：Java 在此 `getAsJsonObject()` 会抛异常，本函数退化成 `plain`
        return plain(Some(value));
    };
    // `:738-740`：`item` 优先，其次 `liquid`
    let target = object.get("item").or_else(|| object.get("liquid"));
    let Some(target) = target else {
        // `:743`：两个都没有 → 空串（此时数量算了也不用）
        return String::new();
    };
    // `:741`：`amount` 缺省 `"1"`
    let amount = match object.get("amount") {
        Some(amount) => primitive_text(amount).unwrap_or_default(),
        None => "1".to_string(),
    };
    format!("{}/{}", primitive_text(target).unwrap_or_default(), amount)
}

/// 堆格的提交值：`trim` 后非空就是**一个裸字符串**，空就是 `None`。
/// 对应 `stackOf`（`:747-750`）——Java 不往这里写 `{item, amount}`，见该函数注释。
pub fn stack_of(text: &str) -> Option<Json> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(Json::str(trimmed))
    }
}

/// 堆数组格（`stackArray`）的显示文本：逗号＋空格连接每个 [`stack_text`]。
/// 对应 `stackArrayText`（`:752-759`）。
pub fn stack_array_text(value: Option<&Json>) -> String {
    match value {
        Some(Json::Arr(items)) => {
            let parts: Vec<String> = items.iter().map(|item| stack_text(Some(item))).collect();
            parts.join(", ")
        }
        other => stack_text(other),
    }
}

/// 堆数组格的提交值：与 [`array_of`] 同结果（Java 就是抄了一遍，差别只在 `:762` 没有
/// `isBlank()` 早退，而空段照样被 `:765` 跳过）。对应 `stackArrayOf`（`:761-770`）。
pub fn stack_array_of(text: &str) -> Option<Json> {
    let items: Vec<Json> = text
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(Json::str)
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(Json::Arr(items))
    }
}

/// 数字格的提交值：整数走整数写法、带小数点走小数写法、解析不了就**原样存字符串**，
/// 全空白 → `None`。对应 `numberOf`（`:772-783`）。
///
/// 调用方还得再看一眼结果是不是数字：Java 的 numberEditor（`:546-554`）只在
/// `parsed.isJsonPrimitive() && parsed.getAsJsonPrimitive().isNumber()` 时才提交，
/// 否则把输入框标红、模型不动——那条判断由 [`commit_text`] 的 [`Commit::Reject`] 承担。
pub fn number_of(text: &str) -> Option<Json> {
    let trimmed = text.trim();
    // `:774-776`
    if trimmed.is_empty() {
        return None;
    }
    // `:778`：含小数点走 `Double.parseDouble`，否则走 `Long.parseLong`
    let numeric = if trimmed.contains('.') {
        java_double(trimmed)
    } else {
        java_long(trimmed)
    };
    // `:778-782`：解析得了就是数字，`NumberFormatException` 就落回原文字符串
    Some(if numeric {
        Json::num(trimmed)
    } else {
        Json::str(trimmed)
    })
}

/// Java `Long.parseLong` 收的那串：可带一个前导 `-`，其后全是十进制数字，且不溢出 `long`。
/// 特别注意 Java **不认**前导 `+`（Rust 的 `i64::from_str` 认），所以这里先自己扫语法。
fn java_long(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) && text.parse::<i64>().is_ok()
}

/// Java `Double.parseDouble` 语法在本仓实际用得到的子集：
/// `[+|-] (数字串)? '.' 数字串 ([eE] [+|-]? 数字串)?` 或 `[+|-] 数字串 ('.' 数字串?)? ([eE] [+|-]? 数字串)?`，
/// 至少要有一个数字。
///
/// 如实登记不收的写法：`NaN`／`Infinity`／十六进制浮点（`0x1p3`）／`f``d` 后缀——
/// Java 认，本函数不认，于是 [`number_of`] 把它们当字符串存；内容文件里不会出现这些写法。
/// 之所以不用 `text.parse::<f64>()`，正是因为 Rust 的 `f64` 词法把 `inf`／`NaN` 也算合法数字。
fn java_double(text: &str) -> bool {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (body, None),
    };
    // 尾数：小数点两侧至少有一侧是完整数字串
    let mut parts = mantissa.splitn(2, '.');
    let head = parts.next().unwrap_or_default();
    let tail = parts.next().unwrap_or_default();
    let mantissa_ok = (head.is_empty() || head.bytes().all(|byte| byte.is_ascii_digit()))
        && (tail.is_empty() || tail.bytes().all(|byte| byte.is_ascii_digit()))
        && !(head.is_empty() && tail.is_empty())
        && parts.next().is_none();
    // 指数：可带符号的纯数字串，且不能缺数字
    let exponent_ok = match exponent {
        None => true,
        Some(exponent) => {
            let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        }
    };
    mantissa_ok && exponent_ok
}

/// 原语值的文本（Gson 的 `getAsString()`：字符串取内容，数字/布尔取其书写形式）。
fn primitive_text(value: &Json) -> Option<String> {
    match value {
        Json::Str(text) | Json::Num(text) => Some(text.clone()),
        Json::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// 一次提交的下场，对应 Java 各编辑器 `commit.accept(...)` 与它旁边那条「标红不提交」的分支。
#[derive(Debug, Clone, PartialEq)]
pub enum Commit {
    /// 写这个值。
    Value(Json),
    /// 删掉这个字段（Java 的 `commit.accept(null)`）。
    Unset,
    /// 什么都不做：文本非法，Java 那边给控件加 `editor-invalid` 样式（`:554`／`:595`）。
    Reject,
}

/// 按 Java `editorRow()`（`:389-406`）的读侧，算出某个字段当前值在该控件里显示的文本。
///
/// 布尔格的「勾没勾」不在这里，见 [`bool_checked`]；颜色格的色块见 [`color_swatch`]。
pub fn display_text(field_type: FieldType, value: Option<&Json>) -> String {
    match field_type {
        // `:391-392`（`text`／`sprite`）与 `:389`（`number`，初值 `plain(value)`，`:538`）
        FieldType::Text | FieldType::Sprite | FieldType::Number => plain(value),
        // `:393-394`
        FieldType::TextArray => string_array_text(value),
        // `:399`
        FieldType::Stack => stack_text(value),
        // `:400-401`
        FieldType::StackArray => stack_array_text(value),
        // `:561-564`：布尔格的文本没人看，给个可读的 `true`/`false` 便于面板之外的场合复用
        FieldType::Bool => value.map_or_else(String::new, |v| primitive_text(v).unwrap_or_default()),
        // `:570`：颜色格显示 `plain(value)`
        FieldType::Color => plain(value),
        // `:602`／`:618`：枚举格与引用格的文本都是 `plain(value)`
        FieldType::Enumeration | FieldType::Content => plain(value),
        // `:631`：原始格就是 `Json.write(value)`，与 [`plain`] 对结构化值的分支同结果
        FieldType::Struct | FieldType::Raw => plain(value),
    }
}

/// 布尔格的勾选态，对应 Java `:561`：`value != null && isJsonPrimitive && getAsBoolean()`。
pub fn bool_checked(value: Option<&Json>) -> bool {
    // `getAsBoolean()` 在 Gson 里对任意原语都能出值（字符串按 `"true"` 判），
    // 但本仓的 `Json::Bool` 才承载布尔；数字/字符串按 Java 的口径落回 `false`。
    match value {
        Some(Json::Bool(flag)) => *flag,
        Some(Json::Str(text)) => text == "true",
        _ => false,
    }
}

/// 颜色格的色块，对应 Java `:566-573`（取文本 → `Color.web`，解不出来用白色）
/// 与 `:577-580`（把颜色写回 `#rrggbb`）。
///
/// 只认十六进制三／四／六／八位与 `0x` 写法；JavaFX 的 `Color.web` 还认整套 CSS 颜色，
/// 那部分不落地（见模块头口径差别 3），落到白色与 Java 的兜底分支一致。
pub fn color_swatch(value: Option<&Json>) -> String {
    let text = plain(value);
    let text = text.trim();
    if text.is_empty() {
        return "#ffffff".to_string();
    }
    match hex_color(text) {
        Some(rgb) => rgb,
        None => "#ffffff".to_string(),
    }
}

/// 把 CSS 十六进制色（`#rgb`／`#rgba`／`#rrggbb`／`#rrggbbaa`，`#` 可省，也收 `0x` 前缀）
/// 折成 `#rrggbb`；任何一位不是十六进制字符、或长度不在那四档里，都返回 `None`。
fn hex_color(text: &str) -> Option<String> {
    let hex = text
        .strip_prefix('#')
        .or_else(|| text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")))
        .unwrap_or(text);
    let nibbles: Vec<u32> = hex
        .chars()
        .map(|ch| ch.to_digit(16))
        .collect::<Option<Vec<u32>>>()?;
    // JavaFX 的 3／4 位写法每位自我重复（`f` → `ff`），6／8 位写法两位一组
    let bytes: Vec<u32> = match nibbles.len() {
        3 | 4 => nibbles.iter().map(|nibble| (nibble << 4) | nibble).collect(),
        6 | 8 => nibbles
            .chunks(2)
            .map(|pair| pair[0] * 16 + pair[1])
            .collect(),
        _ => return None,
    };
    // `:577-580` 只要红绿蓝；带 alpha 的写法丢掉 alpha，与 JavaFX 取同样三分量一致
    Some(format!("#{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2]))
}

/// 按 Java `editorRow()`（`:389-406`）的写侧，把用户在控件里敲的文本换算成要提交的东西。
///
/// `field_type == None` 表示这个字段名在 schema 表里没有——对应 Java 的「添加字段」那一行
/// （`addFieldRow` `:656-670`）：全空白存空串，能按 JSON 读就读，读不了存原文字符串。
pub fn commit_text(field_type: Option<FieldType>, text: &str) -> Commit {
    match field_type {
        // `:389` ＋ numberEditor `:541-556`：空白删字段，非数字不提交
        Some(FieldType::Number) => match number_of(text) {
            None => Commit::Unset,
            Some(value @ Json::Num(_)) => Commit::Value(value),
            Some(_) => Commit::Reject,
        },
        // `:391-392`：文本／贴图原样存字符串（Java 不 `trim`），空串也照样存
        Some(FieldType::Text) | Some(FieldType::Sprite) => Commit::Value(Json::str(text)),
        // `:393-394`：逗号切成数组，切完为空删字段
        Some(FieldType::TextArray) => match array_of(text) {
            Some(value) => Commit::Value(value),
            None => Commit::Unset,
        },
        // `:395-396` ＋ boolEditor `:562`：只有 `true`／`false` 两种文本
        Some(FieldType::Bool) => match text {
            "true" => Commit::Value(Json::Bool(true)),
            "false" => Commit::Value(Json::Bool(false)),
            _ => Commit::Reject,
        },
        // `:397-398` ＋ colorEditor `:585`：提交 `trim` 后的文本；合法性见模块头口径差别 3
        Some(FieldType::Color) => Commit::Value(Json::str(text.trim())),
        // `:402-405` ＋ enumEditor `:608-611`／contentEditor `:622-625`：
        // Java 只在 `now != null` 时提交，前端清空走命令的 `None` 分支，不走这里的空串
        Some(FieldType::Enumeration) | Some(FieldType::Content) => Commit::Value(Json::str(text)),
        // `:399` ＋ `stackOf`：`trim` 后是裸字符串，空则删字段
        Some(FieldType::Stack) => match stack_of(text) {
            Some(value) => Commit::Value(value),
            None => Commit::Unset,
        },
        // `:400-401` ＋ `stackArrayOf`
        Some(FieldType::StackArray) => match stack_array_of(text) {
            Some(value) => Commit::Value(value),
            None => Commit::Unset,
        },
        // `:405-406` ＋ rawEditor `:636-640`：空白删字段，读不动就记一条日志且不提交
        Some(FieldType::Struct) | Some(FieldType::Raw) => {
            if text.trim().is_empty() {
                Commit::Unset
            } else {
                match mm_json::read(text) {
                    Ok(value) => Commit::Value(value),
                    Err(_) => Commit::Reject,
                }
            }
        }
        // 「添加字段」那一行（`:661-667`）
        None => {
            if text.trim().is_empty() {
                Commit::Value(Json::str(""))
            } else {
                match mm_json::read(text) {
                    Ok(value) => Commit::Value(value),
                    Err(_) => Commit::Value(Json::str(text)),
                }
            }
        }
    }
}

/// fields 段被摘掉的那五个字段名。Java `:310-314` 逐字列出的是
/// `type`／`name`／`description`／`details`／`credit`——它们各有专属位置（identity 段、
/// localisation 段），所以通用字段表里不再重复一行。
const HIDDEN_IN_FIELDS: [&str; 5] = ["type", "name", "description", "details", "credit"];

/// 这个字段是否出现在 fields 段的行表里。对应 `RecordInspector.java:309-316` 的 `continue`。
pub fn hidden_in_fields_block(name: &str) -> bool {
    HIDDEN_IN_FIELDS.contains(&name)
}

/// 一个 `content` 格的候选名单。对应 `referenceCandidates`（`:677-697`）。
///
/// 逐支口径：
/// - `:679-680`：`wanted = definition.contentCtype()`、`internal = project.internalName()`；
/// - `:681-689`：先扫本项目 `contents`，`:682` 摘掉 inline 与非 `file_backed` 的记录，
///   `:685-687` 按 `wanted` 过滤（`None` 或 `"any"` 都算不限定），落 `record.fullName(internal)`；
/// - `:690-695`：再按 `ContentType` 全枚举追加 `vanillaNames(type)`，**每个类型各 `limit 120`**；
/// - `:696`：`distinct().limit(400)`——先去重再截 400，顺序为插入序（本项目在前、原版在后）。
pub fn reference_candidates(
    project: &ModProject,
    registry: &SchemaRegistry,
    field: &FieldDef,
) -> Vec<String> {
    let wanted = field.content_ctype.as_deref();
    let internal = project.internal_name();
    let mut candidates: Vec<String> = Vec::new();
    for record in &project.contents {
        // `:682`
        if record.is_inline() || !record.ctype.file_backed() {
            continue;
        }
        // `:685-687`
        if !wanted.is_none_or(|name| name == "any" || record.ctype.name() == name) {
            continue;
        }
        candidates.push(record.full_name(&internal));
    }
    for ctype in ContentType::all() {
        // `:691-693`
        if !wanted.is_none_or(|name| name == "any" || ctype.name() == name) {
            continue;
        }
        candidates.extend(registry.vanilla_names(ctype).into_iter().take(120));
    }
    // `:696`：distinct 保序 + limit 400
    let mut out: Vec<String> = Vec::new();
    for candidate in candidates {
        if out.contains(&candidate) {
            continue;
        }
        out.push(candidate);
        if out.len() == 400 {
            break;
        }
    }
    out
}
