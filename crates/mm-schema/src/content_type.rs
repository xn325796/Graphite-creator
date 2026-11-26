//! `ContentType` 的等价移植（Java `core.fmt.ContentType`）。

use std::fmt;

/// 12 种内容类型，镜像 mindustry.ctype.ContentType，仅限模组能真正声明的部分。
///
/// 只有 `file_backed` 的类型在 ContentParser 里有 JSON 解析器（block、unit、weather、item、
/// liquid、status、sector、planet、team）。其它类型写入 `content/<folder>/` 会以
/// "No parsers for content type" 失败——bullet 等非文件类型只作为宿主内容文件里的内联对象存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentType {
    Item,
    Block,
    Liquid,
    Status,
    Unit,
    Weather,
    Sector,
    Planet,
    Team,
    Bullet,
    UnitCommand,
    UnitStance,
}

/// `values()` 的声明顺序，`creatable`/`accepted_folders` 依赖它。
pub const ALL: [ContentType; 12] = [
    ContentType::Item,
    ContentType::Block,
    ContentType::Liquid,
    ContentType::Status,
    ContentType::Unit,
    ContentType::Weather,
    ContentType::Sector,
    ContentType::Planet,
    ContentType::Team,
    ContentType::Bullet,
    ContentType::UnitCommand,
    ContentType::UnitStance,
];

impl ContentType {
    /// Java 枚举名（schema.json/fields.json 的 `ctype` 字面量，大小写敏感）。
    pub fn name(&self) -> &'static str {
        match self {
            ContentType::Item => "item",
            ContentType::Block => "block",
            ContentType::Liquid => "liquid",
            ContentType::Status => "status",
            ContentType::Unit => "unit",
            ContentType::Weather => "weather",
            ContentType::Sector => "sector",
            ContentType::Planet => "planet",
            ContentType::Team => "team",
            ContentType::Bullet => "bullet",
            ContentType::UnitCommand => "unitCommand",
            ContentType::UnitStance => "unitStance",
        }
    }

    /// Java `ContentType.values()`：按声明顺序的全部 12 种类型。
    pub fn all() -> Vec<ContentType> {
        ALL.into_iter().collect()
    }

    /// `ContentType.valueOf(str)`：只接受精确的枚举名，未知返回 `None`。
    pub fn from_name(s: &str) -> Option<ContentType> {
        ALL.into_iter().find(|t| t.name() == s)
    }

    /// 内容文件省略 "type" 时使用的 Java 类；game 要求显式 `type` 时为空串（如 weather）。
    pub fn default_class(&self) -> &'static str {
        match self {
            ContentType::Item => "Item",
            ContentType::Block => "Block",
            ContentType::Liquid => "Liquid",
            ContentType::Status => "StatusEffect",
            ContentType::Unit => "UnitType",
            ContentType::Weather => "",
            ContentType::Sector => "SectorPreset",
            ContentType::Planet => "Planet",
            ContentType::Team => "TeamEntry",
            ContentType::Bullet => "BulletType",
            ContentType::UnitCommand => "UnitCommand",
            ContentType::UnitStance => "UnitStance",
        }
    }

    /// game 里 `ContentType.folderName` 的确切值。
    pub fn folder_name(&self) -> &'static str {
        match self {
            ContentType::Item => "items",
            ContentType::Block => "blocks",
            ContentType::Liquid => "liquids",
            ContentType::Status => "statuses",
            ContentType::Unit => "units",
            ContentType::Weather => "weather",
            ContentType::Sector => "sectors",
            ContentType::Planet => "planets",
            ContentType::Team => "teams",
            ContentType::Bullet => "bullets",
            ContentType::UnitCommand => "unitCommands",
            ContentType::UnitStance => "unitStances",
        }
    }

    /// UI 强调色，用于节点卡片色条。
    pub fn color(&self) -> &'static str {
        match self {
            ContentType::Item => "#FFA347",
            ContentType::Block => "#5489F2",
            ContentType::Liquid => "#40BFBF",
            ContentType::Status => "#A37AF5",
            ContentType::Unit => "#F27C54",
            ContentType::Weather => "#61BAE6",
            ContentType::Sector => "#77CF77",
            ContentType::Planet => "#8282EB",
            ContentType::Team => "#F57AB8",
            ContentType::Bullet => "#f2964e",
            ContentType::UnitCommand => "#a3c266",
            ContentType::UnitStance => "#8a93a0",
        }
    }

    /// `content/<folder>/*.json` 是否被 game 的内容解析器接受。
    pub fn file_backed(&self) -> bool {
        !matches!(
            self,
            ContentType::Bullet | ContentType::UnitCommand | ContentType::UnitStance
        )
    }

    /// 用户可以创建、且各自成文件的那些类型（按声明顺序）。
    pub fn creatable() -> Vec<ContentType> {
        ALL.into_iter().filter(|t| t.file_backed()).collect()
    }

    /// 加载器为本类型接受的目录名：历史复数形式 + 当前 folderName（LinkedHashSet 去重保序）。
    ///
    /// 历史形式 = 类型名小写，若不以 "s" 结尾则加 "s"。status/weather/unitCommand/unitStance 的
    /// 两种拼法不同。
    pub fn accepted_folders(&self) -> Vec<String> {
        let lower = self.name().to_lowercase();
        let legacy = if lower.ends_with('s') { lower.clone() } else { format!("{lower}s") };
        let mut out: Vec<String> = Vec::new();
        if !out.contains(&legacy) {
            out.push(legacy);
        }
        let folder = self.folder_name().to_string();
        if !out.contains(&folder) {
            out.push(folder);
        }
        out
    }

    /// 把扫描到的目录名解析回类型；目录未知返回 `None`（精确匹配）。
    pub fn from_folder(folder: &str) -> Option<ContentType> {
        ALL.into_iter().find(|t| t.accepted_folders().iter().any(|f| f == folder))
    }

    /// 大小写不敏感变体，用于 Windows 上手工编写的模组。
    pub fn from_folder_loose(folder: &str) -> Option<ContentType> {
        ALL.into_iter().find(|t| {
            t.accepted_folders()
                .iter()
                .any(|f| f.eq_ignore_ascii_case(folder))
        })
    }
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
