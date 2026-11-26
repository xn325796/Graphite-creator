//! 星球战役场景组装（Java `ui/PlanetPreview.setSource/rebuildAppearance` 的数据半边）。
//!
//! 渲染（Canvas 投影、着色、大气逐像素）在 Vue 侧；这里只算**场景数据**：区块网格、表面/云层
//! 网格模型、大气参数、光照方向与星系天体。零 tauri 依赖，可离线测。
//!
//! ## 与 Java 的对应
//!
//! - `setSource` → [`scene`]：sector 解析到所属星球（含 vanilla 尺寸表），非 planet/sector 报
//!   [`SceneError::NotCosmos`]；
//! - `rebuildAppearance` → [`appearance_of`]：surface/cloud 层的 divisions 钳制、半径语义
//!   （`radius_is_planet_relative`）、无 mesh 的 vanilla 走 `serpulo_mesher(0)`；
//! - `readAtmosphere` → [`read_atmosphere`]；
//! - `lightFromSystem` → [`light_from_system`]；
//! - `drawAppearance` 里的 `maxExtent` → [`max_extent_of`]。
//!
//! 网格/轨道的**规则**不在这里重写：`mm_fmt::planet::{grids, meshes, orbits}` 是唯一实现。
//!
//! ## 与 Java 展示层的刻意差异
//!
//! Java 的 `meshTypeInfo` 在无 mesh 时是空串、由 UI 文案回落成 `"SerpuloTerrain"`；本模块直接
//! 下发 `"SerpuloTerrain"`，让前端不必再复制那条回落。字段名与形状不变。

use std::fmt;

use mm_fmt::planet::{grids, meshes, orbits};
use mm_json::Json;
use mm_model::{ContentRecord, ModProject};

/// Java `MAX_DIVISIONS`：`10*3^6` 个 tile 的 2D Canvas 重绘已经吃紧，再高就卡。
const MAX_DIVISIONS: i32 = 6;

/// 记录本身不是星球/扇区时 `cosmos_scene` 的明确失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    /// 工程里没有这条记录（刚删掉、id 过期）。
    RecordNotFound(String),
    /// 记录存在，但不是 `planet`/`sector`。
    NotCosmos { record_id: String, content_type: String },
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::RecordNotFound(id) => write!(f, "找不到记录：{id}"),
            SceneError::NotCosmos { record_id, content_type } => write!(
                f,
                "记录 {record_id} 是 {content_type}，星球战役只接受 planet 或 sector"
            ),
        }
    }
}

impl std::error::Error for SceneError {}

/// 选中的是星球还是扇区（扇区会解析到它所属的星球，但仍回 `kind=sector`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneKind {
    Planet,
    Sector,
}

impl SceneKind {
    pub fn key(self) -> &'static str {
        match self {
            SceneKind::Planet => "planet",
            SceneKind::Sector => "sector",
        }
    }
}

/// 一次场景快照。字段与 IPC 契约一一对应。
#[derive(Debug, Clone)]
pub struct CosmosScene {
    pub record_id: String,
    pub kind: SceneKind,
    /// `planet` 记录上的 `sectorSize`（vanilla 走尺寸表）；0 = 无网格。
    pub sector_size: i32,
    /// 星球半径（`radius` 字段，缺省 1）。vanilla 恒 1。
    pub radius: f32,
    pub grid: Option<CosmosGrid>,
    /// 无网格（`sectorSize <= 0`）时是 `None`。
    pub appearance: Option<CosmosAppearance>,
    pub system: CosmosSystem,
}

#[derive(Debug, Clone)]
pub struct CosmosGrid {
    pub size: i32,
    pub capacity: i32,
    pub tiles: Vec<CosmosGridTile>,
}

#[derive(Debug, Clone)]
pub struct CosmosGridTile {
    pub index: usize,
    /// tile 中心（单位球面）。
    pub center: [f64; 3],
    /// 角点扁平 `x,y,z` 序列（单位球面；网格页不做高度位移）。
    pub corners: Vec<f64>,
    pub occupied: bool,
    /// 落在这一格里的扇区记录 id（保工程顺序）。
    pub sector_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CosmosAppearance {
    /// 规范化后的网格类型；无 mesh 的 vanilla 报 `"SerpuloTerrain"`。
    pub mesh_type: String,
    pub surface: Vec<CosmosLayer>,
    pub clouds: Vec<CosmosLayer>,
    /// Java `lightFromSystem()` 的单位向量。
    pub light: [f64; 3],
    pub atmosphere: CosmosAtmosphere,
    /// surface 角点最大模长（至少 1）；前端 `scale = min(w,h)*0.42/maxExtent`。
    pub max_extent: f64,
}

#[derive(Debug, Clone)]
pub struct CosmosLayer {
    /// 这一层实际用于 `buildHex` 的半径（已按 `radius_is_planet_relative` 乘过星球半径）。
    pub radius: f32,
    pub intensity: f32,
    pub unlit: bool,
    pub sky: bool,
    pub cloud_speed: f32,
    pub tiles: Vec<CosmosTile>,
}

#[derive(Debug, Clone)]
pub struct CosmosTile {
    /// 扁平 `x,y,z`；被跳过的云层 tile 是空表。
    pub corners: Vec<f64>,
    /// 0..1 RGBA；`None` = 该 tile 不可画（被跳过，或颜色携带 alpha=0 的语义由前端判断）。
    pub color: Option<[f32; 4]>,
    pub skipped: bool,
}

#[derive(Debug, Clone)]
pub struct CosmosAtmosphere {
    /// `#rrggbb`。
    pub color: String,
    /// Java `atmosphereRadOut`，最小 0.05。
    pub rad_out: f64,
}

#[derive(Debug, Clone)]
pub struct CosmosSystem {
    pub bodies: Vec<CosmosBody>,
    pub sun_colors: [String; 4],
}

#[derive(Debug, Clone)]
pub struct CosmosBody {
    pub name: String,
    pub radius: f32,
    pub orbit_radius: f32,
    pub color: Option<String>,
    pub selected: bool,
    pub vanilla: bool,
    pub star: bool,
}

/// 由选中记录组装场景。`record_id` 不存在或类型不符时返回 [`SceneError`]。
pub fn scene(project: &ModProject, record_id: &str) -> Result<CosmosScene, SceneError> {
    let record = project
        .content_by_id(record_id)
        .ok_or_else(|| SceneError::RecordNotFound(record_id.to_string()))?;
    let internal = project.internal_name();

    let (kind, planet, bare) = match record.ctype {
        mm_fmt::ContentType::Planet => (SceneKind::Planet, Some(record), record.name.clone()),
        mm_fmt::ContentType::Sector => {
            let bare = bare_planet_name(&grids::sector_planet(record), &internal);
            let planet = planet_by_name(project, &bare);
            (SceneKind::Sector, planet, bare)
        }
        other => {
            return Err(SceneError::NotCosmos {
                record_id: record_id.to_string(),
                content_type: other.name().to_string(),
            })
        }
    };

    let sector_size = match planet {
        Some(planet) => grids::int_field(planet, "sectorSize", 0),
        None => grids::vanilla_size(&bare.to_lowercase()).unwrap_or(0),
    };
    let radius = planet.map(radius_of).unwrap_or(1.0_f32);

    let grid = if sector_size > 0 {
        Some(grid_of(project, &bare, sector_size))
    } else {
        None
    };

    let bodies = orbits::system_of(project, planet, if planet.is_none() { Some(&bare) } else { None });
    let light = light_from_system(&bodies);

    let atmosphere = read_atmosphere(planet);
    let appearance = if sector_size > 0 {
        appearance_of(planet, radius, sector_size, light, atmosphere)
    } else {
        None
    };

    Ok(CosmosScene {
        record_id: record_id.to_string(),
        kind,
        sector_size,
        radius,
        grid,
        appearance,
        system: CosmosSystem {
            bodies: bodies.iter().map(cosmos_body).collect(),
            sun_colors: orbits::SUN_COLORS.map(str::to_string),
        },
    })
}

/// Java `drawGrid` 的数据半边：单位球面的 tile 与占用表。
fn grid_of(project: &ModProject, bare: &str, size: i32) -> CosmosGrid {
    let grid = grids::create(size);
    let occupied = grids::sectors_by_index(project, bare, size);
    let tiles: Vec<CosmosGridTile> = grid
        .tiles
        .iter()
        .enumerate()
        .map(|(index, tile)| {
            let tile = tile.borrow();
            let mut corners = Vec::with_capacity(tile.corners.len() * 3);
            for slot in &tile.corners {
                let corner = slot.as_ref().expect("assigned grid tile corner").borrow();
                corners.push(f64::from(corner.v.x));
                corners.push(f64::from(corner.v.y));
                corners.push(f64::from(corner.v.z));
            }
            let ids: Vec<String> = occupied
                .iter()
                .find(|(key, _)| *key == index as i32)
                .map(|(_, records)| {
                    records
                        .iter()
                        .filter_map(|record| record.id.clone())
                        .collect()
                })
                .unwrap_or_default();
            CosmosGridTile {
                index,
                center: [f64::from(tile.v.x), f64::from(tile.v.y), f64::from(tile.v.z)],
                corners,
                occupied: !ids.is_empty(),
                sector_ids: ids,
            }
        })
        .collect();
    CosmosGrid {
        size,
        capacity: grids::capacity(size),
        tiles,
    }
}

/// Java `rebuildAppearance` 的数据半边。调用前 `size > 0` 已确认。
fn appearance_of(
    planet: Option<&ContentRecord>,
    radius: f32,
    size: i32,
    light: [f64; 3],
    atmosphere: CosmosAtmosphere,
) -> Option<CosmosAppearance> {
    let mesh_config = planet
        .and_then(|record| meshes::normalize_mesh_config(record.fields.get("mesh")));
    let cloud_config = planet
        .and_then(|record| meshes::normalize_mesh_config(record.fields.get("cloudMesh")));

    let mut surface: Vec<CosmosLayer> = Vec::new();
    let mesh_type = match &mesh_config {
        Some(config) => meshes::string_or(Some(config), "type", "NoiseMesh"),
        None => "SerpuloTerrain".to_string(),
    };

    match &mesh_config {
        None => {
            // Vanilla：游戏用 HexMesh(generator, divisions 6)；我们以 serpulo 地形近似。
            let divisions = MAX_DIVISIONS.min(size.max(6));
            let model = meshes::build_model(divisions, radius, meshes::serpulo_mesher(0).as_ref(), 0.2);
            surface.push(layer_of(&model, radius, 0.2, false, false, 0.0));
        }
        Some(config) => {
            for layer in meshes::surface_layers_from_config(Some(config)) {
                let divisions = divisions_for(size, layer.divisions);
                let shell_radius = if layer.radius_is_planet_relative {
                    layer.radius * radius
                } else {
                    layer.radius
                };
                let model = meshes::build_model(divisions, shell_radius, layer.mesher.as_ref(), layer.intensity);
                surface.push(layer_of(
                    &model,
                    shell_radius,
                    layer.intensity,
                    layer.unlit,
                    layer.sky,
                    layer.cloud_speed,
                ));
            }
        }
    }

    let mut clouds: Vec<CosmosLayer> = Vec::new();
    if let Some(config) = &cloud_config {
        for layer in meshes::cloud_layers_from_config(Some(config)) {
            let divisions = MAX_DIVISIONS.min(layer.divisions.max(1));
            let shell_radius = layer.radius * radius;
            let model = meshes::build_model(divisions, shell_radius, layer.mesher.as_ref(), layer.intensity);
            clouds.push(layer_of(
                &model,
                shell_radius,
                layer.intensity,
                layer.unlit,
                layer.sky,
                layer.cloud_speed,
            ));
        }
    }

    if surface.is_empty() && clouds.is_empty() {
        return None;
    }
    let max_extent = max_extent_of(&surface);
    Some(CosmosAppearance {
        mesh_type,
        surface,
        clouds,
        light,
        atmosphere,
        max_extent,
    })
}

fn divisions_for(size: i32, layer_divisions: i32) -> i32 {
    MAX_DIVISIONS.min((if layer_divisions <= 1 { size } else { layer_divisions }).max(1))
}

fn layer_of(
    model: &meshes::MeshModel,
    radius: f32,
    intensity: f32,
    unlit: bool,
    sky: bool,
    cloud_speed: f32,
) -> CosmosLayer {
    let tiles: Vec<CosmosTile> = (0..model.tile_corners.len())
        .map(|index| {
            let skipped = model.tile_skipped.get(index).copied().unwrap_or(false);
            let color = model.tile_colors.get(index).and_then(|value| value.map(rgb_array));
            CosmosTile {
                corners: model.tile_corners[index].clone(),
                color,
                skipped,
            }
        })
        .collect();
    CosmosLayer {
        radius,
        intensity,
        unlit,
        sky,
        cloud_speed,
        tiles,
    }
}

fn rgb_array(color: meshes::Rgb) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

/// Java `readAtmosphere`：`atmosphereColor` / `atmosphereRadOut`，非法值回落默认。
fn read_atmosphere(planet: Option<&ContentRecord>) -> CosmosAtmosphere {
    let mut color = "#4db3ff".to_string();
    let mut rad_out = 0.3_f64;
    if let Some(planet) = planet {
        if let Some(value) = planet.fields.get("atmosphereColor")
            && is_primitive(value)
            && let Some(parsed) = rgb_hex(&value.get_as_string())
        {
            color = parsed;
        }
        if let Some(value) = planet.fields.get("atmosphereRadOut")
            && is_primitive(value)
            && let Some(raw) = as_float(value)
        {
            rad_out = f64::from(raw).max(0.05);
        }
    }
    CosmosAtmosphere { color, rad_out }
}

/// Java `PlanetMeshes.Rgb.of(text)` → `#rrggbb`（alpha 不参与大气色）。
fn rgb_hex(text: &str) -> Option<String> {
    let stripped = text.strip_prefix('#').unwrap_or(text);
    if stripped.is_empty() || stripped.len() > 16 {
        return None;
    }
    if !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let padded = if stripped.len() == 6 { format!("{stripped}ff") } else { stripped.to_string() };
    let value = u64::from_str_radix(&padded, 16).ok()?;
    let r = ((value >> 24) & 0xFF) as u8;
    let g = ((value >> 16) & 0xFF) as u8;
    let b = ((value >> 8) & 0xFF) as u8;
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// Java `drawAppearance` 的 `maxExtent`：所有 surface 层角点模长的最大值，至少 1。
fn max_extent_of(surface: &[CosmosLayer]) -> f64 {
    let mut max = 1.0_f64;
    for layer in surface {
        for corners in layer.tiles.iter().map(|tile| &tile.corners) {
            for point in corners.as_chunks::<3>().0 {
                let length = (point[0] * point[0] + point[1] * point[1] + point[2] * point[2]).sqrt();
                if length > max {
                    max = length;
                }
            }
        }
    }
    max
}

/// Java `lightFromSystem()`：选中天体在示意轨道上的角度指向太阳；无选中走固定方向。
///
/// 索引的递增与 `drawOrbit` 逐字一致——恒星只占一个索引但不画角度。
pub fn light_from_system(bodies: &[orbits::Body]) -> [f64; 3] {
    let mut index = 0_usize;
    for body in bodies {
        if body.star {
            index += 1;
            continue;
        }
        let angle = 0.6 + index as f64 * std::f64::consts::PI * 2.0 / 5.0;
        index += 1;
        if body.selected {
            return normalize(-angle.cos(), -angle.sin(), 0.0);
        }
    }
    normalize(0.75, 0.35, 0.55)
}

fn normalize(x: f64, y: f64, z: f64) -> [f64; 3] {
    let length = (x * x + y * y + z * z).sqrt();
    [x / length, y / length, z / length]
}

fn cosmos_body(body: &orbits::Body) -> CosmosBody {
    CosmosBody {
        name: body.name.clone(),
        radius: body.radius,
        orbit_radius: body.orbit_radius,
        color: body.color.clone(),
        selected: body.selected,
        vanilla: body.vanilla,
        star: body.star,
    }
}

/// Java `barePlanetName`：`sectorPlanet` 的结果可能带本工程前缀。
fn bare_planet_name(target: &str, internal: &str) -> String {
    let prefix = format!("{internal}-");
    if target.starts_with(&prefix) {
        target[prefix.len()..].to_string()
    } else {
        target.to_string()
    }
}

/// Java `planetByName`：裸名或带前缀名都认，只认文件型 planet 记录。
fn planet_by_name<'a>(project: &'a ModProject, bare: &str) -> Option<&'a ContentRecord> {
    let internal = project.internal_name();
    project.contents.iter().find(|record| {
        record.ctype == mm_fmt::ContentType::Planet
            && !record.is_inline()
            && (record.name == bare || record.full_name(&internal) == bare)
    })
}

/// Java `radiusOf`：`radius` 字段，缺省 1。
fn radius_of(planet: &ContentRecord) -> f32 {
    match planet.fields.get("radius") {
        Some(value) if is_primitive(value) => as_float(value).unwrap_or(1.0_f32),
        _ => 1.0_f32,
    }
}

/// Gson `isJsonPrimitive`。
fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Bool(_) | Json::Num(_) | Json::Str(_))
}

/// Gson `getAsFloat` 的可空版本（布尔按读不到处理，与 `orbits` 同口径）。
fn as_float(value: &Json) -> Option<f32> {
    match value {
        Json::Num(text) => text.parse::<f64>().ok().map(|v| v as f32),
        Json::Str(text) => text.parse::<f32>().ok(),
        _ => None,
    }
}
