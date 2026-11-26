//! P5-M4 交付的那条**前端可机检断言**（方案文档 §一·变坏 2 第 60 行、§四 M4 第 274 行）：
//! `src-vue/` 下不得出现领域词，作为 Java `LayeringTest` 思路的前端等价物。
//!
//! Java 那份（`Mindustry-modmaker/src/test/java/dev/modmaker/LayeringTest.java`，74 行）扫的是
//! 源码树里的 `import javafx` 与 `import dev.modmaker.ui` 两根针，防的是「分层只是一句注释」。
//! P5 要防的是同一件事的另一面：方案文档 §一·变坏 2 说的——一旦前端图快自己 `JSON.parse`
//! 一份 HJSON、自己判一遍 `consumes`，`crates/` 的字节保真就再也管不住它，而项目里
//! **现在没有一个前端测试**，所以这条只能靠扫描器钉住。
//!
//! ## 与 Java 那份的两处刻意差异（不是遗漏）
//!
//! 1. **没有 `Assumptions.assumeTrue` 兜底**。Java 那边源树可能在 shaded jar 里不可见，找不到
//!    目录就跳过；这里 `src-vue/` 与测试文件同仓、必定存在，所以找不到目录就是**失败**。
//!    另外加了一条「扫到 0 个文件即失败」的自检（见 [`collect`]）：扫不到东西同样是假绿，
//!    宁可红也不绿得没内容。
//! 2. **命中时报出文件、行号与整行**（Java 只 `file.getFileName()`）。前端文件少，
//!    多给这一眼能省掉一轮复现。
//!
//! ## 这条断言的边界（别把它当成它做不到的事）
//!
//! > **2026-11-17 迁移**：扫描根从 `src-vue/` 改为 `src/`——新壳（GraphiteCreator）的前端
//! > 目录是模板自带的 `src/`，断言本身不变；上文对 `src-vue/` 的表述保留为历史口径。
//!
//! 它是**纯文本针**，跟 Java 一样拦不住改名的变量、拼出来的字符串、或换了拼写的同义概念。
//! 它钉住的是「领域词以本来面目出现在前端源码里」这一种最可能发生、也最容易被 review 漏掉的
//! 走法；真正的字节契约仍由 `crates/` 的测试守着。这一点写在这里，是为了让「测试通过」不被
//! 读成「前端已经证明不算领域逻辑」。

use std::fs;
use std::path::{Path, PathBuf};

/// 扫描的扩展名：前端源码只有这几类（`src-vue/` 里没有 `.ts`，M5 若引入也自动落进覆盖面）。
const EXTENSIONS: [&str; 6] = [".vue", ".js", ".mjs", ".cjs", ".jsx", ".ts"];

/// 不进扫描树的目录名。`dist/` 与 `node_modules/` 里必然带着这些词（打包产物含依赖全文），
/// 扫它们等于永远红，且红得没有信息量。
const SKIP_DIRS: [&str; 3] = ["node_modules", "dist", ".git"];

/// 一处命中：文件、行号、整行文本。
struct Hit {
    file: PathBuf,
    line: usize,
    text: String,
}

/// 前端源码根目录（相对 package 根 `src-tauri/`，cargo 把集成测试的 CWD 设成 package 根）。
fn frontend_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../src")
}

/// 递归收出所有要扫的文件。目录不存在时返回 `None`，由调用方判失败（见模块头第 1 条差异）。
fn sources(root: &Path, out: &mut Vec<PathBuf>) -> Option<()> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            sources(&path, out)?;
            continue;
        }
        let matched = EXTENSIONS.iter().any(|ext| name.ends_with(*ext));
        if matched {
            out.push(path);
        }
    }
    Some(())
}

/// 在文件里找针：命中收集为 [`Hit`]。整行原样带着，方便一眼看出是不是注释误伤。
fn find_needles(files: &[PathBuf], needles: &[&str]) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    for file in files {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(_) => {
                hits.push(Hit { file: file.clone(), line: 0, text: "<读取失败>".to_string() });
                continue;
            }
        };
        for (index, line) in text.lines().enumerate() {
            // 2026-11-20：`hjson` 一枚针改看**代码**，不再看注释与引号字面量。
            // 背景：`project_convert_format(ext)` 的契约把 `"hjson"` 变成了合法的命令参数，
            // 前端因此必须写下这个字符串；文件选择提示里也必然出现 `mod.hjson` 这个文件名。
            // 这条断言要防的从来不是「字母组合出现」，而是「前端自己解析 HJSON」。
            // 其余两枚针（`consumes` / `requirements`）口径不变，注释里出现照样算命中。
            for needle in needles {
                let haystack = if *needle == "hjson" {
                    code_without_format_token(line)
                } else {
                    line.to_string()
                };
                if haystack.contains(*needle) {
                    hits.push(Hit {
                        file: file.clone(),
                        line: index + 1,
                        text: format!("{needle} ← {}", line.trim()),
                    });
                }
            }
        }
    }
    hits
}


/// 一行里属于「代码」的部分：去掉 `//` 行注释与 `/* … */` 段注释，再把引号里的
/// `hjson` 字面量抹成空格（它现在是合法的命令参数）。
///
/// 这是**只给 `hjson` 一枚针用**的窄口径：多行块注释按「上一行未闭合」不跨行跟踪——
/// 漏掉一条注释只会让断言偏向误报，不会放过真正的解析代码。
fn code_without_format_token(line: &str) -> String {
    let mut code = String::with_capacity(line.len());
    let bytes: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        let next = bytes.get(index + 1).copied();
        if current == '/' && next == Some('/') {
            break;
        }
        if current == '/' && next == Some('*') {
            index += 2;
            while index < bytes.len() {
                if bytes[index] == '*' && bytes.get(index + 1) == Some(&'/') {
                    index += 2;
                    break;
                }
                index += 1;
            }
            code.push(' ');
            continue;
        }
        code.push(current);
        index += 1;
    }
    code.replace("\"hjson\"", " ").replace("'hjson'", " ")
}

/// 取到文件清单，并当场钉住两件事：目录必须在、必须真扫到东西。
fn collect() -> Vec<PathBuf> {
    let root = frontend_root();
    assert!(root.is_dir(), "找不到前端源码目录 {}，这条分层断言没有扫任何东西", root.display());
    let mut files: Vec<PathBuf> = Vec::new();
    assert!(
        sources(&root, &mut files).is_some(),
        "扫描 {} 时读目录失败",
        root.display()
    );
    assert!(!files.is_empty(), "扫描了 {} 却没收到一个前端源文件，针法失效", root.display());
    files
}

/// 命中时把文件（相对 `src-vue/`）、行号与整行打出来。
fn report(hits: &[Hit]) -> String {
    let root = frontend_root();
    hits.iter()
        .map(|hit| {
            let shown = hit.file.strip_prefix(&root).unwrap_or(&hit.file);
            format!("\n  {}:{}  {}", shown.display(), hit.line, hit.text)
        })
        .collect::<Vec<String>>()
        .join("")
}

/// 对应 §一·变坏 2 第 60 行点名的那批领域词：`hjson|consumes|requirements|\.hjson`。
///
/// 这三个词只属于 `crates/mm-json`（HJSON 解析）与 `crates/mm-validate`（`consumes` 规则），
/// 前端读到的是它们算完的描述符。所以**注释里也不算豁免**：要在前端解释什么，就用不含这些词的
/// 话说，解释本身留在 Rust 侧。
#[test]
fn frontend_does_not_name_domain_field_rules() {
    let files = collect();
    let hits = find_needles(&files, &["hjson", "consumes", "requirements"]);
    assert!(
        hits.is_empty(),
        "src/ 出现了领域词，字节契约与字段规则正在往前端漏：{}{}",
        report(&hits),
        "\n（这些词只能出现在 crates/ 侧；前端只画不算。）"
    );
}

/// 对应 §一·变坏 2 第 60 行前半句：「前端只允许出现 `invoke(...)` 与 `listen(...)` 两类跨界调用」。
/// 任何一条绕过壳的通道（自己发 HTTP、自己开 socket）都等于把领域逻辑搬回前端。
#[test]
fn frontend_does_not_bypass_the_shell() {
    let files = collect();
    let hits = find_needles(&files, &["fetch(", "XMLHttpRequest", "new WebSocket"]);
    assert!(
        hits.is_empty(),
        "src/ 出现了 invoke/listen 之外的跨界通道：{}",
        report(&hits)
    );
}

/// 对应 §一·变坏 2 第 58-59 行的依赖豁免：runtime 只有 `vue` 与 `@tauri-apps/api` 两个，
/// 且**不引入** Pinia / Vue Router / Vitest。这里扫源码里的 import 目标；
/// `@tauri-apps/plugin-*` 是豁免面扩大的第一个信号（P5-Q3 已经把 `plugin-opener` 两侧删净，
/// 这条针防止它以任何形式回来）。
#[test]
fn frontend_imports_only_the_exempted_packages() {
    let files = collect();
    let hits = find_needles(
        &files,
        &["@tauri-apps/plugin-", "from \"pinia\"", "from \"vue-router\"", "from \"vitest\""],
    );
    assert!(
        hits.is_empty(),
        "src/ 引入了豁免面之外的依赖：{}",
        report(&hits)
    );
}
