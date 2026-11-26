//! DTO 契约快照：前端按这些 JSON 键名编码，**改名就是 breaking change**。
//!
//! 钉住键集合与几种有形状讲究的枚举（null / untagged / 数字源文 / 保键序对象）。
//! 新增命令时，对应的 DTO 必须在本文件补一条断言。
//!
//! 为什么放在 `tests/` 而不是 `dto.rs` 的 `#[cfg(test)]`：本机（Win11 Insider，
//! x86_64-pc-windows-gnu）链接 tauri 的 **lib 单元测试**可执行文件加载即死
//! （0xC0000139），集成测试二进制不受影响（与 `forest.rs` 抽出 tauri-free 模块同一原因）。

use graphitecreator_lib::dto::*;
use mm_io::import::progress::Stage;
use mm_layout::edge_path_math::Style;
use serde::Serialize;
use serde_json::Value;

fn value_of<T: Serialize>(dto: &T) -> Value {
    serde_json::to_value(dto).expect("DTO 必须可序列化")
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let mut actual: Vec<String> = value
        .as_object()
        .expect("对象形状")
        .keys()
        .cloned()
        .collect();
    actual.sort();
    let mut expected: Vec<String> = expected.iter().map(|key| key.to_string()).collect();
    expected.sort();
    assert_eq!(actual, expected, "JSON 键名与契约不符：{value}");
}

fn sample_record() -> RecordDto {
    RecordDto {
        id: Some("i1".into()),
        content_type: "item".into(),
        name: "kiln".into(),
        class_name: "Block".into(),
        class_source: "default".into(),
        patch: false,
        inline: false,
        dirty: false,
        parse_failed: false,
        raw_path: Some("content/items/kiln.hjson".into()),
        fields: vec![("hardness".into(), JsonDto::Text("3".into()))],
    }
}

#[test]
fn record_dto_and_selection_shapes() {
    let expected = [
        "id",
        "contentType",
        "name",
        "className",
        "classSource",
        "patch",
        "inline",
        "dirty",
        "parseFailed",
        "rawPath",
        "fields",
    ];
    // R-02：className 是有效类，classSource 是它的来源（三种拼写见 `mm_app::class_resolution`）。
    let sample = value_of(&sample_record());
    assert_keys(&sample, &expected);
    assert_eq!(sample["className"], "Block");
    assert_eq!(sample["classSource"], "default");
    // `SelectionDto` 是 untagged：Selected 直接是记录，None 是 null。
    assert_keys(&value_of(&SelectionDto::Selected(sample_record())), &expected);
    assert_eq!(value_of(&SelectionDto::None), Value::Null);
}

#[test]
fn json_dto_keeps_number_spelling_and_key_order() {
    assert_eq!(value_of(&JsonDto::Text("0.50".into())), Value::String("0.50".into()));
    let object = JsonDto::Object(vec![
        ("zeta".into(), JsonDto::Text("1".into())),
        ("alpha".into(), JsonDto::Text("2".into())),
    ]);
    let text = serde_json::to_string(&object).expect("序列化");
    assert_eq!(text, r#"{"zeta":"1","alpha":"2"}"#, "对象必须保插入序");
    assert_eq!(value_of(&JsonDto::Null), Value::Null);
}

#[test]
fn empty_board_snapshot_shape() {
    let snapshot = board_snapshot_dto(None, None, None, Style::Bezier, &[], Vec::new());
    let value = value_of(&snapshot);
    assert_keys(
        &value,
        &["boards", "board", "nodes", "cards", "edges", "edgeStyle", "viewport", "issues"],
    );
    assert_eq!(value["board"], Value::Null);
    assert_eq!(value["edgeStyle"], "bezier");
    assert_eq!(value["viewport"], Value::Null);
}

#[test]
fn summary_meta_history_and_session_shapes() {
    let summary = value_of(&project_summary_dto(None, false));
    assert_keys(
        &summary,
        &[
            "displayName",
            "internalName",
            "root",
            "contentCount",
            "boardCount",
            "dirty",
            "locales",
            "schemaProfile",
            "importedFrom",
            "authoringWarning",
            "meta",
        ],
    );
    assert_keys(
        &summary["meta"],
        &["name", "displayName", "author", "description", "version", "minGameVersion"],
    );

    let history = HistoryStateDto { can_undo: true, can_redo: false, undo_depth: 2, dirty: true };
    assert_keys(&value_of(&history), &["canUndo", "canRedo", "undoDepth", "dirty"]);

    let session = GameSessionDto { running: true, data_dir: Some("d".into()), mod_folder: None };
    assert_keys(&value_of(&session), &["running", "dataDir", "modFolder"]);

    let progress = ImportProgressDto { stage: stage_key(Stage::Writing).into(), done: 3, total: 9 };
    let value = value_of(&progress);
    assert_keys(&value, &["stage", "done", "total"]);
    assert_eq!(value["stage"], "writing");
}

#[test]
fn list_and_strings_shapes() {
    assert_keys(&value_of(&strings_dto(None)), &["locales", "entries"]);

    let item = RecordListItemDto {
        id: None,
        content_type: "block".into(),
        name: "wall".into(),
        display_name: "Wall".into(),
        patch: false,
        inline: false,
        dirty: false,
        color: "#ffffff".into(),
    };
    assert_keys(
        &value_of(&item),
        &["id", "contentType", "name", "displayName", "patch", "inline", "dirty", "color"],
    );
}

#[test]
fn card_rule_shapes() {
    let card = CardDto {
        node: Some("n1".into()),
        carrier: false,
        title: "Kiln".into(),
        color: "#ffaa00".into(),
        widget_fields: vec!["size".into()],
        rows: vec![CardRowDto { name: "size".into(), text: "2".into() }],
        input_fields: vec!["size".into()],
        subtitle: None,
        type_label: None,
    };
    let value = value_of(&card);
    assert_keys(
        &value,
        &[
            "node",
            "carrier",
            "title",
            "color",
            "widgetFields",
            "rows",
            "inputFields",
            "subtitle",
            "typeLabel",
        ],
    );
    assert_keys(&value["rows"][0], &["name", "text"]);
}

#[test]
fn issue_args_and_refs_shapes() {
    let issue = IssueDto {
        level: "warning".into(),
        key: "missing.sprite".into(),
        args: vec![ArgDto::Text("kiln".into()), ArgDto::Int(2), ArgDto::Bool(true), ArgDto::Null],
        content_id: None,
        message: "missing sprite".into(),
    };
    let value = value_of(&issue);
    assert_keys(&value, &["level", "key", "args", "contentId", "message"]);
    assert_eq!(value["args"], serde_json::json!(["kiln", 2, true, null]), "ArgDto 是 untagged");

    let board_ref =
        BoardRefDto { index: 0, name: "Main".into(), file: "main.canvas".into(), current: true };
    assert_keys(&value_of(&board_ref), &["index", "name", "file", "current"]);

    assert_keys(&value_of(&ViewportDto { x: 0.0, y: 0.0, zoom: 1.0 }), &["x", "y", "zoom"]);
}

#[test]
fn record_schema_and_field_spec_shapes() {
    let spec = FieldSpecDto {
        name: "size".into(),
        control: "number".into(),
        description: None,
        enum_values: Vec::new(),
        content_ctype: None,
        stack_kind: None,
        default_value: None,
        curated: false,
        shown: true,
        text: "2".into(),
        checked: false,
        swatch: "#ffffff".into(),
        candidates: Vec::new(),
    };
    assert_keys(
        &value_of(&spec),
        &[
            "name",
            "control",
            "description",
            "enumValues",
            "contentCtype",
            "stackKind",
            "defaultValue",
            "curated",
            "shown",
            "text",
            "checked",
            "swatch",
            "candidates",
        ],
    );
    let schema = RecordSchemaDto {
        record_id: Some("b1".into()),
        content_type: "block".into(),
        class_name: "mindustry.content.blocks.GenericCrafter".into(),
        class_source: "declared".into(),
        fields: vec![spec],
        values: Vec::new(),
    };
    assert_keys(
        &value_of(&schema),
        &["recordId", "contentType", "className", "classSource", "fields", "values"],
    );
}

#[test]
fn build_and_export_report_shapes() {
    let plan = BuildPlanDto {
        has_errors: false,
        error_count: 0,
        warning_count: 1,
        root: "r".into(),
        internal_name: "kiln-mod".into(),
        issues: Vec::new(),
    };
    assert_keys(
        &value_of(&plan),
        &["hasErrors", "errorCount", "warningCount", "root", "internalName", "issues"],
    );
    let report =
        ExportReportDto { has_errors: false, error_count: 0, warning_count: 2, summary: "ok".into() };
    assert_keys(&value_of(&report), &["hasErrors", "errorCount", "warningCount", "summary"]);
}

#[test]
fn tech_forest_shapes() {
    let research = ResearchDto {
        parent: Some("kiln".into()),
        root: false,
        name: None,
        requires_unlock: true,
        planet: None,
        reqs: vec!["copper/10".into()],
        objectives: Vec::new(),
    };
    assert_keys(
        &value_of(&research),
        &["parent", "root", "name", "requiresUnlock", "planet", "reqs", "objectives"],
    );
    let node = TechNodeDto {
        id: Some("t1".into()),
        content_type: "item".into(),
        name: "kiln".into(),
        display_name: "Kiln".into(),
        patch: false,
        color: "#ffffff".into(),
        research: Some(research),
    };
    assert_keys(
        &value_of(&node),
        &["id", "contentType", "name", "displayName", "patch", "color", "research"],
    );
    let entry = TechEntryDto {
        record: Some(node),
        key: "kiln".into(),
        vanilla: false,
        problem: Some("ignored".into()),
        children: Vec::new(),
    };
    assert_keys(&value_of(&entry), &["record", "key", "vanilla", "problem", "children"]);
    let forest = TechForestDto { roots: vec![entry], unassigned: Vec::new(), refused: Vec::new() };
    assert_keys(&value_of(&forest), &["roots", "unassigned", "refused"]);
}

#[test]
fn fs_listing_and_sprite_shapes() {
    let listing = FsListingDto {
        path: "C:/mods".into(),
        parent: Some("C:/".into()),
        entries: vec![FsEntryDto {
            name: "MyMod".into(),
            path: "C:/mods/MyMod".into(),
            is_dir: true,
            is_mod: true,
            is_archive: false,
        }],
    };
    let value = value_of(&listing);
    assert_keys(&value, &["path", "parent", "entries"]);
    assert_keys(&value["entries"][0], &["name", "path", "isDir", "isMod", "isArchive"]);

    let sprite = SpriteDto { data_url: "data:image/png;base64,AA==".into(), region: "kiln".into() };
    assert_keys(&value_of(&sprite), &["dataUrl", "region"]);
}

#[test]
fn cosmos_scene_shapes() {
    use graphitecreator_lib::dto::cosmos_scene_dto;
    use mm_app::cosmos::{
        CosmosAppearance, CosmosAtmosphere, CosmosBody, CosmosGrid, CosmosGridTile, CosmosLayer,
        CosmosScene, CosmosSystem, CosmosTile, SceneKind,
    };

    let scene = CosmosScene {
        record_id: "p1".into(),
        kind: SceneKind::Planet,
        sector_size: 2,
        radius: 1.0,
        grid: Some(CosmosGrid {
            size: 2,
            capacity: 93,
            tiles: vec![CosmosGridTile {
                index: 0,
                center: [0.0, 0.0, 1.0],
                corners: vec![0.0, 0.0, 1.0, 0.1, 0.0, 0.99],
                occupied: true,
                sector_ids: vec!["s1".into()],
            }],
        }),
        appearance: Some(CosmosAppearance {
            mesh_type: "NoiseMesh".into(),
            surface: vec![CosmosLayer {
                radius: 1.0,
                intensity: 0.2,
                unlit: false,
                sky: false,
                cloud_speed: 0.0,
                tiles: vec![CosmosTile {
                    corners: vec![0.0, 0.0, 1.0],
                    color: Some([0.3, 0.7, 1.0, 1.0]),
                    skipped: false,
                }],
            }],
            clouds: Vec::new(),
            light: [0.6, 0.0, -0.8],
            atmosphere: CosmosAtmosphere { color: "#4db3ff".into(), rad_out: 0.3 },
            max_extent: 1.04,
        }),
        system: CosmosSystem {
            bodies: vec![CosmosBody {
                name: "serpulo".into(),
                radius: 1.0,
                orbit_radius: 3.0,
                color: Some("5f666e".into()),
                selected: false,
                vanilla: true,
                star: false,
            }],
            sun_colors: ["ff7a38", "ff9638", "ffc64c", "ffe371"].map(str::to_string),
        },
    };

    let value = value_of(&cosmos_scene_dto(&scene));
    assert_keys(
        &value,
        &["recordId", "kind", "sectorSize", "radius", "grid", "appearance", "system"],
    );
    assert_eq!(value["kind"], "planet");
    assert_keys(&value["grid"], &["size", "capacity", "tiles"]);
    assert_keys(
        &value["grid"]["tiles"][0],
        &["index", "center", "corners", "occupied", "sectorIds"],
    );
    let appearance = &value["appearance"];
    assert_keys(
        appearance,
        &["meshType", "surface", "clouds", "light", "atmosphere", "maxExtent"],
    );
    assert_keys(
        &appearance["surface"][0],
        &["radius", "intensity", "unlit", "sky", "cloudSpeed", "tiles"],
    );
    assert_keys(&appearance["surface"][0]["tiles"][0], &["corners", "color", "skipped"]);
    assert_keys(&appearance["atmosphere"], &["color", "radOut"]);
    assert_keys(&value["system"], &["bodies", "sunColors"]);
    assert_keys(
        &value["system"]["bodies"][0],
        &["name", "radius", "orbitRadius", "color", "selected", "vanilla", "star"],
    );
}
