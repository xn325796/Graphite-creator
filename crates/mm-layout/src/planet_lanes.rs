//! Java `dev.modmaker.ui.PlanetLanes`（114 行）的等价移植：科技树「行星泳道」的**纯数学**——
//! 按 `research.planet` 把记录节点归组为一行星一个包围盒、对落点做命中测试、给行星键挑一个稳定颜色。
//!
//! 类注释（`PlanetLanes.java:9-17`）直译：没有任何 JavaFX 状态，所以这套数学能在普通 JVM 上单测
//! （与 [`crate::minimap_math`] 同一个切分）。颜色写成内联值而不是 CSS 类，因为行星数量无上界——
//! 与批量 B 的卡片头调 `ContentType.color()` 是同一个理由。
//!
//! ## 落点理由
//!
//! Java 把它放在 `dev.modmaker.ui` 包里，但它是**零依赖纯计算**，与 `ui/EdgePathMath.java`、
//! `ui/MinimapMath.java` 同类，因此按 P5 计划 §三 已签字的那条口径落在 `mm-layout`（本 crate
//! `[dependencies]` 为空，「几何只在 Rust 侧离线测、前端只画不算」由依赖层强制）。
//! 本模块内部一律 `pub` 而非 `pub(crate)`，理由与 [`crate::edge_path_math`] 顶部登记的同一条：
//! Java 侧是 `package-private`，Rust 侧若对应私有可见性，这些函数在非 test 构建里只被
//! `#[cfg(test)]`/集成测试引用就会报 `dead_code`，而签字口径要求 0 警告、`allow(` 又是禁词。
//!
//! ## 与 Java 的四处结构性偏离（逐字登记，不记暗账）
//!
//! 1. **`LinkedHashMap` → 保序去重的 `Vec`**。本 crate 零依赖，拿不到 `mm-schema::KeyedMap`。
//!    语义等价：仍是「首次出现的顺序」，`boxes` 的键重复命中时原地扩盒、位置不变（Java `:72-80`）。
//! 2. **Java 的 `record` 访问器 → Rust 字段**。`Lane.planet()`/`minX()` 等（`:24`）写成同名字段
//!    `planet`/`min_x`，Java 的 `double` 一律是 `f64`。
//! 3. **`laneAt` 返回 `null` → 返回 `Option<Lane>`**（`:96-107`）。Java 的 `Lane` 是不可变 record，
//!    Rust 侧靠 `Clone` 交出副本；命中判定与「最小盒优先、同面积取靠前者」逐句照抄。
//! 4. **`String.hashCode` 必须逐位复刻**。Java `colorFor`（`:110-113`）取
//!    `PALETTE[Math.floorMod(key.hashCode(), PALETTE.length)]`，而类注释（`:44-48`）明确说这份稳定性
//!    靠的是「`String.hashCode` 有规范、`Object.hashCode` 没有」。Rust 的 `DefaultHasher` 不承诺跨版本
//!    稳定，所以 [`java_hash_code`] 按规范实现（`h = 31*h + c`，遍历 **UTF-16 code unit**、`i32` 溢出回绕）
//!    复刻它，`floorMod` 用 `rem_euclid`（模数为正时二者同值）。颜色字符串返回值取 `&'static str`：
//!    Java 的 `String` 可空、而 `:112` 那条下标运算永不为 null，Rust 侧把这件事交给类型表达。

/// Java `PlanetLanes.Member`（`:21`）：一个节点对泳道的贡献——它的行星键与世界坐标中心。
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// Java 的这个分量可空（用例见 `PlanetLanesTest.java:37` 的 `new Member(null, 0, 0)`），
    /// 故 `Option<String>`。
    pub planet: Option<String>,
    pub x: f64,
    pub y: f64,
}

impl Member {
    /// 测试与调用方的构造器，对应 Java 的 `new PlanetLanes.Member(planet, x, y)`。
    pub fn new(planet: Option<&str>, x: f64, y: f64) -> Self {
        Self { planet: planet.map(|value| value.to_string()), x, y }
    }
}

/// Java `PlanetLanes.Lane`（`:24-42`）：一个行星的泳道盒，世界坐标，**已含外扩**。
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub planet: String,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Lane {
    /// 对应 Java 的 `new PlanetLanes.Lane(planet, minX, minY, maxX, maxY)`（`Lane` record 在
    /// `PlanetLanes.java:24`；测试里直接构造见 `PlanetLanesTest.java:56-57`）。
    pub fn new(planet: &str, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self { planet: planet.to_string(), min_x, min_y, max_x, max_y }
    }

    /// Java `Lane.width()`（`:26-28`）。
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Java `Lane.height()`（`:30-32`）。
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Java `Lane.contains(double, double)`（`:34-37`）：**四条边全含**——正好落在加粗边框上的
    /// 那一次拖放仍算盒内。
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Java `Lane.area()`（`:39-41`，私有）。
    fn area(&self) -> f64 {
        self.width().max(0.0) * self.height().max(0.0)
    }
}

/// Java `PlanetLanes.PALETTE`（`:49-51`）：固定的深色主题友好调色板。
/// 元素顺序可观察（它决定 `colorFor` 的结果），原样照搬。
const PALETTE: [&str; 10] = [
    "#5B8DEF", "#E06C75", "#98C379", "#D19A66", "#C678DD", "#56B6C2", "#E5C07B", "#7FAEE5",
    "#B58FDB", "#6FBF8F",
];

/// Java `PlanetLanes.build(List<Member>, double half, double pad)`（`:61-89`）：
/// `members` 里每个不同的行星一个**外扩后的盒**，按首次出现顺序。没有行星键的成员
/// （null/全空白）不进任何泳道；空输入得到空列表。每个成员的节点方块（中心 ± `half`）
/// 四边各再长出 `pad`。
pub fn build(members: &[Member], half: f64, pad: f64) -> Vec<Lane> {
    // Java `Map<String, double[]> boxes = new LinkedHashMap<>()`（`:62`）——本 crate 零依赖，
    // 用「Vec + 首次出现顺序 + 按键原地更新」承接同一份语义（见模块头差异第 1 条）。
    let mut boxes: Vec<(String, [f64; 4])> = Vec::new();
    for member in members {
        // Java `planet == null || planet.isBlank()`（`:65`）：`isBlank()` 是「全为空白字符」，
        // 含空串；Rust 侧 `trim().is_empty()` 同样按 Unicode 空白判定。
        let blank = match member.planet.as_deref() {
            None => true,
            Some(value) => value.trim().is_empty(),
        };
        if blank {
            continue;
        }
        let planet = member.planet.clone().unwrap_or_default();
        let min_x = member.x - half;
        let min_y = member.y - half;
        let max_x = member.x + half;
        let max_y = member.y + half;
        match boxes.iter_mut().find(|(key, _)| *key == planet) {
            // Java `box == null` 分支（`:73-74`）：首次见即整盒写入，位置留在追加处。
            None => boxes.push((planet, [min_x, min_y, max_x, max_y])),
            // Java `:75-80`：原地把盒撑大到并集，**不移动位置**。
            Some((_, box_)) => {
                box_[0] = box_[0].min(min_x);
                box_[1] = box_[1].min(min_y);
                box_[2] = box_[2].max(max_x);
                box_[3] = box_[3].max(max_y);
            }
        }
    }
    let mut lanes: Vec<Lane> = Vec::new();
    for (planet, box_) in boxes {
        lanes.push(Lane {
            planet,
            min_x: box_[0] - pad,
            min_y: box_[1] - pad,
            max_x: box_[2] + pad,
            max_y: box_[3] + pad,
        });
    }
    lanes
}

/// Java `PlanetLanes.laneAt(List<Lane>, double x, double y)`（`:96-107`）：落点落进的那条泳道，
/// 空白处为 `None`。两条泳道重叠且都含这一点时**最小的那条赢**——盒更紧（更具体）的那一组才是用户
/// 想丢进去的地方；面积相等时取靠前的那条，所以答案是确定的、与传入顺序里「谁先」只在同面积时有关。
pub fn lane_at(lanes: &[Lane], x: f64, y: f64) -> Option<Lane> {
    let mut best: Option<&Lane> = None;
    for lane in lanes {
        if !lane.contains(x, y) {
            continue;
        }
        // Java `best == null || lane.area() < best.area()`（`:102`）：严格小于才换人，
        // 所以面积相等时先到先得。
        match best {
            None => best = Some(lane),
            Some(current) if lane.area() < current.area() => best = Some(lane),
            Some(_) => {}
        }
    }
    best.cloned()
}

/// Java `PlanetLanes.colorFor(String planet)`（`:110-113`）：一个行星键的泳道颜色——跨渲染稳定、
/// 大小写无关、永不为 null。
pub fn color_for(planet: Option<&str>) -> &'static str {
    // Java `planet == null ? "" : planet.toLowerCase(Locale.ROOT)`（`:111`）：`Locale.ROOT` 要的是
    // 与区域无关的小写化，Rust 的 `str::to_lowercase` 同样不读任何环境区域。
    let key = planet.unwrap_or("").to_lowercase();
    PALETTE[java_hash_code(&key).rem_euclid(PALETTE.len() as i32) as usize]
}

/// Java `String.hashCode()` 的复刻——见模块头差异第 4 条。规范是 `s[0]*31^(n-1) + … + s[n-1]`
/// 以 32 位有符号整数回绕计算，逐字符即**UTF-16 code unit**（不是 Rust 的 `char`）。
fn java_hash_code(text: &str) -> i32 {
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    hash
}
