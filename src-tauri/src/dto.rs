//! 壳的 DTO 层：领域类型 → 前端能吃的 JSON。
//!
//! ## 为什么要有这一层（而不是给 `crates/**` 的类型加 `Serialize`）
//!
//! 2026-11-14 实测：整个 `crates/**` 里 `serde` **0 命中**——M1～M3 签的字是「领域层零序列化依赖」。
//! 把 `#[derive(Serialize)]` 直接贴到 `ContentRecord`／`Issue` 上会同时破坏两件事：
//! ① 领域 crate 被序列化框架绑住；② 前端契约与内部结构体字段名焊死，以后改名就是 breaking change。
//! 所以这里建一层只在本目录存在的 DTO：`Serialize + Clone`，字段一律 `rename_all = "camelCase"`。
//!
//! ## 分层纪律：Rust 下发**规则**，Vue 实测**几何**（2026-11-14 口径变更）
//!
//! 方案文档 §三 D5 原写「字段规则、字节契约、图算法一律由 Rust 下发，前端只画不算」，M4 照此把
//! 边点列／bezier 控制点／箭头角／包围盒都算好再发。**2026-11-14 M5 开工时由用户覆盖为两条**
//! （逐字：「排版几何交给 Vue 实测」「点列算法复制到前端」），本文件随之收敛，登记如下：
//!
//! - **退出的计算**（改由 `src-vue/lib/edgePath.js`／`minimapMath.js` 做；Rust 侧
//!   `mm_layout::edge_path_math`／`minimap_math` **不删**，留作 JS 实现的**黄金参考**，
//!   由 `tools/golden_geometry.mjs` + `crates/mm-layout/tests/golden_geometry.rs` 钉住两侧
//!   同一批输入逐点对等）：边的 `start`/`end`/`points`/`curves`/`arrowAngle` 与画布 `bounds`
//!   全部从契约里删掉。理由不只是 IPC 次数：端口锚点是**卡片排版产物**
//!   （`ContentCard.java:156-169`），排版交给 Vue 实测之后只有前端知道锚点，Rust 再算点列
//!   就必须先拿到实测结果，成了倒置依赖。
//! - **留下的计算**：
//!   - 卡片**规则**：哪几行进卡片、哪些字段可连线、标题文本、类型色，见 [`CardDto`]，取自
//!     `mm_app::card_geometry`（Java `NodeCanvasPane.java:500-555 buildCard()` 的分支与
//!     `:558-573 widgetFields()`／`:604-624 inputPorts()`）——这套规则要同时读 schema 和
//!     记录值，前端两样都没有，只能由这里下发；
//!   - issue 的可读文本：走 [`mm_validate::Issue::message()`]，与 Java `--json` 报告同一口径，
//!     前端不解释 `key`/`args`。
//!
//! ## world bounds 的一句就地更正（M4 在此处的论断不完整）
//!
//! M4 在本段写过「Java 侧**没有**现成 helper（`worldBounds` 一族在 `NodeCanvasPane.java` 里
//! 不存在）」。**前半句仍成立**（确实没有叫 `worldBounds` 的方法），但**据此下的结论错了**：
//! Java 在 `NodeCanvasPane.java:962-973 rebuildMinimapBlocks()` 里就地累加同一个包围盒，累加的
//! 是**卡片**矩形（`:967-968` `card.node().getLayoutX() + card.cardWidth()`），不是节点模型里的
//! `w`/`h`；空画布走 `:970-972` 落回 `[0, 0, 1, 1]`。M4 那个按 `node.w/h` 取并集、空画板返回
//! 全零的 `world_bounds()` 因此既不对齐 Java 也用不上，M5-D 整段删除（不保留为参考实现——
//! 黄金参考的职责在 `mm_layout::minimap_math::world_bounds`，那边已按 Java 口径重写过并过了对账门）。
//!
//! ## 数字为什么要以文本承载
//!
//! [`JsonDto::Text`] 同时装 `Str`／`Num`／`Bool` 三种标量，取的是 Java
//! `JsonPrimitive.getAsString()` 的口径（见 `mm_json::Json::get_as_string` 的文档块）：
//! 游戏保留 `0.50`、`1e9`、超出 `i64` 的字面写法，任何 `f64` 往返都会改写源文。
//! 前端因此**永远看不到**"被规范化过的数字"，M6 的字段编辑器直接把这段文本放进输入框。
//! 对象用 `Vec<(String, JsonDto)>` 序列化：serde 把二元组序列渲染成 JSON object，
//! 顺序即 `mm_json::Object` 的插入顺序——这对应 Java `LinkedHashMap`（`ContentRecord.java` 模块头），
//! 也是为什么不直接借用 `serde_json::Value`（默认后端 `BTreeMap` 会把键排序）。

use serde::{Deserialize, Serialize};

use mm_app::{card_geometry, inspector};
use mm_io::export::build_pipeline::Plan;
use mm_io::export::export_report::ExportReport;
use mm_io::import::progress::Stage;
use mm_io::import::report::ImportReport;
use mm_json::Json;
use mm_layout::edge_path_math::Style;
use mm_model::{Board, CanvasEdge, CanvasNode, ContentRecord, ModProject, Viewport};
use mm_schema::{ContentType, FieldDef, FieldType, KeyedMap, SchemaRegistry};
use mm_validate::{Arg, Issue, Level};

/// 一棵 JSON 值的前端表示：`Null` 出 `null`，`Text` 出字符串，`Array` 出数组，
/// `Object` 出**真正的 JSON 对象**（保插入序）。
///
/// > **2026-11-20 就地更正**：`Object` 原先靠 `#[serde(untagged)]` + `Vec<(String, _)>`
/// > 「自然」序列化，注释写着「serde 把二元组序列渲染成 JSON object」——**实测不成立**：
/// > `Vec<(K, V)>` 序列化成 `[["k","v"],…]` 这种**二元组数组**。顶层 `fields` 是有意为之
/// > （前端 `typedFields` 就按 `for (const [k, v] of pairs)` 消费），但**嵌套对象**
/// > （`mesh`、`research`、`objectives`）因此变成数组，前端 `JSON.parse` 出来是数组，
/// > `mesh.color1` 一类读取全部落空。现在 `Object` 手写 `SerializeMap`，输出真正的对象；
/// > 顶层二元组表不动，前端契约不受影响。
#[derive(Clone)]
pub enum JsonDto {
    /// `Json::Null`。
    Null,
    /// 标量：`Str` 的原文、`Num` 的**源文书写**、`Bool` 的 `true`/`false`。
    Text(String),
    Array(Vec<JsonDto>),
    /// 插入序成员表；手写 `SerializeMap` 保证键序与 [`mm_json::Object`] 一致。
    Object(Vec<(String, JsonDto)>),
}

impl Serialize for JsonDto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            JsonDto::Null => serializer.serialize_unit(),
            JsonDto::Text(text) => serializer.serialize_str(text),
            JsonDto::Array(items) => items.serialize(serializer),
            JsonDto::Object(pairs) => {
                let mut map = serializer.serialize_map(Some(pairs.len()))?;
                for (key, value) in pairs {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

/// [`mm_json::Json`] → [`JsonDto`]。
pub fn json_dto(value: &Json) -> JsonDto {
    match value {
        Json::Null => JsonDto::Null, // Json::Bool(text) 与 Json::Num(digits) 都走 get_as_string：
        Json::Bool(_) | Json::Num(_) => JsonDto::Text(value.get_as_string()),
        Json::Str(text) => JsonDto::Text(text.clone()),
        Json::Arr(items) => JsonDto::Array(items.iter().map(json_dto).collect()),
        Json::Obj(object) => JsonDto::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), json_dto(item)))
                .collect(),
        ),
    }
}

/// 一条内容的有序字段表。
fn field_pairs(record: &ContentRecord) -> Vec<(String, JsonDto)> {
    record
        .fields
        .iter()
        .map(|(key, value)| (key.clone(), json_dto(value)))
        .collect()
}

/// `ContentRecord` 的下发形态。对应 Java `onSelectionChanged(Consumer<ContentRecord>)` 的载荷
/// （`ProjectController.java:1844-1846` → `selection-changed`）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecordDto {
    pub id: Option<String>,
    /// [`ContentType::name()`]，即 Java `ctype` 字面量（`"item"`、`"unitCommand"`…）。
    pub content_type: String,
    pub name: String,
    /// **有效类**（2026-11-26，产品评审 R-02）：声明 `type` > 脚本 `extend` 推断 > 类型默认类。
    ///
    /// 移植前这一格是内容文件里的显式 `type`（`None` = 没写）。现在改了语义：真实模组的方块
    /// 115/184 不写 `type`、类写在脚本里，前端只拿到 `None` 时只能显示 `Block` 的错误字段集。
    /// 来源见 [`RecordDto::class_source`]；磁盘上的 `type` 仍只经 `record_schema` 的字段树与
    /// `record_set_field` 读写——这里下发的是**视图口径**，不是要写回的字节。
    pub class_name: String,
    /// 类名来源（camelCase 下发）：`"declared"` / `"script"` / `"default"`。
    pub class_source: String,
    pub patch: bool,
    pub inline: bool,
    pub dirty: bool,
    pub parse_failed: bool,
    pub raw_path: Option<String>,
    pub fields: Vec<(String, JsonDto)>,
}

/// [`ContentRecord`] → [`RecordDto`]。
///
/// 类名与来源由调用方（`bridge::selection_payload`）问
/// [`ProjectController::class_name_and_source_of`](mm_app::project_controller::ProjectController::class_name_and_source_of)
/// 再传进来；本层不自己解析（规则只在 `mm_app::class_resolution` 一份）。
pub fn record_dto(record: &ContentRecord, class_name: &str, class_source: &str) -> RecordDto {
    RecordDto {
        id: record.id.clone(),
        content_type: record.ctype.name().to_string(),
        name: record.name.clone(),
        class_name: class_name.to_string(),
        class_source: class_source.to_string(),
        patch: record.patch,
        inline: record.is_inline(),
        dirty: record.dirty,
        parse_failed: record.parse_failed,
        raw_path: record.raw_path.clone(),
        fields: field_pairs(record),
    }
}

/// 选中项变化的事件载荷。`None` 是**合法值**：Java `NodeCanvasPane.java:1189`、`:1383`
/// 都走 `controller.select(null)` 取消选择，`SelectionModel.java:26-29` 无条件广播，
/// 所以前端必须能收到一条"什么都没选中"的 `selection-changed`。
#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum SelectionDto {
    Selected(RecordDto),
    None,
}

/// 一个画布节点。字段与 Java `CanvasNode.java` 一一对应；`w`/`h` 是**模型里存的**宽高，
/// M4 实测它们不是卡片的排版尺寸（`ContentCard.java:45 WIDTH=192`、`CarrierCard.java:34 WIDTH=260`、
/// `io/JsonCanvas.java:130-131`、`AutoLayout.java:75` 各有各的来源），卡片排版归 M5。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    pub id: Option<String>,
    pub kind: String,
    pub content_id: Option<String>,
    pub ref_name: Option<String>,
    pub ref_type: Option<String>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    pub color: String,
}

fn node_dto(node: &CanvasNode) -> NodeDto {
    NodeDto {
        id: node.id.clone(),
        kind: node.kind.clone(),
        content_id: node.content_id.clone(),
        ref_name: node.ref_name.clone(),
        ref_type: node.ref_type.clone(),
        x: node.x,
        y: node.y,
        w: node.w,
        h: node.h,
        text: node.text.clone(),
        color: node.color.clone(),
    }
}

/// 一条边：**身份 + 端口标识 + 样式键 + 中转点数据**。几何一律不在这里（模块头 2026-11-14 口径）。
///
/// M4 起这里还发过 `start`/`end`/`points`/`curves`/`arrowAngle` 五个字段，本日全部退出契约，
/// 前端改从 `src-vue/lib/edgePath.js` 自算。为了把「前端该怎么还原」钉死在 Java 原文上，
/// `NodeCanvasPane.java:628-664 refreshEdges()` 的四条实况逐字登记在这里：
///
/// - `:633` 样式取自 `board.edgeStyle`——**整个画板一个样式**，不是每条边各一份。本快照的
///   [`BoardSnapshotDto::edge_style`] 就是它，[`EdgeDto::style`] 是同值冗余，保留只为前端少查一次；
/// - `:635` 起点恒为 `portWorld(edge.from, null)`：**源端永远走 ID 输出口**，与 `edge.fieldKey` 无关；
/// - `:636` 终点是 `portWorld(edge.to, edge.fieldKey)`——只有目标端才拿 `fieldKey` 去定位输入口；
/// - `:637-639` 那句 `continue` 的判据是「这条边的 `Path` 节点在不在 `curvesByEdge` 里」，
///   **不是**「端点卡片在不在」。Java 的 `portWorld` 在卡片缺失时返回 `Point2D.ZERO`
///   （`:1426`），边**照画**，两个端点落在世界原点上。
///   （M4 在本文件写过「`portWorld` 取不到卡时会跳过该边，`:632-636`」——**那是错的**，
///   本轮就地更正；`edge_dto` 也因此不再是 `Option`：Rust 侧不做任何跳过。）
///
/// 余下两条也是前端的事，一并登记：`:649-652` 标签落在 `(start + end) / 2` 再减 `(20, 9)`；
/// `:658-664` 箭头尖端从终点沿 `arrowAngle` 反推 7px、旋转角取 `Math.toDegrees(angle)`。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EdgeDto {
    pub id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// 目标端要接的那个字段。`null` 对应 Java `edge.fieldKey == null`，此时终点也走 ID 口
    /// （`portWorld(edge.to, null)` 命中 [`CardDto`] 端口表里那个 `fieldKey == null` 的口）。
    pub field_key: Option<String>,
    pub label: String,
    /// [`Style::key()`]：`"bezier"` / `"orthogonal"` / `"straight"`。
    pub style: String,
    /// 用户拖过的 waypoint 原值（世界坐标）。这是**数据**不是几何：它来自 `.canvas` 文件
    /// （Java `CanvasEdge.reroutes`）、参与点列算法与命中测试，所以留在契约里。
    pub reroutes: Vec<[f64; 2]>,
}

/// 一条边的下发形态：只搬身份与 `reroutes` 数据。
///
/// 签名里**没有** `nodes`：M4 那版要在这里查端点节点再算锚点，锚点既已交出去，这次查找就是纯
/// 开销。返回也不是 `Option`——Java 那边没有「端点节点找不到就不发这条边」的分支，见
/// [`EdgeDto`] 文档里 `:637-639`／`:1426` 两条实况。
pub fn edge_dto(edge: &CanvasEdge, style: Style) -> EdgeDto {
    EdgeDto {
        id: edge.id.clone(),
        from: edge.from.clone(),
        to: edge.to.clone(),
        field_key: edge.field_key.clone(),
        label: edge.label.clone(),
        style: style.key().to_string(),
        reroutes: edge.reroutes.clone(),
    }
}

/// 卡片的一行：字段名 + 已按 Java 规则换算的显示文本（M7b）。
///
/// 换算走 [`mm_app::inspector::display_text`]——与检查器输入框初值同一份规则
/// （`ContentCard` 的各控件读侧），前端只画不算。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CardRowDto {
    pub name: String,
    pub text: String,
}

fn card_rows(
    record: &ContentRecord,
    class_name: &str,
    names: &[String],
    registry: &SchemaRegistry,
) -> Vec<CardRowDto> {
    let fields = registry.fields_of(class_name);
    names
        .iter()
        .map(|name| {
            let text = fields
                .iter()
                .find(|field| field.name == *name)
                .map(|field| inspector::display_text(field.field_type, record.fields.get(name)))
                .unwrap_or_default();
            CardRowDto { name: name.clone(), text }
        })
        .collect()
}

/// 一张卡片该画成什么样：只发**规则**，不发宽高、不发端口坐标（2026-11-14 口径）。
///
/// 对应 Java `NodeCanvasPane.java:500-555 buildCard()` 的判定树里那三样非几何信息：
/// 卡片类别、标题文本、头部色条，外加 `:529-531`／`:547-550` 传给卡构造器的两张字段表。
///
/// ## 前端怎么把两张表还原成端口（逐字照 Java `ContentCard.java:156-169`）
///
/// 1. 按 `widget_fields` 的**顺序**出行，下标即行号（Java `:146` `i * ROW_HEIGHT`）；
/// 2. 命中 `input_fields` 的那些行，左缘各一个输入口，`fieldKey` = 该字段名（`:158-165`：
///    判据是 `inputFields.contains(field.name())`，所以**引用字段若不占行就没有点**）；
/// 3. 全表之后追加唯一那个 ID 输出口，`fieldKey = null`（`:167-169`）；
/// 4. `carrier == true` 时只有第 3 步那一个口（Java `:502`/`:515` 走 `CarrierCard`，
///    `CarrierCard.java:105-108` 只建 ID 口）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CardDto {
    /// 归属节点的 id。Java 那张表就叫 `cardsByNode`（`:116`），键是节点 id。
    pub node: Option<String>,
    /// `true` = 补给卡（item/liquid 记录，或无记录的直连原版节点）。
    pub carrier: bool,
    /// 标题行文本。内容卡 = `displayName`，patch 记录再追加 `"  ·patch"`
    /// （Java `ContentCard.java:130`）；补给卡 = `displayName` 原样，**既不追加 patch 标记
    /// 也不写 ` ·inline`**（`CarrierCard.java:69`）。
    pub title: String,
    /// 头部色条，来自 [`ContentType::color()`]。Java 把它写成内联样式（`ContentCard.java:125`），
    /// 所以这里是色值字符串而不是 CSS 类名。
    pub color: String,
    /// 进卡片的行，**顺序即行号**。Java `NodeCanvasPane.java:558-573 widgetFields()`。
    pub widget_fields: Vec<String>,
    /// 与 `widget_fields` 同序的显示文本（M7b）：卡片行右侧控件的当前值，
    /// 由 [`inspector::display_text`] 换算（与检查器同一份规则）。补给卡为空。
    pub rows: Vec<CardRowDto>,
    /// 可连线的引用字段。Java `:604-624 inputPorts()`。与上一张是**两个独立集合**：
    /// 引用字段可能被 MAX_WIDGET_ROWS 截掉，或本身是非编辑字段而只出现在这里，
    /// 也可能像 `requiredPowerItems` 那样只进上一张表。
    pub input_fields: Vec<String>,
    /// 卡片副标题 = 内容卡的悬停提示（Java `ContentCard.java:171` 把 `subtitleOf` 装进 Tooltip，
    /// `NodeCanvasPane.java:539` 取的就是这个值）。**只有内容卡那一支有值**，两支补给卡都是
    /// `None`——Java 的 `CarrierCard.java:102` 在重绑下拉上挂的是另一条静态消息
    /// （`carrier.pick.hint`），而重绑命令面还没接（无 `carrierCandidates`/rebind 通道），
    /// 那条提示随 M6 一起补，这里不提前下发。
    /// 口径偏差一条：Java 按界面语言查 bundle，Rust 固定英文底账，见 `type_name` 文档。
    pub subtitle: Option<String>,
    /// 补给卡上的类型徽章文本（Java `NodeCanvasPane.java:508`/`:529` 传进
    /// `CarrierCard.java:73-75` 的那个 `typeLabel`）。**只有补给卡那一支有值**：
    /// 内容卡没有独立的类型行，类型信息全在 [`CardDto::subtitle`] 里。
    pub type_label: Option<String>,
}

/// 建一张卡的规则。Java `buildCard()`（`:500-555`）的三个分支逐条对齐：
///
/// 1. `:502-513` 记录是 item/liquid → 补给卡，标题用 `controller.displayName(record)`；
/// 2. `:515-533` 没有记录 → 也是补给卡（直连原版名），标题用 `node.refName`，
///    类型取 `node.refType`、未知名回落 `item`（`:517-522` 那个 for 循环）；
/// 3. `:535-554` 其余记录 → 内容卡，标题走 patch 追加。
///
/// `display_name`/`class_name`/`subtitle`/`type_label` 由调用方（`bridge::card_table`）用
/// controller 解析好传进来：那几个值要读 `ProjectController` 的类名覆盖表与注册表
/// （Java `subtitleOf` `:2301-2309`、`typeName` `:2296-2298`），本层不碰 controller，
/// 免得把「壳怎么问 controller」这件事散到 DTO 里。
/// 调用方对**有记录的节点两样都算**（各一次字符串拼接），这里按分支各取一样装进
/// [`CardDto`]——分支判定以本函数为准，调用方不必复制 `is_carrier_record` 那一步。
///
/// Java `:353-355` 那条 `record == null && refName == null` 就**不建卡**的跳过**不在这里**——
/// 那不是卡片规则，是「这个节点不该进 `cardsByNode`」，归调用方过滤。
// 8 个入参是调用方（bridge::card_table）按 Java 口径解析好的显示值与缓存，逐一传参正是
// 为了让「壳怎么问 controller」不散进 DTO 层（见上注），不打包成结构体。
#[allow(clippy::too_many_arguments)]
pub fn card_dto(
    node: &CanvasNode,
    record: Option<&ContentRecord>,
    display_name: &str,
    class_name: &str,
    subtitle: Option<&str>,
    type_label: Option<&str>,
    registry: &SchemaRegistry,
    ports_cache: &mut KeyedMap<Vec<String>>,
) -> CardDto {
    match record {
        Some(record) if card_geometry::is_carrier_record(record) => CardDto {
            node: node.id.clone(),
            carrier: true,
            title: display_name.to_string(),
            color: record.ctype.color().to_string(),
            // 补给卡没有字段行：Java `CarrierCard` 的构造参数里没有 widget 表。
            widget_fields: Vec::new(),
            rows: Vec::new(),
            input_fields: Vec::new(),
            // Java `:508`：补给卡只把 `typeName` 交给徽章；`subtitleOf` 是内容卡分支的
            // `:539` 才算的，这里不带上（见 [`CardDto::subtitle`]）。
            subtitle: None,
            type_label: type_label.map(str::to_string),
        },
        Some(record) => {
            let widget_fields = card_geometry::widget_fields(record, class_name, registry);
            let rows = card_rows(record, class_name, &widget_fields, registry);
            CardDto {
                node: node.id.clone(),
                carrier: false,
                title: card_geometry::card_title(display_name, record.patch),
                color: record.ctype.color().to_string(),
                // 两张表本来就是**有序的名字列表**（`Vec<String>`），与 Java `:536 widgetNames`
                // 和 `:537 ports` 喂给卡构造器的东西同形，直接搬。
                widget_fields,
                rows,
                // input_ports 只依赖类名（schema 进程内不变）：一次快照里同类节点共享同一份
                // 计算结果，不再逐节点重算。缓存由调用方持有（一次快照一个）。
                input_fields: match ports_cache.get(class_name) {
                    Some(ports) => ports.clone(),
                    None => {
                        let ports = card_geometry::input_ports(class_name, registry);
                        ports_cache.put(class_name.to_string(), ports.clone());
                        ports
                    }
                },
                // Java `:539` 的 subtitle → `ContentCard.java:171` 的 Tooltip。
                subtitle: subtitle.map(str::to_string),
                type_label: None,
            }
        }
        None => {
            // Java `:515-533`：无记录节点一律画补给卡。这里 `display_name` 就是调用方按
            // Java `:523` 的 `node.refName == null ? "" : node.refName` 传进来的值
            // （`CarrierCard.java:69` 的 `text = title` 不做任何追加）。
            let ctype = card_geometry::ref_type_or_item(node.ref_type.as_deref());
            CardDto {
                node: node.id.clone(),
                carrier: true,
                title: display_name.to_string(),
                color: ctype.color().to_string(),
                widget_fields: Vec::new(),
                rows: Vec::new(),
                input_fields: Vec::new(),
                subtitle: None,
                // Java `:529` 的 `controller.typeName(type)`，type 即上面回落过的 ctype，
                // 调用方按同一张 `ref_type_or_item` 算出后传进来。
                type_label: type_label.map(str::to_string),
            }
        }
    }
}

/// 视口（Java `Board.Viewport`）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ViewportDto {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

fn viewport_dto(viewport: &Viewport) -> ViewportDto {
    ViewportDto {
        x: viewport.x,
        y: viewport.y,
        zoom: viewport.zoom,
    }
}

/// **入参**方向唯一的 DTO：卡片拖动落定时前端送回来的一次移动。
///
/// 为什么只有它是 `Deserialize`（本文件其余全是 `Serialize`）：几何交出去之后
/// （模块头口径变更），前端是唯一知道「卡片被拖到哪儿」的一方，坐标必须**回流**；
/// 而坐标一旦回流就必须进撤销栈，否则拖动不可撤销、保存也拿不到新位置。
/// 三条 reroute 命令同理收坐标，只是它们单点单条、直接摊平成参数就够了，
/// 拖动是多卡并行（Java `dragTo` 一次搬整组选中卡），所以要带 id 成表。
///
/// 坐标一律**世界单位**，与快照里 `NodeDto.x/y`、`EdgeDto.reroutes` 同一坐标系。
/// `before` 是 `pointerdown` 那一刻的实测起点、`after` 是落点（Java 用字段
/// `dragPositions` 存前者，`NodeCanvasPane.java:137` 那一片）。
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeMoveDto {
    pub id: String,
    pub before: [f64; 2],
    pub after: [f64; 2],
}

/// 画板下拉里的一项。`current` 用**布尔**表达「这是不是当前画板」：
/// Java `ProjectController` 没有 `currentBoardIndex()`（M4 实测），Rust 侧 `board` 字段
/// 也没有 getter，所以这里不伪造索引，按计划 M4 定的口径只下发布尔标记。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BoardRefDto {
    pub index: usize,
    pub name: String,
    pub file: String,
    pub current: bool,
}

/// `board_snapshot` 的返回体：画板清单 + 当前画板的节点、**卡片规则**、边与该项目的 issues。
///
/// 与 Java 的对应关系（`NodeCanvasPane.java:306-346 refresh()` 一次刷新的那几张表）：
/// `nodes` ≈ `nextNodes`，`cards` ≈ `cardsByNode`（`:116`），`edges` ≈ `curvesByEdge` 的键集，
/// `edgeStyle` ≈ `:633` 读的 `board.edgeStyle`。几何不在这里，见模块头口径变更。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BoardSnapshotDto {
    pub boards: Vec<BoardRefDto>,
    /// 本次快照对应的画板索引；无项目或无画板时为 `null`。
    pub board: Option<usize>,
    pub nodes: Vec<NodeDto>,
    /// 与 `nodes` **同序**的卡片规则；没有卡片的节点（Java `:353-355` 那两条跳过）位置留 `None`。
    /// 用「同序 + Option」而不是「按 id 查表」：前端已经在 `nodes` 里遍历，省一次查找，
    /// 也让 Rust 侧不必建 `HashMap`（本 crate 的禁词）。
    pub cards: Vec<Option<CardDto>>,
    pub edges: Vec<EdgeDto>,
    pub edge_style: String,
    pub viewport: Option<ViewportDto>,
    pub issues: Vec<IssueDto>,
}

/// 组装画板快照。`board` 为 `None`（没有打开项目／项目里没有画板）时返回空数组，
/// 前端据此画空画布，而不是报错。
///
/// `cards` 由调用方（`bridge::card_table`）产出——那要问 controller 拿 `displayName`／
/// `class_name_of`／注册表，本层不碰 controller（理由见 [`card_dto`] 的文档）。
/// 这里只按 `board.nodes` 的顺序 zip 进去，顺序由调用方保证：两边都是同一张
/// `board.nodes` 切片 `iter()` 出来的。
pub fn board_snapshot_dto(
    project: Option<&ModProject>,
    board: Option<&Board>,
    board_index: Option<usize>,
    style: Style,
    issues: &[Issue],
    cards: Vec<Option<CardDto>>,
) -> BoardSnapshotDto {
    let boards: Vec<BoardRefDto> = project
        .map(|project| {
            project
                .boards
                .iter()
                .enumerate()
                .map(|(index, item)| BoardRefDto {
                    index,
                    name: item.name.clone(),
                    file: item.file.clone(),
                    current: board_index == Some(index),
                })
                .collect()
        })
        .unwrap_or_default();
    let (nodes, edges, edge_style, viewport) = match board {
        Some(board) => (
            board.nodes.iter().map(node_dto).collect(),
            board
                .edges
                .iter()
                .map(|edge| edge_dto(edge, style))
                .collect(),
            // 边的样式取自画板 sidecar；`Style::parse` 对 null/空白/未知值回落 bezier，
            // 与 Java `EdgePathMath.Style.parse`（`:30-42`）一致。
            style.key().to_string(),
            Some(viewport_dto(&board.viewport)),
        ),
        None => (
            Vec::new(),
            Vec::new(),
            Style::Bezier.key().to_string(),
            None,
        ),
    };
    BoardSnapshotDto {
        boards,
        board: board_index,
        nodes,
        cards,
        edges,
        edge_style,
        viewport,
        issues: issues.iter().map(issue_dto).collect(),
    }
}

/// 一条校验结论。`message` 由 Rust 渲染（Java `Issue.message()`，`modmaker.properties` 的
/// 英文 bundle），`args` 保留 `Object` 的取值域，与 Java `--json` 报告的字节口径相同。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IssueDto {
    /// `"error"` / `"warning"`（Java `Issue.Level` 的小写常量名）。
    pub level: String,
    pub key: String,
    pub args: Vec<ArgDto>,
    pub content_id: Option<String>,
    pub message: String,
}

/// Java `List<Object>` 里 `Object` 的四支。
#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum ArgDto {
    Null,
    Int(i64),
    Bool(bool),
    Text(String),
}

fn arg_dto(arg: &Arg) -> ArgDto {
    match arg {
        Arg::Null => ArgDto::Null,
        Arg::Int(value) => ArgDto::Int(*value),
        Arg::Bool(value) => ArgDto::Bool(*value),
        Arg::Text(value) => ArgDto::Text(value.clone()),
    }
}

fn level_key(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
    }
}

fn issue_dto(issue: &Issue) -> IssueDto {
    IssueDto {
        level: level_key(issue.level()).to_string(),
        key: issue.key().to_string(),
        args: issue.args().iter().map(arg_dto).collect(),
        content_id: issue.content_id().map(|id| id.to_string()),
        message: issue.message(),
    }
}

/// 把结论摊平成下发用的列表。
pub fn issue_dto_vec(issues: &[Issue]) -> Vec<IssueDto> {
    issues.iter().map(issue_dto).collect()
}

/// 一个字段的下发描述符。
///
/// M6 起这里除了**规则**（`control`/`enumValues`/`contentCtype`…）还带上了**当前值的显示形态**
/// （`text`/`checked`/`swatch`）与候选名单（`candidates`）。这不是把领域逻辑漏给前端，正好相反：
/// Java 在 `editorRow()` 构造控件时就把值算成了控件初值（`RecordInspector.java:389-406`），
/// 换算规则（`mm_app::inspector`）留在 Rust，前端只把 `text` 塞进输入框、把 `candidates`
/// 塞进下拉表，**自己不做任何文本 ↔ 值的判断**。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpecDto {
    pub name: String,
    /// 控件种类的 DTO 键，供 M6 的 `<component :is>` 映射。由 [`FieldType`] 枚举**变体名**
    /// 派生，不是 schema 原始拼写：`"stack[]"` 同时对应 `Stack`/`StackArray` 两支里的后者，
    /// 而 `TextArray` 的原始拼写也是 `"text[]"`，所以不能用原始拼写当键。
    pub control: String,
    pub description: Option<String>,
    pub enum_values: Vec<String>,
    pub content_ctype: Option<String>,
    pub stack_kind: Option<String>,
    pub default_value: Option<JsonDto>,
    pub curated: bool,
    /// 这一行是否进 fields 段的行表。Java `:310-314` 把 `type`／`name`／`description`／
    /// `details`／`credit` 五项 `continue` 掉（它们各有专属段落），摘除的是**行**，
    /// 描述符本身仍然下发，供前端在别的段落（P6 递延的 identity/localisation）取用。
    pub shown: bool,
    /// 输入框初值文本。对应 Java 各编辑器构造时的初值（`:389-406` 读侧），
    /// 由 [`mm_app::inspector::display_text`] 算出：数字格是 `numberOf(v)`、
    /// 数组格是逗号拼接、堆格是 `item/amount`、raw 格是 JSON 文本。
    pub text: String,
    /// 勾选框状态。Java `boolEditor` `:561` 的 `value.getAsBoolean()`；只对 `bool` 控件有意义。
    pub checked: bool,
    /// 色块颜色，`#rrggbb`。Java `colorEditor` `:566-573` 取值 + `:577-580` 写回；
    /// 取不到合法十六进制时是 `"#ffffff"`（Java 的 `Color.WHITE` 兜底）。只对 `color` 控件有意义。
    pub swatch: String,
    /// 下拉候选。Java `contentEditor` `:620` 的 `referenceCandidates(definition)`
    /// （`:677-697`：本项目内容在前、原版名在后，各类型限 120、总数去重后限 400）。
    /// **只有 `content` 控件才填**——`:620` 是 `referenceCandidates` 在 Java 里的唯一调用点；
    /// `enumeration` 的候选另有 `enum_values`（Java `:397` 那一臂传进 `enumEditor`，在 `:605`
    /// `box.getItems().addAll(definition.enumValues())`；**2026-11-16 P5-M6 就地更正**：这里原先
    /// 写 `:395`，那是 `case bool` 那一行）。
    pub candidates: Vec<String>,
}

fn field_spec_dto(
    field: &FieldDef,
    value: Option<&Json>,
    candidates: Vec<String>,
) -> FieldSpecDto {
    FieldSpecDto {
        name: field.name.clone(),
        control: control_key(field.field_type).to_string(),
        description: field.description.clone(),
        enum_values: field.enum_values.clone(),
        content_ctype: field.content_ctype.clone(),
        stack_kind: field.stack_kind.clone(),
        default_value: field.default_value.as_ref().map(json_dto),
        curated: field.curated,
        shown: !inspector::hidden_in_fields_block(&field.name),
        text: inspector::display_text(field.field_type, value),
        checked: inspector::bool_checked(value),
        swatch: inspector::color_swatch(value),
        candidates,
    }
}

fn control_key(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::Number => "number",
        FieldType::Text => "text",
        FieldType::TextArray => "textArray",
        FieldType::Bool => "bool",
        FieldType::Color => "color",
        FieldType::Enumeration => "enumeration",
        FieldType::Content => "content",
        FieldType::Sprite => "sprite",
        FieldType::Stack => "stack",
        FieldType::StackArray => "stackArray",
        FieldType::Struct => "struct",
        FieldType::Raw => "raw",
    }
}

/// `record_schema` 的返回体：类名 + 字段描述符 + 当前值。
///
/// **不含 `required`**：M4 实测 Java `RecordInspector.java` 全文没有 required 概念，
/// Rust/Java 两侧都没有 `requiredFields`，所以这里不伪造必填标记（方案文档 §六 M6 那句
/// 「必填……来自 fields.json」按实测更正，登记在 M4 签字条目）。
///
/// 类名从哪儿来：由调用方（`bridge::record_schema`）问 `controller.class_name_of(record)`
/// 再传进来，本层不自己解析。
///
/// M4 曾在本文件写过一条私有 `class_of`（「显式 `type` 优先、缺省回落 `default_class`、
/// 空串按取不到处理」），并留了一条「与 Java 是否逐支一致，M4 未核对，归 M6 一起核」的欠账。
/// M6 实测**那条欠账是真的**，原实现三条口径都不对齐 Java：
/// - [`mm_schema::SchemaRegistry::resolve_class`]（`SchemaRegistry.java:340-353`）里 `:341-347`
///   规定 **unit 的 `type` 是移动方式（`"flying"`…）不是类名**，永远取 `default_class`、
///   声明值一律忽略——原实现会把 `type: "flying"` 拿去查字段表；
/// - `:348-350` 非 unit 的声明值要过 `canonicalClassName()` 规整成全限定类名——原实现拿原始
///   拼写直接查，`"Draft1"` 与 `"mindustry.content.units.Draft1"` 会走成两条结果；
/// - `ProjectController.classNameOf`（`:2284-2287`）在 `resolveClass` 返回 `null` 时回落
///   `record.type.name()`，所以 Java 的类名**一定非空**；查不到字段是 `fieldsOf` 出空表，
///   对应 Java `RecordInspector.java:300-303` 的「没有模式」提示。
///
/// 这三条已经由 [`crate::ProjectController::class_name_of`] 实现过一份（M2 移植时对齐
/// `:2284-2287`），按硬约束第 6 条「规则只在 Rust 单份实现」，本层**删除**重复的 `class_of`，
/// 改收调用方给的结果；`class_name` 那一格因此由 `Option<String>` 收敛为 `String`。
/// 同一层已有的 [`card_dto`] 早就是这个形状（`:301` 收 `class_name: &str`），此处与之对齐。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecordSchemaDto {
    pub record_id: Option<String>,
    pub content_type: String,
    /// 用来取字段表的类名，即 Java `classNameOf(record)` 的结果。Java 那边一定非空，
    /// 所以这里没有 `Option`；空字段表是「这个类没有模式」，不是「类名取不到」。
    ///
    /// 2026-11-26（R-02）：`classNameOf` 改成有效类（声明 > 脚本推断 > 默认），所以这一格
    /// 现在也可能是脚本推断出的子类；来源见 [`RecordSchemaDto::class_source`]。
    pub class_name: String,
    /// 类名来源：`"declared"` / `"script"` / `"default"`。检视器据此决定类名那一行是否可编辑、
    /// 要不要标「由脚本推断」。
    pub class_source: String,
    pub fields: Vec<FieldSpecDto>,
    pub values: Vec<(String, JsonDto)>,
}

/// 组一张记录的模式描述符。
///
/// `project` 只为 `content` 控件的候选名单而传（Java `referenceCandidates` `:680-689` 要读
/// `project().contents`）；壳未就绪时传 `None`，候选表出空——Java 那时整个面板都建不出来。
pub fn record_schema_dto(
    record: &ContentRecord,
    registry: &SchemaRegistry,
    class_name: &str,
    class_source: &str,
    project: Option<&ModProject>,
) -> RecordSchemaDto {
    let fields = registry
        .fields_of(class_name)
        .iter()
        .map(|field| {
            // Java `:620` 只有 `contentEditor` 会取候选，别的控件那一格留空数组。
            let candidates = match field.field_type {
                FieldType::Content => match project {
                    Some(project) => inspector::reference_candidates(project, registry, field),
                    None => Vec::new(),
                },
                _ => Vec::new(),
            };
            field_spec_dto(field, record.fields.get(&field.name), candidates)
        })
        .collect();
    RecordSchemaDto {
        record_id: record.id.clone(),
        content_type: record.ctype.name().to_string(),
        class_name: class_name.to_string(),
        class_source: class_source.to_string(),
        fields,
        values: field_pairs(record),
    }
}

/// 撤销/重做状态。对应 Java `canUndo()`/`canRedo()`/`isDirty()` 三个查询
/// （`ProjectController.java:1504-1509`、`:518`）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStateDto {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_depth: usize,
    pub dirty: bool,
}

/// 项目概况。对应 Java 窗口标题与侧栏要的那几项。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummaryDto {
    pub display_name: String,
    pub internal_name: String,
    pub root: Option<String>,
    pub content_count: usize,
    pub board_count: usize,
    pub dirty: bool,
    pub locales: Vec<String>,
    pub schema_profile: String,
    pub imported_from: String,
    pub authoring_warning: Option<String>,
    /// 2026-11-19 追加（additive，不破坏既有消费方）：首页元数据对话框只读展示。
    pub meta: MetaDto,
}

/// [`ModProject`] + `controller.is_dirty()` → [`ProjectSummaryDto`]；没有项目时字段取空值。
pub fn project_summary_dto(project: Option<&ModProject>, dirty: bool) -> ProjectSummaryDto {
    match project {
        Some(project) => ProjectSummaryDto {
            display_name: project.display_name(),
            internal_name: project.internal_name(),
            root: project
                .root
                .as_ref()
                .map(|root| root.to_string_lossy().into_owned()),
            content_count: project.contents.len(),
            board_count: project.boards.len(),
            dirty,
            locales: project.locales.clone(),
            schema_profile: project.schema_profile.clone(),
            imported_from: project.imported_from.clone(),
            authoring_warning: project.authoring_warning.clone(),
            meta: meta_dto(&project.meta),
        },
        None => ProjectSummaryDto {
            display_name: String::new(),
            internal_name: String::new(),
            root: None,
            content_count: 0,
            board_count: 0,
            dirty: false,
            locales: Vec::new(),
            schema_profile: String::new(),
            imported_from: String::new(),
            authoring_warning: None,
            meta: meta_dto(&mm_model::ModMeta::default()),
        },
    }
}

/// 一次构建计划的摘要。字段名对齐 Java `BuildPipeline.java:64-70` 的 `Plan`。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanDto {
    pub has_errors: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub root: String,
    pub internal_name: String,
    pub issues: Vec<IssueDto>,
}

pub fn build_plan_dto(plan: &Plan) -> BuildPlanDto {
    BuildPlanDto {
        has_errors: plan.has_errors(),
        error_count: plan.error_count(),
        warning_count: plan.warning_count(),
        root: plan.root.to_string_lossy().into_owned(),
        internal_name: plan.internal_name.clone(),
        issues: issue_dto_vec(&plan.issues),
    }
}

/// 一次写盘（目录或 zip）的摘要。`summary` 直接取 Java `ExportReport.summary()` 的拼串
/// （已签的 `export_report.rs:62-79`），前端不重新拼。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportReportDto {
    pub has_errors: bool,
    /// 逐字取 [`ExportReport`] 自己的计数口径（Java `errorCount()`/`warningCount()` 返回 `long`，
    /// 已签的 `export_report.rs:53-60` 落成 `i64`），不在壳里换算。
    pub error_count: i64,
    pub warning_count: i64,
    pub summary: String,
}

pub fn export_report_dto(report: &ExportReport) -> ExportReportDto {
    ExportReportDto {
        has_errors: report.has_errors(),
        error_count: report.error_count(),
        warning_count: report.warning_count(),
        summary: report.summary(),
    }
}

/// 一次导入的摘要，`project_import` 的返回值。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportReportDto {
    pub source: String,
    pub summary: String,
    pub content_count: i32,
    pub inline_count: i32,
    pub sprite_count: i32,
    pub asset_count: i32,
    pub bundle_count: i32,
    pub string_count: i32,
    pub edge_count: i32,
    pub patch_count: i32,
    pub warnings: Vec<String>,
    /// 解析失败的内容文件：路径 → 原始文本。键序即 Java `LinkedHashMap` 插入序
    /// （Rust 侧由 `KeyedMap` 承接，`report.rs:20-22`）。
    pub parse_failures: Vec<(String, String)>,
    pub skipped: Vec<String>,
}

pub fn import_report_dto(report: &ImportReport) -> ImportReportDto {
    ImportReportDto {
        source: report.source.clone(),
        summary: report.summary(),
        content_count: report.content_count,
        inline_count: report.inline_count,
        sprite_count: report.sprite_count,
        asset_count: report.asset_count,
        bundle_count: report.bundle_count,
        string_count: report.string_count,
        edge_count: report.edge_count,
        patch_count: report.patch_count,
        warnings: report.warnings.clone(),
        parse_failures: report
            .parse_failures
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        skipped: report.skipped.clone(),
    }
}

/// `import-progress` 事件的载荷。`stage` 用 [`Stage`] 变体的小写名
/// （`reading` / `building` / `writing`），对应 Java `ImportProgress.Stage` 的三个常量。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgressDto {
    pub stage: String,
    pub done: i32,
    pub total: i32,
}

pub fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::Reading => "reading",
        Stage::Building => "building",
        Stage::Writing => "writing",
    }
}

/// 边样式：把 DTO 里的字符串解析回 [`Style`]。未知值/空串回落 bezier，
/// 与 Java `Style.parse`（`edge_path_math.rs:46-57`）同一个函数，不另写规则。
pub fn parse_style(value: Option<&str>) -> Style {
    Style::parse(value)
}

/// 供 `bridge`/`lib` 复用：把内容类型字面量解析成 [`ContentType`]。
/// 与 Java `ContentType.valueOf` 一样只接受精确枚举名（`content_type.rs:67`）。
pub fn parse_content_type(name: &str) -> Option<ContentType> {
    ContentType::from_name(name)
}

// ===== 2026-11-19 资源管理首页接线追加（命令面 21 → 34 的 DTO 侧）=====
// 理由登记在 lib.rs 模块头更正块：首页（ItemsPane/ContextPanel/StringsPane 的 IPC 等价物）
// 需要记录清单、新建、patch、raw 读写、字符串表、建项目、示例项目与游戏会话通道；
// 领域层方法（add_content/set_patch/apply_raw_tree/set_string/create_project/game_launcher）
// M2/M3 早已移植，这里只补下发形状，零新增第三方依赖。

/// 资源列表的一行。Java `ItemsPane` 直接读活记录，IPC 下只发网格要的那几样：
/// 身份 + 显示名 + 状态标记 + 色块。字段树不在这里——选中记录的字段走
/// `selection-changed` 的 [`RecordDto`]，逐记录拉全量字段是 O(内容数×字段数) 的浪费。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecordListItemDto {
    pub id: Option<String>,
    pub content_type: String,
    pub name: String,
    /// `controller.display_name(record)`（Java `ItemsPane` 卡片标题同源）。
    pub display_name: String,
    pub patch: bool,
    pub inline: bool,
    pub dirty: bool,
    /// 网格色块：`inspector::color_swatch(fields.color)`，与检查器色块同一口径；
    /// 取不到合法 hex 时回落 `#ffffff`（Java 卡片缺精灵时的类型色块由前端另算）。
    pub color: String,
}

pub fn record_list_item_dto(record: &ContentRecord, display_name: &str) -> RecordListItemDto {
    RecordListItemDto {
        id: record.id.clone(),
        content_type: record.ctype.name().to_string(),
        name: record.name.clone(),
        display_name: display_name.to_string(),
        patch: record.patch,
        inline: record.is_inline(),
        dirty: record.dirty,
        color: inspector::color_swatch(record.fields.get("color")),
    }
}

/// 字符串表快照（Java `StringsPane` 的可编辑表格）。`entries` 只含管理键
/// （`StringTable.values`）；`extra` 的非管理键 Java 那边只读，M6 递延，先不下发。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StringsDto {
    pub locales: Vec<String>,
    /// key -> (locale -> value)，两层都保插入序（`KeyedMap` 迭代序）。
    pub entries: Vec<(String, Vec<(String, String)>)>,
}

pub fn strings_dto(project: Option<&ModProject>) -> StringsDto {
    match project {
        Some(project) => StringsDto {
            locales: project.locales.clone(),
            entries: project
                .strings
                .values
                .iter()
                .map(|(key, by_locale)| {
                    (
                        key.clone(),
                        by_locale.iter().map(|(locale, value)| (locale.clone(), value.clone())).collect(),
                    )
                })
                .collect(),
        },
        None => StringsDto { locales: Vec::new(), entries: Vec::new() },
    }
}

/// 游戏会话状态（Java AppShell 的 `gameSession` 三问：在不在、目录在哪、产物在哪）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameSessionDto {
    pub running: bool,
    pub data_dir: Option<String>,
    pub mod_folder: Option<String>,
}

/// 项目元数据的只读下发（首页「项目元数据」对话框）。编辑通道 Java 走
/// `ProjectInfoDialog` → 控制器 meta 更新，Rust 控制器尚无对应 setter，M6 递延。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MetaDto {
    pub name: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub min_game_version: String,
}

pub fn meta_dto(meta: &mm_model::ModMeta) -> MetaDto {
    MetaDto {
        name: meta.name.clone(),
        display_name: meta.display_name.clone(),
        author: meta.author.clone(),
        description: meta.description.clone(),
        version: meta.version.clone(),
        min_game_version: meta.min_game_version.clone(),
    }
}

// ===== 2026-11-20 M7a：科技森林与批量记录详情（命令面 34 → 36，登记见 lib.rs 模块头）=====
// 科技树工作区原先逐记录拉 record_schema（真实 mod 265 次 IPC）再在前端自算森林；
// 森林构建规则（mm_io::canvas::tech_tree_index，Java TechTreeIndex 的等价移植）早已在领域层，
// 这里只补下发形状。research 摘要由 mm_fmt::tech::read 归一成 "item/amount" 文本表，
// 前端不再解析 research 的 JSON 文本（M6「规则只在 Rust 单份实现」同一口径）。

/// research 字段的归一化下发（[`mm_fmt::tech::Node`] 的前端形状）。
/// `reqs` 是游戏接受 `ItemStack` 的 `"item/amount"` 文本表（ContentParser.java:466）；
/// `objectives` 保留原始对象，未知键经过一次编辑原样存活。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResearchDto {
    pub parent: Option<String>,
    pub root: bool,
    pub name: Option<String>,
    pub requires_unlock: bool,
    pub planet: Option<String>,
    pub reqs: Vec<String>,
    pub objectives: Vec<JsonDto>,
}

pub fn research_dto(node: &mm_fmt::tech::Node) -> ResearchDto {
    ResearchDto {
        parent: node.parent.clone(),
        root: node.root,
        name: node.name.clone(),
        requires_unlock: node.requires_unlock,
        planet: node.planet.clone(),
        reqs: node.requirements.clone(),
        objectives: node
            .objectives
            .iter()
            .map(|objective| json_dto(&Json::Obj(objective.clone())))
            .collect(),
    }
}

/// 科技树里的一个记录节点摘要（森林条目内嵌；unassigned/refused 列表同形）。
/// 字段树不在这里——选中记录的完整字段仍走 selection 通道与 `record_schema`。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TechNodeDto {
    pub id: Option<String>,
    pub content_type: String,
    pub name: String,
    /// `controller.display_name(record)`，与资源列表同一口径。
    pub display_name: String,
    pub patch: bool,
    /// 网格色块口径（`inspector::color_swatch(fields.color)`）。
    pub color: String,
    /// 记录不带 research 字段时（unassigned）为 `None`。
    pub research: Option<ResearchDto>,
}

pub fn tech_node_dto(
    record: &ContentRecord,
    display_name: &str,
    research: Option<ResearchDto>,
) -> TechNodeDto {
    TechNodeDto {
        id: record.id.clone(),
        content_type: record.ctype.name().to_string(),
        name: record.name.clone(),
        display_name: display_name.to_string(),
        patch: record.patch,
        color: inspector::color_swatch(record.fields.get("color")),
        research,
    }
}

/// 森林的一个条目（[`mm_io::canvas::tech_tree_index::Entry`] 的前端形状）。
/// `record` 为 `None` = 模组只是指向的原版组节点（Java `Entry.contentId == null`）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TechEntryDto {
    pub record: Option<TechNodeDto>,
    /// 记录名；原版组节点 = 小写原版名。端口拖拽改父级拿它当 parent 值。
    pub key: String,
    pub vanilla: bool,
    /// `null` / `"unresolvedParent"` / `"ignored"` / `"patchRefused"`（[`problem_key`]）。
    pub problem: Option<String>,
    pub children: Vec<TechEntryDto>,
}

/// `tech_forest` 的返回体（Java `TechTreeIndex.Forest` 的前端形状）。
/// unresolved/ignored 条目同时出现在 `roots` 里（领域层口径：它们的子节点仍挂在下面），
/// 这里不再单列两份——前端按 `problem` 键过滤即得，与 Java `problemEntries` 同构。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TechForestDto {
    pub roots: Vec<TechEntryDto>,
    /// 不带 research 字段的记录，按名字排序（领域层已排）。
    pub unassigned: Vec<TechNodeDto>,
    /// 带 research 的 patch：游戏拒绝它们改科技树（ContentParser.java:1338-1341）。
    pub refused: Vec<TechNodeDto>,
}

/// [`mm_io::canvas::tech_tree_index::Problem`] → 前端问题键。
pub fn problem_key(problem: mm_io::canvas::tech_tree_index::Problem) -> Option<&'static str> {
    use mm_io::canvas::tech_tree_index::Problem;
    match problem {
        Problem::None => None,
        Problem::Ignored => Some("ignored"),
        Problem::UnresolvedParent => Some("unresolvedParent"),
        Problem::PatchRefused => Some("patchRefused"),
    }
}

// ===== 2026-11-20 编辑器基线：目录浏览 / 贴图缩略图（命令面 36 → 41）=====
// 前端已按此契约编码，字段与命令名不得擅改。目录浏览是纯外壳能力（领域层没有「文件对话框」），
// 贴图解析的**规则**（区域命名与家族匹配）仍只有一份：`mm_fmt::sprite`。

/// 目录浏览的一行。`is_mod` 只看当前层（含 `mod.json`/`mod.hjson` 的目录），
/// `is_archive` 认 `.zip`/`.jar`（大小写不敏感）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FsEntryDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_mod: bool,
    pub is_archive: bool,
}

/// `fs_list_dir` 的返回体：当前目录、父目录（根上为 `null`）、条目（目录在前，各自按名字不区分大小写排序）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FsListingDto {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FsEntryDto>,
}

/// 一条记录的贴图缩略图。`data_url` 是可以直接塞进 `<img src>` 的 `data:image/png;base64,…`，
/// `region` 是命中的图集区域名（`mm_fmt::sprite` 的命名规则），供前端做提示与去重。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpriteDto {
    pub data_url: String,
    pub region: String,
}

// ===== 2026-11-21 星球战役场景（`cosmos_scene`）=====
// 字段名与形状按 `annotation/20 面板/星球战役渲染契约.md` 逐字落地：渲染语义（Canvas 投影、
// 着色、大气逐像素）在 Vue，Rust 只产数据。`corners` 一律是扁平 x,y,z 三元组。

/// `cosmos_scene(recordId)` 的返回体。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosSceneDto {
    pub record_id: String,
    /// `"planet"` / `"sector"`（扇区会解析到所属星球，但 kind 仍是 sector）。
    pub kind: String,
    /// 0 = 无网格。
    pub sector_size: i32,
    pub radius: f32,
    pub grid: Option<CosmosGridDto>,
    /// 无网格（`sectorSize <= 0`）时为 `null`，前端显示「无网格」。
    pub appearance: Option<CosmosAppearanceDto>,
    pub system: CosmosSystemDto,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosGridDto {
    pub size: i32,
    pub capacity: i32,
    pub tiles: Vec<CosmosGridTileDto>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosGridTileDto {
    pub index: usize,
    /// 单位球面 tile 中心。
    pub center: [f64; 3],
    /// 单位球面角点（扁平）。
    pub corners: Vec<f64>,
    pub occupied: bool,
    pub sector_ids: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosAppearanceDto {
    pub mesh_type: String,
    pub surface: Vec<CosmosLayerDto>,
    pub clouds: Vec<CosmosLayerDto>,
    /// Java `lightFromSystem()` 的单位向量。
    pub light: [f64; 3],
    pub atmosphere: CosmosAtmosphereDto,
    pub max_extent: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosLayerDto {
    pub radius: f32,
    pub intensity: f32,
    pub unlit: bool,
    pub sky: bool,
    pub cloud_speed: f32,
    pub tiles: Vec<CosmosTileDto>,
}

/// 一个网格 tile：`color` 为 0..1 的 RGBA，`null` = 不可画。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosTileDto {
    pub corners: Vec<f64>,
    pub color: Option<[f32; 4]>,
    pub skipped: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosAtmosphereDto {
    /// `#rrggbb`。
    pub color: String,
    pub rad_out: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosSystemDto {
    pub bodies: Vec<CosmosBodyDto>,
    pub sun_colors: [String; 4],
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CosmosBodyDto {
    pub name: String,
    pub radius: f32,
    pub orbit_radius: f32,
    pub color: Option<String>,
    pub selected: bool,
    pub vanilla: bool,
    pub star: bool,
}

/// `mm_app::cosmos::CosmosScene` → 前端契约。纯映射，不改任何数值。
pub fn cosmos_scene_dto(scene: &mm_app::cosmos::CosmosScene) -> CosmosSceneDto {
    let grid = scene.grid.as_ref().map(|grid| CosmosGridDto {
        size: grid.size,
        capacity: grid.capacity,
        tiles: grid
            .tiles
            .iter()
            .map(|tile| CosmosGridTileDto {
                index: tile.index,
                center: tile.center,
                corners: tile.corners.clone(),
                occupied: tile.occupied,
                sector_ids: tile.sector_ids.clone(),
            })
            .collect(),
    });
    let appearance = scene.appearance.as_ref().map(|appearance| CosmosAppearanceDto {
        mesh_type: appearance.mesh_type.clone(),
        surface: appearance.surface.iter().map(cosmos_layer_dto).collect(),
        clouds: appearance.clouds.iter().map(cosmos_layer_dto).collect(),
        light: appearance.light,
        atmosphere: CosmosAtmosphereDto {
            color: appearance.atmosphere.color.clone(),
            rad_out: appearance.atmosphere.rad_out,
        },
        max_extent: appearance.max_extent,
    });
    CosmosSceneDto {
        record_id: scene.record_id.clone(),
        kind: scene.kind.key().to_string(),
        sector_size: scene.sector_size,
        radius: scene.radius,
        grid,
        appearance,
        system: CosmosSystemDto {
            bodies: scene
                .system
                .bodies
                .iter()
                .map(|body| CosmosBodyDto {
                    name: body.name.clone(),
                    radius: body.radius,
                    orbit_radius: body.orbit_radius,
                    color: body.color.clone(),
                    selected: body.selected,
                    vanilla: body.vanilla,
                    star: body.star,
                })
                .collect(),
            sun_colors: scene.system.sun_colors.clone(),
        },
    }
}

fn cosmos_layer_dto(layer: &mm_app::cosmos::CosmosLayer) -> CosmosLayerDto {
    CosmosLayerDto {
        radius: layer.radius,
        intensity: layer.intensity,
        unlit: layer.unlit,
        sky: layer.sky,
        cloud_speed: layer.cloud_speed,
        tiles: layer
            .tiles
            .iter()
            .map(|tile| CosmosTileDto {
                corners: tile.corners.clone(),
                color: tile.color,
                skipped: tile.skipped,
            })
            .collect(),
    }
}
