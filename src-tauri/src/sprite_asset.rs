//! `record_sprite` 的内核：从项目资源里找一条记录的贴图并编码成 data URL。
//!
//! **规则只有一份**：区域命名与家族匹配全部问 `mm_fmt::sprite`（`SpriteRegions.java` 的移植），
//! 这里只做「资产路径 → 磁盘文件 → base64」的搬运。与 `forest.rs` 一样不依赖 tauri，
//! 好让集成测试只链接领域 crate。
//!
//! 找不到就返回 `None`（记录不存在、没有项目根、项目里没有匹配的 png）——这是正常的
//! 编辑中途状态，不是错误。
//!
//! ## 写侧：`sprite_import`（2026-11-25）
//!
//! 导入的目标路径口径与读侧严格对齐：读侧按 `sprites/`（含 `sprites-override/`）下资产的
//! **文件名**经 `name_rules::sprite_region` 匹配区域，所以写侧落在 `sprites/<记录名>.png`
//! （扁平、小写扩展名——`Mods.java:168-169` 只打包扩展名恰好为 `png` 的文件）。
//! 写完必须同步 `project.assets` 账本：`sprite_for` 只认账本里的路径，不同步则刚导入的
//! 贴图对读侧不可见，直到下一次重新打开项目。
//!
//! ## 纳入 undo 栈（2026-11-25，UI 审计缺陷 #5，用户拍板「纳入 undo 栈」而非 .bak 备份）
//!
//! **授权的新设计**：Java modmaker 没有应用内贴图导入，本功能无 Java 对应物，不是 1:1
//! 移植的偏离。导入成功时向领域历史推一条 `SpriteImportCommand`（mm-app），记录
//! record_id、目标相对路径、旧文件完整字节（首次导入为 None）、新文件字节与账本旧条目；
//! undo/redo 经既有 `undo`/`redo` 命令自然覆盖（命令面、事件表均不新增），磁盘写失败
//! 报稳定码 io/domain、记录被删报 not-found，条目留栈可重试。设计细节与已知语义
//! （undo 恢复导入前快照、不做外部变更检测；字节内存持有的上限策略）登记在
//! `mm_app::project_controller::SpriteImportCommand` 的文档上。

use std::path::Path;

use mm_app::project_controller::ProjectController;
use mm_schema::name_rules;

use crate::dto::SpriteDto;

/// PNG 的固定 8 字节签名：读侧（`read_sprite`）与写侧（`import_sprite`）共用同一份。
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 一条记录的贴图。优先取区域名**完全相等**的资产，其次取同家族（`name1`/`name-top-2` 这类
/// 变体与部件）；两者都没有返回 `None`。
pub fn sprite_for(controller: &ProjectController, record_id: &str) -> Option<SpriteDto> {
    let project = controller.project()?;
    let record = project.content_by_id(record_id)?;
    let internal = project.internal_name();
    if internal.is_empty() {
        return None;
    }
    let expected = mm_fmt::sprite::expected_region(record, &internal);
    if expected.is_empty() {
        return None;
    }
    let root = project.root.as_deref()?;

    // (rank, path, region)：0 = 完全相等，1 = 同家族。
    let mut best: Option<(u8, &str, String)> = None;
    for asset in &project.assets {
        if !asset.kind.starts_with("sprite") {
            continue;
        }
        let override_ = asset.path.starts_with("sprites-override/");
        if !override_ && !asset.path.starts_with("sprites/") {
            continue;
        }
        let file_name = &asset.path[asset.path.rfind('/').map(|at| at + 1).unwrap_or(0)..];
        let Some(dot) = file_name.rfind('.') else {
            continue;
        };
        if !file_name[dot + 1..].eq_ignore_ascii_case("png") {
            continue;
        }
        let region = name_rules::sprite_region(&internal, &file_name[..dot], !override_);
        let rank = if region == expected {
            0
        } else if mm_fmt::sprite::matches_family(&expected, &region) {
            1
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(current, ..)| rank < *current) {
            best = Some((rank, asset.path.as_str(), region));
        }
    }
    let (_, relative, region) = best?;

    let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    // 资产路径来自包（不可信输入）：只允许落在项目根之内。
    if !path.starts_with(root) || !safe_relative(relative) {
        return None;
    }
    read_sprite(&path, region)
}

pub fn read_sprite(path: &Path, region: String) -> Option<SpriteDto> {
    let bytes = std::fs::read(path).ok()?;
    // 空文件或明显不是 PNG 的字节流不该变成一张破图：PNG 签名固定 8 字节。
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return None;
    }
    Some(SpriteDto {
        data_url: format!("data:image/png;base64,{}", base64_encode(&bytes)),
        region,
    })
}

/// 相对路径里不允许出现 `..` 段或盘符——与包读取器的安全口径一致。
pub fn safe_relative(relative: &str) -> bool {
    !relative.split('/').any(|segment| segment == "..") && !relative.contains(':')
}

/// `sprite_import` 的失败分类。壳（`bridge.rs`）按变体映射到 `ShellError` 的稳定码：
/// `NoProject` → no-project，`RecordNotFound`/`SourceMissing` → not-found，
/// `NotPng` → encoding（与「非 UTF-8 文本显式拒绝」同族的内容格式拒收），
/// `Unimportable` → domain，`Io` → io。本枚举不依赖 tauri，集成测试可直接断言变体。
#[derive(Debug)]
pub enum SpriteImportError {
    /// 没有打开的项目，或项目还没有磁盘根目录。
    NoProject,
    /// 记录 id 在项目里找不到。
    RecordNotFound(String),
    /// 记录不适合导入贴图：内联内容不是文件、名字不能做文件名、meta 缺 name。
    Unimportable(String),
    /// 用户选择的源文件不存在。
    SourceMissing(String),
    /// 源字节流不是 PNG（签名校验与读侧同一份）。
    NotPng(String),
    /// 磁盘读写失败。
    Io(std::io::Error),
}

impl std::fmt::Display for SpriteImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProject => write!(f, "还没有打开的项目（或项目尚未落盘），无法导入贴图"),
            Self::RecordNotFound(id) => write!(f, "找不到记录：{id}"),
            Self::Unimportable(reason) => write!(f, "{reason}"),
            Self::SourceMissing(path) => write!(f, "源文件不存在：{path}"),
            Self::NotPng(path) => write!(f, "不是合法的 PNG 文件：{path}"),
            Self::Io(err) => write!(f, "贴图读写失败：{err}"),
        }
    }
}

/// 把用户选择的 PNG 导入为一条记录的贴图（`sprite_import` 的内核，tauri-free）。
///
/// 步骤：解析记录的目标路径（`sprites/<记录名>.png`，与读侧匹配规则一致）→ 只读地读取
/// 源文件并校验 PNG 签名 → 经 mm-io 的原子写设施字节保真地复制进项目 → 同步
/// `project.assets` 账本并作废校验缓存 → 返回与 `record_sprite` 同形的 DTO。
///
/// 安全口径：源路径只读不写；目标路径经 [`mm_io::project_io::safe_resolve`] 归一化后
/// 必须仍落在项目根内（比读侧的词法 `starts_with` 多拦一层 `..` 归一化逃逸）；
/// 记录名必须是单个安全路径段（分隔符/盘符/`..` 一律拒绝），名字本身不可能把目标
/// 带出 `sprites/`。
pub fn import_sprite(
    controller: &mut ProjectController,
    record_id: &str,
    source: &Path,
) -> Result<SpriteDto, SpriteImportError> {
    // 1. 解析记录与目标路径。区域名走 mm_fmt::sprite 的既有规则，不在这里另立口径。
    let (relative, expected, root) = {
        let project = controller.project().ok_or(SpriteImportError::NoProject)?;
        let record = project
            .content_by_id(record_id)
            .ok_or_else(|| SpriteImportError::RecordNotFound(record_id.to_string()))?;
        // 内联内容不是文件，没有自己的贴图（与 mm_fmt::sprite::needs_sprite 同一判定半边）。
        // 类型不设限：读侧 sprite_for 对任何记录都按名字匹配，物品同样需要图标。
        if record.is_inline() {
            return Err(SpriteImportError::Unimportable(format!(
                "记录 {record_id} 是内联内容，没有独立的贴图文件"
            )));
        }
        let internal = project.internal_name();
        if internal.is_empty() {
            return Err(SpriteImportError::Unimportable(
                "项目元数据缺少 name，无法解析贴图区域".to_string(),
            ));
        }
        let root = project.root.clone().ok_or(SpriteImportError::NoProject)?;
        // 名字必须是单个安全路径段：读侧按文件名匹配区域，写侧不能让名字带分隔符
        // 或盘符（Windows 下 `\` 也是分隔符，safe_relative 只认 `/`，这里显式补齐）。
        if record.name.is_empty()
            || record.name.contains(['/', '\\', ':'])
            || !safe_relative(&record.name)
        {
            return Err(SpriteImportError::Unimportable(format!(
                "记录名不能用作贴图文件名：{}",
                record.name
            )));
        }
        let expected = mm_fmt::sprite::expected_region(record, &internal);
        (format!("sprites/{}.png", record.name), expected, root)
    };

    // 2. 源文件只读：整段读进内存（字节保真），再做与读侧同一份的 PNG 签名校验。
    let bytes = std::fs::read(source).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            SpriteImportError::SourceMissing(source.display().to_string())
        } else {
            SpriteImportError::Io(err)
        }
    })?;
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(SpriteImportError::NotPng(source.display().to_string()));
    }

    // 3. 目标必须落在项目根内：safe_resolve 是包读取器对不可信路径的既有防护。
    let target = mm_io::project_io::safe_resolve(&root, &relative).ok_or_else(|| {
        SpriteImportError::Unimportable(format!("贴图目标路径越出项目根：{relative}"))
    })?;

    // 3.5 导入前快照（2026-11-25，贴图导入纳入 undo 栈）：旧文件完整字节（不存在则 None）
    //     与账本旧条目，供 SpriteImportCommand 的 undo 恢复「导入前」状态。
    //     读取失败（非 NotFound）直接中止导入——快照不完整就不该留下不可撤销的写入。
    let old_bytes = match std::fs::read(&target) {
        Ok(existing) => Some(existing),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(SpriteImportError::Io(err)),
    };

    // 4. 原子复制：先写同目录临时文件再换入（mm-io 的 AtomicFiles 口径），
    //    失败清掉临时文件——一次失败的导入不该留下半截贴图或 .mm-tmp 噪音。
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(SpriteImportError::Io)?;
    }
    let temp = mm_io::files::temp_for(&target);
    let landed = std::fs::write(&temp, &bytes)
        .and_then(|()| mm_io::files::move_into_place(&temp, &target));
    if let Err(err) = landed {
        mm_io::files::delete_quietly(&temp);
        return Err(SpriteImportError::Io(err));
    }

    // 5. 同步资产账本：sprite_for 只认 project.assets 里的路径，不同步则刚写入的
    //    贴图对读侧不可见。已存在同路径资产（重复导入/覆盖）只更新字节数。
    let size = bytes.len() as i64;
    let asset_before;
    {
        let project = controller.project_mut().ok_or(SpriteImportError::NoProject)?;
        asset_before = project.assets.iter().find(|asset| asset.path == relative).cloned();
        match project.assets.iter_mut().find(|asset| asset.path == relative) {
            Some(asset) => asset.size = size,
            None => project.assets.push(mm_model::AssetRef::new(&relative, size, "sprite")),
        }
    }
    // 「缺贴图」一族结论依赖资产清单：作废校验缓存（广播那一半由壳负责，
    // 与其余改模型命令的 after_mutation 口径一致）。
    controller.note_assets_changed();

    // 5.5 推一条历史条目（2026-11-25，授权的新设计：Java 无应用内贴图导入，无对应物）：
    //     贴图变更与字段编辑按时间同栈排序，undo/redo 逐条回退/重放。字节整段持有在
    //     内存里（小 PNG，历史上界 200 条封住最坏占用），语义与上限策略登记在
    //     mm_app::project_controller::SpriteImportCommand 的文档上。
    controller.push_sprite_import(mm_app::project_controller::SpriteImportCommand::new(
        record_id,
        &relative,
        old_bytes,
        bytes,
        asset_before,
    ));

    // 6. 返回与 record_sprite 同形的 DTO，前端拿到即可刷新缩略图。
    read_sprite(&target, expected).ok_or_else(|| {
        SpriteImportError::Io(std::io::Error::other(format!(
            "贴图写入后无法读回：{}",
            target.display()
        )))
    })
}

/// 标准 base64（`+`/`/`、带 `=` 填充）。零新增依赖：只为一张内联图片写三十行。
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
