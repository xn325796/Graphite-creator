//! R-02 验收：脚本类推断在「打开项目 → 有效类 → 保存」这条链上的端到端行为。
//!
//! 钉住四件事：
//! 1. `scripts/**/*.js` 的 `extend(<Class>, '<名>')` 在打开项目时被扫进 `script_classes`；
//! 2. 没写 `type` 的方块因此拿到脚本推断出的有效类（`PayloadBlock`），来源是 `script`；
//! 3. 保存**不把推断写回磁盘**：内容文件里仍然没有 `type`，作者层 `project.json` 里也没有
//!    这张表（它每次打开重扫，不落盘）；
//! 4. 重新打开后行为完全一致（推断来自脚本，不来自任何缓存）。
//!
//! 用临时目录自建最小模组，不依赖机器上的真实 mod 夹具。

use std::fs;
use std::path::{Path, PathBuf};

use mm_app::class_resolution::ClassSource;
use mm_app::project_controller::ProjectController;
use mm_schema::SchemaRegistry;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mm-script-class-{}-{:?}",
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

    fn write(&self, relative: &str, text: &str) {
        let path = self
            .0
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("建父目录");
        }
        fs::write(&path, text).expect("写夹具文件");
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

/// 一个最小的真实模组：方块 JSON 故意不写 `type`，类只写在脚本里。
fn write_fixture(root: &TempRoot) {
    root.write(
        "mod.hjson",
        "{\"name\":\"script-infer-test\",\"displayName\":\"Script Infer Test\",\
         \"minGameVersion\":\"160\"}\n",
    );
    root.write(
        "content/blocks/payloadPad.json",
        "{\"health\":200,\"size\":2}\n",
    );
    root.write(
        "scripts/main.js",
        "const pad = extend(PayloadBlock, 'payloadPad', {});\n",
    );
}

#[test]
fn script_extend_decides_the_effective_class_and_never_leaks_to_disk() {
    let root = TempRoot::new();
    write_fixture(&root);

    let mut controller = ProjectController::new(registry());
    controller.open_project(root.path()).expect("打开项目");
    let project = controller.project().expect("项目已打开");
    assert_eq!(
        project.script_classes.get("payloadPad").map(String::as_str),
        Some("PayloadBlock"),
        "打开项目时必须扫到脚本里的 extend"
    );

    let record = project
        .content_by_name(mm_schema::ContentType::Block, "payloadPad")
        .expect("方块记录应在");
    assert_eq!(record.class_name(), None, "文件里本来就没有 type");
    let (class_name, source) = controller.class_name_and_source_of(record);
    assert_eq!(class_name, "PayloadBlock", "有效类必须来自脚本");
    assert_eq!(source, ClassSource::Script);
    assert_eq!(controller.class_name_of(record), "PayloadBlock");

    // 保存：内容文件不得长出 `type`，作者层不得写这张表。
    controller.save().expect("保存");
    let saved = fs::read_to_string(root.path().join("content/blocks/payloadPad.json"))
        .expect("读保存后的内容文件");
    assert!(!saved.contains("type"), "推断结果不得写回内容文件：{saved}");
    let authoring =
        fs::read_to_string(root.path().join("modmaker/project.json")).expect("作者层应在");
    assert!(
        !authoring.contains("scriptClasses") && !authoring.contains("script_classes"),
        "脚本推断表永不落盘：{authoring}"
    );

    // 重新打开：表由脚本重建，有效类仍是 PayloadBlock。
    let mut reopened = ProjectController::new(registry());
    reopened.open_project(root.path()).expect("重新打开");
    let project = reopened.project().expect("项目已打开");
    let record = project
        .content_by_name(mm_schema::ContentType::Block, "payloadPad")
        .expect("方块记录应在");
    assert_eq!(
        project.script_classes.get("payloadPad").map(String::as_str),
        Some("PayloadBlock"),
        "重开必须重新扫描脚本"
    );
    assert_eq!(reopened.class_name_of(record), "PayloadBlock");
    assert_eq!(record.class_name(), None, "重开也不得把推断写进 type");
}

/// 没有 `scripts/` 目录的模组：表为空，有效类回落默认类 `Block`（与移植前一致）。
#[test]
fn mod_without_scripts_falls_back_to_the_default_class() {
    let root = TempRoot::new();
    root.write(
        "mod.hjson",
        "{\"name\":\"no-scripts\",\"minGameVersion\":\"160\"}\n",
    );
    root.write("content/blocks/plainBlock.json", "{\"health\":40}\n");

    let mut controller = ProjectController::new(registry());
    controller.open_project(root.path()).expect("打开项目");
    let project = controller.project().expect("项目已打开");
    assert!(project.script_classes.is_empty());
    let record = project
        .content_by_name(mm_schema::ContentType::Block, "plainBlock")
        .expect("方块记录应在");
    let (class_name, source) = controller.class_name_and_source_of(record);
    assert_eq!(class_name, "Block");
    assert_eq!(source, ClassSource::Default);
}
