//! `PlanetOrbits.java` 的等价移植：轨道预览用的太阳系布局。
//!
//! vanilla 的轨道半径不存在任何地方——它是 `Planet` 构造链推出来的：
//! `orbitRadius = parent.totalRadius + parent.orbitSpacing + totalRadius`（太阳：radius 4、
//! orbitSpacing 2）。预览对它认识的这些天体（vanilla 星球 + 工程里的星球）套用同一个公式；集合之外
//! 的兄弟天体在游戏里会让半径略有变化，所以这个视图被标注为示意图。

use mm_json::Json;
use mm_model::{ContentRecord, ModProject};

use crate::ContentType;

/// Java `PlanetOrbits.Body`：轨道视图里的一个示意天体（Java record）。
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub name: String,
    pub radius: f32,
    pub orbit_radius: f32,
    /// Java 的 `String color` 可以为 `null`。
    pub color: Option<String>,
    pub selected: bool,
    pub vanilla: bool,
    pub star: bool,
}

/// Java `SUN_RADIUS` 等：vanilla 星球半径（Planets.java）；太阳是系统根。
pub const SUN_RADIUS: f32 = 4_f32;
/// Java `SUN_ORBIT_SPACING`。
pub const SUN_ORBIT_SPACING: f32 = 2_f32;
/// Java `SUN_COLOR`。
pub const SUN_COLOR: &str = "ffc64c";
/// Java `SUN_COLORS`。
pub const SUN_COLORS: [&str; 4] = ["ff7a38", "ff9638", "ffc64c", "ffe371"];
/// Java `VANILLA_CHILDREN`（`List.of` 的顺序）。
pub const VANILLA_CHILDREN: [&str; 3] = ["erekir", "tantros", "serpulo"];
/// Java `VANILLA_SIZES`：本类自带的一份 vanilla 尺寸表（网格尺寸，不是半径）。
pub const VANILLA_SIZES: [(&str, i32); 3] = [("serpulo", 3), ("erekir", 2), ("tantros", 2)];

/// Java `systemOf(ModProject, ContentRecord, String)`：目标所属系统的天体，根恒星在前。
///
/// 覆盖四种情况：目标是 vanilla 星球；目标环绕 vanilla 星球；目标环绕 mod 星球；目标自己就是系统根
/// （没有 parent）。月亮（parent 是目标的 mod 星球）也会被包含进来。
///
/// @param planet 目标星球记录；当目标是 `vanilla_name` 所指的那个 vanilla 星球时为 `None`
///               （Java 的两个入参都可以为 `null`，故两者都用 `Option`）。
pub fn system_of(
    project: &ModProject,
    planet: Option<&ContentRecord>,
    vanilla_name: Option<&str>,
) -> Vec<Body> {
    let mut bodies: Vec<Body> = Vec::new();
    let bare: String = match planet {
        Some(record) => record.name.clone(),
        None => vanilla_name.unwrap_or_default().to_string(),
    };
    // vanilla 星球半径都是 1；VANILLA_SIZES 存的是网格尺寸，不是半径
    let target_radius = match planet {
        Some(record) => radius_of(record),
        None => 1_f32,
    };

    // 沿 mod 星球的 parent 链往上走：chain[0] 是最外层的已知 mod 星球。
    let mut chain: Vec<&ContentRecord> = Vec::new();
    let mut base_parent: Option<String> = match planet {
        Some(record) => bare_parent(project, record),
        None => Some("sun".to_string()),
    };
    if let Some(record) = planet {
        chain.push(record);
        let mut current: &ContentRecord = record;
        let mut guard = 0;
        loop {
            if guard >= 8 {
                break;
            }
            guard += 1;
            let parent_bare = match bare_parent(project, current) {
                Some(value) => value,
                None => {
                    // 链的底部本身就是一个系统根
                    base_parent = None;
                    break;
                }
            };
            let parent = match planet_by_name(project, &parent_bare) {
                Some(value) => value,
                None => {
                    // vanilla 的 parent（例如 sun）
                    base_parent = Some(parent_bare);
                    break;
                }
            };
            chain.insert(0, parent);
            current = parent;
        }
    }

    let sun_context = base_parent.as_deref() == Some("sun")
        || (planet.is_none() && vanilla_name == Some("sun"));
    // 目标自成一颗星：chain 只含它自己且它没有可解析的 parent。
    let standalone_root = planet.is_some() && chain.len() == 1 && base_parent.is_none();
    let star_radius = if sun_context {
        SUN_RADIUS
    } else if standalone_root {
        target_radius // the target is its own star
    } else {
        1_f32
    };

    let mut star_total = star_radius;
    bodies.push(Body {
        name: if sun_context {
            "sun".to_string()
        } else if chain.is_empty() {
            bare.clone()
        } else {
            chain[0].name.clone()
        },
        radius: star_radius,
        orbit_radius: 0_f32,
        color: Some(SUN_COLOR.to_string()),
        selected: false,
        vanilla: sun_context,
        star: true,
    });

    if sun_context {
        // 给出 vanilla 兄弟天体作为参照（serpulo/erekir/tantros 共享同一个太阳）
        for name in VANILLA_CHILDREN {
            star_total += 1_f32;
            let is_selected = bare == *name;
            bodies.push(Body {
                name: (*name).to_string(),
                radius: 1_f32,
                orbit_radius: star_total,
                color: None,
                selected: is_selected,
                vanilla: true,
                star: false,
            });
        }
        match planet {
            Some(record) => {
                // 目标是环绕太阳的 mod 星球；它的轨道环排在下一个
                let orbit = explicit_orbit(record, star_total + SUN_ORBIT_SPACING + target_radius);
                bodies.push(Body {
                    name: bare.clone(),
                    radius: target_radius,
                    orbit_radius: orbit,
                    color: Some(icon_color(record)),
                    selected: true,
                    vanilla: false,
                    star: false,
                });
                add_moons(&mut bodies, project, record, orbit, target_radius);
            }
            None => {
                if VANILLA_CHILDREN.contains(&bare.as_str()) {
                    // 目标就是这些 vanilla 兄弟天体之一：原地高亮它
                    mark_selected(&mut bodies, &bare);
                    add_vanilla_moons(&mut bodies, project, &bare, star_total);
                }
            }
        }
    } else if standalone_root {
        // 独立的根：目标就是它自己的恒星
        if let Some(record) = planet {
            mark_selected(&mut bodies, &bare);
            add_moons(&mut bodies, project, record, 0_f32, target_radius);
        }
    } else if !chain.is_empty() {
        // mod 星球链：每个天体环绕它前面那个
        let mut previous_radius = star_radius;
        let mut previous_spacing = 2_f32;
        for record in &chain {
            let radius = radius_of(record);
            // 记录里显式给出的 orbitRadius 优先：游戏的 JSON 赋值发生在构造器推出数值之后，
            // 所以 mod 可以自由挪动一个星球
            let orbit = explicit_orbit(record, previous_radius + previous_spacing + radius);
            let selected = same_record(record, planet);
            bodies.push(Body {
                name: record.name.clone(),
                radius,
                orbit_radius: orbit,
                color: Some(icon_color(record)),
                selected,
                vanilla: false,
                star: false,
            });
            if selected {
                add_moons(&mut bodies, project, record, orbit, radius);
            }
            previous_radius = radius;
            previous_spacing = 2_f32;
        }
    }

    bodies
}

/// Java `record == planet` 的引用身份比较。
///
/// Rust 里记录是值语义（工程与调用方各持一份 clone），故先比指针、再退回比 `id`。
fn same_record(a: &ContentRecord, b: Option<&ContentRecord>) -> bool {
    match b {
        Some(b) => std::ptr::eq(a, b) || (a.id.is_some() && a.id == b.id),
        None => false,
    }
}

/// Java `explicitOrbit(ContentRecord, float)`：记录自己声明的 orbitRadius，否则用推出的 fallback。
fn explicit_orbit(planet: &ContentRecord, derived: f32) -> f32 {
    if let Some(element) = planet.fields.get("orbitRadius")
        && is_primitive(element)
        && let Some(value) = as_float(element)
        && value > 0_f32
    {
        return value;
    }
    derived
}

/// Java `markSelected(List<Body>, String)`：Java 用新的 record 值替换表项，Rust 直接改字段。
fn mark_selected(bodies: &mut [Body], name: &str) {
    for body in bodies.iter_mut() {
        if body.name == name {
            body.selected = true;
        }
    }
}

/// Java `addVanillaMoons(...)`：vanilla 星球名下挂的 mod 月亮没有记录可依据；什么都不加。
fn add_vanilla_moons(
    _bodies: &mut Vec<Body>,
    _project: &ModProject,
    _planet_name: &str,
    _planet_orbit: f32,
) {
}

/// Java `addMoons(List<Body>, ModProject, ContentRecord, float, float)`。
fn add_moons(
    bodies: &mut Vec<Body>,
    project: &ModProject,
    planet: &ContentRecord,
    planet_orbit: f32,
    planet_radius: f32,
) {
    for record in &project.contents {
        if record.ctype != ContentType::Planet
            || record.is_inline()
            || same_record(record, Some(planet))
        {
            continue;
        }
        let parent = bare_parent(project, record);
        if parent.as_deref() == Some(planet.name.as_str()) {
            let radius = radius_of(record);
            let orbit = explicit_orbit(record, planet_orbit + planet_radius + 2_f32 + radius);
            bodies.push(Body {
                name: record.name.clone(),
                radius,
                orbit_radius: orbit,
                color: Some(icon_color(record)),
                selected: false,
                vanilla: false,
                star: false,
            });
        }
    }
}

/// Java `bareParent(ModProject, ContentRecord)`。
fn bare_parent(project: &ModProject, planet: &ContentRecord) -> Option<String> {
    let element = planet.fields.get("parent")?;
    if !is_primitive(element) || element.get_as_string().chars().all(char::is_whitespace) {
        return None;
    }
    let parent = element.get_as_string();
    let internal = project.internal_name();
    let prefix = format!("{internal}-");
    Some(if parent.starts_with(&prefix) {
        parent[prefix.len()..].to_string()
    } else {
        parent
    })
}

/// Java `planetByName(ModProject, String)`。
fn planet_by_name<'a>(project: &'a ModProject, bare: &str) -> Option<&'a ContentRecord> {
    project.contents.iter().find(|&record| {
        record.ctype == ContentType::Planet
            && !record.is_inline()
            && (record.name == bare || record.full_name(&project.internal_name()) == bare)
    })
}

/// Java `radiusOf(ContentRecord)`。
fn radius_of(planet: &ContentRecord) -> f32 {
    match planet.fields.get("radius") {
        Some(element) if is_primitive(element) => as_float(element).unwrap_or(1_f32),
        _ => 1_f32,
    }
}

/// Java `iconColor(ContentRecord)`。
fn icon_color(planet: &ContentRecord) -> String {
    match planet.fields.get("iconColor") {
        Some(element) if is_primitive(element) => element.get_as_string(),
        _ => ContentType::Planet.color().to_string(),
    }
}

/// Gson `JsonElement.isJsonPrimitive()`：`null` 与 `JsonNull` 都不是 primitive。
fn is_primitive(value: &Json) -> bool {
    matches!(value, Json::Bool(_) | Json::Num(_) | Json::Str(_))
}

/// Gson `JsonPrimitive.getAsFloat()`：数字原语走 `Number.floatValue()`（先解析为 double 再窄化），
/// 字符串原语走 `Float.parseFloat`（非法即抛 NumberFormatException，由调用方吞掉）。布尔原语在
/// Java 里抛的是 UnsupportedOperationException，这里同样按“读不到”处理。
fn as_float(value: &Json) -> Option<f32> {
    match value {
        Json::Num(text) => text.parse::<f64>().ok().map(|v| v as f32),
        Json::Str(text) => text.parse::<f32>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! 数值向量移植自 Java `PlanetOrbitsTest`：sun 上下文 + vanilla 兄弟、sector 解析到
    //! vanilla 星球、mod 月亮、独立根恒星。

    use super::*;
    use crate::ContentType;
    use mm_json::Json;

    fn planet(project: &mut ModProject, name: &str, parent: Option<&str>, sector_size: Option<i32>) -> ContentRecord {
        let mut record = ContentRecord::create(&format!("id-{name}"), ContentType::Planet, name);
        if let Some(parent) = parent {
            record.fields.insert("parent", Json::str(parent));
        }
        if let Some(size) = sector_size {
            record.fields.insert("sectorSize", Json::num(size.to_string()));
        }
        project.contents.push(record.clone());
        record
    }

    fn by_name<'a>(bodies: &'a [Body], name: &str) -> &'a Body {
        bodies.iter().find(|body| body.name == name).unwrap_or_else(|| panic!("no body {name}"))
    }

    #[test]
    fn mod_planet_orbiting_the_sun_includes_vanilla_context() {
        let mut project = ModProject::default();
        project.meta.name = "My Mod".to_string();
        let target = planet(&mut project, "my-world", Some("sun"), Some(2));

        let bodies = system_of(&project, Some(&target), None);
        assert_eq!(5, bodies.len(), "{bodies:?}");
        assert!(bodies[0].star);
        assert_eq!("sun", bodies[0].name);
        // vanilla 环：4+1、4+2、4+3（子天体半径逐环累加）
        assert!((bodies[1].orbit_radius - 5.0).abs() < 1e-5);
        assert!((bodies[2].orbit_radius - 6.0).abs() < 1e-5);
        assert!((bodies[3].orbit_radius - 7.0).abs() < 1e-5);
        let itself = &bodies[4];
        assert!(itself.selected);
        assert_eq!("my-world", itself.name);
        assert!((itself.orbit_radius - (7.0 + 2.0 + 1.0)).abs() < 1e-5);
    }

    #[test]
    fn sector_target_resolves_to_its_vanilla_planet() {
        let mut project = ModProject::default();
        project.meta.name = "My Mod".to_string();
        project.contents.push(ContentRecord::create("s", ContentType::Sector, "辐射带"));

        let bodies = system_of(&project, None, Some("serpulo"));
        assert_eq!(4, bodies.len()); // sun + erekir + tantros + serpulo
        assert!(bodies[0].star);
        let serpulo = by_name(&bodies, "serpulo");
        assert!(serpulo.selected);
        assert!((serpulo.orbit_radius - 7.0).abs() < 1e-5);
    }

    #[test]
    fn moons_orbit_their_mod_planet() {
        let mut project = ModProject::default();
        project.meta.name = "My Mod".to_string();
        let home = planet(&mut project, "homeworld", Some("sun"), Some(2));
        planet(&mut project, "luna", Some("homeworld"), Some(1));

        let bodies = system_of(&project, Some(&home), None);
        assert_eq!(6, bodies.len(), "star + 3 vanilla + homeworld + luna");
        let luna = by_name(&bodies, "luna");
        // orbit = homeworld(10) + homeworld radius(1) + spacing(2) + luna radius(1)
        assert!((luna.orbit_radius - (10.0 + 1.0 + 2.0 + 1.0)).abs() < 1e-5);
    }

    #[test]
    fn standalone_root_planet_is_its_own_star() {
        let mut project = ModProject::default();
        project.meta.name = "My Mod".to_string();
        let root = planet(&mut project, "lonely", None, Some(2));
        planet(&mut project, "tiny-moon", Some("lonely"), Some(1));

        let bodies = system_of(&project, Some(&root), None);
        assert_eq!(2, bodies.len(), "{bodies:?}");
        assert!(bodies[0].star);
        assert_eq!("lonely", bodies[0].name);
        assert!((bodies[0].radius - 1.0).abs() < 1e-5);
        assert_eq!(0.0, bodies[0].orbit_radius);
        assert_eq!("tiny-moon", bodies[1].name);
    }
}
