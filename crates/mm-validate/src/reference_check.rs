//! `ReferenceCheck.java`（254 行）的等价移植：引用完整性——记录引用的每个名字都必须能解析到
//! 本模组内容或游戏自带内容；而一个「悄悄绑到游戏自带内容、而不是模组自己的同名内容」的裸名字
//! 也要报出来。
//!
//! 类注释（`ReferenceCheck.java:14-17`）直译：引用完整性——记录引用的每个名字必须解析到模组内容
//! 或原版内容，且一个悄悄绑定到原版而非模组自有内容的裸名字会被报告。
//!
//! ## 与 Java 的语言差异（不改变任何断言语义）
//! 1. **所有** `Issue.warningOn/errorOn(record.id, ...)`（`:53`、`:56`、`:60`）⇒
//!    `Issue::new(Level::Warning/Error, key, &[args], record.id.clone())`：Java 首参是可 null 的
//!    `contentId`，本 crate 的便捷构造器交不出 `None`（口径见 `issue.rs` 模块头）。本 crate 各
//!    Check 一律只用规范构造器。
//! 2. 消息参数里的 `record.type`（`:54`、`:57`、`:61`）：Java 经 `String.valueOf(Object)` → 枚举
//!    常量名 ⇒ [`ContentType::name()`]（小写枚举名，与 `ContentType.java` 的常量拼写一致）。
//! 3. `field.isJsonPrimitive() && field.getAsJsonPrimitive().isString()`（`:213`）与
//!    `!value.getAsJsonPrimitive().isString()`（`:239`）⇒ [`Json::is_string()`]：mm_json 的
//!    `Str` 变体本身就是「primitive 且是字符串」，两步判定合一。`:239` 处 Java 能安全调
//!    `getAsJsonPrimitive()` 是因为 `:197-238` 已排除了 null、数组、对象，只剩 primitive；Rust 的
//!    `if !value.is_string() { return; }` 对 `Bool`/`Num` 走同一分支，等价。
//! 4. `String.matches("item|liquid|payload|block|unit|content")`（`:232`）：`matches` 是**整串**
//!    匹配，而这条正则就是 6 个字面量的交替，故等价于「是否等于这 6 个名字之一」⇒ 复用
//!    [`NAMED_KEYS`]（即 `:211` 那个数组）做精确比较，不引入正则依赖。两者取同一个键集合，
//!    与 Java 里两处字面量必须手写的现状相比是**收敛**（少一次拼写不一致的可能）。
//! 5. `text.indexOf('/')` / `text.substring(0, slash)`（`:220-222`、`:247-249`）：Java 交的是
//!    UTF-16 码元下标，Rust `find('/')` 交的是字节下标。`/` 是 ASCII，两个下标都恰好指向同一个
//!    `/` 字符的起始位置，切出的前缀逐字符相同；`slash > 0` 的判定在两套下标下同真同假
//!    （`/` 在串首时两者都是 0）。
//! 6. `String.trim()`（`:214`、`:242`）⇒ [`java_trim`]：只剥码点 ≤ U+20 的首尾字符，与 Rust
//!    `str::trim()` 的 Unicode 空白集不同口径（口径论证见 `record_check.rs` 模块头第 7 条）。
//! 7. `CheckSupport.stripPrefix(name, internal)` ⇒ [`check_support::strip_prefix`]：它返回入参的
//!    借用（`&'a str`），所以裸名可以直接往下传，无需拷贝。
//! 8. `typeMatches(FieldDef field, ContentType)`（`:185-188`）是 Java `static`，其 `field == null`
//!    一支在本类三个调用点（`:76`、`:120`、`:177`）全部传入 fieldsOf 里取出的非空字段，不可达；
//!    Rust 因此收 `&FieldDef`，Java 的 null 判定不再需要写出，判定方向不变。`vanillaFirst`
//!    （`:152`）同为 `static`，同样保持自由函数形状。其余私有方法是 Java 实例方法 ⇒
//!    `impl ReferenceCheck` 上的 `&self` 方法（与 `record_check.rs` 的口径一致）。
//! 9. `registry.fieldsOf(className)` 返回**所有**继承链上的字段（`registry.rs:284-289`，内部
//!    `LinkedHashMap` 合并 ⇒ 保序、覆盖保位），与 Java 同口径；这里拿到的是 `Vec<FieldDef>`
//!    克隆（Java 交的是同一批活对象，本方法对字段只读，无可观察差异）。
//! 10. `record.fields` 的键查找、`ContentType.values()` ⇒ [`ContentType::all()`]（声明顺序，
//!     `content_type.rs:26-40`）——`:75-79`、`:86-90`、`:119-123` 的循环顺序会影响「先命中的
//!     类型」，但三处都只把循环当作存在性判定（命中即 `return true`），顺序不可观察。
//! 11. **与 Java 的偏离（用户 2026-11-26 授权，唯一一处语义偏离）**：Java `collectNames`
//!     递归进任意容器时，会把数组里的裸字符串一律当引用；内联对象配置里的非内容名数组
//!     （真实样本 `destroyBullet.hitEffect.effects: ["massiveExplosion"]`，该名是原版 Fx
//!     特效名，`Fx.java:1281`）因此被误报 error 并卡死 CLI 校验（exit 2）。现在只收集
//!     顶层字段值/顶层数组里的裸字符串；对象内部递归时不再收集裸字符串，具名键
//!     （item/liquid/payload/block/unit/content）与对象内嵌结构照旧——
//!     `requirements: [{item, amount}]`、`consumes: {liquids: [...]}` 的检查不放松。
//!     逐字理由见 [`ReferenceCheck::collect_names_into`] 文档。

use mm_json::Json;
use mm_model::ModProject;
use mm_schema::{ContentType, FieldDef, FieldType, SchemaRegistry};

use crate::check_support;
use crate::issue::{Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `new String[] {"item", "liquid", "payload", "block", "unit", "content"}`
/// （`ReferenceCheck.java:211`），同时用于 `:232` 的整串正则（模块头第 4 条）。
const NAMED_KEYS: [&str; 6] = ["item", "liquid", "payload", "block", "unit", "content"];

/// Java `String.trim()`（模块头第 6 条）。
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

/// Java `class ReferenceCheck implements ProjectCheck`（`ReferenceCheck.java:18`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ReferenceCheck;

impl ProjectCheck for ReferenceCheck {
    /// Java `id()`（`ReferenceCheck.java:20-23`）。
    fn id(&self) -> &str {
        "references"
    }

    /// Java `check(ValidationContext)`（`ReferenceCheck.java:25-67`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :27-29
        let project: &ModProject = context.project();
        let registry: &SchemaRegistry = context.registry();
        let internal: String = context.internal_name();

        // :30-66
        for record in &project.contents {
            // :31-33  内联对象不是独立内容，引用由宿主文件负责
            if record.is_inline() {
                continue;
            }
            // :34-37  解析不出类名的记录没有字段表可查
            let class_name: Option<String> =
                registry.resolve_class(record.ctype, record.class_name().as_deref());
            let Some(class_name) = class_name else { continue };
            // :38-39  只管引用型字段
            for field in registry.fields_of(&class_name).iter() {
                if !field.is_reference() {
                    continue;
                }
                // :42-45  字段没写就没得查
                let Some(value) = record.fields.get(field.name()) else {
                    continue;
                };
                // :46  collectNames(value, field)
                let names: Vec<String> = self.collect_names(Some(value), field);
                for name in &names {
                    // :47-58  方法注释（`:48-51`）直译：游戏的值解析器对任何具体 Content 子类的字段
                    //         都抛 "No X found with name ..."（ContentParser.java:505-514），整个文件
                    //         因此加载失败——是 error，不是 warning。例外是 "research"：它只打一条
                    //         "isn't in the tech tree" 日志（ContentParser.java:1401-1405），保持 warning。
                    if !self.resolves(registry, context, name.as_str(), field, &internal) {
                        let level: Level = if field.name() == "research" {
                            Level::Warning
                        } else {
                            Level::Error
                        };
                        context.report(Issue::new(
                            level,
                            "validate.reference.unresolved",
                            &[
                                record.ctype.name(),
                                record.name.as_str(),
                                name.as_str(),
                                field.name(),
                            ],
                            record.id.clone(),
                        ));
                    // :59-63  能解析，但裸名悄悄绑到了原版
                    } else if self.shadows_vanilla(registry, context, name.as_str(), field, &internal)
                    {
                        context.report(Issue::new(
                            Level::Warning,
                            "validate.reference.shadowsVanilla",
                            &[
                                record.ctype.name(),
                                record.name.as_str(),
                                name.as_str(),
                                field.name(),
                                internal.as_str(),
                                name.as_str(),
                            ],
                            record.id.clone(),
                        ));
                    }
                }
            }
        }
    }
}

impl ReferenceCheck {
    /// Java `private boolean resolves(SchemaRegistry, ModProject, String, FieldDef, String)`
    /// （`ReferenceCheck.java:69-93`）。
    fn resolves(
        &self,
        registry: &SchemaRegistry,
        context: &ValidationContext<'_>,
        name: &str,
        field: &FieldDef,
        internal: &str,
    ) -> bool {
        // :71-74  去掉 `<internal>-` 前缀后先查本模组内容
        let bare: &str = check_support::strip_prefix(name, internal);
        if self.matches_mod_content(context, bare, field) {
            return true;
        }
        // :75-79  再查按字段槽位提示的那几类原版内容
        for ctype in ContentType::all() {
            if type_matches(field, ctype) && registry.is_vanilla_name(ctype, bare) {
                return true;
            }
        }
        // :80-91  方法注释直译：结构化组会混用引用种类（一个 UnitReq 既指工厂方块又指物品花费），
        //         所以在带提示的类型试完之后，这个名字仍可能作为任意内容解析成功。
        if field.field_type() == FieldType::Struct {
            if self.matches_any_content_unfiltered(context, bare) {
                return true;
            }
            for ctype in ContentType::all() {
                if registry.is_vanilla_name(ctype, bare) {
                    return true;
                }
            }
        }
        false
    }

    /// Java `private boolean matchesAnyContentUnfiltered(ModProject, String)`
    /// （`ReferenceCheck.java:95-103`）。
    ///
    /// 方法注释（`:95`）直译：忽略字段的槽位提示、在所有类型上做的名字匹配。
    /// 名字→记录的索引由 context 惰性持有，语义与逐条全量扫一致（非内联 + 名字精确相等）。
    fn matches_any_content_unfiltered(&self, context: &ValidationContext<'_>, name: &str) -> bool {
        !context.content_indices_by_name(name).is_empty()
    }

    /// Java `private boolean shadowsVanilla(SchemaRegistry, ModProject, String, FieldDef, String)`
    /// （`ReferenceCheck.java:105-125`）。
    ///
    /// 方法注释（`:105-112`）直译：一个裸引用是否「悄悄绑到原版而不是模组自己的同名内容」。只有
    /// 原版优先的查找会让作者吃一惊，而 [`vanilla_first`] 列出了哪些字段属于这类；其它地方游戏都会
    /// 先试 `<mod>-<name>` 再试裸名（ContentParser.java:507-516），所以裸名正是指向模组自有内容的
    /// 正确写法，不该报。补丁记录豁免：补丁按定义就沿用原版名字，对它的裸引用恰恰就是作者的本意。
    fn shadows_vanilla(
        &self,
        registry: &SchemaRegistry,
        context: &ValidationContext<'_>,
        name: &str,
        field: &FieldDef,
        internal: &str,
    ) -> bool {
        // :115-118  `name.startsWith(internal + "-")` 就是「作者已经写了前缀」
        let prefixed: String = format!("{internal}-");
        if !vanilla_first(field) || name.starts_with(prefixed.as_str()) {
            return false;
        }
        if !self.matches_own_content(context, name, field) {
            return false;
        }
        // :119-124
        for ctype in ContentType::all() {
            if type_matches(field, ctype) && registry.is_vanilla_name(ctype, name) {
                return true;
            }
        }
        false
    }

    /// Java `private boolean matchesModContent(ModProject, String, FieldDef)`
    /// （`ReferenceCheck.java:158-161`）。
    ///
    /// 方法注释（`:158`）直译：`name` 是否匹配本模组一个有文件支撑的记录，遵从字段的 ctype。
    fn matches_mod_content(&self, context: &ValidationContext<'_>, name: &str, field: &FieldDef) -> bool {
        self.matches_any_content(context, name, field, true)
    }

    /// Java `private boolean matchesOwnContent(ModProject, String, FieldDef)`
    /// （`ReferenceCheck.java:163-166`）。
    ///
    /// 方法注释（`:163`）直译：同上，但只算模组自己新增的内容——补丁沿用原版名。
    fn matches_own_content(&self, context: &ValidationContext<'_>, name: &str, field: &FieldDef) -> bool {
        self.matches_any_content(context, name, field, false)
    }

    /// Java `private boolean matchesAnyContent(ModProject, String, FieldDef, boolean)`
    /// （`ReferenceCheck.java:168-182`）。
    fn matches_any_content(
        &self,
        context: &ValidationContext<'_>,
        name: &str,
        field: &FieldDef,
        patches_count: bool,
    ) -> bool {
        // 索引已保证非内联且名字精确相等（:171-173），剩下只筛补丁与类型。
        for &i in context.content_indices_by_name(name) {
            let record = &context.project().contents[i];
            // :174-176  豁免补丁时（`patchesCount == false`）跳过沿用原版名的补丁记录
            if !patches_count && record.patch {
                continue;
            }
            if type_matches(field, record.ctype) {
                return true;
            }
        }
        false
    }

    /// Java `private List<String> collectNames(JsonElement, FieldDef)`
    /// （`ReferenceCheck.java:190-194`）：把递归收集用的 `out` 换成返回值。
    fn collect_names(&self, value: Option<&Json>, field: &FieldDef) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        // 顶层允许收集裸字符串（字段值本身是名字串，或顶层名字数组，如 shownPlanets）。
        self.collect_names_into(value, field, &mut out, true);
        out
    }

    /// Java `private void collectNames(JsonElement, FieldDef, List<String>)`
    /// （`ReferenceCheck.java:196-253`）。
    ///
    /// **与 Java 的偏离（用户 2026-11-26 授权）**：新增 `allow_bare`。Java 会递归进任意容器
    /// 并把数组里的裸字符串一律当引用收集，于是内联对象配置里的**非内容名数组**被误判为
    /// 内容引用——真实模组里 `destroyBullet.hitEffect.effects: ["massiveExplosion"]`
    /// （原版 Fx 特效名，`Fx.java:1281`）因此报出 error 并把 CLI 校验卡成 exit 2。
    /// 现规则：对象内部的容器递归时 `allow_bare=false`，裸字符串不再收集；**具名键**
    /// （item/liquid/payload/block/unit/content）与对象内嵌结构里的具名键照旧收集——
    /// `requirements: [{item, amount}]`、`consumes: {liquids: [{liquid, amount}]}` 不受影响。
    /// 顶层字段值/顶层数组仍按 Java 原口径收集（名字串、名字数组的检查不放松）。
    fn collect_names_into(
        &self,
        value: Option<&Json>,
        field: &FieldDef,
        out: &mut Vec<String>,
        allow_bare: bool,
    ) {
        // :197-199  null / JsonNull
        let Some(value) = value else { return };
        if value.is_null() {
            return;
        }

        // :200-205  方法注释直译：炮塔的弹药 map——**键**才是 item/liquid 引用（值是子弹）。
        //         条件不成立、或值不是对象时，Java 往下走数组/对象分支，这里同样只是不 return。
        let field_type: FieldType = field.field_type();
        if field_type == FieldType::Struct && field.stack_kind() == Some("ammoMap")
            && let Json::Obj(object) = value
        {
            for key in object.keys() {
                out.push(key.clone());
            }
            return;
        }

        // :206-209  数组逐个递归（顶层数组保持 Java 原口径，允许收集裸字符串）
        if let Some(elements) = value.as_array() {
            for element in elements {
                self.collect_names_into(Some(element), field, out, allow_bare);
            }
            return;
        }

        // :210-238  对象：先取 6 个具名键，再递归进容器
        if let Some(object) = value.as_object() {
            for key in NAMED_KEYS {
                let named: Option<&Json> = object.get(key);
                let Some(named) = named else { continue };
                // :213  模块头第 3 条：primitive 且是字符串
                if !named.is_string() {
                    continue;
                }
                // :214-217  `getAsString()` 对 `Str` 就是原文；trim 后空串或 "icon-" 前缀跳过
                let raw: String = named.get_as_string();
                let mut text: &str = java_trim(raw.as_str());
                if text.is_empty() || text.starts_with("icon-") {
                    continue;
                }
                // :218-224  方法注释直译：Consume 组及其同类在具名键上也把栈写成 "name/amount"。
                if (field_type.is_stack() || field_type == FieldType::Struct)
                    && let Some(slash) = text.find('/')
                    && slash > 0
                {
                    text = &text[..slash];
                }
                out.push(text.to_string());
            }

            // :228-236  方法注释直译：结构化组把引用嵌在下一层（UnitReq 的 requirements、consume 组
            //           的 items）；只递归进容器，这样一个挂在未知键下的任意字符串（子弹的 "type"）
            //           永远不会被当成引用。
            // 与 Java 的偏离（模块头 collect_names_into 已登记）：对象内部递归时 allow_bare=false，
            // 内联配置里的非内容名数组（hitEffect.effects 等）不再被当作引用；具名键照旧。
            for (key, child) in object.iter() {
                if NAMED_KEYS.contains(&key.as_str()) {
                    continue;
                }
                if child.is_array() || child.is_object() {
                    self.collect_names_into(Some(child), field, out, false);
                }
            }
            return;
        }

        // :239-241  剩下的只有 primitive：非字符串（布尔/数字）不管
        if !value.is_string() {
            return;
        }
        // 偏离（模块头已登记）：只收顶层/顶层数组里的裸字符串；对象内嵌数组里的字符串
        // 是配置值（特效名等），不是内容引用。
        if !allow_bare {
            return;
        }

        // :242-251  标量引用，规则与具名键那支逐字相同
        let raw: String = value.get_as_string();
        let mut text: &str = java_trim(raw.as_str());
        if text.is_empty() || text.starts_with("icon-") {
            return;
        }
        if (field_type.is_stack() || field_type == FieldType::Struct)
            && let Some(slash) = text.find('/')
            && slash > 0
        {
            text = &text[..slash];
        }
        out.push(text.to_string());
    }
}

/// Java `private static boolean vanillaFirst(FieldDef)`（`ReferenceCheck.java:127-156`）。
///
/// 方法注释（`:127-151`）直译：游戏是否**原版优先**地解析这个引用，使得一个原版也在用的裸名字
/// 悄悄绑到原版条目：
/// * status 型字段走 StatusEffect 类解析器，对字符串值是一次 `locate()` 调用
///   （ContentParser.java:144-148）——精选 schema 用 contentCtype "status" 标出这些字段；
/// * `SectorPreset.planet` 会先从 JSON 里取出、再用 `locate()` 查（ContentParser.java:765、784），
///   而且没有别的类有叫 "planet" 的字段；
/// * `research` 根本不是类上的字段：`readFields` 把它从 JSON 里拎出来（ContentParser.java:1270），
///   科技树父节点靠对 `TechTree.all` 的一次 `find` 匹配（ContentParser.java:1401）。那个列表按内容
///   创建顺序排列（TechTree.java:138），而原版科技树在任何模组加载之前就建好了
///   （ContentLoader.java:72-79），所以裸 "copper" 会挑中原版节点，即便模组自己定义了 copper；
///   而带前缀的 "my-mod-copper" 写法能经由同一个谓词的第一支到达模组自己的节点——所以下面的建议
///   在那儿确实是真正的修法。
///
/// 其它一切都是具体的 `Content` 子类，走前缀优先那支。声明类型恰好是 `UnlockableContent` 的字段
/// 也会原版优先（ContentParser.java:497-505），但 schema 不暴露这样的字段——它不带 Java 声明类型，
/// 只带内容类型。
fn vanilla_first(field: &FieldDef) -> bool {
    field.content_ctype() == Some("status")
        || field.name() == "planet"
        || field.name() == "research"
}

/// Java `private static boolean typeMatches(FieldDef, ContentType)`（`ReferenceCheck.java:184-188`）。
///
/// 方法注释（`:184`）直译：字段声明的内容类型是否容得下这个类型；"any" 或者没有声明则容纳所有类型。
/// 模块头第 8 条：Java 的 `field == null` 一支在本类里不可达，故 Rust 收 `&FieldDef`。
fn type_matches(field: &FieldDef, ctype: ContentType) -> bool {
    match field.content_ctype() {
        None => true,
        Some(hint) => hint == "any" || hint == ctype.name(),
    }
}

#[cfg(test)]
mod tests {
    //! 回归（2026-11-26 用户授权）：对象内嵌数组里的裸字符串不是内容引用。
    //! 真实样本：`destroyBullet.hitEffect.effects: ["massiveExplosion"]` 里
    //! `massiveExplosion` 是原版 Fx 特效名（`Mindustry-160.8/.../Fx.java:1281`），
    //! Java 口径会把它当内容引用报 error，真实模组 CLI 校验被卡成 exit 2。

    use super::*;
    use crate::validator::Validator;
    use mm_json::Json;
    use mm_model::ContentRecord;
    use std::path::Path;

    fn registry() -> SchemaRegistry {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        SchemaRegistry::load(&dir).expect("../../schemas 必须在仓库里")
    }

    fn object(entries: &[(&str, Json)]) -> Json {
        let mut obj = mm_json::Object::default();
        for (key, value) in entries {
            obj.insert(*key, value.clone());
        }
        Json::Obj(obj)
    }

    fn block_with(fields: &[(&str, Json)]) -> ModProject {
        let mut project = ModProject::default();
        project.meta.name = "ref-test-mod".to_string();
        project.meta_file = "mod.hjson".to_string();
        let mut record = ContentRecord::create("blk-1", ContentType::Block, "ref-test-block");
        for (key, value) in fields {
            record.fields.insert(*key, value.clone());
        }
        project.contents.push(record);
        project
    }

    fn reference_issues(project: &ModProject) -> Vec<Issue> {
        let registry = registry();
        let validator = Validator::new(&registry);
        validator
            .validate_only(project, Some(&["references"]))
            .expect("references 是已知 id")
    }

    /// 修复点：内联配置（对象）里嵌套数组中的特效名不得被当作内容引用。
    #[test]
    fn effect_names_inside_inline_config_are_not_content_references() {
        let hit_effect = object(&[
            ("type", Json::str("MultiEffect")),
            ("effects", Json::Arr(vec![Json::str("massiveExplosion")])),
        ]);
        let destroy = object(&[("hitEffect", hit_effect)]);
        let project = block_with(&[("destroyBullet", destroy)]);
        let issues = reference_issues(&project);
        assert!(
            issues
                .iter()
                .all(|issue| !issue.message().contains("massiveExplosion")),
            "特效名不是内容引用，不得报 unresolved：{issues:?}"
        );
    }

    /// 不放松：顶层字段值是名字串时，解析不到仍必须报 error（Java 原口径）。
    #[test]
    fn top_level_bare_string_is_still_checked() {
        let project = block_with(&[("destroyBullet", Json::str("no-such-bullet"))]);
        let issues = reference_issues(&project);
        assert!(
            issues.iter().any(|issue| {
                issue.level() == Level::Error && issue.message().contains("no-such-bullet")
            }),
            "顶层名字串解析失败必须报 error：{issues:?}"
        );
    }

    /// 不放松：requirements 数组元素的具名键（item）仍是内容引用。
    #[test]
    fn named_keys_inside_arrays_are_still_checked() {
        let requirements = Json::Arr(vec![object(&[
            ("item", Json::str("no-such-item")),
            ("amount", Json::str("1")),
        ])]);
        let project = block_with(&[("requirements", requirements)]);
        let issues = reference_issues(&project);
        assert!(
            issues.iter().any(|issue| {
                issue.level() == Level::Error && issue.message().contains("no-such-item")
            }),
            "requirements 的 item 具名键必须照旧检查：{issues:?}"
        );
    }
}
