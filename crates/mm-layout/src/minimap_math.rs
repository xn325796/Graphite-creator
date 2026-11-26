//! `ui/MinimapMath.java`（81 行）的等价移植：小地图投影与 zoom LOD 切换的纯计算。
//!
//! 与 [`crate::edge_path_math`] 同理由落在零依赖的 `mm-layout`：Java 类注释(:4-11)写明这些
//! 公式**不含 JavaFX 状态**，且两张画布共用同一个视图模型 `screen = world * zoom + pan`，
//! 所以科技树画布与内容画布用同一组公式。
//!
//! Java 是 `package-private final class` + `record Projection`；Rust 侧全部放开为 `pub`，
//! 理由与 [`crate::edge_path_math`] 顶部登记的同一条（`pub(crate)` 会因只有测试使用而报
//! `dead_code`，而 M1 门要求 0 警告、`allow(` 又是禁词；M2/M3 也要跨 crate 调它们）。

/// 对应 Java `MinimapMath.Projection`(:17-31)：把世界坐标映射进小地图方框的 scale + offset。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Projection {
    /// Java `record` 的规范构造器。
    pub fn new(scale: f64, offset_x: f64, offset_y: f64) -> Self {
        Self { scale, offset_x, offset_y }
    }

    /// 对应 Java `Projection.x(double)`(:19-21)。
    pub fn x(self, world_x: f64) -> f64 {
        self.offset_x + world_x * self.scale
    }

    /// 对应 Java `Projection.y(double)`(:23-25)。
    pub fn y(self, world_y: f64) -> f64 {
        self.offset_y + world_y * self.scale
    }

    /// 对应 Java `Projection.size(double)`(:27-29)。
    pub fn size(self, world_size: f64) -> f64 {
        world_size * self.scale
    }
}

/// 对应 Java `fit(...)`(:33-51)：把世界内容框等比放进小地图方框（四周留 margin），内容居中，
/// 两轴同一个 scale。内容退化（空画布）时回落 scale = 1，让视口矩形仍落在方框内而不除零。
pub fn fit(
    content_min_x: f64,
    content_min_y: f64,
    content_max_x: f64,
    content_max_y: f64,
    map_width: f64,
    map_height: f64,
    margin: f64,
) -> Projection {
    let content_width = (content_max_x - content_min_x).max(0.0);
    let content_height = (content_max_y - content_min_y).max(0.0);
    let box_width = (map_width - 2.0 * margin).max(1.0);
    let box_height = (map_height - 2.0 * margin).max(1.0);
    let scale = if content_width <= 0.0 || content_height <= 0.0 {
        1.0
    } else {
        (box_width / content_width).min(box_height / content_height)
    };
    let offset_x = margin + (box_width - content_width * scale) / 2.0 - content_min_x * scale;
    let offset_y = margin + (box_height - content_height * scale) / 2.0 - content_min_y * scale;
    Projection::new(scale, offset_x, offset_y)
}

/// 对应 Java `viewportWorldRect(...)`(:53-60)：`viewWidth × viewHeight` 的视口当前看到的**世界**
/// 矩形，返回 `[minX, minY, maxX, maxY]`。zoom 为 0 时按 1 处理（Java :54）。
pub fn viewport_world_rect(
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
    view_width: f64,
    view_height: f64,
) -> [f64; 4] {
    let scale = if zoom == 0.0 { 1.0 } else { zoom };
    [
        -pan_x / scale,
        -pan_y / scale,
        (view_width - pan_x) / scale,
        (view_height - pan_y) / scale,
    ]
}

/// 对应 Java `viewportMapRect(...)`(:62-73)：视口矩形投影到小地图坐标，并夹进方框内，
/// 返回 `[left, top, width, height]`。
// 8 个参数与 Java `viewportMapRect` 的签名逐一对应，不打包成结构体。
#[allow(clippy::too_many_arguments)]
pub fn viewport_map_rect(
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
    view_width: f64,
    view_height: f64,
    projection: Projection,
    map_width: f64,
    map_height: f64,
) -> [f64; 4] {
    let world = viewport_world_rect(pan_x, pan_y, zoom, view_width, view_height);
    let left = projection.x(world[0]).clamp(0.0, map_width);
    let top = projection.y(world[1]).clamp(0.0, map_height);
    let right = projection.x(world[2]).clamp(0.0, map_width);
    let bottom = projection.y(world[3]).clamp(0.0, map_height);
    [left, top, right - left, bottom - top]
}

/// 对应 Java `worldXAtMap(...)`(:75-78)：[`Projection::x`] 的逆——小地图点回到世界坐标，
/// 先夹进方框。
pub fn world_x_at_map(map_x: f64, map_width: f64, projection: Projection) -> f64 {
    (map_x.clamp(0.0, map_width) - projection.offset_x) / projection.scale
}

/// 对应 Java `worldYAtMap(...)`(:80-82)。
pub fn world_y_at_map(map_y: f64, map_height: f64, projection: Projection) -> f64 {
    (map_y.clamp(0.0, map_height) - projection.offset_y) / projection.scale
}

/// 对应 Java `panToCenterX(...)`(:84-87)：把某个世界点保持在视口中心所需的 pan——小地图跳转的目标。
pub fn pan_to_center_x(world_x: f64, zoom: f64, view_width: f64) -> f64 {
    view_width / 2.0 - world_x * zoom
}

/// 对应 Java `panToCenterY(...)`(:89-91)。
pub fn pan_to_center_y(world_y: f64, zoom: f64, view_height: f64) -> f64 {
    view_height / 2.0 - world_y * zoom
}

/// 对应 Java `lodActive(...)`(:93-95)：LOD **严格小于**阈值才生效；等于或大于阈值时画布保持全细节。
pub fn lod_active(zoom: f64, lod_threshold: f64) -> bool {
    zoom < lod_threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 对应 Java `MinimapMathTest.EPS`(:16，`private static final double EPS = 1e-9`)。
    const EPS: f64 = 1e-9;

    /// 对应 Java `assertEquals(double expected, double actual, double delta[, String])`。
    fn assert_close(actual: f64, expected: f64, delta: f64, what: &str) {
        assert!((actual - expected).abs() <= delta, "{what}: 实际 {actual}，期望 {expected}");
    }

    /// 对应 Java `MinimapMathTest.java:18-29`
    /// `fitIsAspectPreservingCentredAndInsideTheMargin`。
    #[test]
    fn fit_is_aspect_preserving_centred_and_inside_the_margin() {
        let projection = fit(-100.0, -50.0, 300.0, 150.0, 160.0, 110.0, 8.0);
        // 400x200 的内容进 144x94 的方框：宽度是约束轴，scale 0.36。
        assert_close(projection.scale, 0.36, 1e-12, "p.scale()"); // :22
        assert_close(projection.x(-100.0), 8.0, EPS, "content left edge sits on the margin"); // :23
        assert_close(projection.x(300.0), 152.0, EPS, "content right edge sits on the margin"); // :24
        assert_close(projection.y(-50.0), 19.0, EPS, "the shorter axis is centred, not stretched"); // :25
        assert_close(projection.y(150.0), 91.0, EPS, "p.y(150)"); // :26
        assert_close(projection.x(100.0), 80.0, EPS, "the content centre lands on the map centre"); // :27
        assert_close(projection.y(50.0), 55.0, EPS, "p.y(50)"); // :28
    }

    /// 对应 Java `MinimapMathTest.java:31-39`
    /// `fitOfDegenerateContentFallsBackToUnitScale`。
    #[test]
    fn fit_of_degenerate_content_falls_back_to_unit_scale() {
        let projection = fit(5.0, 5.0, 5.0, 5.0, 160.0, 110.0, 8.0);
        assert_close(projection.scale, 1.0, EPS, "an empty canvas must not divide by a zero content box"); // :34
        let rect = viewport_map_rect(0.0, 0.0, 1.0, 1400.0, 900.0, projection, 160.0, 110.0);
        // Java :36-38 只断 `assertFalse(Double.isNaN(value))`，原样照搬，不加码。
        for value in rect {
            assert!(!value.is_nan(), "the viewport rect stays finite: {}", value);
        }
    }

    /// 对应 Java `MinimapMathTest.java:41-49` `viewportWorldRectInvertsTheViewTransform`。
    /// Java :43 的注释：`screen = world * zoom + pan`，pan(10,20) 在 zoom 2 下看到世界 -5..45 x -10..15。
    #[test]
    fn viewport_world_rect_inverts_the_view_transform() {
        let rect = viewport_world_rect(10.0, 20.0, 2.0, 100.0, 50.0);
        assert_close(rect[0], -5.0, EPS, "rect[0]"); // :45
        assert_close(rect[1], -10.0, EPS, "rect[1]"); // :46
        assert_close(rect[2], 45.0, EPS, "rect[2]"); // :47
        assert_close(rect[3], 15.0, EPS, "rect[3]"); // :48
    }

    /// 对应 Java `MinimapMathTest.java:51-56` `viewportWorldRectGuardsAgainstZeroZoom`。
    #[test]
    fn viewport_world_rect_guards_against_zero_zoom() {
        let rect = viewport_world_rect(0.0, 0.0, 0.0, 100.0, 50.0);
        assert_close(rect[0], 0.0, EPS, "rect[0]"); // :54
        assert_close(rect[2], 100.0, EPS, "a zero zoom reads as 1:1 instead of infinity"); // :55
    }

    /// 对应 Java `MinimapMathTest.java:58-72` `viewportMapRectClampsIntoTheMapBox`。
    #[test]
    fn viewport_map_rect_clamps_into_the_map_box() {
        let identity = Projection::new(1.0, 0.0, 0.0);
        let inside = viewport_map_rect(0.0, 0.0, 1.0, 50.0, 50.0, identity, 160.0, 110.0);
        assert_close(inside[0], 0.0, EPS, "inside[0]"); // :62
        assert_close(inside[2], 50.0, EPS, "inside[2]"); // :63
        assert_close(inside[3], 50.0, EPS, "inside[3]"); // :64
        // Java :66 的注释：比地图又宽又高的窗口，每条边都夹住、绝不越界。
        let clamped = viewport_map_rect(-100.0, -100.0, 1.0, 400.0, 400.0, identity, 160.0, 110.0);
        assert_close(clamped[0], 100.0, EPS, "clamped[0]"); // :68
        assert_close(clamped[1], 100.0, EPS, "clamped[1]"); // :69
        assert_close(clamped[2], 60.0, EPS, "right edge clamps at the map width"); // :70
        assert_close(clamped[3], 10.0, EPS, "bottom edge clamps at the map height"); // :71
    }

    /// 对应 Java `MinimapMathTest.java:74-82`
    /// `mapPointInvertsTheProjectionAndClampsToTheBox`。
    #[test]
    fn map_point_inverts_the_projection_and_clamps_to_the_box() {
        let projection = Projection::new(0.5, 10.0, 0.0);
        assert_close(world_x_at_map(110.0, 160.0, projection), 200.0, EPS, "worldXAtMap(110)"); // :77
        assert_close(world_x_at_map(-5.0, 160.0, projection), -20.0, EPS, "left of the map clamps to x=0"); // :78
        assert_close(world_x_at_map(200.0, 160.0, projection), 300.0, EPS, "right of the map clamps to the width"); // :79
        let other = Projection::new(0.5, 0.0, 10.0);
        assert_close(world_y_at_map(55.0, 110.0, other), 90.0, EPS, "worldYAtMap(55)"); // :81
    }

    /// 对应 Java `MinimapMathTest.java:84-95` `panToCenterRoundTripsThroughTheViewportRect`。
    /// Java 侧不断 `panX`/`panY` 本身，只断它们把世界点送回视口中心，delta 是 1e-6。
    #[test]
    fn pan_to_center_round_trips_through_the_viewport_rect() {
        let world_x = 321.0;
        let world_y = -77.0;
        let zoom = 0.7;
        let pan_x = pan_to_center_x(world_x, zoom, 1400.0);
        let pan_y = pan_to_center_y(world_y, zoom, 900.0);
        let rect = viewport_world_rect(pan_x, pan_y, zoom, 1400.0, 900.0);
        assert_close((rect[0] + rect[2]) / 2.0, world_x, 1e-6,
            "the jumped-on world point must end up at the viewport centre"); // :92-93
        assert_close((rect[1] + rect[3]) / 2.0, world_y, 1e-6, "rect 中心 y"); // :94
    }

    /// 对应 Java `MinimapMathTest.java:97-104` `lodEngagesStrictlyBelowTheThreshold`。
    #[test]
    fn lod_engages_strictly_below_the_threshold() {
        assert!(lod_active(0.39, 0.4), "tech tree: under 0.4 the labels go"); // :99
        assert!(!lod_active(0.4, 0.4), "exactly at the threshold keeps full detail"); // :100
        assert!(!lod_active(0.41, 0.4)); // :101
        assert!(lod_active(0.24, 0.25), "content canvas: under 0.25 the titles go"); // :102
        assert!(!lod_active(0.25, 0.25)); // :103
    }
}
