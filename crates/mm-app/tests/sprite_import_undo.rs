//! 贴图导入纳入 undo 栈：`SpriteImportCommand` 的命令级测试（2026-11-25，UI 审计缺陷 #5）。
//!
//! 口径说明：导入内核（`src-tauri/src/sprite_asset.rs`）在写盘**前**收快照、写盘与登记账本
//! **后**推命令。这里的夹具直接构造「导入已完成」的现场（文件已写、账本已登）再推同一条
//! 命令——被测对象是命令本身（undo/redo 的字节与账本语义、失败时历史是否被破坏），
//! 不是导入内核。端到端的「导入即推栈」见 `src-tauri/tests/sprite_import.rs`。
//!
//! 覆盖：undo 恢复旧字节（字节级）与账本、首次导入 undo 删文件并摘账本、redo 重放、
//! 与字段编辑同栈的时间序、写失败不破坏历史且可重试、记录被带外删除报 not-found。

use std::fs;
use std::path::{Path, PathBuf};

use mm_app::project_controller::{ProjectController, SpriteImportCommand};
use mm_app::undo_manager::CommandError;
use mm_json::Json;
use mm_model::AssetRef;
use mm_schema::{ContentType, SchemaRegistry};

/// 临时项目根：Drop 时尽力删除（断言失败也不留垃圾）。
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mm-sprite-undo-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时项目根");
        TempRoot(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn registry() -> SchemaRegistry {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    SchemaRegistry::load(&dir).expect("../../schemas 必须在仓库里")
}

/// 一个已建好、含一条 item 记录的项目。`add_content` 自己会推一条历史条目，
/// 所以所有测试的栈深都从 1 起算。
fn fixture(tag: &str) -> (TempRoot, ProjectController, String) {
    let root = TempRoot::new(tag);
    let mut controller = ProjectController::new(registry());
    controller
        .create_project("sprite-undo", root.path())
        .expect("建项目");
    let id = controller.add_content(ContentType::Item);
    assert_eq!(controller.undo_stack_depth(), 1, "add_content 占一条");
    (root, controller, id)
}

fn record_name(controller: &ProjectController, id: &str) -> String {
    controller
        .project()
        .and_then(|project| project.content_by_id(id))
        .map(|record| record.name.clone())
        .expect("记录存在")
}

fn relative_for(controller: &ProjectController, id: &str) -> String {
    format!("sprites/{}.png", record_name(controller, id))
}

fn ledger_size(controller: &ProjectController, relative: &str) -> Option<i64> {
    controller
        .project()
        .expect("有项目")
        .assets
        .iter()
        .find(|asset| asset.path == relative)
        .map(|asset| asset.size)
}

/// 目标文件路径。`Path::join` 在 Windows 上接受 `/`，所以相对路径原样拼即可。
fn target_path(root: &Path, relative: &str) -> PathBuf {
    root.join(relative)
}

/// 复刻导入内核「写盘 → 登记账本 → 推命令」三步（内容校验不在此列：命令只搬运字节）。
/// 返回目标相对路径与导入前的旧字节（首次导入为 `None`）。
fn simulate_import(
    controller: &mut ProjectController,
    root: &Path,
    id: &str,
    new_bytes: &[u8],
) -> (String, Option<Vec<u8>>) {
    let relative = relative_for(controller, id);
    let target = target_path(root, &relative);
    let old_bytes = match fs::read(&target) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => panic!("读旧贴图失败：{err}"),
    };
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("建 sprites 目录");
    }
    fs::write(&target, new_bytes).expect("写目标贴图");

    let size = new_bytes.len() as i64;
    let asset_before = {
        let project = controller.project_mut().expect("有项目");
        let before = project.assets.iter().find(|asset| asset.path == relative).cloned();
        match project.assets.iter_mut().find(|asset| asset.path == relative) {
            Some(asset) => asset.size = size,
            None => project.assets.push(AssetRef::new(&relative, size, "sprite")),
        }
        before
    };
    controller.push_sprite_import(SpriteImportCommand::new(
        id,
        &relative,
        old_bytes.clone(),
        new_bytes.to_vec(),
        asset_before,
    ));
    (relative, old_bytes)
}

/// undo 恢复导入前的**旧字节**与账本旧条目；redo 再重放新字节与新条目。
#[test]
fn undo_restores_the_previous_bytes_and_the_ledger() {
    let (root, mut controller, id) = fixture("replace");
    let first = vec![0x89, b'P', b'N', b'G', 0x01];
    let second = vec![0x89, b'P', b'N', b'G', 0x02, 0x03, 0x04];

    simulate_import(&mut controller, root.path(), &id, &first);
    let (relative, old) = simulate_import(&mut controller, root.path(), &id, &second);
    assert_eq!(old.as_deref(), Some(first.as_slice()), "第二次导入的旧字节是第一版");
    assert_eq!(controller.undo_stack_depth(), 3, "add + 两次导入");
    assert_eq!(ledger_size(&controller, &relative), Some(second.len() as i64));

    let target = target_path(root.path(), &relative);
    controller.undo().expect("undo 应成功");
    assert_eq!(fs::read(&target).expect("读回滚后的文件"), first, "旧字节逐字节恢复");
    assert_eq!(ledger_size(&controller, &relative), Some(first.len() as i64), "账本回到旧字节数");
    assert!(!target.with_file_name(format!(
        "{}.mm-tmp",
        target.file_name().expect("有文件名").to_string_lossy()
    )).exists(), "回滚不应留下临时文件");
    assert!(controller.can_redo());

    controller.redo().expect("redo 应成功");
    assert_eq!(fs::read(&target).expect("读重放后的文件"), second, "新字节逐字节重放");
    assert_eq!(ledger_size(&controller, &relative), Some(second.len() as i64));
}

/// 首次导入（导入前没有文件）：undo 删掉文件并摘掉账本条目；redo 把两者都放回来。
#[test]
fn undo_of_a_first_import_deletes_the_file_and_the_ledger_entry() {
    let (root, mut controller, id) = fixture("first");
    let bytes = vec![0x89, b'P', b'N', b'G', 0x05, 0x06];
    let (relative, old) = simulate_import(&mut controller, root.path(), &id, &bytes);
    assert_eq!(old, None, "首次导入没有旧文件");
    let target = target_path(root.path(), &relative);
    assert!(target.is_file());

    controller.undo().expect("undo 应成功");
    assert!(!target.exists(), "首次导入的 undo 是删除文件");
    assert_eq!(ledger_size(&controller, &relative), None, "账本条目一并摘掉");
    assert!(
        controller
            .project()
            .expect("有项目")
            .assets
            .iter()
            .all(|asset| asset.path != relative),
        "同路径不应留任何残留条目"
    );

    controller.redo().expect("redo 应成功");
    assert_eq!(fs::read(&target).expect("重放后应重现文件"), bytes);
    assert_eq!(ledger_size(&controller, &relative), Some(bytes.len() as i64));
}

/// 贴图条目与字段编辑条目按时间同栈，undo/redo 逐条回退/重放。
#[test]
fn sprite_entries_and_field_edits_share_one_stack_in_time_order() {
    let (root, mut controller, id) = fixture("mixed");
    let before = controller
        .project()
        .and_then(|project| project.content_by_id(&id))
        .and_then(|record| record.fields.get("hardness").cloned());
    let bytes = vec![0x89, b'P', b'N', b'G', 0x07];
    let (relative, _) = simulate_import(&mut controller, root.path(), &id, &bytes);
    controller.set_field(&id, "hardness", Some(Json::num("9")));
    assert_eq!(controller.undo_stack_depth(), 3, "add + 导入 + 字段");
    let target = target_path(root.path(), &relative);

    // 第一次 undo 撤字段，贴图不受影响。
    controller.undo().expect("撤销字段");
    assert_eq!(
        controller
            .project()
            .and_then(|project| project.content_by_id(&id))
            .and_then(|record| record.fields.get("hardness").cloned()),
        before,
        "字段回到编辑前"
    );
    assert_eq!(fs::read(&target).expect("贴图还在"), bytes, "撤字段不该碰贴图");

    // 第二次 undo 撤贴图（首次导入 → 删文件）。
    controller.undo().expect("撤销导入");
    assert!(!target.exists(), "同一栈上按时间继续回退");
    assert_eq!(ledger_size(&controller, &relative), None);

    // redo 按相反顺序重放。
    controller.redo().expect("重放导入");
    assert_eq!(fs::read(&target).expect("贴图重现"), bytes);
    controller.redo().expect("重放字段");
    assert_eq!(
        controller
            .project()
            .and_then(|project| project.content_by_id(&id))
            .and_then(|record| record.fields.get("hardness").cloned()),
        Some(Json::num("9")),
        "字段重放到新值"
    );
}

/// 写失败 → `Io`，历史不被破坏（条目留栈），排除故障后重试同一条成功。
#[test]
fn a_failed_sprite_write_keeps_the_history_and_can_be_retried() {
    let (root, mut controller, id) = fixture("writefail");
    let first = vec![0x89, b'P', b'N', b'G', 0x0A];
    let second = vec![0x89, b'P', b'N', b'G', 0x0B, 0x0C];
    simulate_import(&mut controller, root.path(), &id, &first);
    let (relative, _) = simulate_import(&mut controller, root.path(), &id, &second);
    let target = target_path(root.path(), &relative);
    let target_dir = target.parent().expect("sprites 目录").to_path_buf();

    // 造故障：把 sprites 目录换成一个同名文件，undo 的 create_dir_all 必然失败。
    fs::remove_file(&target).expect("摘掉贴图");
    fs::remove_dir(&target_dir).expect("摘掉空目录");
    fs::write(&target_dir, b"not a directory").expect("占位文件");

    let err = controller.undo().expect_err("写盘必须失败");
    assert!(matches!(err, CommandError::Io(_)), "实际：{err:?}");
    assert_eq!(controller.undo_stack_depth(), 3, "条目留在栈上：add + 两次导入");
    assert!(controller.can_undo(), "还能再试");
    assert!(!controller.can_redo(), "失败不进 redo 栈");
    assert_eq!(fs::read(&target_dir).expect("占位文件未被动过"), b"not a directory");
    assert!(!target.exists());

    // 排除故障后重试：同一条命令成功，旧字节回到原位。
    fs::remove_file(&target_dir).expect("清掉占位文件");
    fs::create_dir_all(&target_dir).expect("恢复目录");
    controller.undo().expect("重试应成功");
    assert_eq!(fs::read(&target).expect("旧字节恢复"), first);
    assert_eq!(controller.undo_stack_depth(), 2);
    assert!(controller.can_redo());
}

/// 记录被带外删除：undo 报 `NotFound`，历史与磁盘都不动（不往不存在的记录名下写文件）。
#[test]
fn an_out_of_band_deleted_record_reports_not_found() {
    let (root, mut controller, id) = fixture("norecord");
    let bytes = vec![0x89, b'P', b'N', b'G', 0x0D];
    let (relative, _) = simulate_import(&mut controller, root.path(), &id, &bytes);
    let target = target_path(root.path(), &relative);

    // 带外删除：直接改模型，不经过 DeleteCommand（那会自己进栈、改变栈序）。
    controller
        .project_mut()
        .expect("有项目")
        .contents
        .retain(|record| record.id.as_deref() != Some(id.as_str()));
    assert!(controller.project().and_then(|project| project.content_by_id(&id)).is_none());

    let err = controller.undo().expect_err("记录不在必须报错");
    assert!(
        matches!(&err, CommandError::NotFound(missing) if missing == &id),
        "实际：{err:?}"
    );
    assert_eq!(controller.undo_stack_depth(), 2, "条目留在栈上可重试");
    assert!(controller.can_undo());
    assert_eq!(fs::read(&target).expect("磁盘未被改动"), bytes);
}
