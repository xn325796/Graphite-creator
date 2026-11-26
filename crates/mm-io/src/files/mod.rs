//! `core.io` 中与项目结构无关的文件基元：原子写入与外部路径查找。
//!
//! 对应 Java `dev.modmaker.core.io.AtomicFiles` 与 `dev.modmaker.core.io.SearchPaths`。

pub mod atomic_files;
pub mod search_paths;

pub use atomic_files::{delete_quietly, move_into_place, temp_for, write_text};
pub use search_paths::{find, first_directory, first_file, override_path, working_directory};

/// 本 crate 测试共用的 scratch 目录。
#[cfg(test)]
pub(crate) mod scratch {
    use std::fs;
    use std::path::PathBuf;

    /// 每个用例一个专属子目录，用完即删；不依赖任何外部 fixture。
    ///
    /// **为什么不是 `std::env::temp_dir()`**：本机实测（probe）测试二进制在 `%TEMP%` 与
    /// `C:\Windows\Temp` 下建目录一律 `ERROR_ACCESS_DENIED`（拒绝访问，code 5），只有
    /// workspace 内可写。因此 scratch 根落在 crate 自己的 `target/` 之下（cargo 把测试
    /// 二进制的 cwd 设为 crate 目录，`target/` 也已被 git 忽略）。命名依旧唯一。
    pub(crate) fn temp_case(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = root().join(format!("{tag}-{}-{serial}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("创建用例目录 {} 失败：{e}", dir.display()));
        dir
    }

    fn root() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("target")
            .join("mm-io-tests")
    }
}
