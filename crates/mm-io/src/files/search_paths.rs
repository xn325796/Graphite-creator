//! `SearchPaths.java` 的等价移植：定位那些住在项目目录之外的文件与文件夹——schema 扫描所依据的
//! Mindustry 源码树、游戏 jar、噪音端口用来比对的 arc jar、golden 测试与实时编辑探针所跑的真实模组。
//!
//! 每次查找都爬同一段短梯子，于是任意一台机器上的检出都能工作，而不依赖写死在源码里的某个绝对
//! 路径：先显式设置，再是常规工作区布局提供的相对候选，最后是工作目录周边文件夹的一次浅扫。
//! 什么都没命中时怎么办由调用方决定——工具带着一条消息失败，测试跳过。

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// 某个位置的显式覆盖：命中时返回绝对且已 normalize 的路径，未设置时返回 `None`。
///
/// **与 Java 的事实差异**：Java 先读 JVM 系统属性（`System.getProperty(property)`，也就是
/// `-Dmodmaker.gameJar=...` 这类命令行设置），为空时再读环境变量。系统属性这条通道在 Rust 侧
/// **不存在**——没有 JVM、也没有与之等价的进程内可注入属性表——配置只走环境变量。保留 `_property`
/// 形参（Java 的第一个参数）是为了与本 crate 后续移植的每一个 Java 调用点一一对应，读它没有意义，
/// 故命名带下划线前缀以免警告。
pub fn override_path(_property: &str, environment: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(environment);
    // 非 UTF-8 的极端环境值（Windows 上只有孤立代理项才会遇到）按「未设置」处理。
    override_from_value(raw.as_deref().and_then(OsStr::to_str))
}

/// [`override_path`] 的纯函数内核：候选原始值 → 命中的绝对规范路径。
///
/// 拆出来只为可测——真实的 [`std::env::var_os`] 通道见测试里的说明。
/// 空或全空白对应 Java 的 `String.isBlank()`，命中对应 `Path.of(value).toAbsolutePath().normalize()`。
fn override_from_value(value: Option<&str>) -> Option<PathBuf> {
    let value = value?;
    if value.trim().is_empty() {
        return None;
    }
    Some(normalize_absolute(Path::new(value)))
}

/// Gradle、工具和测试共同起步的工作目录。
///
/// **已知差异（移植后续类时必须记住）**：Java 下 `System.getProperty("user.dir")` 在 Gradle
/// 测试里是**项目根**；而 cargo 运行测试二进制时 cwd 是**crate 目录**（例如
/// `crates/mm-io`）。因此后面移植 `FixtureProject` 时，Java 里那些相对候选路径（如 `../mod`）
/// 都要按 cargo 的 cwd 重新核对，不能盲信 Java 的字面量。
///
/// Java 的 `user.dir` 总有值；[`std::env::current_dir`] 在 cwd 已被删除等情况下会失败，此时退回
/// 空路径，让调用方按「什么都没命中」处理，而不是让一次查找变成 panic。
pub fn working_directory() -> PathBuf {
    lexical_normalize(&std::env::current_dir().unwrap_or_else(|_| PathBuf::new()))
}

/// 第一个确实是目录的候选；一个都不是时返回 `None`。
pub fn first_directory(candidates: &[PathBuf]) -> Option<PathBuf> {
    for candidate in candidates {
        let normalized = normalize_absolute(candidate);
        if normalized.is_dir() {
            return Some(normalized);
        }
    }
    None
}

/// 第一个确实是普通文件的候选；一个都不是时返回 `None`。
pub fn first_file(candidates: &[PathBuf]) -> Option<PathBuf> {
    for candidate in candidates {
        let normalized = normalize_absolute(candidate);
        if normalized.is_file() {
            return Some(normalized);
        }
    }
    None
}

/// `matches` 在 `root` 及其之下、至多 `depth` 层里的最浅、名称优先的命中。确定性的：同一棵树，
/// 同一个答案——**搜索顺序是契约的一部分**，因为调用方（导入、schema 扫描）必须可复现。
///
/// 逐条对应 Java：`root` 为 None、`depth < 1`、`root` 不是目录 → `None`；收集
/// `Files.walk(root, depth)` 中所有满足谓词的路径（**包含 root 自身**，深度上限为 depth）；
/// 全部 `toAbsolutePath().normalize()` 后按 `getNameCount()` 升序、再按 `toString()` 升序排序，
/// 取第一个。读不了的目录被跳过而不中断遍历（Java 的 `Files.walk` 自 JDK 13 起的行为）；顶层就
/// 无法建立遍历（Java 的 IOException）→ `None`。
pub fn find(root: Option<&Path>, depth: i32, matches: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let root = root?;
    if depth < 1 || !root.is_dir() {
        return None;
    }
    let mut found = Vec::new();
    // Files.walk 的第一个元素就是 root 本身，因此它也参与谓词。
    if matches(root) {
        found.push(root.to_path_buf());
    }
    if walk(root, depth as usize, matches, &mut found).is_err() {
        return None;
    }
    let mut resolved: Vec<PathBuf> = found
        .into_iter()
        .map(|path| normalize_absolute(&path))
        .collect();
    // Java: sorted(comparingInt(getNameCount).thenComparing(toString)).findFirst()
    resolved.sort_by(|a, b| {
        name_count(a)
            .cmp(&name_count(b))
            .then_with(|| a.as_os_str().cmp(b.as_os_str()))
    });
    resolved.first().cloned()
}

/// 递归收集 `dir` 之下至多 `remaining` 层内满足谓词的路径。
///
/// 只有**本次** `read_dir` 的失败会返回 `Err`：顶层调用（`find` 里的那一次）据此对应 Java 的
/// 「流建立失败 → null」，而递归调用点把子目录的错误吞掉，对应 Java 跳过读不了的目录。
fn walk(
    dir: &Path,
    remaining: usize,
    matches: &dyn Fn(&Path) -> bool,
    found: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let entries = fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if matches(&path) {
            found.push(path.clone());
        }
        // 目录无论是否命中都要继续下钻；remaining > 1 保证不会越过 depth 层。
        if remaining > 1 && path.is_dir() && walk(&path, remaining - 1, matches, found).is_err() {
            // 子目录读不了：跳过，不中断整体遍历。
        }
    }
    Ok(())
}

/// Java 的 `Path.getNameCount()`：只数「名字段」，根成分不计入。
///
/// [`std::path::Path::components`] 的计数不能直接用：Windows 下 `C:\a\b` 在 Rust 里被拆成
/// `Prefix("C:")` + `RootDir` + `Normal("a")` + `Normal("b")` 共 4 节，而 Java 的根组件
/// （`C:\`）只算一个 root、name count 是 2；Unix 下 `/foo/bar` 两者同为 2。因此这里只数
/// [`Component::Normal`]，与 Java 逐节对齐。排序结果本身不受影响（同一次 `find` 的所有候选
/// 共享同一个 root 前缀，偏移量恒定），但计数保持一致便于与 Java 对照。
fn name_count(path: &Path) -> usize {
    path.components().filter(|component| matches!(component, Component::Normal(_))).count()
}

/// Java 的 `toAbsolutePath().normalize()`：先绝对化（不查文件系统），再按字面去掉 `.` 与冗余 `..`。
///
/// [`std::path::absolute`] 在取不到当前目录时返回错误，而 Java 的 `toAbsolutePath()` 从不抛
/// IOException，故此处退回原值（仍做一次词法 normalize，与 Java 的输出形态保持一致）。
///
/// **可见性放宽的理由**：`crate::fixture_project` 移植 `FixtureProject.asModRoot`
/// （`FixtureProject.java:85`、`:91` 的 `toAbsolutePath().normalize()`）必须复用这一个实现，
/// 以免出现第二份 normalize 语义；`mm_app::project_actions` 移植
/// `ProjectActions.readImport`（`ProjectActions.java:74-75` 的同一对调用）也要用它，
/// 故从 `pub(crate)` 放宽到 `pub`。
pub fn normalize_absolute(path: &Path) -> PathBuf {
    match std::path::absolute(path) {
        Ok(absolute) => lexical_normalize(&absolute),
        Err(_) => lexical_normalize(path),
    }
}

/// Java `Path.normalize()` 的等价实现（纯词法，绝不碰文件系统、绝不解析符号链接）。
///
/// 标准库在这里帮不上忙：rustc 1.99.1 上 `Path::normalize` 仍未稳定，而
/// [`std::fs::canonicalize`] 会解析符号链接、在 Windows 上还会加 `\\?\` 前缀，语义与 Java 差得更远。
/// 规则照抄 JDK：丢弃所有 `.`；`..` 吃掉前一个**普通名字段**；前一段本身就是 `..` 时把 `..` 留下；
/// 绝对路径在根之后无处可退时把 `..` 丢弃，相对路径则保留前导 `..`（`Paths.get("a/..")` 归一为
/// 空路径，`Paths.get("/..")` 归一为 `/`，`Paths.get("../..")` 保持 `../..`）。
///
/// 重建时用平台分隔符逐段拼接，因此 Windows 下的 `toString()` 形态与 Java 一致（全 `\`、无尾随
/// 分隔符）。`\\?\` 这类 verbatim 前缀本工具的输入不会出现（[`std::path::absolute`] 不会产生它），
/// 若真出现也只是按 Rust 解析出的 Prefix + RootDir 原样拼回。
///
/// **可见性放宽的理由**：这是本工程唯一的 Java `Path.normalize()` 实现，任何调用点都不该再写第二份。
/// `tests/fixture_project.rs` 要复刻 `FixtureProjectTest.java:40` 的
/// `SearchPaths.workingDirectory().resolve("../mod").normalize()`，而集成测试是独立的 crate 外部
/// 二进制、看不到 `pub(crate)`，故此处必须是 `pub`。
pub fn lexical_normalize(path: &Path) -> PathBuf {
    const SEPARATOR: &str = std::path::MAIN_SEPARATOR_STR;
    let mut components = path.components().peekable();
    // head = 根部分（Windows 上是前缀 + 根，Unix 上是一个分隔符），可能为空。
    let mut head = OsString::new();
    // 对应 Java 的 `getRoot() != null`：有根时 `..` 撞上根就丢弃，绝不超过去。
    let mut rooted = false;
    loop {
        match components.peek().cloned() {
            Some(Component::Prefix(prefix)) => {
                head.push(prefix.as_os_str());
                let _ = components.next();
            }
            Some(Component::RootDir) => {
                head.push(SEPARATOR);
                rooted = true;
                let _ = components.next();
            }
            _ => break,
        }
    }
    let has_root = !head.is_empty();

    let mut names: Vec<&OsStr> = Vec::new();
    for component in components {
        match component {
            // CurDir（`.`）被丢弃；Java 同样如此
            Component::CurDir => {}
            // 根部分已在上面的循环里消费完，components 保证它只出现在开头
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(name) => names.push(name),
            Component::ParentDir => {
                if names.last().is_some_and(|last| *last != OsStr::new("..")) {
                    names.pop();
                } else if names.is_empty() && has_root {
                    // 已经在根上，无处可退：丢弃
                } else {
                    names.push(OsStr::new(".."));
                }
            }
        }
    }

    // 逐段拼接：根部分自带结尾分隔符时不再补，其余情况段与段之间补一个，
    // 因此既不会产生尾随分隔符（Java 的 normalize 也不会留下它），也不会把根的分隔符翻倍。
    let mut out = head;
    for (index, name) in names.iter().enumerate() {
        if index > 0 || (!out.is_empty() && !rooted) {
            out.push(SEPARATOR);
        }
        out.push(name);
    }
    PathBuf::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::files::scratch::temp_case;

    #[test]
    fn components_count_vs_java_name_count_on_windows() {
        // 确认本文件 name_count 的取舍：Windows 下 `C:\a\b` 的 components().count() 是 4，
        // Java 的 getNameCount() 是 2，只数 Normal 段才与 Java 一致。
        let path = Path::new(r"C:\a\b");
        if cfg!(windows) {
            assert_eq!(path.components().count(), 4);
            assert_eq!(name_count(path), 2);
        }
        assert_eq!(name_count(Path::new("a/b")), 2);
        assert_eq!(name_count(Path::new("")), 0);
    }

    #[test]
    fn lexical_normalize_matches_javas_documented_examples() {
        // normalize 在 rustc 1.99.1 上尚未稳定，本文件自带实现；这几条对着
        // java.nio.file.Path.normalize() 的 javadoc 例子逐条核。Rust 的 Path 在 Windows
        // 上同时接受 `/` 与 `\`，所以例子按 Java 原文写 `/` 也能测。
        assert_eq!(lexical_normalize(Path::new("foo/bar/..")), PathBuf::from("foo"));
        assert_eq!(lexical_normalize(Path::new("a/.")), PathBuf::from("a"));
        assert_eq!(lexical_normalize(Path::new("a/..")), PathBuf::from(""));
        assert_eq!(lexical_normalize(Path::new("/foo/..")), PathBuf::from("/"));
        assert_eq!(lexical_normalize(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(lexical_normalize(Path::new("../..")), PathBuf::from("..").join(".."));
        assert_eq!(lexical_normalize(Path::new("a/b/../../c")), PathBuf::from("c"));
        // Windows 专属：`..` 撞上根就丢弃，绝不越过盘符
        assert_eq!(lexical_normalize(Path::new(r"C:\a\..\..\b")), PathBuf::from(r"C:\b"));
    }

    #[test]
    fn blank_override_value_counts_as_unset() {
        // Java: value == null || value.isBlank() → null
        assert_eq!(override_from_value(None), None);
        assert_eq!(override_from_value(Some("")), None);
        assert_eq!(override_from_value(Some("   \t ")), None);
    }

    #[test]
    fn non_blank_override_value_is_absolute_and_normalized() {
        // Java: Path.of(value).toAbsolutePath().normalize()
        let hit = override_from_value(Some("mod")).expect("命中");
        assert!(hit.is_absolute());
        assert!(
            !hit.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir)),
            "不应残留 `.`/`..`：{}",
            hit.display()
        );

        let root = temp_case("override");
        let with_dots = root.join("a").join("..").join("b");
        let hit = override_from_value(Some(with_dots.to_str().expect("UTF-8 路径"))).expect("命中");
        assert_eq!(hit, normalize_absolute(&root.join("b")));
        assert!(!hit.to_string_lossy().contains(".."));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn override_path_reads_only_the_environment_channel() {
        // 真实环境变量这条不测：`std::env::set_var` 会改动整个测试二进制的进程级状态，
        // 并行用例互相串扰，而且在 edition 2024 语义下它还是 unsafe。上面的纯函数已覆盖
        // 「空白视为未设置 / 非空返回 absolute+normalize」这两条 Java 分支；剩下的
        // var_os 转发只是标准库调用。这里只断言未设置时返回 None（该环境变量名为本 crate 独有）。
        assert_eq!(override_path("modmaker.definitelyUnset", "MODMAKER_RS_UNSET_PROBE"), None);
    }

    #[test]
    fn working_directory_is_the_normalized_current_directory() {
        let cwd = std::env::current_dir().expect("current_dir");
        assert_eq!(working_directory(), lexical_normalize(&cwd));
        // cargo 把测试二进制的 cwd 放在 crate 目录（crates/mm-io），而不是工作区根；
        // 这正是与 Gradle user.dir 的差异所在，后续移植相对候选路径时要按此核对。
        assert!(working_directory().is_absolute());
    }

    #[test]
    fn first_directory_and_first_file_pick_the_first_hit() {
        let root = temp_case("first");
        let dir = root.join("board");
        fs::create_dir_all(&dir).expect("建目录");
        let file = root.join("mod.json");
        fs::write(&file, "{}").expect("建文件");

        // 只给路径的一部分：文件不是目录，目录不是文件
        assert_eq!(first_directory(&[file.clone(), dir.clone()]), Some(dir.clone()));
        assert_eq!(first_file(&[dir.clone(), file.clone()]), Some(file.clone()));
        // 命中者已绝对化 + normalize
        assert_eq!(first_directory(&[PathBuf::from("inexistent-dir-xyz")]), None);
        assert_eq!(first_file(&[]), None);
        // 单个文件作为候选时它自己不是目录，单个目录也不是文件
        assert_eq!(first_directory(std::slice::from_ref(&file)), None);
        assert_eq!(first_file(std::slice::from_ref(&dir)), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn find_prefers_the_shallowest_match() {
        let root = temp_case("shallow");
        let deep = root.join("a").join("b");
        fs::create_dir_all(&deep).expect("建深层目录");
        fs::write(deep.join("m.json"), "{}").expect("写深层文件");
        fs::write(root.join("m.json"), "{}").expect("写浅层文件");

        let hit = find(Some(&root), 3, &|p: &Path| {
            p.file_name() == Some(OsStr::new("m.json"))
        })
        .expect("应命中");
        assert_eq!(hit, normalize_absolute(&root.join("m.json")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn find_respects_the_depth_limit() {
        let root = temp_case("depth");
        // root/shallow.json 在第 1 层，root/a/b/deep.json 在第 3 层
        fs::write(root.join("shallow.json"), "{}").expect("写第 1 层的文件");
        let deep_dir = root.join("a").join("b");
        fs::create_dir_all(&deep_dir).expect("建深层目录");
        fs::write(deep_dir.join("deep.json"), "{}").expect("写第 3 层的文件");

        let at = |depth: i32, name: &str| {
            find(Some(&root), depth, &|p: &Path| {
                p.file_name() == Some(OsStr::new(name))
            })
        };
        assert_eq!(at(1, "shallow.json"), Some(normalize_absolute(&root.join("shallow.json"))));
        // 第 3 层要 depth = 3 才看得见；depth 更小时整棵树里没有别的候选 → None
        assert_eq!(at(2, "deep.json"), None);
        assert_eq!(at(1, "deep.json"), None);
        assert_eq!(at(3, "deep.json"), Some(normalize_absolute(&deep_dir.join("deep.json"))));
        // Java: depth < 1 → null
        assert_eq!(find(Some(&root), 0, &|_: &Path| true), None);
        assert_eq!(find(Some(&root), -1, &|_: &Path| true), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn find_can_match_the_root_itself() {
        let root = temp_case("rootself");
        fs::write(root.join("marker"), "").expect("写标记文件");
        let root_name = root.file_name().expect("临时目录有名").to_os_string();
        // root 自身作为目录参与谓词（Files.walk 的第一个元素），且它是最浅的，必定先于子项命中
        let hit = find(Some(&root), 1, &|p: &Path| p.file_name() == Some(root_name.as_os_str()))
            .expect("root 自身应命中");
        assert_eq!(hit, normalize_absolute(&root));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn find_breaks_ties_by_the_whole_path_string() {
        // Java: thenComparing(Path::toString)，即整串比较，而不是 Path 的按段比较。
        // 两条候选同为第 2 层：a/b.txt 与 a-b/c.txt。按 Java 的字符串序，
        // 分隔符 `\`(U+005C) > `-`(U+002D)，所以 a-b\c.txt 先。
        let root = temp_case("tie");
        fs::create_dir_all(root.join("a")).expect("建 a");
        fs::create_dir_all(root.join("a-b")).expect("建 a-b");
        fs::write(root.join("a").join("b.txt"), "").expect("写 a/b.txt");
        fs::write(root.join("a-b").join("c.txt"), "").expect("写 a-b/c.txt");

        let hit = find(Some(&root), 2, &|p: &Path| {
            p.file_name().map(|n| n.to_string_lossy().ends_with(".txt")).unwrap_or(false)
        })
        .expect("应命中");
        assert_eq!(hit, normalize_absolute(&root.join("a-b").join("c.txt")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn find_returns_none_for_a_missing_or_non_directory_root() {
        let root = temp_case("missing");
        let file = root.join("f.txt");
        fs::write(&file, "").expect("写文件");
        assert_eq!(find(Some(&root.join("nope")), 3, &|_: &Path| true), None);
        // Java: !Files.isDirectory(root) → null（普通文件不算目录）
        let expected = file.clone();
        assert_eq!(find(Some(&file), 3, &|p: &Path| p == expected), None);
        assert_eq!(find(None, 3, &|_: &Path| true), None);
        let _ = fs::remove_dir_all(&root);
    }
}
