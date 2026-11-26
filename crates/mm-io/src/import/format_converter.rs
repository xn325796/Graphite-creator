//! `FormatConverter.java` 的等价移植：游戏接受的两种内容格式之间的批量转换。
//!
//! 对应 Java `dev.modmaker.core.io.FormatConverter`。
//!
//! Java 是 `public final class` + 私有构造器（纯静态方法容器）；Rust 以模块级函数表达，
//! 私有构造器无需移植。

use std::fmt;

use mm_model::ModProject;

/// `convert` 收到 json/hjson 之外的目标扩展名时的明确失败。
///
/// **2026-11-25 加固决策**：这里原先是 `panic!`（照搬 Java `IllegalArgumentException`
/// 「不受检异常直达顶层」的语义）。但 `ext` 是命令面参数，最终来自用户输入——壳线程里
/// 一次 panic 会把整个模型线程打成 poisoned，坏输入不该有这种杀伤力。改为 `Result` 显式
/// 传播（与 `cancelled.rs` 模块头「panic 不作业务控制流」的既有口径一致），错误消息保留
/// Java 原文案。壳侧 `bridge.rs` 的前置守卫保留，作为第一道防线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedFormatError {
    /// 调用方交来的非法目标扩展名。
    pub ext: String,
}

impl fmt::Display for UnsupportedFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Java: throw new IllegalArgumentException("unsupported content format: " + ext)
        write!(f, "unsupported content format: {}", self.ext)
    }
}

impl std::error::Error for UnsupportedFormatError {}

/// 游戏接受的两种内容格式之间的批量转换。转换会把已解析的字段树经
/// `mm_json::hjson::HjsonWriter`（Java 的 `HjsonWriter`）或规范化 JSON writer 重新序列化：
/// 数据本身原样保留，但被转换文件里的注释和手工排版保不住。
/// 保存才会把改名后的文件落盘（并删掉旧文件），构建仍是「拷贝已保存目录」的常规动作——
/// 导出时没有单独的转换步骤。
///
/// 扩展名与目标格式不一致的文件型（非 inline）记录数。
pub fn pending_count(project: &ModProject, ext: &str) -> i32 {
    project
        .contents
        .iter()
        .filter(|record| !record.is_inline())
        // convert() 会跳过这些记录，把它们计入只会让对话框承诺一件永远不会发生的活。
        .filter(|record| !record.parse_failed)
        // Java 的 `record.ext == null ? "json" : record.ext`：Rust 模型里 ext 是非可选
        // String、缺省即 "json"（ContentRecord::new()/create()），null 兜底已内建于模型。
        .filter(|record| ext != record.ext)
        .count() as i32
}

/// meta 文件是否是会被这次转换改名的 mod.json/mod.hjson。解析失败的 meta 不动：
/// 它按原始文本保留着，改名加重新序列化会把作者元数据换成一个空对象。
pub fn meta_pending(project: &ModProject, ext: &str) -> bool {
    if project.meta_parse_failed {
        return false;
    }
    is_mod_meta(&project.meta_file) && project.meta_file != format!("mod.{ext}")
}

/// 把所有文件型记录（以及 mod.json/mod.hjson 的 meta）翻到目标扩展名。被转换的记录置
/// dirty 并丢掉原始字节，于是下一次保存会按新格式重新序列化它们；plugin.* 的 meta 文件不动。
///
/// 目标扩展名不是 json/hjson 时返回 [`UnsupportedFormatError`]，**不改动 project**
/// （校验先于一切写入）；决策理由见该错误类型的文档。
pub fn convert(project: &mut ModProject, ext: &str) -> Result<(), UnsupportedFormatError> {
    if ext != "json" && ext != "hjson" {
        return Err(UnsupportedFormatError { ext: ext.to_string() });
    }
    for record in &mut project.contents {
        if record.is_inline() {
            continue;
        }
        let current = record.ext.as_str();
        if current == ext {
            continue;
        }
        if record.parse_failed {
            // 字段树是空的；重新序列化会把文件换成一个空对象。在有人手工修好这个文件之前,
            // 原始字节原样保留。
            continue;
        }
        record.ext = ext.to_string();
        record.dirty = true;
        record.raw = None;
    }
    if meta_pending(project, ext) {
        project.previous_meta_file = Some(project.meta_file.clone());
        project.meta_file = format!("mod.{ext}");
        project.meta_dirty = true;
    }
    Ok(())
}

fn is_mod_meta(meta_file: &str) -> bool {
    meta_file == "mod.json" || meta_file == "mod.hjson"
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_model::ContentRecord;
    use mm_schema::ContentType;

    /// 带一条文件型记录的项目（ext=json、未解析失败，正是 convert 会动的那种）。
    fn project_with_one_record() -> ModProject {
        let mut project = ModProject::default();
        project.contents.push(ContentRecord::create("r1", ContentType::Item, "copper"));
        project
    }

    /// 加固回归：非法扩展名必须得到错误而非 panic，且 project 一个字段都不许被改动。
    #[test]
    fn unsupported_extension_is_an_error_and_leaves_the_project_untouched() {
        let mut project = project_with_one_record();
        project.meta_file = "mod.json".to_string();
        let err = convert(&mut project, "yaml").expect_err("yaml 不是游戏接受的内容格式");
        assert_eq!(err.ext, "yaml");
        assert_eq!(err.to_string(), "unsupported content format: yaml");
        // 失败路径不得改动项目：扩展名、meta 改名计划、脏标记都保持原样。
        assert_eq!(project.contents[0].ext, "json");
        assert_eq!(project.meta_file, "mod.json");
        assert_eq!(project.previous_meta_file, None);
        assert!(!project.meta_dirty);
    }

    /// 合法目标格式的行为不变：记录翻到 hjson、置 dirty、丢 raw，meta 一起改名。
    #[test]
    fn supported_extension_still_converts_records_and_meta() {
        let mut project = project_with_one_record();
        project.meta_file = "mod.json".to_string();
        project.contents[0].raw = Some("{}".to_string());
        convert(&mut project, "hjson").expect("hjson 是合法目标格式");
        assert_eq!(project.contents[0].ext, "hjson");
        assert!(project.contents[0].dirty);
        assert_eq!(project.contents[0].raw, None);
        assert_eq!(project.meta_file, "mod.hjson");
        assert_eq!(project.previous_meta_file.as_deref(), Some("mod.json"));
        assert!(project.meta_dirty);
    }
}
