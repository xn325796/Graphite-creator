//! `AtomicFiles.java` 的等价移植：经由同目录的临时文件写完，再换入目标位置，
//! 于是读取方——或一次崩溃——永远看不到半截文件。
//!
//! 本工具的两个写入方需要同一条保证，理由也相同：一次保存不能留下半截的模组文件（游戏会把
//! 它加载起来，作者因此丢掉上一个版本），一次构建也不能在中途失败时毁掉上一个好归档。这条契约
//! 过去被拼写了两遍、用的标志位还略有不同——保存侧的 rename 甚至没有要求 `ATOMIC_MOVE`——所以它
//! 现在只在这里出现一次，就放在双方早已一致的 [`TEMP_SUFFIX`] 旁边。
//!
//! 可见性说明：Java 的 `AtomicFiles` 是 package-private 的 final class（`final` 在 Rust 没有
//! 对应物，私有构造函数即「不可实例化」，Rust 用模块级自由函数天然等价）。这里声明为 `pub`
//! 是因为它要被本 crate 的 project/export/package 等模块共享，同时避免 `dead_code` 警告；
//! **使用约定仍是仅限 mm-io 内部**。

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::project::TEMP_SUFFIX;

/// 把 `text` 以 UTF-8 写入 `target`，需要时创建父目录。
///
/// Java 侧 `text == null` 会被写成空串；Rust 的参数是 `&str`，类型系统已保证非空，
/// 因此那一分支不需要对应物。错误统一用 [`std::io::Result`] 承载（Java 抛 `IOException`）。
pub fn write_text(target: &Path, text: &str) -> io::Result<()> {
    // 对应 Java 的 target.toAbsolutePath()：这里**没有** normalize，不要多加。
    let absolute = absolute_unnormalized(target);
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temp_for(&absolute);
    // Rust 的 `&str` 已是 UTF-8 字节，等价于 Java 的 StandardCharsets.UTF_8 + CREATE/TRUNCATE/WRITE。
    match fs::write(&temp, text).and_then(|()| move_into_place(&temp, &absolute)) {
        Ok(()) => Ok(()),
        Err(failure) => {
            // 残留的临时文件无害（读取端把它当噪音），但一次失败的保存之后仍然留着它，
            // 会把真正的失败藏在目录列表里。
            delete_quietly(&temp);
            Err(failure)
        }
    }
}

/// 把一个已经写完的临时文件改名到它的目标上。临时文件是 sibling（同目录），所以在任何真实
/// 文件系统上这都是一次 rename；我们优先要求原子替换，不能原子时退化为普通替换。
///
/// **Windows 上的关键差异**：Java 用的是
/// `Files.move(temp, target, REPLACE_EXISTING, ATOMIC_MOVE)`，语义是「原子地替换已存在的目标」。
/// 而 `std::fs::rename` 与 `REPLACE_EXISTING` 并不等价：它只是「rename」，能不能覆盖已存在的
/// 目标由平台实现决定。本机实测（x86_64-pc-windows-gnu、rustc 1.99.1，见
/// `writing_the_same_target_twice_replaces_it`）对**已存在的普通文件**目标返回成功，所以常见的
/// 覆盖写入走的是第 1 步这条原子路径；但当目标只读、被别的句柄打开、或是一个（非空）目录时，
/// 底层的替换会被拒绝并返回 AccessDenied 一类错误——这些都不是 `REPLACE_EXISTING` 的语义。
/// 因此这里的实现是：
///
/// 1. 先试 `fs::rename`——它本身就是原子的，目标不存在时天然成功；
/// 2. 失败且目标已存在时，退化为「删除目标后再次 rename」。
///
/// 第 2 步只在原子 rename 被拒绝时才会走到，它**不是**原子的：两次操作之间有一个短暂的窗口，
/// 崩溃在那里会让目标暂时缺失——这是换取「与 Java 一样能替换已存在文件」所接受的代价。
/// Java 的第二层退化（`AtomicMoveNotSupportedException` → 不带 `ATOMIC_MOVE` 的普通替换）
/// 由同一条 delete+rename 路径覆盖：Rust 侧无法探测「provider 是否支持原子移动」，`rename`
/// 的错误不区分「不支持原子」与「其他失败」，所以只能统一按被拒绝处理。
pub fn move_into_place(temp: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(temp, target) {
        Ok(()) => Ok(()),
        Err(failure) => {
            if !target.exists() {
                // 目标不存在时 rename 已经覆盖了 REPLACE_EXISTING 的全部语义，失败就是真失败。
                return Err(failure);
            }
            // 退化路径：先删掉已存在的目标，再 rename。删除失败（只读、被占用、目标是非空目录）
            // 直接上报——那正是「不能毁掉上一个好归档」要拦的情况。
            fs::remove_file(target)?;
            fs::rename(temp, target)
        }
    }
}

/// 尽力清理；调用方自己的失败才值得上报。
pub fn delete_quietly(temp: &Path) {
    // 残留的临时文件无害：下一个写入会覆盖它。
    let _ = fs::remove_file(temp);
}

/// 一次写入首先经过的 sibling 临时文件，例如 `mod.json.mm-tmp`。
///
/// 对应 Java 的 `target.resolveSibling(target.getFileName() + TEMP_SUFFIX)`，即用
/// [`Path::with_file_name`] 保持父目录不变。用 [`OsString`] 直接拼接后缀，避免任何
/// 有损的字符串往返（Java 的 `Path` 本来就只存字符串）。Java 的 `getFileName()` 对根路径
/// 返回 null，会被拼成 `"null.mm-tmp"`；本函数的调用方从不传根路径，Rust 侧按空名处理。
pub fn temp_for(target: &Path) -> PathBuf {
    let name: &OsStr = target.file_name().unwrap_or(OsStr::new(""));
    let mut temp_name = name.to_os_string();
    temp_name.push(TEMP_SUFFIX);
    target.with_file_name(temp_name)
}

/// Java 的 `toAbsolutePath()`：绝对化但**不**做 normalize，也不触碰文件系统去解析符号链接。
///
/// [`std::path::absolute`] 正是这个语义。它在取不到当前目录时返回错误，而 Java 的
/// `toAbsolutePath()` 从不抛 `IOException`，故此处退回原值，保持调用方后续的相对路径行为。
fn absolute_unnormalized(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::files::scratch::temp_case;

    #[test]
    fn temp_for_appends_the_suffix_to_the_file_name() {
        // Java: target.resolveSibling(fileName + TEMP_SUFFIX)
        let target = Path::new("out").join("mod.json");
        assert_eq!(temp_for(&target), Path::new("out").join("mod.json.mm-tmp"));
        // 无父目录时同理
        assert_eq!(temp_for(Path::new("mod.json")), Path::new("mod.json.mm-tmp"));
    }

    #[test]
    fn write_text_creates_the_missing_parents() {
        let root = temp_case("parents");
        let target = root.join("a").join("b").join("mod.json");
        write_text(&target, "{}").expect("写入应成功");
        assert!(target.is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_text_lands_utf8_and_leaves_no_temp_file() {
        let root = temp_case("utf8");
        let target = root.join("bundle.properties");
        // 非 ASCII：验证 UTF-8 落盘（Java 的 StandardCharsets.UTF_8）
        write_text(&target, "unit.name=钻头\n").expect("写入应成功");
        let bytes = fs::read(&target).expect("读取目标");
        let mut expect = b"unit.name=".to_vec();
        expect.extend_from_slice("钻头\n".as_bytes());
        assert_eq!(bytes, expect);
        assert!(!temp_for(&target).exists(), "不应留下 .mm-tmp 残留");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn writing_the_same_target_twice_replaces_it() {
        // 专门验证 Java 的 REPLACE_EXISTING：第二次写入时目标已存在，必须成功替换。
        // 本机实测这一步走的是原子 rename 主路径；delete+rename 退化只在 rename 被拒绝时
        // 才走到（见 move_into_place 的文档与本文件末尾那条失败用例）。
        let root = temp_case("replace");
        let target = root.join("project.json");
        write_text(&target, "first").expect("首次写入");
        write_text(&target, "second").expect("覆盖写入——目标已存在");
        assert_eq!(fs::read_to_string(&target).expect("读取"), "second");
        assert!(!temp_for(&target).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_write_removes_the_temp_file_it_left_behind() {
        // 构造失败：目标位置是一个非空目录，于是 rename 与被拒绝后的 remove_file 都会失败，
        // 而临时文件已经写完。断言错误被上报、临时文件被清掉、原有内容未被毁掉。
        let root = temp_case("failure");
        let target = root.join("content");
        fs::create_dir_all(&target).expect("创建目录");
        fs::write(target.join("keep.txt"), "上一个好归档".as_bytes()).expect("填充目录");
        let failure = write_text(&target, "boom").expect_err("非空目录作为目标必须失败");
        assert!(!temp_for(&target).exists(), "失败后不应残留临时文件：{failure}");
        assert_eq!(fs::read_to_string(target.join("keep.txt")).expect("读取"), "上一个好归档");
        let _ = fs::remove_dir_all(&root);
    }
}
