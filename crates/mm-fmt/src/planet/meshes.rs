//! `PlanetMeshes.java` 的等价移植：星球表面/云层网格的构建，镜像 `MeshBuilder.buildHex` 与游戏里
//! 的各个 mesh 类——每个星球网格都是「一组网格 tile，其角点按 `(1 + height * intensity) * radius`
//! 缩放」，逐 tile 由 mesher 上色，可选跳过（云）。Simplex 噪声调用与游戏完全一致（`NoiseMesh`
//! 使用 5 + position 的偏移、7 + seed 与 8 + seed 两个种子）。
//!
//! Java 侧 `core\fmt\PlanetMeshes.java`；对应的包内引用 `PlanetGrids` 在 Rust 里是本包的 [`grids`]，
//! `MeshTypes` 是 [`mesh_types`]。
//!
//! Java 里各个 mesher 是**捕获局部变量的匿名类**；Rust 没有等价的闭包捕获 + 接口对象，故为每个匿名
//! 类建一个 `struct`（捕获的变量成为字段），并用 `Box<dyn Mesher>` 复刻「一个 `Mesher` 引用」的用法：
//! - `noiseMesh` 的匿名类 → [`NoiseMesher`]
//! - `sunMesh` 的匿名类 → [`SunMesher`]
//! - `hexSkyMesh` 的匿名类 → [`HexSkyMesher`]
//! - `serpuloMesher` 的匿名类（含其 private 方法）→ [`SerpuloMesher`]
//!
//! Java 的引用语义在 Rust 里是值语义：`normalizeMeshConfig` / `meshLayerConfigs` 返回的是 JSON 子树
//! 的副本（Java 返回同一 `JsonObject` 引用），调用方修改副本不会影响原树。

use std::rc::Rc;

use mm_json::{Json, Object};

use super::grids::{self, Vec3};
use super::mesh_types;
use super::noise::noise3d;

/// Java `PlanetMeshes.Rgb`：网格数学用的简单 RGBA 颜色（Java record）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgb {
    /// Java `Rgb.of(String)`：`#` 前缀可选，6 位补 `ff`；非法文本在 Java 里抛 `NumberFormatException`。
    pub fn of(hex: &str) -> Rgb {
        Rgb::try_of(hex).unwrap_or_else(|| panic!("NumberFormatException: {hex}"))
    }

    /// Java `Rgb.of(String)` 的不抛异常版本，供 `colorOr` 复刻它的 `catch (NumberFormatException)`。
    ///
    /// Java 用 `long`（有符号）承接 `parseUnsignedLong` 的结果再做 `>> 24`；只有超过 8 位十六进制的
    /// 文本才会让符号位影响最高字节的取值，那种文本在本工具的调色板里不存在，这里直接用 `u64`。
    pub fn try_of(hex: &str) -> Option<Rgb> {
        let text = if let Some(stripped) = hex.strip_prefix('#') {
            stripped
        } else {
            hex
        };
        let padded = if text.len() == 6 {
            format!("{text}ff")
        } else {
            text.to_string()
        };
        let value = u64::from_str_radix(&padded, 16).ok()?;
        Some(Rgb {
            r: ((value >> 24) & 0xFF) as f32 / 255.0_f32,
            g: ((value >> 16) & 0xFF) as f32 / 255.0_f32,
            b: ((value >> 8) & 0xFF) as f32 / 255.0_f32,
            a: (value & 0xFF) as f32 / 255.0_f32,
        })
    }
}

/// Java `PlanetMeshes.Mesher`：逐 tile 的网格回调，镜像游戏里的 `HexMesher` 接口。
///
/// Java 的 `default boolean skip` 对应这里的 trait 方法默认实现。
pub trait Mesher {
    fn height(&self, position: &Vec3) -> f32;

    fn color(&self, position: &Vec3) -> Rgb;

    fn skip(&self, position: &Vec3) -> bool {
        let _ = position;
        false
    }
}

/// Java `PlanetMeshes.MeshModel`：一层网格渲染出的几何——tile 多边形 + 面颜色。
pub struct MeshModel {
    pub sector_size: i32,
    pub radius: f32,
    /// 每个 tile：角点展平成 `[x,y,z, x,y,z, ...]`（已按 height * radius 缩放）。
    pub tile_corners: Vec<Vec<f64>>,
    /// Java 的 `List<Rgb>` 会被塞进 `null`（被跳过的 tile），故这里是 `Option<Rgb>`。
    pub tile_colors: Vec<Option<Rgb>>,
    pub tile_skipped: Vec<bool>,
    pub tile_indexes: Vec<i32>,
}

impl MeshModel {
    /// Java `MeshModel(int sectorSize, float radius)`（Java 里是包内可见）。
    pub(crate) fn new(sector_size: i32, radius: f32) -> MeshModel {
        MeshModel {
            sector_size,
            radius,
            tile_corners: Vec::new(),
            tile_colors: Vec::new(),
            tile_skipped: Vec::new(),
            tile_indexes: Vec::new(),
        }
    }
}

// --- mesher builders (defaults mirror ContentParser.parseMesh) ------------------------------

/// Java `noiseMesh(...)` 里 `new Mesher() { ... }` 的匿名类：捕获 seed/octaves/persistence/scale/
/// mag 与两个颜色及其噪声参数。
pub struct NoiseMesher {
    pub seed: i32,
    pub octaves: i32,
    pub persistence: f32,
    pub scale: f32,
    pub mag: f32,
    pub color1: Rgb,
    pub color2: Rgb,
    pub color_oct: i32,
    pub color_persistence: f32,
    pub color_scale: f32,
    pub color_threshold: f32,
}

impl Mesher for NoiseMesher {
    fn height(&self, position: &Vec3) -> f32 {
        // Java：`5f + position.x` 是 float 加法，再作为 double 实参传给 noise3d。
        noise3d(
            self.seed.wrapping_add(7),
            f64::from(self.octaves),
            f64::from(self.persistence),
            f64::from(self.scale),
            f64::from(5.0_f32 + position.x),
            f64::from(5.0_f32 + position.y),
            f64::from(5.0_f32 + position.z),
        ) * self.mag
    }

    fn color(&self, position: &Vec3) -> Rgb {
        let mask: f64 = noise3d(
            self.seed.wrapping_add(8),
            f64::from(self.color_oct),
            f64::from(self.color_persistence),
            f64::from(self.color_scale),
            f64::from(5.0_f32 + position.x),
            f64::from(5.0_f32 + position.y),
            f64::from(5.0_f32 + position.z),
        )
        .into();
        if mask > f64::from(self.color_threshold) {
            self.color2
        } else {
            self.color1
        }
    }
}

/// Java `noiseMesh(...)`。
#[allow(clippy::too_many_arguments)]
pub fn noise_mesh(
    seed: i32,
    octaves: i32,
    persistence: f32,
    scale: f32,
    mag: f32,
    color1: Rgb,
    color2: Rgb,
    color_oct: i32,
    color_persistence: f32,
    color_scale: f32,
    color_threshold: f32,
) -> Box<dyn Mesher> {
    Box::new(NoiseMesher {
        seed,
        octaves,
        persistence,
        scale,
        mag,
        color1,
        color2,
        color_oct,
        color_persistence,
        color_scale,
        color_threshold,
    })
}

/// Java `singleColorMesh(...)`：两个颜色相同，故颜色掩码永远落在同一侧。
pub fn single_color_mesh(
    seed: i32,
    octaves: i32,
    persistence: f32,
    scale: f32,
    mag: f32,
    color: Rgb,
) -> Box<dyn Mesher> {
    noise_mesh(
        seed,
        octaves,
        persistence,
        scale,
        mag,
        color,
        color,
        1,
        0.5_f32,
        1.0_f32,
        0.5_f32,
    )
}

/// Java `sunMesh(...)` 里匿名类捕获的字段（注意 Java 的形参是 `double`，只有 `colorScale` 是
/// `float`）。
pub struct SunMesher {
    pub octaves: f64,
    pub persistence: f64,
    pub scl: f64,
    pub pow: f64,
    pub mag: f64,
    pub color_scale: f32,
    pub colors: Vec<Rgb>,
}

impl Mesher for SunMesher {
    fn height(&self, _position: &Vec3) -> f32 {
        0.0
    }

    fn color(&self, position: &Vec3) -> Rgb {
        // Java：`Math.pow((double) noise3d(...), pow) * mag` —— float 结果先提升为 double 再取幂。
        let height: f64 = f64::from(noise3d(
            0,
            self.octaves,
            self.persistence,
            self.scl,
            f64::from(position.x),
            f64::from(position.y),
            f64::from(position.z),
        ))
        .powf(self.pow)
            * self.mag;
        // Java：`(int) (height * colors.size())` 向零截断，再夹进 [0, size-1]。
        let truncated = height * self.colors.len() as f64;
        let index = 0_i32.max(
            (self.colors.len() as i32 - 1).min(truncated as i32),
        ) as usize;
        let color = self.colors[index];
        Rgb {
            r: color.r * self.color_scale,
            g: color.g * self.color_scale,
            b: color.b * self.color_scale,
            a: color.a,
        }
    }
}

/// Java `sunMesh(double octaves, double persistence, double scl, double pow, double mag,
/// float colorScale, List<Rgb> colors)`。
pub fn sun_mesh(
    octaves: f64,
    persistence: f64,
    scl: f64,
    pow: f64,
    mag: f64,
    color_scale: f32,
    colors: Vec<Rgb>,
) -> Box<dyn Mesher> {
    Box::new(SunMesher {
        octaves,
        persistence,
        scl,
        pow,
        mag,
        color_scale,
        colors,
    })
}

/// Java `hexSkyMesh(...)` 里匿名类捕获的字段。
pub struct HexSkyMesher {
    pub seed: i32,
    pub octaves: i32,
    pub persistence: f32,
    pub scl: f32,
    pub thresh: f32,
    pub color: Rgb,
}

impl Mesher for HexSkyMesher {
    fn height(&self, _position: &Vec3) -> f32 {
        1.0
    }

    fn color(&self, _position: &Vec3) -> Rgb {
        self.color
    }

    fn skip(&self, position: &Vec3) -> bool {
        // Java：`position.y * 3f` 是 float 乘法，再作为 double 实参。
        noise3d(
            self.seed.wrapping_add(7),
            f64::from(self.octaves),
            f64::from(self.persistence),
            f64::from(self.scl),
            f64::from(position.x),
            f64::from(position.y * 3.0_f32),
            f64::from(position.z),
        ) >= self.thresh
    }
}

/// Java `hexSkyMesh(int seed, int octaves, float persistence, float scl, float thresh, Rgb color)`。
pub fn hex_sky_mesh(
    seed: i32,
    octaves: i32,
    persistence: f32,
    scl: f32,
    thresh: f32,
    color: Rgb,
) -> Box<dyn Mesher> {
    Box::new(HexSkyMesher {
        seed,
        octaves,
        persistence,
        scl,
        thresh,
        color,
    })
}

// --- config parsing (defaults mirror ContentParser.parseMesh) -------------------------------

/// Java `colorOr(JsonElement, String)`。
pub fn color_or(element: Option<&Json>, fallback: &str) -> Rgb {
    let text = match element {
        Some(value) if is_primitive(value) => value.get_as_string(),
        _ => return Rgb::of(fallback),
    };
    match Rgb::try_of(&text) {
        Some(rgb) => rgb,
        // Java: catch (NumberFormatException ignored) { return Rgb.of(fallback); }
        None => Rgb::of(fallback),
    }
}

/// Java `mesherFromConfig(JsonObject)`：从星球的 `mesh` 对象构建表面 mesher（任何可解析类型；
/// 未知类型回退）。
pub fn mesher_from_config(mesh: &Object) -> Box<dyn Mesher> {
    let type_ = mesh_type(Some(mesh));
    if type_ == "SunMesh" {
        let mut colors: Vec<Rgb> = Vec::new();
        if let Some(raw) = mesh.get("colors")
            && let Some(array) = raw.as_array()
        {
            for element in array {
                colors.push(Rgb::of(&element.get_as_string()));
            }
        }
        if colors.is_empty() {
            colors.push(Rgb::of("ff7a38"));
        }
        return sun_mesh(
            f64::from(int_or(Some(mesh), "octaves", 1)),
            num_or(Some(mesh), "persistence", f64::from(0.5_f32)),
            num_or(Some(mesh), "scl", f64::from(1.0_f32)),
            num_or(Some(mesh), "pow", f64::from(1.0_f32)),
            num_or(Some(mesh), "mag", f64::from(0.5_f32)),
            num_or(Some(mesh), "colorScale", f64::from(1.0_f32)) as f32,
            colors,
        );
    }
    if type_ == "HexSkyMesh" {
        return hex_sky_mesh(
            int_or(Some(mesh), "seed", 0),
            int_or(Some(mesh), "octaves", 1),
            num_or(Some(mesh), "persistence", f64::from(0.5_f32)) as f32,
            num_or(Some(mesh), "scale", f64::from(1.0_f32)) as f32,
            num_or(Some(mesh), "thresh", f64::from(0.5_f32)) as f32,
            color_or(mesh.get("color"), "ffffff"),
        );
    }
    if type_ == "HexMesh" {
        // 游戏里 HexMesh 由星球的 generator 渲染；没有 generator 类时，serpulo 的地形是
        // 我们能算出的最接近的视觉近似。
        return serpulo_mesher(int_or(Some(mesh), "seed", 0));
    }
    if type_ == "MatMesh" {
        let nested = mesh.get("mesh");
        if let Some(object) = nested.and_then(|value| value.as_object()) {
            return mesher_from_config(object);
        }
        return default_mesher();
    }
    if type_ == "MultiMesh" {
        let children = mesh.get("meshes");
        if let Some(array) = children.and_then(|value| value.as_array())
            && !array.is_empty()
        {
            // Java: children.getAsJsonArray().get(0).getAsJsonObject()
            return mesher_from_config(
                array[0]
                    .as_object()
                    .expect("cannot convert a non-object mesh to JsonObject"),
            );
        }
        return default_mesher();
    }
    if type_ != "NoiseMesh" {
        return default_mesher(); // 无法解析的类型一律回退
    }
    noise_mesh(
        int_or(Some(mesh), "seed", 0),
        int_or(Some(mesh), "octaves", 1),
        num_or(Some(mesh), "persistence", f64::from(0.5_f32)) as f32,
        num_or(Some(mesh), "scale", f64::from(1.0_f32)) as f32,
        num_or(Some(mesh), "mag", f64::from(0.5_f32)) as f32,
        color_or(mesh.get("color1"), &string_or(Some(mesh), "color", "ffffff")),
        color_or(mesh.get("color2"), &string_or(Some(mesh), "color", "ffffff")),
        int_or(Some(mesh), "colorOct", 1),
        num_or(Some(mesh), "colorPersistence", f64::from(0.5_f32)) as f32,
        num_or(Some(mesh), "colorScale", f64::from(1.0_f32)) as f32,
        num_or(Some(mesh), "colorThreshold", f64::from(0.5_f32)) as f32,
    )
}

/// Java `defaultMesher()`。
pub fn default_mesher() -> Box<dyn Mesher> {
    single_color_mesh(0, 1, 0.5_f32, 1.0_f32, 0.5_f32, Rgb::of("ffffff"))
}

// --- vanilla serpulo terrain (port of SerpuloPlanetGenerator, visual path) ------------------

/// Java 匿名类里的 `private static final float SCL`：vanilla serpulo 地形的高度/温度采样尺度。
const SERPULO_SCL: f32 = 5.0_f32;
/// Java `HEIGHT_Y_OFFSET = 42.7f`。
const SERPULO_HEIGHT_Y_OFFSET: f32 = 42.7_f32;
/// Java `WATER_OFFSET = 0.04f`。
const SERPULO_WATER_OFFSET: f32 = 0.04_f32;
/// Java `HEIGHT_SCL = 1.01f`。
const SERPULO_HEIGHT_SCL: f32 = 1.01_f32;
/// Java `WATER = 2f / 13`：float 除法，海平面下限。
const SERPULO_WATER: f32 = 2.0_f32 / 13.0_f32;

/// Java `WATER` 等：serpulo 地形的方块调色板——颜色取自 vanilla 精灵采样（游戏侧
/// `mapColor` 由精灵中心像素导出），alpha = 1 - block.albedo，让渲染器补上游戏 shader 由
/// albedo 推出的同一种高光。
const SERPULO_WATER_RGB: Rgb = Rgb {
    r: 0x59 as f32 / 255.0_f32,
    g: 0x6a as f32 / 255.0_f32,
    b: 0xb8 as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_DEEP_WATER: Rgb = Rgb {
    r: 0x4d as f32 / 255.0_f32,
    g: 0x5c as f32 / 255.0_f32,
    b: 0xa4 as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_TAINTED_WATER: Rgb = Rgb {
    r: 0x5e as f32 / 255.0_f32,
    g: 0x48 as f32 / 255.0_f32,
    b: 0x92 as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_DEEP_TAINTED_WATER: Rgb = Rgb {
    r: 0x43 as f32 / 255.0_f32,
    g: 0x33 as f32 / 255.0_f32,
    b: 0x6a as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_DARKSAND_WATER: Rgb = Rgb {
    r: 0x41 as f32 / 255.0_f32,
    g: 0x44 as f32 / 255.0_f32,
    b: 0x66 as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_SAND_WATER: Rgb = Rgb {
    r: 0x92 as f32 / 255.0_f32,
    g: 0x83 as f32 / 255.0_f32,
    b: 0x9c as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_DARKSAND: Rgb = Rgb {
    r: 0x3c as f32 / 255.0_f32,
    g: 0x38 as f32 / 255.0_f32,
    b: 0x38 as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_SAND: Rgb = Rgb {
    r: 0xd3 as f32 / 255.0_f32,
    g: 0xae as f32 / 255.0_f32,
    b: 0x8d as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_SALT: Rgb = Rgb {
    r: 1.0_f32,
    g: 1.0_f32,
    b: 1.0_f32,
    a: 1.0_f32,
};
const SERPULO_DARKSAND_TAINTED_WATER: Rgb = Rgb {
    r: 0x48 as f32 / 255.0_f32,
    g: 0x3d as f32 / 255.0_f32,
    b: 0x5d as f32 / 255.0_f32,
    a: 0.1_f32,
};
const SERPULO_STONE: Rgb = Rgb {
    r: 0x54 as f32 / 255.0_f32,
    g: 0x54 as f32 / 255.0_f32,
    b: 0x5b as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_SNOW: Rgb = Rgb {
    r: 0xe1 as f32 / 255.0_f32,
    g: 0xe9 as f32 / 255.0_f32,
    b: 0xf0 as f32 / 255.0_f32,
    a: 0.3_f32,
};
const SERPULO_ICE_SNOW: Rgb = Rgb {
    r: 0xe1 as f32 / 255.0_f32,
    g: 0xe9 as f32 / 255.0_f32,
    b: 0xf0 as f32 / 255.0_f32,
    a: 0.4_f32,
};
const SERPULO_ICE: Rgb = Rgb {
    r: 0xc2 as f32 / 255.0_f32,
    g: 0xbf as f32 / 255.0_f32,
    b: 0xfb as f32 / 255.0_f32,
    a: 0.35_f32,
};
const SERPULO_BASALT: Rgb = Rgb {
    r: 0x51 as f32 / 255.0_f32,
    g: 0x51 as f32 / 255.0_f32,
    b: 0x51 as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_MOSS: Rgb = Rgb {
    r: 0x6b as f32 / 255.0_f32,
    g: 0x44 as f32 / 255.0_f32,
    b: 0x74 as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_HOTROCK: Rgb = Rgb {
    r: 0x3c as f32 / 255.0_f32,
    g: 0x38 as f32 / 255.0_f32,
    b: 0x38 as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_SPORE_MOSS: Rgb = Rgb {
    r: 0x6b as f32 / 255.0_f32,
    g: 0x44 as f32 / 255.0_f32,
    b: 0x74 as f32 / 255.0_f32,
    a: 1.0_f32,
};
const SERPULO_SHALE: Rgb = Rgb {
    r: 0x67 as f32 / 255.0_f32,
    g: 0x65 as f32 / 255.0_f32,
    b: 0x93 as f32 / 255.0_f32,
    a: 1.0_f32,
};

/// Java `SERPULO_ARR`：温度带 × 高度档的方块表，逐元素照抄 `SerpuloPlanetGenerator.arr`。
const SERPULO_ARR: [[Rgb; 13]; 13] = [
    [SERPULO_WATER_RGB, SERPULO_DARKSAND_WATER, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_DARKSAND_TAINTED_WATER, SERPULO_STONE, SERPULO_STONE],
    [SERPULO_WATER_RGB, SERPULO_DARKSAND_WATER, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_DARKSAND_TAINTED_WATER, SERPULO_STONE, SERPULO_STONE, SERPULO_STONE],
    [SERPULO_WATER_RGB, SERPULO_DARKSAND_WATER, SERPULO_DARKSAND, SERPULO_SAND, SERPULO_SALT, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_DARKSAND_TAINTED_WATER, SERPULO_STONE, SERPULO_STONE, SERPULO_STONE],
    [SERPULO_WATER_RGB, SERPULO_SAND_WATER, SERPULO_SAND, SERPULO_SALT, SERPULO_SALT, SERPULO_SALT, SERPULO_SAND, SERPULO_STONE, SERPULO_STONE, SERPULO_STONE, SERPULO_SNOW, SERPULO_ICE_SNOW, SERPULO_ICE],
    [SERPULO_DEEP_WATER, SERPULO_WATER_RGB, SERPULO_SAND_WATER, SERPULO_SAND, SERPULO_SALT, SERPULO_SAND, SERPULO_SAND, SERPULO_BASALT, SERPULO_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_ICE],
    [SERPULO_DEEP_WATER, SERPULO_WATER_RGB, SERPULO_SAND_WATER, SERPULO_SAND, SERPULO_SAND, SERPULO_SAND, SERPULO_MOSS, SERPULO_ICE_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_ICE, SERPULO_SNOW, SERPULO_ICE],
    [SERPULO_DEEP_WATER, SERPULO_SAND_WATER, SERPULO_SAND, SERPULO_SAND, SERPULO_MOSS, SERPULO_MOSS, SERPULO_SNOW, SERPULO_BASALT, SERPULO_BASALT, SERPULO_BASALT, SERPULO_ICE, SERPULO_SNOW, SERPULO_ICE],
    [SERPULO_DEEP_TAINTED_WATER, SERPULO_DARKSAND_TAINTED_WATER, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_BASALT, SERPULO_MOSS, SERPULO_BASALT, SERPULO_HOTROCK, SERPULO_BASALT, SERPULO_ICE, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE],
    [SERPULO_DARKSAND_WATER, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_MOSS, SERPULO_SPORE_MOSS, SERPULO_SNOW, SERPULO_BASALT, SERPULO_BASALT, SERPULO_ICE, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE],
    [SERPULO_DARKSAND_WATER, SERPULO_DARKSAND, SERPULO_DARKSAND, SERPULO_SPORE_MOSS, SERPULO_ICE, SERPULO_ICE, SERPULO_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE],
    [SERPULO_DEEP_TAINTED_WATER, SERPULO_DARKSAND_TAINTED_WATER, SERPULO_DARKSAND, SERPULO_SPORE_MOSS, SERPULO_SPORE_MOSS, SERPULO_ICE, SERPULO_ICE, SERPULO_SNOW, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE],
    [SERPULO_TAINTED_WATER, SERPULO_DARKSAND_TAINTED_WATER, SERPULO_DARKSAND, SERPULO_SPORE_MOSS, SERPULO_MOSS, SERPULO_SPORE_MOSS, SERPULO_ICE_SNOW, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE],
    [SERPULO_DARKSAND_WATER, SERPULO_DARKSAND, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_SNOW, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE, SERPULO_ICE],
];

/// Java `serpuloMesher(int seed)` 里的匿名类（含它的 private 常量与方法）。
pub struct SerpuloMesher {
    pub seed: i32,
}

impl SerpuloMesher {
    /// Java `private float rawHeight(PlanetGrids.Vec3 position)`。
    fn raw_height(&self, position: &Vec3) -> f32 {
        let height: f64 = f64::from(
            noise3d(
                self.seed,
                7.0,
                f64::from(0.5_f32),
                f64::from(1.0_f32 / 3.0_f32),
                f64::from(position.x * SERPULO_SCL),
                f64::from(position.y * SERPULO_SCL + SERPULO_HEIGHT_Y_OFFSET),
                f64::from(position.z * SERPULO_SCL),
            ) * SERPULO_HEIGHT_SCL,
        );
        // Java：`(float) ((Math.pow(height, 2.3) + WATER_OFFSET) / (1 + WATER_OFFSET))`，
        // 其中 `1 + WATER_OFFSET` 是 int + float ⇒ float，再提升回 double 参与除法。
        ((height.powf(2.3) + f64::from(SERPULO_WATER_OFFSET)) / f64::from(1.0_f32 + SERPULO_WATER_OFFSET)) as f32
    }

    /// Java `private double distanceToSouthPole(PlanetGrids.Vec3 position)`：游戏里的
    /// `position.dst(0, 0, 1)`。
    fn distance_to_south_pole(&self, position: &Vec3) -> f64 {
        // Java：`double dz = position.z - 1;` —— 先在 float 上减，再提升到 double。
        let dx: f64 = f64::from(position.x);
        let dy: f64 = f64::from(position.y);
        let dz: f64 = f64::from(position.z - 1.0_f32);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

impl Mesher for SerpuloMesher {
    fn height(&self, position: &Vec3) -> f32 {
        java_max_f32(self.raw_height(position), SERPULO_WATER)
    }

    fn color(&self, position: &Vec3) -> Rgb {
        let raw = self.raw_height(position);
        let px: f32 = position.x * SERPULO_SCL;
        let py: f32 = position.y * SERPULO_SCL;
        let pz: f32 = position.z * SERPULO_SCL;
        let mut temp: f32 = java_max_f32(0.0, java_min_f32(1.0, f32::abs(py * 2.0_f32) / SERPULO_SCL));
        let tnoise: f32 = noise3d(
            self.seed,
            7.0,
            f64::from(0.56_f32),
            f64::from(1.0_f32 / 3.0_f32),
            f64::from(px),
            f64::from(py + 999.0_f32 - 0.1_f32),
            f64::from(pz),
        );
        temp = temp + (tnoise - temp) * 0.5_f32;
        let height: f32 = java_max_f32(0.0, java_min_f32(1.0, raw * 1.2_f32));
        let tar: f32 = (f64::from(
            noise3d(
                self.seed,
                4.0,
                f64::from(0.55_f32),
                f64::from(1.0_f32 / 2.0_f32),
                f64::from(px),
                f64::from(py + 999.0_f32),
                f64::from(pz),
            ) * 0.3_f32,
        ) + self.distance_to_south_pole(position) * f64::from(0.2_f32))
            as f32;
        // Java：`(int) (temp * 13)` 先向零截断（NaN → 0），再走 int 重载的 Math.min(12, ·) /
        // Math.max(0, ·)。
        let row = (0_i32).max(((temp * 13.0_f32) as i32).min(12)) as usize;
        let col = (0_i32).max(((height * 13.0_f32) as i32).min(12)) as usize;
        let mut res = SERPULO_ARR[row][col];
        // Java 用 `res == MOSS` 比较引用同一性；Rust 这里是值相等。调色板里与 MOSS/SPORE_MOSS
        // 数值相同的只有 SPORE_MOSS 本身（两者取值完全一致），故判定集合不变。
        if tar > 0.5_f32 && (res == SERPULO_MOSS || res == SERPULO_SPORE_MOSS) {
            res = SERPULO_SHALE;
        }
        if res == SERPULO_SALT {
            res = SERPULO_SAND; // SerpuloPlanetGenerator.getColor 把 salt 换成 sand
        }
        res
    }
}

/// Java `serpuloMesher(int seed)`：vanilla serpulo 地形，移植 `SerpuloPlanetGenerator.rawHeight/
/// getBlock`（视觉路径，不含 base 光照），让没有 mesh 配置的星球看起来和游戏里一样。
pub fn serpulo_mesher(seed: i32) -> Box<dyn Mesher> {
    Box::new(SerpuloMesher { seed })
}

/// Java `cloudLayersFromConfig(JsonObject)`：从 `cloudMesh` 对象得到云层——MultiMesh 的子项，
/// 或该网格本身。
pub fn cloud_layers_from_config(cloud: Option<&Object>) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    let Some(cloud) = cloud else { return layers };
    if mesh_type(Some(cloud)) == "MultiMesh"
        && let Some(array) = cloud.get("meshes").and_then(|value| value.as_array())
    {
        for element in array {
            if let Some(object) = element.as_object() {
                layers.push(layer_from(object));
            }
        }
        return layers;
    }
    layers.push(layer_from(cloud));
    layers
}

/// Java `private static Layer layerFrom(JsonObject config)`。
///
/// 半径语义直接来自游戏源码：
/// - `NoiseMesh` 用 JSON 的 radius 作为**绝对**网格半径来构建（`NoiseMesh.java`:
///   `buildHex(mesher, divisions, radius, 0.2f)`）；
/// - `SunMesh`/`HexMesh` 一类的构建用 `planet.radius`（`HexMesh.java`），且太阳的 shader 不受光
///   （完全没有漫反射着色）；
/// - `HexSkyMesh` 在 `planet.radius` 上构建，JSON 的 radius 是 `buildHex` 的 intensity
///   （`HexSkyMesh.java`），并按 `Time.globalTime * speed / 40` 旋转。
fn layer_from(config: &Object) -> Layer {
    let type_ = mesh_type(Some(config));
    let sky = type_ == "HexSkyMesh";
    let sun = type_ == "SunMesh";
    let intensity = if sky {
        num_or(Some(config), "radius", f64::from(1.0_f32)) as f32
    } else {
        0.2_f32
    };
    let relative = sky || sun;
    let radius = if relative {
        1.0_f32
    } else {
        num_or(Some(config), "radius", f64::from(1.0_f32)) as f32
    };
    let divisions = 1.max(int_or(Some(config), "divisions", if sky { 3 } else { 1 }));
    let cloud_speed = if sky {
        num_or(Some(config), "speed", f64::from(0.0_f32)) as f32
    } else {
        0.0_f32
    };
    let mesher: Box<dyn Mesher> = if sky {
        hex_sky_mesh(
            int_or(Some(config), "seed", 0),
            int_or(Some(config), "octaves", 1),
            num_or(Some(config), "persistence", f64::from(0.5_f32)) as f32,
            num_or(Some(config), "scale", f64::from(1.0_f32)) as f32,
            num_or(Some(config), "thresh", f64::from(0.5_f32)) as f32,
            color_or(config.get("color"), "ffffff"),
        )
    } else {
        mesher_from_config(config)
    };
    Layer {
        mesher,
        intensity,
        radius,
        divisions,
        unlit: sun,
        sky,
        radius_is_planet_relative: relative,
        cloud_speed,
    }
}

/// Java `PlanetMeshes.Layer`（record）：一层渲染网格——mesher 加它的半径、buildHex intensity 与
/// 网格 divisions。
///
/// `unlit` 对应 `Shaders.unlit`（SunMesh 直接渲染原始 albedo）；`sky` 标记 `HexSkyMesh` 层
/// （clouds.vert 着色：无高光，alpha 取自颜色）；`radius_is_planet_relative` 对应游戏实际选用的
/// 那次 `buildHex` 调用（`planet.radius` 还是 JSON 的 radius）；`cloud_speed` 驱动 `HexSkyMesh`
/// 的旋转（`relRot = time * speed / 40`）。
pub struct Layer {
    pub mesher: Box<dyn Mesher>,
    pub intensity: f32,
    pub radius: f32,
    pub divisions: i32,
    pub unlit: bool,
    pub sky: bool,
    pub radius_is_planet_relative: bool,
    pub cloud_speed: f32,
}

/// Java `private static String meshType(JsonObject mesh)`：规范化后的类型名，与游戏一致
/// （`Strings.capitalize`）。
fn mesh_type(mesh: Option<&Object>) -> String {
    let raw = string_or(mesh, "type", "NoiseMesh");
    // Java 的入参恒非 null，故 canonical 也恒非 null。
    mesh_types::canonical(Some(&raw)).unwrap_or(raw)
}

/// Java `normalizeMeshConfig(JsonElement)`：把 `mesh`/`cloudMesh` 的值规范成各层提取器接受的
/// 对象形式。
///
/// 游戏的 JSON 解析器把裸数组当作 MultiMesh 简写（`ContentParser.parseMesh`:
/// `if(data.isArray()) return new MultiMesh(...)`），而真实 mod 就是多生态星球就这么写的——
/// 于是 `[{...},{...}]` 变成 `{type: MultiMesh, meshes: [...]}`。缺失或非网格的值返回 `None`。
///
/// Java 对对象形式返回**同一个** `JsonObject` 引用；Rust 是值语义，返回副本。
pub fn normalize_mesh_config(value: Option<&Json>) -> Option<Object> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    if let Json::Obj(object) = value {
        return Some(object.clone());
    }
    if let Json::Arr(array) = value
        && !array.is_empty()
    {
        let mut multi = Object::new();
        multi.insert("type", Json::str("MultiMesh"));
        multi.insert("meshes", Json::Arr(array.clone()));
        return Some(multi);
    }
    None
}

/// Java `meshLayerConfigs(JsonElement)`：`mesh`/`cloudMesh` 值的原始层对象，供结构化编辑——
/// MultiMesh 对象贡献它的子项，裸数组（游戏的 MultiMesh 简写）贡献它的元素，单个网格对象贡献
/// 自己。它是「打包成一个对象 / 一个数组」的逆运算。
pub fn mesh_layer_configs(value: Option<&Json>) -> Vec<Object> {
    let mut layers: Vec<Object> = Vec::new();
    let Some(normalized) = normalize_mesh_config(value) else {
        return layers;
    };
    if mesh_type(Some(&normalized)) == "MultiMesh"
        && let Some(array) = normalized.get("meshes").and_then(|item| item.as_array())
    {
        for element in array {
            if let Some(object) = element.as_object() {
                layers.push(object.clone());
            }
        }
        return layers;
    }
    layers.push(normalized);
    layers
}

/// Java `surfaceLayersFromConfig(JsonObject)`：`mesh` 对象的所有表面层——MultiMesh 的子项各自渲染
/// 成独立的一层壳（环、卫星），MatMesh 包一层嵌套网格，其余都是单层。
pub fn surface_layers_from_config(mesh: Option<&Object>) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    let Some(mesh) = mesh else { return layers };
    if mesh_type(Some(mesh)) == "MultiMesh"
        && let Some(array) = mesh.get("meshes").and_then(|value| value.as_array())
    {
        for element in array {
            if let Some(object) = element.as_object() {
                layers.push(layer_from(object));
            }
        }
        return layers;
    }
    layers.push(layer_from(mesh));
    layers
}

// --- model building -------------------------------------------------------------------------

/// Java `buildModel(int sectorSize, float radius, Mesher mesher, float intensity)`：为一层构建
/// tile 几何，镜像 `MeshBuilder.buildHex`。
pub fn build_model(sector_size: i32, radius: f32, mesher: &dyn Mesher, intensity: f32) -> MeshModel {
    let mut model = MeshModel::new(sector_size, radius);
    if sector_size <= 0 {
        return model;
    }
    let grid = grids::create(sector_size);
    let mut heights = vec![0.0_f32; grid.corners.len()];
    for i in 0..heights.len() {
        let corner = grid.corners[i].borrow();
        heights[i] = (1.0_f32 + mesher.height(&corner.v) * intensity) * radius;
    }
    for (i, tile_entry) in grid.tiles.iter().enumerate() {
        let tile_rc = Rc::clone(tile_entry);
        let tile = tile_rc.borrow();
        if mesher.skip(&tile.v) {
            model.tile_skipped.push(true);
            model.tile_colors.push(None);
            model.tile_corners.push(Vec::new());
            model.tile_indexes.push(i as i32);
            continue;
        }
        model.tile_skipped.push(false);
        model.tile_colors.push(Some(mesher.color(&tile.v)));
        let mut corners = vec![0.0_f64; tile.corners.len() * 3];
        for k in 0..tile.corners.len() {
            // Java 的 `tile.corners` 是构建时填满的数组；这里的 `Option` 对应 Java 的 `null` 槽。
            let corner_ref = tile.corners[k]
                .as_ref()
                .expect("unassigned grid tile corner");
            let corner = corner_ref.borrow();
            let height = heights[corner.id as usize];
            corners[k * 3] = f64::from(corner.v.x * height);
            corners[k * 3 + 1] = f64::from(corner.v.y * height);
            corners[k * 3 + 2] = f64::from(corner.v.z * height);
        }
        model.tile_corners.push(corners);
        model.tile_indexes.push(i as i32);
    }
    model
}

/// Java `Math.min(float, float)`：任一操作数是 NaN 时结果为 NaN（Rust 的 `f32::min` 会忽略 NaN）。
fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a <= b {
        a
    } else {
        b
    }
}

/// Java `Math.max(float, float)`：同上，NaN 传染。
fn java_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a >= b {
        a
    } else {
        b
    }
}

// --- small parsers --------------------------------------------------------------------------

/// Java `stringOr(JsonObject, String, String)`（Java 的 object 入参可为 `null`）。
pub fn string_or(object: Option<&Object>, key: &str, fallback: &str) -> String {
    match object.and_then(|item| item.get(key)) {
        Some(value) if is_primitive(value) => value.get_as_string(),
        _ => fallback.to_string(),
    }
}

/// Java `intOr(JsonObject, String, int)`。
pub fn int_or(object: Option<&Object>, key: &str, fallback: i32) -> i32 {
    match object.and_then(|item| item.get(key)) {
        Some(value) if is_primitive(value) => match as_int(value) {
            Some(parsed) => parsed,
            // Java: catch (NumberFormatException ignored) { return fallback; }
            None => fallback,
        },
        _ => fallback,
    }
}

/// Java `numOr(JsonObject, String, double)`。
pub fn num_or(object: Option<&Object>, key: &str, fallback: f64) -> f64 {
    match object.and_then(|item| item.get(key)) {
        Some(value) if is_primitive(value) => match as_f64(value) {
            Some(parsed) => parsed,
            // Java: catch (NumberFormatException ignored) { return fallback; }
            None => fallback,
        },
        _ => fallback,
    }
}

/// Java `lower(String)`：与 locale 无关的解析守卫，供格式化噪声输入的调用方使用。
pub fn lower(text: Option<&str>) -> String {
    match text {
        Some(value) => value.to_lowercase(),
        None => String::new(),
    }
}

/// Gson `JsonElement.isJsonPrimitive()`：`null` 与 `JsonNull` 都不是 primitive。
fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Bool(_) | Json::Num(_) | Json::Str(_))
}

/// Gson `JsonPrimitive.getAsInt()`：数字原语走 `Number.intValue()`（向零截断），字符串原语走
/// `Integer.parseInt`（非法即抛 NumberFormatException，由调用方吞掉）。布尔原语在 Java 里抛的是
/// UnsupportedOperationException（不被这里的 catch 吞掉），本工具同样按「读不到 → 回退」处理。
fn as_int(value: &Json) -> Option<i32> {
    match value {
        Json::Num(text) => text.parse::<f64>().ok().map(|v| v as i32),
        Json::Str(text) => text.parse::<i32>().ok(),
        _ => None,
    }
}

/// Gson `JsonPrimitive.getAsDouble()`：`Number.doubleValue()`，字符串原语走 `Double.parseDouble`。
fn as_f64(value: &Json) -> Option<f64> {
    match value {
        Json::Num(text) | Json::Str(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! 数值向量移植自 Java `PlanetMeshesTest`：噪声高度公式、双色阈值、云层跳过、角点半径、
    //! 配置回落与 MultiMesh 简写。

    use super::*;
    use crate::planet::grids;

    fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x as f32, y as f32, z as f32)
    }

    fn color_is_one_of(actual: Rgb, candidates: &[Rgb]) {
        for candidate in candidates {
            if (actual.r - candidate.r).abs() < 1e-6
                && (actual.g - candidate.g).abs() < 1e-6
                && (actual.b - candidate.b).abs() < 1e-6
            {
                return;
            }
        }
        panic!("color {actual:?} matches no candidate");
    }

    #[test]
    fn noise_mesh_height_matches_the_game_formula() {
        let mesher = noise_mesh(
            2, 3, 0.5_f32, 1.0_f32, 0.08_f32,
            Rgb::of("3a6b8c"), Rgb::of("5e8c4a"), 1, 0.5_f32, 1.0_f32, 0.5_f32,
        );
        let position = vec3(0.31, -0.52, 0.79);
        // 锁定公式：seed + 7、位置 + 5、再乘 mag（噪声本身由 SimplexNoiseTest 的同一份实现负责）。
        let expected = noise3d(
            7 + 2,
            3.0,
            0.5,
            1.0,
            f64::from(5.0_f32 + position.x),
            f64::from(5.0_f32 + position.y),
            f64::from(5.0_f32 + position.z),
        ) * 0.08_f32;
        assert_eq!(expected, mesher.height(&position));
    }

    #[test]
    fn noise_mesh_color_is_thresholded_between_two_colors() {
        let color1 = Rgb::of("3a6b8c");
        let color2 = Rgb::of("5e8c4a");
        let mesher = noise_mesh(2, 3, 0.5_f32, 1.0_f32, 0.08_f32, color1, color2, 1, 0.5_f32, 1.0_f32, 0.5_f32);
        let grid = grids::create(2);
        for tile in &grid.tiles {
            let tile = tile.borrow();
            color_is_one_of(mesher.color(&tile.v), &[color1, color2]);
        }
    }

    #[test]
    fn hex_sky_clouds_skip_tiles_above_the_threshold() {
        let mesher = hex_sky_mesh(11, 2, 0.45_f32, 0.9_f32, 0.38_f32, Rgb::of("684eb9bf"));
        let grid = grids::create(2);
        let (mut drawn, mut skipped) = (0, 0);
        for tile in &grid.tiles {
            let tile = tile.borrow();
            if mesher.skip(&tile.v) {
                skipped += 1;
            } else {
                drawn += 1;
                assert_eq!(1.0_f32, mesher.height(&tile.v), "cloud tiles float above the surface");
            }
        }
        assert!(drawn > 0 && skipped > 0, "clouds should cover part of the planet");
        assert_eq!(92, drawn + skipped);
    }

    #[test]
    fn build_model_scales_corners_by_height_and_radius() {
        let mesher = single_color_mesh(0, 1, 0.5_f32, 1.0_f32, 0.0_f32, Rgb::of("ffffff"));
        // mag = 0 → 高度恒为 radius，每个角点都落在单位球上。
        let model = build_model(2, 1.0_f32, mesher.as_ref(), 0.2_f32);
        assert_eq!(92, model.tile_corners.len());
        for corners in &model.tile_corners {
            for point in corners.as_chunks::<3>().0 {
                let length = (point[0] * point[0] + point[1] * point[1] + point[2] * point[2]).sqrt();
                assert!((length - 1.0).abs() < 1e-5, "corners scaled by radius only stay on the sphere");
            }
        }
    }

    #[test]
    fn config_parsing_falls_back_like_parse_mesh() {
        let mut mesh = Object::new();
        mesh.insert("type", Json::str("NoiseMesh"));
        mesh.insert("seed", Json::num("2"));
        // color1/color2 缺失 → 两者都回落到 color
        mesh.insert("color", Json::str("3a6b8c"));
        let mesher = mesher_from_config(&mesh);
        let grid = grids::create(1);
        let expected = Rgb::of("3a6b8c");
        color_is_one_of(mesher.color(&grid.tiles[3].borrow().v), &[expected]);
    }

    #[test]
    fn unparseable_mesh_types_fall_back_instead_of_crashing() {
        let mut mesh = Object::new();
        mesh.insert("type", Json::str("HexMesh"));
        let mesher = mesher_from_config(&mesh);
        let grid = grids::create(1);
        assert!(mesher.height(&grid.tiles[0].borrow().v).is_finite());
    }

    #[test]
    fn cloud_layers_come_from_multi_mesh_children() {
        let cloud = mm_json::parse(
            r#"{"type": "MultiMesh", "meshes": [
                {"type": "HexSkyMesh", "seed": 11, "radius": 0.13, "color": "684eb9bf"},
                {"type": "HexSkyMesh", "seed": 1, "radius": 0.16, "color": "b3a3e4bf"}
            ]}"#,
        )
        .expect("解析云层配置");
        let config = normalize_mesh_config(Some(&cloud)).expect("对象原样通过");
        let layers = cloud_layers_from_config(Some(&config));
        assert_eq!(2, layers.len());
        assert!((layers[0].intensity - 0.13_f32).abs() < 1e-6);
        assert!((layers[1].intensity - 0.16_f32).abs() < 1e-6);
        assert!(layers.iter().all(|layer| layer.sky));
    }

    #[test]
    fn bare_mesh_arrays_normalise_to_multi_mesh_like_the_game() {
        let array = mm_json::parse(
            "[{type: NoiseMesh, color1: 252d40, divisions: 5},\
              {type: NoiseMesh, color1: 505e7f, divisions: 5}]",
        )
        .expect("解析裸数组");
        let multi = normalize_mesh_config(Some(&array)).expect("裸数组必须规范化");
        assert_eq!("MultiMesh", string_or(Some(&multi), "type", ""));
        let layers = surface_layers_from_config(Some(&multi));
        assert_eq!(2, layers.len(), "each biome renders as its own shell");
        assert_eq!(5, layers[0].divisions);

        let clouds = mm_json::parse("[{type: HexSkyMesh, radius: 0.15, divisions: 5, color: e2effc60}]")
            .expect("解析云层数组");
        let cloud_config = normalize_mesh_config(Some(&clouds)).expect("云层数组必须规范化");
        assert_eq!(1, cloud_layers_from_config(Some(&cloud_config)).len());

        // 对象原样通过；缺失/空数组是 None。
        let mut object = Object::new();
        object.insert("type", Json::str("NoiseMesh"));
        let value = Json::Obj(object);
        assert!(normalize_mesh_config(Some(&value)).is_some());
        assert!(normalize_mesh_config(None).is_none());
        let empty = mm_json::parse("[]").expect("解析空数组");
        assert!(normalize_mesh_config(Some(&empty)).is_none());
    }
}
