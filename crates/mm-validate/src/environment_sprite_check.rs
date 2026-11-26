//! `EnvironmentSpriteCheck.java`（81 行）的等价移植：环境方块（地板、矿石、墙、装饰）也要贴图，
//! 以及贴图是否放在地图编辑器读的那个目录里。
//!
//! 类注释（`EnvironmentSpriteCheck.java:12-18`）直译：环境方块绘制地形，所以和任何方块一样需要
//! 一张贴图——变体与部件的约定见 `SpriteRegions.matchesFamily`，那是游戏自己用的。目录影响的是
//! 地图编辑器调色板而不是渲染（`Mods.java` 按路径把贴图归到环境页），第二条 warning 说的就是它。
//!
//! 与 Java 的语言差异（只有下面这几处，均无可观察行为变化）：
//! 1. `Set<String> regions = context.spriteRegions()`（`:30`）⇒ 一次 `to_vec()`：Rust 里
//!    [`ValidationContext::sprite_regions`] 返回挂在 `&self` 上的切片，而 `:45` 起要 `&mut context`
//!    上报，两者不能并存。本方法对它**只读**（`:31` 的 `isEmpty()`、`:45` 的 `hasSprite`），故克隆
//!    没有可观察差异；克隆点仍在 `check()` 开头，惰性计算的触发时机与 Java 同一行。
//! 2. `String internal = context.internalName()`（`:29`）返回 Java 的 `String`，这里取 `String`
//!    值；Java 的 `internal` 在此处已保证非空（`requiresInternalName()` 默认 true，
//!    `Validator.java:102-110` 会整条跳过空内部名的工程）。
//! 3. `String className = ...resolveClass(...)`（`:40`）的可空性 ↔ [`Option<String>`]，
//!    `className == null ||` 短路后 `Some(...)` 交棒给
//!    [`check_support::is_environment_class`]（它的入参本来就是 `Option<&str>`）。
//! 4. `String.substring(int)` 的 UTF-16 计数与 Rust 字节切片：本文件只剥掉 `.png`（4 个 ASCII
//!    字节）与在 `'/'` 之后取值，切点必落在字符边界上，两者逐字符相同。

use mm_fmt::sprite;
use mm_model::ModProject;
use mm_schema::{name_rules, ContentType};

use crate::check_support;
use crate::issue::{Issue, Level};
use crate::project_check::ProjectCheck;
use crate::validation_context::ValidationContext;

/// Java `class EnvironmentSpriteCheck implements ProjectCheck`（`EnvironmentSpriteCheck.java:19`）。
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvironmentSpriteCheck;

impl ProjectCheck for EnvironmentSpriteCheck {
    /// Java `id()`（`EnvironmentSpriteCheck.java:21-24`）。
    fn id(&self) -> &str {
        "environmentSprites"
    }

    /// Java `check(ValidationContext)`（`EnvironmentSpriteCheck.java:26-53`）。
    /// Java 未覆写 `requiresInternalName()`，沿用 `ProjectCheck.java:22-24` 的默认 `true`。
    fn check(&self, context: &mut ValidationContext<'_>) {
        // :28-30  ModProject project / String internal / Set<String> regions
        let project: &ModProject = context.project();
        let internal: String = context.internal_name();
        let regions: Vec<String> = context.sprite_regions().to_vec();

        // :31-35  一张贴图资源都没有：工程还没用贴图，逐块唠叨地形贴图是噪音。
        //         与方块贴图检查同一立场（`SpriteMissingCheck` 的前置 return）。
        if regions.is_empty() {
            return;
        }

        // :36  for (ContentRecord record : project.contents)
        for record in &project.contents {
            // :37-39  只看非补丁、非内联的方块
            if record.ctype != ContentType::Block || record.patch || record.is_inline() {
                continue;
            }

            // :40-43  解析类名为 null 或不是环境类 ⇒ 与本规则无关
            let class_name: Option<String> =
                context.registry().resolve_class(record.ctype, record.class_name().as_deref());
            let Some(class_name) = class_name else { continue };
            if !check_support::is_environment_class(context.registry(), Some(class_name.as_str())) {
                continue;
            }

            // :44  String region = SpriteRegions.expectedRegion(record, internal)
            let region: String = sprite::expected_region(record, &internal);

            // :45-47  完全没有同族贴图 ⇒ warning（三个参数都记 record.name，照 Java 原样）
            if !sprite::has_sprite(&regions, &region) {
                context.report(Issue::new(
                    Level::Warning,
                    "validate.environment.noSprite",
                    &[record.name.as_str(), record.name.as_str(), record.name.as_str()],
                    record.id.clone(),
                ));
            // :48-51  有贴图但没放在环境目录 ⇒ warning
            } else if !files_environment_sprite(project, &internal, &region) {
                context.report(Issue::new(
                    Level::Warning,
                    "validate.environment.wrongFolder",
                    &[record.name.as_str()],
                    record.id.clone(),
                ));
            }
        }
    }
}

/// Java `private static boolean filesEnvironmentSprite(ModProject, String, String)`
/// （`EnvironmentSpriteCheck.java:55-80`）。
///
/// 方法注释（`:55-60`）直译：这条记录的贴图里是否有任何一张被放在地图编辑器读取的那个目录下。
/// 游戏只用两个 substring 测试来判定，不带结尾斜杠，且两棵贴图树都算（`Mods.java:445-452`，
/// 打包在 `Mods.java:168-174`），所以 `sprites/blocks/environmental/` 与 sprites-override 的副本
/// 同样算数。
fn files_environment_sprite(project: &ModProject, internal: &str, region: &str) -> bool {
    // :62  for (AssetRef asset : project.assets)
    for asset in &project.assets {
        // :63-66  只认 sprites/ 与 sprites-override/ 两棵树
        let override_: bool = asset.path.starts_with("sprites-override/");
        if !override_ && !asset.path.starts_with("sprites/") {
            continue;
        }

        // :67  asset.path.lastIndexOf('/') + 1：没有斜杠时 Java 得 0，即整条路径
        let file_name: &str = &asset.path[asset.path.rfind('/').map(|i| i + 1).unwrap_or(0)..];

        // :68-70  非 png 跳过
        if !file_name.ends_with(".png") {
            continue;
        }

        // :71-72  fileName.substring(0, fileName.length() - 4) ⇒ 剥掉 ".png"
        let candidate: String = name_rules::sprite_region(
            internal,
            &file_name[..file_name.len() - 4],
            !override_,
        );

        // :73-77  同族 + 落在环境目录里才算
        if sprite::matches_family(region, &candidate)
            && (asset.path.contains("sprites/blocks/environment")
                || asset.path.contains("sprites-override/blocks/environment"))
        {
            return true;
        }
    }

    // :79
    false
}
