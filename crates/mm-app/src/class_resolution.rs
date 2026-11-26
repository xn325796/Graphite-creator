//! 有效类解析：一条内容记录「实际会被游戏当成哪个类加载」。
//!
//! ## 为什么要有这个模块（与 Java 的偏离，2026-11-26 产品评审 R-02 授权的新设计）
//!
//! Java `ProjectController.classNameOf`（`:2284-2287`）只有两级：内容 JSON 里声明的 `type`
//! → 类型默认类（`SchemaRegistry.resolveClass`，`:340-353`）。真实模组在这两级之间还有一级：
//! 方块常常不写 `type`，类由 `scripts/**/*.js` 的 `extend(<Class>, '<name>')` 决定
//! （样本模组 184 个方块里 115 个如此，见 `mm_io::script_scan` 模块头的实测数字）。
//!
//! 于是有效类的优先级是**三级**：
//!
//! 1. [`ClassSource::Declared`]：记录自己声明了非空白 `type`——文件优先，与 Java 逐字一致；
//! 2. [`ClassSource::Script`]：脚本里有 `extend(<Class>, '<记录名>')`（[`ModProject::script_classes`]）；
//! 3. [`ClassSource::Default`]：`ContentType::default_class()`（方块 → `Block`），Java 的兜底。
//!
//! 三条硬边界：
//!
//! - **不写回模型**。推断结果绝不进 `ContentRecord::set_class_name`／`fields["type"]`：那是磁盘
//!   上的字节，一次打开项目就改用户的模组文件不可接受。保存路径因此完全不知道本模块存在。
//! - **unit 不参与脚本推断**。`type` 在单位上是移动方式（`"flying"`…）不是类名，
//!   `SchemaRegistry.resolve_class` 对单位一律取默认类（`:341-347`）；脚本能写出的
//!   `extend(UnitType, '<名>')` 类名也与默认类相同，故单位恒为 [`ClassSource::Default`]，
//!   行为与移植前逐字一致。
//! - **空类名保留空串**。`Weather` 的 `default_class()` 是空串（游戏没有默认的天气类），
//!   此时 [`EffectiveClass::class_name`] 为空；要不要回落到 `ctype.name()` 由调用方决定
//!   （[`ProjectController::class_name_of`](crate::project_controller::ProjectController::class_name_of)
//!   沿用 Java 的 `record.type.name()` 回落）。
//!
//! [`ModProject::script_classes`]: mm_model::ModProject::script_classes

use mm_model::{ContentRecord, ModProject};
use mm_schema::{ContentType, SchemaRegistry};

/// 有效类的来源。序列化拼写（`"declared"`／`"script"`／`"default"`）由 [`ClassSource::as_str`]
/// 给出，DTO 层直接用它，前端据此决定类名那一行是否可编辑、要不要标「由脚本推断」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassSource {
    /// 记录 JSON 里显式声明了 `type`。
    Declared,
    /// 脚本 `extend(<Class>, '<名>')` 推断出来的。
    Script,
    /// 类型默认类（没有声明、脚本里也没有）。
    Default,
}

impl ClassSource {
    /// 下发/落日志用的稳定拼写。
    pub fn as_str(self) -> &'static str {
        match self {
            ClassSource::Declared => "declared",
            ClassSource::Script => "script",
            ClassSource::Default => "default",
        }
    }
}

/// 一条记录的有效类与它的来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveClass {
    /// schema 类名（已按 `SchemaRegistry::canonical_class_name` 归一）；无默认类的类型为空串。
    pub class_name: String,
    /// 这个类名是从哪一级来的。
    pub source: ClassSource,
}

/// 解析有效类。`project` 为 `None`（壳还没打开项目）时脚本这一级不存在，与 Java 两级解析同形。
pub fn effective_class(project: Option<&ModProject>, record: &ContentRecord) -> EffectiveClass {
    // 单位的 `type` 是移动方式，`resolve_class` 也不读它（模块头第 2 条边界）。
    if record.ctype == ContentType::Unit {
        return default_class(record.ctype);
    }
    if let Some(declared) = record.class_name().as_deref().map(str::trim)
        && !declared.is_empty()
    {
        return EffectiveClass {
            class_name: SchemaRegistry::canonical_class_name(declared),
            source: ClassSource::Declared,
        };
    }
    if let Some(script) = project.and_then(|project| project.script_classes.get(&record.name)) {
        return EffectiveClass {
            class_name: SchemaRegistry::canonical_class_name(script),
            source: ClassSource::Script,
        };
    }
    default_class(record.ctype)
}

fn default_class(ctype: ContentType) -> EffectiveClass {
    EffectiveClass {
        class_name: ctype.default_class().to_string(),
        source: ClassSource::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(ctype: ContentType, name: &str, declared: Option<&str>) -> ContentRecord {
        let mut record = ContentRecord {
            ctype,
            name: name.to_string(),
            ..Default::default()
        };
        if let Some(declared) = declared {
            record.set_class_name(Some(declared));
            record.dirty = false;
        }
        record
    }

    fn project_with(entries: &[(&str, &str)]) -> ModProject {
        let mut project = ModProject::default();
        for (name, class) in entries {
            project
                .script_classes
                .insert((*name).to_string(), (*class).to_string());
        }
        project
    }

    /// 三级优先级的正面顺序：declared > script > default。
    #[test]
    fn declared_beats_script_and_script_beats_default() {
        let project = project_with(&[("载荷发射台", "PayloadBlock"), ("声明过的", "Drill")]);

        let declared = record(ContentType::Block, "声明过的", Some("unloader"));
        let effective = effective_class(Some(&project), &declared);
        assert_eq!(effective.class_name, "Unloader");
        assert_eq!(effective.source, ClassSource::Declared);

        let scripted = record(ContentType::Block, "载荷发射台", None);
        let effective = effective_class(Some(&project), &scripted);
        assert_eq!(effective.class_name, "PayloadBlock");
        assert_eq!(effective.source, ClassSource::Script);

        let unknown = record(ContentType::Block, "没人认识", None);
        let effective = effective_class(Some(&project), &unknown);
        assert_eq!(effective.class_name, "Block");
        assert_eq!(effective.source, ClassSource::Default);
    }

    /// 脚本类名要过 `canonical_class_name`：全限定名与首字母小写写法归一成短类名。
    #[test]
    fn script_classes_are_canonicalized() {
        let project = project_with(&[
            ("全限定", "mindustry.world.blocks.payloads.PayloadBlock"),
            ("小写", "payloadBlock"),
        ]);
        for name in ["全限定", "小写"] {
            let effective =
                effective_class(Some(&project), &record(ContentType::Block, name, None));
            assert_eq!(effective.class_name, "PayloadBlock", "{name}");
            assert_eq!(effective.source, ClassSource::Script);
        }
    }

    /// 空白 `type` 与「没写 `type`」同一个语义：让位给脚本推断。
    #[test]
    fn blank_declared_type_falls_through_to_script() {
        let project = project_with(&[("空白声明", "PayloadBlock")]);
        let record = record(ContentType::Block, "空白声明", Some("  "));
        let effective = effective_class(Some(&project), &record);
        assert_eq!(effective.class_name, "PayloadBlock");
        assert_eq!(effective.source, ClassSource::Script);
    }

    /// 单位恒为默认类：`type` 是移动方式，脚本也不参与（模块头第 2 条边界）。
    #[test]
    fn units_stay_on_the_default_class() {
        let project = project_with(&[("飞行兵", "PayloadBlock")]);
        let unit = record(ContentType::Unit, "飞行兵", Some("flying"));
        let effective = effective_class(Some(&project), &unit);
        assert_eq!(effective.class_name, "UnitType");
        assert_eq!(effective.source, ClassSource::Default);
    }

    /// DTO 下发的三种拼写是前端契约（`classSource`），改名等同 breaking change。
    #[test]
    fn class_source_spellings_are_stable() {
        assert_eq!(ClassSource::Declared.as_str(), "declared");
        assert_eq!(ClassSource::Script.as_str(), "script");
        assert_eq!(ClassSource::Default.as_str(), "default");
    }

    /// 推断结果不回流到模型：`fields["type"]` 必须仍然是空的。
    #[test]
    fn inference_never_writes_back_to_the_record() {
        let project = project_with(&[("载荷发射台", "PayloadBlock")]);
        let record = record(ContentType::Block, "载荷发射台", None);
        let effective = effective_class(Some(&project), &record);
        assert_eq!(effective.source, ClassSource::Script);
        assert_eq!(
            record.class_name(),
            None,
            "推断只影响视图，不得改记录的 type"
        );
        assert!(!record.dirty, "读取有效类不是一次编辑");
        assert!(!record.fields.contains("type"), "fields 里不得出现 type");
    }
}
