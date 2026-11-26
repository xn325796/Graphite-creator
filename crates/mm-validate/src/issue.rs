//! `Issue.java` 的等价移植：校验器的一条发现（error / warning）。
//!
//! Java 侧是 `record Issue(Level level, String key, List<Object> args, String contentId)`。
//! Rust 里用普通 struct + 访问器方法一一对应 record 的四个组件访问器（`level()`、`key()`、
//! `args()`、`contentId()` → `content_id()`）；record 会自动生成 equals/hashCode/toString，
//! 因此这里派生 `PartialEq`/`Eq`/`Debug`，与 Java 的隐含契约对齐，不额外提供 Display 等。
//!
//! 一条发现只携带消息 **键** 与参数，绝不携带拼好的句子：格式内核没有 UI，不能依赖 UI 的
//! 本地化类。`message()` 渲染英文，用于日志/控制台/测试断言；校验面板按用户 locale 渲染同一个
//! 键，所以同一条发现会随 UI 语言读作中文或英文。

use std::collections::HashMap;
use std::sync::OnceLock;

use mm_fmt::codec::PropertiesFile;
use mm_fmt::text::fill_str;

/// Java `Issue.ENGLISH = ResourceBundle.getBundle("messages", Locale.ENGLISH)`。
///
/// `messages_en.properties` 是**故意空的**（里面只有注释，没有一个键），它的存在只为让英文请求
/// 落到英文基准 bundle，而不是在默认 locale 为 zh_CN 的机器上回落到中文 bundle。因此英文文案的
/// 真正来源是默认 bundle `messages.properties`——这里取的是它的逐字节原样副本
/// `crates\mm-validate\assets\messages.properties`（SHA-256 与 Java 资源文件相同），由
/// `include_str!` 在编译期嵌入，避免依赖仓库外的绝对路径。
const MESSAGES_PROPERTIES: &str = include_str!("../assets/messages.properties");

/// 惰性解析出的英文键值表，对应 Java 那个静态 `ENGLISH` bundle。
///
/// 解析复用 `mm_fmt::codec::PropertiesFile`（即 Java `PropertiesFile`/`PropertiesUtils.load`
/// 的等价实现），因此 `#` 注释行、空行、以及 `assets.importExport.hint`、`about.body`、
/// `dialog.projectDirNotWritable` 这类含 `\n` 转义的值都按同一套规则解码，不另写解析器。
fn english() -> &'static HashMap<String, String> {
    static BUNDLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        PropertiesFile::parse(MESSAGES_PROPERTIES)
            .values
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    })
}

/// Java `Issue.Level`。
///
/// Java 的枚举常量是小写 `error` / `warning`；Rust 的变体命名规范要求首字母大写，故写作
/// `Error` / `Warning`。这只是拼写差异，两个常量与 Java 一一对应，语义与判定完全不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 游戏会拒绝或会误读的东西。
    Error,
    /// 游戏能容忍、但作者多半并非有意为之的东西。
    Warning,
}

/// Java `Issue.args()` 的元素类型，即那个 `List<Object>` 里 `Object` 的**实际取值域**。
///
/// 四个变体逐一对应 `ModMakerCli.argument(Object)`（`ModMakerCli.java:255-266`）的四个分支：
/// `null` → `JsonNull`、`Number` → JSON 数字、`Boolean` → JSON 布尔、其余 → JSON 字符串。
/// 之所以必须把类型带在 `Issue` 上而不是渲染成 `String` 了事：`--json` 报告直接把 `args`
/// 序列化出去，Java 写的是 `"args":["huge",20,16]`，字符串化会退化成 `"["huge","20","16"]"`。
///
/// **取值域的两点如实登记**（对 Java 全仓 55 个 `Issue.*` 调用点逐个核查的结论）：
/// 1. `Int` 覆盖全部可达的 `Number`——11 个站点传的都是 `int` / `long` / `Integer`
///    （`(int) Math.ceil(..)`、`Math.round(..)`、`.intValue()`），**没有一处传 `Double` 或
///    其它浮点 `Number`**：Java 在传浮点前一律先过 `CheckSupport.trim(double)`（返回 `String`）。
///    所以这里不需要浮点变体，也因此避开了 `String.valueOf(1.0)`="1.0" 与 Rust
///    `1.0f64.to_string()`="1" 的文本分歧。
/// 2. `Null` 与 `Bool` 两个分支在 Java 侧**当前不可达**（无站点显式传 `null` / `Boolean`），
///    保留它们是为了让 `argument()` 的四分支移植在 Rust 侧有对应的输入类型，而不是缺两支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    /// Java 的 `null` 实参：`argument()` 给 `JsonNull`，`TextTemplate.fill` 渲染成空串。
    Null,
    /// Java 的整型 `Number`（`int` / `long` / `Integer`）。
    Int(i64),
    /// Java 的 `Boolean`。
    Bool(bool),
    /// Java 的 `String`，以及走 `String.valueOf(value)` 兜底的其它类型（枚举、集合等）。
    Text(String),
}

impl Arg {
    /// 文本实参。给调用方一个与 [`Arg::Text`] 同名的构造入口，纯文本站点写起来不啰嗦。
    pub fn text(value: impl Into<String>) -> Arg {
        Arg::Text(value.into())
    }

    /// 整型实参，对应 Java 那些 `(int) Math.ceil(..)` / `Math.round(..)` 的站点。
    pub fn int(value: i64) -> Arg {
        Arg::Int(value)
    }

    /// Java `String.valueOf(value)`（`TextTemplate.fill` 与 `Issue.message()` 用它渲染实参）。
    ///
    /// `null` 特判成空串来自 Java `TextTemplate.fill` 的 `value == null ? "" : String.valueOf(value)`；
    /// `Int`/`Bool` 的十进制与 `true`/`false` 拼写与 Java `String.valueOf(int/long/boolean)` 一致。
    pub fn text_of(&self) -> String {
        match self {
            Arg::Null => String::new(),
            Arg::Int(value) => value.to_string(),
            Arg::Bool(value) => value.to_string(),
            Arg::Text(value) => value.clone(),
        }
    }
}

/// Java 的 `int` 实参（11 个数字站点全部写作 `int` 或由 `(int)` 强转而来）。
impl From<i32> for Arg {
    fn from(value: i32) -> Arg {
        Arg::Int(value as i64)
    }
}

/// Java 的 `long` 实参（`Math.round(double)` 的返回类型）。
impl From<i64> for Arg {
    fn from(value: i64) -> Arg {
        Arg::Int(value)
    }
}

/// Java 的 `Boolean` 实参。
impl From<bool> for Arg {
    fn from(value: bool) -> Arg {
        Arg::Bool(value)
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Arg {
        Arg::Text(value.to_string())
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Arg {
        Arg::Text(value)
    }
}

/// Java `record Issue(...)`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    level: Level,
    key: String,
    /// Java 的 `List<Object>`。这里用 [`Arg`] 承载 `Object` 的取值域，**不是** `Vec<String>`：
    /// 后者会把数字压成字符串，让 `--json` 报告与 Java 产生字节差（见 [`Arg`] 的文档）。
    args: Vec<Arg>,
    content_id: Option<String>,
}

/// 把变参位置的参数收成表：对应 Java 的 `Arrays.asList(args)` + 紧凑构造器里的拷贝。
/// 收表要 `Clone`：Java 的变参是 `Object[]`，元素由调用方交出引用，这里必须自己复制一份
/// （对应紧凑构造器里 `new ArrayList<>(args)` 的深拷贝语义）。
fn copy_args<S: Into<Arg> + Clone>(args: &[S]) -> Vec<Arg> {
    args.iter().map(|a| a.clone().into()).collect()
}

impl Issue {
    /// Java 的规范构造器（含紧凑构造器）。
    ///
    /// 紧凑构造器做的是 `args = Collections.unmodifiableList(new ArrayList<>(args))`——
    /// 注释写明**用 ArrayList 拷贝而不是 `List.copyOf`，因为参数允许为 null**
    /// （`List.copyOf` 会拒绝 null 元素）。Rust 侧对应「从切片新建一个 owned `Vec`」，
    /// 同样是深拷贝，与调用方持有的 `Vec` 完全脱钩；`args()` 只交出 `&[String]`，
    /// 等价于 Java 那个不可变视图。
    pub fn new(level: Level, key: impl Into<String>, args: &[impl Into<Arg> + Clone], content_id: Option<String>) -> Issue {
        Issue {
            level,
            key: key.into(),
            args: copy_args(args),
            content_id,
        }
    }

    /// Java `error(String key, Object... args)`：contentId 传 null。
    pub fn error(key: impl Into<String>, args: &[impl Into<Arg> + Clone]) -> Issue {
        Issue::new(Level::Error, key, args, None)
    }

    /// Java `warning(String key, Object... args)`：contentId 传 null。
    pub fn warning(key: impl Into<String>, args: &[impl Into<Arg> + Clone]) -> Issue {
        Issue::new(Level::Warning, key, args, None)
    }

    /// Java `errorOn(String contentId, String key, Object... args)`。
    pub fn error_on(content_id: impl Into<String>, key: impl Into<String>, args: &[impl Into<Arg> + Clone]) -> Issue {
        Issue::new(Level::Error, key, args, Some(content_id.into()))
    }

    /// Java `warningOn(String contentId, String key, Object... args)`。
    pub fn warning_on(content_id: impl Into<String>, key: impl Into<String>, args: &[impl Into<Arg> + Clone]) -> Issue {
        Issue::new(Level::Warning, key, args, Some(content_id.into()))
    }

    /// Java `Level level()`。
    pub fn level(&self) -> Level {
        self.level
    }

    /// Java `String key()`。
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Java `List<Object> args()`（这里是它的只读视图，元素类型见 [`Arg`]）。
    pub fn args(&self) -> &[Arg] {
        &self.args
    }

    /// Java `String contentId()`，`error`/`warning` 两个工厂下为 null。
    pub fn content_id(&self) -> Option<&str> {
        self.content_id.as_deref()
    }

    /// 这条发现的英文文案，用于日志、控制台与测试断言。
    ///
    /// 逐条对应 Java：`ENGLISH.containsKey(key) ? TextTemplate.fill(ENGLISH.getString(key), args) : key`
    /// ——键**不存在**时原样返回键本身（校验测试大量依赖这个兜底），存在时按 `{0}`、`{1}` 填充参数。
    ///
    /// 填充前先把每个 [`Arg`] 按 Java `TextTemplate.fill` 内部的 `String.valueOf(value)` 渲染成
    /// 文本（[`Arg::text_of`]），所以数字实参在这里与 Java 一样只是**十进制串**，类型信息只在
    /// `--json` 报告里才以 JSON 数字的形式体现。
    pub fn message(&self) -> String {
        match english().get(&self.key) {
            Some(pattern) => {
                let rendered: Vec<String> = self.args.iter().map(|a| a.text_of()).collect();
                let args: Vec<&str> = rendered.iter().map(|a| a.as_str()).collect();
                fill_str(pattern, &args)
            }
            None => self.key.clone(),
        }
    }

    /// Java `boolean isError()`。
    pub fn is_error(&self) -> bool {
        self.level == Level::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_factory_carries_level_key_args_and_null_content_id() {
        // Java: Issue.error("validate.name.invalid", "Block", "core", "bad")
        let issue = Issue::error("validate.name.invalid", &["Block", "core", "bad"]);
        assert_eq!(issue.level(), Level::Error);
        assert_eq!(issue.key(), "validate.name.invalid");
        assert_eq!(
            issue.args(),
            [Arg::text("Block"), Arg::text("core"), Arg::text("bad")].as_slice()
        );
        // 紧凑构造器里 contentId 传的是 null
        assert_eq!(issue.content_id(), None);
        assert!(issue.is_error());
    }

    #[test]
    fn warning_factory_is_not_error_and_has_no_content_id() {
        let issue = Issue::warning("validate.some.warning", &["a"]);
        assert_eq!(issue.level(), Level::Warning);
        assert!(!issue.is_error());
        assert_eq!(issue.content_id(), None);
    }

    #[test]
    fn on_factories_keep_the_content_id() {
        let e = Issue::error_on("duo-payload", "validate.name.duplicate", &["Block", "duo"]);
        assert!(e.is_error());
        assert_eq!(e.content_id(), Some("duo-payload"));
        let w = Issue::warning_on("duo-payload", "validate.name.duplicate", &["Block", "duo"]);
        assert!(!w.is_error());
        assert_eq!(w.content_id(), Some("duo-payload"));
        assert_eq!(w.args(), [Arg::text("Block"), Arg::text("duo")].as_slice());
    }

    #[test]
    fn varargs_with_no_argument_gives_an_empty_list() {
        // Java 的 `Issue.error("k")` → Arrays.asList() 为空表
        let issue = Issue::error("validate.name.duplicate", &[] as &[&str]);
        assert!(issue.args().is_empty());
    }

    #[test]
    fn args_is_a_copy_of_the_caller_list() {
        // 对应 Java「`new ArrayList<>(args)` 而不是包装原表」：外部改动不得穿透进 Issue
        let mut external: Vec<String> = vec!["Block".to_string(), "duo".to_string()];
        let issue = Issue::new(Level::Error, "validate.name.duplicate", &external, None);
        external.push("extra".to_string());
        external[0] = "Wall".to_string();
        assert_eq!(issue.args(), [Arg::text("Block"), Arg::text("duo")].as_slice());
    }

    #[test]
    fn message_fills_an_existing_english_template() {
        // 真实存在于 messages.properties 的键，含两个占位符
        let issue = Issue::error("validate.name.duplicate", &["Block", "duo"]);
        assert_eq!(
            issue.message(),
            "two Block records are named 'duo'; the game throws on duplicate content names"
        );
    }

    #[test]
    fn message_of_unknown_key_is_the_key_itself() {
        let issue = Issue::warning("validate.this.key.does.not.exist", &["x"]);
        assert_eq!(issue.message(), "validate.this.key.does.not.exist");
    }

    #[test]
    fn template_without_arguments_renders_unchanged() {
        // Java: TextTemplate.fill(pattern, List.of()) 直接返回 pattern，占位符保持原样
        let issue = Issue::error("validate.type.notFileBacked", &[] as &[&str]);
        assert_eq!(
            issue.message(),
            "{0} '{1}' cannot be a file: the game has no parser for content/{2}"
        );
    }

    #[test]
    fn multiline_value_is_decoded_by_the_properties_parser() {
        // `about.body` 的值里带 `\n` 转义：走 PropertiesFile 解码后应拿到真实换行
        let issue = Issue::error("about.body", &[] as &[&str]);
        let text = issue.message();
        assert!(text.contains('\n'), "expected a decoded newline in {text:?}");
        assert!(!text.contains("\\n"));
    }

    #[test]
    fn null_argument_is_an_empty_string() {
        // Java `TextTemplate.fill` 在 `String.valueOf` 前判空，null 渲染成空串；Rust 侧由
        // [`Arg::Null`] 承载同一个语义（见 [`Arg::text_of`]）。
        let issue = Issue::error(
            "validate.name.invalid",
            &[Arg::text("Block"), Arg::text("core"), Arg::Null],
        );
        assert_eq!(
            issue.message(),
            "Block 'core': "
        );
    }
}
