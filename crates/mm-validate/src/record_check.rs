//! `RecordCheck.java`（`dev.modmaker.core.validate.RecordCheck`，223 行、2 个 `@Override`、
//! 5 个私有方法：`validateClass`:91 / `checkBlockSize`:135 / `validateUnitType`:153 /
//! `validateUnitTemplate`:185 / `validateSprites`:205）的 1:1 忠实移植。
//!
//! Java 类注释（`RecordCheck.java:18-21`）直译：逐记录规则 = 身份侧（内联宿主、重名、文件名安全、
//! 文件型别）与类别/贴图侧（声明类、方块尺寸、单位 type/模板、贴图区域）。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. **所有** `Issue.errorOn/warningOn(record.id, ...)`（`:45`、`:58`、`:61`、`:71`、`:79`、`:83`、
//!    `:104`、`:114`、`:119`、`:122`、`:142`、`:160`、`:169`、`:201`、`:220`）⇒
//!    `Issue::new(Level::Error/Warning, key, &[args], record.id.clone())`：Java 首参是可 null 的
//!    `contentId`，本 crate 的便捷构造器收 `impl Into<String>` 交不出 `None`（`unwrap_or_default()`
//!    会把「没有 id」写成「空串 id」= 静默改语义，见 `issue.rs` 模块头）。本 crate 各 Check 一律
//!    只用规范构造器，口径唯一。
//! 2. 消息参数里的枚举（`record.type`、`schema.ctype`）：Java 经 `String.valueOf(Object)` →
//!    默认 `toString()` → 枚举常量名（`ContentType` 的常量本就小写）⇒ `ContentType::name()`；
//!    `record.type.folderName()`（`:84`）⇒ `folder_name()`。`record.type.name()`（`:50`）在 Java 是
//!    枚举的 `name()`（常量名），与 Rust 的 `ContentType::name()`（同为小写枚举名）取值相同。
//! 3. 两个 `LinkedHashMap`（`:38-39`）⇒ [`mm_schema::KeyedMap`]：`put` 返回**上一个**值且**保持
//!    首次插入位置**（`LinkedHashMap.put` 的两个观察性质），`get` 精确键匹配。表里存记录引用 ⇒
//!    `&'a ContentRecord`（`context.project()` 交出 `&'a ModProject`，不挂在 `&self` 上，见
//!    `validation_context.rs` 模块头），故持有这些引用跨 `context.report(&mut)` 与 Java 的
//!    「边遍历边 report」同样成立。
//!    `key.toLowerCase(Locale.ROOT)`（`:68`）⇒ Rust `to_lowercase()`。**登记一处罕见差异**：Java 是
//!    逐字符单码点映射，Rust 是 Unicode **全量小写映射**，个别字符长度会变（U+0130 `İ` →
//!    `"i̇"` 两个码点，Java 给单个 `i`）。该表只用于「同类型同名 / 仅大小写不同」的判定，差异仅可能
//!    出现在含这类字符的内容名上，而这种名字已经先被 `validate.name.invalid`（`:77-81`）判错；
//!    判定方向不变。
//! 4. `project.contentById(record.inlineOwnerId)`（`:44`）⇒
//!    `record.inline_owner_id.as_deref().and_then(|id| project.content_by_id(id))`：Java 的
//!    `contentById(String)` 收到 null 恒返回 null，`as_deref()` 正是这份 null 传播。
//! 5. `Set<String> spriteRegions = context.spriteRegions()`（`:36`）⇒ 一次 `to_vec()`：Rust 里
//!    `sprite_regions()` 返回挂在 `&self` 上的切片，而 `:45` 起要 `&mut context` 上报，两者不能并存。
//!    Java 拿的是同一个 `Set` 引用；本方法对它**只读**（`:217` 的 `regions.isEmpty()`、`:217` 的
//!    `hasSprite`），故克隆没有可观察差异；克隆点仍在 `check()` 开头，惰性计算的触发时机与 Java
//!    同一行（`ValidationContext.java:74-81`）。
//! 6. `Double.sizeValue` 一路：`CheckSupport.number` ⇒ [`crate::check_support::number`]
//!    （`Double` ↔ `Option<f64>`）；`size.intValue()`（`:140`）⇒ [`java_trunc_i32`]（JLS 5.1.3 窄化：
//!    向零截断、NaN→0、超界饱和，Rust `as i32` 同口径）。`:127-134` 的方法注释解释了为什么先截断：
//!    游戏侧 `Block.size` 是 `int`，`"size": 16.9` 在游戏里就是 16 号方块，不是错误。
//!    Java 的 `if (size == null) return;`（`:137-139`）是提前返回，不是嵌套 if，照搬。
//! 7. `String.trim()`（`:192`）⇒ [`java_trim`]：只剥码点 ≤ U+0020 的首尾字符，与 Rust `str::trim()`
//!    的 Unicode 空白集不同口径。`name.indexOf('.') >= 0`（`:197`）⇒ `contains('.')`（字符级判定，
//!    与 UTF-16 下标口径在此等价：`.` 是 ASCII）。
//! 8. `registry.unitTypes()` 作为消息参数（`:170`）：Java 交的是 `List<String>`，`Issue` 用
//!    `String.valueOf(args[i])` ⇒ `AbstractCollection.toString()` 的 `[a, b, c]` 形式
//!    （`issue.rs:45-46` 已登记）⇒ [`java_list_to_string`] 复刻。`isEmpty()`（`:163`）⇒ `is_empty()`。
//! 9. `record.className()` ⇒ [`mm_model::ContentRecord::class_name`]（可 null `String` ↔
//!    `Option<String>`）；`NameRules.contentNameProblem` ⇒ [`mm_schema::name_rules::content_name_problem`]；
//!    `SpriteRegions.needsSprite/expectedRegion/hasSprite` ⇒ [`mm_fmt::sprite`] 同名函数；
//!    `SchemaRegistry.canonicalClassName` ⇒ [`mm_schema::SchemaRegistry::canonical_class_name`]；
//!    `CheckSupport.isEnvironmentClass` ⇒ [`crate::check_support::is_environment_class`]；
//!    `record.type/record.patch/record.isInline()` ⇒ `ctype`/`patch`/`is_inline()`。
//! 10. `validateUnitType` 的 `:158-162` 读的是**原始 `JsonElement`**（`record.fields.get("type")`），
//!     不是 `className()`——注释说明了理由：游戏对对象/数组会抛
//!     "Unit '...' has an incorrect type. Types must be strings."（`ContentParser.java:627-628`），
//!     而 `className()` 只看 primitive，看不见它。故这里判 `Json::Obj | Json::Arr`，且必须排在
//!     `declared == null` 的提前返回**之前**。
//! 11. Java 的 5 个私有方法在这里是 `impl RecordCheck` 的私有方法（`&self` 未使用，与 Java 的实例
//!     方法形状一致）；参数表逐项照搬，包括 `:205-206` 那个冗长的
//!     `(registry, record, internal, regions, context)` 顺序。

use mm_fmt::sprite;
use mm_json::Json;
use mm_model::{ContentRecord, ModProject};
use mm_schema::{name_rules, ContentType, KeyedMap, SchemaRegistry};

use crate::check_support;
use crate::issue::{Arg, Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `MAX_BLOCK_SIZE`（`RecordCheck.java:23-24`）= `Vars.maxBlockSize`：超过它的方块会被
/// 方块解析器直接拒掉（`ContentParser.java:604-605`）。
const MAX_BLOCK_SIZE: i32 = 16;

/// Java `String.trim()`（`RecordCheck.java:192`）：剥掉首尾所有码点 ≤ U+0020 的字符
/// （不是 Unicode 空白集），返回借用切片（`char_indices` 保证切点落在字符边界）。
fn java_trim(text: &str) -> &str {
    let lead = text
        .char_indices()
        .find(|(_, c)| *c > '\u{20}')
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let trail = text
        .char_indices()
        .rev()
        .find(|(_, c)| *c > '\u{20}')
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(lead);
    // 全空白（或空串）时 lead == trail，取空切片
    if lead >= trail {
        return "";
    }
    &text[lead..trail]
}

/// Java `Double.intValue()`（`RecordCheck.java:140`）：窄化转换为 `int`（向零截断、NaN→0、
/// 超界饱和到 `Integer.MAX_VALUE`/`MIN_VALUE`）；Rust `as i32` 自 1.45 起逐值相同。
fn java_trunc_i32(value: f64) -> i32 {
    value as i32
}

/// Java `AbstractCollection.toString()`（`registry.unitTypes()` 走消息参数的
/// `String.valueOf(Object)`，`RecordCheck.java:170`）：`[a, b, c]`，分隔符 `", "`。
fn java_list_to_string(values: &[String]) -> String {
    let mut out = String::with_capacity(2 + values.iter().map(|v| v.len() + 2).sum::<usize>());
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(value);
    }
    out.push(']');
    out
}

/// Java `class RecordCheck implements ProjectCheck`（`RecordCheck.java:21`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct RecordCheck;

impl ProjectCheck for RecordCheck {
    /// Java `id()`（`RecordCheck.java:26-29`）。
    fn id(&self) -> &str {
        "records"
    }

    /// Java `check(ValidationContext)`（`RecordCheck.java:31-89`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :33-35  ModProject project / SchemaRegistry registry / String internal
        let project: &ModProject = context.project();
        let registry: &SchemaRegistry = context.registry();
        let internal: String = context.internal_name();
        // :36  Set<String> spriteRegions = context.spriteRegions();（模块头第 5 条）
        let regions: Vec<String> = context.sprite_regions().to_vec();

        // :38-39  Map<String, ContentRecord> byName / byFoldedName = new LinkedHashMap<>();
        let mut by_name: KeyedMap<&ContentRecord> = KeyedMap::new();
        let mut by_folded_name: KeyedMap<&ContentRecord> = KeyedMap::new();

        // :40  for (ContentRecord record : project.contents)
        for record in &project.contents {
            // :41-49  内联内容没有自己的名字（只是宿主文件里的一个对象）：
            //         重名与文件名规则都不适用，只查宿主是否存在
            if record.is_inline() {
                if record
                    .inline_owner_id
                    .as_deref()
                    .and_then(|id| project.content_by_id(id))
                    .is_none()
                {
                    context.report(Issue::new(
                        Level::Error,
                        "validate.inline.noHost",
                        &[record.ctype.name(), record.name.as_str()],
                        record.id.clone(),
                    ));
                }
                continue;
            }

            // :50-51  String key = record.type.name() + ":" + record.name;
            //         ContentRecord previous = byName.put(key, record);
            let key: String = format!("{}:{}", record.ctype.name(), record.name);
            let previous: Option<&ContentRecord> = by_name.put(key.clone(), record);

            if let Some(previous) = previous {
                // :52-63  游戏只在**新**内容对象被注册两次时才抛（ContentLoader.handleMappableContent）：
                //         补丁复用已定位的内容，新内容以 "<mod>-<name>" 注册，所以牵涉补丁就可以容忍，
                //         只有「两个都是新内容」才会让文件加载失败。
                if !record.patch && !previous.patch {
                    context.report(Issue::new(
                        Level::Error,
                        "validate.name.duplicate",
                        &[record.ctype.name(), record.name.as_str()],
                        record.id.clone(),
                    ));
                } else {
                    context.report(Issue::new(
                        Level::Warning,
                        "validate.name.duplicatePatch",
                        &[record.ctype.name(), record.name.as_str()],
                        record.id.clone(),
                    ));
                }
            } else {
                // :64-76  只差大小写的名字在游戏地图里是不同内容，但在 Windows/macOS 上无法并存为
                //         两个文件：项目文件夹本身会在下一次保存时丢掉其中一条。
                let folded_key: String = key.to_lowercase();
                let folded: Option<&ContentRecord> = by_folded_name.get(&folded_key).copied();
                if let Some(folded) = folded {
                    if folded.name != record.name {
                        context.report(Issue::new(
                            Level::Error,
                            "validate.name.caseCollision",
                            &[
                                record.ctype.name(),
                                folded.name.as_str(),
                                record.name.as_str(),
                            ],
                            record.id.clone(),
                        ));
                    }
                } else {
                    // :73-75  else if (folded == null) byFoldedName.put(foldedKey, record);
                    by_folded_name.put(folded_key, record);
                }
            }

            // :77-81  String problem = NameRules.contentNameProblem(record.name);
            if let Some(problem) = name_rules::content_name_problem(&record.name) {
                context.report(Issue::new(
                    Level::Error,
                    "validate.name.invalid",
                    &[record.ctype.name(), record.name.as_str(), problem.as_str()],
                    record.id.clone(),
                ));
            }

            // :82-85  非文件支撑的类型（bullet/unitCommand/unitStance）不能各成一文件
            if !record.ctype.file_backed() {
                context.report(Issue::new(
                    Level::Error,
                    "validate.type.notFileBacked",
                    &[
                        record.ctype.name(),
                        record.name.as_str(),
                        record.ctype.folder_name(),
                    ],
                    record.id.clone(),
                ));
            }

            // :86-87
            self.validate_class(registry, record, context);
            self.validate_sprites(registry, record, &internal, &regions, context);
        }
    }
}

impl RecordCheck {
    /// Java `private void validateClass(SchemaRegistry, ContentRecord, ValidationContext)`
    /// （`RecordCheck.java:91-125`）。
    fn validate_class(
        &self,
        registry: &SchemaRegistry,
        record: &ContentRecord,
        context: &mut ValidationContext<'_>,
    ) {
        // :93-95  block 先查尺寸（与后续类名规则互不排斥：Java 就在这里无条件调用）
        if record.ctype == ContentType::Block {
            self.check_block_size(record, context);
        }
        // :96  String declared = record.className();
        let declared: Option<String> = record.class_name();
        // :97-101  单位另走两条规则并**直接 return**（不再查通用类名）
        if record.ctype == ContentType::Unit {
            self.validate_unit_type(registry, record, declared.as_deref(), context);
            self.validate_unit_template(registry, record, context);
            return;
        }
        // :102-108  declared == null：只有「该类型本身没有缺省类」（`defaultClass()` 为空串，
        //           如 weather）才算错误；消息里第 1、3 个参数在 Java 都是 `record.type`
        let Some(declared) = declared else {
            if record.ctype.default_class().is_empty() {
                context.report(Issue::new(
                    Level::Error,
                    "validate.class.missingType",
                    &[record.ctype.name(), record.name.as_str(), record.ctype.name()],
                    record.id.clone(),
                ));
            }
            return;
        };

        // :109-112  像游戏那样先规范化，好让写了 "beamDrill" 的模组解析成 BeamDrill，
        //           而不是被报成未知类
        let simple: String = SchemaRegistry::canonical_class_name(&declared);
        // :112  ClassSchema schema = registry.schema(simple);
        let Some(schema) = registry.schema(&simple) else {
            // :113-117  未知类 ⇒ warning（第 3 个参数是**原始声明串** declared，不是 simple）
            context.report(Issue::new(
                Level::Warning,
                "validate.class.unknown",
                &[record.ctype.name(), record.name.as_str(), declared.as_str()],
                record.id.clone(),
            ));
            return;
        };

        // :118-120  补丁方块带上 type 毫无意义：游戏定位到既有内容后并不重新构造它
        if record.patch && record.ctype == ContentType::Block {
            context.report(Issue::new(
                Level::Warning,
                "validate.block.patchWithType",
                &[record.name.as_str()],
                record.id.clone(),
            ));
        }
        // :121-124  非补丁记录的 schema 类型与记录类型不一致
        //           Java 读字段 `schema.ctype`（可 null ⇒ 跳过），Rust 是 `ClassSchema::ctype()`
        if !record.patch
            && let Some(schema_ctype) = schema.ctype()
            && schema_ctype != record.ctype
        {
            context.report(Issue::new(
                Level::Warning,
                "validate.class.typeMismatch",
                &[
                    record.ctype.name(),
                    record.name.as_str(),
                    declared.as_str(),
                    schema_ctype.name(),
                ],
                record.id.clone(),
            ));
        }
    }

    /// Java `private void checkBlockSize(ContentRecord, ValidationContext)`
    /// （`RecordCheck.java:135-145`），连同 `:127-134` 的注释所述语义（模块头第 6 条）。
    fn check_block_size(&self, record: &ContentRecord, context: &mut ValidationContext<'_>) {
        // :136-139  Double size = CheckSupport.number(record.fields.get("size"));
        //           if (size == null) return;
        let Some(size) = check_support::number(record.fields.get("size")) else {
            return;
        };
        // :140  int declared = size.intValue();
        let declared: i32 = java_trunc_i32(size);
        // :141-144  declared > MAX_BLOCK_SIZE ⇒ error validate.block.tooLarge
        if declared > MAX_BLOCK_SIZE {
            // Java 交的是 `record.name, declared, MAX_BLOCK_SIZE`，后两个是 `int` ⇒ [`Arg::Int`]，
            // `--json` 报告里写成 JSON 数字而不是串（对应模块头与 `issue.rs` 的 `Arg` 文档）。
            context.report(Issue::new(
                Level::Error,
                "validate.block.tooLarge",
                &[
                    Arg::text(record.name.as_str()),
                    Arg::int(declared as i64),
                    Arg::int(MAX_BLOCK_SIZE as i64),
                ],
                record.id.clone(),
            ));
        }
    }

    /// Java `private void validateUnitType(SchemaRegistry, ContentRecord, String, ValidationContext)`
    /// （`RecordCheck.java:153-171`）。方法注释（`:147-152`）：单位的 `type` 不是类名，而是这个单位
    /// 生成的实体，取自游戏 `ContentParser.unitType` 里一个封闭集合并对其它值抛异常——所以类名写在
    /// 这里是硬错误，而不是回落到原始 JSON；不写没问题（游戏默认普通的飞行实体）。
    fn validate_unit_type(
        &self,
        registry: &SchemaRegistry,
        record: &ContentRecord,
        declared: Option<&str>,
        context: &mut ValidationContext<'_>,
    ) {
        // :158-162  游戏对对象/数组会抛 "Unit '...' has an incorrect type. Types must be strings."
        //           （ContentParser.java:627-628），而 className() 看不见它们（只读 primitive）
        //           ⇒ 必须排在 declared == null 的提前返回之前（模块头第 10 条）
        if let Some(Json::Obj(_) | Json::Arr(_)) = record.fields.get("type") {
            context.report(Issue::new(
                Level::Error,
                "validate.unit.typeNotString",
                &[record.name.as_str()],
                record.id.clone(),
            ));
            return;
        }
        // :163-165  declared == null（游戏用默认实体）或注册表里根本没有单位类型 ⇒ 无法判定
        let Some(declared) = declared else {
            return;
        };
        if registry.unit_types().is_empty() {
            return;
        }
        // :166-168  是已知单位类型 ⇒ 合法（Java 交的是原始 declared，不做规范化）
        if registry.is_unit_type(declared) {
            return;
        }
        // :169-170  error，第 3 个参数是整个单位类型清单
        let unit_types_text: String = java_list_to_string(registry.unit_types());
        context.report(Issue::new(
            Level::Error,
            "validate.unit.badType",
            &[record.name.as_str(), declared, unit_types_text.as_str()],
            record.id.clone(),
        ));
    }

    /// Java `private void validateUnitTemplate(SchemaRegistry, ContentRecord, ValidationContext)`
    /// （`RecordCheck.java:185-202`）。方法注释（`:173-184`）：单位的类来自 `template`
    /// （ContentParser.java:622）；未知名字会让游戏的 `resolve` 记一条
    /// "No type ... found, defaulting to UnitType" 并构造普通类（ContentParser.java:1454），
    /// 于是模板构造函数里的默认值（TankUnitType 的 speed/rotateSpeed、MissileUnitType 的十几项）
    /// 被静默丢掉——这值得一条 warning，而不是 error。template 不改变有哪些字段，故字段集仍读
    /// UnitType：游戏自带的每个模板类（ErekirUnitType、TankUnitType、MissileUnitType、
    /// NeoplasmUnitType）都不自带字段，只在构造函数里赋继承来的 UnitType 字段。
    fn validate_unit_template(
        &self,
        registry: &SchemaRegistry,
        record: &ContentRecord,
        context: &mut ValidationContext<'_>,
    ) {
        // :187-191  template 缺失 / 非 primitive / 非字符串 ⇒ 不判
        let Some(Json::Str(raw)) = record.fields.get("template") else {
            return;
        };
        // :192  String name = template.getAsString().trim();
        let name: &str = java_trim(raw.as_str());
        // :193-200  带点的名是游戏按名解析的全限定类名，是否存在只有加载后才知道 ⇒ 沉默不猜；
        //           与 validateClass 同样先规范化（游戏在查类表前把首字母大写，
        //           ContentParser.java:1438），所以 "tankUnitType" 能解析。
        if name.is_empty()
            || name.contains('.')
            || registry
                .class_names()
                .iter()
                .any(|class| *class == SchemaRegistry::canonical_class_name(name))
        {
            return;
        }
        // :201  warning validate.unit.badTemplate
        context.report(Issue::new(
            Level::Warning,
            "validate.unit.badTemplate",
            &[record.name.as_str(), name],
            record.id.clone(),
        ));
    }

    /// Java `private void validateSprites(SchemaRegistry, ContentRecord, String, Set<String>,
    /// ValidationContext)`（`RecordCheck.java:205-222`）。注释（`:204`）：方块与单位需要贴图，
    /// 物品、液体以及其它都不需要。
    fn validate_sprites(
        &self,
        registry: &SchemaRegistry,
        record: &ContentRecord,
        internal: &str,
        regions: &[String],
        context: &mut ValidationContext<'_>,
    ) {
        // :207-209  if (!SpriteRegions.needsSprite(record)) return;
        if !sprite::needs_sprite(record) {
            return;
        }
        // :210-215  environment 类的贴图由 EnvironmentSpriteCheck 负责（它认识地块约定与
        //           地图编辑器读取它们的目录）
        let class_name: Option<String> =
            registry.resolve_class(record.ctype, record.class_name().as_deref());
        if check_support::is_environment_class(registry, class_name.as_deref()) {
            return;
        }
        // :216  String region = SpriteRegions.expectedRegion(record, internal);
        let region: String = sprite::expected_region(record, internal);
        // :217-219  regions 为空 ⇒ 加载器什么都没加载出来，交给下面的 noneLoaded 语义？
        //           照 Java：`if (regions.isEmpty() || hasSprite(...)) return;`
        if regions.is_empty() || sprite::has_sprite(regions, &region) {
            return;
        }
        // :220-221  六个名字参数的分工照 Java 原样：第 3 个是**路径形态**（带 mod 前缀的完整区域名），
        //           后三个是**裸名**（编辑器里可直接输入，前缀由游戏自动补）
        context.report(Issue::new(
            Level::Warning,
            "validate.sprite.missing",
            &[
                record.ctype.name(),
                record.name.as_str(),
                region.as_str(),
                record.name.as_str(),
                record.name.as_str(),
                record.name.as_str(),
            ],
            record.id.clone(),
        ));
    }
}
