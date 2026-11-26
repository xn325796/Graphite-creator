//! 脚本类推断：扫 `<project>/scripts/**/*.js` 里的 `extend(<Class>, '<name>')`，得到
//! 「内容名 → 类名」表，落在 [`ModProject::script_classes`]。
//!
//! ## 为什么要有这个模块（与 Java 的偏离，2026-11-26 产品评审 R-02 授权的新设计）
//!
//! Java 版 modmaker（`dev.modmaker.core`）只认内容 JSON 里的 `type`：没有 `type` 就按类型的
//! 默认类（方块是 `Block`）解析字段表。真实模组不是这么写的——2026-11-26 评审实测，样本模组
//! 184 个方块里有 115 个**故意不写** `type`，类由脚本 `extend(PayloadBlock, '载荷发射台', {})`
//! 决定；只按 JSON 解析会让这 115 个方块显示 `Block` 的字段集（实测 `载荷发射台` 拿到 189 个
//! `Block` 字段，而 `payload` 这类 `PayloadBlock` 字段一个都没有）。
//!
//! 这是 Java 侧**没有的能力**，因此属于授权的新设计而不是移植：Java 的 `ProjectIo` 里没有
//! 这一步，也没有任何「读脚本」的代码。设计边界刻意保守：
//!
//! 1. **只做静态文本匹配**，不执行脚本、不建 JS 引擎。匹配规则与
//!    `extend` + 可选空白 + `(` + 类标识符 + `,` + 引号字面量（1..=64 字符）等价。
//! 2. **不覆盖动态名字**。`extend(GenericCrafter, name, {...})` 里的名字是变量，
//!    静态扫描给不出答案——这类方块保持「未知」，由 `mm-validate` 的 `scriptClass` 规则提醒。
//! 3. **不裁决歧义**。同名出现在多处且类不同时取**字典序第一处**（见 [`scan_script_classes`]），
//!    只保证确定性，不保证语义最优——真正的裁决需要求值脚本，超出本工具的能力边界。
//! 4. **结论不进模型字节**。[`ModProject::script_classes`] 只活在内存里，保存路径不写它；
//!    推断结果也**不得**回填到 `ContentRecord::class_name()`（那是磁盘上的 `type`），
//!    否则一次打开项目就会改写用户的模组文件。
//!
//! [`ModProject::script_classes`]: mm_model::ModProject::script_classes

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 脚本目录名（Mindustry 模组布局的约定）。
const SCRIPTS_DIR: &str = "scripts";
/// `extend` 关键字本身。
const KEYWORD: &str = "extend";
/// 名字字面量的长度上限（字符数）。真实内容名远短于此，超长的多半不是内容名。
const MAX_NAME_LEN: usize = 64;

/// 扫描 `root/scripts/**.js`，得到「内容名 → 类名」表；`scripts/` 不存在或读不出时返回空表。
///
/// 文件按**路径字典序**遍历，同名多次出现（且类不同）时取**第一处**——确定性优先，不做歧义
/// 裁决（模块头第 3 条）。同名同类重复出现无影响：`or_insert` 保留先到的那一条。
///
/// 类名保持脚本里的原文（可能带包名前缀，如 `mindustry.world.blocks.payloads.PayloadBlock`），
/// 归一化成 schema 类名是解析侧（`mm-app`）的事，本模块只管「脚本怎么写的」。
pub fn scan_script_classes(root: &Path) -> BTreeMap<String, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_scripts(&root.join(SCRIPTS_DIR), &mut files);
    files.sort();

    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for file in files {
        let Ok(bytes) = fs::read(&file) else {
            // 读不出（权限、被占用）就当这个文件没有脚本：扫描是尽力而为的旁证，不是校验。
            continue;
        };
        // 逻辑损失：非 UTF-8 的字节变成替换字符。语法真实的脚本是 UTF-8；这比整文件跳过更有用。
        let source = String::from_utf8_lossy(&bytes);
        for call in extend_calls(&source) {
            // 字典序遍历 + or_insert ⇒「同名取第一处」（模块头第 3 条）。
            found.entry(call.name).or_insert(call.class);
        }
    }
    found
}

/// 递归收集 `.js` 文件。`file_type()` 不跟随符号链接，环状链接不会把扫描拖进无限递归。
fn collect_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        // 没有 scripts/（或读不开）：空表，不报错——没有脚本是合法布局。
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            collect_scripts(&path, out);
        } else if kind.is_file() && is_script_file(&path) {
            out.push(path);
        }
    }
}

fn is_script_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
}

/// 一处 `extend(Class, 'name'` 的静态命中。
struct ExtendCall {
    name: String,
    class: String,
}

/// 剥掉注释后扫出所有命中。
fn extend_calls(source: &str) -> Vec<ExtendCall> {
    let text = strip_comments(source);
    let bytes = text.as_bytes();
    let mut out: Vec<ExtendCall> = Vec::new();
    let mut index = 0;
    while index + KEYWORD.len() <= bytes.len() {
        // `bytes[index] == b'e'` 是 ASCII，所以 `text[index..]` 一定落在字符边界上。
        if bytes[index] == b'e'
            && text[index..].starts_with(KEYWORD)
            && is_word_start(bytes, index)
            && let Some((class, name, next)) = parse_extend(&text, bytes, index + KEYWORD.len())
        {
            out.push(ExtendCall { name, class });
            index = next;
            continue;
        }
        index += 1;
    }
    out
}

/// 注释剥离的状态机：三种代码态（普通 / 行注释 / 块注释）与两种字符串态（引号内 / 转义）。
///
/// 换行在注释里被保留，让下游看到的行距与原文一致（便于以后加行号诊断）。
/// 反引号按字符串处理——模板字面量里的 `http://…` 不会被当成行注释；`${…}` 里的代码同样
/// 被当作字符串内容（简化，登记在模块头第 1 条「只做静态文本匹配」）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Code,
    LineComment,
    BlockComment,
    Quoted(char),
    Escaped(char),
}

fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut state = State::Code;
    while let Some(ch) = chars.next() {
        state = match state {
            State::Code => {
                if ch == '/' && chars.peek() == Some(&'/') {
                    chars.next();
                    State::LineComment
                } else if ch == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    State::BlockComment
                } else {
                    out.push(ch);
                    if ch == '\'' || ch == '"' || ch == '`' {
                        State::Quoted(ch)
                    } else {
                        State::Code
                    }
                }
            }
            State::LineComment => {
                if ch == '\n' {
                    out.push('\n');
                    State::Code
                } else {
                    State::LineComment
                }
            }
            State::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    State::Code
                } else {
                    if ch == '\n' {
                        out.push('\n');
                    }
                    State::BlockComment
                }
            }
            State::Quoted(quote) => {
                out.push(ch);
                if ch == '\\' {
                    State::Escaped(quote)
                } else if ch == quote {
                    State::Code
                } else {
                    State::Quoted(quote)
                }
            }
            State::Escaped(quote) => {
                out.push(ch);
                State::Quoted(quote)
            }
        };
    }
    out
}

/// `index` 处能否作为标识符形的词首：前面不是标识符字符（ASCII 字母数字、`_`、`$`）或非 ASCII
/// 字节（UTF-8 的续字节与多字节字符的首字节都 ≥ 0x80）。
fn is_word_start(bytes: &[u8], index: usize) -> bool {
    match index.checked_sub(1).and_then(|last| bytes.get(last)) {
        None => true,
        Some(&ch) => !(ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$' || ch >= 0x80),
    }
}

fn skip_whitespace(bytes: &[u8], index: &mut usize) {
    while matches!(
        bytes.get(*index),
        Some(b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
    ) {
        *index += 1;
    }
}

fn is_identifier_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_identifier_continue(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'.'
}

/// 从 `index`（紧跟 `extend` 之后）解析 `(Class, 'name'`，成功时返回 `(类名, 名字, 续扫位置)`。
fn parse_extend(text: &str, bytes: &[u8], mut index: usize) -> Option<(String, String, usize)> {
    skip_whitespace(bytes, &mut index);
    if bytes.get(index) != Some(&b'(') {
        return None;
    }
    index += 1;
    skip_whitespace(bytes, &mut index);

    // 类标识符：`[A-Za-z_][A-Za-z0-9_.]*`
    if !matches!(bytes.get(index), Some(&ch) if is_identifier_start(ch)) {
        return None;
    }
    let class_start = index;
    index += 1;
    while matches!(bytes.get(index), Some(&ch) if is_identifier_continue(ch)) {
        index += 1;
    }
    // 起点是 ASCII 标识符首字节，终点停在非标识符字节上，切片必落在字符边界。
    let class = text.get(class_start..index)?.to_string();
    skip_whitespace(bytes, &mut index);

    if bytes.get(index) != Some(&b',') {
        return None;
    }
    index += 1;
    skip_whitespace(bytes, &mut index);

    let quote = match bytes.get(index) {
        Some(&b'\'') => b'\'',
        Some(&b'"') => b'"',
        _ => return None,
    };
    index += 1;
    let name_start = index;
    while let Some(&ch) = bytes.get(index) {
        if ch == b'\\' {
            // 转义序列：跳过反斜杠与被转义的**字符**（可能是多字节），名字按原文收。
            index += 1;
            if let Some(next) = text.get(index..).and_then(|rest| rest.chars().next()) {
                index += next.len_utf8();
            }
            continue;
        }
        if ch == quote {
            break;
        }
        if ch == b'\n' {
            // 未闭合的字面量：宁可不认，也不把后面一整段代码当成名字。
            return None;
        }
        index += 1;
    }
    if bytes.get(index) != Some(&quote) {
        return None;
    }
    let name = text.get(name_start..index)?;
    if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
        return None;
    }
    Some((class, name.to_string(), index + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::scratch::temp_case;

    use std::fs;

    struct Case {
        root: PathBuf,
    }

    impl Case {
        fn new(tag: &str) -> Case {
            Case {
                root: temp_case(tag),
            }
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self
                .root
                .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("建父目录");
            }
            fs::write(&path, text).expect("写夹具脚本");
        }
    }

    impl Drop for Case {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 单/双引号、带包名前缀的类名、中文与短横线名字都要命中；变量名字面量不得命中。
    #[test]
    fn literal_names_are_collected_in_both_quote_styles() {
        let case = Case::new("scan-quotes");
        case.write(
            "scripts/main.js",
            concat!(
                "const pad = extend(LaunchPad, \"my-advanced-launch-pad\", {});\n",
                "const 载荷发射台 = extend(PayloadBlock, '载荷发射台', {});\n",
                "const qualified = extend(mindustry.world.blocks.payloads.PayloadBlock, '搬运舱', {});\n",
                "const dynamic = extend(GenericCrafter, someName, {});\n",
            ),
        );
        let found = scan_script_classes(&case.root);
        assert_eq!(
            found.get("my-advanced-launch-pad").map(String::as_str),
            Some("LaunchPad")
        );
        assert_eq!(
            found.get("载荷发射台").map(String::as_str),
            Some("PayloadBlock")
        );
        assert_eq!(
            found.get("搬运舱").map(String::as_str),
            Some("mindustry.world.blocks.payloads.PayloadBlock"),
            "包名前缀原样保留，归一化交给解析侧"
        );
        assert!(
            !found.values().any(|class| class.contains("GenericCrafter")),
            "变量名字面量不得命中"
        );
        assert_eq!(found.len(), 3);
    }

    /// 注释里的 `extend(...)` 一律不得计数：行注释、块注释、块注释里的行注释都要剥干净。
    #[test]
    fn extend_inside_comments_is_ignored() {
        let case = Case::new("scan-comments");
        case.write(
            "scripts/list.js",
            concat!(
                "// const dead = extend(GenericCrafter, '注释掉的', {});\n",
                "/*const SILEX = extend(GenericCrafter, 'SILEX', {});*/\n",
                "/*\n",
                " * extend(Wall, '块注释里的', {}); // 这行整段在注释里\n",
                " */\n",
                "const live = extend(StackConveyor, '活着的', {});\n",
            ),
        );
        let found = scan_script_classes(&case.root);
        assert_eq!(
            found.get("活着的").map(String::as_str),
            Some("StackConveyor")
        );
        assert!(!found.contains_key("注释掉的"));
        assert!(!found.contains_key("SILEX"));
        assert!(!found.contains_key("块注释里的"));
        assert_eq!(found.len(), 1);
    }

    /// 字符串里的 `//` 不是注释：剥离器不能把同一行后面的真调用一起吃掉。
    #[test]
    fn slashes_inside_strings_do_not_start_comments() {
        let case = Case::new("scan-strings");
        case.write(
            "scripts/main.js",
            "const url = 'http://example.invalid/x'; const pad = extend(LaunchPad, 'a-pad', {});\n",
        );
        let found = scan_script_classes(&case.root);
        assert_eq!(found.get("a-pad").map(String::as_str), Some("LaunchPad"));
    }

    /// 同名出现在两个文件、类不同：按路径字典序取第一处（`a.js` 先于 `b.js`）。
    #[test]
    fn duplicate_names_take_the_first_in_path_order() {
        let case = Case::new("scan-conflict");
        case.write("scripts/b.js", "const b = extend(Wall, '撞名', {});\n");
        case.write("scripts/a.js", "const a = extend(Drill, '撞名', {});\n");
        let found = scan_script_classes(&case.root);
        assert_eq!(found.get("撞名").map(String::as_str), Some("Drill"));
        assert_eq!(found.len(), 1);
    }

    /// 没有 `scripts/` 目录是合法布局：空表，不报错。
    #[test]
    fn missing_scripts_folder_yields_an_empty_map() {
        let case = Case::new("scan-missing");
        case.write("content/blocks/wall.json", "{}");
        assert!(scan_script_classes(&case.root).is_empty());
    }
}
