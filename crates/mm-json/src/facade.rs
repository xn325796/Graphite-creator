//! 对应 Java `core.json.Json` 静态门面（`dev.modmaker.core.json.Json`）。
//!
//! **注意与 `writer::write` 不是一条通道**：Java 侧 `HjsonWriter` 负责写出 HJSON（无引号键、
//! 根对象不带花括号），而本模块的 [`write`] 复刻的是 Gson 那条
//! `new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create()`：标准 JSON、键一律带
//! 引号、2 空格缩进、`"key": value`。`ProjectIo` 写 `mod.json`、`modmaker/project.json`、
//! `modmaker/boards/*.canvas` 走的是后者，逐字节 golden 往返比对依赖这里的格式。
//!
//! 下面的格式规则不是凭 Gson 文档记忆写的，是拿本工程实际依赖的 gson-2.11.0.jar（见 Java 仓库
//! `build.gradle.kts`）在本机 Java 27 上实测出来的，实测要点：
//! - 缩进 2 空格/层；非空对象 `{\n` + 成员 + `\n` + 回退缩进 `}`；数组同理；成员间 `,` 后换行。
//! - 空对象/空数组一律同行 `{}` / `[]`（顶层空容器输出长度就是 2，没有换行）。
//! - `Gson.toJson(...)` 结尾**不**追加换行。
//! - `disableHtmlEscaping()` 下 `< > & = ' /` 原样输出；转义只有 `\"` `\\` 与
//!   `\b \t \n \f \r`，其余 `< 0x20` 走 `\u00xx`（**小写十六进制**）；另外 U+2028/U+2029 会被转义
//!   成 `\u2028` `\u2029`（Gson 2.11 实测），而非 ASCII 字符（含 é、中文、代理对 emoji）与 U+007F、
//!   U+00A0 都原样输出。
//! - 数字是「原样落纸」：Gson 把解析得到的字面量文本直接交给 writer，`10.0`、`1e3`、`0.50`、
//!   `12345678901234567890`、`-0.0`、`1E+3` 都不改写。本工程的 [`Json::Num`] 存的就是原文，
//!   所以这里只做字符串拼接，绝不 parse 再 format。
//! - **`write` 与 `write_compact` 对 null 的处理不同**：Gson 默认 `serializeNulls = false`，
//!   `Gson.toJson(JsonElement)` 因此整条跳过值为 null 的**对象成员**（数组元素里的 null 照写）；
//!   而 `JsonElement.toString()`（= [`write_compact`]）内部显式开了 serializeNulls，null 成员保留。
//!   这是 `ProjectIo` 逐字节 golden 比对最容易踩的一条，见 [`write`] 里的注释与测试。
//!
//! Java 的 `write(Object)`（对 POJO 反射序列化）在 Rust 侧没有对应物，也不被 `ProjectIo` 使用，
//! 故不移植。

use std::io::Error;
use std::path::Path;

use crate::reader::{self, HjsonError};
use crate::value::{Json, Object};

/// Java `Json.readFile(Path)`：UTF-8 读入后去掉开头的 BOM——真实模组文件确实带 BOM。
pub fn read_file(file: &Path) -> Result<String, Error> {
    let text = std::fs::read_to_string(file)?;
    Ok(reader::strip_bom(&text).to_string())
}

/// Java `Json.stripBom(String)`：复用 `reader::strip_bom`（BOM 在 UTF-8 里是 3 字节，按字符剥）。
pub use crate::reader::strip_bom;

/// Java `Json.writeFile(Path, String)`：先建父目录再写 UTF-8。
///
/// Java 这里用的是 `Files.writeString`，**不是**原子写（没有先写临时文件再 rename），
/// 所以忠实照搬，不要自作主张改成原子写。
pub fn write_file(file: &Path, text: &str) -> Result<(), Error> {
    // Java 是 `Files.createDirectories(file.toAbsolutePath().getParent())`。
    if let Some(parent) = file.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, text)
}

/// Java `Json.parse(String)`：HJSON 容错读（`HjsonReader.read`）。
pub fn parse(text: &str) -> Result<Json, HjsonError> {
    reader::read(text)
}

/// Java `Json.parseObject(String)`：顶层必须是对象，否则报 `Expected a JSON object`。
pub fn parse_object(text: &str) -> Result<Object, HjsonError> {
    reader::read_object(text)
}

/// Java `Json.parseStrictObject(String)`：给「本程序自己写出去的项目文件」用。
///
/// Java 侧走的是 `JsonParser.parseString(text).getAsJsonObject()`。实测 gson-2.11.0：
/// `JsonParser` 会把 `JsonReader` 设为 lenient，所以它其实**也接受**无引号键、单引号、`//` 与 `#`
/// 注释，只拒绝尾随逗号与「根对象不带花括号」；而 `read_json_object` 这条 Hjson 通道对前几项同样
/// 接受，只是更宽松（尾随逗号、无括号根也放行）。两侧对本门面真正的输入（[`write`] 写出的标准
/// JSON）结果一致，故复用 `reader::read_json_object`；差异只出现在非法输入上，见测试里的说明。
pub fn parse_strict_object(text: &str) -> Result<Object, HjsonError> {
    reader::read_json_object(text)
}

/// Java `Json.write(JsonElement)`：Gson pretty printing + `disableHtmlEscaping()`。
pub fn write(element: &Json) -> String {
    let mut out = String::new();
    write_pretty(element, 0, &mut out);
    out
}

/// Gson `JsonElement.toString()` 的等价物：紧凑输出（无缩进、`"key":value` 冒号后**没有**空格）。
pub fn write_compact(element: &Json) -> String {
    let mut out = String::new();
    write_value(element, &mut out);
    out
}

/// 每层 2 空格，与实测的 `setPrettyPrinting()` 一致。
fn indent(level: usize) -> String {
    " ".repeat(level * 2)
}

fn write_pretty(value: &Json, level: usize, out: &mut String) {
    match value {
        Json::Obj(object) => {
            // Gson 默认 `serializeNulls = false`，而 `Gson.toJson(JsonElement)` 用的是这份设置：
            // pretty 通道会**整条跳过值为 null 的对象成员**（键也没了），成员全为 null 时输出 `{}`。
            // 数组里的 null 不受影响（实测 `[null,null]` → 两行 `null`），紧凑通道
            // （[`write_compact`]，对应 `JsonElement.toString()`，它显式 `setSerializeNulls(true)`）
            // 则照写 `"z":null`。实测：`{"a":null,"b":1}` → pretty `{\n  "b": 1\n}`、
            // compact `{"a":null,"b":1}`。
            let members: Vec<(&String, &Json)> =
                object.iter().filter(|(_, v)| !v.is_null()).collect();
            if members.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.push_str(&indent(level + 1));
                write_string(key, out);
                out.push_str(": ");
                write_pretty(item, level + 1, out);
            }
            out.push('\n');
            out.push_str(&indent(level));
            out.push('}');
        }
        Json::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.push_str(&indent(level + 1));
                write_pretty(item, level + 1, out);
            }
            out.push('\n');
            out.push_str(&indent(level));
            out.push(']');
        }
        other => write_value(other, out),
    }
}

/// 标量与紧凑通道的容器：数字原样、布尔 `true`/`false`、null → `null`。
fn write_value(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(number) => out.push_str(number),
        Json::Str(text) => write_string(text, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Json::Obj(object) => {
            out.push('{');
            for (i, (key, item)) in object.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_value(item, out);
            }
            out.push('}');
        }
    }
}

/// Gson `JsonWriter` 在 `disableHtmlEscaping()` 下的字符串写法（转义集合见模块头实测记录）。
fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
