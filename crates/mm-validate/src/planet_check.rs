//! `PlanetCheck.java`（165 行）的等价移植：行星与其扇区预设——扇区 id 受行星的扇区数约束，
//! 后者是测地网格的 `10 × 3^sectorSize + 2` 个地块，再加游戏恒追加的 1 个退化扇区
//! （`Planet.java:227-233` 建地块、`ContentParser.java:856-857` 追加扇区），
//! [`mm_fmt::planet::grids::capacity`] 恰是这一计数。
//!
//! 与 Java 的语言差异（只有下面这几处）：
//! 1. `Set.of(...)`（`:29-31`、`:38-39`）是**无序**集合。成员判定与顺序无关，但
//!    `String.join(", ", MeshTypes.PARSEABLE)`（`:126`）的拼接顺序在 Java 里由哈希种子决定，
//!    同一句话在不同进程里可能换序。这里按 `MeshTypes.ALL` 的声明顺序去重保序输出，是 Java 的
//!    一种合法输出，属**收敛**而非新增行为。
//! 2. `Map<String, ContentRecord> planets`（`:49`）↔ [`KeyedMap<&ContentRecord>`]：保插入顺序、
//!    `put` 覆盖保位并返回旧值，与 `LinkedHashMap` 逐一对应。值用借用（`&'a ContentRecord`），
//!    避免深拷贝整棵 `fields` 树；Java 交的是同一批活对象，观察效果一致。
//! 3. `String.toLowerCase(Locale.ROOT)`（`:153`）↔ Rust `to_lowercase()`：仅 U+0130 一类码点的
//!    映射不同，行星名语料是 ASCII，判定方向一致。
//! 4. `(int) Math.ceil(x)`（`:73`、`:78`、`:97`、`:159`、`:163`）↔ `x.ceil() as i32`：
//!    `f64::ceil` 与 `Math.ceil` 逐值相同，`as i32` 的向零截断 + NaN→0 + 超界饱和与 Java
//!    窄化转换（JLS 5.1.3）一致。
//! 5. `Integer sectors`（`:91`）↔ [`Option<i32>`]；`sectorIndex >= sectors` 里的自动拆箱提升
//!    为 `f64` 比较，这里显式 `sectors as f64`。
//! 6. `MeshTypes.canonical(String)` 只在入参为 `null` 时返回 `null`，而 `getAsString()` 对
//!    primitive 恒不为 `null`，故 `:122` 之后 Java 不会在 `Set.contains(null)` 上抛 NPE；
//!    Rust 的 `canonical(Some(..))` 因此必为 `Some`，`unwrap_or_else` 一支不可达，只为满足类型系统。
//! 7. `:163` 的 `size`（装箱 `Double`）在这一行必非 `null`（`:147-151` 已把 `size == null` 的行星
//!    分支 return 掉），Java 在此自动拆箱；Rust 用 `match size { None => return None }` 承接这个
//!    不可达分支，与 Java 的「不可能走到」一致，不产生额外发现。

use mm_fmt::planet::{grids, mesh_types};
use mm_json::Json;
use mm_model::{ContentRecord, ModProject};
use mm_schema::{ContentType, KeyedMap};

use crate::check_support;
use crate::issue::{Arg, Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `VANILLA_GENERATORS`（`PlanetCheck.java:23-31`）。
///
/// 字段 javadoc 直译：游戏自带的行星生成器（`mindustry.maps.planet` 与 `maps.generators`，
/// v160.5）。它们一个都不起作用——注册好的 `PlanetGenerator` 解析器无视 JSON 的值，永远造一个
/// `AsteroidGenerator`（`ContentParser.java:270-276`）——所以这份名单只用来区分「游戏认识的名字」
/// 和「游戏不认识的名字」，也就是下面那条 warning 要报的内容。
const VANILLA_GENERATORS: [&str; 6] = [
    "PlanetGenerator",
    "BlankPlanetGenerator",
    "AsteroidGenerator",
    "ErekirPlanetGenerator",
    "SerpuloPlanetGenerator",
    "TantrosPlanetGenerator",
];

/// Java `VANILLA_MESHES`（`PlanetCheck.java:33-39`）。
///
/// 字段 javadoc 直译：游戏自带的行星网格（`mindustry.graphics.g3d` 加
/// `graphics/CubemapMesh.java`，v160.5）；`parseMesh` 对类型名做 switch
/// （`ContentParser.java:1128-1156`），名单外的名字建不出来。可解析的子集在 [`mesh_types`] 里。
const VANILLA_MESHES: [&str; 10] = [
    "GenericMesh",
    "HexMesh",
    "HexSkyMesh",
    "MatMesh",
    "MultiMesh",
    "NoiseMesh",
    "PlanetMesh",
    "ShaderSphereMesh",
    "SunMesh",
    "CubemapMesh",
];

/// Gson `JsonElement.isJsonPrimitive()`（`PlanetCheck.java:59`、`:119`、`:141`）：
/// `Bool`/`Num`/`Str`。mm_json 未导出同名方法，按 `mm-json/src/value.rs` 的既有判定在本文件内实现。
fn is_json_primitive(value: &Json) -> bool {
    value.is_bool() || value.is_number() || value.is_string()
}

/// Java `(int) Math.ceil(x)`（模块头第 4 条）。
fn java_ceil_i32(value: f64) -> i32 {
    value.ceil() as i32
}

/// Java `class PlanetCheck implements ProjectCheck`（`PlanetCheck.java:21`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanetCheck;

impl ProjectCheck for PlanetCheck {
    /// Java `id()`（`PlanetCheck.java:41-44`）。
    fn id(&self) -> &str {
        "planets"
    }

    /// Java `check(ValidationContext)`（`PlanetCheck.java:46-100`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :48  ModProject project = context.project();
        let project: &ModProject = context.project();

        // :49  Map<String, ContentRecord> planets = new LinkedHashMap<>();
        let mut planets: KeyedMap<&ContentRecord> = KeyedMap::new();

        // :50-52  脚本型模组（rhino JS）能定义自己的类，纯 JSON 模组不能。
        let script_capable: bool = project
            .assets
            .iter()
            .any(|asset| asset.path.starts_with("scripts/"));

        // :53-82  第一个循环：行星本身
        for record in &project.contents {
            // :54-56
            if record.ctype != ContentType::Planet || record.is_inline() {
                continue;
            }
            // :57  planets.put(record.name, record)
            planets.put(record.name.clone(), record);

            // :58-60  generator 是 primitive 且不在游戏自带的名单里
            let generator: Option<&Json> = record.fields.get("generator");
            if let Some(generator) = generator
                && is_json_primitive(generator)
            {
                let text: String = generator.get_as_string();
                // :61-69  方法注释（`:61-67`）直译：游戏的 PlanetGenerator 解析器完全无视 JSON
                //         的值、永远返回 AsteroidGenerator（ContentParser.java:270-276），所以这个名字
                //         对行星没有影响——但行星仍能加载，故不是 error。脚本也提供不了生成器：解析器是
                //         普通 Java lambda，不是类表查找，所以对脚本模组用同一措辞。游戏确实自带的那些
                //         名字即便同样被无视也保持沉默，因为每个原版行星文件都写着一个，报警纯属噪音。
                if !VANILLA_GENERATORS.contains(&text.as_str()) {
                    context.report(Issue::new(
                        Level::Warning,
                        "validate.planet.badGenerator",
                        &[record.name.as_str(), text.as_str()],
                        record.id.clone(),
                    ));
                }
            }

            // :71-79  sectorSize 超出测地网格可表示范围
            let sector_size: Option<f64> = check_support::number(record.fields.get("sectorSize"));
            if let Some(sector_size) = sector_size
                && sector_size > 0.0
                && !grids::is_representable(java_ceil_i32(sector_size))
            {
                // 方法注释（`:74-76`）直译：游戏由这个值导出一个 int 尺寸的网格
                // （10·3^n 地块、30·3^n 条边，Planet.java:227-233），所以超出可表示范围时行星
                // 根本建不出来——而且它的扇区数会绕回一个荒唐的数字。
                // Java 的 `:78` 交的是 `record.name`（`String`）加 `(int) Math.ceil(sectorSize)`
                // 与 `PlanetGrids.MAX_SECTOR_SIZE`（`int`）⇒ 后两支是 [`Arg::int`]。
                context.report(Issue::new(
                    Level::Error,
                    "validate.planet.sectorSizeTooLarge",
                    &[
                        Arg::text(record.name.as_str()),
                        Arg::int(java_ceil_i32(sector_size) as i64),
                        Arg::int(grids::MAX_SECTOR_SIZE as i64),
                    ],
                    record.id.clone(),
                ));
            }

            // :80-81  mesh 与 cloudMesh 同一套规则，字段名进消息
            self.check_mesh_type(context, record, record.fields.get("mesh"), "mesh", script_capable);
            self.check_mesh_type(
                context,
                record,
                record.fields.get("cloudMesh"),
                "cloudMesh",
                script_capable,
            );
        }

        // :83-99  第二个循环：扇区预设
        for record in &project.contents {
            // :84-86
            if record.ctype != ContentType::Sector || record.is_inline() {
                continue;
            }
            // :87-90  `sectorIndex == null || sectorIndex < 0` ⇒ continue。
            //         这一条必须在 :91 之前短路：Java 在此跳过时**不会**调用 planetSectorCount，
            //         因而也就不会带上那条 `validate.planet.noSectorSize` 副发现。
            let Some(sector_index) = check_support::number(record.fields.get("sector")) else {
                continue;
            };
            if sector_index < 0.0 {
                continue;
            }
            // :91-94
            let Some(sectors) = self.planet_sector_count(context, project, &planets, record) else {
                continue;
            };
            // :95-98  模块头第 5 条：Java 的 `sectorIndex >= sectors` 拆箱后按 f64 比
            if sector_index >= sectors as f64 {
                // Java 的 `:97` 交的是 `record.name`（`String`）、`(int) Math.ceil(sectorIndex)`、
                // `sectors`（`Integer`，装箱后仍是 `Number`）与 `sectors - 1`（拆箱后相减的 `int`）
                // ⇒ 后三支都是 [`Arg::int`]。
                context.report(Issue::new(
                    Level::Warning,
                    "validate.sector.outOfRange",
                    &[
                        Arg::text(record.name.as_str()),
                        Arg::int(java_ceil_i32(sector_index) as i64),
                        Arg::int(sectors as i64),
                        Arg::int((sectors - 1) as i64),
                    ],
                    record.id.clone(),
                ));
            }
        }
    }
}

impl PlanetCheck {
    /// Java `private void checkMeshType(ValidationContext, ContentRecord, JsonElement, String,
    /// boolean)`（`PlanetCheck.java:102-135`）。
    fn check_mesh_type(
        &self,
        context: &mut ValidationContext<'_>,
        record: &ContentRecord,
        mesh: Option<&Json>,
        field: &str,
        script_capable: bool,
    ) {
        // :104-106
        let Some(mesh) = mesh else { return };

        // :107-117  注释直译：裸数组是 parseMeshes 直接接受的 MultiMesh 简写，但既非对象又非数组的
        //           东西是致命的：行星解析器在读内容时用 "Meshes must be objects." 拒掉它
        //           （mesh 在 ContentParser.java:813-815，cloudMesh 在 828-830），行星永不加载。
        if !mesh.is_object() {
            if !mesh.is_array() {
                context.report(Issue::new(
                    Level::Error,
                    "validate.planet.meshNotObject",
                    &[record.name.as_str(), field],
                    record.id.clone(),
                ));
            }
            return;
        }

        // :118-121  type 缺失或非 primitive ⇒ 不判
        let Json::Obj(object) = mesh else { return };
        let Some(type_) = object.get("type") else { return };
        if !is_json_primitive(type_) {
            return;
        }

        // :122  模块头第 6 条：canonical 只对 null 入参返回 null，这里必为 Some
        let name: String = mesh_types::canonical(Some(type_.get_as_string().as_str()))
            .unwrap_or_else(|| type_.get_as_string());

        if VANILLA_MESHES.contains(&name.as_str()) {
            // :124-127  游戏认识但建不出来（parseMesh 会抛 "Unknown mesh type"）
            if !mesh_types::is_parseable(Some(name.as_str())) {
                let parseable: String = mesh_types::parseable().join(", ");
                context.report(Issue::new(
                    Level::Warning,
                    "validate.planet.meshUnparseable",
                    &[record.name.as_str(), field, name.as_str(), parseable.as_str()],
                    record.id.clone(),
                ));
            }
        // :128-130  脚本型模组：也许正是脚本注册了自己的类，降为 warning
        } else if script_capable {
            context.report(Issue::new(
                Level::Warning,
                "validate.planet.meshUnknownScript",
                &[record.name.as_str(), field, name.as_str()],
                record.id.clone(),
            ));
        // :131-133  纯 JSON 模组：建不出来，error
        } else {
            context.report(Issue::new(
                Level::Error,
                "validate.planet.meshUnknownJson",
                &[record.name.as_str(), field, name.as_str()],
                record.id.clone(),
            ));
        }
    }

    /// Java `private Integer planetSectorCount(ValidationContext, ModProject,
    /// Map<String, ContentRecord>, ContentRecord)`（`PlanetCheck.java:137-164`）。
    ///
    /// 方法注释（`:137`）直译：某个扇区预设所在行星的扇区槽位数；无法确定时为 null。
    fn planet_sector_count(
        &self,
        context: &mut ValidationContext<'_>,
        project: &ModProject,
        planets: &KeyedMap<&ContentRecord>,
        sector: &ContentRecord,
    ) -> Option<i32> {
        // :140-143  planet 字段缺失或非 primitive ⇒ 判不了
        let planet_ref = sector.fields.get("planet")?;
        if !is_json_primitive(planet_ref) {
            return None;
        }

        // :144-145  去掉 `<internal>-` 前缀后查行星表。两个 String 都要先落成具名局部量，
        //           否则 `strip_prefix` 返回的借用会指向语句结束就被释放的临时值。
        let referenced: String = planet_ref.get_as_string();
        let internal: String = project.internal_name();
        let name: &str = check_support::strip_prefix(referenced.as_str(), internal.as_str());
        let planet: Option<&ContentRecord> = planets.get(name).copied();

        // :146  Double size = planet == null ? null : number(planet.fields.get("sectorSize"))
        let size: Option<f64> =
            planet.and_then(|planet| check_support::number(planet.fields.get("sectorSize")));

        // :147-151  行星在本工程里、但没写或写小了 sectorSize
        if let Some(planet) = planet {
            let missing_or_small = match size {
                None => true,
                Some(size) => size <= 0.0,
            };
            if missing_or_small {
                context.report(Issue::new(
                    Level::Warning,
                    "validate.planet.noSectorSize",
                    &[planet.name.as_str(), sector.name.as_str()],
                    sector.id.clone(),
                ));
                return None;
            }
        }

        // :152-158  行星不在本工程里：查游戏自带的尺寸
        if planet.is_none() {
            let vanilla_size: i32 = grids::vanilla_size(&name.to_lowercase())?;
            return Some(grids::capacity(vanilla_size));
        }

        // :159-162  注释：PlanetCheck 已经报过一次；这里的扇区数没有意义
        //         模块头第 7 条：走到这里 size 必为 Some
        let size: f64 = size?;
        if !grids::is_representable(java_ceil_i32(size)) {
            return None;
        }

        // :163
        Some(grids::capacity(java_ceil_i32(size)))
    }
}
