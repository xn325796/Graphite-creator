//! `dev.modmaker.core.noise.SimplexNoise` 的等价移植。
//!
//! Java 侧位于 `core\noise\SimplexNoise.java`（本批属于星球格式规则，故落在 `planet\noise.rs`）。
//! 它是 `arc.util.noise.Simplex` 的忠实移植（只取星球网格用到的部分：3D 多倍频噪声），逐字照抄，
//! 目的是让预览在参数相同时渲染出与游戏一致的地形——星球网格调用的是
//! `Simplex.noise3d(7 + seed, octaves, persistence, scale, 5 + x, 5 + y, 5 + z)`。
//!
//! 位级一致性要点：
//! - Java `noise3d` 的形参是 `double`（`octaves`/`persistence`/`scale`/坐标），返回 `float`；
//!   调用方传入的 `float` 实参（如 `0.5f`、`5f + position.x`）先在 `f32` 上算完再精确提升为
//!   `f64`，所以这里参数仍是 `f64`，而窄化点与 Java 逐一对应。
//! - `perm` 是 Java 的 `int` 位运算（`>>>` 为零填充右移、乘法按 32 位回绕），用
//!   `u32` 移位 + `wrapping_mul`/`wrapping_add` 复刻。
//! - `fastfloor` 对应 `(int) x`（向零截断）再减一，Java 对 `NaN`/越界的饱和语义与 Rust 的
//!   `as i32` 一致。

/// Java `private static final int[][] GRAD3`：梯度表，元素顺序即 Java 字面顺序。
const GRAD3: [[i32; 3]; 12] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
];

/// Java `noise3d(int seed, double octaves, double persistence, double scale, double x, double y,
/// double z)`：与 arc 的 `Simplex.noise3d` 同语义的 3D 多倍频 simplex 噪声。
pub fn noise3d(seed: i32, octaves: f64, persistence: f64, scale: f64, x: f64, y: f64, z: f64) -> f32 {
    let mut total: f64 = 0.0;
    let mut frequency: f64 = scale;
    let mut amplitude: f64 = 1.0;
    let mut max_amplitude: f64 = 0.0;

    // Java 的循环变量是 `int i`，与 `double octaves` 比较：i 先提升为 double。
    let mut i: i32 = 0;
    while f64::from(i) < octaves {
        total += (raw3d(seed, x * frequency, y * frequency, z * frequency) + 1.0) / 2.0 * amplitude;

        frequency *= 2.0;
        max_amplitude += amplitude;
        amplitude *= persistence;
        i += 1;
    }

    (total / max_amplitude) as f32
}

/// Java `raw3d(int seed, double x, double y, double z)`：逐字照抄 arc 的 `Simplex.raw3d`。
pub fn raw3d(seed: i32, x: f64, y: f64, z: f64) -> f64 {
    let f3: f64 = 1.0 / 3.0;
    let s: f64 = (x + y + z) * f3;
    let i: i32 = fastfloor(x + s);
    let j: i32 = fastfloor(y + s);
    let k: i32 = fastfloor(z + s);

    let g3: f64 = 1.0 / 6.0;
    // Java：`(i + j + k) * g3`——int 相加（Java 的回绕语义）后再提升为 double。
    let t: f64 = f64::from(i.wrapping_add(j).wrapping_add(k)) * g3;
    let x0: f64 = x - (f64::from(i) - t);
    let y0: f64 = y - (f64::from(j) - t);
    let z0: f64 = z - (f64::from(k) - t);

    let i1: i32;
    let j1: i32;
    let k1: i32;
    let i2: i32;
    let j2: i32;
    let k2: i32;

    if x0 >= y0 {
        if y0 >= z0 {
            i1 = 1;
            j1 = 0;
            k1 = 0;
            i2 = 1;
            j2 = 1;
            k2 = 0;
        } else if x0 >= z0 {
            i1 = 1;
            j1 = 0;
            k1 = 0;
            i2 = 1;
            j2 = 0;
            k2 = 1;
        } else {
            i1 = 0;
            j1 = 0;
            k1 = 1;
            i2 = 1;
            j2 = 0;
            k2 = 1;
        }
    } else if y0 < z0 {
        i1 = 0;
        j1 = 0;
        k1 = 1;
        i2 = 0;
        j2 = 1;
        k2 = 1;
    } else if x0 < z0 {
        i1 = 0;
        j1 = 1;
        k1 = 0;
        i2 = 0;
        j2 = 1;
        k2 = 1;
    } else {
        i1 = 0;
        j1 = 1;
        k1 = 0;
        i2 = 1;
        j2 = 1;
        k2 = 0;
    }

    // Java：`x0 - i1 + g3` 里 int 项提升为 double。
    let x1: f64 = x0 - f64::from(i1) + g3;
    let y1: f64 = y0 - f64::from(j1) + g3;
    let z1: f64 = z0 - f64::from(k1) + g3;
    let x2: f64 = x0 - f64::from(i2) + 2.0 * g3;
    let y2: f64 = y0 - f64::from(j2) + 2.0 * g3;
    let z2: f64 = z0 - f64::from(k2) + 2.0 * g3;
    let x3: f64 = x0 - 1.0 + 3.0 * g3;
    let y3: f64 = y0 - 1.0 + 3.0 * g3;
    let z3: f64 = z0 - 1.0 + 3.0 * g3;

    let ii: i32 = i & 255;
    let jj: i32 = j & 255;
    let kk: i32 = k & 255;
    let gi0: i32 = perm(seed, ii + perm(seed, jj + perm(seed, kk))) % 12;
    let gi1: i32 = perm(seed, ii + i1 + perm(seed, jj + j1 + perm(seed, kk + k1))) % 12;
    let gi2: i32 = perm(seed, ii + i2 + perm(seed, jj + j2 + perm(seed, kk + k2))) % 12;
    let gi3: i32 = perm(seed, ii + 1 + perm(seed, jj + 1 + perm(seed, kk + 1))) % 12;

    let mut t0: f64 = 0.6 - x0 * x0 - y0 * y0 - z0 * z0;
    let n0: f64 = if t0 < 0.0 {
        0.0
    } else {
        t0 *= t0;
        t0 * t0 * dot(&GRAD3[gi0 as usize], x0, y0, z0)
    };

    let mut t1: f64 = 0.6 - x1 * x1 - y1 * y1 - z1 * z1;
    let n1: f64 = if t1 < 0.0 {
        0.0
    } else {
        t1 *= t1;
        t1 * t1 * dot(&GRAD3[gi1 as usize], x1, y1, z1)
    };

    let mut t2: f64 = 0.6 - x2 * x2 - y2 * y2 - z2 * z2;
    let n2: f64 = if t2 < 0.0 {
        0.0
    } else {
        t2 *= t2;
        t2 * t2 * dot(&GRAD3[gi2 as usize], x2, y2, z2)
    };

    let mut t3: f64 = 0.6 - x3 * x3 - y3 * y3 - z3 * z3;
    let n3: f64 = if t3 < 0.0 {
        0.0
    } else {
        t3 *= t3;
        t3 * t3 * dot(&GRAD3[gi3 as usize], x3, y3, z3)
    };

    32.0 * (n0 + n1 + n2 + n3)
}

/// Java `perm(int seed, int x)`（包内可见）：arc 的带种子哈希——`seed`（任意）+ `x`（掩码到
/// 0-255）→ 0-255。Java 全程 `int` 算术（乘法回绕、`>>>` 零填充右移），这里逐运算对应。
pub(crate) fn perm(seed: i32, x: i32) -> i32 {
    let mut x: i32 = (x & 255).wrapping_mul(0x45d9f3b_i32);
    x = ((x as u32 >> 16) as i32 ^ x).wrapping_mul(0x45d9f3b_i32.wrapping_add(seed));
    x = (x as u32 >> 16) as i32 ^ x;
    x & 0xff
}

/// Java `fastfloor(double x)`（包内可见）：`(int) x` 向零截断，非正数再减一（Java 的 `- 1` 是
/// 会绕回的 `int` 算术）。
pub(crate) fn fastfloor(x: f64) -> i32 {
    if x > 0.0 {
        x as i32
    } else {
        (x as i32).wrapping_sub(1)
    }
}

/// Java `dot(int[] g, double x, double y, double z)`（包内可见）：int 分量提升为 double 后相乘。
pub(crate) fn dot(g: &[i32; 3], x: f64, y: f64, z: f64) -> f64 {
    f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
}
