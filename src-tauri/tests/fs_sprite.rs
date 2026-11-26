//! `fs_list_dir` / `record_sprite` 内核的测试（tauri-free 模块，直接从 lib 引用）。
//!
//! 这两个模块刻意不依赖 tauri：本机链接 tauri 的 lib 单元测试二进制加载即死
//! （0xC0000139），放这里能真跑。

use std::fs;
use std::path::PathBuf;

use graphitecreator_lib::fs_explore::{is_archive_name, list_dir};
use graphitecreator_lib::sprite_asset::{base64_encode, read_sprite, safe_relative};

fn temp_dir(tag: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("gc-{tag}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

#[test]
fn base64_matches_the_rfc_examples() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_encode(&[0x00, 0xFF, 0x10]), "AP8Q");
}

#[test]
fn archive_names_are_case_insensitive() {
    assert!(is_archive_name("MyMod.ZIP"));
    assert!(is_archive_name("mod.jar"));
    assert!(!is_archive_name("mod.hjson"));
    assert!(!is_archive_name("zip"));
}

#[test]
fn listing_puts_directories_first_and_flags_mods() {
    let root = temp_dir("fs-list");
    fs::create_dir_all(root.join("B-mod")).expect("建目录");
    fs::write(root.join("B-mod").join("mod.hjson"), "name: b\n").expect("写 meta");
    fs::write(root.join("a.zip"), b"PK").expect("写归档");
    fs::write(root.join("note.txt"), b"x").expect("写文件");

    let listing = list_dir(&root.to_string_lossy()).expect("列目录");
    let names: Vec<&str> = listing.entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, vec!["B-mod", "a.zip", "note.txt"], "目录在前，其余按名字");

    assert!(listing.entries[0].is_dir);
    assert!(listing.entries[0].is_mod, "含 mod.hjson 的目录要标成模组");
    assert!(listing.entries[1].is_archive);
    assert!(!listing.entries[1].is_dir);
    assert_eq!(listing.parent, root.parent().map(|parent| parent.to_string_lossy().into_owned()));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_plain_file_and_an_empty_path_are_refused() {
    let root = temp_dir("fs-list-file");
    let file = root.join("mod.json");
    fs::write(&file, b"{}").expect("写文件");
    assert!(list_dir(&file.to_string_lossy()).is_err(), "文件不是目录");
    assert!(list_dir("").is_err(), "空路径");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn traversal_paths_are_not_safe() {
    assert!(safe_relative("sprites/kiln.png"));
    assert!(!safe_relative("sprites/../../mod.json"));
    assert!(!safe_relative("C:/x.png"));
}

#[test]
fn a_file_without_a_png_signature_is_not_a_sprite() {
    let dir = temp_dir("sprite-bad");
    let path = dir.join("bad.png");
    fs::write(&path, b"not a png").expect("写文件");
    assert!(read_sprite(&path, "r".to_string()).is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_real_png_becomes_a_data_url() {
    let dir = temp_dir("sprite-ok");
    let path = dir.join("ok.png");
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&[0x00, 0x01, 0x02]);
    fs::write(&path, &bytes).expect("写文件");

    let sprite = read_sprite(&path, "kiln".to_string()).expect("应当解出一张图");
    assert_eq!(sprite.region, "kiln");
    assert!(sprite.data_url.starts_with("data:image/png;base64,iVBORw0KGgo"));
    let _ = fs::remove_dir_all(&dir);
}

/// `record_sprite` 的内核端到端：打开一个真实模组目录 → 按记录 id 找到贴图。
#[test]
fn an_opened_project_resolves_its_record_sprite() {
    use graphitecreator_lib::sprite_asset::sprite_for;
    use mm_app::project_controller::ProjectController;
    use mm_schema::SchemaRegistry;
    use std::path::Path;

    let root = temp_dir("sprite-project");
    fs::create_dir_all(root.join("content/items")).expect("建内容目录");
    fs::create_dir_all(root.join("sprites")).expect("建贴图目录");
    fs::write(root.join("mod.json"), br#"{"name":"spriteme","minGameVersion":"160"}"#)
        .expect("写 meta");
    fs::write(root.join("content/items/kiln.json"), br#"{"name":"kiln","hardness":2}"#)
        .expect("写内容");
    let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x01, 0x02];
    fs::write(root.join("sprites/kiln.png"), png).expect("写贴图");

    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas");
    let registry = SchemaRegistry::load(&schemas).expect("schemas/");
    let mut controller = ProjectController::new(registry);
    controller.open_project(&root).expect("打开模组");

    let id = controller
        .project()
        .and_then(|project| project.content_by_id("kiln"))
        .and_then(|record| record.id.clone());
    let id = match id {
        Some(id) => id,
        None => {
            // 导入会给记录重新生成 id：按名字找。
            controller
                .project()
                .expect("有项目")
                .contents
                .iter()
                .find(|record| record.name == "kiln")
                .and_then(|record| record.id.clone())
                .expect("kiln 记录")
        }
    };

    let sprite = sprite_for(&controller, &id).expect("应当找到 sprites/kiln.png");
    assert_eq!(sprite.region, "spriteme-kiln", "区域名走 mm_fmt::sprite 的命名规则");
    assert!(sprite.data_url.starts_with("data:image/png;base64,"));
    assert!(sprite_for(&controller, "no-such-id").is_none());
    let _ = fs::remove_dir_all(&root);
}
