//! `ProjectFactory.java` 的等价移植：造一个空项目——只有元数据与一块画布，没有任何内容。
//!
//! 对应 Java `dev.modmaker.core.io.ProjectFactory`。

use std::io;
use std::path::Path;

use mm_fmt::ids::uid;
use mm_model::{Board, ModProject};

use crate::project_io;

/// 建一个空项目并立刻落盘。对应 Java `ProjectFactory.create(String, Path)`。
///
/// Java 里这一步直接调 `ProjectIo.save`（`static synchronized`），所以这里同样走公开的
/// [`project_io::save`]：调用方通常就跑在持有模型的那个线程上，与 Java 的退出流程、
/// 新建项目流程一致（见 `ProjectIo.save` 的文档）。
pub fn create(mod_name: &str, root: &Path) -> io::Result<ModProject> {
    let mut project = ModProject::default();
    project.meta.name = mod_name.to_string();
    project.meta.display_name = mod_name.to_string();
    // Java 显式写空串（`ModMeta.author` 的默认值本就是 `""`），照搬这一赋值以保持逐字段一致。
    project.meta.author = String::new();
    project.meta.version = "1.0".to_string();
    project.meta.min_game_version = "160".to_string();
    project.meta_file = "mod.hjson".to_string();
    project.meta_dirty = true;

    // Java `Board.with_id(...)` 这类便捷构造在 mm-model 里不存在，故按 Java 的字段赋值写法
    // 直接给 `id` 赋 `Some(...)`，语义完全相同。
    let board = Board {
        id: Some(uid("b")),
        name: "Main".to_string(),
        file: "main.canvas".to_string(),
        ..Default::default()
    };
    project.boards.push(board);

    project.root = Some(root.to_path_buf());
    project_io::save(&mut project, root)?;
    Ok(project)
}
