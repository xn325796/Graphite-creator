//! `ui/EdgePathMath.java`（159 行）的等价移植：画布连线三种路由形状的纯计算。
//!
//! Java 类注释(:7-15)把它成立的理由写得很清楚：这些计算**不含任何 JavaFX 状态**，所以能在
//! 纯 JVM 上单测。Rust 侧同理落在零依赖的 `mm-layout`（P5 计划 §三：几何不进 `mm-app`、
//! 更不进前端 TS——「前端只画不算」）。
//!
//! Java 是 `package-private final class`，测试靠同包访问。Rust 侧**没有等价的可见性**：
//! Java 的 `dev.modmaker.ui` 包 ≠ Rust 的 crate，这里要用 `pub`，理由有两条，登记为对已签字
//! 口径（「Rust 侧对应 `pub(crate)`」）的一处更正——
//! ① `pub(crate)` 的几何函数只被本 crate 的 `#[cfg(test)]` 用到，非 test 构建即报 `dead_code`，
//!    而 M1 的验收门是 0 警告，且 `allow(` 是禁词、压不住；
//!    （`cargo test` 会同时编 lib 的非 test 目标，所以这门必然破。）
//! ② M2/M3 的画板快照要跨 crate 调这些公式，`pub` 是它们迟早要还的。
//! 分层保证不靠可见性，靠依赖层：`mm-layout` 的 `Cargo.toml` 零依赖，前端拿不到它。
//!
//! 所有点都是**世界坐标**。waypoint 列表是不可信输入（来自用户可编辑的 `.canvas` 文件），
//! 所以每个入口都要容忍 `null` 与长度不足的项（Java :12-14/:151-163）。

/// 对应 Java `List<double[]> reroutes`：外层可为 `null`、元素可为 `null`、数组可短于 2。
/// `[f64; 2]` 表达不了后两种畸形，所以入参保留这个形状，把 Java 的容错面原样搬过来；
/// 经过 [`via_points`] 过滤后才是干净的 `[[f64; 2]]`。
pub type RawReroutes<'a> = Option<&'a [Option<Vec<f64>>]>;

/// 对应 Java `EdgePathMath.Style`(:21-48)：工作区工具栏循环的三种路由样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bezier,
    Orthogonal,
    Straight,
}

impl Style {
    /// 对应 Java `Style.key()`(:26-28)：存进项目 sidecar 的小写键
    /// （Java 用 `name().toLowerCase(Locale.ROOT)`）。
    pub fn key(self) -> &'static str {
        match self {
            Style::Bezier => "bezier",
            Style::Orthogonal => "orthogonal",
            Style::Straight => "straight",
        }
    }

    /// 对应 Java `Style.parse(String)`(:30-42)：解析 sidecar 值，`null`、空白、未知值一律
    /// 回落到历史默认的 bezier 形状（Java `valueOf(trim().toUpperCase(Locale.ROOT))` 抛
    /// `IllegalArgumentException` 时 catch 回落）。
    pub fn parse(value: Option<&str>) -> Style {
        let Some(value) = value else {
            return Style::Bezier;
        };
        // 这三枚举名全是 ASCII，`to_ascii_uppercase` 与 `Locale.ROOT` 的大写化在这三个值上等价。
        match value.trim().to_ascii_uppercase().as_str() {
            "BEZIER" => Style::Bezier,
            "ORTHOGONAL" => Style::Orthogonal,
            "STRAIGHT" => Style::Straight,
            _ => Style::Bezier,
        }
    }

    /// 对应 Java `Style.next()`(:44-48)：工具栏循环里的下一个样式。
    pub fn next(self) -> Style {
        match self {
            Style::Bezier => Style::Orthogonal,
            Style::Orthogonal => Style::Straight,
            Style::Straight => Style::Bezier,
        }
    }
}

/// 对应 Java `viaPoints(List<double[]>)`(:53-64)：可用 waypoint 的有序列表——`null` 与畸形项
/// 全部丢掉。下游（路径生成、箭头角度、命中测试）都只经这个过滤器读边的 reroutes。
pub fn via_points(reroutes: RawReroutes<'_>) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    if let Some(reroutes) = reroutes {
        for point in reroutes {
            if let Some(point) = point
                && point.len() >= 2
            {
                out.push([point[0], point[1]]);
            }
        }
    }
    out
}

/// 对应 Java `bezierSegment(double, double, double, double)`(:66-74)：一段 bezier 的控制点——
/// 两端都是水平切线、交汇于该段的中点 x，也就是画布一直在画的 S 形。
/// 返回 `{x1, y1, cx1, cy1, cx2, cy2, x2, y2}`。
pub fn bezier_segment(x1: f64, y1: f64, x2: f64, y2: f64) -> [f64; 8] {
    let mid_x = x1 + (x2 - x1) * 0.5;
    [x1, y1, mid_x, y1, mid_x, y2, x2, y2]
}

/// 对应 Java `straightPoints(...)`(:76-85)：直线折线 = 起点、按序的每个 waypoint、终点。
pub fn straight_points(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    reroutes: RawReroutes<'_>,
) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    add_point(&mut points, x1, y1);
    for point in via_points(reroutes) {
        add_point(&mut points, point[0], point[1]);
    }
    add_point(&mut points, x2, y2);
    points
}

/// 对应 Java `orthogonalPoints(...)`(:87-107)：正交折线——每段都折向自己的中点 x，于是路径
/// 轴对齐（横、竖、横），进出端口的方式与 bezier 一致。相邻的重复点会被折叠，
/// 所以已经落在直线段上的 waypoint 不会添出零长度段。
pub fn orthogonal_points(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    reroutes: RawReroutes<'_>,
) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    add_point(&mut points, x1, y1);
    let mut prev_x = x1;
    let mut prev_y = y1;
    for point in via_points(reroutes) {
        append_bend(&mut points, prev_x, prev_y, point[0], point[1]);
        add_point(&mut points, point[0], point[1]);
        prev_x = point[0];
        prev_y = point[1];
    }
    append_bend(&mut points, prev_x, prev_y, x2, y2);
    add_point(&mut points, x2, y2);
    points
}

/// 对应 Java 私有 `appendBend(...)`(:109-118)：一段的两个中点 x 折点；该段已经轴对齐时什么都不加。
fn append_bend(points: &mut Vec<[f64; 2]>, x1: f64, y1: f64, x2: f64, y2: f64) {
    if x1 == x2 || y1 == y2 {
        return;
    }
    let mid_x = x1 + (x2 - x1) * 0.5;
    add_point(points, mid_x, y1);
    add_point(points, mid_x, y2);
}

/// 对应 Java 私有 `addPoint(...)`(:120-128)：与上一点完全重合时不加。
fn add_point(points: &mut Vec<[f64; 2]>, x: f64, y: f64) {
    if let Some(last) = points.last()
        && last[0] == x && last[1] == y
    {
        return;
    }
    points.push([x, y]);
}

/// 对应 Java `arrowAngle(...)`(:130-157)：路径抵达目标端口时的行进角度（弧度），给箭头用。
/// bezier 的最后一段控制点落在终点 y 上（位于该段中点 x），于是箭头是水平进入端口的；
/// 两种折线则沿最后一段的方向。
pub fn arrow_angle(
    style: Style,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    reroutes: RawReroutes<'_>,
) -> f64 {
    let via = via_points(reroutes);
    if style == Style::Bezier {
        let mut prev_x = x1;
        for point in &via {
            prev_x = point[0];
        }
        let mid_x = prev_x + (x2 - prev_x) * 0.5;
        // Java :145 注释：最后一个控制点与终点同 y，所以进入端口的切线是水平的。
        return (0.0f64).atan2(x2 - mid_x);
    }
    let points = match style {
        Style::Orthogonal => orthogonal_points(x1, y1, x2, y2, reroutes),
        _ => straight_points(x1, y1, x2, y2, reroutes),
    };
    if points.len() >= 2 {
        let before = points[points.len() - 2];
        return (y2 - before[1]).atan2(x2 - before[0]);
    }
    (y2 - y1).atan2(x2 - x1)
}

/// 对应 Java `hitReroute(...)`(:159-173)：世界点之下的 waypoint 下标。
/// Java 用 `-1` 表示「半径内没有」，Rust 侧对应 `None`；下标是**过滤后** `via` 里的下标
/// （Java :166 `best = i` 同样是 `via` 的下标，不是原始列表的）。
pub fn hit_reroute(
    world_x: f64,
    world_y: f64,
    reroutes: RawReroutes<'_>,
    radius: f64,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut best_distance = radius;
    let via = via_points(reroutes);
    for (i, point) in via.iter().enumerate() {
        let distance = (point[0] - world_x).hypot(point[1] - world_y);
        if distance <= best_distance {
            best_distance = distance;
            best = Some(i);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    /// 对应 Java `List<double[]>` 字面量夹具：`{x, y}` → `Some(vec![x, y])`。
    fn pt(x: f64, y: f64) -> Option<Vec<f64>> {
        Some(vec![x, y])
    }

    /// 对应 Java `EdgePathMathTest.flatten(...)`(:118-125)：把点列表摊平成 x0,y0,x1,y1,…
    fn flatten(points: &[[f64; 2]]) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::new();
        for point in points {
            out.push(point[0]);
            out.push(point[1]);
        }
        out
    }

    /// 对应 Java `assertEquals(expected, actual, delta)`。
    fn assert_close(actual: f64, expected: f64, delta: f64, what: &str) {
        assert!((actual - expected).abs() <= delta, "{what}: 实际 {actual}，期望 {expected}");
    }

    /// 对应 Java `EdgePathMathTest.java:19-29`
    /// `styleParsesSidecarKeysAndCyclesThroughTheToolbarOrder`。
    #[test]
    fn style_parses_sidecar_keys_and_cycles_through_the_toolbar_order() {
        assert_eq!(Style::parse(None), Style::Bezier); // :20
        assert_eq!(Style::parse(Some("bezier")), Style::Bezier); // :21
        assert_eq!(Style::parse(Some("ORTHOGONAL")), Style::Orthogonal); // :22
        assert_eq!(Style::parse(Some(" straight ")), Style::Straight); // :23
        assert_eq!(Style::parse(Some("spaghetti")), Style::Bezier); // :24
        assert_eq!(Style::Orthogonal.key(), "orthogonal"); // :25
        assert_eq!(Style::Bezier.next(), Style::Orthogonal); // :26
        assert_eq!(Style::Orthogonal.next(), Style::Straight); // :27
        assert_eq!(Style::Straight.next(), Style::Bezier); // :28
    }

    /// 对应 Java `EdgePathMathTest.java:32-39`
    /// `viaPointsDropsMalformedWaypointsFromUserEditedFiles`。
    #[test]
    fn via_points_drops_malformed_waypoints_from_user_edited_files() {
        assert_eq!(via_points(None).len(), 0); // :33
        let raw: Vec<Option<Vec<f64>>> = vec![None, Some(vec![5.0]), Some(vec![1.0, 2.0])]; // :34-37
        assert_eq!(via_points(Some(raw.as_slice())).len(), 1); // :38
    }

    /// 对应 Java `EdgePathMathTest.java:42-52`
    /// `bezierSegmentUsesTheHistoricHorizontalTangentShape`。
    #[test]
    fn bezier_segment_uses_the_historic_horizontal_tangent_shape() {
        let segment = bezier_segment(0.0, 10.0, 100.0, 30.0);
        assert_close(segment[0], 0.0, EPS, "segment[0]");
        assert_close(segment[1], 10.0, EPS, "segment[1]");
        assert_close(segment[2], 50.0, EPS, "segment[2]");
        assert_close(segment[3], 10.0, EPS, "segment[3]");
        assert_close(segment[4], 50.0, EPS, "segment[4]");
        assert_close(segment[5], 30.0, EPS, "segment[5]");
        assert_close(segment[6], 100.0, EPS, "segment[6]");
        assert_close(segment[7], 30.0, EPS, "segment[7]");
    }

    /// 对应 Java `EdgePathMathTest.java:55-64`
    /// `straightPointsRunThroughEveryWaypointInOrder`。
    #[test]
    fn straight_points_run_through_every_waypoint_in_order() {
        let reroutes = vec![pt(40.0, 10.0), pt(70.0, 30.0)];
        let points = straight_points(0.0, 0.0, 100.0, 50.0, Some(reroutes.as_slice()));
        assert_eq!(points.len(), 4); // :58
        assert_close(points[0][0], 0.0, EPS, "points[0][0]");
        assert_close(points[1][0], 40.0, EPS, "points[1][0]");
        assert_close(points[2][0], 70.0, EPS, "points[2][0]");
        assert_close(points[3][0], 100.0, EPS, "points[3][0]");
        assert_close(points[3][1], 50.0, EPS, "points[3][1]");
    }

    /// 对应 Java `EdgePathMathTest.java:67-71`
    /// `straightPointsCollapseWaypointsThatSitOnNeighbours`。
    #[test]
    fn straight_points_collapse_waypoints_that_sit_on_neighbours() {
        let reroutes = vec![pt(0.0, 0.0), pt(50.0, 0.0), pt(100.0, 0.0)];
        let points = straight_points(0.0, 0.0, 100.0, 0.0, Some(reroutes.as_slice()));
        assert_eq!(points.len(), 3, "与邻居重合的 waypoint 不得添出零长度段"); // :70
    }

    /// 对应 Java `EdgePathMathTest.java:74-83` `orthogonalPointsStayAxisAlignedThroughBends`。
    #[test]
    fn orthogonal_points_stay_axis_aligned_through_bends() {
        let points = orthogonal_points(0.0, 0.0, 100.0, 40.0, None);
        assert_eq!(flatten(&points), vec![0.0, 0.0, 50.0, 0.0, 50.0, 40.0, 100.0, 40.0]); // :76
        for i in 1..points.len() {
            let a = points[i - 1];
            let b = points[i];
            assert!(a[0] == b[0] || a[1] == b[1], "第 {i} 段必须水平或垂直"); // :80-81
        }
    }

    /// 对应 Java `EdgePathMathTest.java:86-90`
    /// `orthogonalPointsAddNoBendToAlreadyAxisAlignedLegs`。
    #[test]
    fn orthogonal_points_add_no_bend_to_already_axis_aligned_legs() {
        let reroutes = vec![pt(100.0, 0.0)];
        let points = orthogonal_points(0.0, 0.0, 100.0, 60.0, Some(reroutes.as_slice()));
        assert_eq!(flatten(&points), vec![0.0, 0.0, 100.0, 0.0, 100.0, 60.0]); // :89
    }

    /// 对应 Java `EdgePathMathTest.java:93-107` `arrowAngleFollowsTheFinalDirectionOfEachStyle`。
    #[test]
    fn arrow_angle_follows_the_final_direction_of_each_style() {
        // bezier：最后一个控制点与终点同 y，所以箭头水平进入端口。
        assert_close(
            arrow_angle(Style::Bezier, 0.0, 0.0, 100.0, 40.0, None),
            0.0,
            EPS,
            "arrow bezier",
        ); // :95
        // 落在终点之后的 reroute 会翻转最后一段的方向。
        let reroutes = vec![pt(200.0, 20.0)];
        assert_close(
            arrow_angle(Style::Bezier, 0.0, 0.0, 100.0, 40.0, Some(reroutes.as_slice())),
            std::f64::consts::PI,
            EPS,
            "arrow bezier flipped",
        ); // :97-99
        // 直线：沿最后一段。
        let reroutes = vec![pt(100.0, 0.0)];
        assert_close(
            arrow_angle(Style::Straight, 0.0, 0.0, 100.0, 40.0, Some(reroutes.as_slice())),
            std::f64::consts::FRAC_PI_2,
            EPS,
            "arrow straight",
        ); // :101-103
        // 正交：最后一段是水平进入端口的。
        assert_close(
            arrow_angle(Style::Orthogonal, 0.0, 0.0, 100.0, 40.0, None),
            0.0,
            EPS,
            "arrow orthogonal",
        ); // :105-106
    }

    /// 对应 Java `EdgePathMathTest.java:110-116`
    /// `hitReroutePicksTheNearestWaypointInsideTheRadius`。
    #[test]
    fn hit_reroute_picks_the_nearest_waypoint_inside_the_radius() {
        let reroutes = vec![pt(10.0, 10.0), pt(30.0, 10.0)];
        assert_eq!(hit_reroute(12.0, 10.0, Some(reroutes.as_slice()), 7.0), Some(0)); // :112
        assert_eq!(hit_reroute(31.0, 12.0, Some(reroutes.as_slice()), 7.0), Some(1)); // :113
        assert_eq!(hit_reroute(20.0, 10.0, Some(reroutes.as_slice()), 5.0), None); // :114（Java -1）
        assert_eq!(hit_reroute(0.0, 0.0, None, 7.0), None); // :115（Java -1）
    }
}
