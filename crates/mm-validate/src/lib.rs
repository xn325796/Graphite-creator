//! mm-validate：`core.validate`（21 类）的移植。
//!
//! 支撑层 4 个类已就位：`Issue.java`（P2 阶段的种子，因为 mm-io 的 `ExportReport`/`ModExporter`
//! 要用它）与 P3 的 `ProjectCheck.java`、`ValidationContext.java`、`CheckSupport.java`。
//! 16 个 `*Check` 规则（Java `core.validate` 包）已全部 1:1 移植完毕：meta、minGameVersion、
//! encoding、contentOrder、board、strings、teams、bulletReach、drillTiers、crafterOutputs、
//! recipeCapacity、records、environmentSprites、sounds、planets、references。
//! `Validator.java` 本身（`defaultChecks()` 的 16 项顺序、两个 `validate` 重载、内部名闸门）也已
//! 落地为 [`validator::Validator`]，`core.validate` 的 21 个类至此移植完毕。
//!
//! 2026-11-26（产品评审 R-02）**新增第 17 条** [`script_class_check::ScriptClassCheck`]
//! （id `scriptClass`）：方块没写 `type`、脚本里也没有 `extend` 它时给一条 warning。它没有 Java
//! 对应类——Java 只认 JSON 里的 `type`，而真实模组把类写在脚本里，这是授权的新设计而不是移植
//! （规则理由见该模块头）。
//!
//! 依赖方向说明（保持 P2 的结论）：`ExportReport.java` 与 `ModExporter.java` 都 import
//! `dev.modmaker.core.validate.Issue`，而 `core.validate` 里没有任何类 import `core.io`，
//! 所以 `mm-io → mm-validate` 是单向安全依赖。
//!
//! 依赖只到 `mm-fmt`：`Issue.message()` 需要 `TextTemplate.fill`，
//! `ValidationContext::sprite_regions()` 需要 `SpriteRegions.regionsOf`，`check_support::trim()`
//! 需要 `java_double_text`；`mm-json`/`mm-model`/`mm-schema` 都在 `mm-fmt` 之下，是这三者本就
//! 需要的类型来源。刻意**不**依赖 mm-io，以免与 mm-io 成环。

pub mod check_support;
pub mod issue;
pub mod project_check;
pub mod validation_context;
pub mod meta_check;
pub mod min_game_version_check;
pub mod encoding_check;
pub mod content_order_check;
pub mod board_check;
pub mod strings_check;
pub mod team_check;
pub mod bullet_reach_check;
pub mod drill_tier_check;
pub mod crafter_output_check;
pub mod recipe_capacity_check;
pub mod record_check;
pub mod script_class_check;
pub mod environment_sprite_check;
pub mod sound_reference_check;
pub mod planet_check;
pub mod reference_check;
pub mod validator;

pub use issue::{Arg, Issue, Level};
pub use project_check::ProjectCheck;
pub use validator::{default_checks, UnknownChecksError, Validator};
