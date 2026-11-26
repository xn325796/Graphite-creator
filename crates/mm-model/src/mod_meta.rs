//! `ModMeta.java` 的等价移植：`mod.json` / `mod.hjson` 的内容，镜像 `Mods.ModMeta`。
//!
//! 未建模的键保存在 [`ModMeta::extra`]（[`mm_json::Object`]，对应 Java 的
//! `LinkedHashMap<String, JsonElement>`）里，使声明了本工具不识别字段的模组仍能原样往返。

use mm_json::{Json, Object};
use mm_schema::name_rules;

/// 模组元数据。
#[derive(Debug, Clone)]
pub struct ModMeta {
    pub name: String,
    pub display_name: String,
    pub author: String,
    pub description: String,
    pub subtitle: String,
    pub version: String,
    pub min_game_version: String,
    pub dependencies: Vec<String>,
    pub soft_dependencies: Vec<String>,
    pub hidden: bool,
    pub repo: String,
    pub texturescale: f64,
    pub content_order: Vec<String>,

    /// 类模组的字段，为往返而保留；类模组本身不在范围内。
    pub main_class: String,
    pub java: bool,
    pub kotlin: bool,

    /// 未建模的原始键。
    pub extra: Object,
}

impl Default for ModMeta {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            author: String::new(),
            description: String::new(),
            subtitle: String::new(),
            version: "1.0".to_string(),
            min_game_version: "160".to_string(),
            dependencies: Vec::new(),
            soft_dependencies: Vec::new(),
            hidden: false,
            repo: String::new(),
            texturescale: 1.0,
            content_order: Vec::new(),
            main_class: String::new(),
            java: false,
            kotlin: false,
            extra: Object::new(),
        }
    }
}

impl ModMeta {
    /// 每个内容名、精灵区域与 bundle 键的前缀（`Mods.ModMeta.cleanup`）。
    pub fn internal_name(&self) -> String {
        name_rules::internal_name(&self.name)
    }

    /// 是否声明了 `legacyCompatible: true`。它是游戏脱离两个版本下限的唯一出路
    /// （Mods.java:1329-1331），与校验器一样从 [`extra`](ModMeta::extra) 读取。
    pub fn legacy_compatible(&self) -> bool {
        match self.extra.get("legacyCompatible") {
            // Java: isJsonPrimitive -> boolean 用 getAsBoolean，其余原始量用 getAsString。
            Some(Json::Bool(b)) => *b,
            Some(v @ (Json::Str(_) | Json::Num(_))) => {
                v.get_as_string().eq_ignore_ascii_case("true")
            }
            _ => false,
        }
    }

    /// 设置或清除 `legacyCompatible`；清除会移除该键，使未触碰的模组保持逐字原样。
    pub fn set_legacy_compatible(&mut self, value: bool) {
        if value {
            self.extra.insert("legacyCompatible", Json::Bool(true));
        } else {
            self.extra.remove("legacyCompatible");
        }
    }

    /// 元数据文件名，遵从游戏探测的顺序（Mods.java:34）。
    pub const META_FILES: [&'static str; 4] =
        ["mod.json", "mod.hjson", "plugin.json", "plugin.hjson"];

    /// 供脱离模型线程做只读工作的分离副本。`Json`/`Object` 为 owned，`clone` 即深拷贝。
    pub fn snapshot(&self) -> ModMeta {
        self.clone()
    }
}
