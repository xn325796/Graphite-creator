//! `PlanetGrids.java` 的等价移植：游戏的 `mindustry.graphics.g3d.PlanetGrid`——扇区所依据的测地
//! icosphere 网格。
//!
//! SectorPreset JSON 里的扇区下标就是这张网格里 tile 的下标，所以移植必须逐字复现源码——同样的
//! 常量、同样的细分顺序、同样的 tile 编号。
//!
//! 本类还带着星球工作台需要的 sector-preset 簿记：一个星球能容纳多少扇区（`10*3^size + 2` 个
//! tile，再加上游戏总是追加的那个退化槽），以及工程里的 sector 记录占据哪些下标。
//!
//! Java 用引用语义连接这张图（`Tile[] tiles` 里存的是同一个 `Tile` 对象）；Rust 里用
//! `Rc<RefCell<_>>` 复刻同一套共享可变对象图，`Rc::ptr_eq` 对应 Java 的 `==`。

use std::cell::RefCell;
use std::rc::Rc;

use mm_json::Json;
use mm_model::{ContentRecord, ModProject};

use crate::ContentType;

/// Java `VANILLA_SIZES = Map.of(...)`：mod 扇区常引用的 vanilla 星球的尺寸（Planets.java）。
pub const VANILLA_SIZES: [(&str, i32); 3] = [("serpulo", 3), ("erekir", 2), ("tantros", 2)];

/// Java `VANILLA_SIZES.get(name)`。
pub fn vanilla_size(name: &str) -> Option<i32> {
    VANILLA_SIZES.iter().find(|(key, _)| *key == name).map(|(_, value)| *value)
}

/// Java `private static final float PX`。
// 字面量与 Java 源码逐字一致（舍入到 f32 后同值），为保持「同样的常量」登记不缩短写法。
#[allow(clippy::excessive_precision)]
const PX: f32 = -0.525731112119133606_f32;
/// Java `private static final float PZ`。
// 同上：保留 Java 原始字面量，excessive_precision 的建议值与其 f32 舍入结果相同。
#[allow(clippy::excessive_precision)]
const PZ: f32 = -0.850650808352039932_f32;

/// Java `private static final float[][] I_TILES`：正二十面体的 12 个顶点。
const I_TILES: [[f32; 3]; 12] = [
    [-PX, 0.0, PZ],
    [PX, 0.0, PZ],
    [-PX, 0.0, -PZ],
    [PX, 0.0, -PZ],
    [0.0, PZ, PX],
    [0.0, PZ, -PX],
    [0.0, -PZ, PX],
    [0.0, -PZ, -PX],
    [PZ, PX, 0.0],
    [-PZ, PX, 0.0],
    [PZ, -PX, 0.0],
    [-PZ, -PX, 0.0],
];

/// Java `private static final int[][] I_TILES_P`：与每个面相邻的面。
const I_TILES_P: [[i32; 5]; 12] = [
    [9, 4, 1, 6, 11],
    [4, 8, 10, 6, 0],
    [11, 7, 3, 5, 9],
    [2, 7, 10, 8, 5],
    [9, 5, 8, 1, 0],
    [2, 3, 8, 4, 9],
    [0, 1, 10, 7, 11],
    [11, 6, 10, 3, 2],
    [5, 3, 10, 1, 4],
    [2, 5, 4, 0, 11],
    [3, 7, 6, 1, 8],
    [7, 2, 9, 0, 6],
];

/// Java `MAX_SECTOR_SIZE`：游戏按 int 大小的数组分配网格，仍能装下的最大 `sectorSize`。
///
/// `Planet.java:227-233` 会构造 `10 * 3^n + 2` 个 tile、`30 * 3^n` 条边的 `PlanetGrid`，而
/// `30 * 3^n <= Integer.MAX_VALUE` 成立到 n = 16。超过它就根本不可表示，工具必须明说，而不是
/// 报一个已经回绕的计数。
pub const MAX_SECTOR_SIZE: i32 = 16;

/// Java `tileCount(int size)`。
pub fn tile_count(size: i32) -> i32 {
    saturate(10_i64 * pow3(size) + 2)
}

/// Java `cornerCount(int size)`。
pub fn corner_count(size: i32) -> i32 {
    saturate(20_i64 * pow3(size))
}

/// Java `edgeCount(int size)`。
pub fn edge_count(size: i32) -> i32 {
    saturate(30_i64 * pow3(size))
}

/// 游戏那些 int 大小的网格数组能否装下这个 `sectorSize` 的星球。
pub fn is_representable(size: i32) -> bool {
    size <= 0 || size <= MAX_SECTOR_SIZE
}

/// 3^size，以 long 计算并在超出 int 时饱和，使任何调用方都拿不到回绕后的值。
fn pow3(size: i32) -> i64 {
    let mut result = 1_i64;
    for _ in 0..size {
        result *= 3;
        if result > i64::from(i32::MAX) {
            return i64::from(i32::MAX);
        }
    }
    result
}

fn saturate(value: i64) -> i32 {
    if value >= i64::from(i32::MAX) {
        i32::MAX
    } else {
        value as i32
    }
}

// --- grid construction (ported) -------------------------------------------------------------

/// Java `PlanetGrids.Vec3`（对应游戏的 `Vec3`，全程 float）。
///
/// 分量都是 `f32`，逐运算保持 Java 的 float 语义：`x*x + y*y + z*z` 按 float 累加，`Math.sqrt`
/// 收到被隐式提升为 double 的参数并返回 double，再 `(float)` 窄化回来。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    /// Java `Vec3(float x, float y, float z)`。
    pub fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// Java `Vec3(Vec3 other)`。
    pub fn from_other(other: &Vec3) -> Vec3 {
        Vec3 {
            x: other.x,
            y: other.y,
            z: other.z,
        }
    }

    /// Java `set(Vec3 other)`（返回 this 以便链式调用）。
    pub fn set(&mut self, other: &Vec3) -> &mut Vec3 {
        self.x = other.x;
        self.y = other.y;
        self.z = other.z;
        self
    }

    /// Java `add(Vec3 other)`。
    pub fn add(&mut self, other: &Vec3) -> &mut Vec3 {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
        self
    }

    /// Java `nor()`。
    pub fn nor(&mut self) -> &mut Vec3 {
        let sum: f32 = self.x * self.x + self.y * self.y + self.z * self.z;
        let length = (sum as f64).sqrt() as f32;
        if length != 0.0 {
            self.x /= length;
            self.y /= length;
            self.z /= length;
        }
        self
    }
}

/// Java 里被共享的 `Tile` 引用（Java 的 `Tile` 对象在整张图中被同一实例引用）。
pub type TileRef = Rc<RefCell<Tile>>;
/// Java 里被共享的 `Corner` 引用。
pub type CornerRef = Rc<RefCell<Corner>>;

/// Java `PlanetGrids.Tile`。`tiles`/`corners` 的下标语义与 Java 一致；Java 的 `null` 槽对应 `None`。
pub struct Tile {
    pub id: i32,
    pub edge_count: i32,
    pub tiles: Vec<Option<TileRef>>,
    pub corners: Vec<Option<CornerRef>>,
    pub v: Vec3,
}

impl Tile {
    /// Java `Tile(int id, int edgeCount)`（Java 里是包内可见）。
    pub(crate) fn new(id: i32, edge_count: i32) -> Tile {
        Tile {
            id,
            edge_count,
            tiles: vec![None; edge_count as usize],
            corners: vec![None; edge_count as usize],
            v: Vec3::default(),
        }
    }
}

/// Java `PlanetGrids.Corner`。
pub struct Corner {
    pub id: i32,
    pub tiles: Vec<Option<TileRef>>,
    pub corners: Vec<Option<CornerRef>>,
    pub v: Vec3,
}

impl Corner {
    /// Java `Corner(int id)`（Java 里是包内可见）。
    pub(crate) fn new(id: i32) -> Corner {
        Corner {
            id,
            tiles: vec![None; 3],
            corners: vec![None; 3],
            v: Vec3::default(),
        }
    }
}

/// Java `PlanetGrids.Grid`。
pub struct Grid {
    pub size: i32,
    pub tiles: Vec<TileRef>,
    pub corners: Vec<CornerRef>,
}

impl Grid {
    /// Java `private Grid(int size)`。
    fn new(size: i32) -> Grid {
        let tiles: Vec<TileRef> = (0..tile_count(size) as usize)
            .map(|i| Rc::new(RefCell::new(Tile::new(i as i32, if i < 12 { 5 } else { 6 }))))
            .collect();
        let corners: Vec<CornerRef> = (0..corner_count(size) as usize)
            .map(|i| Rc::new(RefCell::new(Corner::new(i as i32))))
            .collect();
        Grid { size, tiles, corners }
    }
}

/// Java `create(int size)`。
pub fn create(size: i32) -> Grid {
    if size <= 0 {
        return initial_grid();
    }
    subdivided_grid(&create(size - 1))
}

/// Java `initialGrid()`：照抄 `PlanetGrid.java:33-99`。顶点、相邻面表、面心，再把每个角接到它的
/// 邻居。边簿记在这里对几何与下标都没有影响，但后续每一级网格都用得到它。
// 本函数与 subdivided_grid/pos/pos_corner 的下标循环是 Java 源码的逐字转写（文件头约定：
// 同样的细分顺序、同样的 tile 编号）。改成迭代器/clone_from_slice 虽行为等价，但会破坏与
// Java 行号的逐行对应；`_next_edge` 更是刻意保留的 Java nextEdge 簿记（见函数内注释）。
#[allow(clippy::needless_range_loop, clippy::manual_memcpy, clippy::explicit_counter_loop)]
fn initial_grid() -> Grid {
    let grid = Grid::new(0);

    for tile in &grid.tiles {
        let id = tile.borrow().id as usize;
        tile.borrow_mut().v = Vec3::new(I_TILES[id][0], I_TILES[id][1], I_TILES[id][2]);
        for k in 0..5 {
            let target = Rc::clone(&grid.tiles[I_TILES_P[id][k] as usize]);
            tile.borrow_mut().tiles[k] = Some(target);
        }
    }
    for i in 0..5 {
        add_corner(&grid, i, 0, I_TILES_P[0][(i + 4) % 5] as usize, I_TILES_P[0][i] as usize);
    }
    for i in 0..5 {
        add_corner(
            &grid,
            i + 5,
            3,
            I_TILES_P[3][(i + 4) % 5] as usize,
            I_TILES_P[3][i] as usize,
        );
    }
    add_corner(&grid, 10, 10, 1, 8);
    add_corner(&grid, 11, 1, 10, 6);
    add_corner(&grid, 12, 6, 10, 7);
    add_corner(&grid, 13, 6, 7, 11);
    add_corner(&grid, 14, 11, 7, 2);
    add_corner(&grid, 15, 11, 2, 9);
    add_corner(&grid, 16, 9, 2, 5);
    add_corner(&grid, 17, 9, 5, 4);
    add_corner(&grid, 18, 4, 5, 8);
    add_corner(&grid, 19, 4, 8, 1);

    for ci in 0..grid.corners.len() {
        let corner_cell = Rc::clone(&grid.corners[ci]);
        let mut next: Vec<Option<CornerRef>> = Vec::with_capacity(3);
        {
            let c = corner_cell.borrow();
            for k in 0..3 {
                let tile = c.tiles[k].as_ref().expect("unconnected grid corner").clone();
                let p = pos_corner(&tile, &grid.corners[ci]);
                let idx = ((p + 1) as usize) % 5;
                next.push(tile.borrow().corners[idx].clone());
            }
        }
        for k in 0..3 {
            corner_cell.borrow_mut().corners[k] = next[k].clone();
        }
    }

    // Java 在这里还维护 `int nextEdge = 0; ... nextEdge++`；本类没有任何读取点（Java 的注释同样
    // 说明边簿记对 tile 几何与下标都没有影响），故以 `_` 前缀保留这行计数。
    let mut _next_edge = 0;
    for tile in &grid.tiles {
        {
            let t = tile.borrow();
            for k in 0..5 {
                if t.tiles[k].is_none() {
                    panic!("unconnected grid tile {}", t.id);
                }
            }
        }
        _next_edge += 1;
    }

    grid
}

/// Java `subdividedGrid(Grid prev)`：照抄 `PlanetGrid.java:101-145`。tile 的排序不是可选的——
/// `addCorner` 依赖它决定 `id` 落在 `prev.tiles` 还是上一级的 `prev.corners`。
// 下标循环逐字对应 Java（理由见 initial_grid 上的登记）。
#[allow(clippy::needless_range_loop, clippy::manual_memcpy)]
fn subdivided_grid(prev: &Grid) -> Grid {
    let grid = Grid::new(prev.size + 1);

    let prev_tiles = prev.tiles.len();
    let prev_corners = prev.corners.len();

    for i in 0..prev_tiles {
        grid.tiles[i].borrow_mut().v = prev.tiles[i].borrow().v;
        let edges = grid.tiles[i].borrow().edge_count as usize;
        let mut links: Vec<usize> = Vec::with_capacity(edges);
        {
            let p = prev.tiles[i].borrow();
            for k in 0..edges {
                let corner = p.corners[k].as_ref().expect("unconnected grid tile corner");
                links.push(corner.borrow().id as usize + prev_tiles);
            }
        }
        for k in 0..edges {
            let target = Rc::clone(&grid.tiles[links[k]]);
            grid.tiles[i].borrow_mut().tiles[k] = Some(target);
        }
    }
    for i in 0..prev_corners {
        grid.tiles[i + prev_tiles].borrow_mut().v = prev.corners[i].borrow().v;
        let mut links: Vec<(usize, usize)> = Vec::with_capacity(3);
        {
            let c = prev.corners[i].borrow();
            for k in 0..3 {
                let corner = c.corners[k].as_ref().expect("unconnected grid corner");
                let tile = c.tiles[k].as_ref().expect("unconnected grid corner tile");
                links.push((
                    corner.borrow().id as usize + prev_tiles,
                    tile.borrow().id as usize,
                ));
            }
        }
        for (k, (corner_tile, prev_tile)) in links.iter().enumerate() {
            let first = Rc::clone(&grid.tiles[*corner_tile]);
            grid.tiles[i + prev_tiles].borrow_mut().tiles[2 * k] = Some(first);
            let second = Rc::clone(&grid.tiles[*prev_tile]);
            grid.tiles[i + prev_tiles].borrow_mut().tiles[2 * k + 1] = Some(second);
        }
    }

    let mut next_corner = 0_usize;
    for n in &prev.tiles {
        let t = &grid.tiles[n.borrow().id as usize];
        let edges = t.borrow().edge_count as usize;
        let mut links: Vec<(usize, usize, usize)> = Vec::with_capacity(edges);
        {
            let tile = t.borrow();
            for k in 0..edges {
                let a = tile.tiles[(k + edges - 1) % edges]
                    .as_ref()
                    .expect("unconnected grid tile")
                    .borrow()
                    .id;
                let b = tile.tiles[k]
                    .as_ref()
                    .expect("unconnected grid tile")
                    .borrow()
                    .id;
                links.push((tile.id as usize, a as usize, b as usize));
            }
        }
        for (id, a, b) in links {
            add_corner(&grid, next_corner, id, a, b);
            next_corner += 1;
        }
    }

    for ci in 0..grid.corners.len() {
        let corner_cell = Rc::clone(&grid.corners[ci]);
        let mut next: Vec<Option<CornerRef>> = Vec::with_capacity(3);
        {
            let c = corner_cell.borrow();
            for k in 0..3 {
                let tile = c.tiles[k].as_ref().expect("unconnected grid corner").clone();
                let p = pos_corner(&tile, &grid.corners[ci]);
                let idx = ((p + 1) as usize) % tile.borrow().edge_count as usize;
                next.push(tile.borrow().corners[idx].clone());
            }
        }
        for k in 0..3 {
            corner_cell.borrow_mut().corners[k] = next[k].clone();
        }
    }

    grid
}

/// Java `static void addCorner(int id, Grid grid, int t1, int t2, int t3)`（包内可见）。
pub(crate) fn add_corner(grid: &Grid, id: usize, t1: usize, t2: usize, t3: usize) {
    let c = &grid.corners[id];
    let t = [&grid.tiles[t1], &grid.tiles[t2], &grid.tiles[t3]];

    // Java: c.v.set(t[0].v).add(t[1].v).add(t[2].v).nor();
    let mut v = t[0].borrow().v;
    v.add(&t[1].borrow().v);
    v.add(&t[2].borrow().v);
    v.nor();
    c.borrow_mut().v.set(&v);

    for i in 0..3 {
        let p = pos(t[i], t[(i + 2) % 3]);
        t[i].borrow_mut().corners[p as usize] = Some(Rc::clone(c));
        c.borrow_mut().tiles[i] = Some(Rc::clone(t[i]));
    }
}

/// Java `static int pos(Tile t, Tile n)`：Java 的 `==` 是引用身份，这里用 `Rc::ptr_eq`。
// 下标循环逐字对应 Java 的 `for (int i = 0; i < edgeCount; i++)`（理由见 initial_grid 上的登记）。
#[allow(clippy::needless_range_loop)]
pub(crate) fn pos(t: &TileRef, n: &TileRef) -> i32 {
    let tile = t.borrow();
    for i in 0..tile.edge_count as usize {
        if let Some(other) = &tile.tiles[i]
            && Rc::ptr_eq(other, n)
        {
            return i as i32;
        }
    }
    -1
}

/// Java `static int pos(Tile t, Corner c)`（同名重载）。
// 下标循环逐字对应 Java（理由见 initial_grid 上的登记）。
#[allow(clippy::needless_range_loop)]
pub(crate) fn pos_corner(t: &TileRef, c: &CornerRef) -> i32 {
    let tile = t.borrow();
    for i in 0..tile.edge_count as usize {
        if let Some(other) = &tile.corners[i]
            && Rc::ptr_eq(other, c)
        {
            return i as i32;
        }
    }
    -1
}

// --- planet bookkeeping ---------------------------------------------------------------------

/// Java `capacity(int sectorSize)`：这个 `sectorSize` 的星球能寻址的扇区数——`10*3^size + 2` 个
/// 网格 tile，再加上游戏总是追加的那个退化槽。没有网格的星球（sectorSize = 0）只寻址一个退化扇区。
pub fn capacity(sector_size: i32) -> i32 {
    if sector_size <= 0 {
        return 1;
    }
    saturate(i64::from(tile_count(sector_size)) + 1)
}

/// Java `sectorPlanet(ContentRecord sector)`：sector 记录声明的星球，否则用游戏的默认值（serpulo）。
pub fn sector_planet(sector: &ContentRecord) -> String {
    let value = match sector.fields.get("planet") {
        Some(value) => value,
        None => return "serpulo".to_string(),
    };
    if !is_primitive(value) || value.get_as_string().chars().all(char::is_whitespace) {
        return "serpulo".to_string();
    }
    value.get_as_string()
}

/// Java `sectorIndex(ContentRecord sector, int capacity)`。
pub fn sector_index(sector: &ContentRecord, capacity: i32) -> i32 {
    let mut index = 0;
    if let Some(value) = sector.fields.get("sector")
        && is_primitive(value)
    {
        // Gson 的 getAsInt() 抛 NumberFormatException 时 Java 吞掉异常保持 0。
        index = as_int(value).unwrap_or(0);
    }
    // 游戏用的是普通 %，负的扇区号会得到负下标；floorMod 让预览保持有定义，同时不改变任何正下标。
    index.rem_euclid(capacity)
}

/// Java `sectorsByIndex(ModProject, String, int)`：把工程的 sector 记录按它们在命名星球网格上占据
/// 的下标分组。sector 命中当且仅当它的 `planet` 字段指向这个星球（裸名或带前缀名）；指向别处
/// （包括其他 mod 星球）的 sector 不属于这个星球的映射。
///
/// 返回切片的顺序就是 Java `LinkedHashMap` 的插入顺序，下标唯一。
pub fn sectors_by_index<'a>(
    project: &'a ModProject,
    planet_bare_name: &str,
    sector_size: i32,
) -> Vec<(i32, Vec<&'a ContentRecord>)> {
    let mut out: Vec<(i32, Vec<&'a ContentRecord>)> = Vec::new();
    let capacity = capacity(sector_size);
    for sector in &project.contents {
        if sector.ctype != ContentType::Sector || sector.is_inline() {
            continue;
        }
        let target = sector_planet(sector);
        let internal = project.internal_name();
        let prefix = format!("{internal}-");
        let bare = if target.starts_with(&prefix) {
            &target[prefix.len()..]
        } else {
            target.as_str()
        };
        if bare != planet_bare_name {
            continue;
        }
        let index = sector_index(sector, capacity);
        match out.iter_mut().find(|(key, _)| *key == index) {
            Some((_, list)) => list.push(sector),
            None => out.push((index, vec![sector])),
        }
    }
    out
}

/// Java `intField(ContentRecord record, String field, int fallback)`。
pub fn int_field(record: &ContentRecord, field: &str, fallback: i32) -> i32 {
    let value = match record.fields.get(field) {
        Some(value) => value,
        None => return fallback,
    };
    if !is_primitive(value) {
        return fallback;
    }
    as_int(value).unwrap_or(fallback)
}

/// 名字是否指向一个有战役网格的已知 vanilla 星球。
pub fn is_vanilla_planet(name: &str) -> bool {
    vanilla_size(&name.to_lowercase()).is_some()
}

/// Java `vanillaPlanetNames()`：仅为测试保留的严格性 helper（Java 返回 `LinkedHashSet`）。
pub fn vanilla_planet_names() -> Vec<String> {
    VANILLA_SIZES
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect()
}

/// Gson `JsonElement.isJsonPrimitive()`：`null` 与 `JsonNull` 都不是 primitive。
fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Bool(_) | Json::Num(_) | Json::Str(_))
}

/// Gson `JsonPrimitive.getAsInt()`：数字原语走 `Number.intValue()`（向零截断），字符串原语走
/// `Integer.parseInt`（非法即抛 NumberFormatException，由调用方吞掉）。布尔原语在 Java 里抛的是
/// UnsupportedOperationException，这里同样按“读不到”处理。
fn as_int(value: &Json) -> Option<i32> {
    match value {
        Json::Num(text) => text.parse::<f64>().ok().map(|v| v as i32),
        Json::Str(text) => text.parse::<i32>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! 数值向量移植自 Java `PlanetGridsTest`：tile/corner/edge 计数、单位球、角点共享、
    //! 确定性与 `sectorsByIndex` 的分组口径。

    use super::*;
    use mm_json::Json;
    use mm_model::ContentRecord;

    use crate::ContentType;

    fn length(v: &Vec3) -> f64 {
        (f64::from(v.x) * f64::from(v.x)
            + f64::from(v.y) * f64::from(v.y)
            + f64::from(v.z) * f64::from(v.z))
        .sqrt()
    }

    #[test]
    fn counts_follow_the_geodesic_formulas() {
        let expected: [[i32; 3]; 4] = [
            [12, 20, 30],   // size 0: icosahedron
            [32, 60, 90],   // size 1
            [92, 180, 270], // size 2: erekir
            [272, 540, 810],// size 3: serpulo
        ];
        for (size, row) in expected.iter().enumerate() {
            let grid = create(size as i32);
            assert_eq!(row[0] as usize, grid.tiles.len(), "tiles at size {size}");
            assert_eq!(row[1] as usize, grid.corners.len(), "corners at size {size}");
            assert_eq!(row[2], edge_count(size as i32), "edges at size {size}");
            // 球面的欧拉示性数：V - E + F = 2
            assert_eq!(
                2,
                grid.tiles.len() as i32 - edge_count(size as i32) + grid.corners.len() as i32
            );
        }
    }

    #[test]
    fn all_vectors_are_unit_length() {
        let grid = create(3);
        for tile in &grid.tiles {
            let tile = tile.borrow();
            assert!((length(&tile.v) - 1.0).abs() < 1e-4, "tile {} not normalized", tile.id);
            for corner in &tile.corners {
                let corner = corner.as_ref().expect("assigned corner").borrow();
                assert!((length(&corner.v) - 1.0).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn every_corner_is_shared_by_exactly_three_tiles() {
        let grid = create(2);
        let mut uses: Vec<i32> = vec![0; grid.corners.len()];
        for tile in &grid.tiles {
            let tile = tile.borrow();
            assert_eq!(tile.edge_count as usize, tile.corners.len(), "tile {}", tile.id);
            for corner in &tile.corners {
                let corner = corner.as_ref().expect("assigned corner").borrow();
                uses[corner.id as usize] += 1;
            }
        }
        for (index, count) in uses.iter().enumerate() {
            assert_eq!(3, *count, "corner {index} use count");
        }
    }

    #[test]
    fn construction_is_deterministic() {
        let a = create(3);
        let b = create(3);
        for i in 0..a.tiles.len() {
            let left = a.tiles[i].borrow();
            let right = b.tiles[i].borrow();
            assert_eq!(left.v.x, right.v.x);
            assert_eq!(left.v.y, right.v.y);
            assert_eq!(left.v.z, right.v.z);
        }
    }

    #[test]
    fn capacity_matches_the_games_sector_list() {
        assert_eq!(1, capacity(0), "gridless planets have only the degenerate slot");
        assert_eq!(93, capacity(2), "erekir-style grid: 92 tiles + 1 slot");
        assert_eq!(273, capacity(3), "serpulo-style grid: 272 tiles + 1 slot");
        assert_eq!(93, capacity(vanilla_size("erekir").expect("erekir 是 vanilla 星球")));
    }

    fn sector(name: &str, planet: Option<&str>, index: Option<i32>) -> ContentRecord {
        let mut record = ContentRecord::create(name, ContentType::Sector, name);
        if let Some(planet) = planet {
            record.fields.insert("planet", Json::str(planet));
        }
        if let Some(index) = index {
            record.fields.insert("sector", Json::num(index.to_string()));
        }
        record
    }

    #[test]
    fn sector_presets_group_by_planet_and_index() {
        let mut project = mm_model::ModProject::default();
        project.meta.name = "My Mod".to_string();
        let mut planet = ContentRecord::create("p", ContentType::Planet, "homeworld");
        planet.fields.insert("sectorSize", Json::num("2"));
        project.contents.push(planet);

        project.contents.push(sector("bare-sector", Some("homeworld"), Some(5)));
        project.contents.push(sector("prefixed-sector", Some("my-mod-homeworld"), Some(40)));
        project.contents.push(sector("negative-sector", Some("homeworld"), Some(-3)));
        project.contents.push(sector("elsewhere", Some("serpulo"), Some(7)));
        project.contents.push(sector("orphan", None, Some(9)));

        let map = sectors_by_index(&project, "homeworld", 2);
        assert_eq!(3, map.len(), "{map:?}");
        assert_eq!("bare-sector", map.iter().find(|(index, _)| *index == 5).expect("5").1[0].name);
        assert_eq!(
            "prefixed-sector",
            map.iter().find(|(index, _)| *index == 40).expect("40").1[0].name
        );
        // floorMod: -3 mod 93 = 90
        assert_eq!(
            "negative-sector",
            map.iter().find(|(index, _)| *index == 90).expect("90").1[0].name
        );
        assert!(map
            .iter()
            .flat_map(|(_, records)| records)
            .all(|record| record.name != "elsewhere" && record.name != "orphan"));
    }
}
