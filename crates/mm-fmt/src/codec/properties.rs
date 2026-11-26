//! `PropertiesFile.java` 的等价移植：Java `.properties`（`bundles/<name>.properties`）读写。
//!
//! 游戏用 `PropertiesUtils.load`（Mods.java:651-663）把 bundle 叠加合并，且原版 bundle 以裸 UTF-8
//! 发布，因此值原样以 UTF-8 写出，只转义结构上必需的字符。
//!
//! bundle 同时也是一份手工编辑的文档——示例模组的 bundle.properties 带 62 行注释——所以编辑遵循
//! 文件本身而不是重建它。解析保留每一源行，改动的键只重写自己那一行（保留键拼写、分隔符、其周围
//! 空格与行尾），未触碰的行逐字节输出，新键追加，消失的键丢弃其行。解析后不改直接写回即得原文。

use mm_schema::KeyedMap;

/// Java `Character.isWhitespace`：ASCII 空白与控制分隔符，加上 Unicode 空格/分隔符类别，
/// 但**不含** U+00A0 不间断空格（bundle 里用它做视觉对齐，必须留在值内）与 U+0085 NEL。
pub(crate) fn is_ws(c: char) -> bool {
    if c == '\u{a0}' || c == '\u{85}' {
        return false;
    }
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
        || matches!(c, '\u{1c}'..='\u{1f}')
        || c == ' '
        || c.is_whitespace()
}

/// Java `String.stripLeading()`。
fn strip_leading(s: &str) -> &str {
    s.trim_start_matches(is_ws)
}

/// Java `String.trim()`（按 `c <= ' '` 判定，与 `strip` 不同）。
fn trim_ascii(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// 一个逻辑源行：原文连同其后继行尾，外加（对定义键的行）只重写值所需的全部信息。
#[derive(Debug, Clone)]
struct Slice {
    /// 文件里出现的该行文本，续行也并入
    raw: String,
    /// 跟随其后的行尾（没有行尾的最后一行为空串）
    terminator: String,
    /// 值之前的一切：缩进、键拼写、分隔符、空格
    prefix: Option<String>,
    /// 该行是否有显式的 `=` 或 `:`（裸键没有）
    separated: bool,
    key: Option<String>,
    value: Option<String>,
}

/// Java `.properties` 的读取/写入器。
#[derive(Debug, Clone, Default)]
pub struct PropertiesFile {
    /// 解析出的值，按文件定义顺序；之后新增的键追加在末尾。
    pub values: KeyedMap<String>,
    /// 读取时发现的非致命问题：游戏会拒绝的转义与前导 BOM。
    pub problems: Vec<String>,
    /// 按顺序保存的源行，使文档自身结构能在编辑后存活。
    source: Vec<Slice>,
    /// 源文本以之开头的字节序标记，按原样重新写出。
    leading: String,
}

impl PropertiesFile {
    pub fn parse(text: &str) -> PropertiesFile {
        let mut file = PropertiesFile::default();
        file.read(text);
        file
    }

    /// 设置一个值；文件已定义的键就地重写，新键追加。
    pub fn put(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values.put(key, value.into());
    }

    /// 连同定义它的行一起删除一个键。
    pub fn remove(&mut self, key: &str) {
        self.values.remove(key);
    }

    /// 文件的文本形式：未触碰的行逐字，改动的值重写，新键追加。
    pub fn write(&self) -> String {
        let mut out = self.leading.clone();
        let mut last_of_key: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for (i, slice) in self.source.iter().enumerate() {
            if let Some(key) = &slice.key {
                last_of_key.insert(key, i);
            }
        }
        let mut written: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (i, slice) in self.source.iter().enumerate() {
            let Some(key) = &slice.key else {
                out.push_str(&slice.raw);
                out.push_str(&slice.terminator);
                continue;
            };
            written.insert(key.as_str());
            let Some(value) = self.values.get(key) else {
                continue; // 已删除：定义它的每一行都消失
            };
            let effective = last_of_key.get(key.as_str()) == Some(&i);
            if !effective || Some(value.as_str()) == slice.value.as_deref() {
                // 游戏本来就会覆盖的前面重复项，或未被改动的值：该行按传入时原样输出。
                out.push_str(&slice.raw);
                out.push_str(&slice.terminator);
                continue;
            }
            out.push_str(slice.prefix.as_deref().unwrap_or(""));
            if !slice.separated {
                out.push('=');
            }
            out.push_str(&escape_value(value));
            out.push_str(&slice.terminator);
        }
        for (k, v) in self.values.iter() {
            if written.contains(k.as_str()) {
                continue;
            }
            if !out.is_empty() && !ends_with_line_break(&out) {
                if ends_with_odd_backslashes(&out) {
                    // 最后一个逻辑行以续行反斜杠结尾。把新键直接跟在它后面会让键成为那个值的一部分
                    // （PropertiesUtils.load 会把它们连接起来），所以先把悬空反斜杠变成字面量：
                    // 再加一个反斜杠既保住值的含义，又结束该行。
                    out.push('\\');
                }
                out.push('\n');
            }
            out.push_str(&escape_key(k));
            out.push('=');
            out.push_str(&escape_value(v));
            out.push('\n');
        }
        out
    }

    // --- 读取 ---------------------------------------------------------------------------------

    fn read(&mut self, text: &str) {
        let mut body = text;
        if let Some(rest) = text.strip_prefix('\u{feff}') {
            self.leading = "\u{feff}".to_string();
            body = rest;
            // 游戏的加载器按原样读字节，于是 BOM 在那里落进第一个键的名字里，该条目在游戏内失效；
            // 而本读取器看到的是一个干净的键。BOM 在写回时保留（逐字节规则），由作者自行移除。
            self.problems.push(
                "the file starts with a UTF-8 BOM: the game keeps it inside the first key's name, so that key is unreachable in game"
                    .to_string(),
            );
        }

        let chars: Vec<char> = body.chars().collect();
        let mut raws: Vec<String> = Vec::new();
        let mut endings: Vec<String> = Vec::new();
        let mut i = 0usize;
        let mut line_start = 0usize;
        while i < chars.len() {
            let c = chars[i];
            if c != '\n' && c != '\r' {
                i += 1;
                continue;
            }
            let ending: &str = if c == '\r' && i + 1 < chars.len() && chars[i + 1] == '\n' {
                "\r\n"
            } else if c == '\r' {
                "\r"
            } else {
                "\n"
            };
            raws.push(chars[line_start..i].iter().collect());
            endings.push(ending.to_string());
            i += ending.chars().count();
            line_start = i;
        }
        if line_start < chars.len() {
            raws.push(chars[line_start..].iter().collect());
            endings.push(String::new());
        }

        // 反斜杠续行把若干物理行合成一个逻辑行。精确字节被保留以便重新输出；被解析的是连接后的
        // 文本（转义与换行已去除）。java.util.Properties 和游戏的 PropertiesUtils 从不续行注释行，
        // 所以那里的行尾反斜杠是字面文本并结束该逻辑行。
        let mut raw = String::new();
        let mut joined = String::new();
        let mut comment = false;
        for (idx, part) in raws.iter().enumerate() {
            raw.push_str(part);
            if joined.is_empty() {
                let head = strip_leading(part);
                comment = matches!(head.chars().next(), Some('#') | Some('!'));
                joined.push_str(part);
            } else {
                joined.push_str(strip_leading(part));
            }
            if ends_with_odd_backslashes(part) && !comment {
                raw.push_str(&endings[idx]);
                joined.pop();
                continue;
            }
            self.slice(raw.clone(), &joined, endings[idx].clone());
            raw.clear();
            joined.clear();
        }
        if !joined.is_empty() {
            self.slice(raw, &joined, String::new());
        }
    }

    /// 记录一个逻辑行：注释与空行原样，键行拆出其值。
    fn slice(&mut self, raw: String, text: &str, terminator: String) {
        let chars: Vec<char> = text.chars().collect();
        let body = strip_leading(text);
        let body_chars = body.chars().count();
        if body.is_empty() || matches!(body.chars().next(), Some('#') | Some('!')) {
            self.source.push(Slice {
                raw,
                terminator,
                prefix: None,
                separated: false,
                key: None,
                value: None,
            });
            return;
        }
        let key_start = chars.len() - body_chars;
        let separator = find_separator(&chars, key_start);
        let after_key = separator.unwrap_or(chars.len());
        let mut i = after_key;
        while i < chars.len() && is_ws(chars[i]) {
            i += 1;
        }
        let separated = i < chars.len() && matches!(chars[i], '=' | ':');
        if separated {
            i += 1;
        }
        let value: String = chars[i..].iter().collect();
        let value = strip_leading(&value).to_string();
        let prefix: String = chars[..chars.len() - value.chars().count()].iter().collect();
        let key_src: String = chars[key_start..after_key].iter().collect();
        let mut problems = std::mem::take(&mut self.problems);
        let key = decode(trim_ascii(&key_src), "bundle key", &mut problems);
        let where_ = format!("bundle value for '{key}'");
        let decoded = decode(&value, &where_, &mut problems);
        self.problems = problems;
        self.source.push(Slice {
            raw,
            terminator,
            prefix: Some(prefix),
            separated,
            key: Some(key.clone()),
            value: Some(decoded.clone()),
        });
        self.values.put(key, decoded);
    }
}

fn ends_with_odd_backslashes(line: &str) -> bool {
    let mut backslashes = 0usize;
    for c in line.chars().rev() {
        if c != '\\' {
            break;
        }
        backslashes += 1;
    }
    backslashes % 2 == 1
}

fn ends_with_line_break(text: &str) -> bool {
    matches!(text.chars().last(), Some('\n') | Some('\r'))
}

fn find_separator(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if matches!(c, '=' | ':') || is_ws(c) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 供外部使用的无告警解码（Java 的包级 `unescape(String)`，problems 传 null）。
pub fn unescape(text: &str) -> String {
    decode(text, "bundle", &mut Vec::new())
}

/// 按文件的 problems 列表读入一段文本，使无法修复的转义被报告出来。
fn decode(text: &str, where_: &str, problems: &mut Vec<String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c != '\\' || i + 1 >= chars.len() {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let next = chars[i];
        match next {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'f' => out.push('\u{c}'),
            'u' => match unicode_escape(&chars, i) {
                Some(code) => {
                    out.push(code);
                    i += 4;
                }
                None => {
                    // 游戏的 PropertiesUtils.load 对不足四位十六进制的 `\u` 抛异常，随后
                    // Mods.buildFiles 丢掉整个 bundle 文件，于是其中每个字符串都静默回落。
                    // 这里保留字面文本——文件仍保有它的字节——但要说明，而不是悄悄解码成别的值。
                    problems.push(format!(
                        "{where_}: invalid \\u escape (four hex digits are required); the game rejects the whole file"
                    ));
                    // 反斜杠也要保留：解码后的值必须是转义标记完整的字面文本，而不是标记已消失的
                    // "uZZZZ"。
                    out.push('\\');
                    out.push(next);
                }
            },
            _ => out.push(next),
        }
        i += 1;
    }
    out
}

/// 位于 `u` 处的 `\uXXXX` 转义所表示的字符，格式不正确时返回 `None`。
fn unicode_escape(chars: &[char], u: usize) -> Option<char> {
    if u + 4 >= chars.len() {
        return None;
    }
    let hex: String = chars[u + 1..u + 5].iter().collect();
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let code = u32::from_str_radix(&hex, 16).ok()?;
    // Java 在这里追加一个 UTF-16 码元；代理区外的值与 Rust `char` 一一对应。孤立的代理码元
    // 无法在 Rust 字符串里表示，退化为替换字符。
    char::from_u32(code).or_else(|| char::from_u32(0xFFFD))
}

/// 转义一个键。键里每一个空白字符都是结构性的：加载器和本读取器都在第一个空白处结束键，
/// 所以未转义的内部空格会把 "a b" 变成键 "a"、值 "b=c"。
pub fn escape_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for c in key.chars() {
        let structural =
            matches!(c, '\\' | '=' | ':' | '#' | '!') || is_ws(c);
        if structural {
            out.push('\\');
        }
        out.push(match c {
            '\n' => 'n',
            '\r' => 'r',
            '\t' => 't',
            '\u{c}' => 'f',
            _ => c,
        });
    }
    out
}

/// 转义一个值：只有结构上必需的字符被转义。
pub fn escape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, c) in value.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => {
                if i == 0 && matches!(c, ' ' | '#' | '!') {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    out
}
