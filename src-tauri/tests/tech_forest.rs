//! M7a `tech_forest` 下发映射的钉子：领域层森林（`mm_io::canvas::tech_tree_index`，
//! Java `TechTreeIndex` 的移植）→ 前端 DTO 这一段此前没有测试覆盖。
//!
//! 钉住四件事：
//! 1. 七类记录各归其位（root/子节点/悬空父名/无父名/原版组/无 research/patch 拒绝）；
//! 2. research 摘要由 `mm_fmt::tech::read` 归一——`{item, amount}` 对象变成 `"item/amount"`
//!    文本（ContentParser.java:466 的游戏口径），objectives 原样保留；
//! 3. 问题键与前端约定逐字相同（`unresolvedParent` / `ignored`）；
//! 4. 无项目时回空森林，不 panic。
//!
//! 不依赖机器上的真实 mod 夹具：临时目录里 `create_project` + `add_content` 自建八条记录。

use std::fs;
use std::path::PathBuf;

use graphitecreator_lib::forest::tech_forest_dto;
use mm_app::project_controller::ProjectController;
use mm_json::{Json, Object};
use mm_schema::{ContentType, SchemaRegistry};

/// 临时项目根：Drop 时尽力删除（断言失败也不留垃圾）。
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mm-m7a-forest-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("建临时项目根");
        TempRoot(dir)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn obj(pairs: &[(&str, Json)]) -> Json {
    let mut object = Object::new();
    for (key, value) in pairs {
        object.insert(*key, value.clone());
    }
    Json::Obj(object)
}

fn add_item(controller: &mut ProjectController, research: Option<Json>) -> String {
    let id = controller.add_content(ContentType::Item);
    if let Some(value) = research {
        controller.set_field(&id, "research", Some(value));
    }
    id
}

fn name_of(controller: &ProjectController, id: &str) -> String {
    controller
        .project()
        .and_then(|project| project.content_by_id(id))
        .map(|record| record.name.clone())
        .expect("刚建的记录应当在项目里")
}

#[test]
fn forest_dto_classifies_and_normalizes() {
    let registry =
        SchemaRegistry::load_default().expect("schemas 目录（cargo 测试 CWD=src-tauri）");
    let vanilla = registry
        .vanilla_names(ContentType::Item)
        .first()
        .cloned()
        .expect("schema 应当带原版 item 名单");
    let mut controller = ProjectController::new(registry);

    // 无项目：空森林。
    let empty = tech_forest_dto(&controller);
    assert!(empty.roots.is_empty() && empty.unassigned.is_empty() && empty.refused.is_empty());

    let root = TempRoot::new();
    controller
        .create_project("testmod", &root.0)
        .expect("建项目");

    let a = add_item(&mut controller, Some(obj(&[("root", Json::Bool(true))])));
    let a_name = name_of(&controller, &a);
    let b = add_item(&mut controller, Some(Json::Str(a_name.clone())));
    let b_name = name_of(&controller, &b);
    let c = add_item(
        &mut controller,
        Some(obj(&[(
            "parent",
            Json::Str("no-such-content-xyz".to_string()),
        )])),
    );
    let c_name = name_of(&controller, &c);
    let d = add_item(&mut controller, Some(obj(&[])));
    let d_name = name_of(&controller, &d);
    let e = add_item(&mut controller, Some(Json::Str(vanilla.clone())));
    let e_name = name_of(&controller, &e);
    let f = add_item(&mut controller, None);
    let f_name = name_of(&controller, &f);
    let g = add_item(&mut controller, Some(Json::Str(a_name.clone())));
    let g_name = name_of(&controller, &g);
    controller.set_patch(&g, true);
    let h = add_item(
        &mut controller,
        Some(obj(&[
            ("parent", Json::Str(a_name.clone())),
            (
                "requirements",
                Json::Arr(vec![obj(&[
                    ("item", Json::Str("copper".to_string())),
                    ("amount", Json::Num("10".to_string())),
                ])]),
            ),
            (
                "objectives",
                Json::Arr(vec![obj(&[
                    ("type", Json::Str("Research".to_string())),
                    ("content", Json::Str("copper".to_string())),
                ])]),
            ),
            ("planet", Json::Str("serpulo".to_string())),
        ])),
    );
    let h_name = name_of(&controller, &h);

    let dto = tech_forest_dto(&controller);
    let find = |key: &str| dto.roots.iter().find(|entry| entry.key == key);

    // root a：problem 为空，research.root = true，b/h 挂在下面。
    let a_entry = find(&a_name).expect("root 节点应当成根");
    assert_eq!(a_entry.problem, None);
    assert!(!a_entry.vanilla);
    let a_record = a_entry.record.as_ref().expect("模组节点带记录摘要");
    assert!(a_record.research.as_ref().expect("root 带 research").root);
    assert!(!a_record.display_name.is_empty());
    let child_keys: Vec<&str> = a_entry.children.iter().map(|c| c.key.as_str()).collect();
    assert!(
        child_keys.contains(&b_name.as_str()),
        "裸父名应当接上: {child_keys:?}"
    );
    assert!(
        child_keys.contains(&h_name.as_str()),
        "对象父名应当接上: {child_keys:?}"
    );

    // h：requirements 对象归一成 "item/amount" 文本，objectives/planet 原样。
    let h_entry = a_entry.children.iter().find(|c| c.key == h_name).unwrap();
    let h_research = h_entry.record.as_ref().unwrap().research.as_ref().unwrap();
    assert_eq!(h_research.reqs, vec!["copper/10".to_string()]);
    assert_eq!(h_research.planet.as_deref(), Some("serpulo"));
    assert_eq!(h_research.parent.as_deref(), Some(a_name.as_str()));
    assert_eq!(h_research.objectives.len(), 1);

    // 悬空父名 / 无父名：问题键与前端约定逐字相同，且仍出现在 roots 里。
    assert_eq!(
        find(&c_name).unwrap().problem.as_deref(),
        Some("unresolvedParent")
    );
    assert_eq!(find(&d_name).unwrap().problem.as_deref(), Some("ignored"));

    // 原版组：record 为空、vanilla 标记、e 挂在组下。
    let group = dto
        .roots
        .iter()
        .find(|entry| entry.vanilla && entry.key == vanilla)
        .expect("原版父名应当成组");
    assert!(group.record.is_none());
    assert_eq!(group.children.len(), 1);
    assert_eq!(group.children[0].key, e_name);

    // 无 research：unassigned，摘要为空。
    assert!(dto.unassigned.iter().any(|node| node.name == f_name));
    assert!(dto.unassigned.iter().all(|node| node.research.is_none()));

    // patch 带 research：refused，永不入树。
    assert!(
        dto.refused
            .iter()
            .any(|node| node.name == g_name && node.patch)
    );
    assert!(find(&g_name).is_none());
}
