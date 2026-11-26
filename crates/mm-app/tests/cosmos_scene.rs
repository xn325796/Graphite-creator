//! 星球战役场景（`mm_app::cosmos::scene`）的端到端测试：从工程模型到 IPC 契约里的场景数据。
//!
//! 数值口径与 Java `PlanetPreview.setSource/rebuildAppearance` 一致；网格/噪声本身由
//! `mm-fmt` 的移植测试钉住，这里只验组装：sector 解析、网格占用、层钳制、大气、光照、轨道。

use mm_app::cosmos::{scene, SceneError, SceneKind};
use mm_json::{Json, Object};
use mm_model::{ContentRecord, ModProject};
use mm_schema::ContentType;

fn new_project() -> ModProject {
    let mut project = ModProject::default();
    project.meta.name = "Test Mod".to_string();
    project
}

fn planet(
    project: &mut ModProject,
    name: &str,
    parent: Option<&str>,
    size: i32,
    radius: f32,
    mesh: Option<Json>,
) -> ContentRecord {
    let mut record = ContentRecord::create(&format!("id-{name}"), ContentType::Planet, name);
    if let Some(parent) = parent {
        record.fields.insert("parent", Json::str(parent));
    }
    record.fields.insert("sectorSize", Json::num(size.to_string()));
    record.fields.insert("radius", Json::num(format!("{radius}")));
    if let Some(mesh) = mesh {
        record.fields.insert("mesh", mesh);
    }
    project.contents.push(record.clone());
    record
}

fn sector(project: &mut ModProject, name: &str, planet: &str, index: i32) -> ContentRecord {
    let mut record = ContentRecord::create(&format!("id-{name}"), ContentType::Sector, name);
    record.fields.insert("planet", Json::str(planet));
    record.fields.insert("sector", Json::num(index.to_string()));
    project.contents.push(record.clone());
    record
}

fn mesh_config(pairs: &[(&str, Json)]) -> Json {
    let mut object = Object::new();
    for (key, value) in pairs {
        object.insert(*key, value.clone());
    }
    Json::Obj(object)
}

const UNIT_GRID_TILES: i32 = 92; // 10*3^2 + 2

#[test]
fn mod_planet_scene_has_grid_appearance_and_system() {
    let mut project = new_project();
    let mesh = mesh_config(&[("type", Json::str("NoiseMesh")), ("mag", Json::num("0.08"))]);
    let planet = planet(&mut project, "endpoint", Some("sun"), 2, 1.2, Some(mesh));
    let sector = sector(&mut project, "sector-a", "endpoint", 5);

    let scene = scene(&project, planet.id.as_deref().expect("planet id")).expect("场景");
    assert_eq!(SceneKind::Planet, scene.kind);
    assert_eq!(2, scene.sector_size);
    assert!((scene.radius - 1.2).abs() < 1e-6);

    let grid = scene.grid.as_ref().expect("有网格");
    assert_eq!(2, grid.size);
    assert_eq!(93, grid.capacity);
    assert_eq!(UNIT_GRID_TILES as usize, grid.tiles.len());
    for tile in &grid.tiles {
        let length = (tile.center[0] * tile.center[0]
            + tile.center[1] * tile.center[1]
            + tile.center[2] * tile.center[2])
            .sqrt();
        assert!((length - 1.0).abs() < 1e-4, "tile {} not on the unit sphere", tile.index);
        assert_eq!(tile.corners.len() % 3, 0);
    }
    let occupied = grid.tiles.iter().find(|tile| tile.index == 5).expect("下标 5");
    assert!(occupied.occupied, "下标 5 被 sector-a 占用");
    assert_eq!(sector.id.as_ref(), occupied.sector_ids.first());

    let appearance = scene.appearance.as_ref().expect("有外观");
    assert_eq!("NoiseMesh", appearance.mesh_type);
    assert_eq!(1, appearance.surface.len());
    // NoiseMesh 的 radius 是绝对半径（不乘星球半径），divisions<=1 回落到 sectorSize=2。
    assert!((appearance.surface[0].radius - 1.0).abs() < 1e-6);
    assert_eq!(UNIT_GRID_TILES as usize, appearance.surface[0].tiles.len());
    assert!((appearance.max_extent - 1.0).abs() < 0.5);
    // sun + 三个 vanilla 兄弟 + endpoint
    assert_eq!(5, scene.system.bodies.len());
    assert!(scene.system.bodies[0].star);
    assert_eq!(4, scene.system.sun_colors.len());
}

#[test]
fn sector_scene_resolves_to_its_planet_but_keeps_sector_kind() {
    let mut project = new_project();
    let mesh = mesh_config(&[("type", Json::str("NoiseMesh"))]);
    planet(&mut project, "endpoint", Some("sun"), 2, 1.5, Some(mesh));
    let sector = sector(&mut project, "sector-a", "test-mod-endpoint", 9);

    let scene = scene(&project, sector.id.as_deref().expect("sector id")).expect("场景");
    assert_eq!(SceneKind::Sector, scene.kind);
    assert_eq!(sector.id.as_deref().map(str::to_string), Some(scene.record_id.clone()));
    assert!((scene.radius - 1.5).abs() < 1e-6, "半径来自解析到的星球");
    let grid = scene.grid.as_ref().expect("有网格");
    let tile = grid.tiles.iter().find(|tile| tile.index == 9).expect("下标 9");
    assert_eq!(sector.id.as_ref(), tile.sector_ids.first());
}

#[test]
fn vanilla_planet_uses_the_serpulo_terrain_and_size_table() {
    let mut project = new_project();
    let sector = sector(&mut project, "sector-a", "serpulo", 1);

    let scene = scene(&project, sector.id.as_deref().expect("sector id")).expect("场景");
    assert_eq!(3, scene.sector_size, "serpulo 尺寸来自 vanilla 表");
    assert!((scene.radius - 1.0).abs() < 1e-6);
    let appearance = scene.appearance.as_ref().expect("有外观");
    assert_eq!("SerpuloTerrain", appearance.mesh_type);
    assert_eq!(1, appearance.surface.len());
    // vanilla 固定 divisions = min(6, max(size, 6)) = 6 → 10*3^6 + 2 个 tile
    assert_eq!(7292, appearance.surface[0].tiles.len());
    assert!(appearance.surface[0].tiles.iter().any(|tile| tile.color.is_some()));
    let serpulo = scene
        .system
        .bodies
        .iter()
        .find(|body| body.name == "serpulo")
        .expect("serpulo 在系统里");
    assert!(serpulo.selected);
}

#[test]
fn atmosphere_defaults_and_overrides_follow_java() {
    let mut project = new_project();
    let plain = planet(&mut project, "plain", Some("sun"), 2, 1.0, None);
    let default = scene(&project, plain.id.as_deref().expect("id")).expect("场景");
    let default = default.appearance.expect("有外观").atmosphere;
    assert_eq!("#4db3ff", default.color);
    assert!((default.rad_out - 0.3).abs() < 1e-9);

    let mut second = new_project();
    let mut custom = planet(&mut second, "custom", Some("sun"), 2, 1.0, None);
    custom.fields.insert("atmosphereColor", Json::str("ff0000"));
    custom.fields.insert("atmosphereRadOut", Json::num("0.01"));
    let last = second.contents.len() - 1;
    second.contents[last] = custom;
    let atmosphere = scene(&second, "id-custom").expect("场景").appearance.expect("有外观").atmosphere;
    assert_eq!("#ff0000", atmosphere.color);
    assert!((atmosphere.rad_out - 0.05).abs() < 1e-9, "非法小值被抬到 0.05");

    let mut third = new_project();
    let mut broken = planet(&mut third, "broken", Some("sun"), 2, 1.0, None);
    broken.fields.insert("atmosphereColor", Json::str("not-a-color"));
    let last = third.contents.len() - 1;
    third.contents[last] = broken;
    let atmosphere = scene(&third, "id-broken").expect("场景").appearance.expect("有外观").atmosphere;
    assert_eq!("#4db3ff", atmosphere.color, "非法颜色回落默认");
}

#[test]
fn light_points_from_the_selected_body_to_the_sun() {
    let mut project = new_project();
    let mesh = mesh_config(&[("type", Json::str("NoiseMesh"))]);
    let planet = planet(&mut project, "endpoint", Some("sun"), 2, 1.0, Some(mesh));

    let scene = scene(&project, planet.id.as_deref().expect("id")).expect("场景");
    // 恒星先占一个索引；三个 vanilla 兄弟占 1..3；本星球在第 4 个非恒星位置。
    let angle = 0.6 + 4.0 * std::f64::consts::PI * 2.0 / 5.0;
    let expected = [-angle.cos(), -angle.sin(), 0.0];
    let light = scene.appearance.as_ref().expect("有外观").light;
    for index in 0..3 {
        assert!((light[index] - expected[index]).abs() < 1e-9, "{light:?} vs {expected:?}");
    }
}

#[test]
fn a_root_planet_without_selection_falls_back_to_the_fixed_light() {
    let mut project = new_project();
    let mesh = mesh_config(&[("type", Json::str("NoiseMesh"))]);
    let root = planet(&mut project, "lonely", None, 2, 1.0, Some(mesh));

    let scene = scene(&project, root.id.as_deref().expect("id")).expect("场景");
    let light = scene.appearance.as_ref().expect("有外观").light;
    let length = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2]).sqrt();
    assert!((length - 1.0).abs() < 1e-9, "光照必须是单位向量");
    // 独立根星球没有非恒星选中天体 → normalize(0.75, 0.35, 0.55)
    let fallback = [0.75_f64, 0.35, 0.55];
    let norm = (fallback[0] * fallback[0] + fallback[1] * fallback[1] + fallback[2] * fallback[2]).sqrt();
    assert!((light[0] - fallback[0] / norm).abs() < 1e-9);
}

#[test]
fn gridless_planets_have_no_grid_or_appearance() {
    let mut project = new_project();
    let planet = planet(&mut project, "flat", Some("sun"), 0, 1.0, None);
    let scene = scene(&project, planet.id.as_deref().expect("id")).expect("场景");
    assert_eq!(0, scene.sector_size);
    assert!(scene.grid.is_none());
    assert!(scene.appearance.is_none(), "无网格 → 外观为 null，前端显示无网格");
}

#[test]
fn non_cosmos_records_are_rejected_with_an_explicit_error() {
    let mut project = new_project();
    project.contents.push(ContentRecord::create("i1", ContentType::Item, "kiln"));
    match scene(&project, "i1") {
        Err(SceneError::NotCosmos { content_type, .. }) => assert_eq!("item", content_type),
        other => panic!("期望 NotCosmos，实际：{other:?}"),
    }
    match scene(&project, "missing") {
        Err(SceneError::RecordNotFound(id)) => assert_eq!("missing", id),
        other => panic!("期望 RecordNotFound，实际：{other:?}"),
    }
}

#[test]
fn mesh_layers_clamp_divisions_and_scale_relative_radii() {
    let mut project = new_project();
    // Sky 层半径相对星球（radius 是 buildHex 的 intensity）；这里用 SunMesh 触发 relative 分支。
    let mesh = mesh_config(&[
        ("type", Json::str("SunMesh")),
        ("divisions", Json::num("99")),
        ("colors", Json::Arr(vec![Json::str("ff7a38")])),
    ]);
    let planet = planet(&mut project, "star", Some("sun"), 2, 2.0, Some(mesh));

    let scene = scene(&project, planet.id.as_deref().expect("id")).expect("场景");
    let layer = &scene.appearance.as_ref().expect("有外观").surface[0];
    assert!(layer.unlit, "SunMesh 不受光");
    assert!((layer.radius - 2.0).abs() < 1e-6, "relative 层的半径乘星球半径");
    // divisions 99 → clamp 6 → 10*3^6 + 2
    assert_eq!(7292, layer.tiles.len());
}
