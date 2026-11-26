//! `SoundReferenceCheck.java`（81 行）的等价移植：`sounds/` 下的资源必须是游戏音频加载器认得的
//! 扩展名，而引用音效的字符串字段不该带文件后缀。
//!
//! 类注释（`SoundReferenceCheck.java:13`）直译：音效资源必须是真正的 OGG 文件，且引用从不带文件扩展名。
//!
//! 与 Java 的语言差异（只有下面这几处）：
//! 1. `Set.of("mp3", "ogg")`（`:17`）是**无序**集合，`String.join(" or ", SOUND_EXTENSIONS)`
//!    （`:36`）的拼接顺序在 Java 里由 `Set.of` 的哈希种子决定，同一条消息在不同 JVM 进程里可能是
//!    "mp3 or ogg" 也可能是 "ogg or mp3"。这里按声明顺序固定为 `["mp3", "ogg"]` 并产出
//!    "mp3 or ogg"：判定分支（是否包含该扩展名）与顺序无关，只有这条消息的文本受影响，
//!    因此这是**收敛**（取 Java 的一种合法输出）而非新增行为。
//! 2. `String.toLowerCase(Locale.ROOT)`（`:33`、`:73`）↔ Rust `to_lowercase()`：Java 做的是
//!    逐码点（UTF-16 code unit）映射，Rust 走完整 Unicode 小写映射，二者只在 U+0130（`İ`）
//!    一类少数码点上结果不同（Rust 多出 combining dot）。本文件的两处小写只用于「是否等于
//!    mp3/ogg」与「是否以 .ogg 结尾」，实际语料都是 ASCII，判定方向一致。
//! 3. `String.substring`/`lastIndexOf` 的 UTF-16 计数 ↔ Rust 字节切片：这里剥的都是 ASCII 的
//!    `.png`/`.ogg`/`.`，切点必落在字符边界，产出的字符串内容逐字符相同。
//! 4. Java 有两个同名重载 `checkFields(ValidationContext, Map, ContentRecord)`（`:44-60`，会递归）
//!    与 `checkFields(ValidationContext, JsonObject, ContentRecord)`（`:62-66`，只判一层）。
//!    `record.fields` 的**静态类型**是 `Map<String, JsonElement>`，故 `:40` 绑定到前者；
//!    后者只在递归里用到。Rust 没有重载，改名 `check_fields_map` / `check_fields_object` 并逐一对应，
//!    递归深度因此与 Java 完全一致（顶层 → 嵌套对象一层；顶层 → 数组里的对象一层；不再 deeper）。
//! 5. `JsonElement.isJsonPrimitive() && getAsJsonPrimitive().isString()`（`:70-71`）
//!    ↔ `Json::Str(_)`：mm_json 的 `Str` 变体恰是「字符串 primitive」，数字与布尔各在
//!    `Num`/`Bool`，故一个模式匹配替代两次判定，无差异。

use mm_json::{Json, Object};
use mm_model::{ContentRecord, ModProject};

use crate::issue::{Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `SOUND_EXTENSIONS`（`SoundReferenceCheck.java:16-17`）：游戏数据加载器在 `sounds/` 下
/// 接受的音频扩展名（`DataAssetType.sound`）。顺序说明见模块头第 1 条。
const SOUND_EXTENSIONS: [&str; 2] = ["mp3", "ogg"];

/// Java `class SoundReferenceCheck implements ProjectCheck`（`SoundReferenceCheck.java:14`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SoundReferenceCheck;

impl ProjectCheck for SoundReferenceCheck {
    /// Java `id()`（`SoundReferenceCheck.java:19-22`）。
    fn id(&self) -> &str {
        "sounds"
    }

    /// Java `check(ValidationContext)`（`SoundReferenceCheck.java:24-42`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`；
    /// 本规则其实不读内部名，沿用默认只会让「内部名为空的工程」整条跳过，与 Java 同。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :26  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :27-38  sounds/ 下每个资源的扩展名
        for asset in &project.assets {
            // :28-30
            if !asset.path.starts_with("sounds/") {
                continue;
            }
            // :31  asset.path.lastIndexOf('/') + 1：没有斜杠时 Java 得 0
            let file_name: &str = &asset.path[asset.path.rfind('/').map(|i| i + 1).unwrap_or(0)..];
            // :32-33  无点 ⇒ 扩展名是空串，仍然会走到下面报一条 badExtension
            let dot: Option<usize> = file_name.rfind('.');
            let extension: String = match dot {
                Some(i) => file_name[i + 1..].to_lowercase(),
                None => String::new(),
            };
            // :34-37  warning validate.sound.badExtension（无 contentId）
            if !SOUND_EXTENSIONS.contains(&extension.as_str()) {
                let joined: String = SOUND_EXTENSIONS.join(" or ");
                context.report(Issue::warning(
                    "validate.sound.badExtension",
                    &[asset.path.as_str(), extension.as_str(), joined.as_str()],
                ));
            }
        }

        // :39-41  for (ContentRecord record : project.contents) checkFields(context, record.fields, record)
        for record in &project.contents {
            self.check_fields_map(context, &record.fields, record);
        }
    }
}

impl SoundReferenceCheck {
    /// Java `private void checkFields(ValidationContext, Map<String, JsonElement>, ContentRecord)`
    /// （`SoundReferenceCheck.java:44-60`）。注释（`:49`）：递归，好让武器数组等嵌套对象继续被检查。
    fn check_fields_map(
        &self,
        context: &mut ValidationContext<'_>,
        fields: &Object,
        record: &ContentRecord,
    ) {
        // :46-47  for (Map.Entry<String, JsonElement> entry : fields.entrySet())
        for (key, value) in fields.iter() {
            // :48
            self.check_field(context, key, value, record);
            // :50-58  对象 ⇒ 进入「只判一层」的重载；数组 ⇒ 其中每个对象进入该重载
            match value {
                Json::Obj(object) => self.check_fields_object(context, object, record),
                Json::Arr(elements) => {
                    for element in elements {
                        if let Json::Obj(object) = element {
                            self.check_fields_object(context, object, record);
                        }
                    }
                }
                Json::Null | Json::Bool(_) | Json::Num(_) | Json::Str(_) => {}
            }
        }
    }

    /// Java `private void checkFields(ValidationContext, JsonObject, ContentRecord)`
    /// （`SoundReferenceCheck.java:62-66`）：只逐键判定，不再递归。
    fn check_fields_object(
        &self,
        context: &mut ValidationContext<'_>,
        object: &Object,
        record: &ContentRecord,
    ) {
        // :63-65
        for (key, value) in object.iter() {
            self.check_field(context, key, value, record);
        }
    }

    /// Java `private void checkField(ValidationContext, String, JsonElement, ContentRecord)`
    /// （`SoundReferenceCheck.java:68-80`）：键名以 `Sound` 结尾、值是字符串，且小写后以 `.ogg`
    /// 结尾 ⇒ 引用带了扩展名。
    fn check_field(
        &self,
        context: &mut ValidationContext<'_>,
        key: &str,
        value: &Json,
        record: &ContentRecord,
    ) {
        // :70  key.endsWith("Sound")
        if !key.ends_with("Sound") {
            return;
        }
        // :70-71  模块头第 5 条：Json::Str 恰是「字符串 primitive」
        let Json::Str(raw) = value else { return };
        // :72-73
        if !raw.to_lowercase().ends_with(".ogg") {
            return;
        }
        // :77  name.substring(0, name.lastIndexOf('.'))：上一步已保证串里至少有一个 '.'
        //      （小写后以 ".ogg" 结尾 ⇒ 长度 ≥ 4 且倒数第 4 个字符是 '.'）
        let without_extension: &str = &raw[..raw.rfind('.').unwrap_or(0)];
        // :74-78  warningOn(record.id, ...) ⇒ 交 Java 的可空 contentId，用 Issue::new
        context.report(Issue::new(
            Level::Warning,
            "validate.sound.referenceCarriesExtension",
            &[
                record.ctype.name(),
                record.name.as_str(),
                key,
                raw.as_str(),
                without_extension,
            ],
            record.id.clone(),
        ));
    }
}
