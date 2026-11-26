//! Java 画布卡片的**排版尺寸与端口锚点**——`dev.modmaker.ui` 卡片目录的等价移植。
//!
//! ## 为什么在 Rust 算
//!
//! 方案文档 §〇 决策 3 与 §一·变坏 2 的口径是「前端只画不算」。画布上的边必须接在**端口**上，
//! 而端口的位置来自卡片排版（哪几行进卡片、第几行、卡片多高），这套规则在 Java 里是
//! `NodeCanvasPane.java:558-573 widgetFields()`、`:604-624 inputPorts()`、
//! `ContentCard.java:114-115`（`height = HEADER_HEIGHT + rows * ROW_HEIGHT`）与
//! `:156-169`（端口落点）。前端拿不到 schema、也拿不到字段值，所以这三件事只能在这里算完，
//! 由 `src-tauri/src/dto.rs` 随 `board_snapshot` 下发。
//!
//! M4 的 `dto.rs::port_anchor` 只有「节点矩形中线端点」这一档回退（Java `portWorld` 的
//! `:1429-1432`），本模块补上 Java 的**精确锚点**那一档，并把 M4 卡片宽高误用 `node.w/h` 的账结掉。
//!
//! ## 与 Java 的两处结构性偏离（逐字登记，不记暗账）
//!
//! 1. **Java 是三个 UI 类，这里是一个纯函数模块**。Java 把几何藏在控件对象里
//!    （`ContentCard` 是 `StackPane` 子类、`CarrierCard` 是 `BorderPane` 子类，构造时顺手把
//!    `ports` 填进列表）；
//!    Rust 侧不可能有 JavaFX 节点，于是只保留**数据**：`CardMetrics{width,height,ports}`。
//!    类名映射：`ContentCard.java:101-172` 构造器 → [`content_card`]；
//!    `CarrierCard.java:50-109` 构造器 → [`carrier_card`]；
//!    `ContentCard.java:441 portAnchor(fieldKey)` / `CarrierCard.java:130 portAnchor(fieldKey)`
//!    → [`port_anchor`]。
//! 2. **`MiniNodeCard`（科技树专用卡，Java 123 行）不在本模块**。科技树画板是 P6 的
//!    `TechTreePane` 等价物，M5 只做内容画板；`mm-layout/src/tech_tree_canvas_layout.rs` 已在 M1
//!    移植了布局算法，缺的是那张卡的绘制，登记在 M5 签字条目的欠账里。
//!
//! 控件本身（滑块、开关、下拉、文本框——Java `ContentCard.java:202-260 rowPanel` 一族）**不移植**：
//! 那是 M6 的 `RecordInspector` 与 P6 的画布内联编辑要管的事，M5 只画矩形、标题、行标签与端口。

use mm_model::ContentRecord;
use mm_schema::{ContentType, FieldType, SchemaRegistry};

// --- 卡片目录的常量（逐字取自 Java）---------------------------------------------------------------

/// Java `ContentCard.java:45 WIDTH=192`。
pub const CONTENT_CARD_WIDTH: f64 = 192.0;
/// Java `ContentCard.java:46 HEIGHT=150`——**只是回退值**：卡片真实高度是
/// `HEADER_HEIGHT + rows * ROW_HEIGHT`（`:114-115`），而 `:1429-1432 portWorld` 与
/// `:1404-1405` 的拖放落位用的是这个固定 150。画布命中（`nodeAt` `:1507-1510` 走
/// `card.cardWidth()/cardHeight()`）用真实高度，
/// 端口回退用 150，两者在 Java 里就不一致，这里原样保留。
///
/// 还要记住回退用的 150 **不是任何真实卡片的高度**：真实高度恒为 `32 + rows*32`（`:115`），
/// 而 `(150 - 32) / 32 = 3.6875` 不是整数行。它是原型稿 `方块构建.html` 的固定值，
/// 只在上述两处回退里出现，这里不做"折算成行数"的合理化。
pub const CONTENT_CARD_FALLBACK_HEIGHT: f64 = 150.0;

/// Java `ContentCard.java:47 HEADER_HEIGHT=32`。
pub const HEADER_HEIGHT: f64 = 32.0;
/// Java `ContentCard.java:48 ROW_HEIGHT=32`。
pub const ROW_HEIGHT: f64 = 32.0;
/// Java `ContentCard.java:49 LABEL_WIDTH=66`（行标签宽 66+12，控件面板从 `8+66` 起，`:192`/`:203`）。
pub const LABEL_WIDTH: f64 = 66.0;
/// Java `ContentCard.java:50 PORT_RADIUS=4`。
pub const PORT_RADIUS: f64 = 4.0;
/// Java `ContentCard.java:51 MAX_INPUT_PORTS=8`——**实读确认 Java 里没有任何使用点**
/// （`Grep` 只命中定义行），端口数的真实上限是 [`MAX_WIDGET_ROWS`]。常量保留只为口径对齐。
pub const MAX_INPUT_PORTS: usize = 8;
/// Java `ContentCard.java:52 MAX_WIDGET_ROWS=8`，用在 `NodeCanvasPane.java:572 .limit(8)`。
pub const MAX_WIDGET_ROWS: usize = 8;
/// Java `CarrierCard.java:34 WIDTH=260`。
pub const CARRIER_WIDTH: f64 = 260.0;
/// Java `CarrierCard.java:35 HEIGHT=64`。
pub const CARRIER_HEIGHT: f64 = 64.0;

/// Java `NodeCanvasPane.java:1569-1571 PREVIEW_PRIORITY`（在 `:561` 与 `:1578` 两处取用）。
///
/// 逐字照抄 13 项，顺序即占行顺序。注意优先级段**只看 schema、不看记录**（`:562`
/// `defs.stream().filter(name).findFirst().ifPresent(ordered::add)`），所以一个字段只要在这
/// 个类里存在就占一行，哪怕记录里根本没写它——Java 的卡片就是这么"没写也显示"的。
pub const PREVIEW_PRIORITY: [&str; 13] = [
    "color",
    "health",
    "damage",
    "requirements",
    "research",
    "reload",
    "range",
    "speed",
    "powerCapacity",
    "itemCapacity",
    "liquidCapacity",
    "size",
    "category",
];

/// Java `NodeCanvasPane.java:575-576 EXCLUDED_WIDGET_FIELDS`。
///
/// 使用位置是 `:571 ordered.removeIf(...)`——在两个循环**之后**、`:572 .limit(8)` 之前统一剔除，
/// 不是"只作用于自动补行"：优先级列表里若出现这五项（当前 13 项里没有），同样会被摘掉，
/// 而且摘掉后名额不再回补。上一轮调研报告把排除项说成 `requirements`/`research`，是转述错误。
pub const EXCLUDED_WIDGET_FIELDS: [&str; 5] = [
    "type",
    "name",
    "description",
    "details",
    "credit",
];

// --- 下发给前端的形状-----------------------------------------------------------------------------

/// 一个可连线的端口。`field_key == None` 是 ID 输出口（Java `ContentCard.Port`：
/// `:169 new Port(null, null, WIDTH - PORT_RADIUS, HEADER_HEIGHT / 2.0, id)`）。
#[derive(Debug, Clone, PartialEq)]
pub struct CardPort {
    /// 归属字段；`None` = ID 口。
    pub field_key: Option<String>,
    /// 相对卡片左上角的锚点（Java `CanvasCard.java:29 portAnchor` 的返回值口径）。
    pub x: f64,
    pub y: f64,
}

/// 一张卡的排版结果。前端只需 `width`/`height`/`ports`，不需要知道它是怎么排出来的。
#[derive(Debug, Clone, PartialEq)]
pub struct CardMetrics {
    /// `true` = 补给卡（item/liquid 或纯原版引用），`false` = 内容卡。
    pub carrier: bool,
    pub width: f64,
    pub height: f64,
    pub ports: Vec<CardPort>,
    /// 卡片标题行（Java `displayName` + `"  ·patch"`，`ContentCard.java:130`）。
    pub title: String,
    /// 内容卡的类型色条用 [`ContentType::color()`]；Java `ContentCard.java:125` 把它写成内联
    /// `-fx-background-color`，前端对应矩形填充色。补给卡同样用类型色（`CarrierCard` 的 head）。
    pub color: String,
}

/// 记录是否画成补给卡——Java `NodeCanvasPane.java:502`：
/// `record.type == ContentType.item || record.type == ContentType.liquid`。
/// 判定只看**记录的 ctype**，与 `node.kind` 无关（一个 `kind=="block"` 的节点挂着 item 记录，
/// Java 依然画 CarrierCard）。
pub fn is_carrier_record(record: &ContentRecord) -> bool {
    matches!(record.ctype, ContentType::Item | ContentType::Liquid)
}

// --- 行与端口选择（Java NodeCanvasPane 的两个私有方法）--------------------------------------------

/// Java `NodeCanvasPane.java:558-573 widgetFields(record)`。
///
/// 口径逐条照 Java：
/// - `:561-563` 优先级段：遍历 [`PREVIEW_PRIORITY`]，**只问 schema**（这个类里有没有这个字段），
///   有就进，与记录是否写过无关；
/// - `:564-570` 自动补行段：`!ordered.contains(def)`（`:565`）+ `record.fields.get(name) != null`
///   （`:565`，Gson 的 `get` 只在**键不存在**时返回 null，显式 JSON null 是 `JsonNull` 非 null，
///   所以 Rust 侧对应的就是 [`mm_json::Object::get`] 的 `is_some()`）+ 类型 ∈
///   bool/number/text/sprite（`:566-567`）；
/// - `:571` 最后统一 `removeIf(EXCLUDED_WIDGET_FIELDS)`；`:572` 再 `.limit(8)`。
///
/// 这里**没有**「预览文本为空/null/unknown 就不占行」这一条：Java 的 `widgetFields` 里查不到
/// 任何 preview 判空逻辑，`""`/`"null"`/`"unknown"` 只是 `ContentCard.java:310-315 plainText`
/// 与 `:1575 preview(record)` 的**显示**口径，不影响行数（上一轮调研报告把它当成占行条件，
/// 本函数按实读纠正）。
///
/// 返回**字段名**列表（Java 返回 `FieldDef`；卡片只需要名字，类型只在 [`input_ports`] 那侧起作用）。
pub fn widget_fields(record: &ContentRecord, class_name: &str, registry: &SchemaRegistry) -> Vec<String> {
    let fields = registry.fields_of(class_name);
    let mut ordered: Vec<String> = Vec::new();
    for name in PREVIEW_PRIORITY {
        if fields.iter().any(|field| field.name == *name) {
            ordered.push((*name).to_string());
        }
    }
    for field in fields.iter() {
        if ordered.iter().any(|item| item == &field.name) {
            continue;
        }
        if record.fields.get(&field.name).is_none() {
            continue;
        }
        if !matches!(
            field.field_type,
            FieldType::Bool | FieldType::Number | FieldType::Text | FieldType::Sprite
        ) {
            continue;
        }
        ordered.push(field.name.clone());
    }
    ordered.retain(|name| !EXCLUDED_WIDGET_FIELDS.contains(&name.as_str()));
    ordered.truncate(MAX_WIDGET_ROWS);
    ordered
}

/// Java `NodeCanvasPane.java:604-624 inputPorts(record, className)`。
///
/// 只登记「能连线」的字段，与 `widgetFields` 是两个**独立**集合：一张卡可能有一个引用字段
/// 因为类型不在 bool/number/text/sprite 里而不占行，但它仍在端口表里——只是画不出来，
/// 于是 `ContentCard.java:156-160` 的 `inputFields.contains(name)` 决定哪一行左缘有点。
///
/// 分组顺序照 Java `:613-622`：struct 先、stack（Java `FieldType.isStack()`，含 Stack 与
/// StackArray）次、其余最后。**Java 不截断**——真正的数量上限来自卡片：`ContentCard.java:156-160`
/// 只为「同时是 widget 行」的引用字段画点，而行数被 `:572 .limit(8)` 卡住。
/// （`ContentCard.MAX_INPUT_PORTS=8` 在 Java 里没有任何使用点，见 [`MAX_INPUT_PORTS`]。）
///
/// 参数只要 `class_name`：Java 的 `record` 只用来 `classNameOf(record)`（`:608`），这一步在
/// Rust 侧由调用方（`ProjectController::class_name_of`）做完再传进来。
pub fn input_ports(class_name: &str, registry: &SchemaRegistry) -> Vec<String> {
    let mut structs: Vec<String> = Vec::new();
    let mut stacks: Vec<String> = Vec::new();
    let mut others: Vec<String> = Vec::new();
    for field in registry.fields_of(class_name).iter() {
        if !field.is_reference() {
            continue;
        }
        match field.field_type {
            FieldType::Struct => structs.push(field.name.clone()),
            FieldType::Stack | FieldType::StackArray => stacks.push(field.name.clone()),
            _ => others.push(field.name.clone()),
        }
    }
    let mut ordered = structs;
    ordered.extend(stacks);
    ordered.extend(others);
    ordered
}

// --- 卡片几何------------------------------------------------------------------------------------

/// Java `ContentCard.java:101-172` 构造器留下的几何：宽 192、高 `32 + rows*32`、
/// 每个「同时是引用字段的行」在左缘有一个输入端口、头部右缘一个 ID 输出口。
///
/// 端口纵坐标逐字来自 Java：
/// - 输入口 `:161` `y = HEADER_HEIGHT + i * ROW_HEIGHT + ROW_HEIGHT / 2`，锚点 `:164`
///   `(1 + PORT_RADIUS, y)` = `(5, 48 + 32i)`；
/// - ID 口 `:169` 锚点 `(WIDTH - PORT_RADIUS, HEADER_HEIGHT / 2)` = `(188, 16)`。
///
/// Java 把圆点放在 `place(dot, 1, y - PORT_RADIUS)`（`:163`）、ID 口放在
/// `place(id, WIDTH - 2 * PORT_RADIUS, HEADER_HEIGHT / 2 - PORT_RADIUS)`（`:168`），
/// 而锚点是 `(1 + PORT_RADIUS, y)` / `(WIDTH - PORT_RADIUS, HEADER_HEIGHT / 2)`（`:164`/`:169`）。
///
/// 这两组数**不矛盾**：`Circle` 的圆心在局部原点，其 layout bounds 从 `-r` 起，
/// JavaFX 摆放非 managed 子节点时按 **layout bounds 左上角**对齐 layoutX/layoutY，
/// 于是圆心 = `(layoutX + r, layoutY + r)` = 锚点。也就是说 Java 的端口圆点圆心正好落在锚点上，
/// 前端直接按锚点画半径 [`PORT_RADIUS`] 的圆即可，不需要任何补偿。
///
/// （上一轮调研报告称这里「ID 口锚点 (188,16) 但圆心实际 (188,12)，差 4px」——
/// 那是把 JavaFX 的 layout 语义当成局部原点读出的转述错误，本节按实读 `place`/`Port` 两处纠正。）
///
/// 内容卡几何。`class_name` 由调用方按 Java `classNameOf` 解析（`ProjectController.class_name_of`），
/// `display_name` 同理（`ProjectController::display_name`），Java 也是在 UI 层取这两样
/// （`NodeCanvasPane.java:535-539`）。
pub fn content_card(
    record: &ContentRecord,
    class_name: &str,
    display_name: &str,
    registry: &SchemaRegistry,
) -> CardMetrics {
    let widgets = widget_fields(record, class_name, registry);
    let ports_names = input_ports(class_name, registry);
    let mut ports: Vec<CardPort> = Vec::new();
    for (index, name) in widgets.iter().enumerate() {
        if !ports_names.iter().any(|port| port == name) {
            continue;
        }
        let y = HEADER_HEIGHT + (index as f64) * ROW_HEIGHT + ROW_HEIGHT / 2.0;
        ports.push(CardPort {
            field_key: Some(name.clone()),
            // Java `:164` 的 `1 + PORT_RADIUS`：`1` 是圆点 layoutX（左缘压半个点），加半径得圆心。
            x: 1.0 + PORT_RADIUS,
            y,
        });
    }
    ports.push(CardPort {
        field_key: None,
        x: CONTENT_CARD_WIDTH - PORT_RADIUS,
        y: HEADER_HEIGHT / 2.0,
    });
    CardMetrics {
        carrier: false,
        width: CONTENT_CARD_WIDTH,
        height: content_card_height(widgets.len()),
        ports,
        title: card_title(display_name, record.patch),
        color: record.ctype.color().to_string(),
    }
}

/// 卡片高度：Java `ContentCard.java:114-115`。
pub fn content_card_height(rows: usize) -> f64 {
    HEADER_HEIGHT + (rows as f64) * ROW_HEIGHT
}

/// Java `NodeCanvasPane.java:508-511` / `:529-532` 用的补给卡（`CarrierCard.java:34-36` 尺寸、
/// `:105-108` 端口、`:177-184` 对外几何）：260×64，只有一个 ID 输出口在右缘。
///
/// 三条与内容卡不同的口径，逐字照 Java：
/// - 标题**就是** `displayName`：`CarrierCard.java:69 title.setText(displayName)`，
///   既不追加 `"  ·patch"`（那是 `ContentCard.java:130` 的口径），也**没有任何 `◆` 前缀**——
///   Java 的 inline 标记只在 `ContentCard.matches()` 签名里（`:107`/`:375`），从不进文本。
///   补给卡的 displayName 两个来源：绑定记录时 `controller.displayName(record)`（`:503`），
///   直连原版名时 `node.refName == null ? "" : node.refName`（`:523`）；
/// - 副标题位是**类型徽章** `typeBadge`（文本 `controller.typeName(type)`，`:73-74` 与 `:508`），
///   内容卡那边没有可见副标题，`subtitleOf(record)` 只进 Tooltip（`ContentCard.java:171`）；
///   `typeName`/`subtitleOf` 在 Rust 侧仍未移植（M4 已登记明账）；
/// - 端口锚点公式与内容卡**不同形**：`:107-108` 是 `WIDTH - 8 + PORT_RADIUS`（=256），
///   内容卡 `:169` 是 `WIDTH - PORT_RADIUS`（=188）。两者数值都是「圆点圆心」，
///   但 Java 写的是两个不同表达式，这里照抄表达式而不是结果。
pub fn carrier_card(content_type: ContentType, display_name: &str) -> CardMetrics {
    CardMetrics {
        carrier: true,
        width: CARRIER_WIDTH,
        height: CARRIER_HEIGHT,
        ports: vec![CardPort {
            field_key: None,
            x: CARRIER_WIDTH - 8.0 + PORT_RADIUS,
            y: CARRIER_HEIGHT / 2.0,
        }],
        title: display_name.to_string(),
        color: content_type.color().to_string(),
    }
}

/// Java `ContentCard.java:130`：`displayName + (record.patch ? "  ·patch" : "")`。
/// 内容卡**没有** `◆` 前缀：`isInline()` 只参与 `matches()` 的重建判定（`:107`、`:375`），
/// 不改标题文本；补给卡更是连 `·patch` 都没有（`CarrierCard.java:69`）。
pub fn card_title(display_name: &str, patch: bool) -> String {
    if patch {
        format!("{display_name}  ·patch")
    } else {
        display_name.to_string()
    }
}

/// Java `NodeCanvasPane.java:517-522`：无记录节点的类型解析，未知回落 `item`。
/// Java 用 `ContentType.valueOf(...)` 抛 `IllegalArgumentException` 兜住；Rust 用
/// [`ContentType::from_name`] 的 `Option`，语义相同。
pub fn ref_type_or_item(ref_type: Option<&str>) -> ContentType {
    ContentType::from_name(ref_type.unwrap_or("item")).unwrap_or(ContentType::Item)
}

/// Java `ContentCard.java:441 portAnchor(fieldKey)` / `CarrierCard.java:130 portAnchor(fieldKey)`
/// （接口声明在 `CanvasCard.java:27`）。
///
/// 找不到这个端口返回 `None`——于是画布回退到矩形中线端点（`NodeCanvasPane.java:1429-1432`），
/// 该回退在 `src-tauri/src/dto.rs::port_anchor` 里保留。
/// Java 用 `Objects.equals(port.fieldKey, fieldKey)`，所以 `field_key == None` 能匹配 ID 口。
pub fn port_anchor(metrics: &CardMetrics, field_key: Option<&str>) -> Option<[f64; 2]> {
    metrics
        .ports
        .iter()
        .find(|port| port.field_key.as_deref() == field_key)
        .map(|port| [port.x, port.y])
}
