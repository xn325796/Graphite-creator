//! `sprite_import` 内核的集成测试（tauri-free 模块，直接从 lib 引用，口径同 `fs_sprite.rs`）。
//!
//! 覆盖：成功导入后 `record_sprite` 的内核 `sprite_for` 能读到（账本同步）、字节保真与
//! 原子写无残留、重复导入覆盖、非 PNG 拒绝、源不存在报错、记录名路径穿越拒绝、
//! 内联记录拒绝、未知记录 not-found；以及 2026-11-25 新增的「导入纳入 undo 栈」端到端
//! 链路（导入推栈 → undo 回到导入前 → redo 重放；命令级语义见
//! `crates/mm-app/tests/sprite_import_undo.rs`）。临时目录手写（项目无 dev-dependencies，
//! 沿用 `fs_sprite.rs` 的 temp_dir 模式）。

use std::fs;
use std::path::{Path, PathBuf};

use graphitecreator_lib::sprite_asset::{import_sprite, sprite_for, SpriteImportError};
use mm_app::project_controller::ProjectController;
use mm_schema::SchemaRegistry;

fn temp_dir(tag: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("gc-{tag}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// PNG 签名 + 任意载荷：读侧只校验签名，测试不需要真解码一张图。
fn png_bytes(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(payload);
    bytes
}

/// 一个可打开的最小模组：mod.json + 一条 item 记录 kiln（起初没有贴图）。
fn fixture_project(tag: &str) -> (PathBuf, ProjectController) {
    let root = temp_dir(tag);
    fs::create_dir_all(root.join("content/items")).expect("建内容目录");
    fs::write(
        root.join("mod.json"),
        br#"{"name":"importme","minGameVersion":"160"}"#,
    )
    .expect("写 meta");
    fs::write(root.join("content/items/kiln.json"), br#"{"hardness":2}"#).expect("写内容");

    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas");
    let registry = SchemaRegistry::load(&schemas).expect("schemas/");
    let mut controller = ProjectController::new(registry);
    controller.open_project(&root).expect("打开模组");
    (root, controller)
}

/// 导入器会给记录重新生成 id：按名字回查（口径同 fs_sprite.rs）。
fn record_id(controller: &ProjectController, name: &str) -> String {
    controller
        .project()
        .expect("有项目")
        .contents
        .iter()
        .find(|record| record.name == name)
        .and_then(|record| record.id.clone())
        .expect("记录存在")
}

#[test]
fn imported_sprite_is_readable_by_the_record_sprite_kernel() {
    let (root, mut controller) = fixture_project("import-ok");
    let id = record_id(&controller, "kiln");
    assert!(sprite_for(&controller, &id).is_none(), "导入前没有贴图");

    // 源文件放在项目之外的独立目录：导入只读它。
    let source_dir = temp_dir("import-ok-src");
    let source = source_dir.join("picked.png");
    let bytes = png_bytes(&[0x01, 0x02, 0x03, 0x04]);
    fs::write(&source, &bytes).expect("写源 PNG");

    let dto = import_sprite(&mut controller, &id, &source).expect("导入应成功");
    assert_eq!(dto.region, "importme-kiln", "区域名走 mm_fmt::sprite 的既有规则");
    assert!(dto.data_url.starts_with("data:image/png;base64,"));

    // 目标落在读侧认得的位置：sprites/<记录名>.png，字节保真。
    let target = root.join("sprites").join("kiln.png");
    assert_eq!(fs::read(&target).expect("读目标"), bytes);
    assert!(!target.with_file_name("kiln.png.mm-tmp").exists(), "不应留下临时文件");
    assert_eq!(fs::read(&source).expect("源只读"), bytes, "源文件不得被改动");

    // 账本已同步：sprite_for（record_sprite 的内核）立刻能读到同一张图。
    let read_back = sprite_for(&controller, &id).expect("导入后应能读到贴图");
    assert_eq!(read_back.region, dto.region);
    assert_eq!(read_back.data_url, dto.data_url);
    assert!(
        controller
            .project()
            .expect("有项目")
            .assets
            .iter()
            .any(|asset| asset.path == "sprites/kiln.png" && asset.kind == "sprite"),
        "资产账本应登记新贴图"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

#[test]
fn reimport_replaces_bytes_and_updates_the_ledger_in_place() {
    let (root, mut controller) = fixture_project("import-replace");
    let id = record_id(&controller, "kiln");
    let source_dir = temp_dir("import-replace-src");
    let source = source_dir.join("picked.png");

    fs::write(&source, png_bytes(&[0x01])).expect("写第一版");
    import_sprite(&mut controller, &id, &source).expect("首次导入");
    let second = png_bytes(&[0x02, 0x03, 0x04, 0x05, 0x06]);
    fs::write(&source, &second).expect("写第二版");
    let dto = import_sprite(&mut controller, &id, &source).expect("覆盖导入");

    assert_eq!(fs::read(root.join("sprites/kiln.png")).expect("读目标"), second);
    assert_eq!(dto.data_url, sprite_for(&controller, &id).expect("读回").data_url);
    let entries: Vec<_> = controller
        .project()
        .expect("有项目")
        .assets
        .iter()
        .filter(|asset| asset.path == "sprites/kiln.png")
        .collect();
    assert_eq!(entries.len(), 1, "同路径资产只更新，不重复登记");
    assert_eq!(entries[0].size, second.len() as i64);

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

#[test]
fn a_non_png_source_is_refused_and_writes_nothing() {
    let (root, mut controller) = fixture_project("import-bad");
    let id = record_id(&controller, "kiln");
    let source_dir = temp_dir("import-bad-src");
    let source = source_dir.join("fake.png");
    fs::write(&source, b"not a png at all").expect("写假 PNG");

    let Err(err) = import_sprite(&mut controller, &id, &source) else {
        panic!("非 PNG 必须拒绝");
    };
    assert!(matches!(err, SpriteImportError::NotPng(_)), "实际：{err:?}");
    assert!(!root.join("sprites").exists(), "拒绝后不应创建任何目标目录");
    assert!(sprite_for(&controller, &id).is_none());

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

#[test]
fn a_missing_source_is_reported_as_not_found() {
    let (root, mut controller) = fixture_project("import-missing");
    let id = record_id(&controller, "kiln");
    let missing = root.join("no-such-file.png");

    let Err(err) = import_sprite(&mut controller, &id, &missing) else {
        panic!("源不存在必须报错");
    };
    assert!(matches!(err, SpriteImportError::SourceMissing(_)), "实际：{err:?}");
    assert!(sprite_for(&controller, &id).is_none());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn traversal_record_names_are_refused_before_any_write() {
    let (root, mut controller) = fixture_project("import-traversal");
    let id = record_id(&controller, "kiln");
    let source_dir = temp_dir("import-traversal-src");
    let source = source_dir.join("picked.png");
    fs::write(&source, png_bytes(&[0x01])).expect("写源 PNG");

    // 记录名来自磁盘文件，正常导入器不会产出带分隔符的名字；这里经 project_mut
    // 直接构造脏模型，验证内核自身的守卫（与读侧 safe_relative 同一口径）。
    for bad_name in ["../evil", "..\\evil", "a/b", "C:evil", ".."] {
        controller
            .project_mut()
            .expect("有项目")
            .content_by_id_mut(&id)
            .expect("记录")
            .name = bad_name.to_string();
        let Err(err) = import_sprite(&mut controller, &id, &source) else {
            panic!("危险名字 {bad_name:?} 必须拒绝");
        };
        assert!(
            matches!(err, SpriteImportError::Unimportable(_)),
            "名字 {bad_name:?} 实际：{err:?}"
        );
    }
    // 项目根之外没有落下任何文件，项目内也没有 sprites 目录。
    assert!(!root.join("sprites").exists());
    assert!(!root.parent().expect("有父目录").join("evil.png").exists());

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

#[test]
fn an_inline_record_has_no_sprite_file_to_import() {
    let (root, mut controller) = fixture_project("import-inline");
    let id = record_id(&controller, "kiln");
    let source_dir = temp_dir("import-inline-src");
    let source = source_dir.join("picked.png");
    fs::write(&source, png_bytes(&[0x01])).expect("写源 PNG");

    // 内联记录写回宿主文件而非独立文件（is_inline 认 inline_owner_id）。
    controller
        .project_mut()
        .expect("有项目")
        .content_by_id_mut(&id)
        .expect("记录")
        .inline_owner_id = Some("host".to_string());
    let Err(err) = import_sprite(&mut controller, &id, &source) else {
        panic!("内联记录必须拒绝");
    };
    assert!(matches!(err, SpriteImportError::Unimportable(_)), "实际：{err:?}");
    assert!(!root.join("sprites").exists());

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

#[test]
fn an_unknown_record_id_is_not_found() {
    let (root, mut controller) = fixture_project("import-noid");
    let source_dir = temp_dir("import-noid-src");
    let source = source_dir.join("picked.png");
    fs::write(&source, png_bytes(&[0x01])).expect("写源 PNG");

    let Err(err) = import_sprite(&mut controller, "no-such-id", &source) else {
        panic!("未知 id 必须报错");
    };
    assert!(matches!(err, SpriteImportError::RecordNotFound(_)), "实际：{err:?}");

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

/// 导入成功即推一条历史条目；undo 回到导入前（文件不在、读侧看不到），redo 原样重放。
/// 2026-11-25，UI 审计缺陷 #5：贴图变更与字段编辑同栈，undo/redo 走既有命令。
#[test]
fn import_pushes_one_history_entry_and_undo_restores_the_pre_import_state() {
    let (root, mut controller) = fixture_project("import-undo");
    let id = record_id(&controller, "kiln");
    let source_dir = temp_dir("import-undo-src");
    let source = source_dir.join("picked.png");
    let bytes = png_bytes(&[0x11, 0x12]);
    fs::write(&source, &bytes).expect("写源 PNG");

    let depth_before = controller.undo_stack_depth();
    import_sprite(&mut controller, &id, &source).expect("导入应成功");
    assert_eq!(controller.undo_stack_depth(), depth_before + 1, "导入必须推一条历史条目");
    let target = root.join("sprites").join("kiln.png");
    assert!(target.is_file());

    controller.undo().expect("undo 应成功");
    assert!(!target.exists(), "首次导入的 undo 是删掉文件");
    assert!(sprite_for(&controller, &id).is_none(), "账本回滚后读侧应看不到贴图");
    assert!(controller.can_redo(), "undo 之后可重做");

    controller.redo().expect("redo 应成功");
    assert_eq!(fs::read(&target).expect("重放后文件回来"), bytes, "redo 逐字节重放");
    let read_back = sprite_for(&controller, &id).expect("redo 后读侧应重新看到贴图");
    assert!(read_back.data_url.starts_with("data:image/png;base64,"));

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}

/// 覆盖导入（旧文件存在）的 undo 恢复**上一版字节**与账本字节数；redo 再回到新版。
#[test]
fn undo_of_a_reimport_brings_back_the_previous_bytes() {
    let (root, mut controller) = fixture_project("import-undo-replace");
    let id = record_id(&controller, "kiln");
    let source_dir = temp_dir("import-undo-replace-src");
    let source = source_dir.join("picked.png");
    let first = png_bytes(&[0x21]);
    let second = png_bytes(&[0x22, 0x23, 0x24]);
    fs::write(&source, &first).expect("写第一版");
    import_sprite(&mut controller, &id, &source).expect("首次导入");
    fs::write(&source, &second).expect("写第二版");
    import_sprite(&mut controller, &id, &source).expect("覆盖导入");
    let target = root.join("sprites").join("kiln.png");

    controller.undo().expect("undo 应成功");
    assert_eq!(fs::read(&target).expect("读回滚后的文件"), first, "恢复导入前快照");
    let size = controller
        .project()
        .expect("有项目")
        .assets
        .iter()
        .find(|asset| asset.path == "sprites/kiln.png")
        .map(|asset| asset.size)
        .expect("账本条目还在（只是回到旧字节数）");
    assert_eq!(size, first.len() as i64, "账本字节数与文件同步回滚");

    controller.redo().expect("redo 应成功");
    assert_eq!(fs::read(&target).expect("读重放后的文件"), second);

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&source_dir);
}