//! `fs_list_dir` 的内核：只读目录浏览。**不依赖 tauri**，集成测试可以直接调
//! （与 `forest.rs` 同一理由：链接 tauri 的测试可执行文件在本机加载即死）。
//!
//! 领域层没有「文件对话框」这种东西——它属于外壳。这里只做三件事：列目录、判模组目录、
//! 判归档文件；排序与标记都是纯展示口径，不写任何文件。

use std::fs;
use std::path::{Path, PathBuf};

use crate::dto::{FsEntryDto, FsListingDto};

/// 游戏会探测的 meta 文件名（与 `mm_model::ModMeta::META_FILES` 同源的含义，
/// 这里只用于「一眼看出这是不是一个模组目录」的展示判断）。
const MOD_META_FILES: [&str; 2] = ["mod.json", "mod.hjson"];

/// 列一个目录。`path` 为空串时报错（前端应当传一个具体目录）。
///
/// 返回的 `path`/`parent`/`entries[].path` 都是**绝对**路径（相对路径按当前工作目录展开），
/// 方便前端把返回值直接回传。父目录在盘根上为 `None`。
pub fn list_dir(path: &str) -> Result<FsListingDto, String> {
    if path.trim().is_empty() {
        return Err("空路径".to_string());
    }
    let given = PathBuf::from(path);
    let absolute = if given.is_absolute() {
        given
    } else {
        std::env::current_dir()
            .map_err(|err| format!("取当前目录失败：{err}"))?
            .join(given)
    };
    let metadata = fs::metadata(&absolute).map_err(|err| format!("{}: {err}", absolute.display()))?;
    if !metadata.is_dir() {
        return Err(format!("不是目录：{}", absolute.display()));
    }

    let parent = absolute.parent().map(display_path);
    let mut entries: Vec<FsEntryDto> = Vec::new();
    let reader = fs::read_dir(&absolute).map_err(|err| format!("{}: {err}", absolute.display()))?;
    for entry in reader {
        let entry = entry.map_err(|err| format!("{}: {err}", absolute.display()))?;
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            // 坏条目（权限/断链）不该让整个目录打不开：跳过它，其余照列。
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = entry.path();
        let is_dir = file_type.is_dir();
        entries.push(FsEntryDto {
            is_dir,
            is_mod: is_dir && is_mod_dir(&child),
            is_archive: !is_dir && is_archive_name(&name),
            name,
            path: display_path(&child),
        });
    }
    // 目录在前；同组内按名字不区分大小写，再按原样拼写破平局，保证结果稳定。
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(FsListingDto {
        path: display_path(&absolute),
        parent,
        entries,
    })
}

/// `.zip`/`.jar`（大小写不敏感）：两者都是 Mindustry 认的模组归档。
pub fn is_archive_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".zip") || lower.ends_with(".jar")
}

fn is_mod_dir(path: &Path) -> bool {
    MOD_META_FILES.iter().any(|name| path.join(name).is_file())
}

/// Windows 上 `\\?\C:\…` 这类 verbatim 前缀对前端没有意义；能剥就剥，剥不掉就用原样。
fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => text.into_owned(),
    }
}
