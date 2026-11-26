//! B 项验收：`ProjectController` 的命令级集成测试（tempdir，无 GUI、无 tauri）。
//!
//! 钉住五件事：
//! 1. create → add → set_field 之后模型与磁盘一致，脏标记跟着翻转；
//! 2. 撤销/重做把字段值改回去，且栈深随命令增减；
//! 3. 保存后**没有改过的** meta 文件逐字节不变（v1 字节保真的控制器侧断言）；
//! 4. 保存 → 重新打开后字段值、记录 id 都能回来；
//! 5. `require_project()` 之外，这些命令在正常路径上不 panic（任何一步 unwrap 落空都会让测试红）。
//!
//! 不依赖机器上的真实 mod 夹具：临时目录里自建项目。

use std::fs;
use std::path::{Path, PathBuf};

use mm_app::project_controller::ProjectController;
use mm_json::Json;
use mm_schema::{ContentType, SchemaRegistry};

/// 临时项目根：Drop 时尽力删除（断言失败也不留垃圾）。
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mm-controller-flow-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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

/// meta 文件在磁盘上的原始字节（作者层 `modmaker/` 不在此列）。
fn meta_bytes(root: &Path) -> Vec<u8> {
    for name in ["mod.hjson", "mod.json"] {
        let path = root.join(name);
        if path.exists() {
            return fs::read(path).expect("读 meta");
        }
    }
    panic!("项目里没有 meta 文件：{}", root.display());
}

#[test]
fn create_edit_undo_save_and_reopen() {
    let root = TempRoot::new();
    let root_path = root.path().to_path_buf();

    let mut controller = ProjectController::new(registry());
    controller
        .create_project("flow-test", &root_path)
        .expect("建项目");
    let meta_before = meta_bytes(&root_path);
    assert!(!controller.is_dirty(), "刚建好的项目不该是脏的");

    let id = controller.add_content(ContentType::Item);
    assert!(!id.is_empty(), "add_content 必须给 id");
    controller.set_field(&id, "hardness", Some(Json::num("3")));
    assert!(controller.is_dirty(), "改过字段就必须脏");

    let depth = controller.undo_stack_depth();
    assert!(depth >= 2, "add + set_field 至少两条命令，实际 {depth}");

    // 撤销一次：把 hardness 撤掉（回到“没有这个字段”的初值）。
    // 2026-11-25 起 `undo()` 带 `Result`（贴图命令写磁盘可失败，见 `CommandError`）；
    // 纯内存命令恒 `Ok`，所以这里照旧 expect。
    controller.undo().expect("内存命令的撤销不该失败");
    let after_undo = controller
        .project()
        .and_then(|project| project.content_by_id(&id))
        .expect("撤销不该删掉记录")
        .fields
        .get("hardness")
        .cloned();
    assert_eq!(after_undo, None, "撤销应把 hardness 撤回未设置");
    assert!(controller.can_redo(), "撤销之后应当能重做");

    controller.redo().expect("内存命令的重做不该失败");
    let after_redo = controller
        .project()
        .and_then(|project| project.content_by_id(&id))
        .and_then(|record| record.fields.get("hardness").cloned());
    assert_eq!(after_redo, Some(Json::num("3")), "重做应恢复 hardness");

    controller.save().expect("保存");
    assert!(!controller.is_dirty(), "保存后脏标记必须清掉");
    assert_eq!(
        meta_bytes(&root_path),
        meta_before,
        "没改过的 meta 文件在保存后必须逐字节不变"
    );

    // 重新打开：字段值与记录 id 都要回来。
    let mut reopened = ProjectController::new(registry());
    reopened.open_project(&root_path).expect("重新打开");
    let record = reopened
        .project()
        .and_then(|project| project.content_by_id(&id))
        .expect("记录 id 应在保存后仍可定位");
    assert_eq!(record.fields.get("hardness"), Some(&Json::num("3")));
    assert!(!reopened.is_dirty());
}

/// `save` 在空项目上也不该 panic；连续两次保存是幂等的（meta 仍是同一份字节）。
#[test]
fn saving_twice_keeps_the_same_bytes() {
    let root = TempRoot::new();
    let root_path = root.path().to_path_buf();
    let mut controller = ProjectController::new(registry());
    controller
        .create_project("twice", &root_path)
        .expect("建项目");

    controller.save().expect("第一次保存");
    let first = fs::read(root_path.join("mod.hjson")).expect("读 meta");
    controller.save().expect("第二次保存");
    let second = fs::read(root_path.join("mod.hjson")).expect("读 meta");
    assert_eq!(first, second, "重复保存必须写出同一份字节");
}

/// 格式切换：转换成 hjson 之后保存，内容文件真的换了扩展名；meta 也一起改名。
#[test]
fn convert_format_renames_files_on_the_next_save() {
    let root = TempRoot::new();
    let root_path = root.path().to_path_buf();
    let mut controller = ProjectController::new(registry());
    controller.create_project("convert", &root_path).expect("建项目");
    let id = controller.add_content(ContentType::Item);
    let name = controller
        .project()
        .and_then(|project| project.content_by_id(&id))
        .map(|record| record.name.clone())
        .expect("新记录");
    controller.save().expect("先落一次 json");

    let pending = controller.convert_format("hjson").expect("格式切换");
    assert!(pending >= 1, "至少有一条内容要转换，实际 {pending}");
    assert!(controller.is_dirty(), "格式切换必须让项目变脏");

    controller.save().expect("转换后保存");
    let hjson = root_path.join("content/items").join(format!("{name}.hjson"));
    assert!(hjson.is_file(), "转换后应有 {hjson:?}");
    let json = root_path.join("content/items").join(format!("{name}.json"));
    assert!(!json.exists(), "旧扩展名的文件应当被清掉");
    // 新建项目的 meta 本来就是 mod.hjson，转换成 hjson 时它不需要改名。
    assert!(root_path.join("mod.hjson").is_file(), "meta 应留在 mod.hjson");
}

/// 加固回归（2026-11-25）：非法目标扩展名在控制器层得到错误而非 panic，
/// 且项目不被改动、不额外变脏（领域层校验先于一切写入）。
#[test]
fn convert_format_with_unsupported_extension_is_an_error() {
    let root = TempRoot::new();
    let root_path = root.path().to_path_buf();
    let mut controller = ProjectController::new(registry());
    controller.create_project("badext", &root_path).expect("建项目");
    controller.save().expect("先落一次盘");
    let dirty_before = controller.is_dirty();
    let err = controller.convert_format("yaml").expect_err("yaml 不是合法内容格式");
    assert_eq!(err.to_string(), "unsupported content format: yaml");
    assert_eq!(controller.is_dirty(), dirty_before, "失败路径不得把项目弄脏");
}

/// 整板重排与重算连线在空画板上是安全的空操作（命令面会先查项目再调）。
#[test]
fn tidy_and_rebuild_are_safe_on_an_empty_board() {
    let root = TempRoot::new();
    let root_path = root.path().to_path_buf();
    let mut controller = ProjectController::new(registry());
    controller.create_project("layout", &root_path).expect("建项目");
    controller.tidy_layout();
    assert_eq!(controller.rebuild_links(), 0, "空画板没有边可改");
    assert!(controller.project().expect("有项目").boards[0].edges.is_empty());
}
